from __future__ import annotations

import subprocess
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]


def is_ignored(path: str) -> bool:
    result = subprocess.run(
        ["git", "check-ignore", "--no-index", "--quiet", "--", path],
        cwd=REPO_ROOT,
        check=False,
    )
    return result.returncode == 0


class GitignorePolicyTests(unittest.TestCase):
    def test_private_and_generated_paths_are_ignored(self) -> None:
        ignored = (
            "target/debug/jjump",
            "crates/core/target/debug/libcore.rlib",
            "dist/j-jump.tar.gz",
            "coverage/lcov.info",
            ".env",
            ".env.local",
            "developer.key",
            "credentials/typesafe.json",
            ".j-jump/state.sqlite",
            "local-data/evaluation.db",
            "scratch.sqlite3",
            "debug.log",
            "__pycache__/validator.pyc",
            ".idea/workspace.xml",
            ".DS_Store",
            "specs/REQUIREMENTS.md",
            "specs/iterations/_template/SPEC.md",
            ".agents/rules/01-spec-workflow.md",
            "docs/DESIGN.md",
            "docs/PROGRESS.md",
            "docs/evidence/run.json",
            "research/half-life-0031/PROTOCOL.md",
        )
        for path in ignored:
            with self.subTest(path=path):
                self.assertTrue(is_ignored(path), path)

    def test_source_lockfiles_templates_and_fixtures_remain_trackable(self) -> None:
        trackable = (
            "Cargo.toml",
            "Cargo.lock",
            "src/main.rs",
            ".cargo/config.toml",
            ".env.example",
            ".env.test.example",
            "AGENTS.md",
            "docs/CONFIGURATION-AND-HELP.md",
            "docs/RELEASE.md",
            "packaging/README.md",
            "tests/fixtures/navigation.sqlite",
            "tests/fixtures/history.db",
        )
        for path in trackable:
            with self.subTest(path=path):
                self.assertFalse(is_ignored(path), path)


if __name__ == "__main__":
    unittest.main()
