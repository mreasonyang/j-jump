"""Negative controls for public repository disclosure, using invented content."""
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import verify_workflow


class DisclosureTests(unittest.TestCase):
    def root(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        return Path(temporary.name)

    def test_personal_paths_rejected_across_text_formats(self):
        for suffix in (".rs", ".json", ".jsonl", ".txt", ".log", ".md"):
            with self.subTest(suffix=suffix):
                root = self.root()
                (root / ("leak" + suffix)).write_text("/" + "Users" + "/invented-owner/private/file\n")
                self.assertTrue(any("absolute user path" in error for error in verify_workflow.validate_secrets_and_paths(root)))

    def test_typesafe_credentials_rejected_in_json(self):
        root = self.root()
        (root / "leak.json").write_text(json.dumps({"credential": "apikey_" + "x" * 32}))
        self.assertTrue(any("TypeSafe credential" in error for error in verify_workflow.validate_secrets_and_paths(root)))

    def test_recorded_environment_rejected(self):
        root = self.root()
        (root / "run.json").write_text(json.dumps({"environment": {"hostname": "invented-host"}}))
        self.assertTrue(any("recorded host environment" in error for error in verify_workflow.validate_secrets_and_paths(root)))

    def test_internal_artifact_directory_rejected(self):
        root = self.root()
        path = root / "docs/evidence/run.json"
        path.parent.mkdir(parents=True)
        path.write_text("{}")
        self.assertTrue(any("internal development artifact" in error for error in verify_workflow.validate_secrets_and_paths(root)))

    def test_process_documents_rejected_even_when_force_staged(self):
        root = self.root()
        subprocess.run(["git", "init", "-b", "main"], cwd=root, check=True, capture_output=True)
        (root / ".gitignore").write_text("specs/\ndocs/\n")
        for relative in ("specs/iterations/1234-example/SPEC.md", "docs/DESIGN.md", "notes/review.md"):
            with self.subTest(relative=relative):
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("# Invented process document\n")
                subprocess.run(["git", "add", "-f", relative], cwd=root, check=True, capture_output=True)
                self.assertTrue(any(relative in error and "internal development artifact" in error
                                    for error in verify_workflow.validate_secrets_and_paths(root)))

    def test_credential_background_rejected(self):
        root = self.root()
        field = "key_" + "source"
        (root / "run.json").write_text(json.dumps({field: "previous authorization from another project"}))
        self.assertTrue(any("credential provenance" in error for error in verify_workflow.validate_secrets_and_paths(root)))

    def test_internal_conversation_background_rejected(self):
        for suffix in (".md", ".json", ".log"):
            with self.subTest(suffix=suffix):
                root = self.root()
                (root / ("notes" + suffix)).write_text("User" + ": quoted private instruction")
                self.assertTrue(any("conversation background" in error for error in verify_workflow.validate_secrets_and_paths(root)))

    def test_portable_synthetic_home_is_allowed_only_in_test_scope(self):
        root = self.root()
        (root / "tests").mkdir()
        fixture = "/" + "home" + "/alpha/project"
        (root / "tests/fixture.rs").write_text(fixture)
        self.assertEqual([], verify_workflow.validate_secrets_and_paths(root))
        (root / "report.txt").write_text(fixture)
        self.assertTrue(any("absolute user path" in error for error in verify_workflow.validate_secrets_and_paths(root)))

    def test_git_inventory_ignores_user_state_but_checks_tracked_logs(self):
        root = self.root()
        subprocess.run(["git", "init", "-b", "main"], cwd=root, check=True, capture_output=True)
        (root / "local-data").mkdir()
        (root / "local-data/private.json").write_text("apikey_" + "x" * 32)
        (root / "unrelated.json").write_text("apikey_" + "x" * 32)
        self.assertEqual([], verify_workflow.validate_secrets_and_paths(root))
        (root / "tracked.log").write_text("/" + "Users" + "/invented-owner/private/file")
        subprocess.run(["git", "add", "tracked.log"], cwd=root, check=True, capture_output=True)
        self.assertTrue(any("tracked.log" in error for error in verify_workflow.validate_secrets_and_paths(root)))


if __name__ == "__main__":
    unittest.main()
