#!/usr/bin/env python3
"""Build a release binary with portable Rust and native source paths."""
import argparse
import os
from pathlib import Path
import shlex
import subprocess

from release import TARGETS, check_binary_disclosure


def build_environment(root, environment):
    env = dict(environment)
    home = Path(env.get("HOME", str(Path.home())))
    locations = [
        (home, "/build/home"),
        (Path(env.get("RUSTUP_HOME", str(home / ".rustup"))), "/build/rustup"),
        (Path(env.get("CARGO_HOME", str(home / ".cargo"))), "/build/cargo"),
    ]
    if env.get("CARGO_TARGET_DIR"):
        locations.append((Path(env["CARGO_TARGET_DIR"]), "/build/target"))
    locations.append((Path(root), "/build/j-jump"))
    mappings = []
    for source, destination in locations:
        if not source.is_absolute():
            source = Path(root) / source
        for variant in (source.absolute(), source.resolve()):
            pair = (str(variant), destination)
            if variant == Path(variant.anchor):
                raise ValueError("cannot remap a filesystem root")
            if pair not in mappings:
                mappings.append(pair)
    mappings.sort(key=lambda pair: len(pair[0]))

    # Last matching Rust mapping wins, so the most specific roots come last.
    # Encoded arguments retain spaces without a shell or changes to caller flags.
    if "CARGO_ENCODED_RUSTFLAGS" in env:
        flags = env["CARGO_ENCODED_RUSTFLAGS"].split("\x1f") if env["CARGO_ENCODED_RUSTFLAGS"] else []
    else:
        flags = env.get("RUSTFLAGS", "").split()
    flags.extend(f"--remap-path-prefix={source}={destination}" for source, destination in mappings)
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)

    # cc-rs must also remap __FILE__ and native debug paths. Preserve each existing
    # global/host/target flag list using its current parsing rule before encoding.
    escaped = env.get("CC_SHELL_ESCAPED_FLAGS", "").lower() in {"1", "true"}
    for key in set(env) | {"CFLAGS", "CXXFLAGS"}:
        if key == "CFLAGS" or key == "CXXFLAGS" or key.startswith(("CFLAGS_", "CXXFLAGS_")) or key in {
            "HOST_CFLAGS", "TARGET_CFLAGS", "HOST_CXXFLAGS", "TARGET_CXXFLAGS"
        }:
            values = shlex.split(env.get(key, "")) if escaped else env.get(key, "").split()
            if key in {"CFLAGS", "CXXFLAGS"}:
                values.extend(f"-ffile-prefix-map={source}={destination}" for source, destination in mappings)
            env[key] = shlex.join(values)
    env["CC_SHELL_ESCAPED_FLAGS"] = "1"
    return env, [source for source, _ in mappings]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=TARGETS, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    environment, roots = build_environment(root, os.environ)
    subprocess.run(["cargo", "build", "--locked", "--release", "--target", args.target],
                   cwd=root, env=environment, check=True)
    target_directory = Path(environment.get("CARGO_TARGET_DIR", "target"))
    if not target_directory.is_absolute():
        target_directory = root / target_directory
    check_binary_disclosure((target_directory / args.target / "release/jjump").read_bytes(), roots)


if __name__ == "__main__":
    main()
