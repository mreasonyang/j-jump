#!/usr/bin/env python3
"""Paired synthetic process benchmark; no user state, network, or page-cache purge."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import platform
import random
import sqlite3
import statistics
import subprocess
import tempfile
import time


def invoke(binary, env, cwd, args):
    started = time.perf_counter_ns()
    child = subprocess.Popen([str(binary), *args], env=env, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    stdout, stderr = child.communicate()
    return child.returncode, stdout, stderr, (time.perf_counter_ns() - started) / 1e6


def percentile(values, p):
    return sorted(values)[max(0, math.ceil(len(values) * p) - 1)]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--baseline', required=True, type=Path)
    ap.add_argument('--candidate', required=True, type=Path)
    ap.add_argument('--samples', type=int, default=100)
    ap.add_argument('--out', required=True, type=Path)
    a = ap.parse_args()
    if a.samples < 2:
        ap.error('--samples must be >= 2')
    binaries = {label: path.resolve(strict=True) for label, path in [('baseline', a.baseline), ('candidate', a.candidate)]}
    meta = {label: {'path': str(path), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest(), 'version': subprocess.check_output([str(path), '--version'], text=True).strip()} for label, path in binaries.items()}
    cases = []
    with tempfile.TemporaryDirectory(prefix='jjump-0030-paired-') as tmp:
        root = Path(tmp).resolve()
        # Every fixture shares the same fixed timestamps between binaries; each
        # binary owns a separate DB so fixtures cannot alter each other.
        fixed_now = int(time.time())
        for size in [100, 1000, 10000]:
            for alphabet, stem in [('ascii', 'project'), ('unicode', '项目')]:
                home = root / f'{alphabet}-{size}' / 'home'
                cwd = home / 'neutral'
                cwd.mkdir(parents=True)
                rows = []
                for i in range(size):
                    path = home / f'{stem}-{i:05d}'
                    path.mkdir()
                    rows.append((str(path), str(path).lower(), 100, fixed_now, 100.0))
                envs = {}
                for label, binary in binaries.items():
                    state = home.parent / label
                    env = {'HOME': str(home), 'PWD': str(cwd), 'J_JUMP_HOME': str(state), 'PATH': '/usr/bin:/bin', 'LANG': 'en_US.UTF-8'}
                    rc, out, err, _ = invoke(binary, env, cwd, ['--offline', 'doctor'])
                    if rc:
                        raise RuntimeError(f'{label} doctor failed: {rc} {out!r} {err!r}')
                    with sqlite3.connect(state / 'data' / 'visits.db') as db:
                        db.executemany('INSERT INTO visits(path,key,count,last_seen,weight) VALUES(?,?,?,?,?)', rows)
                    envs[label] = env
                broad_winner = (rows[0][0] + '\n').encode()
                unique_winner = (rows[-1][0] + '\n').encode()
                for mode, query, expected_rc, expected_out in [
                    ('broad', stem, 0, broad_winner),
                    ('unique', f'{stem}-{size - 1:05d}', 0, unique_winner),
                    ('no_match', 'never-matching-token-xyz', 3, b''),
                ]:
                    cases.append({'inventory': size, 'alphabet': alphabet, 'mode': mode, 'query': query, 'expected_rc': expected_rc, 'expected_out': expected_out, 'cwd': cwd, 'envs': envs, 'samples': {label: [] for label in binaries}, 'pairs': []})
        def run(case, label):
            rc, stdout, stderr, elapsed = invoke(binaries[label], case['envs'][label], case['cwd'], ['--offline', 'query', case['query']])
            if rc != case['expected_rc'] or stdout != case['expected_out'] or (rc == 0 and stderr):
                raise RuntimeError(f"{label} {case['inventory']} {case['alphabet']} {case['mode']} wrong output: {rc}, {stdout!r}, {stderr!r}")
            return elapsed
        # One untimed warmup per case/binary. No cold-cache claim.
        for case in cases:
            for label in binaries:
                run(case, label)
        rng = random.Random(300017)
        for sample in range(a.samples):
            order = list(range(len(cases)))
            rng.shuffle(order)
            for i in order:
                case = cases[i]
                labels = ['baseline', 'candidate'] if (sample + i) % 2 == 0 else ['candidate', 'baseline']
                pair = {'round': sample, 'order': labels}
                for label in labels:
                    value = run(case, label)
                    case['samples'][label].append(value)
                    pair[label + '_ms'] = value
                case['pairs'].append(pair)
        results = []
        for case in cases:
            result = {k: case[k] for k in ['inventory', 'alphabet', 'mode', 'query']}
            result['n_per_binary'] = a.samples
            result['validated_exit_code'] = case['expected_rc']
            result['validated_stdout'] = case['expected_out'].decode()
            result['successful_output_assertions'] = 2 * (a.samples + 1)
            result['timings'] = {label: {'p50_ms': statistics.median(values), 'p95_ms': percentile(values, .95), 'max_ms': max(values)} for label, values in case['samples'].items()}
            result['median_paired_speedup'] = statistics.median(p['baseline_ms'] / p['candidate_ms'] for p in case['pairs'])
            result['paired_samples'] = case['pairs']
            results.append(result)
        report = {'schema_version': 1, 'host': {'system': platform.system(), 'release': platform.release(), 'machine': platform.machine()}, 'binaries': meta, 'synthetic': True, 'fixed_visit_timestamp': fixed_now, 'samples_per_binary_case': a.samples, 'ordering': 'balanced alternating A/B and B/A pairs; deterministic shuffled cases per round', 'cache': 'one warmup per case/binary; fresh process each sample; OS caches not purged', 'limits': ['Synthetic local directories only, no human utility claim', 'Steady state query timings exclude schema initialization/setup', 'Explicit weights are seeded identically at a shared timestamp', 'Child processes fully reaped; wall time only, CPU/RSS not measured'], 'results': results}
        a.out.parent.mkdir(parents=True, exist_ok=True)
        a.out.write_text(json.dumps(report, indent=2) + '\n')
        for result in results:
            print(json.dumps({k: v for k, v in result.items() if k != 'paired_samples'}))


if __name__ == '__main__':
    main()
