"""Fresh-install onboarding through real Bash, Zsh and Fish navigation."""
import contextlib
import json
import os
import re
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest

from test_navigation_fixes import Terminal

ROOT = Path(__file__).resolve().parents[2]
BIN = Path(os.environ.get('JJ_TEST_BIN', ROOT / 'target/debug/jjump')).resolve()
SHELLS = ('bash', 'zsh', 'fish')


class FirstUse(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name).resolve()
        self.home = self.root / 'home'
        self.cwd = self.home / 'neutral'
        self.target = self.home / 'alpha space 中文'
        self.cwd.mkdir(parents=True)
        self.target.mkdir()
        self.env = dict(HOME=str(self.home), PATH=str(BIN.parent) + ':' + os.environ['PATH'],
                        LANG='en_US.UTF-8', J_JUMP_LANG='en', J_JUMP_PICKER='numbered',
                        J_JUMP_OFFLINE='1', PS1='FIRST> ', NO_COLOR='1')
        self.profile('initial')

    def profile(self, name):
        self.state = self.root / name
        self.config = self.state / 'config/config.json'
        self.env['J_JUMP_HOME'] = str(self.state)

    def cli(self, *args, cwd=None):
        result = subprocess.run([str(BIN), *args], env=dict(self.env, PWD=str(cwd or self.cwd)),
                                cwd=cwd or self.cwd, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result

    @contextlib.contextmanager
    def shell(self, shell, name='j'):
        init = self.root / (shell + '.init')
        init.write_bytes(self.cli('init', shell, '--cmd', name).stdout)
        t = Terminal(shell, self.env, self.cwd)
        try:
            t.cmd('source ' + shlex.quote(str(init)), 'READY')
            self.assertNotIn(b'Welcome to J-Jump', t.output)
            self.assertNotIn(b'command conflict', t.output)
            yield t
        finally:
            t.close()

    def local_draft(self, t):
        t.until(b'Semantic provider')
        t.send('off\r')
        t.until(b'Local visit tracking')
        t.send('\r')
        t.until(b'Advanced settings')


    def where(self, t, path):
        t.cmd('printf "WHERE:%s\\n" "$PWD"', 'LOCATION')
        self.assertIn(('WHERE:' + str(path)).encode(), t.output)

    def test_save_resumes_j_and_ji_and_does_not_repeat(self):
        for shell in SHELLS:
            for command in ('j', 'j-path', 'ji'):
                with self.subTest(shell=shell, command=command):
                    self.profile(shell + command)
                    self.cli('record', '--', str(self.target), cwd=self.target)
                    with self.shell(shell) as t:
                        t.send({'j': 'j', 'j-path': 'j -- ' + shlex.quote(str(self.target)), 'ji': 'ji'}[command] + '\r')
                        self.local_draft(t)
                        self.assertFalse(self.config.exists())
                        t.send('\r')
                        t.until(b'Saved.')
                        if command == 'ji':
                            if b'Empty/q: cancel' not in t.output:
                                t.until(b'Empty/q: cancel')
                            chosen = re.search(rb'(\d+)\. ~/alpha space ', t.output)
                            self.assertIsNotNone(chosen, t.output)
                            t.send(chosen.group(1).decode() + '\r')
                        t.drain(.2)
                        self.where(t, self.home if command == 'j' else self.target)
                        cfg = json.loads(self.config.read_text())
                        self.assertFalse(cfg['semantic'])
                        self.assertNotIn('request_limit', cfg)
                        start = len(t.output)
                        t.cmd('j -- ' + shlex.quote(str(self.cwd)), 'AGAIN')
                        self.assertNotIn(b'Semantic provider', t.output[start:])
                    with self.shell(shell) as t:
                        t.cmd('j -- ' + shlex.quote(str(self.target)), 'NEW_SHELL')
                        self.assertNotIn(b'Semantic provider', t.output)
                    self.assertFalse(Path(str(self.config) + '.notice').exists())

    def test_cancel_eof_interrupt_stop_navigation_and_retry(self):
        for shell in SHELLS:
            for key in ('q\r', '\x04', '\x03'):
                with self.subTest(shell=shell, key=repr(key)):
                    self.profile(shell + str(ord(key[0])))
                    with self.shell(shell) as t:
                        t.send('j -- ' + shlex.quote(str(self.target)) + '\r')
                        t.until(b'Semantic provider')
                        t.send(key)
                        t.drain(.25)
                        self.where(t, self.cwd)
                        self.assertFalse(self.config.exists())
                        t.send('ji\r')
                        t.until(b'Semantic provider')
                        t.send('q\r')
                        t.drain(.15)
                        self.assertFalse(self.config.exists())

    def test_previous_directory_and_custom_name_are_onboarded(self):
        for shell in SHELLS:
            with self.subTest(shell=shell):
                self.profile(shell)
                with self.shell(shell, 'ski') as t:
                    t.cmd('cd ' + shlex.quote(str(self.target)), 'CD')
                    t.send('ski -\r')
                    self.local_draft(t)
                    t.send('\r')
                    t.until(b'Saved.')
                    t.drain(.15)
                    self.where(t, self.cwd)

    def test_help_completion_and_raw_query_do_not_start_setup(self):
        for shell in SHELLS:
            with self.subTest(shell=shell):
                self.profile(shell)
                self.cli('record', '--', str(self.target), cwd=self.target)
                with self.shell(shell) as t:
                    t.cmd('j --help; ji --help', 'HELP')
                    t.send('j alpha \t')
                    t.until(b'Empty/q: cancel')
                    t.send('q\r')
                    t.drain(.15)
                    t.send('\x15')
                    t.cmd('true', 'CANCELLED_COMPLETION')
                    self.assertNotIn(b'Semantic provider', t.output)
                    self.assertFalse(self.config.exists())
                self.cli('query', '--', str(self.target))
                self.assertFalse(self.config.exists())

    def test_pipe_input_and_redirected_diagnostics_skip_setup(self):
        for shell in SHELLS:
            with self.subTest(shell=shell):
                self.profile(shell)
                init = self.root / (shell + '.init')
                init.write_bytes(self.cli('init', shell).stdout)
                read = 'read -l line' if shell == 'fish' else 'IFS= read -r line'
                script = 'source "$INIT"; j -- "$DEST"; ' + read + '; printf "INPUT:%s\\nWHERE:%s\\n" "$line" "$PWD"'
                flags = {'bash': ['--noprofile', '--norc'], 'zsh': ['-f'], 'fish': ['--no-config']}[shell]
                r = subprocess.run([shell, *flags, '-c', script], input=b'business-data\n',
                                   env=dict(self.env, INIT=str(init), DEST=str(self.target)),
                                   capture_output=True, cwd=self.cwd, timeout=10)
                self.assertEqual(r.returncode, 0, r.stderr)
                self.assertIn(b'INPUT:business-data', r.stdout)
                self.assertIn(('WHERE:' + str(self.target)).encode(), r.stdout)
                self.assertFalse(self.config.exists())
                with self.shell(shell) as t:
                    t.cmd('j -- ' + shlex.quote(str(self.target)) + ' 2>' + shlex.quote(str(self.root / 'stderr')), 'REDIRECTED')
                    self.where(t, self.target)
                    t.cmd('j - 2>' + shlex.quote(str(self.root / 'stderr')), 'REDIRECTED_PREVIOUS')
                    self.where(t, self.cwd)
                    self.assertFalse(self.config.exists())

    def test_changed_profile_is_checked_even_after_saved_setup(self):
        for shell in SHELLS:
            with self.subTest(shell=shell):
                self.profile(shell)
                self.cli('config', 'set', 'semantic', 'off')
                fresh = self.config.with_name('other.json')
                # An old notice is not proof that setup was completed.
                Path(str(fresh) + '.notice').write_text('')
                with self.shell(shell) as t:
                    t.cmd('j -- ' + shlex.quote(str(self.target)), 'EXISTING')
                    export = 'set -gx J_JUMP_CONFIG ' if shell == 'fish' else 'export J_JUMP_CONFIG='
                    t.cmd(export + shlex.quote(str(fresh)), 'PROFILE')
                    t.send('j\r')
                    t.until(b'Semantic provider')
                    t.send('q\r')
                    t.drain(.15)
                    self.assertFalse(fresh.exists())

    def test_save_conflict_preserves_external_config_and_cwd(self):
        for shell in SHELLS:
            with self.subTest(shell=shell):
                self.profile(shell)
                with self.shell(shell) as t:
                    t.send('j -- ' + shlex.quote(str(self.target)) + '\r')
                    self.local_draft(t)
                    self.cli('config', 'set', 'tracking', 'off')
                    saved = self.config.read_bytes()
                    t.send('\r')
                    t.until(b'J7:')
                    self.where(t, self.cwd)
                    self.assertEqual(self.config.read_bytes(), saved)

    def test_existing_bad_config_keeps_direct_and_previous_navigation(self):
        for shell in SHELLS:
            with self.subTest(shell=shell):
                self.profile(shell)
                self.cli('config', 'set', 'semantic', 'off')
                self.config.write_text('{broken')
                with self.shell(shell) as t:
                    t.cmd('j -- ' + shlex.quote(str(self.target)), 'DIRECT')
                    self.where(t, self.target)
                    t.cmd('j -', 'PREVIOUS')
                    start = len(t.output)
                    self.where(t, self.cwd)
                    self.assertIn(('WHERE:' + str(self.cwd)).encode(), t.output[start:])
                    self.assertNotIn(b'Semantic provider', t.output)
                self.assertEqual(self.config.read_text(), '{broken')

    def test_chinese_first_use_can_save_without_another_command(self):
        self.env['J_JUMP_LANG'] = 'zh'
        for shell in SHELLS:
            with self.subTest(shell=shell):
                self.profile(shell)
                with self.shell(shell) as t:
                    t.send('j -- ' + shlex.quote(str(self.target)) + '\r')
                    for label, reply in [('语义服务', 'off'), ('本地访问记录', ''),
                                         ('高级设置', '')]:
                        t.until(label.encode())
                        t.send(reply + '\r')
                    t.until('已保存'.encode())
                    t.drain(.15)
                    self.where(t, self.target)
                    self.assertNotIn(b'jj credential', t.output)
                    self.assertTrue(self.config.is_file())
