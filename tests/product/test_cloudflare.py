"""Provider selection and real PTY setup; synthetic keys, no live cloud calls."""
import contextlib
import json
from pathlib import Path
import subprocess
import termios
import unittest

import test_0036_first_use as first
import test_0033_current_contract as current
from test_0037_setup_keys import FIXTURE
from test_pty import Terminal

ACCOUNT = '00000000000000000000000000000001'
SECRET = 'synthetic-cloudflare-token'


class Cloudflare(unittest.TestCase):
    setUp = first.FirstUse.setUp
    profile = first.FirstUse.profile

    def cli(self, *args, code=0, extra=None, cwd=None):
        env = dict(self.env, PWD=str(cwd or self.cwd), **(extra or {}))
        result = subprocess.run([str(first.BIN), *map(str, args)], env=env,
                                cwd=cwd or self.cwd, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, code, result.stderr)
        return result

    @contextlib.contextmanager
    def process(self, fixture=False, extra=None):
        t = Terminal([str(FIXTURE if fixture else first.BIN), 'setup'],
                     dict(self.env, **(extra or {})), self.cwd)
        try:
            yield t
        finally:
            self.assertNotIn(SECRET.encode(), t.output)
            t.close()

    def choose(self, t, provider='clef-flash'):
        t.until(b'Semantic provider'); t.send(provider + '\r')
        t.until(b'Network permission'); t.send('\r')
        t.until(b'Fields'); t.send('\r')
        if provider == 'clef-flash':
            t.until(b'Cloudflare Account ID ['); t.send(ACCOUNT + '\r')
        t.until(b'API key [')

    def finish(self, t):
        t.until(b'Local visit tracking'); t.send('\r')
        t.until(b'Advanced settings'); t.send('\r')
        t.until(b'Saved.')

    def test_offline_config_and_selected_credential_status(self):
        self.cli('config', 'set', 'provider', 'clef-flash')
        self.cli('config', 'set', 'cloudflare_account_id', ACCOUNT)
        self.cli('config', 'set', 'semantic', 'on')
        self.env['TYPESAFE_API_KEY'] = 'synthetic-jev-key'
        status = json.loads(self.cli('credential', 'status').stdout)
        self.assertEqual(status['provider'], 'clef-flash')
        self.assertFalse(status['environment_present'])
        self.env['CLOUDFLARE_AUTH_TOKEN'] = SECRET
        status = json.loads(self.cli('credential', 'status').stdout)
        self.assertTrue(status['environment_present'])
        self.assertNotIn(SECRET.encode(), self.cli('config', 'show', '--json').stdout)
        self.assertEqual(json.loads(self.cli('doctor', '--json').stdout)['provider'], 'clef-flash')
        before = self.config.read_bytes()
        for field, value in [('provider', 'clef'), ('cloudflare_account_id', '../another'), ('api_token', SECRET)]:
            self.cli('config', 'set', field, value, code=2)
            self.assertEqual(self.config.read_bytes(), before)
        self.cli('config', 'set', 'provider', 'jev')
        self.assertEqual(json.loads(self.cli('credential', 'status').stdout)['provider'], 'jev')

    def test_account_and_token_environment_readiness_without_network(self):
        self.env.pop('J_JUMP_OFFLINE')
        self.cli('config', 'set', 'provider', 'clef-flash')
        self.cli('config', 'set', 'semantic', 'on')
        self.env['CLOUDFLARE_API_TOKEN'] = SECRET
        status = json.loads(self.cli('doctor', '--json').stdout)
        self.assertFalse(status['environment_ready'])
        self.assertIn(b'Account ID missing or invalid', self.cli('doctor').stdout)
        self.env['CLOUDFLARE_ACCOUNT_ID'] = ACCOUNT
        status = json.loads(self.cli('doctor', '--json').stdout)
        self.assertTrue(status['environment_ready'])
        self.assertEqual(status['provider_test'], 'not run')
        self.env['CLOUDFLARE_ACCOUNT_ID'] = '../another'
        self.assertFalse(json.loads(self.cli('doctor', '--json').stdout)['environment_ready'])
        self.assertIn(b'provider: jev|clef-flash', self.cli('config', 'set', '--help').stdout)

    def test_old_profile_keeps_jev_and_local_navigation(self):
        self.cli('config', 'set', 'tracking', 'on')
        cfg = json.loads(self.config.read_text())
        for key in ('provider', 'cloudflare_account_id', 'cloudflare_credential'):
            cfg.pop(key)
        self.config.write_text(json.dumps(cfg))
        self.assertEqual(json.loads(self.cli('config', 'show', '--json').stdout)['saved']['provider'], 'jev')
        self.cli('record', '--', self.target, cwd=self.target)
        self.cli('config', 'set', 'provider', 'clef-flash')
        self.cli('config', 'set', 'semantic', 'on')
        self.assertEqual(self.cli('query', 'alpha').stdout.strip(), str(self.target).encode())

    def test_invalid_account_environment_never_falls_back_to_saved_account(self):
        self.env.pop('J_JUMP_OFFLINE')
        self.cli('config', 'set', 'provider', 'clef-flash')
        self.cli('config', 'set', 'cloudflare_account_id', ACCOUNT)
        self.cli('config', 'set', 'semantic', 'on')
        self.env['CLOUDFLARE_AUTH_TOKEN'] = SECRET
        for value in ('../wrong-account', b'\xff'):
            with self.subTest(value=value):
                status = json.loads(self.cli('doctor', '--json',
                                            extra={'CLOUDFLARE_ACCOUNT_ID': value}).stdout)
                self.assertFalse(status['environment_ready'])
                output = self.cli('doctor', extra={'CLOUDFLARE_ACCOUNT_ID': value}).stdout
                self.assertIn(b'Account ID missing or invalid', output)
                env = dict(self.env, CLOUDFLARE_ACCOUNT_ID=value)
                t = Terminal([str(first.BIN), '--force-semantic', 'query', 'backend'],
                             env, self.cwd)
                try:
                    t.until(b'J5:')
                    self.assertNotIn(b'Choose a directory', t.output)
                    self.assertNotIn(SECRET.encode(), t.output)
                finally:
                    t.close()
                self.assertFalse((Path(self.env['J_JUMP_HOME']) / 'cache/semantic-drivers.db').exists())
        status = json.loads(self.cli('doctor', '--json',
                                    extra={'CLOUDFLARE_ACCOUNT_ID': ''}).stdout)
        self.assertTrue(status['environment_ready'])

    def test_hidden_cloudflare_token_saves_to_selected_entry(self):
        with self.process(fixture=True) as t:
            self.choose(t); t.send('enter\r')
            t.until(b'Clef-Flash API key (hidden;')
            self.assertFalse(termios.tcgetattr(t.fd)[3] & termios.ECHO)
            t.send(SECRET + '\r'); self.finish(t)
            self.assertIn(b'FIXTURE: credential stored in memory', t.output)
        cfg = json.loads(self.config.read_text())
        self.assertEqual(cfg['provider'], 'clef-flash')
        self.assertEqual(cfg['cloudflare_account_id'], ACCOUNT)
        self.assertEqual(cfg['cloudflare_credential'], 'system')
        self.assertEqual(cfg['credential'], 'environment')
        self.assertNotIn(SECRET, self.config.read_text())

    def test_switch_discards_unsaved_token_and_retains_existing_jev_source(self):
        with self.process(fixture=True) as t:
            self.choose(t); t.send('enter\r')
            t.until(b'Clef-Flash API key (hidden;'); t.send(SECRET + '\r')
            t.until(b'Local visit tracking'); t.send('b\r')
            self.choose(t, 'jev'); t.send('skip\r'); self.finish(t)
            self.assertNotIn(b'FIXTURE: credential stored in memory', t.output)
        cfg = json.loads(self.config.read_text())
        self.assertEqual(cfg['provider'], 'jev')
        self.assertEqual(cfg['credential'], 'environment')
        self.assertEqual(cfg['cloudflare_credential'], 'environment')

    def test_cancelling_cloudflare_setup_leaves_no_config_or_key(self):
        with self.process(fixture=True) as t:
            self.choose(t); t.send('enter\r')
            t.until(b'Clef-Flash API key (hidden;'); t.send(SECRET + '\x03')
            t.cancelled()
            self.assertNotIn(b'FIXTURE: credential stored in memory', t.output)
        self.assertFalse(self.config.exists())
        self.assertFalse((self.config.parent / 'credential.epoch').exists())

    def test_jev_environment_is_not_offered_for_cloudflare(self):
        with self.process(extra={'TYPESAFE_API_KEY': 'synthetic-jev-key'}) as t:
            self.choose(t)
            self.assertIn(b'API key [enter/keep/environment/skip/b/q; enter]', t.output)
            t.send('skip\r'); self.finish(t)
        self.assertEqual(json.loads(self.config.read_text())['cloudflare_credential'], 'environment')


