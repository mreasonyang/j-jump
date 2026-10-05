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

    def lookup(self):
        return subprocess.run(['secret-tool', 'lookup', 'service', 'j-jump.jev',
                               'username', 'typesafe-api-key'],
                              env=self.env, capture_output=True, timeout=10)

    def terminal(self, *args, env=None):
        t = Terminal([str(BIN), *args], env or self.env, ROOT)
        self.addCleanup(t.close)
        return t

    def key(self, t, secret):
        t.until(b'Jev API key (hidden;')
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


if __name__ == '__main__':
    unittest.main(verbosity=2)
