from __future__ import annotations

import shutil
import json
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))
import verify_workflow  # noqa: E402


class WorkflowValidatorTests(unittest.TestCase):
    def test_release_builder_and_packager_require_product_claim(self):
        for path in ("scripts/build-release.py", "scripts/package.py"):
            with self.subTest(path=path), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                subprocess.run(["git", "init", "-b", "main", str(root)], check=True, capture_output=True)
                subprocess.run(["git", "config", "core.hooksPath", ".githooks"], cwd=root, check=True)
                (root / "scripts").mkdir()
                (root / path).write_text("# initial fixture\n")
                subprocess.run(["git", "add", path], cwd=root, check=True)
                subprocess.run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                                "commit", "-m", "fixture"], cwd=root, check=True, capture_output=True)
                base = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
                (root / path).write_text("# changed fixture\n")
                errors = verify_workflow.validate_local_git(root, [], base)
                self.assertTrue(any("without one active Product claim" in error for error in errors), errors)

    def test_download_installer_requires_an_active_product_claim(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            subprocess.run(["git", "init", "-b", "main", str(root)], check=True, capture_output=True)
            subprocess.run(["git", "config", "core.hooksPath", ".githooks"], cwd=root, check=True)
            (root / "install.sh").write_text("#!/bin/sh\nexit 0\n")
            subprocess.run(["git", "add", "install.sh"], cwd=root, check=True)
            subprocess.run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.com",
                            "commit", "-m", "fixture"], cwd=root, check=True, capture_output=True)
            base = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
            (root / "install.sh").write_text("#!/bin/sh\nexit 2\n")
            errors = verify_workflow.validate_local_git(root, [], base)
            self.assertTrue(any("without one active Product claim" in error for error in errors), errors)

    def fixture(self) -> tempfile.TemporaryDirectory[str]:
        temporary = tempfile.TemporaryDirectory()
        destination = Path(temporary.name) / "repo"
        shutil.copytree(
            REPO_ROOT,
            destination,
            ignore=shutil.ignore_patterns(".git", ".cargo", "__pycache__", "*.pyc", "target", "dist", "local-data", ".j-jump"),
        )
        records = Path(temporary.name) / "private-records"
        (records / "specs/iterations/0000-workflow-bootstrap").mkdir(parents=True)
        for relative in verify_workflow.PROCESS_FILES:
            target = records / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text("# Synthetic process fixture\n")
        (records / "specs/REQUIREMENTS.md").write_text("| RQ-TEST-001 | Test | Confirmed |\n")
        (records / "specs/README.md").write_text(
            "| [0000](iterations/0000-workflow-bootstrap/SPEC.md) | 0.0.1 | Fixture | Verified |\n")
        metadata = {
            "Status": "Verified", "Previous version": "New project", "Target version": "0.0.1",
            "Version approval": "Normal patch", "Spec type": "Governance-Process-only",
            "Coordinator": "Fixture", "Created": "2000-01-01", "Base SHA": "a" * 40,
            "Proposal commit": "a" * 40, "Approved by": "Fixture", "Approval basis": "User fixture",
            "Approved revision": "Fixture", "Approval scope": "Fixture", "Claim commit": "a" * 40,
            "Remote claim": "N/A", "Requirements": "N/A", "External effects": "None",
        }
        spec = "# Synthetic contract\n\n## Metadata\n\n| Field | Value |\n| --- | --- |\n"
        spec += "".join(f"| {key} | {value} |\n" for key, value in metadata.items())
        spec += "\n## Acceptance criteria\n" + "".join(
            f"\n### AC-{i:03d} [Must]\nFixture criterion.\n" for i in range(1, 6))
        spec += "\n## Test plan\n" + "".join(
            f"| TEST-{i:03d} | AC-{i:03d} | Fixture |\n" for i in range(1, 6))
        (records / "specs/iterations/0000-workflow-bootstrap/SPEC.md").write_text(spec)
        (destination / "VERSION").write_text("0.0.1\n")
        self.addCleanup(temporary.cleanup)
        return temporary

    def process_root(self, root):
        return root.parent / "private-records"

    def validate_fixture(self, root):
        return verify_workflow.validate(root, process_root=self.process_root(root))

    def fixture_root(self) -> Path:
        temporary = self.fixture()
        return Path(temporary.name) / "repo"

    def test_current_repository_structure_passes(self) -> None:
        self.assertEqual([], verify_workflow.validate(REPO_ROOT))

    def test_public_checkout_and_external_records_both_pass(self):
        root = self.fixture_root()
        self.assertFalse((root / "specs").exists())
        self.assertEqual([], verify_workflow.validate(root))
        self.assertEqual([], self.validate_fixture(root))

    def test_external_record_root_must_exist_outside_git(self):
        root = self.fixture_root()
        for path in (root, root / "absent", self.process_root(root) / "absent", Path("relative")):
            with self.subTest(path=path):
                self.assertTrue(verify_workflow.validate(root, process_root=path))
        link = root.parent / "record-link"
        link.symlink_to(root, target_is_directory=True)
        self.assertTrue(any("outside the repository" in error
                            for error in verify_workflow.validate(root, process_root=link)))

    def test_external_claim_binds_record_hash(self):
        root = self.fixture_root()
        spec = self.process_root(root) / "specs/iterations/0000-workflow-bootstrap/SPEC.md"
        text = spec.read_text().replace("| Field | Value |", "| Field | Value |\n| Record storage | External |")
        text = text.replace("| Proposal commit | " + "a" * 40,
                            "| Proposal commit | External record: " + "b" * 64)
        text = text.replace("| Claim commit | " + "a" * 40,
                            "| Claim commit | External record: " + "b" * 64)
        spec.write_text(text)
        self.assertEqual([], self.validate_fixture(root))
        spec.write_text(text.replace("External record: " + "b" * 64, "External record: invalid"))
        self.assertTrue(any("external record hash" in error for error in self.validate_fixture(root)))

    def test_frozen_original_commit_can_be_absent_but_active_base_must_exist(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            subprocess.run(["git", "init", "-b", "main"], cwd=root, check=True, capture_output=True)
            subprocess.run(["git", "config", "core.hooksPath", ".githooks"], cwd=root, check=True)
            old = verify_workflow.Iteration("1234", root / "SPEC.md", {
                "Base SHA": "a" * 40, "Status": "Verified"}, "")
            self.assertEqual([], verify_workflow.validate_local_git(root, [old], None))
            active = verify_workflow.Iteration("1234", old.path, {
                "Base SHA": "a" * 40, "Status": "In Progress"}, "")
            self.assertTrue(any("Base SHA does not exist" in error
                                for error in verify_workflow.validate_local_git(root, [active], None)))

    def test_external_product_claim_enforces_changed_scope(self):
        root = self.fixture_root()
        subprocess.run(["git", "init", "-b", "main"], cwd=root, check=True, capture_output=True)
        subprocess.run(["git", "config", "core.hooksPath", ".githooks"], cwd=root, check=True)
        subprocess.run(["git", "add", "."], cwd=root, check=True, capture_output=True)
        subprocess.run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                        "commit", "-m", "fixture"], cwd=root, check=True, capture_output=True)
        base = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
        path = root / "scripts/build-release.py"
        path.write_text(path.read_text() + "\n# changed fixture\n")
        self.assertTrue(any("without one active Product claim" in error for error in
                            verify_workflow.validate(root, local=True, changed_since=base)))
        records = self.process_root(root)
        spec = records / "specs/iterations/0000-workflow-bootstrap/SPEC.md"
        text = spec.read_text().replace("| Status | Verified |", "| Status | In Progress |")
        text = text.replace("| Spec type | Governance-Process-only |", "| Spec type | Product |")
        text = text.replace("| Base SHA | " + "a" * 40, "| Base SHA | " + base)
        text = text.replace("| Requirements | N/A |", "| Requirements | RQ-TEST-001 |")
        spec.write_text(text)
        index = records / "specs/README.md"
        index.write_text(index.read_text().replace("Verified", "In Progress"))
        self.assertEqual([], verify_workflow.validate(root, local=True, changed_since=base, process_root=records))
        (records / "specs/REQUIREMENTS.md").write_text("| RQ-TEST-001 | Test | Deferred |\n")
        self.assertTrue(any("not Confirmed" in error for error in
                            verify_workflow.validate(root, local=True, changed_since=base, process_root=records)))

    def test_documentation_change_does_not_require_product_claim(self):
        root = self.fixture_root()
        subprocess.run(["git", "init", "-b", "main"], cwd=root, check=True, capture_output=True)
        subprocess.run(["git", "config", "core.hooksPath", ".githooks"], cwd=root, check=True)
        subprocess.run(["git", "add", "."], cwd=root, check=True, capture_output=True)
        subprocess.run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                        "commit", "-m", "fixture"], cwd=root, check=True, capture_output=True)
        base = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
        path = root / "packaging/README.md"
        path.write_text(path.read_text() + "\nFixture guide update.\n")
        self.assertEqual([], verify_workflow.validate_local_git(root, [], base))

    def test_rejects_two_active_claims(self) -> None:
        root = self.fixture_root()
        source = self.process_root(root) / "specs/iterations/0000-workflow-bootstrap/SPEC.md"
        destination = self.process_root(root) / "specs/iterations/9999-second-claim"
        destination.mkdir()
        text = source.read_text(encoding="utf-8").replace(
            "| Status | Verified |", "| Status | In Progress |", 1
        )
        source.write_text(text, encoding="utf-8")
        text = text.replace("tracking ID 0000", "tracking ID 9999", 1)
        (destination / "SPEC.md").write_text(text, encoding="utf-8")
        errors = self.validate_fixture(root)
        self.assertTrue(any("more than one active claim" in error for error in errors), errors)

    def test_rejects_new_project_version_other_than_0_0_1(self) -> None:
        root = self.fixture_root()
        spec = self.process_root(root) / "specs/iterations/0000-workflow-bootstrap/SPEC.md"
        spec.write_text(
            spec.read_text(encoding="utf-8").replace("| Target version | 0.0.1 |", "| Target version | 1.0.0 |"),
            encoding="utf-8",
        )
        errors = self.validate_fixture(root)
        self.assertTrue(any("new project must start at 0.0.1" in error for error in errors), errors)

    def test_rejects_invalid_version_format(self) -> None:
        root = self.fixture_root()
        (root / "VERSION").write_text("v0.0.1\n", encoding="utf-8")
        errors = self.validate_fixture(root)
        self.assertTrue(any("invalid SemVer in VERSION" in error for error in errors), errors)

    def test_rejects_must_acceptance_without_test(self) -> None:
        root = self.fixture_root()
        spec = self.process_root(root) / "specs/iterations/0000-workflow-bootstrap/SPEC.md"
        spec.write_text(
            spec.read_text(encoding="utf-8").replace(
                "| TEST-005 | AC-005 |", "| TEST-005 | AC-001 |"
            ),
            encoding="utf-8",
        )
        errors = self.validate_fixture(root)
        self.assertTrue(any("Must AC-005 has no Test plan coverage" in error for error in errors), errors)

    def test_rejects_terminal_claim_placeholder(self) -> None:
        root = self.fixture_root()
        spec = self.process_root(root) / "specs/iterations/0000-workflow-bootstrap/SPEC.md"
        text = spec.read_text(encoding="utf-8")
        text = text.replace("| Status | In Progress |", "| Status | Verified |", 1)
        text = re.sub(
            r"^\| Claim commit \|.*$",
            "| Claim commit | Pending push: this claim commit |",
            text,
            count=1,
            flags=re.MULTILINE,
        )
        spec.write_text(text, encoding="utf-8")
        errors = self.validate_fixture(root)
        self.assertTrue(any("cannot retain a claim placeholder" in error for error in errors), errors)

    def test_rejects_broken_relative_link(self) -> None:
        root = self.fixture_root()
        readme = root / "README.md"
        readme.write_text(
            readme.read_text(encoding="utf-8") + "\n[missing](docs/DOES_NOT_EXIST.md)\n",
            encoding="utf-8",
        )
        errors = self.validate_fixture(root)
        self.assertTrue(any("broken relative link" in error for error in errors), errors)

    def test_rejects_likely_secret(self) -> None:
        root = self.fixture_root()
        token = "ghp_" + ("a" * 30)
        (root / "leak.md").write_text(token + "\n", encoding="utf-8")
        errors = self.validate_fixture(root)
        self.assertTrue(any("possible GitHub token" in error for error in errors), errors)

    def test_rejects_unapproved_github_actions_workflow(self) -> None:
        root = self.fixture_root()
        workflow = root / ".github/workflows/verify.yml"
        workflow.parent.mkdir(parents=True, exist_ok=True)
        workflow.write_text("name: unapproved\n", encoding="utf-8")
        errors = self.validate_fixture(root)
        self.assertTrue(any("hosted CI is opt-in" in error for error in errors), errors)

    def release_workflow(self):
        root = self.fixture_root()
        path = root / ".github/workflows/release.yml"
        return root, path, json.loads(path.read_text())

    def test_release_automatic_triggers_are_rejected(self):
        for event in ("push", "pull_request", "schedule", "release", "workflow_run", "repository_dispatch"):
            with self.subTest(event=event):
                root, path, workflow = self.release_workflow()
                workflow["on"][event] = {}
                path.write_text(json.dumps(workflow))
                self.assertTrue(any("only workflow_dispatch" in error for error in verify_workflow.validate_hosted_ci(root)))

    def test_release_duplicate_on_key_is_rejected(self):
        root, path, workflow = self.release_workflow()
        path.write_text(json.dumps(workflow).removesuffix("}") + ', "on": {"push": {}}}')
        self.assertTrue(any("duplicate workflow key" in error for error in verify_workflow.validate_hosted_ci(root)))

    def test_release_activation_and_dependency_chain_are_enforced(self):
        changes = (("preflight", "if", "true"), ("publish", "if", "true"),
                   ("homebrew", "if", "true"), ("build", "needs", []))
        for job, key, value in changes:
            root, path, workflow = self.release_workflow()
            workflow["jobs"][job][key] = value
            path.write_text(json.dumps(workflow))
            self.assertTrue(verify_workflow.validate_hosted_ci(root), (job, key))

    def test_release_publish_defaults_and_action_pins_are_enforced(self):
        root, path, workflow = self.release_workflow()
        workflow["on"]["workflow_dispatch"]["inputs"]["publish"]["default"] = True
        workflow["jobs"]["build"]["steps"][0]["uses"] = "actions/checkout@main"
        path.write_text(json.dumps(workflow))
        errors = verify_workflow.validate_hosted_ci(root)
        self.assertTrue(any("default off" in error for error in errors))
        self.assertTrue(any("pinned" in error for error in errors))

    def test_release_unreviewed_job_and_permissions_are_rejected(self):
        root, path, workflow = self.release_workflow()
        workflow["jobs"]["unreviewed"] = {"runs-on": "ubuntu-24.04", "steps": [{"run": "true"}]}
        workflow["permissions"]["contents"] = "write"
        path.write_text(json.dumps(workflow))
        self.assertTrue(any("reviewed release jobs" in error for error in verify_workflow.validate_hosted_ci(root)))
        self.assertTrue(any("contents: read" in error for error in verify_workflow.validate_hosted_ci(root)))


