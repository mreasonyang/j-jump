#!/usr/bin/env python3
"""Validate release identities and generate a binary Homebrew Formula, offline."""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import struct
import subprocess
import tarfile
import urllib.request
from pathlib import Path, PurePosixPath

TARGETS = (
    "aarch64-apple-darwin", "x86_64-apple-darwin",
    "x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl",
)
VERSION = re.compile(r"(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)")
SHA = re.compile(r"[0-9a-f]{40}")
REPOSITORY = re.compile(r"[A-Za-z0-9][A-Za-z0-9-]{0,38}/[A-Za-z0-9][A-Za-z0-9_.-]{0,99}")
BUILD_PREFIXES = (b"/Users/", b"/home/", b"/root/", b"/private/var/folders/", b"/var/folders/", b"/private/tmp/", b"/tmp/")
BUILD_PATH = re.compile(b"|".join(re.escape(prefix) for prefix in BUILD_PREFIXES) + rb"|[A-Za-z]:[\\/]Users[\\/]", re.I)


def check_binary_disclosure(data, build_roots=()):
    """Reject known personal/build prefixes without printing the sensitive path."""
    if BUILD_PATH.search(data) or any(str(root).encode() in data for root in build_roots):
        raise ValueError("build-machine path disclosure; rebuild with scripts/build-release.py")


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def source_identity(root: Path, tag: str | None = None):
    version = (root / "VERSION").read_text().strip()
    if not VERSION.fullmatch(version):
        raise ValueError("VERSION must be a plain three-part version")
    # The current manifest deliberately has one literal package version. Refuse
    # workspace inheritance/duplicate declarations instead of guessing a value.
    package = re.search(r"^\[package\]\s*\n(.*?)(?=^\[|\Z)", (root / "Cargo.toml").read_text(), re.M | re.S)
    declared = re.findall(r'^version\s*=\s*"([^"]+)"\s*(?:#.*)?$', package[1] if package else "", re.M)
    if declared != [version]:
        raise ValueError("VERSION and Cargo.toml disagree")
    sha = git(root, "rev-parse", "HEAD")
    if git(root, "status", "--porcelain", "--untracked-files=all"):
        raise ValueError("release preparation requires a clean checkout, including untracked build configuration")
    if tag is not None:
        if tag != "v" + version:
            raise ValueError("tag must equal v + VERSION")
        if git(root, "rev-parse", f"refs/tags/{tag}^{{commit}}") != sha:
            raise ValueError("release tag does not identify the checked-out source")
    return {"version": version, "source_sha": sha, "tag": "v" + version}


def check_binary(data: bytes, target: str):
    if target not in TARGETS:
        raise ValueError(f"unsupported release target: {target}")
    if target.endswith("apple-darwin"):
        if len(data) < 32 or data[:4] != b"\xcf\xfa\xed\xfe":
            raise ValueError("expected a native 64-bit Mach-O executable")
        cpu, = struct.unpack_from("<I", data, 4)
        expected = 0x100000C if target.startswith("aarch64") else 0x1000007
        if cpu != expected:
            raise ValueError("Mach-O architecture does not match target")
        if struct.unpack_from("<I", data, 12)[0] != 2:
            raise ValueError("Mach-O member is not an executable")
    else:
        if len(data) < 64 or data[:6] != b"\x7fELF\x02\x01":
            raise ValueError("expected a little-endian ELF64 executable")
        machine, = struct.unpack_from("<H", data, 18)
        if machine != (183 if target.startswith("aarch64") else 62):
            raise ValueError("ELF architecture does not match target")
        if struct.unpack_from("<H", data, 16)[0] not in (2, 3):
            raise ValueError("ELF member is not an executable or static PIE")
        offset, = struct.unpack_from("<Q", data, 32)
        size, count = struct.unpack_from("<HH", data, 54)
        if count and (size < 56 or offset + size * count > len(data)):
            raise ValueError("invalid ELF program-header table")
        if any(struct.unpack_from("<I", data, offset + index * size)[0] == 3 for index in range(count)):
            raise ValueError("musl release must not require a dynamic interpreter")


