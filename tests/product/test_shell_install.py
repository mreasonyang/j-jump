"""Automatic integration through real startup files in isolated user homes."""
import hashlib
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
BIN = Path(os.environ.get("JJ_TEST_BIN", ROOT / "target/debug/jjump")).resolve()
INSTALLER = Path(os.environ.get("JJ_TEST_INSTALLER", ROOT / "packaging/install.sh"))
BEGIN = "# >>> j-jump shell integration >>>"


class ShellInstallation(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.home = self.root / "home"
        self.home.mkdir()
        self.prefix = self.root / "prefix with 'quotes' $dollar `ticks` ;semicolon"
        self.bin = self.prefix / "bin"
        self.bin.mkdir(parents=True)
        self.binary = self.bin / "jjump"
        shutil.copy2(BIN, self.binary)
        self.state = self.home / "state"
        self.target = self.home / "target with spaces"
        self.target.mkdir()
        self.env = dict(os.environ, HOME=str(self.home), SHELL="/bin/zsh",
                        PATH="/usr/bin:/bin", J_JUMP_HOME=str(self.state),
                        JJ_TARGET=str(self.target))
        for key in ("ZDOTDIR", "XDG_CONFIG_HOME", "BASH_ENV", "ENV", "J_JUMP_CONFIG",
                    "TYPESAFE_API_KEY", "J_JUMP_SEMANTIC", "J_JUMP_OFFLINE"):
            self.env.pop(key, None)

    def cli(self, *args, env=None, code=0):
        result = subprocess.run([str(self.binary), *args], env=env or self.env,
                                cwd=self.home, capture_output=True, text=True, timeout=15)
        self.assertEqual(code, result.returncode, result.stdout + result.stderr)
        return result

    def connect(self, shell="zsh", *args, env=None, code=0):
        return self.cli("shell", "install", "--shell", shell, *args, env=env, code=code)

    def remove(self, shell="zsh", *args, env=None, code=0):
        return self.cli("shell", "uninstall", "--shell", shell, *args, env=env, code=code)

    def backups(self, parent=None):
        return list((parent or self.home).glob("*.j-jump-backup-*"))

    def test_real_startup_navigation_and_exact_undo_for_all_shells(self):
        cases = [("bash", self.home / ".bashrc", b"export JJ_USER_SETTING=kept\n"),
                 ("zsh", self.home / ".zshrc", b"export JJ_USER_SETTING=kept\n"),
                 ("fish", self.home / ".config/fish/config.fish", b"set -gx JJ_USER_SETTING kept\n")]
        for shell, rc, original in cases:
            with self.subTest(shell=shell):
                rc.parent.mkdir(parents=True, exist_ok=True)
                rc.write_bytes(original)
                rc.chmod(0o640)
                self.connect(shell)
                self.assertFalse(self.state.exists(), "shell installation must not create product state")
                self.assertEqual(0o640, stat.S_IMODE(rc.stat().st_mode))
                backups = self.backups(rc.parent)
                self.assertTrue(any(path.read_bytes() == original for path in backups))
                self.assertTrue(all(stat.S_IMODE(path.stat().st_mode) == 0o600 for path in backups))
                installed, timestamp = rc.read_bytes(), rc.stat().st_mtime_ns
                self.connect(shell)
                self.assertEqual(installed, rc.read_bytes())
                self.assertEqual(timestamp, rc.stat().st_mtime_ns)
                self.assertEqual(backups, self.backups(rc.parent))
                self.cli("config", "set", "semantic", "off")
                command = 'j --offline -- "$JJ_TARGET"; printf "%s\\n" "$PWD" "$JJ_USER_SETTING"'
                modes = [["-i", "-c"]]
                if shell == "bash":
                    modes = [["--noprofile", "-i", "-c"], ["--login", "-i", "-c"]]
                for flags in modes:
                    result = subprocess.run([shutil.which(shell), *flags, command], env=self.env,
                                            cwd=self.home, capture_output=True, text=True, timeout=15)
                    self.assertEqual(0, result.returncode, result.stdout + result.stderr)
                    self.assertEqual([str(self.target), "kept"], result.stdout.strip().splitlines())
                # Undo preserves edits made outside the managed block, byte for byte.
                rc.write_bytes(installed + b"# user addition\n")
                self.remove(shell)
                self.assertEqual(original + b"# user addition\n", rc.read_bytes())
                self.remove(shell)
                shutil.rmtree(self.state)

    def test_no_trailing_newline_is_restored_exactly(self):
        rc = self.home / ".zshrc"
        rc.write_bytes(b"# original without newline")
        self.connect()
        self.remove()
        self.assertEqual(b"# original without newline", rc.read_bytes())

    def test_manual_init_in_a_message_does_not_skip_installation(self):
        rc = self.home / ".zshrc"
        rc.write_text("# documentation\necho 'jjump init zsh'\n")
        self.connect()
        self.assertIn(BEGIN, rc.read_text())

    def test_unrelated_commands_and_comments_do_not_count_as_manual_init(self):
        cases = [
            ("bash", self.home / ".bashrc", 'eval "$(printf :)" # jjump init bash\n'),
            ("zsh", self.home / ".zshrc", 'eval "$(printf :)" # jjump init zsh\n'),
            ("fish", self.home / ".config/fish/config.fish", 'echo "jjump init fish | source"\n'),
        ]
        self.cli("config", "set", "semantic", "off")
        for shell, rc, original in cases:
            with self.subTest(shell=shell):
                rc.parent.mkdir(parents=True, exist_ok=True)
                rc.write_text(original)
                self.connect(shell)
                self.assertIn(BEGIN, rc.read_text())
                flags = ["--noprofile", "-i", "-c"] if shell == "bash" else ["-i", "-c"]
                result = subprocess.run(
                    [shutil.which(shell), *flags, 'j --offline -- "$JJ_TARGET"; pwd'],
                    env=self.env, cwd=self.home, capture_output=True, text=True, timeout=15)
                self.assertEqual(0, result.returncode, result.stdout + result.stderr)
                self.assertEqual(str(self.target), result.stdout.strip().splitlines()[-1])
                self.remove(shell)
                self.assertEqual(original, rc.read_text())

    def test_bash_login_sources_the_user_rc_after_an_unrelated_bashrc(self):
        shared = self.home / "global.bashrc"
        shared.write_text("export JJ_GLOBAL_SETTING=kept\n")
        profile = self.home / ".bash_profile"
        original = f'source "{shared}"\n'
        profile.write_text(original)
        self.connect("bash")
        self.cli("config", "set", "semantic", "off")
        result = subprocess.run(
            ["bash", "--login", "-i", "-c",
             'j --offline -- "$JJ_TARGET"; printf "%s\\n" "$PWD" "$JJ_GLOBAL_SETTING"'],
            env=self.env, cwd=self.home, capture_output=True, text=True, timeout=15)
        self.assertEqual(0, result.returncode, result.stdout + result.stderr)
        self.assertEqual([str(self.target), "kept"], result.stdout.strip().splitlines())
        self.remove("bash")
        self.assertEqual(original, profile.read_text())

    def test_bash_login_does_not_source_an_already_initialized_rc_twice(self):
        rc = self.home / ".bashrc"
        rc.write_text('export JJ_RC_COUNT=$(( ${JJ_RC_COUNT:-0} + 1 ))\n')
        profile = self.home / ".bash_profile"
        original = '. "$HOME/.bashrc"\n'
        profile.write_text(original)
        self.connect("bash")
        result = subprocess.run(
            ["bash", "--login", "-i", "-c", 'type j >/dev/null; printf "%s\\n" "$JJ_RC_COUNT"'],
            env=self.env, cwd=self.home, capture_output=True, text=True, timeout=15)
        self.assertEqual(0, result.returncode, result.stdout + result.stderr)
        self.assertEqual("1", result.stdout.strip())
        self.remove("bash")
        self.assertEqual(original, profile.read_text())

    def test_managed_prefix_can_change_but_new_conflicts_are_preserved(self):
        rc = self.home / ".zshrc"
        self.connect()
        rc.write_text("alias jump='echo existing'\n" + rc.read_text())
        original = rc.read_bytes()
        self.connect("zsh", "--cmd", "jump", code=2)
        self.assertEqual(original, rc.read_bytes())
        self.connect("zsh", "--cmd", "jjnavtest")
        self.assertEqual(1, rc.read_text().count(BEGIN))
        self.assertIn("--cmd 'jjnavtest'", rc.read_text())
        self.remove()
        self.assertEqual("alias jump='echo existing'\n", rc.read_text())

    def test_two_bash_entry_points_cannot_resolve_to_one_dotfile(self):
        real = self.home / "dotfiles/bash"
        real.parent.mkdir()
        real.write_text("# shared startup file\n")
        (self.home / ".bashrc").symlink_to(real)
        (self.home / ".bash_profile").symlink_to(real)
        self.connect("bash", code=2)
        self.assertEqual("# shared startup file\n", real.read_text())
        self.assertEqual([], self.backups(real.parent))

    def test_login_profile_precedence_and_existing_bashrc_source(self):
        profile = self.home / ".bash_login"
        profile.write_text('# login configuration\n. "$HOME/.bashrc"\n')
        self.connect("bash")
        self.assertFalse((self.home / ".bash_profile").exists())
        self.assertTrue(profile.read_text().startswith('# login configuration\n. "$HOME/.bashrc"\n'))
        self.assertIn(BEGIN, profile.read_text())
        self.remove("bash")
        self.assertEqual('# login configuration\n. "$HOME/.bashrc"\n', profile.read_text())

    def test_custom_shell_locations_and_explicit_rc(self):
        for shell, variable, suffix in [("zsh", "ZDOTDIR", ".zshrc"),
                                        ("fish", "XDG_CONFIG_HOME", "fish/config.fish")]:
            with self.subTest(shell=shell):
                base = self.home / (shell + " config")
                env = dict(self.env, **{variable: str(base)})
                self.connect(shell, env=env)
                self.assertIn(BEGIN, (base / suffix).read_text())
                self.remove(shell, env=env)
                self.assertEqual("", (base / suffix).read_text())
        explicit = self.home / "custom/rc"
        self.connect("bash", "--rc", str(explicit))
        self.assertFalse((self.home / ".bashrc").exists())
        self.assertFalse((self.home / ".bash_profile").exists())
        self.remove("bash", "--rc", str(explicit))
        self.assertEqual("", explicit.read_text())

    def test_symlinked_dotfile_keeps_link_and_backs_up_real_file(self):
        real = self.home / "dotfiles/zshrc"
        real.parent.mkdir()
        real.write_text("# original\n")
        link = self.home / ".zshrc"
        link.symlink_to(real)
        self.connect()
        self.assertTrue(link.is_symlink())
        self.assertTrue(any(path.read_text() == "# original\n" for path in self.backups(real.parent)))
        self.remove()
        self.assertTrue(link.is_symlink())
        self.assertEqual("# original\n", real.read_text())

    def test_existing_manual_integration_is_preserved_without_backup(self):
        rc = self.home / ".zshrc"
        text = 'export USER_SETTING=kept\neval "$(jjump init zsh)"\n'
        rc.write_text(text)
        result = self.connect()
        self.assertIn("Existing shell integration preserved", result.stdout)
        self.assertEqual(text, rc.read_text())
        self.assertEqual([], self.backups())
        self.remove()
        self.assertEqual(text, rc.read_text())

    def test_actual_manual_init_with_custom_names_and_comments_is_preserved(self):
        cases = [
            ("bash", self.home / ".bashrc", 'eval "$(command jjump init bash --cmd jump)" # keep\n'),
            ("zsh", self.home / ".zshrc", 'eval "$(j-jump init zsh --cmd \'jump\')" # keep\n'),
            ("fish", self.home / ".config/fish/config.fish", 'jjump init fish --cmd jump | source # keep\n'),
        ]
        for shell, rc, original in cases:
            with self.subTest(shell=shell):
                rc.parent.mkdir(parents=True, exist_ok=True)
                rc.write_text(original)
                backups = self.backups(rc.parent)
                self.connect(shell)
                self.assertEqual(original, rc.read_text())
                self.assertEqual(backups, self.backups(rc.parent))
                self.remove(shell)
                self.assertEqual(original, rc.read_text())

    def test_known_alias_function_and_path_conflicts_have_actionable_remedy(self):
        rc = self.home / ".zshrc"
        for text in ["alias j='echo existing'\n", "ji() { echo existing; }\n", "function j { echo existing; }\n"]:
            with self.subTest(text=text):
                rc.write_text(text)
                result = self.connect(code=2)
                self.assertIn("--cmd jump", result.stderr)
                self.assertEqual(text, rc.read_text())
                self.assertEqual([], self.backups())
        self.connect("zsh", "--cmd", "jump")
        self.assertIn("--cmd 'jump'", rc.read_text())
        self.remove()
        rc.write_text("# no aliases\n")
        collision = self.bin / "j"
        collision.write_text("#!/bin/sh\nexit 0\n")
        collision.chmod(0o755)
        self.connect(env=dict(self.env, PATH=str(self.bin) + ":/usr/bin:/bin"), code=2)
        self.assertEqual("# no aliases\n", rc.read_text())

    def test_edited_or_malformed_managed_blocks_are_not_overwritten_or_removed(self):
        rc = self.home / ".zshrc"
        self.connect()
        damaged = rc.read_text().replace("--cmd 'j'", "--cmd 'custom'")
        rc.write_text(damaged)
        for action in (self.connect, self.remove):
            result = action(code=2)
            self.assertIn("edited", result.stderr)
            self.assertEqual(damaged, rc.read_text())
        for text in [BEGIN + "\n", "# <<< j-jump shell integration <<<\n"]:
            rc.write_text(text)
            self.remove(code=2)
            self.assertEqual(text, rc.read_text())

    def test_unsupported_shell_or_invalid_paths_make_no_changes(self):
        self.cli("shell", "install", env=dict(self.env, SHELL="/bin/nu"), code=2)
        self.connect("zsh", "--rc", "relative", code=2)
        self.connect("zsh", env=dict(self.env, ZDOTDIR="relative"), code=2)
        self.connect("fish", env=dict(self.env, XDG_CONFIG_HOME="relative"), code=2)
        self.assertFalse((self.home / ".zshrc").exists())
        self.assertFalse(self.state.exists())
        self.assertEqual([], self.backups())

    def test_dangling_links_and_nonregular_files_are_preserved(self):
        rc = self.home / ".zshrc"
        rc.symlink_to(self.home / "absent")
        self.connect(code=2)
        self.assertTrue(rc.is_symlink())
        rc.unlink()
        rc.mkdir()
        self.connect(code=2)
        self.assertTrue(rc.is_dir())
        rc.rmdir()
        rc.write_text("# hardlinked fixture\n")
        os.link(rc, self.home / "second-link")
        self.connect(code=2)
        self.assertEqual("# hardlinked fixture\n", rc.read_text())

    def test_brew_style_path_uses_stable_binary_symlink_across_upgrade(self):
        stable = self.root / "brew/bin"
        stable.mkdir(parents=True)
        (stable / "jjump").symlink_to(self.binary)
        env = dict(self.env, PATH=str(stable) + ":/usr/bin:/bin")
        self.cli("shell", "install", env=env)
        rc = self.home / ".zshrc"
        self.assertIn(str(stable / "jjump"), rc.read_text())
        self.assertNotIn(str(self.prefix), rc.read_text())
        newer = self.root / "Cellar/j-jump/next/bin/jjump"
        newer.parent.mkdir(parents=True)
        shutil.copy2(BIN, newer)
        (stable / "jjump").unlink()
        (stable / "jjump").symlink_to(newer)
        self.cli("config", "set", "semantic", "off")
        result = subprocess.run(["zsh", "-i", "-c", 'j --offline -- "$JJ_TARGET"; pwd'], env=env,
                                capture_output=True, text=True, timeout=15)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(str(self.target), result.stdout.strip())

    def test_installed_binary_takes_priority_over_another_installation(self):
        old = self.home / "older/bin"
        old.mkdir(parents=True)
        (old / "jjump").write_text("#!/bin/sh\nprintf unexpected-old-binary\\n\nexit 99\n")
        (old / "jjump").chmod(0o755)
        env = dict(self.env, PATH=str(old) + ":" + str(self.bin) + ":/usr/bin:/bin")
        self.cli("config", "set", "semantic", "off")
        for shell in ("bash", "zsh", "fish"):
            with self.subTest(shell=shell):
                self.connect(shell, env=env)
                flags = ["--noprofile", "-i", "-c"] if shell == "bash" else ["-i", "-c"]
                result = subprocess.run([shutil.which(shell), *flags, 'j --offline -- "$JJ_TARGET"; pwd'],
                                        env=env, capture_output=True, text=True, timeout=15)
                self.assertEqual(0, result.returncode, result.stderr)
                self.assertEqual(str(self.target), result.stdout.strip())

    def test_archive_default_repeat_optout_and_configuration_failure(self):
        package = self.root / "package"
        package.mkdir()
        shutil.copy2(BIN, package / "jjump")
        shutil.copy2(INSTALLER, package / "install.sh")
        (package / "binary.sha256").write_text(hashlib.sha256(BIN.read_bytes()).hexdigest() + "\n")
        def install(prefix, *args):
            return subprocess.run(["sh", str(package / "install.sh"), "--prefix", str(prefix), *args],
                                  env=self.env, capture_output=True, text=True, timeout=15)
        skipped = self.root / "skipped"
        result = install(skipped, "--no-shell")
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertFalse((self.home / ".zshrc").exists())
        result = install(skipped)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertIn(BEGIN, (self.home / ".zshrc").read_text())
        installed = (self.home / ".zshrc").read_bytes()
        self.assertEqual(0, install(skipped).returncode)
        self.assertEqual(installed, (self.home / ".zshrc").read_bytes())
        self.assertEqual(0, install(skipped, "--uninstall").returncode)
        # The guarded block remains harmless after removal of the binary.
        result = subprocess.run(["zsh", "-i", "-c", "printf ok"], env=self.env,
                                capture_output=True, text=True, timeout=15)
        self.assertEqual("ok", result.stdout)
        self.assertEqual("", result.stderr)
        (self.home / ".zshrc").write_text("alias j='echo existing'\n")
        blocked = self.root / "blocked"
        result = install(blocked)
        self.assertEqual(2, result.returncode, result.stdout + result.stderr)
        self.assertTrue((blocked / "bin/jjump").exists())
        self.assertIn("binary is installed", result.stderr)
        self.assertEqual("alias j='echo existing'\n", (self.home / ".zshrc").read_text())


if __name__ == "__main__":
    unittest.main()
