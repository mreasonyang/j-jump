#!/usr/bin/env python3
"""Update only the verified current Formula in an explicitly enabled tap checkout."""
import argparse
import os
import subprocess
from pathlib import Path

from release import formula, git, verify_bundle, verify_public_assets


def update(dist, tap, repository):
    if os.environ.get("J_JUMP_PUBLISH_ENABLED") != "true":
        raise ValueError("tap publication is deferred; explicit publish activation is required")
    bundle = verify_bundle(dist)
    content = formula(bundle, repository)
    if git(tap, "branch", "--show-current") != "main" or git(tap, "status", "--porcelain", "--untracked-files=all"):
        raise ValueError("tap requires a clean main checkout")
    remote = git(tap, "remote", "get-url", "origin").lower().removesuffix(".git")
    if remote != "https://github.com/mreasonyang/homebrew-taps":
        raise ValueError("refusing a tap other than the approved homebrew-taps repository")
    destination = tap / "j-jump.rb"
    if destination.is_symlink() or (destination.exists() and not destination.is_file()):
        raise ValueError("Formula destination must be a regular file")
    verify_public_assets(bundle, repository)
    # Root placement preserves the existing root flomo.rb discovery behavior.
    destination.write_text(content)
    subprocess.run(["git", "-C", str(tap), "add", "--", "j-jump.rb"], check=True)
    staged = git(tap, "diff", "--cached", "--name-only")
    if not staged:
        print("Homebrew Formula is already current")
        return
    if staged != "j-jump.rb":
        raise ValueError("tap update includes unrelated files")
    subprocess.run(["git", "-C", str(tap), "config", "user.name", "github-actions[bot]"], check=True)
    subprocess.run(["git", "-C", str(tap), "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], check=True)
    subprocess.run(["git", "-C", str(tap), "diff", "--cached", "--check"], check=True)
    subprocess.run(["git", "-C", str(tap), "commit", "-m", "j-jump " + bundle["version"]], check=True)
    subprocess.run(["git", "-C", str(tap), "push", "origin", "HEAD:refs/heads/main"], check=True)
    remote_sha = git(tap, "ls-remote", "origin", "refs/heads/main").split()[0]
    if remote_sha != git(tap, "rev-parse", "HEAD"):
        raise ValueError("tap remote SHA readback mismatch")
    print("Homebrew Formula delivered and read back")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dist", type=Path, required=True)
    parser.add_argument("--tap-worktree", type=Path, required=True)
    parser.add_argument("--release-repository", required=True)
    args = parser.parse_args()
    try:
        update(args.dist, args.tap_worktree, args.release_repository)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"tap update stopped: {error}\n")


if __name__ == "__main__":
    main()