def verify_archive(path: Path):
    archive_sha = hashlib.sha256(path.read_bytes()).hexdigest()
    sidecar = path.with_name(path.name + ".sha256")
    if sidecar.read_text() != f"{archive_sha}  {path.name}\n":
        raise ValueError(f"archive checksum mismatch: {path.name}")
    with tarfile.open(path, "r:gz") as archive:
        members = archive.getmembers()
        names = [item.name for item in members]
        if len(names) != len(set(names)):
            raise ValueError("duplicate archive members")
        stem = path.name.removesuffix(".tar.gz")
        for item in members:
            parts = PurePosixPath(item.name).parts
            if not parts or parts[0] != stem or ".." in parts or "\\" in item.name or str(PurePosixPath(item.name)) != item.name:
                raise ValueError("unsafe archive member")
            if item.issym():
                if item.name != stem + "/j-jump" or item.linkname != "jjump":
                    raise ValueError("unexpected archive symlink")
            elif not (item.isdir() or item.isfile()):
                raise ValueError("unexpected archive member type")
            if item.uid != 0 or item.gid != 0 or item.uname or item.gname:
                raise ValueError("archive ownership must be neutral")
            if set(item.pax_headers) - {"path", "linkpath"}:
                raise ValueError("archive contains host-specific extended metadata")
            if item.isfile():
                if item.size > 100 * 1024 * 1024:
                    raise ValueError("archive member too large")
                check_binary_disclosure(archive.extractfile(item).read())

        def read(name):
            member = archive.getmember(stem + "/" + name)
            if not member.isfile():
                raise ValueError(f"expected regular archive file: {name}")
            if member.size > 100 * 1024 * 1024:
                raise ValueError(f"archive member too large: {name}")
            return archive.extractfile(member).read()

        manifest = json.loads(read("manifest.json"), object_pairs_hook=unique_object)
        version, target, source_sha = (manifest.get(key) for key in ("version", "target", "source_sha"))
        if not isinstance(version, str) or not VERSION.fullmatch(version):
            raise ValueError("invalid archive version")
        if not isinstance(source_sha, str) or not SHA.fullmatch(source_sha):
            raise ValueError("invalid source SHA")
        if manifest.get("schema_version") != 1 or manifest.get("dirty_source") is not False:
            raise ValueError("release archive must have a clean current manifest")
        if manifest.get("signing") != "unsigned":
            raise ValueError("unsupported signing status; signing requires a separate contract")
        if stem != f"j-jump-{version}-{target}":
            raise ValueError("archive name and manifest disagree")
        binary = read("jjump")
        binary_sha = hashlib.sha256(binary).hexdigest()
        if manifest.get("binary_sha256") != binary_sha or read("binary.sha256") != (binary_sha + "\n").encode():
            raise ValueError("binary checksum mismatch")
        check_binary(binary, target)
        if not archive.getmember(stem + "/jjump").mode & 0o111:
            raise ValueError("packaged binary is not executable")
        alias = archive.getmember(stem + "/j-jump")
        if not alias.issym() or alias.linkname != "jjump":
            raise ValueError("missing equivalent management alias")
        if b"MIT License" not in read("LICENSE.md"):
            raise ValueError("project MIT license is absent")
        if not any(name.startswith(stem + "/licenses/") for name in names):
            raise ValueError("dependency notices are absent")
        dependencies = json.loads(read("dependencies.json"), object_pairs_hook=unique_object)
        if dependencies.get("target") != target or dependencies.get("schema_version") != 2:
            raise ValueError("dependency inventory target/schema mismatch")
    return {"version": version, "source_sha": source_sha, "target": target,
            "filename": path.name, "size_bytes": path.stat().st_size, "sha256": archive_sha, "binary_sha256": binary_sha}


def verify_bundle(dist: Path, targets=TARGETS, version=None, source_sha=None):
    targets = tuple(targets)
    if not targets or len(set(targets)) != len(targets) or set(targets) - set(TARGETS):
        raise ValueError("expected a unique supported target set")
    archives = sorted(dist.glob("*.tar.gz"))
    if len(archives) != len(targets):
        raise ValueError("bundle must contain exactly the requested target archives")
    assets = [verify_archive(path) for path in archives]
    if {item["target"] for item in assets} != set(targets):
        raise ValueError("missing or duplicate target archives")
    if len({item["version"] for item in assets}) != 1 or len({item["source_sha"] for item in assets}) != 1:
        raise ValueError("mixed release version or source SHA")
    first = assets[0]
    if (version is not None and first["version"] != version) or (source_sha is not None and first["source_sha"] != source_sha):
        raise ValueError("bundle does not match requested source identity")
    expected_sidecars = {item["filename"] + ".sha256" for item in assets}
    if {path.name for path in dist.glob("*.tar.gz.sha256")} != expected_sidecars:
        raise ValueError("unexpected checksum sidecars")
    return {"schema_version": 1, "version": first["version"], "tag": "v" + first["version"],
            "source_sha": first["source_sha"], "signing": "unsigned", "assets": assets}


