#!/usr/bin/env python3
"""Compare H7 study replay with linked production Rust, without reading a DB.

The default verifies every calibration trace. --smoke verifies handwritten
mathematical edges only and does not import or generate calibration/holdout data.
Canonical parity evidence and environment/artifact bindings are reported apart.
"""
import argparse
import copy
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
CAP = 2147483647
HALF_LIFE = 604800.0
I64_MAX = 9223372036854775807
ABS_TOL = 5e-12
REL_TOL = 2e-12


def canonical_bytes(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=False, allow_nan=False).encode("utf-8")


def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def runtime_inputs():
    paths = [ROOT / name for name in ("Cargo.toml", "Cargo.lock", "VERSION")]
    paths.extend(sorted((ROOT / "src").rglob("*.rs")))
    paths.extend(sorted(path for path in (ROOT / "shell").iterdir() if path.is_file()))
    return {str(path.relative_to(ROOT)): sha256(path) for path in paths}


def run(command, env=None):
    result = subprocess.run(command, cwd=str(ROOT), env=env, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, universal_newlines=True)
    if result.returncode:
        raise RuntimeError("command failed: {}\n{}\n{}".format(
            command, result.stdout[-4000:], result.stderr[-4000:]))
    return result


def build_bridge(build_dir, baseline):
    build_dir.mkdir(parents=True, exist_ok=True)
    before = runtime_inputs()
    run(["git", "diff", "--quiet", baseline, "--"] + sorted(before))
    environment = dict(os.environ, RUSTC_WRAPPER="")
    cargo_command = ["cargo", "build", "--lib", "--offline", "--locked",
                     "--message-format=json"]
    build = run(cargo_command, env=environment)
    (build_dir / "cargo-artifacts.jsonl").write_text(build.stdout, encoding="utf-8")
    (build_dir / "cargo-stderr.txt").write_text(build.stderr, encoding="utf-8")
    libraries = {}
    native_paths = set()
    for line in build.stdout.splitlines():
        item = json.loads(line)
        if item.get("reason") == "compiler-artifact":
            name = item.get("target", {}).get("name")
            if name in ("j_jump", "serde_json"):
                matches = [Path(value).resolve() for value in item["filenames"]
                           if value.endswith(".rlib")]
                if len(matches) == 1:
                    if name in libraries and libraries[name] != matches[0]:
                        raise RuntimeError("ambiguous linked artifact: " + name)
                    libraries[name] = matches[0]
        elif item.get("reason") == "build-script-executed":
            native_paths.update(item.get("linked_paths", []))
    if set(libraries) != {"j_jump", "serde_json"}:
        raise RuntimeError("cargo did not identify both exact Rust libraries")
    executable = build_dir / "production_bridge"
    rustc_command = ["rustc", "--edition=2024", "--crate-name", "half_life_production_bridge",
                     str(HERE / "production_bridge.rs"), "-o", str(executable)]
    for parent in sorted({path.parent for path in libraries.values()}):
        rustc_command.extend(["-L", "dependency=" + str(parent)])
    for name, path in sorted(libraries.items()):
        rustc_command.extend(["--extern", "{}={}".format(name, path)])
    for path in sorted(native_paths):
        rustc_command.extend(["-L", path])
    compile_result = run(rustc_command, env=environment)
    (build_dir / "rustc-stderr.txt").write_text(compile_result.stderr, encoding="utf-8")
    if before != runtime_inputs():
        raise RuntimeError("runtime source changed while building the bridge")
    binding = {
        "cargo_command": cargo_command,
        "rustc_command": rustc_command,
        "rustc": run(["rustc", "--version"]).stdout.strip(),
        "cargo": run(["cargo", "--version"]).stdout.strip(),
        "libraries": {name: {"path": str(path), "sha256": sha256(path)}
                      for name, path in sorted(libraries.items())},
        "executable": {"path": str(executable), "sha256": sha256(executable)},
        "cargo_artifacts_sha256": sha256(build_dir / "cargo-artifacts.jsonl"),
    }
    return executable, before, binding


def age(now, last):
    return min(max(now - last, 0), I64_MAX)


class EdgeReference:
    """Independent scalar reference for injected weighted and numerical cases."""
    def __init__(self, paths, initial):
        self.paths = paths
        self.state = {
            row["id"]: {"count": row["count"], "last_seen": row["last_seen"],
                        "weight": row["weight"]}
            for row in initial
        }

    def rank(self, now):
        def key(target):
            row = self.state[target]
            weight = row["weight"]
            score = (-math.inf if weight == 0 else math.log2(weight))
            score -= age(now, row["last_seen"]) / HALF_LIFE
            return (-score, -row["last_seen"], self.paths[target].encode("utf-8"))
        return sorted(self.state, key=key)

    def record(self, target, now):
        old = self.state.get(target)
        if old is None:
            self.state[target] = {"count": 1, "last_seen": now, "weight": 1.0}
        else:
            weight = old["weight"]
            self.state[target] = {
                "count": min(old["count"] + 1, CAP),
                "last_seen": max(old["last_seen"], now),
                "weight": min(weight * 2.0 ** (-age(now, old["last_seen"]) / HALF_LIFE)
                              + 1.0, float(CAP)),
            }

    def snapshot(self):
        return copy.deepcopy(self.state)


