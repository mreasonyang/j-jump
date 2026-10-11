"""Imported definitions through the installed CLI, local HTTP and real shell IPC."""
import contextlib
import http.server
import json
import subprocess
import threading
import unittest
from pathlib import Path

import test_0032_ux as ux
import test_0036_first_use as first
import test_tev1 as local
from test_0037_setup_keys import FIXTURE

ROOT = Path(__file__).resolve().parents[2]


def ref(path):
    return {'op': 'ref', 'path': path}


def obj(**fields):
    return {'op': 'object', 'fields': fields}


class ProviderDSL(unittest.TestCase):
    setUp = first.FirstUse.setUp
    profile = first.FirstUse.profile
    cli = local.Tev1.cli
    shell = local.Tev1.shell
    process = ux.UXTasks.process

    @contextlib.contextmanager
    def server(self, outcome='ok'):
        requests = []
        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_): pass
            def reply(self, value):
                body = json.dumps(value).encode()
                self.send_response(200)
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            def do_GET(self):
                requests.append((self.path, dict(self.headers), None))
                self.reply({'version': '1.0'})
            def do_POST(self):
                value = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                requests.append((self.path, dict(self.headers), value))
                if outcome == 'redirect':
                    self.send_response(302)
                    self.send_header('Location', '/unexpected')
                    self.end_headers()
                    return
                criteria = value['payload']['questions']['destination']['criteria']
                # Other recorded directories (such as the cwd) may also be offered.
                choice = next((k for k, v in criteria.items() if isinstance(v, dict) and v.get('name') == 'api-project'),
                              next(k for k in criteria if k != 'none'))
                if outcome == 'none': choice = 'none'
                if outcome == 'unknown': choice = 'forged'
                if outcome == 'refusal':
                    self.reply({'result': {'model': value['model'], 'answers': {'destination': {'type': 'refusal'}}}})
                    return
                self.reply({'result': {'model': 'wrong' if outcome == 'model' else value['model'],
                           'answers': {'destination': {'type': 'choice', 'choice': choice,
                           'confidence': .9, 'probabilities': {k: float(k == choice) for k in criteria}}}}})
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try: yield f'http://127.0.0.1:{server.server_port}/custom', requests
        finally: server.shutdown(); server.server_close(); thread.join()

    def definition(self, url):
        task = {'state': 'Choose ready', 'questions': {'destination': {
            'type': 'choice', 'instructions': 'Choose ready', 'criteria': {'ready': 'Ready', 'none': 'No match'}}}}
        return {'version': 1, 'id': 'synthetic-local', 'model': 'gpt-6-luna', 'url': url,
                'transport': 'loopback', 'auth': {'type': 'none'},
                'wire': {'request': obj(model=ref('/model'), payload=ref('/task')),
                         'response': ref('/response/result'), 'questions': ref('/request/payload/questions')},
                'probe': {'steps': [{'path': '/version', 'checks': [{'op': 'eq', 'left': ref('/response/version'),
                        'right': {'op': 'literal', 'value': '1.0'}}]},
                        {'path': '/custom', 'task': task, 'expect_choice': 'ready'}],
                        'report': {'op': 'literal', 'value': {'synthetic_request': 'passed', 'directory_quality': 'not measured'}}}}

    def import_definition(self, d, code=0):
        path = self.root / 'provider.json'
        path.write_text(json.dumps(d))
        return self.cli('config', 'import-provider', path, code=code)

    def configure(self, url):
        self.env.pop('J_JUMP_OFFLINE', None)
        self.import_definition(self.definition(url))
        self.cli('config', 'set', 'semantic', 'on')
        self.cli('config', 'set', 'consent', 'always')
        self.docs = self.home / 'api-project'
        self.docs.mkdir(exist_ok=True)
        self.cli('record', '--', self.docs, cwd=self.docs)

    def test_import_is_offline_copied_and_preserves_policy(self):
        with self.server() as (url, requests):
            out = self.import_definition(self.definition(url)).stdout
            self.assertIn(b'Semantic setting and consent preserved; no request sent.', out)
            config = json.loads(self.cli('config', 'show', '--json').stdout)
            self.assertEqual(config['saved']['provider'], 'dsl')
            self.assertFalse(config['saved']['semantic'])
            self.assertEqual(config['saved']['consent'], 'ask')
            (self.root / 'provider.json').unlink()
            self.cli('record', '--', self.target, cwd=self.target)
            preview = json.loads(self.cli('preview', 'backend').stdout)
            self.assertEqual(preview['model'], 'gpt-6-luna')
            self.assertIn('payload', preview)
            self.assertNotIn(str(self.home), json.dumps(preview))
            self.assertFalse(json.loads(self.cli('credential', 'status').stdout)['environment_present'])
            self.cli('doctor', '--json')
            self.assertEqual(requests, [])
            self.cli('config', 'set', 'provider', 'jev')
            self.cli('config', 'set', 'provider', 'dsl')
            self.assertEqual(json.loads(self.cli('config', 'show', '--json').stdout)['saved']['api_definition']['id'], 'synthetic-local')

    def test_new_endpoint_resets_consent_and_identical_reimport_keeps_it(self):
        with self.server() as (url, requests):
            self.configure(url)
            moved = dict(self.definition(url), url=url.replace('/custom', '/moved'))
            out = self.import_definition(moved).stdout
            self.assertIn(b'Semantic setting preserved; consent reset to ask because the endpoint is new; no request sent.', out)
            saved = json.loads(self.config.read_text())
            self.assertEqual((saved['semantic'], saved['consent']), (True, 'ask'))
            self.cli('config', 'set', 'consent', 'always')
            out = self.import_definition(moved).stdout
            self.assertIn(b'Semantic setting and consent preserved; no request sent.', out)
            self.assertEqual(json.loads(self.config.read_text())['consent'], 'always')
            self.env['J_JUMP_LANG'] = 'zh'
            out = self.import_definition(self.definition(url)).stdout
            self.assertIn('请求许可已重置为每次询问'.encode(), out)
            self.assertEqual(json.loads(self.config.read_text())['consent'], 'ask')
            self.assertEqual(requests, [])

    def test_invalid_import_never_changes_saved_configuration(self):
        with self.server() as (url, requests):
            d = self.definition(url)
            self.import_definition(d)
            before = self.config.read_bytes()
            for key, value in [('version', 99), ('url', 'http://example.invalid/api'), ('id', '../jev'), ('unknown', True)]:
                invalid = dict(d, **{key: value})
                self.import_definition(invalid, code=2)
                self.assertEqual(self.config.read_bytes(), before)
            self.assertEqual(requests, [])

    def test_explicit_local_probe_and_proxy_bypass(self):
        with self.server() as (url, requests):
            self.configure(url)
            self.env.update(HTTP_PROXY='http://127.0.0.1:1', HTTPS_PROXY='http://127.0.0.1:1', ALL_PROXY='http://127.0.0.1:1')
            self.cli('doctor', '--json')
            self.assertEqual(requests, [])
            report = json.loads(self.cli('provider-check', '--json').stdout)
            self.assertEqual(report['requests'], 2)
            self.assertEqual(report['synthetic_request'], 'passed')
            self.assertEqual([r[0] for r in requests], ['/version', '/custom'])
            self.assertTrue(all('Authorization' not in r[1] for r in requests))
            self.assertFalse((self.state / 'config/credential.epoch').exists())
            self.cli('--offline', 'provider-check', code=2)

    def test_custom_api_selection_and_cancel_in_all_shells(self):
        for name in ('bash', 'zsh', 'fish'):
            with self.subTest(shell=name), self.server() as (url, requests):
                self.profile(name)
                self.configure(url)
                with self.shell(name) as t:
                    t.send('ji backend\r'); t.until(b'Empty/q: cancel', seconds=8); t.send('1\r'); t.drain(.3)
                    t.cmd('printf "WHERE:%s\\n" "$PWD"', 'SELECTED')
                    self.assertIn(('WHERE:' + str(self.docs)).encode(), t.output)
                    t.cmd('cd ' + local.shlex.quote(str(self.cwd)), 'RETURNED')
                    t.send('ji backend\r'); t.until(b'Empty/q: cancel', seconds=8); t.send('q\r'); t.drain(.3)
                    t.cmd('printf "STILL:%s\\n" "$PWD"', 'CANCELLED')
                    self.assertIn(('STILL:' + str(self.cwd)).encode(), t.output)
                self.assertEqual(len(requests), 2)
                self.assertTrue(all(r[0] == '/custom' for r in requests))
                self.assertNotIn(str(self.home), json.dumps(requests))

    def test_invalid_answers_refusal_and_redirect_never_offer_navigation(self):
        for outcome in ('none', 'refusal', 'unknown', 'model', 'redirect'):
            with self.subTest(outcome=outcome), self.server(outcome) as (url, requests):
                self.profile(outcome)
                self.configure(url)
                with self.shell('bash') as t:
                    t.send('ji backend\r'); t.until(b'J3:' if outcome in ('none', 'refusal') else b'J5:', seconds=8)
                    self.assertNotIn(b'Empty/q: cancel', t.output)
                    t.cmd('printf "STILL:%s\\n" "$PWD"', 'DONE')
                    self.assertIn(('STILL:' + str(self.cwd)).encode(), t.output)
                self.assertEqual(len(requests), 1)

    def test_local_tev_preset_uses_declarative_runtime_checks(self):
        for outcome in ('ok', 'old', 'missing', 'mlx'):
            with self.subTest(outcome=outcome), local.Tev1.server(self, outcome) as (url, requests):
                self.profile(outcome)
                self.env.pop('J_JUMP_OFFLINE', None)
                d = json.loads(self.cli('config', 'provider-example', 'local-tev1').stdout)
                d['url'] = url + '/v1/systemone'
                self.import_definition(d)
                result = self.cli('provider-check', '--json', code=0 if outcome == 'ok' else 5)
                self.assertEqual(len(requests), 3 if outcome == 'ok' else 1 if outcome == 'old' else 2)
                if outcome == 'ok': self.assertEqual(json.loads(result.stdout)['synthetic_request'], 'passed')

    def test_gateway_exports_and_credential_environment_are_isolated(self):
        for name, env_name in [('openrouter-jev', 'OPENROUTER_API_KEY'), ('openrouter-microsoft', 'OPENROUTER_API_KEY'), ('openrouter-openai', 'OPENROUTER_API_KEY'), ('openrouter-clef', 'OPENROUTER_API_KEY'), ('vercel-openai', 'AI_GATEWAY_API_KEY')]:
            definition = json.loads(self.cli('config', 'provider-example', name).stdout)
            self.assertEqual(definition, json.loads((ROOT / 'examples/providers' / (name + '.json')).read_text()))
            self.import_definition(definition)
            self.env['OPENAI_API_KEY'] = 'synthetic-other-provider'
            self.assertFalse(json.loads(self.cli('credential', 'status').stdout)['environment_present'])
            self.env[env_name] = 'synthetic-gateway-value'
            output = self.cli('config', 'show', '--json').stdout
            self.assertNotIn(b'synthetic-gateway-value', output)
            self.assertTrue(json.loads(self.cli('credential', 'status').stdout)['environment_present'])
            self.env[env_name] = 'invalid\nsynthetic'
            status = self.cli('doctor', '--json').stdout
            self.assertFalse(json.loads(status)['request_configuration_ready'])
            self.assertNotIn(b'invalid\nsynthetic', status)
            self.env.pop(env_name)

        # A credential-free HTTPS API must never be described as local.
        definition['auth'] = {'type': 'none'}
        definition['url'] = 'https://example.invalid/decision'
        self.import_definition(definition)
        self.env.pop('J_JUMP_OFFLINE', None)
        self.cli('config', 'set', 'semantic', 'on')
        doctor = self.cli('doctor').stdout
        self.assertIn(b'API connection: not tested here.', doctor)
        self.assertNotIn(b'Local service and model:', doctor)

    def corrupt(self, **change):
        saved = json.loads(self.config.read_text())
        saved['api_definition']['wire']['response'] = {'op': 'removed-in-this-version'}
        saved.update(change)
        self.config.write_text(json.dumps(saved))

    def test_invalid_saved_definition_never_blocks_builtin_navigation_and_is_repairable(self):
        with self.server() as (url, requests):
            self.import_definition(self.definition(url))
            self.cli('config', 'set', 'provider', 'jev')
            self.corrupt()
            self.cli('record', '--', self.target, cwd=self.target)
            self.assertEqual(self.cli('query', 'alpha').stdout.strip(), str(self.target).encode())
            self.cli('config', 'show')
            self.cli('config', 'set', 'semantic', 'on')
            self.assertIn('removed-in-this-version', self.config.read_text())
            doctor = self.cli('doctor')
            self.assertIn(b'Saved custom API definition: invalid', doctor.stdout)
            self.assertIn(b'jjump config remove-provider', doctor.stdout)
            self.assertEqual(json.loads(self.cli('doctor', '--json').stdout)['provider_definition'], 'invalid')
            refused = self.cli('config', 'set', 'provider', 'dsl', code=7)
            self.assertIn(b'saved custom API definition is invalid for this version', refused.stderr)
            imported = self.import_definition(self.definition(url))
            self.assertIn(b'Replaced the invalid saved definition.', imported.stdout)
            self.assertEqual(json.loads(self.cli('doctor', '--json').stdout)['provider_definition'], 'ok')
            self.cli('config', 'set', 'provider', 'jev')
            self.corrupt()
            removed = self.cli('config', 'remove-provider')
            self.assertIn(b'Removed the invalid saved custom API definition.', removed.stdout)
            self.assertIn(b'not accessed or changed', removed.stdout)
            saved = json.loads(self.config.read_text())
            self.assertNotIn('api_definition', saved)
            self.assertEqual((saved['provider'], saved['semantic']), ('jev', True))
            self.assertEqual(requests, [])

    def test_invalid_selected_definition_refuses_provider_use_with_remedy(self):
        with self.server() as (url, requests):
            self.configure(url)
            self.cli('record', '--', self.target, cwd=self.target)
            self.corrupt()
            self.assertEqual(self.cli('--offline', 'query', 'alpha').stdout.strip(), str(self.target).encode())
            self.assertEqual(self.cli('query', 'alpha').stdout.strip(), str(self.target).encode())
            for args in [('preview', 'backend'), ('provider-check',), ('credential', 'delete')]:
                result = self.cli(*args, code=7)
                self.assertIn(b'jjump config remove-provider', result.stderr)
            doctor = self.cli('doctor', code=7)
            self.assertIn(b'replace it: jjump config import-provider FILE, or remove it: jjump config remove-provider', doctor.stdout)
            self.assertNotIn(b'config recover', doctor.stdout)
            removed = self.cli('config', 'remove-provider')
            self.assertIn(b'provider: dsl -> jev\nsemantic: on -> off', removed.stdout)
            self.assertIn(b'OS credential entries', removed.stdout)
            self.cli('doctor')
            saved = json.loads(self.config.read_text())
            self.assertEqual((saved['provider'], saved['semantic'], 'api_definition' in saved), ('jev', False, False))
            self.assertEqual(requests, [])

    def test_missing_definition_selection_is_input_error_and_setup_reprompts(self):
        result = self.cli('config', 'set', 'provider', 'dsl', code=2)
        self.assertIn(b'import a definition first: jjump config import-provider FILE', result.stderr)
        self.assertFalse(self.config.exists())
        with self.process('setup') as t:
            t.until(b'Semantic provider'); t.send('dsl\n')
            t.until(b'import a definition first'); t.send('off\n')
            t.until(b'Local visit tracking')
            self.assertGreaterEqual(t.output.count(b'Semantic provider ['), 2)
            t.send('q\n'); t.cancelled()
        self.assertFalse(self.config.exists())

    def test_import_summary_credential_variable_and_identity_preserving_source(self):
        jev = json.loads(self.cli('config', 'provider-example', 'openrouter-jev').stdout)
        out = self.import_definition(jev).stdout
        for line in [b'Definition: openrouter-jev', b'Endpoint: ' + jev['url'].encode(), b'Transport: https',
                     b'Authentication: bearer token from $OPENROUTER_API_KEY', b'no request sent']:
            self.assertIn(line, out)
        self.assertNotIn(b'Replaced definition', out)
        self.assertEqual(json.loads(self.cli('credential', 'status').stdout)['environment_variable'], 'OPENROUTER_API_KEY')
        show = self.cli('config', 'show').stdout
        self.assertIn(b'Endpoint: ' + jev['url'].encode(), show)
        self.assertIn(b'$OPENROUTER_API_KEY', show)
        self.cli('config', 'set', 'semantic', 'on')
        self.assertIn(b'Set $OPENROUTER_API_KEY or run jjump credential set', self.cli('doctor').stdout)
        saved = json.loads(self.config.read_text())
        saved['providers']['dsl']['credential'] = 'system'
        self.config.write_text(json.dumps(saved))
        self.import_definition(jev)
        self.assertEqual(json.loads(self.config.read_text())['providers']['dsl']['credential'], 'system')
        vercel = json.loads(self.cli('config', 'provider-example', 'vercel-openai').stdout)
        vercel['auth'] = {'type': 'header', 'name': 'x-api-key', 'env': 'AI_GATEWAY_API_KEY'}
        out = self.import_definition(vercel).stdout
        self.assertIn(b'Replaced definition openrouter-jev', out)
        self.assertIn(b'The stored key for openrouter-jev was kept; delete it with '
                      b'jjump credential delete --definition openrouter-jev --apply', out)
        self.assertIn(b'Authentication: header x-api-key from $AI_GATEWAY_API_KEY', out)
        self.assertEqual(json.loads(self.config.read_text())['providers']['dsl']['credential'], 'environment')
        # Without a stored-key source there is nothing orphaned to mention.
        self.assertNotIn(b'The stored key for', self.import_definition(jev).stdout)

    def test_credential_delete_by_definition_id_regardless_of_selection(self):
        self.import_definition(json.loads(self.cli('config', 'provider-example', 'openrouter-jev').stdout))
        self.cli('config', 'set', 'provider', 'jev')
        epoch = self.config.parent / 'credential.epoch'
        for bad in ['../jev', 'Upper', '', 'x' * 65, 'a.b']:
            result = self.cli('credential', 'delete', '--definition', bad, '--apply', code=2)
            self.assertIn(b'definition id must be 1-64 lowercase letters, digits or -', result.stderr)
        preview = self.cli('credential', 'delete', '--definition', 'old-gateway').stdout
        self.assertIn(b'Custom API OS credential entry for old-gateway would be deleted; add --apply.', preview)
        self.assertFalse(epoch.exists())
        # --apply reaches a credential store, so it runs only in the memory-only fixture binary.
        self.assertTrue(FIXTURE.is_file(), 'build the setup-credentials-fixture example first')
        applied = subprocess.run([str(FIXTURE), 'credential', 'delete', '--definition', 'old-gateway', '--apply'],
                                 env=dict(self.env, PWD=str(self.cwd)), cwd=self.cwd, capture_output=True, timeout=10)
        self.assertEqual(applied.returncode, 0, applied.stderr)
        self.assertIn(b'FIXTURE: deleted j-jump.dsl.old-gateway/api-key from memory', applied.stderr)
        self.assertIn(b'Custom API OS credential entry for old-gateway deleted.', applied.stdout)
        self.assertTrue(epoch.exists())
        self.assertEqual(json.loads(self.config.read_text())['provider'], 'jev')

    def test_provider_check_is_suggested_only_with_a_probe_plan(self):
        with self.server() as (url, _):
            self.configure(url)
            self.assertIn(b'Run jjump provider-check explicitly', self.cli('doctor').stdout)
            d = self.definition(url)
            del d['probe']
            self.import_definition(d)
            doctor = self.cli('doctor').stdout
            self.assertNotIn(b'Run jjump provider-check', doctor)
            self.assertIn(b'no provider-check plan', doctor)

    def test_consent_names_the_custom_endpoint_host(self):
        with self.server() as (url, requests):
            self.configure(url)
            self.cli('config', 'set', 'consent', 'ask')
            with self.shell('bash') as t:
                t.send('ji backend\r'); t.until(b'[y/N]', seconds=8)
                self.assertIn(('Custom API (DSL) at ' + url.split('/')[2]).encode(), t.output)
                t.send('n\r')
                t.cmd('printf "STILL:%s\\n" "$PWD"', 'DONE')
                self.assertIn(('STILL:' + str(self.cwd)).encode(), t.output)
            self.assertEqual(requests, [])
