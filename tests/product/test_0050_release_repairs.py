"""Installer directory boundaries and truthful missing-visit diagnostics."""
import hashlib
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
BIN = Path(os.environ.get("JJ_TEST_BIN", ROOT / "target/debug/jjump")).resolve()
INSTALLER = Path(os.environ.get("JJ_TEST_INSTALLER", ROOT / "packaging/install.sh"))


def snapshot(root):
    if not root.exists():
        return []
    return sorted((str(path.relative_to(root)),
                   ("link", os.readlink(path)) if path.is_symlink() else
                   ("directory",) if path.is_dir() else ("file", path.read_bytes()))
                  for path in root.rglob("*"))


class InstallerDirectoryBoundary(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.package = self.root / "package"
        self.package.mkdir()
        shutil.copy2(BIN, self.package / "jjump")
        shutil.copy2(INSTALLER, self.package / "install.sh")
        self.digest = hashlib.sha256(BIN.read_bytes()).hexdigest() + "\n"
        (self.package / "binary.sha256").write_text(self.digest)
        self.env = dict(os.environ, HOME=str(self.root), J_JUMP_HOME=str(self.root / "state"))
        (self.root / ".zshrc").write_text("# user startup fixture\n")
        (self.root / "state").mkdir()
        (self.root / "state/keep").write_bytes(b"user state fixture")

    def run_installer(self, prefix, *args, env=None):
        return subprocess.run([str(self.package / "install.sh"), "--prefix", str(prefix), *args],
                              env=env or self.env, capture_output=True, text=True, timeout=10)

    def linked_installation(self, name, kind, ownership):
        prefix = self.root / (name + " prefix with spaces")
        (prefix / "bin").mkdir(parents=True)
        outside = self.root / (name + " outside")
        if kind != "dangling":
            outside.mkdir()
            (outside / "sentinel").write_bytes(b"external fixture must remain")
        if ownership != "fresh":
            shutil.copy2(BIN, prefix / "bin/jjump")
            (prefix / "bin/j-jump").symlink_to("jjump")
            metadata = outside / "j-jump-install"
            metadata.mkdir()
            if ownership == "receipt":
                (metadata / "installed.sha256").write_text(self.digest)
            else:
                (metadata / ".install-in-progress").write_bytes(b"interrupted fixture")
        destination = outside
        if kind == "relative":
            destination = os.path.relpath(outside, prefix)
        elif kind == "chained":
            destination = self.root / (name + " intermediate")
            destination.symlink_to(outside, target_is_directory=True)
        (prefix / "share").symlink_to(destination, target_is_directory=True)
        return prefix, outside

    def test_share_links_refuse_all_lifecycles_without_changes(self):
        for kind in ("absolute", "relative", "chained", "dangling"):
            states = ("fresh",) if kind == "dangling" else ("fresh", "receipt", "marker")
            for ownership in states:
                prefix, outside = self.linked_installation(kind + ownership, kind, ownership)
                before_prefix, before_outside = snapshot(prefix), snapshot(outside)
                for args in ((), ("--replace",), ("--uninstall",)):
                    with self.subTest(kind=kind, ownership=ownership, operation=args):
                        result = self.run_installer(prefix, *args)
                        self.assertEqual(2, result.returncode, result.stdout + result.stderr)
                        self.assertIn(str(prefix / "share"), result.stderr)
                        self.assertIn("symbolic link", result.stderr)
                        self.assertEqual(before_prefix, snapshot(prefix))
                        self.assertEqual(before_outside, snapshot(outside))
                        self.assertEqual(b"user state fixture", (self.root / "state/keep").read_bytes())
                        self.assertEqual("# user startup fixture\n", (self.root / ".zshrc").read_text())

    def test_share_guard_refuses_equivalent_prefix_spellings(self):
        prefix, outside = self.linked_installation("spelling", "relative", "receipt")
        (prefix / "padding").mkdir()
        before_prefix, before_outside = snapshot(prefix), snapshot(outside)
        for value in (str(prefix) + "/", str(prefix) + "/.", str(prefix) + "/padding/..",
                      str(prefix.parent) + "//" + prefix.name):
            with self.subTest(prefix=value):
                result = self.run_installer(value, "--replace")
                self.assertEqual(2, result.returncode, result.stdout + result.stderr)
                self.assertIn("symbolic link", result.stderr)
                self.assertEqual(before_prefix, snapshot(prefix))
                self.assertEqual(before_outside, snapshot(outside))

    def test_post_creation_share_link_refuses_before_drafts(self):
        prefix, outside = self.root / "post-create-prefix", self.root / "outside"
        (outside / "j-jump-install").mkdir(parents=True)
        (outside / "sentinel").write_bytes(b"keep")
        before_outside = snapshot(outside)
        shims = self.root / "shims"
        shims.mkdir()
        shim = shims / "mkdir"
        shim.write_text('#!/bin/sh\n"$JJ_REAL_MKDIR" "$@" || exit $?\n'
                        '"$JJ_REAL_MV" "$JJ_PREFIX/share" "$JJ_PREFIX/original-share" || exit $?\n'
                        'ln -s "$JJ_OUTSIDE" "$JJ_PREFIX/share"\n')
        shim.chmod(0o755)
        env = dict(self.env, PATH=str(shims) + os.pathsep + os.environ["PATH"],
                   JJ_REAL_MKDIR=shutil.which("mkdir"), JJ_REAL_MV=shutil.which("mv"),
                   JJ_PREFIX=str(prefix), JJ_OUTSIDE=str(outside))
        result = self.run_installer(prefix, env=env)
        self.assertEqual(2, result.returncode, result.stdout + result.stderr)
        self.assertIn("symbolic link", result.stderr)
        self.assertEqual(before_outside, snapshot(outside))
        self.assertEqual([], list((prefix / "bin").iterdir()))
        self.assertEqual([], list((prefix / "original-share/j-jump-install").iterdir()))

    def test_ancestor_link_preserves_normal_lifecycle_and_state(self):
        real_parent = self.root / "real parent"
        real_parent.mkdir()
        linked_parent = self.root / "linked parent"
        linked_parent.symlink_to(real_parent, target_is_directory=True)
        prefix = linked_parent / "prefix with spaces"
        self.assertEqual(0, self.run_installer(prefix).returncode)
        before_repeat = snapshot(real_parent)
        repeat = self.run_installer(prefix)
        self.assertEqual(0, repeat.returncode, repeat.stderr)
        self.assertIn("Same archive already installed", repeat.stdout)
        self.assertEqual(before_repeat, snapshot(real_parent))
        self.assertEqual(0, self.run_installer(prefix, "--replace").returncode)
        self.assertEqual(0, self.run_installer(prefix, "--uninstall").returncode)
        self.assertFalse((prefix / "bin/jjump").exists())
        self.assertFalse((prefix / "bin/j-jump").is_symlink())
        self.assertFalse((prefix / "share/j-jump-install").exists())
        self.assertEqual(b"user state fixture", (self.root / "state/keep").read_bytes())
        self.assertEqual("# user startup fixture\n", (self.root / ".zshrc").read_text())


class PruneDiagnostics(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.home = self.root / "home"
        self.home.mkdir()
        self.state = self.root / "state"
        self.env = dict(os.environ, HOME=str(self.home), J_JUMP_HOME=str(self.state))
        for name in ("TYPESAFE_API_KEY", "J_JUMP_CONFIG", "J_JUMP_OFFLINE"):
            self.env.pop(name, None)
        self.cli("config", "set", "semantic", "off")

    def cli(self, *args, cwd=None):
        cwd = cwd or self.home
        result = subprocess.run([str(BIN), *map(str, args)], cwd=cwd,
                                env=dict(self.env, PWD=str(cwd)), capture_output=True,
                                text=True, timeout=10)
        self.assertEqual(0, result.returncode, result.stderr)
        return result.stdout

    def visit(self, name):
        path = self.home / name
        path.mkdir(parents=True)
        self.cli("record", "--", path, cwd=path)
        return path

    def stored(self):
        with sqlite3.connect(self.state / "data/visits.db") as db:
            return (db.execute("SELECT * FROM visits ORDER BY id").fetchall(),
                    db.execute("SELECT * FROM meta").fetchall())

    def test_accessible_parent_preview_and_apply_agree(self):
        gone = self.visit("accessible/gone")
        gone.rmdir()
        before = self.stored()
        preview = self.cli("history", "prune")
        self.assertEqual("1 missing visits would be deleted\n", preview)
        self.assertEqual(before, self.stored())
        self.assertEqual("1 missing visits deleted\n", self.cli("history", "prune", "--apply"))
        self.assertEqual([], self.stored()[0])

    def test_missing_parent_is_retained_in_preview_and_apply(self):
        gone = self.visit("unmounted/gone")
        shutil.rmtree(gone.parent)
        before = self.stored()
        for args, status in (((), "would be deleted"), (("--apply",), "deleted")):
            with self.subTest(operation=args):
                output = self.cli("history", "prune", *args)
                self.assertTrue(output.startswith(f"0 missing visits {status}\n"), output)
                self.assertIn("1 missing visits kept because their parent folder is also missing", output)
                self.assertIn(str(gone), output)
                self.assertEqual(before, self.stored())

    def test_mixed_inventory_lists_only_missing_parent_as_kept(self):
        deletable = self.visit("accessible/gone")
        retained = self.visit("unmounted/gone")
        existing = self.visit("present")
        deletable.rmdir()
        shutil.rmtree(retained.parent)
        before = self.stored()
        preview = self.cli("history", "prune")
        self.assertTrue(preview.startswith("1 missing visits would be deleted\n"), preview)
        self.assertIn("1 missing visits kept because", preview)
        self.assertIn(str(retained), preview)
        self.assertNotIn(str(deletable), preview)
        self.assertNotIn(str(existing), preview)
        self.assertEqual(before, self.stored())
        applied = self.cli("history", "prune", "--apply")
        self.assertTrue(applied.startswith("1 missing visits deleted\n"), applied)
        self.assertIn(str(retained), applied)
        with sqlite3.connect(self.state / "data/visits.db") as db:
            self.assertEqual({str(retained), str(existing)},
                             {row[0] for row in db.execute("SELECT path FROM visits")})

    def test_dangling_visit_symlink_uses_the_deletion_rule(self):
        gone = self.visit("accessible/gone")
        gone.rmdir()
        gone.symlink_to(gone.parent / "absent", target_is_directory=True)
        self.assertEqual("1 missing visits would be deleted\n", self.cli("history", "prune"))
        self.assertEqual("1 missing visits deleted\n", self.cli("history", "prune", "--apply"))
        self.assertTrue(gone.is_symlink())
        self.assertEqual([], self.stored()[0])

    def test_nondirectory_parent_is_not_reported_as_missing(self):
        gone = self.visit("replaced/gone")
        shutil.rmtree(gone.parent)
        gone.parent.write_bytes(b"existing file")
        before = self.stored()
        self.assertEqual("0 missing visits would be deleted\n", self.cli("history", "prune"))
        self.assertEqual("0 missing visits deleted\n", self.cli("history", "prune", "--apply"))
        self.assertEqual(before, self.stored())


if __name__ == "__main__":
    unittest.main()