def edge_traces():
    paths = {"d{}".format(index): "/study/d{}/api".format(index) for index in range(4)}
    base = 1800000000

    def row(target, count, last_seen, weight):
        result = {"id": target, "path": paths[target], "count": count,
                  "last_seen": last_seen}
        result["weight"] = weight
        return result

    cases = [
        ("constant-clock-path-ties", [],
         [(base, "d3"), (base, "d0"), (base, "d1"), (base, "d2"), (base, "d2")]),
        ("half-life-and-idle-rank", [row("d0", 8, base, 8.0), row("d1", 3, base, 3.0)],
         [(base, None), (base + 604800, None), (base + 10 * 604800, None),
          (base + 604800, "d0")]),
        ("weighted-first-and-second-revisit", [row("d0", 1000, base - 180 * 86400, 1000.0)],
         [(base, "d0"), (base, "d0"), (base + 604800, "d0")]),
        ("rollback-does-not-move-anchor", [row("d0", 20, base + 604800, 8.0)],
         [(base, "d0"), (base + 1, "d0"), (base + 604800, "d0"),
          (base + 2 * 604800, "d0")]),
        ("independent-saturation", [row("d0", CAP, base, float(CAP)),
                                    row("d1", CAP, base, 1.0)],
         [(base, "d0"), (base, "d1"), (base + 604800, "d0")]),
        ("zero-and-ancient-log-order", [row("d0", 1000, 0, 0.0),
                                       row("d1", 1000000, 0, 0.25),
                                       row("d2", 8, 0, 8.0)],
         [(base, None), (I64_MAX, None), (I64_MAX, "d0")]),
        ("i64-saturating-age", [row("d0", 8, -I64_MAX - 1, 8.0),
                                row("d1", 1, 0, 1.0)],
         [(I64_MAX, None), (I64_MAX, "d0")]),
    ]
    for name, initial, events in cases:
        yield {"id": "edge/" + name, "paths": paths, "initial": initial,
               "events": [{"now": now, "target": target} for now, target in events]}


class Verifier:
    def __init__(self, executable):
        self.process = subprocess.Popen([str(executable)], stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                        universal_newlines=True, encoding="utf-8")
        self.events = 0
        self.weight_checks = 0
        self.rank_checks = 0
        self.max_abs_error = 0.0
        self.max_relative_error = 0.0
        self.input_digest = hashlib.sha256()

    def response(self):
        line = self.process.stdout.readline()
        if not line:
            error = self.process.stderr.read()
            raise AssertionError("production bridge ended unexpectedly: " + error)
        return json.loads(line)

    def check_row(self, actual, expected, target, paths):
        assert actual["id"] == target
        assert actual["path"] == paths[target]
        for field in ("count", "last_seen"):
            assert actual[field] == expected[field], (target, field, actual, expected)
        actual_weight = actual["weight"]
        expected_weight = expected["weight"]
        assert actual_weight is not None and math.isfinite(actual_weight)
        error = abs(actual_weight - expected_weight)
        relative = error / max(abs(expected_weight), 1e-300)
        self.max_abs_error = max(self.max_abs_error, error)
        self.max_relative_error = max(self.max_relative_error, relative)
        self.weight_checks += 1
        assert math.isclose(actual_weight, expected_weight, rel_tol=REL_TOL, abs_tol=ABS_TOL), (
            target, actual_weight, expected_weight)

    def trace(self, trace, reference):
        encoded = canonical_bytes(trace)
        self.input_digest.update(len(encoded).to_bytes(8, "big"))
        self.input_digest.update(encoded)
        self.process.stdin.write(encoded.decode("utf-8") + "\n")
        self.process.stdin.flush()
        for index, event in enumerate(trace["events"]):
            actual = self.response()
            assert actual["kind"] == "event" and actual["index"] == index
            now = event["now"]
            assert actual["before"] == reference.rank(now), (trace["id"], index, "before")
            target = event.get("target")
            if target is None:
                assert actual["updated"] is None
            else:
                reference.record(target, now)
                self.check_row(actual["updated"], reference.snapshot()[target], target, trace["paths"])
            assert actual["after"] == reference.rank(now), (trace["id"], index, "after")
            self.events += 1
            self.rank_checks += 2
        final = self.response()
        assert final["kind"] == "done" and final["id"] == trace["id"]
        assert final["events"] == len(trace["events"])
        expected = reference.snapshot()
        assert [row["id"] for row in final["state"]] == sorted(expected)
        for row in final["state"]:
            self.check_row(row, expected[row["id"]], row["id"], trace["paths"])

    def close(self):
        self.process.stdin.close()
        try:
            extra = self.process.stdout.read()
            error = self.process.stderr.read()
            code = self.process.wait(timeout=10)
        finally:
            self.process.stdout.close()
            self.process.stderr.close()
        if code or extra:
            raise AssertionError("bridge termination: {} {!r} {!r}".format(code, extra, error))

    def abort(self):
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait(timeout=10)
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            if not stream.closed:
                try:
                    stream.close()
                except BrokenPipeError:
                    pass


