#!/usr/bin/env python3
"""Frozen synthetic study; imports do not run an evaluation.

Execution: calibration --out FILE; freeze --calibration FILE --checks FILE
--out DECISION; holdout --decision DECISION --out FILE. Outputs never overwrite
existing files. A holdout claim remains even if execution fails: do not delete it
to repeat an inspected holdout. Runtime code and user stores are never modified.
"""
import argparse
import collections
import hashlib
import json
import math
import os
from pathlib import Path
import random
import sys
import tempfile

ROOT = Path(__file__).resolve().parent
CAP = 2147483647
DAY = 86400


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'), allow_nan=False) + '\n'


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def load_protocol():
    return json.loads((ROOT / 'protocol.json').read_text())


def policy_name(policy):
    return policy if isinstance(policy, str) else '{:g}d'.format(policy)


class Ranker:
    """Online reference with readable state; prediction precedes record()."""
    def __init__(self, policy):
        if policy not in ('frequency', 'recency') and (not isinstance(policy, (int, float)) or policy <= 0 or not math.isfinite(policy)):
            raise ValueError('policy must be positive finite days, frequency, or recency')
        self.policy, self.state = policy, {}

    def rank(self, now):
        def key(item):
            target, row = item
            if self.policy == 'frequency':
                score = row['count']
            elif self.policy == 'recency':
                score = row['last_seen']
            else:
                age = min(2 ** 63 - 1, max(0, now - row['last_seen']))
                score = math.log2(row['weight']) - age / (DAY * self.policy) if row['weight'] else -math.inf
            return -score, -row['last_seen'], ('/study/' + target + '/api').encode('utf-8')
        return [target for target, _ in sorted(self.state.items(), key=key)]

    def record(self, target, now):
        old = self.state.get(target)
        if old is None:
            self.state[target] = dict(count=1, last_seen=now, weight=1.0)
            return
        age = min(2 ** 63 - 1, max(0, now - old['last_seen']))
        factor = 1.0 if isinstance(self.policy, str) else 2.0 ** (-age / (DAY * self.policy))
        self.state[target] = dict(count=min(CAP, old['count'] + 1), last_seen=max(now, old['last_seen']), weight=min(float(CAP), old['weight'] * factor + 1.0))

    def snapshot(self):
        return {target: dict(row) for target, row in self.state.items()}


