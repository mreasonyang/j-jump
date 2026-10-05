"""Iteration 0046 regressions for defects found by the 0.0.27 retest; isolated synthetic state only."""
import contextlib, hashlib, os, pathlib, shutil, socket, sqlite3, subprocess, tempfile, unittest
from test_0042_alignment import BIN, ROOT, Terminal


class RetestRemediation(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name).resolve()
        self.home = self.root / 'home'
        self.cwd = self.home / 'neutral'
        self.cwd.mkdir(parents=True)
        self.state = self.root / 'state'
        self.env = dict(HOME=str(self.home), J_JUMP_HOME=str(self.state),
                        PATH=str(BIN.parent) + ':' + os.environ['PATH'], LANG='en_US.UTF-8',
                        J_JUMP_LANG='en', TERM='xterm', NO_COLOR='1', J_JUMP_PICKER='numbered')

    def tearDown(self):
        self.tmp.cleanup()

    def cli(self, *args, code=0, cwd=None, env=None):
        where = cwd or self.cwd
        r = subprocess.run([str(BIN), *map(str, args)], env=dict(env or self.env, PWD=str(where)),
                           cwd=where, capture_output=True, timeout=15)
        self.assertEqual(r.returncode, code, (args, r.stdout, r.stderr))
        return r

    def visit(self, path):
        path.mkdir(parents=True, exist_ok=True)
        self.cli('record', '--', path, cwd=path)

    @contextlib.contextmanager
    def terminal(self, *args, env=None):
        t = Terminal([str(BIN), *map(str, args)], env or self.env, self.cwd)
        try:
            yield t
        finally:
            t.close()

    def test_setup_enter_at_key_prompts_proceeds_and_warns(self):
        """AC-001, AC-003, AC-008: no loop, a missing-key warning, no internal activation note."""
        with self.terminal('setup') as t:
            t.until(b'Semantic provider')
            t.send('jev\r')
            for prompt in (b'Network permission', b'Fields'):
                t.until(prompt)
                t.send('\r')
            t.until(b'API key [')
            t.send('\r')
            t.until(b'Jev API key (hidden')
            t.send('\r')
            t.until(b'No key entered')
            for prompt in (b'Local visit tracking', b'Advanced settings'):
                t.until(prompt)
                t.send('\r')
            t.until(b'Saved.')
            t.until(b'no key configured')
            self.assertEqual(t.wait(), 0)
            self.assertNotIn(b'Shell activation is not verified', t.output)
        self.assertIn(b'semantic=true', self.cli('config', 'show').stdout)

    def test_no_credential_routes_locally_without_consent(self):
        """AC-002."""
        target = self.home / 'work/backend-service'
        self.visit(target)
        self.cli('config', 'set', 'semantic', 'on')
        with self.terminal('query', '--interactive', 'backend') as t:
            t.until(b'Jev skipped: no key configured')
            t.until(b'> ')
            self.assertNotIn(b'[y/N]', t.output)
            t.send('1\r')
            self.assertEqual(t.wait(), 0)
            self.assertIn(str(target).encode(), t.output)
        with self.terminal('query', 'zzz-nothing') as t:
            self.assertEqual(t.wait(), 3)
            self.assertIn(b'Jev skipped: no key configured', t.output)
            self.assertNotIn(b'[y/N]', t.output)
        with self.terminal('--force-semantic', 'query', 'backend') as t:
            self.assertEqual(t.wait(), 5)
            self.assertIn(b'forced Jev needs a key', t.output)

    def test_chinese_busy_error_is_consistent(self):
        """AC-004."""
        self.visit(self.home / 'code/alpha')
        holder = sqlite3.connect(self.state / 'data/visits.db', isolation_level=None)
        holder.execute('BEGIN EXCLUSIVE')
        try:
            r = self.cli('query', 'alpha', code=7, env=dict(self.env, J_JUMP_LANG='zh'))
        finally:
            holder.execute('ROLLBACK')
            holder.close()
        text = r.stderr.decode()
        self.assertIn('访问库正忙', text)
        self.assertNotIn('配置或数据状态异常', text)

    def test_provider_error_names_the_offline_browse_command(self):
        """AC-005: a refused connection reports a remedy that exists."""
        closed = socket.socket()
        closed.bind(('127.0.0.1', 0))
        port = closed.getsockname()[1]
        closed.close()
        self.visit(self.home / 'work/backend-service')
        self.cli('config', 'set', 'semantic', 'on')
        self.cli('config', 'set', 'consent', 'always')
        env = dict(self.env, TYPESAFE_API_KEY='synthetic-test-key', HTTPS_PROXY=f'http://127.0.0.1:{port}')
        try:
            with self.terminal('query', '--interactive', 'finance', env=env) as t:
                self.assertEqual(t.wait(timeout=15), 5)
                self.assertIn(b'jjump --offline query --interactive', t.output)
                self.assertNotIn(b'use local selection', t.output)
        finally:
            self.cli('adapter', 'stop', env=env)

    def test_vanished_tree_names_path_and_prune_reports_kept(self):
        """AC-007."""
        gone = self.home / 'scratch/old-experiment'
        self.visit(gone)
        shutil.rmtree(self.home / 'scratch')
        r = self.cli('query', 'old-exp', code=6)
        self.assertIn(f"jjump history forget --apply -- '{gone}'".encode(), r.stderr)
        out = self.cli('history', 'prune').stdout.decode()
        self.assertIn('kept because their parent folder is also missing', out)
        self.assertIn(str(gone), out)

    def test_help_hides_internal_commands(self):
        """AC-008."""
        text = self.cli('--help').stdout.decode()
        commands = [line.split()[0] for line in text.split('Commands:')[1].split('Options:')[0].splitlines() if line.strip()]
        self.assertNotIn('record', commands)
        self.assertIn('Show or change settings', text)
        self.assertNotIn('parent-shell activation', text)

    def test_uninstall_removes_installer_directory(self):
        """AC-009."""
        package = self.root / 'package'
        package.mkdir()
        shutil.copy2(BIN, package / 'jjump')
        shutil.copy2(ROOT / 'packaging/install.sh', package / 'install.sh')
        (package / 'binary.sha256').write_text(hashlib.sha256(BIN.read_bytes()).hexdigest() + '\n')
        prefix = self.root / 'prefix'
        env = dict(os.environ, HOME=str(self.root), J_JUMP_HOME=str(self.root / 'isolated'))
        for args in ((), ('--uninstall',)):
            r = subprocess.run([str(package / 'install.sh'), '--prefix', str(prefix), '--no-shell', *args],
                               capture_output=True, timeout=15, env=env)
            self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse((prefix / 'share/j-jump-install').exists())


if __name__ == '__main__':
    unittest.main()
