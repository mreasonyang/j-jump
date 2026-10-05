"""Verify dual-entry ownership, collisions and interrupted publication."""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
BIN = Path(os.environ.get('JJ_TEST_BIN', ROOT / 'target/debug/jjump')).resolve()


class AliasInstaller(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.package = self.root / 'package'
        self.package.mkdir()
        shutil.copy2(BIN, self.package / 'jjump')
        shutil.copy2(ROOT / 'packaging/install.sh', self.package / 'install.sh')
        (self.package / 'binary.sha256').write_text(hashlib.sha256(BIN.read_bytes()).hexdigest() + '\n')
        self.prefix = self.root / 'prefix'
        (self.prefix / 'bin').mkdir(parents=True)
        self.binary = self.prefix / 'bin/jjump'
        self.alias = self.prefix / 'bin/j-jump'
        self.receipt = self.prefix / 'share/j-jump-install/installed.sha256'
        self.marker = self.prefix / 'share/j-jump-install/.install-in-progress'
        self.unrelated = self.prefix / 'bin/jj'
        self.unrelated.write_text('unrelated command\n')

    def install(self, *args, code=0, env=None):
        result = subprocess.run([str(self.package / 'install.sh'), '--prefix', str(self.prefix), '--no-shell', *args],
                                capture_output=True, timeout=10, env=dict(env or os.environ, HOME=str(self.root), J_JUMP_HOME=str(self.root / "isolated-state")))
        self.assertEqual(result.returncode, code, result.stderr)
        self.assertEqual(self.unrelated.read_text(), 'unrelated command\n')
        return result

    def assert_pair(self):
        self.assertTrue(self.binary.is_file())
        self.assertTrue(self.alias.is_symlink())
        self.assertEqual(os.readlink(self.alias), 'jjump')
        self.assertEqual(subprocess.check_output([str(self.binary), '--version']),
                         subprocess.check_output([str(self.alias), '--version']))

    def remove_alias(self):
        if self.alias.is_symlink() or self.alias.is_file():
            self.alias.unlink()
        elif self.alias.is_dir():
            shutil.rmtree(self.alias)

    def make_collision(self, kind):
        if kind == 'file':
            self.alias.write_text('foreign content')
        elif kind == 'directory':
            self.alias.mkdir()
            (self.alias / 'keep').write_text('foreign content')
        else:
            self.alias.symlink_to({'link': str(self.unrelated), 'dangling': 'absent',
                                   'unowned-exact': 'jjump'}[kind])

    def snapshot(self):
        return sorted((str(p.relative_to(self.prefix)),
                       ('link', os.readlink(p)) if p.is_symlink() else
                       ('dir',) if p.is_dir() else ('file', p.read_bytes()))
                      for p in self.prefix.rglob('*'))

    def test_install_repair_missing_names_and_uninstall(self):
        self.install()
        self.assert_pair()
        self.alias.unlink()
        self.install('--replace')
        self.assert_pair()
        self.binary.unlink()
        self.install(code=2)
        self.install('--replace')
        self.assert_pair()
        self.install('--uninstall')
        self.assertFalse(self.binary.exists())
        self.assertFalse(self.alias.is_symlink())
        self.assertFalse(self.receipt.exists())

    def test_foreign_aliases_refuse_all_operations_without_changes(self):
        for kind in ('file', 'directory', 'link', 'dangling', 'unowned-exact'):
            with self.subTest(kind=kind):
                self.make_collision(kind)
                before = self.snapshot()
                for args in ([], ['--replace'], ['--uninstall']):
                    self.install(*args, code=2)
                    self.assertEqual(self.snapshot(), before)
                self.remove_alias()

    def test_changed_owned_alias_refuses_replace_and_uninstall(self):
        self.install()
        for kind in ('file', 'directory', 'link', 'dangling'):
            with self.subTest(kind=kind):
                self.remove_alias()
                self.make_collision(kind)
                before = self.snapshot()
                for args in (['--replace'], ['--uninstall']):
                    self.install(*args, code=2)
                    self.assertEqual(self.snapshot(), before)
        self.remove_alias()
        self.alias.symlink_to('jjump')
        self.install('--uninstall')
        self.assertFalse(self.alias.is_symlink())

    def test_interrupted_alias_publication_recovers_or_uninstalls(self):
        shims = self.root / 'shims'
        shims.mkdir()
        counter = self.root / 'count'
        shim = shims / 'mv'
        shim.write_text('#!/bin/sh\nn=0\n[ ! -f "$JJ_MV_COUNT" ] || n=$(cat "$JJ_MV_COUNT")\n'
                        'n=$((n + 1))\nprintf "%s" "$n" > "$JJ_MV_COUNT"\n'
                        '[ "$n" != "$JJ_MV_FAIL" ] || exit 71\n'
                        'exec "' + shutil.which('mv') + '" "$@"\n')
        shim.chmod(0o755)
        for fail_on in (3, 4):
            for action in ('--replace', '--uninstall'):
                with self.subTest(fail_on=fail_on, action=action):
                    counter.unlink(missing_ok=True)
                    env = dict(os.environ, PATH=str(shims) + os.pathsep + os.environ['PATH'],
                               JJ_MV_COUNT=str(counter), JJ_MV_FAIL=str(fail_on))
                    self.install(code=71, env=env)
                    self.assertTrue(self.marker.is_file())
                    self.assertTrue(self.binary.is_file())
                    self.assertEqual(self.alias.is_symlink(), fail_on == 4)
                    self.install(action)
                    if action == '--replace':
                        self.assert_pair()
                        self.install('--uninstall')
                    self.assertFalse(self.binary.exists())
                    self.assertFalse(self.alias.is_symlink())
                    self.assertFalse(self.marker.exists())
                    self.assertFalse(self.receipt.exists())
