"""Run only via scripts/test-linux-credentials.sh; never touch a login keyring."""
import json
import os
from pathlib import Path
import subprocess
import sys
import termios
import unittest

sys.path.insert(0, str(Path(__file__).parent / 'product'))
from test_pty import Terminal

BIN = Path(os.environ['JJ_TEST_BIN'])
ROOT = Path(os.environ['JJ_ISOLATED_CREDENTIAL_TEST'])
assert str(ROOT).startswith('/tmp/jjump-credential-test.')
assert Path(os.environ['HOME']) == ROOT
assert Path(os.environ['XDG_DATA_HOME']) == ROOT / 'data'
assert os.environ.get('DBUS_SESSION_BUS_ADDRESS')


class LinuxCredentials(unittest.TestCase):
    def setUp(self):
        self.state = ROOT / self._testMethodName
        self.env = dict(os.environ, J_JUMP_HOME=str(self.state), J_JUMP_OFFLINE='1')
        self.config = self.state / 'config/config.json'
        self.secret = 'synthetic-linux-credential-中文'
        self.delete()
        self.addCleanup(self.delete)

    def cli(self, *args, env=None):
        return subprocess.run([str(BIN), *args], env=env or self.env,
                              cwd=ROOT, capture_output=True, timeout=10)

    def delete(self):
        result = self.cli('credential', 'delete', '--apply')
        self.assertEqual(result.returncode, 0, result.stderr)

    def lookup(self, provider='jev'):
        service, username = ('j-jump.jev', 'typesafe-api-key') if provider == 'jev' else (
            'j-jump.cloudflare', 'workers-ai-api-token')
        return subprocess.run(['secret-tool', 'lookup', 'service', service,
                               'username', username],
                              env=self.env, capture_output=True, timeout=10)

    def terminal(self, *args, env=None):
        t = Terminal([str(BIN), *args], env or self.env, ROOT)
        self.addCleanup(t.close)
        return t

    def key(self, t, secret, provider='jev'):
        label = 'Jev' if provider == 'jev' else 'Clef-Flash'
        t.until((label + ' API key (hidden;').encode())
        self.assertFalse(termios.tcgetattr(t.fd)[3] & termios.ECHO)
        t.send(secret + '\r')

    def assert_private(self, t):
        self.assertNotIn(self.secret.encode(), t.output)
        for path in self.state.rglob('*'):
            if path.is_file():
                self.assertNotIn(self.secret.encode(), path.read_bytes())

    def test_cli_create_replace_delete(self):
        for value in (self.secret, self.secret + '-replacement'):
            t = self.terminal('credential', 'set')
            self.key(t, value)
            t.until(b'Stored in OS credential store.')
            self.assertEqual(self.lookup().stdout.rstrip(b'\n'), value.encode())
            self.assertEqual(json.loads(self.config.read_text())['credential'], 'system')
            self.assert_private(t)
        self.delete()
        self.assertNotEqual(self.lookup().returncode, 0)

    def test_setup_saves_hidden_key_to_real_service(self):
        t = self.terminal('setup')
        t.until(b'Semantic provider'); t.send('jev\r')
        t.until(b'Network permission'); t.send('\r')
        t.until(b'Fields'); t.send('\r')
        t.until(b'API key ['); t.send('enter\r')
        self.key(t, self.secret)
        t.until(b'Local visit tracking'); t.send('off\r')
        t.until(b'Advanced settings')
        self.assertFalse(self.config.exists())
        self.assertNotEqual(self.lookup().returncode, 0)
        t.send('\r'); t.until(b'Saved.')
        self.assertEqual(self.lookup().stdout.rstrip(b'\n'), self.secret.encode())
        self.assertEqual(json.loads(self.config.read_text())['credential'], 'system')
        self.assert_private(t)

    def test_cancel_retains_prior_system_key(self):
        t = self.terminal('credential', 'set')
        self.key(t, self.secret); t.until(b'Stored in OS credential store.')
        before = self.config.read_bytes()
        t = self.terminal('credential', 'set')
        self.key(t, 'discarded-synthetic-value\x03')
        t.cancelled()
        self.assertEqual(self.config.read_bytes(), before)
        self.assertEqual(self.lookup().stdout.rstrip(b'\n'), self.secret.encode())
        self.assert_private(t)

    def test_missing_service_refuses_save_and_status_stays_offline(self):
        env = dict(self.env, DBUS_SESSION_BUS_ADDRESS='unix:path=' + str(ROOT / 'absent.sock'))
        self.assertEqual(self.cli('credential', 'status', env=env).returncode, 0)
        t = self.terminal('credential', 'set', env=env)
        t.until(b'J5:')
        if b'no plaintext fallback' not in t.output:
            t.until(b'no plaintext fallback')
        self.assertFalse(self.config.exists())
        self.assertNotIn(b'Jev API key (hidden;', t.output)
        self.assertNotEqual(self.lookup().returncode, 0)
        self.assert_private(t)

    def cloudflare(self):
        self.assertEqual(self.cli('config', 'set', 'provider', 'clef-flash').returncode, 0)
        self.assertEqual(self.cli('config', 'set', 'cloudflare_account_id',
                                  '00000000000000000000000000000001').returncode, 0)

    def save_key(self, provider, value):
        self.assertEqual(self.cli('config', 'set', 'provider', provider).returncode, 0)
        t = self.terminal('credential', 'set')
        self.key(t, value, provider)
        t.until(b'Stored in OS credential store.')
        self.assertEqual(self.lookup(provider).stdout.rstrip(b'\n'), value.encode())
        self.assertNotIn(value.encode(), t.output)
        self.assert_private(t)

    def test_cloudflare_create_replace_delete(self):
        self.cloudflare()
        for value in (self.secret, self.secret + '-replacement'):
            self.save_key('clef-flash', value)
            self.assertEqual(json.loads(self.config.read_text())['cloudflare_credential'], 'system')
        self.delete()
        self.assertNotEqual(self.lookup('clef-flash').returncode, 0)

    def test_real_provider_entries_remain_independent_when_switched_and_deleted(self):
        jev = self.secret + '-jev'
        cloudflare = self.secret + '-cloudflare'
        self.save_key('jev', jev)
        self.cloudflare()
        self.save_key('clef-flash', cloudflare)
        self.assertEqual(self.lookup('jev').stdout.rstrip(b'\n'), jev.encode())
        cfg = json.loads(self.config.read_text())
        self.assertEqual((cfg['credential'], cfg['cloudflare_credential']), ('system', 'system'))
        self.delete()
        self.assertNotEqual(self.lookup('clef-flash').returncode, 0)
        self.assertEqual(self.lookup('jev').stdout.rstrip(b'\n'), jev.encode())
        self.assertEqual(self.cli('config', 'set', 'provider', 'jev').returncode, 0)
        self.delete()
        self.assertNotEqual(self.lookup('jev').returncode, 0)

    def test_cloudflare_cancel_preserves_saved_key_and_config(self):
        self.cloudflare()
        self.save_key('clef-flash', self.secret)
        before = self.config.read_bytes()
        t = self.terminal('credential', 'set')
        self.key(t, 'discarded-synthetic-value\x03', 'clef-flash')
        t.cancelled()
        self.assertEqual(self.config.read_bytes(), before)
        self.assertEqual(self.lookup('clef-flash').stdout.rstrip(b'\n'), self.secret.encode())
        self.assert_private(t)

    def test_cloudflare_setup_saves_hidden_key_to_real_service(self):
        t = self.terminal('setup')
        t.until(b'Semantic provider'); t.send('clef-flash\r')
        t.until(b'Network permission'); t.send('\r')
        t.until(b'Fields'); t.send('\r')
        t.until(b'Cloudflare Account ID ['); t.send('00000000000000000000000000000001\r')
        t.until(b'API key ['); t.send('enter\r')
        self.key(t, self.secret, 'clef-flash')
        t.until(b'Local visit tracking'); t.send('off\r')
        t.until(b'Advanced settings')
        self.assertFalse(self.config.exists())
        self.assertNotEqual(self.lookup('clef-flash').returncode, 0)
        t.send('\r'); t.until(b'Saved.')
        cfg = json.loads(self.config.read_text())
        self.assertEqual(cfg['provider'], 'clef-flash')
        self.assertEqual(cfg['cloudflare_credential'], 'system')
        self.assertEqual(self.lookup('clef-flash').stdout.rstrip(b'\n'), self.secret.encode())
        self.assert_private(t)

    def test_cloudflare_missing_service_preserves_config_and_refuses_plaintext(self):
        self.cloudflare()
        before = self.config.read_bytes()
        env = dict(self.env, DBUS_SESSION_BUS_ADDRESS='unix:path=' + str(ROOT / 'absent.sock'))
        self.assertEqual(self.cli('credential', 'status', env=env).returncode, 0)
        t = self.terminal('credential', 'set', env=env)
        t.until(b'J5:')
        if b'no plaintext fallback' not in t.output:
            t.until(b'no plaintext fallback')
        self.assertEqual(self.config.read_bytes(), before)
        self.assertNotIn(b'Clef-Flash API key (hidden;', t.output)
        self.assertNotEqual(self.lookup('clef-flash').returncode, 0)
        self.assert_private(t)

    def test_back_after_provider_switch_preserves_both_existing_system_keys(self):
        jev, cloudflare = self.secret + '-jev', self.secret + '-cloudflare'
        try:
            self.save_key('jev', jev)
            self.cloudflare()
            self.save_key('clef-flash', cloudflare)
            for source, destination in (('jev', 'clef-flash'), ('clef-flash', 'jev')):
                with self.subTest(source=source):
                    self.assertEqual(self.cli('config', 'set', 'provider', source).returncode, 0)
                    t = self.terminal('setup')
                    menu = b's Save; q Exit without saving'
                    t.until(menu); t.send('6\r')
                    if source == 'clef-flash':
                        t.until(b'Cloudflare Account ID ['); t.send('\r')
                    t.until(b'API key ['); t.send('enter\r')
                    self.key(t, 'discarded-pending-key', source)
                    t.until(menu); t.send('1\r')
                    t.until(b'Semantic provider'); t.send(destination + '\r')
                    t.until(b'Network permission'); t.send('b\r')
                    t.until(b'Semantic provider'); t.send('b\r')
                    t.until(menu); t.send('s\r'); t.until(b'Saved.')
                    self.assertEqual(self.lookup('jev').stdout.rstrip(b'\n'), jev.encode())
                    self.assertEqual(self.lookup('clef-flash').stdout.rstrip(b'\n'), cloudflare.encode())
                    self.assertNotIn(b'discarded-pending-key', t.output)
                    self.assert_private(t)
        finally:
            for provider in ('jev', 'clef-flash'):
                self.cli('config', 'set', 'provider', provider)
                self.delete()


if __name__ == '__main__':
    unittest.main(verbosity=2)