class CloudflareSelection(unittest.TestCase):
    setUp = current.CurrentContract.setUp
    tearDown = current.CurrentContract.tearDown
    cli = current.CurrentContract.cli
    config = current.CurrentContract.config
    seed = current.CurrentContract.seed
    shell = current.CurrentContract.shell
    process = current.CurrentContract.process
    runtime = current.CurrentContract.runtime
    recv_exact = staticmethod(current.CurrentContract.recv_exact)
    adapter_reply = current.CurrentContract.adapter_reply
    assert_dispatch = current.CurrentContract.assert_dispatch

    def configure(self, suffix):
        current.CurrentContract.configure_semantics(self, suffix)
        self.env['CLOUDFLARE_AUTH_TOKEN'] = SECRET
        self.env['CLOUDFLARE_ACCOUNT_ID'] = ACCOUNT
        self.cli('config', 'set', 'provider', 'clef-flash')

    def test_cloudflare_suggestions_require_explicit_selection_in_three_shells(self):
        for shell in ('bash', 'zsh', 'fish'):
            self.configure('cloudflare-' + shell)
            with self.subTest(shell=shell), self.shell(shell) as t, self.adapter_reply('valid') as frames:
                t.send('ji notlexical\r'); t.until(b'Empty/q: cancel')
                self.assertIn(b'[Clef-Flash]', t.output)
                self.assertIn(b'Clef-Flash suggestion:', t.output)
                self.assertNotIn(b'[Jev]', t.output)
                t.send_and_wait_prompt('1\r')
                t.cmd('printf "WHERE:%s\\n" "$PWD"', 'SELECTED')
                self.assertIn(('WHERE:' + str(self.target)).encode(), t.output)
                self.assertNotIn(SECRET.encode(), t.output)
                self.assertEqual(frames, ['Send']); self.assert_dispatch()

    def test_cloudflare_errors_and_abstention_never_open_a_picker(self):
        for outcome in ('none', 'invalid', 'error', 'binding'):
            self.configure('cloudflare-' + outcome)
            with self.subTest(outcome=outcome), self.process('query', '--interactive', 'notlexical') as t, self.adapter_reply(outcome) as frames:
                t.until(b'J3:' if outcome == 'none' else b'J5:')
                self.assertNotIn(b'Choose a directory', t.output)
                self.assertNotIn(SECRET.encode(), t.output)
                self.assertEqual(frames, ['Send']); self.assert_dispatch()
