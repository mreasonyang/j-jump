"""Safety boundaries for the opt-in real Homebrew lifecycle test."""
import importlib.util
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("homebrew_acceptance", ROOT / "scripts/test-homebrew-release.py")
acceptance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(acceptance)


class HomebrewAcceptanceSafety(unittest.TestCase):
    def test_credentials_shell_hooks_and_existing_brew_state_are_not_inherited(self):
        inherited = {key: "synthetic-value" for key in (
            "HOME", "J_JUMP_CONFIG", "TYPESAFE_API_KEY", "CLOUDFLARE_AUTH_TOKEN",
            "CLOUDFLARE_API_TOKEN", "CLOUDFLARE_ACCOUNT_ID", "GH_TOKEN", "DBUS_SESSION_BUS_ADDRESS",
            "BASH_ENV", "ZDOTDIR", "GIT_CONFIG_COUNT", "HOMEBREW_PREFIX", "HOMEBREW_CELLAR")}
        inherited.update(PATH=os.defpath, LANG="C")
        with patch.dict(os.environ, inherited, clear=True):
            root = Path(tempfile.gettempdir()) / "synthetic-homebrew-profile"
            env = acceptance.isolated_environment(root)
        for key in inherited.keys() - {"HOME", "PATH", "LANG"}:
            self.assertNotIn(key, env)
        self.assertEqual(str(root / "home"), env["HOME"])
        for key in ("HOMEBREW_CACHE", "HOMEBREW_LOGS", "HOMEBREW_TEMP", "J_JUMP_HOME"):
            self.assertTrue(Path(env[key]).is_relative_to(root))
        self.assertEqual("1", env["J_JUMP_OFFLINE"])

    def test_wrong_native_target_is_rejected_before_download_or_install(self):
        with patch.object(acceptance.platform, "system", return_value="Linux"), \
             patch.object(acceptance.platform, "machine", return_value="aarch64"), \
             patch.object(acceptance, "run") as run:
            args = type("Args", (), {"target": "x86_64-apple-darwin"})()
            with self.assertRaisesRegex(ValueError, "native host"):
                acceptance.lifecycle(args, Path(tempfile.gettempdir()))
            run.assert_not_called()

    def test_upgrade_and_current_install_use_independent_profiles(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            current = acceptance.profile_environment(acceptance.isolated_environment(root), root / "current-home")
            upgrade = acceptance.profile_environment(current, root / "upgrade-home")
            for key in ("HOME", "J_JUMP_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_DATA_HOME"):
                self.assertNotEqual(current[key], upgrade[key])
                self.assertTrue(Path(upgrade[key]).is_relative_to(root / "upgrade-home"))

    def test_preservation_checks_detect_deletions_and_mutations(self):
        with tempfile.TemporaryDirectory() as temporary:
            file = Path(temporary) / "config.toml"
            file.write_text("semantic = false\n")
            before = acceptance.snapshot([file])
            file.write_text("semantic = true\n")
            with self.assertRaises(AssertionError):
                acceptance.require_preserved(before, [file])
            file.unlink()
            with self.assertRaises(AssertionError):
                acceptance.require_preserved(before, [file])

    def test_public_manifest_source_mismatch_stops_acceptance(self):
        text = '{"schema_version":1,"version":"0.0.38","tag":"v0.0.38","source_sha":"wrong"}'
        with patch.object(acceptance, "run", return_value=text):
            with self.assertRaisesRegex(ValueError, "source mismatch"):
                acceptance.public_manifest("0.0.38", "x86_64-unknown-linux-musl", "a" * 40, {}, ROOT)
