#!/usr/bin/env python3
"""Opt-in native acceptance of a public release in a disposable Homebrew prefix.

Uses real public Formulae and assets. Does not access provider credentials or
change the existing Homebrew installation. Requires git, curl, Bash, Zsh, Fish,
a C compiler and make (for Homebrew's Formula test dependencies).
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import tempfile
from pathlib import Path


BREW_SHA = "a57af195cf9d7addb48bdb1204c9151313cd0057"  # Homebrew 7.0.8
FORMULA = "mreasonyang/taps/j-jump"
TAP_URL = "https://github.com/mreasonyang/homebrew-taps.git"
RELEASE_URL = "https://github.com/mreasonyang/j-jump/releases/download"
SHA_RE = re.compile(r"[0-9a-f]{40}")
VERSION_RE = re.compile(r"0|[1-9][0-9]*")


def native_target():
    system, machine = platform.system(), platform.machine().lower()
    arch = {"arm64": "aarch64", "aarch64": "aarch64", "x86_64": "x86_64", "amd64": "x86_64"}.get(machine)
    if not arch or system not in {"Darwin", "Linux"}:
        raise ValueError("requires a native supported macOS/Linux 64-bit target")
    if system == "Darwin" and int(platform.mac_ver()[0].split(".")[0]) < 15:
        raise ValueError("requires macOS 15 or later")
    return f"{arch}-{'apple-darwin' if system == 'Darwin' else 'unknown-linux-musl'}"


def isolated_environment(root: Path):
    # An allowlist prevents inherited provider keys, credential backends, Git
    # settings, shell hooks and Homebrew state from entering the acceptance run.
    env = {key: os.environ[key] for key in ("PATH", "LANG", "LC_ALL", "TERM", "TZ") if key in os.environ}
    home, prefix = root / "home", root / "brew"
    env.update(HOME=str(home), XDG_CONFIG_HOME=str(home / ".config"),
               XDG_CACHE_HOME=str(home / ".cache"), XDG_DATA_HOME=str(home / ".local/share"),
               J_JUMP_HOME=str(home / "state"), J_JUMP_OFFLINE="1",
               PATH=str(prefix / "bin") + os.pathsep + env.get("PATH", os.defpath),
               SHELL="/bin/bash", GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
               GIT_TERMINAL_PROMPT="0", HOMEBREW_NO_AUTO_UPDATE="1", HOMEBREW_NO_ANALYTICS="1",
               HOMEBREW_NO_INSTALL_CLEANUP="1", HOMEBREW_NO_ENV_HINTS="1", HOMEBREW_NO_SUDO="1",
               HOMEBREW_CACHE=str(root / "cache"), HOMEBREW_LOGS=str(root / "logs"),
               HOMEBREW_TEMP=str(root / "temp"), NONINTERACTIVE="1", skip_global_compinit="1")
    return env


def profile_environment(env, home: Path):
    # Installing an older version into a newer version's profile would test an
    # unsupported downgrade. The real upgrade starts with its own old profile.
    home.mkdir()
    return dict(env, HOME=str(home), XDG_CONFIG_HOME=str(home / ".config"),
                XDG_CACHE_HOME=str(home / ".cache"), XDG_DATA_HOME=str(home / ".local/share"),
                J_JUMP_HOME=str(home / "state"))


def run(command, env, cwd, capture=False):
    result = subprocess.run([str(item) for item in command], env=env, cwd=cwd,
                            text=True, stdout=subprocess.PIPE if capture else None,
                            stderr=subprocess.PIPE if capture else None, timeout=600)
    if result.returncode:
        raise RuntimeError(f"{command[0]} failed ({result.returncode}): "
                           + ((result.stdout or "") + (result.stderr or "")))
    return result.stdout or ""


def public_manifest(version, target, source, env, root):
    text = run(["curl", "--fail", "--silent", "--show-error", "--location",
                "--proto", "=https", "--proto-redir", "=https", "--retry", "3",
                f"{RELEASE_URL}/v{version}/release-manifest.json"], env, root, capture=True)
    manifest = json.loads(text)
    if manifest.get("schema_version") != 1 or manifest.get("version") != version or manifest.get("tag") != f"v{version}":
        raise ValueError("public release manifest identity mismatch")
    if source and manifest.get("source_sha") != source:
        raise ValueError("public release manifest source mismatch")
    assets = [asset for asset in manifest["assets"] if asset.get("target") == target]
    if len(assets) != 1 or not re.fullmatch(r"[0-9a-f]{64}", assets[0].get("binary_sha256", "")):
        raise ValueError("missing or ambiguous native asset identity")
    return manifest, assets[0]


def snapshot(paths):
    return {str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in paths if path.is_file()}


def require_preserved(before, paths):
    if snapshot(paths) != before:
        raise AssertionError("configuration, visits or unrelated shell content changed")


def verify_install(version, asset, source, prefix, env, root):
    binary = prefix / "bin/jjump"
    for name in ("jjump", "j-jump"):
        if run([prefix / "bin" / name, "--version"], env, root, capture=True).strip() != f"jjump {version}":
            raise AssertionError("installed version or command alias mismatch")
    if hashlib.sha256(binary.read_bytes()).hexdigest() != asset["binary_sha256"]:
        raise AssertionError("installed binary differs from public release")
    package = json.loads((prefix / f"share/j-jump/manifest.json").read_text())
    if package["source_sha"] != source or package["version"] != version:
        raise AssertionError("installed package source identity mismatch")


def shell_navigation(shell, target, env, home, version):
    executable = shutil.which(shell, path=env["PATH"])
    if not executable:
        raise ValueError(f"missing required test shell: {shell}")
    shell_env = dict(env, SHELL=executable, JJ_TARGET=str(target))
    command = 'j --offline alpha; printf "%s\\n" "$PWD" "$JJ_USER_SETTING"; command jjump --version'
    modes = [["--noprofile", "-i", "-c"], ["--login", "-i", "-c"]] if shell == "bash" else [["-i", "-c"]]
    for flags in modes:
        output = run([executable, *flags, command], shell_env, home, capture=True)
        if output.strip().splitlines() != [str(target), "kept", f"jjump {version}"]:
            raise AssertionError(f"{shell} startup, installed version or offline history navigation failed")


def shell_profiles(home):
    return {"bash": home / ".bashrc", "zsh": home / ".zshrc", "fish": home / ".config/fish/config.fish"}


def install_shells(binary, target, env, home, version):
    originals = {}
    for shell, rc in shell_profiles(home).items():
        rc.parent.mkdir(parents=True, exist_ok=True)
        originals[rc] = (b"set -gx JJ_USER_SETTING kept\n" if shell == "fish"
                         else b"export JJ_USER_SETTING=kept\n")
        rc.write_bytes(originals[rc])
        run([binary, "shell", "install", "--shell", shell], env, home)
        installed, timestamp = rc.read_bytes(), rc.stat().st_mtime_ns
        run([binary, "shell", "install", "--shell", shell], env, home)
        if rc.read_bytes() != installed or rc.stat().st_mtime_ns != timestamp:
            raise AssertionError("repeated shell installation is not idempotent")
        shell_navigation(shell, target, env, home, version)
    return originals


def uninstall_shells(binary, originals, env, home):
    for shell, rc in shell_profiles(home).items():
        run([binary, "shell", "uninstall", "--shell", shell], env, home)
        run([binary, "shell", "uninstall", "--shell", shell], env, home)
        if rc.read_bytes() != originals[rc]:
            raise AssertionError("shell uninstall did not restore unrelated startup content")


def lifecycle(args, root):
    target = native_target()
    if args.target != target:
        raise ValueError("requested target does not match this native host")
    env = isolated_environment(root)
    for tool in ("git", "curl", "bash", "zsh", "fish", "cc", "make"):
        if not shutil.which(tool, path=env["PATH"]):
            raise ValueError(f"missing required acceptance dependency: {tool}")
    home, prefix = root / "home", root / "brew"
    for directory in (home, root / "cache", root / "logs", root / "temp"):
        directory.mkdir(parents=True)
    repository = prefix / "Homebrew"
    run(["git", "clone", "--depth", "1", "--branch", "7.0.8", "https://github.com/Homebrew/brew.git", repository], env, root)
    if run(["git", "rev-parse", "HEAD"], env, repository, capture=True).strip() != BREW_SHA:
        raise AssertionError("Homebrew revision mismatch")
    (prefix / "bin").mkdir()
    (prefix / "bin/brew").symlink_to(repository / "bin/brew")
    brew = prefix / "bin/brew"
    reported = Path(run([brew, "--prefix"], env, root, capture=True).strip()).resolve()
    if reported != prefix.resolve():
        raise AssertionError("Homebrew escaped the independent test prefix")
    run([brew, "--version"], env, root)
    run([brew, "tap", "mreasonyang/taps", TAP_URL], env, root)
    tap = Path(run([brew, "--repository", "mreasonyang/taps"], env, root, capture=True).strip())
    if not tap.resolve().is_relative_to(prefix.resolve()):
        raise AssertionError("tap escaped the independent test prefix")
    current_manifest, current_asset = public_manifest(args.version, target, args.source_sha, env, root)
    old_manifest, old_asset = public_manifest(args.upgrade_from, target, None, env, root)
    binary = prefix / "bin/jjump"
    checks = []
    for phase, version, tap_sha, manifest, asset in (
        ("current", args.version, args.tap_sha, current_manifest, current_asset),
        ("upgrade", args.upgrade_from, args.old_tap_sha, old_manifest, old_asset),
    ):
        home = root / f"{phase}-home"
        env = profile_environment(env, home)
        target_dir = home / "alpha space"
        target_dir.mkdir()
        run(["git", "checkout", "--detach", tap_sha], env, tap)
        run([brew, "install", FORMULA], env, root)
        verify_install(version, asset, manifest["source_sha"], prefix, env, root)
        run([binary, "config", "set", "semantic", "off"], env, home)
        run([binary, "record", "--", target_dir], env, target_dir)
        originals = install_shells(binary, target_dir, env, home, version)
        preserved = list((home / "state").rglob("*")) + list(originals) + [home / ".bash_profile"]
        before = snapshot(preserved)
        if phase == "current":
            run([brew, "install", FORMULA], env, root)
            require_preserved(before, preserved)
            run([brew, "reinstall", FORMULA], env, root)
            checks.extend(["install", "repeat-install", "reinstall"])
        else:
            run(["git", "checkout", "--detach", args.tap_sha], env, tap)
            run([brew, "upgrade", FORMULA], env, root)
            checks.append(f"upgrade-{args.upgrade_from}-to-{args.version}")
        require_preserved(before, preserved)
        if phase == "upgrade":
            run([brew, "cleanup", FORMULA], env, root)
            if (prefix / "Cellar/j-jump" / args.upgrade_from).exists():
                raise AssertionError("upgrade cleanup left the old keg")
            require_preserved(before, preserved)
            checks.append("upgrade-cleanup-removes-old-keg")
        verify_install(args.version, current_asset, args.source_sha, prefix, env, root)
        run([brew, "test", FORMULA], env, root)
        for shell in shell_profiles(home):
            shell_navigation(shell, target_dir, env, home, args.version)
        uninstall_shells(binary, originals, env, home)
        before_uninstall = snapshot(preserved)
        run([brew, "uninstall", FORMULA], env, root)
        if binary.exists() or (prefix / "bin/j-jump").exists():
            raise AssertionError("Homebrew uninstall left command links")
        require_preserved(before_uninstall, preserved)
        checks.extend([f"{phase}-brew-test", f"{phase}-bash-zsh-fish-startup-navigation-undo",
                       f"{phase}-uninstall-preserves-state"])
    return {"version": args.version, "source_sha": args.source_sha, "target": target,
            "brew_sha": BREW_SHA, "tap_sha": args.tap_sha, "old_tap_sha": args.old_tap_sha,
            "binary_sha256": current_asset["binary_sha256"], "checks": checks, "result": "passed"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--tap-sha", required=True)
    parser.add_argument("--upgrade-from", default="0.0.35")
    parser.add_argument("--old-tap-sha", default="307f17572881af7cf7cf8cd7445d6f8d9e6d624a")
    parser.add_argument("--report", required=True, type=Path)
    args = parser.parse_args()
    for version in (args.version, args.upgrade_from):
        if len(version.split(".")) != 3 or not all(VERSION_RE.fullmatch(part) for part in version.split(".")):
            parser.error("versions must be plain SemVer")
    if not all(SHA_RE.fullmatch(value) for value in (args.source_sha, args.tap_sha, args.old_tap_sha)):
        parser.error("source/tap revisions must be immutable full commit SHAs")
    # Homebrew's Linux sandbox grants writes to /tmp and /var/tmp. Keep its
    # repository outside those directories so it can protect its own launcher.
    with tempfile.TemporaryDirectory(prefix="jjump-homebrew-", dir=Path.home()) as temporary:
        report = lifecycle(args, Path(temporary).resolve())
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))


if __name__ == "__main__":
    main()
