"""Exercise the short entry point with equivalent names and no dependency on jj."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
BIN = Path(os.environ.get('JJ_TEST_BIN', ROOT / 'target/debug/jjump')).resolve()


class CommandAliases(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.home = self.root / 'home'
        self.home.mkdir()
        self.bindir = self.root / 'bin'
        self.bindir.mkdir()
        (self.bindir / 'jjump').symlink_to(BIN)
        (self.bindir / 'j-jump').symlink_to(BIN.with_name('j-jump'))
        poison = self.bindir / 'jj'
        poison.write_text('#!/bin/sh\nprintf old-name-called > "$HOME/poison"\nexit 99\n')
        poison.chmod(0o755)
        self.env = dict(os.environ, HOME=str(self.home),
                        J_JUMP_HOME=str(self.root / 'state'), J_JUMP_LANG='en',
                        J_JUMP_OFFLINE='1', J_JUMP_PICKER='numbered',
                        PATH=str(self.bindir) + os.pathsep + os.environ['PATH'])
        for key in ('J_JUMP_CONFIG', 'TYPESAFE_API_KEY'):
            self.env.pop(key, None)

    def run_jj(self, *args, code=0, name='jjump'):
        result = subprocess.run([name, *args], env=self.env, cwd=self.home,
                                capture_output=True, timeout=10)
        self.assertEqual(result.returncode, code, result.stderr)
        return result.stdout

    def test_management_and_local_state(self):
        self.assertEqual(self.run_jj('--version').decode().strip(),
                         'jjump ' + (ROOT / 'VERSION').read_text().strip())
        for language in ('en', 'zh'):
            self.env['J_JUMP_LANG'] = language
            for args in [('--help',), ('config', 'set', '--help'), ('--version',), ('credential', '--help')]:
                output = self.run_jj(*args)
                self.assertIn(b'jjump ', output)
                self.assertNotIn(b'j-jump ', output)
                self.assertEqual(output, self.run_jj(*args, name='j-jump'))
        self.run_jj('config', 'set', 'language', 'en', name='j-jump')
        saved = json.loads(self.run_jj('config', 'show', '--json'))['saved']
        self.assertEqual(saved['schema_version'], 3)
        self.assertEqual(saved['language'], 'en')
        self.assertTrue((self.root / 'state/config/config.json').is_file())
        self.assertEqual(self.run_jj('doctor', '--json'), self.run_jj('doctor', '--json', name='j-jump'))
        error1 = subprocess.run(['jjump', 'not-a-command'], env=self.env, capture_output=True)
        error2 = subprocess.run(['j-jump', 'not-a-command'], env=self.env, capture_output=True)
        self.assertEqual((error1.returncode, error1.stdout, error1.stderr),
                         (error2.returncode, error2.stdout, error2.stderr))
        self.assertFalse((self.home / 'poison').exists())

    def test_three_shells_never_call_jj(self):
        destination = self.home / 'alpha space 中文'
        destination.mkdir()
        for shell in ('bash', 'zsh', 'fish'):
            with self.subTest(shell=shell):
                init = self.root / (shell + '.init')
                canonical = self.run_jj('init', shell)
                self.assertEqual(canonical, self.run_jj('init', shell, name='j-jump'))
                init.write_bytes(canonical)
                self.assertNotIn(b'command j-jump', init.read_bytes())
                # INIT/TARGET are environment data, never interpolated shell code.
                script = 'source "$INIT"; j -- "$TARGET"; pwd; ji --help'
                flags = {'bash': ['--noprofile', '--norc'],
                         'zsh': ['-f'], 'fish': ['--no-config']}[shell]
                result = subprocess.run([shutil.which(shell), *flags, '-c', script],
                                        env=dict(self.env, INIT=str(init), TARGET=str(destination)),
                                        cwd=self.home, capture_output=True, timeout=10)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn((str(destination) + '\n').encode(), result.stdout)
                self.assertFalse((self.home / 'poison').exists())
                self.run_jj('init', shell, '--cmd', 'jjump', code=2)

    def test_two_cargo_entries(self):
        metadata = json.loads(subprocess.check_output(
            ['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1'],
            cwd=ROOT, timeout=30))
        package = next(p for p in metadata['packages'] if p['name'] == 'j-jump')
        self.assertEqual(sorted(t['name'] for t in package['targets'] if 'bin' in t['kind']), ['j-jump', 'jjump'])

    def test_shared_history_and_alias_setup(self):
        from test_pty import Terminal
        destination = self.home / 'shared-history'
        destination.mkdir()
        result = subprocess.run(['j-jump', 'record', '--', str(destination)],
                                cwd=destination, env=dict(self.env, PWD=str(destination)),
                                capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.run_jj('query', 'shared-history'),
                         (str(destination) + '\n').encode())
        self.assertEqual(self.run_jj('query', 'shared-history'),
                         self.run_jj('query', 'shared-history', name='j-jump'))
        terminal = Terminal(['j-jump', 'setup'], self.env, self.home)
        try:
            terminal.until(b'Semantic provider')
            terminal.send('q\n')
            terminal.cancelled()
        finally:
            terminal.close()
        self.assertFalse((self.root / 'state/config/config.json').exists())