def _trace(split, family, rate, variant_index, seed, p):
    variant = p['families'][family][split][variant_index]
    identity = '{}/{}/{}/{}/{}'.format(split, family, rate, variant_index, seed)
    rng = random.Random(int.from_bytes(hashlib.sha256(identity.encode()).digest()[:8], 'big'))
    projects = ['d{}'.format(i) for i in range(p['candidates'])]
    rng.shuffle(projects)
    warm, duration = p['warmup_days'], p['evaluation_days']
    returned = warm + (variant if family == 'holiday' else 0)
    events, seen, ordinary_evaluations = [], set(), 0

    def append(day, second, target, dominant, evaluated, aux, probe=False):
        reason = 'warmup' if not evaluated else 'probe' if probe else 'first_visit' if target not in seen else None
        events.append(dict(now=p['base_timestamp'] + day * DAY + second, target=target, dominant=dominant, scored=reason is None, exclusion=reason, aux=aux if reason is None else []))
        seen.add(target)

    for day in range(returned + duration):
        if family == 'holiday' and warm <= day < returned:
            continue
        if family == 'sparse' and day % variant:
            continue
        evaluation_day = day - returned
        evaluated = evaluation_day >= 0
        dominant, count = 0, rate
        probability = variant if family == 'stable' else p['dominant_probability']
        if family == 'switch' and evaluated:
            dominant = (1 + evaluation_day // variant) % p['candidates']
        elif family == 'periodic':
            dominant = (day // variant) % p['candidates']
        elif family == 'burst' and evaluated:
            offset = p['families']['burst']['offset_days']
            phase = evaluation_day % p['families']['burst']['cycle_days']
            if offset <= phase < offset + variant[0]:
                dominant, count = 1, rate * variant[1]
        if family == 'holiday' and day == returned:
            append(day, 28800, projects[1], projects[0], True, [], probe=True)
        for index in range(count):
            start = 28860 if family == 'holiday' and day == returned else 28800
            second = start + index * (64800 - start) // (count - 1)
            target = projects[dominant] if rng.random() < probability else rng.choice([project for i, project in enumerate(projects) if i != dominant])
            aux = []
            if evaluated:
                if family == 'switch' and evaluation_day % variant < 14:
                    aux.append('switch')
                if family == 'burst' and offset + variant[0] <= phase < offset + variant[0] + 2:
                    aux.append('burst_recovery')
                if family == 'holiday' and ordinary_evaluations < 10:
                    aux.append('holiday')
                if family == 'sparse':
                    aux.append('sparse')
                ordinary_evaluations += 1
            append(day, second, target, projects[dominant], evaluated, aux)
    return dict(id=identity, family=family, rate=rate, variant=variant, seed=seed, paths={target: '/study/{}/api'.format(target) for target in projects}, events=events)


def generate_traces(split, protocol=None, *, allow_holdout=False):
    """Yield deterministic trace dictionaries; only run_holdout enables holdout."""
    if split == 'holdout' and not allow_holdout:
        raise ValueError('holdout generation requires a frozen decision through the holdout command')
    p = protocol or load_protocol()
    if split not in p['splits']:
        raise ValueError('unknown split')
    for family in p['execution_details']['family_order']:
        for rate in p['splits'][split]['rates_per_active_day']:
            for variant_index in range(len(p['families'][family][split])):
                for seed in p['splits'][split]['seeds']:
                    yield _trace(split, family, rate, variant_index, seed, p)


def mean(values):
    values = list(values)
    return math.fsum(values) / len(values) if values else None


def _manifest(trace):
    return dict(id=trace['id'], events=len(trace['events']), scored=sum(event['scored'] for event in trace['events']), sha256=hashlib.sha256(canonical(trace).encode()).hexdigest())


def input_manifest(traces):
    manifest = [_manifest(trace) for trace in traces]
    return manifest, hashlib.sha256(canonical(manifest).encode()).hexdigest()


def evaluate_traces(traces, policies):
    """Replay all visits, scoring only the generator's predeclared scored events."""
    rows, manifest = [], []
    for trace in traces:
        manifest.append(_manifest(trace))
        row = {key: trace[key] for key in ('id', 'family', 'rate', 'variant', 'seed') if key in trace}
        row.update(events=len(trace['events']), exclusions=dict(collections.Counter(event['exclusion'] for event in trace['events'] if not event['scored'])), policies={})
        for policy in policies:
            ranker, n, correct, reciprocal, auxiliary = Ranker(policy), 0, 0, [], {}
            for event in trace['events']:
                if event['scored']:
                    ranked = ranker.rank(event['now'])
                    position = ranked.index(event['target']) + 1
                    n += 1
                    correct += position == 1
                    reciprocal.append(1.0 / position)
                    for mask in event['aux']:
                        counts = auxiliary.setdefault(mask, dict(n=0, dominant_correct=0))
                        counts['n'] += 1
                        counts['dominant_correct'] += ranked[0] == event['dominant']
                ranker.record(event['target'], event['now'])
            for counts in auxiliary.values():
                counts['dominant_selection_rate'] = counts['dominant_correct'] / counts['n']
            row['policies'][policy_name(policy)] = dict(n=n, correct=correct, top1=correct / n if n else None, mrr=mean(reciprocal), error_rate=1 - correct / n if n else None, aux=auxiliary)
        rows.append(row)
    families = {}
    for family in dict.fromkeys(row['family'] for row in rows):
        subset = [row for row in rows if row['family'] == family]
        summary = dict(n_traces=len(subset), policies={})
        for name in map(policy_name, policies):
            values = [row['policies'][name] for row in subset]
            metric = {key: mean(value[key] for value in values if value[key] is not None) for key in ('top1', 'mrr', 'error_rate')}
            metric.update(n_events=sum(value['n'] for value in values), n_traces=sum(value['n'] > 0 for value in values), aux={})
            masks = set(mask for value in values for mask in value['aux'])
            expected = {'switch': 'switch', 'burst': 'burst_recovery', 'holiday': 'holiday', 'sparse': 'sparse'}.get(family)
            if expected:
                masks.add(expected)
            for mask in sorted(masks):
                available = [value['aux'][mask] for value in values if mask in value['aux']]
                metric['aux'][mask] = dict(n_events=sum(value['n'] for value in available), n_traces=len(available), censored_traces=len(values) - len(available), dominant_selection_rate=mean(value['dominant_selection_rate'] for value in available))
            summary['policies'][name] = metric
        families[family] = summary
    macro = {name: {key: mean(family['policies'][name][key] for family in families.values() if family['policies'][name][key] is not None) for key in ('top1', 'mrr', 'error_rate')} for name in map(policy_name, policies)}
    return dict(traces=rows, families=families, macro=macro, manifest=manifest, input_sha256=hashlib.sha256(canonical(manifest).encode()).hexdigest())


def select_challenger(result, protocol=None):
    p = protocol or load_protocol()
    incumbent = p['incumbent_days']
    base = policy_name(incumbent)
    candidates = []
    for days in p['half_lives_days']:
        if days == incumbent:
            continue
        name = policy_name(days)
        deltas = {family: result['families'][family]['policies'][name]['top1'] - result['families'][family]['policies'][base]['top1'] for family in p['execution_details']['family_order']}
        candidates.append(dict(selected_days=days, eligible=min(deltas.values()) >= -p['decision']['calibration_family_guard_pp'] / 100, macro_gain=result['macro'][name]['top1'] - result['macro'][base]['top1'], family_deltas=deltas))
    pool = [candidate for candidate in candidates if candidate['eligible']] or candidates
    best = max(candidate['macro_gain'] for candidate in pool)
    tied = [candidate for candidate in pool if best - candidate['macro_gain'] <= p['execution_details']['calibration_tie_tolerance']]
    selected = min(tied, key=lambda candidate: (abs(math.log2(candidate['selected_days'] / incumbent)), candidate['selected_days']))
    return dict(selected, incumbent_days=incumbent, candidates=candidates)


def quantile(values, q):
    ordered = sorted(values)
    index = (len(ordered) - 1) * q
    lower = int(index)
    return ordered[lower] + (ordered[min(lower + 1, len(ordered) - 1)] - ordered[lower]) * (index - lower)


def paired_bootstrap(result, challenger, protocol=None):
    p = protocol or load_protocol()
    names = policy_name(challenger), policy_name(p['incumbent_days'])
    groups = {family: [row['policies'][names[0]]['top1'] - row['policies'][names[1]]['top1'] for row in result['traces'] if row['family'] == family] for family in p['execution_details']['family_order']}
    rng, macro, draws = random.Random(p['decision']['bootstrap_seed']), [], {family: [] for family in groups}
    for _ in range(p['decision']['bootstrap_replicates']):
        means = []
        for family, values in groups.items():
            value = mean(rng.choices(values, k=len(values)))
            draws[family].append(value)
            means.append(value)
        macro.append(mean(means))
    return dict(macro_delta=mean(mean(values) for values in groups.values()), macro_lower=quantile(macro, p['decision']['macro_lower_quantile']), family_deltas={family: mean(values) for family, values in groups.items()}, family_lowers={family: quantile(values, p['decision']['family_simultaneous_lower_quantile']) for family, values in draws.items()}, replicates=len(macro))


def decision_gates(summary, eligible, checks_passed, protocol=None):
    p = protocol or load_protocol()
    gates = dict(calibration_eligible=bool(eligible), minimum_macro_gain=summary['macro_delta'] >= p['decision']['minimum_macro_gain_pp'] / 100, positive_macro_lower=summary['macro_lower'] > 0, family_noninferiority=all(value >= -p['decision']['maximum_family_loss_pp'] / 100 for value in summary['family_lowers'].values()), invariants=bool(checks_passed))
    return dict(gates=gates, switch_recommended=all(gates.values()))


def bindings():
    return dict(protocol_sha256=sha(ROOT / 'protocol.json'), evaluator_sha256=sha(__file__))


def _read(path):
    return json.loads(Path(path).read_text())


def _verify_bindings(value):
    if any(value.get(key) != expected for key, expected in bindings().items()):
        raise ValueError('protocol or evaluator hash changed')


def _verify_checks(path):
    checks = _read(path)
    _verify_bindings(checks)
    if checks.get('passed') is not True or not checks.get('evidence'):
        raise ValueError('passing invariant evidence is required')
    for item in checks['evidence']:
        evidence = Path(item['path'])
        if not evidence.is_absolute():
            evidence = Path(path).resolve().parent / evidence
        if item.get('result') not in ('pass', 'passed', True) or sha(evidence) != item['sha256']:
            raise ValueError('invariant evidence failed or changed')
    return checks


def write_new(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix='.' + path.name + '-', dir=str(path.parent))
    try:
        with os.fdopen(fd, 'w', encoding='utf-8') as stream:
            stream.write(canonical(value))
            stream.flush()
            os.fsync(stream.fileno())
        os.link(temporary, path)  # Atomic publication; refuses an existing path.
    finally:
        os.unlink(temporary)


def run_calibration(out_path):
    if Path(out_path).exists():
        raise FileExistsError(out_path)
    p = load_protocol()
    result = evaluate_traces(generate_traces('calibration', p), p['half_lives_days'] + p['controls'])
    result.update(bindings(), schema_version=1, split='calibration')
    write_new(out_path, result)
    return result


def freeze_decision(calibration_path, checks_path, out_path):
    if Path(out_path).exists():
        raise FileExistsError(out_path)
    calibration = _read(calibration_path)
    _verify_bindings(calibration)
    if calibration.get('split') != 'calibration':
        raise ValueError('expected calibration output')
    _verify_checks(checks_path)
    manifest, digest = input_manifest(generate_traces('calibration'))
    if digest != calibration['input_sha256'] or manifest != calibration['manifest']:
        raise ValueError('calibration input manifest mismatch')
    decision = dict(bindings(), schema_version=1, selection=select_challenger(calibration), input_sha256=digest, calibration=dict(path=str(Path(calibration_path).resolve()), sha256=sha(calibration_path)), checks=dict(path=str(Path(checks_path).resolve()), sha256=sha(checks_path)))
    write_new(out_path, decision)
    return decision


def run_holdout(decision_path, out_path):
    if Path(out_path).exists():
        raise FileExistsError(out_path)
    decision = _read(decision_path)
    _verify_bindings(decision)
    for field in ('calibration', 'checks'):
        if sha(decision[field]['path']) != decision[field]['sha256']:
            raise ValueError(field + ' evidence changed after freeze')
    _verify_checks(decision['checks']['path'])
    calibration = _read(decision['calibration']['path'])
    if decision['selection'] != select_challenger(calibration) or decision['input_sha256'] != calibration['input_sha256']:
        raise ValueError('frozen calibration decision changed')
    claim_path = Path(str(Path(decision_path).resolve()) + '.holdout-claim.json')
    claim = dict(decision_sha256=sha(decision_path), output=str(Path(out_path).resolve()))
    write_new(claim_path, claim)
    p = load_protocol()
    challenger = decision['selection']['selected_days']
    result = evaluate_traces(generate_traces('holdout', p, allow_holdout=True), [p['incumbent_days'], challenger] + p['controls'])
    uncertainty = paired_bootstrap(result, challenger, p)
    result.update(bindings(), schema_version=1, split='holdout', decision_sha256=claim['decision_sha256'], uncertainty=uncertainty, decision=decision_gates(uncertainty, decision['selection']['eligible'], True, p))
    write_new(out_path, result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    calibration = commands.add_parser('calibration')
    calibration.add_argument('--out', required=True, type=Path)
    freeze = commands.add_parser('freeze')
    freeze.add_argument('--calibration', required=True, type=Path)
    freeze.add_argument('--checks', required=True, type=Path)
    freeze.add_argument('--out', required=True, type=Path)
    holdout = commands.add_parser('holdout')
    holdout.add_argument('--decision', required=True, type=Path)
    holdout.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    if args.command == 'calibration':
        run_calibration(args.out)
    elif args.command == 'freeze':
        freeze_decision(args.calibration, args.checks, args.out)
    else:
        run_holdout(args.decision, args.out)
    print(canonical(dict(command=args.command, output=str(args.out))), end='')


if __name__ == '__main__':
    try:
        main()
    except (ValueError, KeyError, OSError) as error:
        print('Study stopped: ' + str(error), file=sys.stderr)
        sys.exit(1)
