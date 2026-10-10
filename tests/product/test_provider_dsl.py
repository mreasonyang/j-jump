"""Imported definitions through the installed CLI, local HTTP and real shell IPC."""
import contextlib
import http.server
import json
import threading
import unittest
from pathlib import Path

import test_0036_first_use as first
import test_tev1 as local

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
                choice = next(k for k in criteria if k != 'none')
                if outcome == 'none': choice = 'none'
                if outcome == 'unknown': choice = 'forged'
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
            self.import_definition(self.definition(url))
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

    def test_invalid_import_never_changes_saved_configuration(self):
        with self.server() as (url, requests):
            d = self.definition(url)
            self.import_definition(d)
            before = self.config.read_bytes()
            for key, value in [('version', 99), ('url', 'http://example.invalid/api'), ('id', '../jev'), ('unknown', True)]:
                invalid = dict(d, **{key: value})
                self.import_definition(invalid, code=5)
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
        for outcome in ('none', 'unknown', 'model', 'redirect'):
            with self.subTest(outcome=outcome), self.server(outcome) as (url, requests):
                self.profile(outcome)
                self.configure(url)
                with self.shell('bash') as t:
                    t.send('ji backend\r'); t.until(b'J3:' if outcome == 'none' else b'J5:', seconds=8)
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