class HookTests(unittest.TestCase):
    def git_repo(self) -> tempfile.TemporaryDirectory[str]:
        temporary = tempfile.TemporaryDirectory()
        root = Path(temporary.name)
        subprocess.run(["git", "init", "-b", "main"], cwd=root, check=True, capture_output=True)
        (root / "scripts").mkdir()
        (root / ".githooks").mkdir()
        shutil.copy2(REPO_ROOT / "scripts/install-hooks.sh", root / "scripts/install-hooks.sh")
        shutil.copy2(REPO_ROOT / ".githooks/pre-push", root / ".githooks/pre-push")
        self.addCleanup(temporary.cleanup)
        return temporary

    def test_install_hook_is_idempotent(self) -> None:
        temporary = self.git_repo()
        root = Path(temporary.name)
        script = root / "scripts/install-hooks.sh"
        subprocess.run([str(script)], cwd=root, check=True, capture_output=True, text=True)
        subprocess.run([str(script)], cwd=root, check=True, capture_output=True, text=True)
        value = subprocess.run(
            ["git", "config", "--get", "core.hooksPath"],
            cwd=root,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
        self.assertEqual(".githooks", value)

    def test_install_hook_refuses_existing_other_path(self) -> None:
        temporary = self.git_repo()
        root = Path(temporary.name)
        subprocess.run(["git", "config", "core.hooksPath", ".custom-hooks"], cwd=root, check=True)
        result = subprocess.run(
            [str(root / "scripts/install-hooks.sh")],
            cwd=root,
            capture_output=True,
            text=True,
        )
        self.assertNotEqual(0, result.returncode)
        self.assertIn("refusing to replace", result.stderr)

    def test_pre_push_rejects_non_main_branch(self) -> None:
        temporary = self.git_repo()
        root = Path(temporary.name)
        hook = root / ".githooks/pre-push"
        hook.chmod(0o755)
        line = (
            "refs/heads/feature "
            + ("a" * 40)
            + " refs/heads/feature "
            + ("0" * 40)
            + "\n"
        )
        result = subprocess.run(
            [str(hook), "origin", "unused"],
            cwd=root,
            input=line,
            capture_output=True,
            text=True,
        )
        self.assertNotEqual(0, result.returncode)
        self.assertIn("main-only policy", result.stderr)


if __name__ == "__main__":
    unittest.main()
