"""Iteration 0027 W5 regressions.

AC-009 installer recoverability: a damaged installation must still reach an
actionable outcome, a prior binary must survive a failed verification, and an
unowned tree must still be refused.

AC-013 licence inventory: a built package must account for every dependency it
ships, either with collected licence text or with an explicit recorded omission.
"""
import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
BIN = pathlib.Path(os.environ.get('JJ_TEST_BIN', ROOT / 'target/debug/jjump')).resolve()
INSTALLER = ROOT / 'packaging/install.sh'
RECEIPT = 'share/j-jump-install/installed.sha256'
MARKER = 'share/j-jump-install/.install-in-progress'


def sha256(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


def remove(path):
    """Remove a file, a directory or a symlink, whichever sits at the path."""
    path = pathlib.Path(path)
    if path.is_symlink() or path.is_file():
        path.unlink()
    elif path.is_dir():
        shutil.rmtree(path)


class InstallerRecoverability(unittest.TestCase):
    """AC-009. Every case installs into an isolated prefix under a scratch dir."""

    def setUp(self):
        if not BIN.is_file():
            self.skipTest(f'no test binary at {BIN}')
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.scratch = pathlib.Path(temporary.name).resolve()
        self.package = self.scratch / 'package'
        self.package.mkdir()
        shutil.copy2(BIN, self.package / 'jjump')
        shutil.copy2(INSTALLER, self.package / 'install.sh')
        (self.package / 'binary.sha256').write_text(sha256(BIN) + '\n')
        self.prefix = self.scratch / 'prefix'

    @property
    def script(self):
        return self.package / 'install.sh'

    def run_installer(self, *arguments, prefix=None, env=None):
        command = [str(self.script), '--prefix', str(prefix or self.prefix), *arguments]
        command.append('--no-shell')
        return subprocess.run(command, capture_output=True, text=True, timeout=60, env=dict(env or os.environ, HOME=str(self.scratch), J_JUMP_HOME=str(self.scratch / "isolated-state")))

    def install(self, *arguments, prefix=None, env=None):
        result = self.run_installer(*arguments, prefix=prefix, env=env)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result

    def test_replace_keeps_only_current_binary_and_uninstalls(self):
        """TEST-009: an untouched owned installation replaces and uninstalls."""
        self.install()
        installed = self.prefix / 'bin/jjump'
        self.assertEqual(subprocess.check_output([str(installed), '--version']).strip(),
                         b'jjump ' + (ROOT / 'VERSION').read_text().strip().encode())
        self.install('--replace')
        self.assertFalse((self.prefix / 'bin/jjump.previous').exists())
        self.assertEqual(installed.read_bytes(), (self.package / 'jjump').read_bytes())
        removed = self.install('--uninstall')
        self.assertIn('retained', removed.stdout)
        self.assertFalse(installed.exists())
        self.assertFalse((self.prefix / RECEIPT).exists())

    def test_modified_executable_still_uninstalls_and_replace_refuses_safely(self):
        """TEST-009: modification blocks replacement, never removal."""
        self.install()
        installed = self.prefix / 'bin/jjump'
        installed.write_bytes(b'user-modified')
        refused = self.run_installer('--replace')
        self.assertEqual(refused.returncode, 2, refused.stderr)
        self.assertEqual(installed.read_bytes(), b'user-modified',
                         'a modified prior binary must not be destroyed by --replace')
        self.assertIn('--uninstall', refused.stderr, 'the refusal must name a working remedy')
        self.assertIn(str(self.prefix), refused.stderr)
        removed = self.run_installer('--uninstall')
        self.assertEqual(removed.returncode, 0, removed.stderr)
        self.assertIn('modified', removed.stdout)
        self.assertFalse(installed.exists())
        self.assertFalse((self.prefix / RECEIPT).exists())
        # the printed remedy re-establishes both operations on the same prefix
        self.install()
        self.install('--uninstall')
        self.assertFalse(installed.exists())

    def test_damaged_receipt_refuses_with_a_remedy_and_stays_recoverable(self):
        """TEST-009: garbage, empty, directory and symlinked receipts."""
        for kind in ('garbage', 'empty', 'directory', 'symlink'):
            with self.subTest(receipt=kind):
                prefix = self.scratch / f'receipt-{kind}'
                self.install(prefix=prefix)
                receipt = prefix / RECEIPT
                if kind == 'garbage':
                    receipt.write_text('not-a-sha256\n')
                elif kind == 'empty':
                    receipt.write_text('')
                elif kind == 'directory':
                    receipt.unlink()
                    receipt.mkdir()
                else:
                    receipt.unlink()
                    receipt.symlink_to(self.scratch / 'unrelated-target')
                for operation in ('--uninstall', '--replace'):
                    result = self.run_installer(operation, prefix=prefix)
                    self.assertEqual(result.returncode, 2,
                                     f'{kind} receipt must refuse {operation}, not fail silently')
                    self.assertIn(str(receipt), result.stderr,
                                  'the refusal must name the damaged object')
                    self.assertIn('rm -rf', result.stderr,
                                  'the refusal must print an actionable removal remedy')
                # Following the printed remedy must restore both operations, so
                # "refused" never becomes "permanently blocked".
                remove(prefix / 'bin/jjump')
                remove(prefix / 'bin/j-jump')
                remove(receipt)
                self.install(prefix=prefix)
                self.install('--uninstall', prefix=prefix)
                self.assertFalse((prefix / 'bin/jjump').exists())

    def test_install_interrupted_between_its_two_renames_is_recoverable(self):
        """TEST-009: an install killed between its renames."""
        real_mv = shutil.which('mv', path=os.environ.get('PATH', ''))
        self.assertIsNotNone(real_mv, 'the fixture needs a real mv on PATH')
        shim = self.scratch / 'shim'
        shim.mkdir()
        counter = self.scratch / 'mv-count'
        # The installer renames the marker, then the executable, then the receipt.
        # Failing on call JJ_MV_FAIL stops it at a chosen point in that sequence.
        (shim / 'mv').write_text(
            '#!/bin/sh\n'
            'n=$(cat "$JJ_MV_COUNT" 2>/dev/null || echo 0)\n'
            'n=$((n + 1))\n'
            'printf %s "$n" > "$JJ_MV_COUNT"\n'
            '[ "$n" -eq "$JJ_MV_FAIL" ] && exit 1\n'
            f'exec "{real_mv}" "$@"\n')
        os.chmod(shim / 'mv', 0o755)

        def interrupt(prefix, fail_on):
            environment = dict(os.environ, PATH=str(shim) + os.pathsep + os.environ.get('PATH', ''),
                               JJ_MV_COUNT=str(counter), JJ_MV_FAIL=str(fail_on))
            result = subprocess.run([str(self.script), '--prefix', str(prefix)],
                                    capture_output=True, text=True, timeout=60, env=environment)
            self.assertNotEqual(result.returncode, 0, 'the fixture must stop the installer mid-install')

        # Stopped after the marker rename but before the executable rename.
        marker_only = self.scratch / 'interrupted-marker-only'
        interrupt(marker_only, 2)
        self.assertTrue((marker_only / MARKER).is_file())
        self.assertFalse((marker_only / 'bin/jjump').exists())
        self.install('--replace', prefix=marker_only)
        self.assertFalse((marker_only / MARKER).exists())

        # Stopped between the executable rename and the receipt rename: exactly
        # the state a kill between the two renames leaves.
        replaced = self.scratch / 'interrupted-replace'
        counter.unlink(missing_ok=True)
        interrupt(replaced, 3)
        self.assertTrue((replaced / 'bin/jjump').is_file())
        self.assertFalse((replaced / RECEIPT).exists())
        self.assertTrue((replaced / MARKER).is_file())
        self.install('--replace', prefix=replaced)
        self.assertTrue((replaced / RECEIPT).is_file())
        self.assertFalse((replaced / MARKER).exists())
        self.install('--uninstall', prefix=replaced)

        removed = self.scratch / 'interrupted-uninstall'
        counter.unlink(missing_ok=True)
        interrupt(removed, 3)
        result = self.run_installer('--uninstall', prefix=removed)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((removed / 'bin/jjump').exists())
        self.assertFalse((removed / MARKER).exists())

    def test_symlinked_marker_cannot_damage_another_file(self):
        """A linked ownership marker must not be followed, written through or trusted."""
        payload = b'IRREPLACEABLE-USER-DATA-0123456789'
        victim = self.scratch / 'unrelated-user-file'
        victim.write_bytes(payload)
        digest = sha256(victim)
        prefix = self.scratch / 'linked-marker'
        (prefix / 'bin').mkdir(parents=True)
        (prefix / 'share/j-jump-install').mkdir(parents=True)
        foreign = prefix / 'bin/jjump'
        foreign.write_bytes(b'someone-elses-binary')
        marker = prefix / MARKER
        marker.symlink_to(victim)

        # A link at the marker path is not ownership evidence for someone else's tree.
        result = self.run_installer('--uninstall', prefix=prefix)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(foreign.read_bytes(), b'someone-elses-binary',
                         'a linked marker must not make an unrelated tree removable')
        # ... and an install must neither write through it nor truncate its target.
        for operation in ((), ('--replace',)):
            with self.subTest(operation=operation or ('install',)):
                result = self.run_installer(*operation, prefix=prefix)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertIn(str(marker), result.stderr)
                self.assertIn('symbolic link', result.stderr)
                self.assertEqual(sha256(victim), digest, 'the linked file must be byte-identical')
                self.assertEqual(victim.read_bytes(), payload)
                self.assertEqual(foreign.read_bytes(), b'someone-elses-binary')
        self.assertTrue(marker.is_symlink(), 'refusing must not replace the link')

        # A dangling link is refused the same way, so nothing is created at its target.
        absent = self.scratch / 'never-created'
        marker.unlink()
        marker.symlink_to(absent)
        result = self.run_installer(prefix=prefix)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertFalse(absent.exists(), 'no file may be created at the link target')

        # The printed remedy restores a clean install and uninstall.
        remove(marker)
        remove(foreign)
        self.install(prefix=prefix)
        self.install('--uninstall', prefix=prefix)
        self.assertFalse((prefix / 'bin/jjump').exists())
        self.assertEqual(victim.read_bytes(), payload)

    def test_unowned_tree_is_refused_and_left_untouched(self):
        """TEST-009: no receipt and no marker means no ownership."""
        prefix = self.scratch / 'unowned'
        (prefix / 'bin').mkdir(parents=True)
        foreign = prefix / 'bin/jjump'
        foreign.write_bytes(b'someone-elses-binary')
        for operation in ((), ('--uninstall',), ('--replace',)):
            result = self.run_installer(*operation, prefix=prefix)
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertIn('cannot prove it owns', result.stderr)
            self.assertIn('rm -rf', result.stderr, 'the refusal must print a remedy')
            self.assertEqual(foreign.read_bytes(), b'someone-elses-binary')

    def test_symlinked_prefix_is_refused_but_a_symlinked_ancestor_is_not(self):
        """Resolved behaviour: the installer refuses a linked prefix."""
        real = self.scratch / 'real-prefix'
        real.mkdir()
        link = self.scratch / 'linked-prefix'
        link.symlink_to(real)
        for operation in ((), ('--uninstall',), ('--replace',)):
            result = self.run_installer(*operation, prefix=link)
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertIn('symbolic link', result.stderr)
            self.assertIn(str(link), result.stderr)
        self.assertFalse((real / 'bin/jjump').exists())
        # A link above the prefix is not the installation's own path component.
        ancestor = self.scratch / 'ancestor-link'
        ancestor.symlink_to(self.scratch)
        nested = ancestor / 'prefix-through-ancestor-link'
        self.install(prefix=nested)
        self.assertTrue((self.scratch / 'prefix-through-ancestor-link/bin/jjump').is_file())

    def test_failed_package_verification_leaves_the_prior_binary_alone(self):
        """A prior binary must not be destroyed if the new one cannot be verified."""
        self.install()
        installed = self.prefix / 'bin/jjump'
        before = installed.read_bytes()
        receipt = (self.prefix / RECEIPT).read_text()
        (self.package / 'binary.sha256').write_text('wrong\n')
        result = self.run_installer('--replace')
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn('checksum mismatch', result.stderr)
        self.assertEqual(installed.read_bytes(), before)
        self.assertEqual((self.prefix / RECEIPT).read_text(), receipt)
        self.assertFalse((self.prefix / 'bin/jjump.previous').exists())


class PackageLicenseInventory(unittest.TestCase):
    """AC-013. Builds one real archive into a scratch directory and inspects it."""

    def test_neutral_tar_and_gzip_metadata(self):
        header = self.archive.read_bytes()[:10]
        self.assertEqual(b"\x00\x00\x00\x00", header[4:8])
        self.assertEqual(0, header[3])
        with tarfile.open(self.archive) as archive:
            timestamps = set()
            for member in archive.getmembers():
                self.assertEqual((0, 0, "", ""), (member.uid, member.gid, member.uname, member.gname))
                self.assertFalse(set(member.pax_headers) - {"path", "linkpath"})
                expected = 0o755 if member.isdir() or member.name.endswith(("/jjump", "/install.sh")) else 0o777 if member.issym() else 0o644
                self.assertEqual(expected, member.mode)
                timestamps.add(member.mtime)
            self.assertEqual(1, len(timestamps))

    def test_repeated_packing_preserves_exact_bytes(self):
        destination = self.scratch / "repeat"
        subprocess.run([sys.executable, str(ROOT / 'scripts/package.py'), '--binary', str(BIN),
                        '--target', self.target, '--dist', str(destination)],
                       cwd=ROOT, check=True, capture_output=True, timeout=900)
        self.assertEqual(self.archive.read_bytes(), next(destination.glob('*.tar.gz')).read_bytes())

    @classmethod
    def setUpClass(cls):
        if not BIN.is_file():
            raise unittest.SkipTest(f'no test binary at {BIN}')
        try:
            host = subprocess.check_output(['rustc', '-vV'], text=True, timeout=60)
            cls.target = re.search(r'^host: (\S+)', host, re.M).group(1)
        except (OSError, subprocess.SubprocessError, AttributeError):
            raise unittest.SkipTest('rustc is unavailable')
        try:
            subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT,
                                    stderr=subprocess.DEVNULL, timeout=60)
            subprocess.check_output(['cargo', 'metadata', '--locked', '--format-version', '1',
                                     '--filter-platform', cls.target], cwd=ROOT,
                                    stderr=subprocess.DEVNULL, timeout=300)
        except (OSError, subprocess.SubprocessError) as error:
            raise unittest.SkipTest(f'the package build environment is unavailable: {error}')
        cls.temporary = tempfile.TemporaryDirectory()
        cls.scratch = pathlib.Path(cls.temporary.name)
        build = subprocess.run([sys.executable, str(ROOT / 'scripts/package.py'),
                                '--binary', str(BIN), '--target', cls.target,
                                '--dist', str(cls.scratch)],
                               capture_output=True, text=True, timeout=900)
        if build.returncode != 0:
            raise AssertionError(f'scripts/package.py failed: {build.stderr.strip()}')
        cls.archive = next(cls.scratch.glob('*.tar.gz'))
        with tarfile.open(cls.archive) as tar:
            cls.members = tar.getnames()
            cls.folder = cls.members[0].split('/')[0]
            cls.dependencies = json.loads(
                tar.extractfile(f'{cls.folder}/dependencies.json').read())
            cls.missing_file = tar.extractfile(
                f'{cls.folder}/licenses/MISSING.txt').read().decode()
        cls.version = (ROOT / 'VERSION').read_text().strip()
        cls.by_name = {}
        for package in cls.dependencies['packages']:
            cls.by_name.setdefault(package['name'], []).append(package)

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def test_every_shipped_dependency_has_text_or_a_recorded_omission(self):
        """TEST-013: the inventory covers the resolved graph, silence is not evidence."""
        inventory = {(package['name'], package['version'])
                     for package in self.dependencies['packages']}
        self.assertIn(f'{self.folder}/jjump', self.members)
        self.assertIn(f'{self.folder}/j-jump', self.members)
        missing = {(entry['name'], entry['version'])
                   for entry in self.dependencies['missing_license_text']}
        locked = {(match.group(1), match.group(2)) for match in re.finditer(
            r'^name = "(.*)"\nversion = "(.*)"', (ROOT / 'Cargo.lock').read_text(), re.M)}
        self.assertTrue(inventory, 'the inventory must not be empty')
        self.assertLessEqual(inventory, locked, 'every inventoried package must come from Cargo.lock')
        self.assertIn(('j-jump', self.version), inventory, 'the project itself is part of the graph')
        for package in self.dependencies['packages']:
            key = (package['name'], package['version'])
            self.assertFalse(package['license_files'] and key in missing,
                             f'{key} is both collected and recorded as missing')
            self.assertTrue(package['license_files'] or key in missing,
                            f'{key} ships no licence text and its omission is not recorded')
        for entry in self.dependencies['missing_license_text']:
            self.assertTrue(entry['reason'], f'{entry} must record why the text is absent')
            self.assertEqual(entry['declared_license'], self.by_name[entry['name']][0]['license'])
        self.assertEqual(self.dependencies['package_count'], len(self.dependencies['packages']))
        self.assertEqual(self.dependencies['packages_with_license_text'] + len(missing),
                         self.dependencies['package_count'])
        self.assertEqual(self.dependencies['schema_version'], 2)
        self.assertEqual(self.dependencies['target'], self.target)

    def test_licence_texts_the_fixed_name_list_used_to_miss(self):
        """TEST-013: variant names, nested texts and crates that never matched."""
        expected = {
            'tinyvec': ['LICENSE-APACHE.md', 'LICENSE-MIT.md', 'LICENSE-ZLIB.md'],
            'unicode-ident': ['LICENSE-UNICODE'],
            'ring': ['LICENSE-BoringSSL', 'LICENSE-other-bits'],
        }
        for name, texts in expected.items():
            self.assertIn(name, self.by_name, f'{name} must be part of the resolved graph')
            for package in self.by_name[name]:
                for text in texts:
                    self.assertIn(text, package['license_files'],
                                  f'{name}-{package["version"]} must collect {text}')
                    self.assertIn(f'{self.folder}/licenses/{name}-{package["version"]}/{text}',
                                  self.members, f'{name}-{package["version"]}/{text} must be in the archive')

    def test_project_licence_is_in_the_archive(self):
        """TEST-013: the project's own notice ships with the archive."""
        self.assertIn(f'{self.folder}/LICENSE.md', self.members)
        self.assertIn(f'{self.folder}/licenses/j-jump-{self.version}/LICENSE.md', self.members)
        self.assertIn('LICENSE.md', self.by_name['j-jump'][0]['license_files'])

    def test_recorded_omissions_are_machine_readable(self):
        """TEST-013: an omission is explicit evidence, not silence."""
        entries = self.dependencies['missing_license_text']
        lines = [line for line in self.missing_file.splitlines() if line.strip()]
        self.assertEqual(len(lines) - 1, len(entries), 'the first line is the MISSING.txt header')
        for entry in entries:
            self.assertIn(f"{entry['name']}-{entry['version']}", self.missing_file)
        self.assertEqual(len(entries), 1, 'only crates that genuinely ship no text may be listed')
        self.assertEqual(entries[0]['name'], 'unicode-casefold',
                         'the one crate known to ship no licence or notice file')


if __name__ == '__main__':
    unittest.main()