def formula(bundle, repository: str):
    if not REPOSITORY.fullmatch(repository):
        raise ValueError("release repository must be OWNER/REPOSITORY")
    assets = {item["target"]: item for item in bundle["assets"]}
    if set(assets) != set(TARGETS):
        raise ValueError("Homebrew Formula requires all four targets")
    lines = ["class JJump < Formula", '  desc "Independent directory navigation with optional Jev suggestions"',
             f'  homepage "https://github.com/{repository}"', '  license "MIT"', ""]
    for os_name, suffix in (("macos", "apple-darwin"), ("linux", "unknown-linux-musl")):
        lines.append(f"  on_{os_name} do")
        if os_name == "macos":
            lines.append('    depends_on macos: :sequoia')
        for arch, triple in (("arm", "aarch64"), ("intel", "x86_64")):
            asset = assets[triple + "-" + suffix]
            lines.extend([f"    on_{arch} do", f'      url "https://github.com/{repository}/releases/download/{bundle["tag"]}/{asset["filename"]}"',
                          f'      sha256 "{asset["sha256"]}"', "    end"])
        lines.extend(["  end", ""])
    lines.extend(['  def install', '    bin.install "jjump"', '    bin.install_symlink "jjump" => "j-jump"',
                  '    pkgshare.install "LICENSE.md", "licenses", "dependencies.json", "manifest.json", "binary.sha256"', '  end', '',
                  '  def caveats', '    <<~EOS', '      Activate J-Jump in your shell startup file:',
                  '        Bash: eval "$(jjump init bash)"', '        Zsh:  eval "$(jjump init zsh)"', '        Fish: jjump init fish | source',
                  '      First j/ji opens setup. Local-only setup needs no API key.', '    EOS', '  end', '',
                  '  test do', '    ENV["J_JUMP_HOME"] = (testpath/"state").to_s',
                  '    ENV.delete("J_JUMP_CONFIG")', '    ENV.delete("TYPESAFE_API_KEY")',
                  '    assert_match "jjump #{version}", shell_output("#{bin}/jjump --version")',
                  '    assert_match "jjump #{version}", shell_output("#{bin}/j-jump --version")',
                  '    system bin/"jjump", "config", "set", "semantic", "off"',
                  '    target = testpath/"alpha space"', '    target.mkpath',
                  '    cd target do', '      system bin/"jjump", "record", "--", target', '    end',
                  '    assert_equal "#{target}\\n", shell_output("#{bin}/jjump --offline query alpha")',
                  '    ENV.prepend_path "PATH", bin',
                  '    system "bash", "--noprofile", "--norc", "-c",',
                  '           \'set -e; eval "$(jjump init bash)"; j --offline alpha; test "$PWD" = "$1"\', "--", target',
                  '  end', 'end', ''])
    return "\n".join(lines)


def verify_public_assets(bundle, repository):
    """Check the exact unauthenticated downloads Homebrew will use."""
    if not REPOSITORY.fullmatch(repository):
        raise ValueError("release repository must be OWNER/REPOSITORY")
    for item in bundle["assets"]:
        url = f"https://github.com/{repository}/releases/download/{bundle['tag']}/{item['filename']}"
        with urllib.request.urlopen(url, timeout=60) as response:
            digest, size = hashlib.sha256(), 0
            while chunk := response.read(1024 * 1024):
                size += len(chunk)
                if size > item["size_bytes"]:
                    raise ValueError("anonymous release download exceeds verified size")
                digest.update(chunk)
        if size != item["size_bytes"] or digest.hexdigest() != item["sha256"]:
            raise ValueError("anonymous release download checksum/size mismatch")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    source = commands.add_parser("check-source")
    source.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    source.add_argument("--tag")
    for command in ("verify", "formula"):
        sub = commands.add_parser(command)
        sub.add_argument("--dist", type=Path, required=True)
        sub.add_argument("--version")
        sub.add_argument("--source-sha")
        if command == "verify":
            sub.add_argument("--targets", nargs="+", default=TARGETS)
            sub.add_argument("--output", type=Path)
        else:
            sub.add_argument("--release-repository", required=True)
            sub.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "check-source":
            print(json.dumps(source_identity(args.root, args.tag), indent=2))
        else:
            bundle = verify_bundle(args.dist, getattr(args, "targets", TARGETS), args.version, args.source_sha)
            content = formula(bundle, args.release_repository) if args.command == "formula" else json.dumps(bundle, indent=2) + "\n"
            if args.output:
                args.output.write_text(content)
                print(args.output)
            else:
                print(content, end="")
    except (ValueError, KeyError, OSError, tarfile.TarError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"release validation failed: {error}\n")


if __name__ == "__main__":
    main()