def load_evaluator():
    spec = importlib.util.spec_from_file_location("half_life_evaluator", HERE / "evaluate.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--build-dir", type=Path, required=True)
    parser.add_argument("--runtime-ref", default="HEAD", help="exact current implementation ref to verify; worktree must match")
    parser.add_argument("--smoke", action="store_true", help="handwritten edges only; no generated traces")
    args = parser.parse_args(argv)
    if sys.flags.optimize:
        parser.error("verification requires assertions; run Python without -O")
    if args.out.exists() or args.out.is_symlink():
        parser.error("report already exists; choose a new --out path")
    protocol = json.loads((HERE / "protocol.json").read_text(encoding="utf-8"))
    code_paths = [HERE / name for name in ("production_bridge.rs", "verify_production.py", "protocol.json")]
    if not args.smoke:
        code_paths.append(HERE / "evaluate.py")
    research_sources = {path.name: sha256(path) for path in code_paths}
    runtime_ref = run(["git", "rev-parse", args.runtime_ref]).stdout.strip()
    executable, sources, binding = build_bridge(args.build_dir.resolve(), runtime_ref)
    binding["implementation_sha"] = runtime_ref
    verifier = Verifier(executable)
    edge_count = 0
    calibration_count = 0
    calibration_events = 0
    try:
        for trace in edge_traces():
            verifier.trace(trace, EdgeReference(trace["paths"], trace["initial"]))
            edge_count += 1
        if not args.smoke:
            evaluate = load_evaluator()
            for trace in evaluate.generate_traces("calibration", protocol):
                verifier.trace(trace, evaluate.Ranker(7))
                calibration_count += 1
                calibration_events += len(trace["events"])
            split = protocol["splits"]["calibration"]
            expected_count = (sum(len(family["calibration"]) for family in protocol["families"].values())
                              * len(split["rates_per_active_day"]) * len(split["seeds"]))
            assert calibration_count == expected_count, (calibration_count, expected_count)
        verifier.close()
    except BaseException:
        verifier.abort()
        raise
    if sources != runtime_inputs():
        raise RuntimeError("runtime source changed during parity verification")
    if research_sources != {path.name: sha256(path) for path in code_paths}:
        raise RuntimeError("research source changed during parity verification")
    canonical = {
        "passed": True,
        "mode": "edges-only" if args.smoke else "edges-and-all-calibration",
        "runtime_baseline": protocol["runtime_baseline"],
        "runtime_source_sha256": sources,
        "research_source_sha256": research_sources,
        "replayed_input_sha256": verifier.input_digest.hexdigest(),
        "edge_traces": edge_count,
        "calibration_traces": calibration_count,
        "calibration_events": calibration_events,
        "total_events": verifier.events,
        "rank_checks": verifier.rank_checks,
        "weight_checks": verifier.weight_checks,
        "absolute_weight_tolerance": ABS_TOL,
        "relative_weight_tolerance": REL_TOL,
        "holdout_generated": False,
        "scope": "production engine functions; harness mirrors recorder transition; no SQLite or shell acceptance",
    }
    report = {
        "schema_version": 1,
        "study": "half-life-0031",
        "canonical": canonical,
        "environment": {
            "python": sys.version,
            "platform": platform.platform(),
            "git_head": run(["git", "rev-parse", "HEAD"]).stdout.strip(),
            "binding": binding,
            "observed_max_absolute_weight_error": verifier.max_abs_error,
            "observed_max_relative_weight_error": verifier.max_relative_error,
        },
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("x", encoding="utf-8") as output:
        output.write(json.dumps(report, sort_keys=True, indent=2, allow_nan=False) + "\n")
    print(json.dumps({name: canonical[name] for name in (
        "passed", "mode", "edge_traces", "calibration_traces", "total_events", "rank_checks", "weight_checks")},
                     sort_keys=True))


if __name__ == "__main__":
    main()
