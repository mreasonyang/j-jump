#!/usr/bin/env python3
"""Measure installed prompt observer processes with an isolated synthetic visit store."""
import argparse
import hashlib
import json
import os
import pathlib
import platform
import resource
import sqlite3
import statistics
import subprocess
import tempfile
import time


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[int(fraction * (len(ordered) - 1))]


def summary(values):
    return {
        "avg_ms": statistics.mean(values),
        "p50_ms": statistics.median(values),
        "p90_ms": percentile(values, .90),
        "p95_ms": percentile(values, .95),
        "p99_ms": percentile(values, .99),
        "max_ms": max(values),
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=pathlib.Path, required=True)
    parser.add_argument("--samples", type=int, default=1000)
    parser.add_argument("--out", type=pathlib.Path, required=True)
    args = parser.parse_args()
    if args.samples < 100:
        parser.error("at least 100 samples required")
    binary = args.binary.resolve(strict=True)
    results = []
    with tempfile.TemporaryDirectory(prefix="jj-hook-bench-") as temp:
        root = pathlib.Path(temp).resolve()
        home = root / "home"
        home.mkdir()
        target = home / "target"
        target.mkdir()
        env = dict(os.environ, HOME=str(home), J_JUMP_HOME=str(root / "state"), PWD=str(target))
        env.pop("TYPESAFE_API_KEY", None)
        env.pop("J_JUMP_CONFIG", None)
        subprocess.run([binary, "doctor"], cwd=target, env=env, stdout=subprocess.DEVNULL, check=True)
        db = sqlite3.connect(root / "state/data/visits.db")
        for inventory in (513, 10000):
            db.execute("DELETE FROM visits")
            db.execute("INSERT INTO visits(path,key,count,last_seen,weight) VALUES(?,?,1,?,1)", (str(target), "target", int(time.time())))
            for i in range(inventory - 1):
                directory = home / f"sample-{i:05d}"
                directory.mkdir(exist_ok=True)
                db.execute("INSERT INTO visits(path,key,count,last_seen,weight) VALUES(?,?,1,?,1)", (str(directory), directory.name, int(time.time())))
            db.commit()
            starts = []
            completions = []
            cpus = []
            rss = []
            expected_count = 1
            for sample in range(args.samples):
                start = time.perf_counter_ns()
                process = subprocess.Popen([binary, "observe", "--", str(target)], cwd=target, env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
                output = process.stdout.read()
                _, status, usage = os.wait4(process.pid, 0)
                process.returncode = os.waitstatus_to_exitcode(status)
                process.stdout.close()
                foreground_end = time.perf_counter_ns()
                if process.returncode:
                    raise RuntimeError(f"observe failed at sample {sample}: {process.returncode}")
                marker = output.strip()
                if marker and not marker.isdigit():
                    raise RuntimeError("observer stdout was not one PID")
                supervisor_pid = int(marker) if marker else None
                deadline = time.monotonic() + 2
                while time.monotonic() < deadline:
                    count = db.execute("SELECT count FROM visits WHERE path=?", (str(target),)).fetchone()[0]
                    alive = False
                    if supervisor_pid is not None:
                        try:
                            os.kill(supervisor_pid, 0)
                            alive = True
                        except ProcessLookupError:
                            pass
                    if count == expected_count + 1 and not alive:
                        break
                    time.sleep(.001)
                else:
                    raise RuntimeError(f"observer did not finish a single write at sample {sample}; count={count}, supervisor_alive={alive}")
                expected_count += 1
                starts.append((foreground_end - start) / 1e6)
                completions.append((time.perf_counter_ns() - foreground_end) / 1e6)
                cpus.append((usage.ru_utime + usage.ru_stime) * 1000)
                rss.append(usage.ru_maxrss / 1024 if platform.system() == "Darwin" else usage.ru_maxrss)
            results.append({
                "inventory": inventory,
                "samples": args.samples,
                "foreground": summary(starts),
                "completion_after_foreground": summary(completions),
                "foreground_cpu_p95_ms": percentile(cpus, .95),
                "foreground_max_rss_kib": max(rss),
                "final_target_count": expected_count,
            })
        db.close()
    report = {
        "schema_version": 1,
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "host": {"os": platform.system(), "release": platform.release(), "arch": platform.machine()},
        "synthetic": True,
        "cache": "Fresh process per sample, warm OS cache after setup; no page-cache purge",
        "resource_scope": "CPU and RSS describe the foreground launcher only; detached supervisor and worker are excluded",
        "results": results,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
