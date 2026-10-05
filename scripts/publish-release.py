#!/usr/bin/env python3
"""Publish a verified bundle only when explicitly invoked with an enabled gate."""
import argparse
import json
import os
import subprocess
import tempfile
from pathlib import Path

from release import REPOSITORY, source_identity, verify_bundle, verify_public_assets


def publish(root, dist, repository, tag):
    if os.environ.get("J_JUMP_PUBLISH_ENABLED") != "true":
        raise ValueError("publication is deferred; explicit publish activation is required")
    if not REPOSITORY.fullmatch(repository):
        raise ValueError("release repository must be OWNER/REPOSITORY")
    identity = source_identity(root, tag)
    bundle = verify_bundle(dist, version=identity["version"], source_sha=identity["source_sha"])
    metadata = json.loads(subprocess.check_output(["gh", "api", f"repos/{repository}"], text=True))
    if metadata.get("private") is not False:
        raise ValueError("Homebrew publication needs a public release repository; private download URLs are refused")
    existing = subprocess.run(["gh", "release", "view", tag, "--repo", repository], capture_output=True)
    if existing.returncode == 0:
        raise ValueError("release already exists; existing assets are never overwritten")
    manifest = dist / "release-manifest.json"
    manifest.write_text(json.dumps(bundle, indent=2) + "\n")
    assets = [dist / item["filename"] for item in bundle["assets"]]
    files = [str(path) for path in assets] + [str(path) + ".sha256" for path in assets] + [str(manifest)]
    with tempfile.TemporaryDirectory() as temporary:
        notes = Path(temporary) / "notes.md"
        notes.write_text(f"J-Jump {bundle['version']}\n\nSource SHA: `{bundle['source_sha']}`\n\n"
                         "Unsigned archives. Verify SHA256 before installation.\n"
                         "Shell initialization is manual. Platform results are in the corresponding workflow run.\n")
        subprocess.run(["gh", "release", "create", tag, "--repo", repository, "--draft",
                        "--title", "J-Jump " + bundle["version"], "--notes-file", str(notes), *files], check=True)
        download = Path(temporary) / "download"
        subprocess.run(["gh", "release", "download", tag, "--repo", repository, "--dir", str(download)], check=True)
        verify_bundle(download, version=identity["version"], source_sha=identity["source_sha"])
        if (download / manifest.name).read_bytes() != manifest.read_bytes():
            raise ValueError("uploaded release manifest differs from the verified bundle")
        subprocess.run(["gh", "release", "edit", tag, "--repo", repository, "--draft=false"], check=True)
    # Anonymous downloads model Homebrew access. A failure stops before tap update.
    verify_public_assets(bundle, repository)
    print(f"https://github.com/{repository}/releases/tag/{tag}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--dist", type=Path, required=True)
    parser.add_argument("--release-repository", required=True)
    parser.add_argument("--tag", required=True)
    args = parser.parse_args()
    try:
        publish(args.root, args.dist, args.release_repository, args.tag)
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"release publication stopped: {error}\n")


if __name__ == "__main__":
    main()
