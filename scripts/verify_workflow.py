#!/usr/bin/env python3
"""Mechanical repository checks, with optional external Spec-first records.

This validator checks repository structure and internally consistent evidence.
It cannot prove that a user approved a change, a product works, or hosted CI ran.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


SEMVER_RE = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$")
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
EXTERNAL_RECORD_RE = re.compile(r"^External record: [0-9a-f]{64}$")
ITERATION_DIR_RE = re.compile(r"^(\d{4})-[a-z0-9][a-z0-9-]*$")
REQUIREMENT_ID_RE = re.compile(r"^RQ-[A-Z]+-\d{3}$")
AC_RE = re.compile(r"^### (AC-\d{3}) \[(Must|Should|May)\]", re.MULTILINE)
LINK_RE = re.compile(r"(?<!!)\[[^\]]+\]\(([^)]+)\)")
SECRET_PATTERNS = {
    "private key": re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----"),
    "GitHub token": re.compile(r"\b(?:ghp|github_pat)_[A-Za-z0-9_]{20,}\b"),
    "provider-style secret": re.compile(r"\bsk-[A-Za-z0-9_-]{20,}\b"),
    "TypeSafe credential": re.compile(r"\bapikey_[A-Za-z0-9_./+=-]{24,}\b"),
}

LOCAL_ONLY_PARTS = {".git", "target", "dist", "local-data", ".j-jump", ".cargo", "coverage", "__pycache__", ".pytest_cache"}
USER_PATH_RE = re.compile(r"/(?:Users|home)/([^/\s`\"']+)/")
CREDENTIAL_BACKGROUND_RE = re.compile(
    r'"(?:key_source|credential_source)"\s*:\s*"[^"\n]*(?:prior|previous|recovered|another project)',
    re.IGNORECASE,
)
INTERNAL_DIALOGUE_RE = re.compile(
    r"\bUser[:：]|用户[:：]|依据用户[“「]|用户\d{4}-\d{2}-\d{2}批准|in this (?:conversation|thread)",
    re.IGNORECASE,
)
SYNTHETIC_HOME_NAMES = {"alpha", "beta", "test", "jjtest", "neutral", "one", "old", "active", "committed", "api-project", "jj-target"}

REQUIRED_FILES = (
    "AGENTS.md",
    "VERSION",
    ".githooks/pre-push",
    "scripts/verify-workflow.sh",
    "scripts/install-hooks.sh",
    ".github/workflows/release.yml",
)

PROCESS_FILES = (
    "specs/REQUIREMENTS.md", "specs/TRACEABILITY.md", "specs/GATES.md",
    "specs/ERRATA.md", "specs/README.md", "specs/iterations/_template/SPEC.md",
)
PUBLIC_MARKDOWN = {
    "AGENTS.md", "README.md", "LICENSE.md", "docs/README.md",
    "docs/CONFIGURATION-AND-HELP.md", "docs/RELEASE.md", "packaging/README.md",
    "research/semantic-pilot/README.md", "research/half-life-0031/README.md",
}
PROCESS_PREFIXES = (
    "specs/", ".agents/", "docs/evidence/", "docs/history/",
    "research/adapter-live/", "research/jev-latency/", "research/semantic-benchmark-v2/",
    "research/semantic-binary-v1/", "research/synthetic-live/", "research/zoxide-review/",
)

HOSTED_CI_ROOT = ".github/workflows"
APPROVED_RELEASE_WORKFLOW = ".github/workflows/release.yml"

VALID_CONFIRMATIONS = {"Observed", "Proposed", "Confirmed", "Deferred", "Rejected"}
VALID_STATUSES = {
    "Proposed",
    "Approved",
    "In Progress",
    "Implemented",
    "Verified",
    "Blocked",
    "Deferred",
    "Rejected",
    "Cancelled",
    "Superseded",
}
ACTIVE_STATUSES = {"In Progress", "Implemented"}
TERMINAL_STATUSES = {"Verified", "Rejected", "Cancelled", "Superseded"}
PRODUCT_PATHS = (
    "install.sh",
    ".github/workflows/",
    "scripts/release.py",
    "scripts/package.py",
    "scripts/build-release.py",
    "scripts/ci-release.sh",
    "scripts/publish-release.py",
    "scripts/update-homebrew-tap.py",
    "src/",
    "Cargo.toml",
    "Cargo.lock",
    "build.rs",
    "assets/",
    "packaging/",
)


@dataclass(frozen=True)
class Iteration:
    ident: str
    path: Path
    metadata: dict[str, str]
    text: str

    @property
    def status(self) -> str:
        return self.metadata.get("Status", "")


class ValidationError(Exception):
    pass


def table_cells(line: str) -> list[str]:
    return [cell.strip() for cell in line.strip().strip("|").split("|")]


def parse_metadata(text: str) -> dict[str, str]:
    metadata: dict[str, str] = {}
    in_metadata = False
    for line in text.splitlines():
        if line.strip() == "## Metadata":
            in_metadata = True
            continue
        if in_metadata and line.startswith("## "):
            break
        if not in_metadata or not line.startswith("|"):
            continue
        cells = table_cells(line)
        if len(cells) != 2 or cells[0] in {"Field", "---"}:
            continue
        metadata[cells[0]] = cells[1].strip("`")
    return metadata


def discover_iterations(root: Path) -> list[Iteration]:
    iterations_root = root / "specs/iterations"
    iterations: list[Iteration] = []
    if not iterations_root.is_dir():
        return iterations
    seen: set[str] = set()
    for path in sorted(iterations_root.iterdir()):
        if path.name == "_template":
            continue
        if not path.is_dir() or not ITERATION_DIR_RE.fullmatch(path.name):
            raise ValidationError(f"invalid iteration directory: {path.relative_to(root)}")
        ident = path.name.split("-", 1)[0]
        if ident in seen:
            raise ValidationError(f"duplicate iteration ID: {ident}")
        seen.add(ident)
        spec_path = path / "SPEC.md"
        if not spec_path.is_file():
            raise ValidationError(f"missing iteration Spec: {spec_path.relative_to(root)}")
        text = spec_path.read_text(encoding="utf-8")
        iterations.append(Iteration(ident, spec_path, parse_metadata(text), text))
    return iterations


def parse_requirements(root: Path) -> dict[str, str]:
    path = root / "specs/REQUIREMENTS.md"
    requirements: dict[str, str] = {}
    if not path.is_file():
        return requirements
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.startswith("| RQ-"):
            continue
        cells = table_cells(line)
        if len(cells) < 3:
            raise ValidationError(f"malformed requirement row: {line}")
        ident, confirmation = cells[0], cells[2]
        if not REQUIREMENT_ID_RE.fullmatch(ident):
            raise ValidationError(f"invalid requirement ID: {ident}")
        if ident in requirements:
            raise ValidationError(f"duplicate requirement ID: {ident}")
        if confirmation not in VALID_CONFIRMATIONS:
            raise ValidationError(f"invalid confirmation for {ident}: {confirmation}")
        requirements[ident] = confirmation
    return requirements


def parse_index(root: Path) -> dict[str, tuple[str, str]]:
    path = root / "specs/README.md"
    entries: dict[str, tuple[str, str]] = {}
    row_re = re.compile(
        r"^\| \[(\d{4})\]\([^)]+\) \| ([^|]+) \| [^|]+ \| ([^|]+) \|"
    )
    for line in path.read_text(encoding="utf-8").splitlines():
        match = row_re.match(line)
        if not match:
            continue
        ident, version, status = (value.strip() for value in match.groups())
        if ident in entries:
            raise ValidationError(f"duplicate iteration index entry: {ident}")
        entries[ident] = (version, status)
    return entries


def parse_version(value: str, context: str) -> tuple[int, int, int]:
    match = SEMVER_RE.fullmatch(value)
    if not match:
        raise ValidationError(f"invalid SemVer in {context}: {value}")
    return tuple(int(part) for part in match.groups())  # type: ignore[return-value]


def validate_transition(iteration: Iteration) -> None:
    previous = iteration.metadata.get("Previous version", "")
    target = iteration.metadata.get("Target version", "")
    approval = iteration.metadata.get("Version approval", "")
    target_tuple = parse_version(target, f"iteration {iteration.ident} Target version")
    if previous == "New project":
        if target_tuple != (0, 0, 1):
            raise ValidationError(
                f"iteration {iteration.ident}: new project must start at 0.0.1"
            )
        return
    previous_tuple = parse_version(previous, f"iteration {iteration.ident} Previous version")
    normal_patch = (previous_tuple[0], previous_tuple[1], previous_tuple[2] + 1)
    if target_tuple in {previous_tuple, normal_patch}:
        return
    lower = approval.lower()
    if "user" not in lower or any(word in lower for word in ("none", "pending", "normal patch")):
        raise ValidationError(
            f"iteration {iteration.ident}: non-default version transition requires explicit user approval"
        )


def referenced_tests(text: str) -> set[str]:
    covered: set[str] = set()
    in_plan = False
    for line in text.splitlines():
        if line.strip() == "## Test plan":
            in_plan = True
            continue
        if in_plan and line.startswith("## "):
            break
        if not in_plan or not line.startswith("| TEST-"):
            continue
        cells = table_cells(line)
        if len(cells) >= 2:
            covered.update(re.findall(r"AC-\d{3}", cells[1]))
    return covered


def validate_iteration(iteration: Iteration, requirements: dict[str, str]) -> list[str]:
    errors: list[str] = []
    status = iteration.status
    if status not in VALID_STATUSES:
        errors.append(f"iteration {iteration.ident}: invalid status {status!r}")

    required_metadata = (
        "Status",
        "Previous version",
        "Target version",
        "Version approval",
        "Spec type",
        "Coordinator",
        "Created",
        "Base SHA",
        "Proposal commit",
        "Approved by",
        "Approval basis",
        "Approved revision",
        "Approval scope",
        "Claim commit",
        "Remote claim",
        "Requirements",
        "External effects",
    )
    for field in required_metadata:
        if not iteration.metadata.get(field):
            errors.append(f"iteration {iteration.ident}: missing metadata {field}")

    if not SHA_RE.fullmatch(iteration.metadata.get("Base SHA", "")):
        errors.append(f"iteration {iteration.ident}: Base SHA must be 40 lowercase hex characters")

    if status in {"Approved", "In Progress", "Implemented", "Verified", "Blocked"}:
        basis = iteration.metadata.get("Approval basis", "").lower()
        if "user" not in basis:
            errors.append(f"iteration {iteration.ident}: approved state lacks concrete user approval basis")

    pending_claim = "Pending push" in iteration.metadata.get("Claim commit", "")
    if status in {"Implemented", "Verified"} and pending_claim:
        errors.append(f"iteration {iteration.ident}: {status} cannot retain a claim placeholder")

    if status in {"Implemented", "Verified"}:
        for field in ("Proposal commit", "Claim commit"):
            value = iteration.metadata.get(field, "")
            external = iteration.metadata.get("Record storage") == "External" and EXTERNAL_RECORD_RE.fullmatch(value)
            if not SHA_RE.fullmatch(value) and not external:
                errors.append(f"iteration {iteration.ident}: {field} must bind an exact commit or external record hash at {status}")

    if status in TERMINAL_STATUSES and "Pending" in iteration.text:
        errors.append(f"iteration {iteration.ident}: terminal Spec contains Pending evidence")

    try:
        validate_transition(iteration)
    except ValidationError as exc:
        errors.append(str(exc))

    must_acs = {ident for ident, priority in AC_RE.findall(iteration.text) if priority == "Must"}
    covered = referenced_tests(iteration.text)
    for ident in sorted(must_acs - covered):
        errors.append(f"iteration {iteration.ident}: Must {ident} has no Test plan coverage")

    spec_type = iteration.metadata.get("Spec type", "")
    if not spec_type.startswith("Governance-Process-only"):
        for ident in re.findall(r"RQ-[A-Z]+-\d{3}", iteration.metadata.get("Requirements", "")):
            if ident not in requirements:
                errors.append(f"iteration {iteration.ident}: unknown requirement {ident}")
            elif requirements[ident] != "Confirmed":
                errors.append(
                    f"iteration {iteration.ident}: product requirement {ident} is {requirements[ident]}, not Confirmed"
                )
    return errors


def repository_files(root: Path) -> Iterable[Path]:
    """Inspect tracked content, with a portable fallback for exported fixtures.

    Ignored local profiles and unrelated build configuration are never read.
    New delivery files enter this inventory when staged in Git.
    """
    top = run_git(root, "rev-parse", "--show-toplevel", check=False)
    if top.returncode == 0 and Path(top.stdout.strip()).resolve() == root.resolve():
        names = run_git(root, "ls-files", "-z").stdout.split("\0")
        paths = (root / name for name in names if name)
    else:
        paths = root.rglob("*")
    for path in paths:
        relative = path.relative_to(root)
        if any(part in LOCAL_ONLY_PARTS for part in relative.parts):
            continue
        if path.is_symlink() or not path.is_file():
            continue
        yield path


def markdown_files(root: Path) -> Iterable[Path]:
    return (path for path in repository_files(root) if path.suffix == ".md")


def validate_links(root: Path) -> list[str]:
    errors: list[str] = []
    for path in markdown_files(root):
        text = path.read_text(encoding="utf-8")
        for target in LINK_RE.findall(text):
            target = target.strip().strip("<>")
            if target.startswith(("http://", "https://", "mailto:", "#")):
                continue
            local = target.split("#", 1)[0]
            if not local:
                continue
            resolved = (path.parent / local).resolve()
            try:
                resolved.relative_to(root.resolve())
            except ValueError:
                errors.append(f"{path.relative_to(root)}: link escapes repository: {target}")
                continue
            if not resolved.exists():
                errors.append(f"{path.relative_to(root)}: broken relative link: {target}")
    return errors


def validate_secrets_and_paths(root: Path) -> list[str]:
    errors: list[str] = []
    for path in repository_files(root):
        relative = path.relative_to(root)
        name = relative.as_posix()
        if name.startswith(PROCESS_PREFIXES) or (
            path.suffix.lower() == ".md" and name not in PUBLIC_MARKDOWN
        ):
            errors.append(f"{relative}: internal development artifact is tracked")
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        for label, pattern in SECRET_PATTERNS.items():
            if pattern.search(text):
                errors.append(f"{relative}: possible {label}")
        for match in USER_PATH_RE.finditer(text):
            synthetic = text[match.start():].startswith("/home/") and relative.parts[0] in {"tests", "research"} and (
                match.group(1) in SYNTHETIC_HOME_NAMES or re.fullmatch(r"project-\d+", match.group(1))
            )
            if not synthetic:
                errors.append(f"{relative}: machine-specific absolute user path")
                break
        if CREDENTIAL_BACKGROUND_RE.search(text):
            errors.append(f"{relative}: internal credential provenance")
        if INTERNAL_DIALOGUE_RE.search(text):
            errors.append(f"{relative}: internal conversation background")
        if path.suffix in {".json", ".jsonl"}:
            records = text.splitlines() if path.suffix == ".jsonl" else [text]
            for record in records:
                try:
                    value = json.loads(record)
                except (ValueError, TypeError):
                    continue
                if isinstance(value, dict) and isinstance(value.get("environment"), dict) and {
                    "platform", "python", "machine", "hostname", "kernel"
                }.intersection(value["environment"]):
                    errors.append(f"{relative}: recorded host environment")
                    break
    return errors


def external_process_root(root: Path, explicit: Path | None = None) -> Path | None:
    """Private records live outside both the worktree and its Git directory."""
    configured = explicit or os.environ.get("J_JUMP_PROCESS_ROOT")
    if not configured:
        configured = run_git(root, "config", "--get", "j-jump.processRoot", check=False).stdout.strip()
    if not configured:
        return None
    path = Path(configured).expanduser()
    if not path.is_absolute():
        raise ValidationError("external process root must be absolute")
    path = path.resolve()
    git_dir = run_git(root, "rev-parse", "--absolute-git-dir", check=False)
    forbidden = [root.resolve()]
    if git_dir.returncode == 0:
        forbidden.append(Path(git_dir.stdout.strip()).resolve())
    if any(path == parent or parent in path.parents for parent in forbidden):
        raise ValidationError("process records must be outside the repository and Git directory")
    if not path.is_dir():
        raise ValidationError("configured external process root does not exist")
    return path


def run_git(root: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args],
        cwd=root,
        text=True,
        capture_output=True,
        check=check,
    )


def validate_local_git(root: Path, iterations: list[Iteration], changed_since: str | None) -> list[str]:
    errors: list[str] = []
    inside = run_git(root, "rev-parse", "--is-inside-work-tree", check=False)
    if inside.returncode != 0 or inside.stdout.strip() != "true":
        return ["--local requires a Git worktree"]
    branch = run_git(root, "branch", "--show-current").stdout.strip()
    if branch != "main":
        errors.append(f"main-only policy violation: current branch is {branch or '(detached)'}")
    hooks_path = run_git(root, "config", "--get", "core.hooksPath", check=False).stdout.strip()
    if hooks_path != ".githooks":
        errors.append("core.hooksPath must be .githooks; run ./scripts/install-hooks.sh")
    for iteration in iterations:
        # Frozen records bind the original commits, which may have been removed
        # by an authorized history cleanup. Active work must bind current Git.
        if iteration.status not in ACTIVE_STATUSES:
            continue
        base = iteration.metadata.get("Base SHA", "")
        if SHA_RE.fullmatch(base):
            exists = run_git(root, "cat-file", "-e", f"{base}^{{commit}}", check=False)
            if exists.returncode != 0:
                errors.append(f"iteration {iteration.ident}: Base SHA does not exist locally: {base}")

    if changed_since:
        exists = run_git(root, "cat-file", "-e", f"{changed_since}^{{commit}}", check=False)
        if exists.returncode != 0:
            errors.append(f"changed-since commit is unavailable: {changed_since}")
        else:
            changed = run_git(root, "diff", "--name-only", f"{changed_since}..HEAD").stdout.splitlines()
            changed += run_git(root, "diff", "--name-only").stdout.splitlines()
            changed += run_git(root, "diff", "--cached", "--name-only").stdout.splitlines()
            product_changed = any(
                (name == prefix or name.startswith(prefix)) and not name.endswith(".md")
                for name in changed
                for prefix in PRODUCT_PATHS
            )
            if product_changed:
                active = [item for item in iterations if item.status in ACTIVE_STATUSES]
                if len(active) != 1 or not active[0].metadata.get("Spec type", "").startswith("Product"):
                    errors.append("product/runtime paths changed without one active Product claim")
    diff_check = run_git(root, "diff", "--check", check=False)
    if diff_check.returncode != 0:
        errors.append("git diff --check failed:\n" + diff_check.stdout + diff_check.stderr)
    return errors


def validate_hosted_ci(root: Path) -> list[str]:
    """The existing policy permits manual configuration, never automatic execution.

    JSON is a YAML subset. Parsing the workflow with the standard library avoids
    coercing the 'on' key or overlooking anchors/duplicate event declarations.
    """
    directory = root / HOSTED_CI_ROOT
    if directory.is_symlink():
        return ["hosted CI workflow directory symlinks are prohibited"]
    if not directory.exists():
        return []
    errors: list[str] = []
    for path in directory.rglob("*"):
        if path.is_symlink():
            errors.append("hosted CI workflow symlinks are prohibited")
            continue
        if not path.is_file():
            continue
        if path.relative_to(root).as_posix() != APPROVED_RELEASE_WORKFLOW:
            errors.append(f"hosted CI is opt-in; unapproved workflow: {path.relative_to(root)}")
            continue
        def unique(pairs):
            result = {}
            for key, value in pairs:
                if key in result:
                    raise ValueError(f"duplicate workflow key: {key}")
                result[key] = value
            return result
        try:
            workflow = json.loads(path.read_text(), object_pairs_hook=unique)
            events = workflow.get("on")
            if not isinstance(events, dict) or set(events) != {"workflow_dispatch"}:
                errors.append("hosted execution is deferred; only workflow_dispatch is permitted")
            if workflow.get("permissions") != {"contents": "read"}:
                errors.append("release workflow must default to contents: read")
            jobs = workflow.get("jobs", {})
            expected_needs = {"preflight": None, "build": "preflight", "bundle": "build", "publish": "bundle", "homebrew": "publish"}
            if set(jobs) != set(expected_needs):
                errors.append("only the reviewed release jobs are permitted")
            for name, needs in expected_needs.items():
                if jobs.get(name, {}).get("needs") != needs:
                    errors.append(f"{name} must follow the guarded release dependency chain")
                permissions = jobs.get(name, {}).get("permissions", {"contents": "read"})
                if permissions != {"contents": "write" if name == "publish" else "read"}:
                    errors.append(f"unexpected permissions in release job {name}")
            if jobs.get("preflight", {}).get("if") != "${{ vars.J_JUMP_ACTIONS_ENABLED == 'true' }}":
                errors.append("release preflight requires the dormant Actions activation guard")
            for name, guard in (
                ("publish", "${{ inputs.publish && vars.J_JUMP_PUBLISH_ENABLED == 'true' }}"),
                ("homebrew", "${{ inputs.update_homebrew && vars.J_JUMP_PUBLISH_ENABLED == 'true' }}"),
            ):
                if jobs.get(name, {}).get("if") != guard:
                    errors.append(f"{name} requires explicit publication activation")
            dispatch = events.get("workflow_dispatch", {}) if isinstance(events, dict) else {}
            inputs = dispatch.get("inputs", {}) if isinstance(dispatch, dict) else {}
            for name in ("publish", "update_homebrew"):
                if inputs.get(name, {}).get("default") is not False:
                    errors.append(f"{name} must default off")
            for job in jobs.values():
                for step in job.get("steps", []):
                    if "uses" in step and not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+@[0-9a-f]{40}", step["uses"]):
                        errors.append("release actions must be pinned to exact commit SHAs")
        except (OSError, ValueError, AttributeError, TypeError) as exc:
            errors.append(f"cannot inspect manual-only release workflow: {exc}")
    return errors


def validate_process_records(process_root: Path, version: str) -> tuple[list[str], list[Iteration]]:
    errors: list[str] = []
    for relative in PROCESS_FILES:
        if not (process_root / relative).is_file():
            errors.append(f"missing external process file: {relative}")

    try:
        requirements = parse_requirements(process_root)
    except ValidationError as exc:
        errors.append(str(exc))
        requirements = {}
    if not requirements:
        errors.append("no requirements found")

    try:
        iterations = discover_iterations(process_root)
    except ValidationError as exc:
        errors.append(str(exc))
        iterations = []
    if not iterations:
        errors.append("no numbered iterations found")

    active = [item for item in iterations if item.status in ACTIVE_STATUSES]
    if len(active) > 1:
        errors.append("more than one active claim: " + ", ".join(item.ident for item in active))

    for iteration in iterations:
        errors.extend(validate_iteration(iteration, requirements))

    try:
        index = parse_index(process_root)
    except (OSError, ValidationError) as exc:
        errors.append(f"cannot parse iteration index: {exc}")
        index = {}
    for iteration in iterations:
        if iteration.ident not in index:
            errors.append(f"iteration {iteration.ident} missing from specs/README.md")
            continue
        indexed_version, indexed_status = index[iteration.ident]
        if indexed_status != iteration.status:
            errors.append(
                f"iteration {iteration.ident}: index status {indexed_status!r} != Spec status {iteration.status!r}"
            )
        if indexed_version != iteration.metadata.get("Target version", ""):
            errors.append(f"iteration {iteration.ident}: index version does not match Target version")
    for ident in sorted(set(index) - {item.ident for item in iterations}):
        errors.append(f"index references missing iteration: {ident}")

    if version and iterations:
        current_targets = {
            item.metadata.get("Target version", "")
            for item in iterations
            if item.status not in {"Rejected", "Cancelled", "Superseded"}
        }
        if version not in current_targets:
            errors.append(f"VERSION {version} is not represented by a current iteration Target version")

    return errors, iterations


def validate(root: Path, local: bool = False, changed_since: str | None = None,
             process_root: Path | None = None) -> list[str]:
    errors: list[str] = []
    for relative in REQUIRED_FILES:
        if not (root / relative).is_file():
            errors.append(f"missing required file: {relative}")
    errors.extend(validate_hosted_ci(root))
    version_path = root / "VERSION"
    version = version_path.read_text(encoding="utf-8").strip() if version_path.is_file() else ""
    if version:
        try:
            parse_version(version, "VERSION")
        except ValidationError as exc:
            errors.append(str(exc))
    iterations: list[Iteration] = []
    try:
        records = external_process_root(root, process_root)
        if records:
            process_errors, iterations = validate_process_records(records, version)
            errors.extend(process_errors)
    except ValidationError as exc:
        errors.append(str(exc))
    errors.extend(validate_links(root))
    errors.extend(validate_secrets_and_paths(root))
    if local:
        errors.extend(validate_local_git(root, iterations, changed_since))
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--local", action="store_true", help="also validate current Git branch, SHAs, hook, and diff")
    parser.add_argument("--changed-since", help="check changed-scope policy from this commit through HEAD/worktree")
    parser.add_argument("--process-root", type=Path, help="private records directory outside the repository")
    args = parser.parse_args()

    root = args.root.resolve()
    errors = validate(root, local=args.local, changed_since=args.changed_since, process_root=args.process_root)
    if errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        print(f"workflow verification failed with {len(errors)} error(s)", file=sys.stderr)
        return 1
    print(f"workflow verification passed: {root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
