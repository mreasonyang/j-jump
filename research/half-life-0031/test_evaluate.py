"""Independent small-fixture tests for the frozen 0031 study.

These tests never generate or inspect the formal holdout. Mathematical tests
use hand-written events; generator tests use a visibly reduced calibration
protocol. The coordinator owns formal calibration and holdout execution.
"""
import copy
import importlib.util
import math
from pathlib import Path
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("half_life_evaluate", ROOT / "evaluate.py")
study = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(study)

DAY = 86400
NOW = 1_800_000_000


def policy_name(days):
    return f"{float(days):g}d"


def event(target, now, scored=True, exclusion=None, dominant=None, aux=None):
    return {
        "target": target,
        "now": now,
        "scored": scored,
        "dominant": dominant or target,
        "aux": aux or [],
        "exclusion": exclusion,
    }


def trace(events, name="tiny", family="stable"):
    return {
        "id": name,
        "family": family,
        "rate": 2,
        "variant": 0.8,
        "seed": 123,
        "paths": {f"d{i}": f"/study/d{i}/api" for i in range(4)},
        "events": events,
    }


def metrics(accuracy):
    return {"n": 100, "top1": accuracy, "mrr": accuracy, "error_rate": 1 - accuracy}


def calibration_result(deltas, protocol=None):
    """Build coherent family/macro numbers without generating any histories."""
    protocol = protocol or study.load_protocol()
    families = protocol["execution_details"]["family_order"]
    result = {"families": {}, "macro": {}, "traces": []}
    for index, family in enumerate(families):
        result["families"][family] = {"policies": {}}
        for days in protocol["half_lives_days"]:
            gain = 0.0 if days == 7 else deltas[days][index]
            result["families"][family]["policies"][policy_name(days)] = metrics(0.6 + gain)
    for days in protocol["half_lives_days"]:
        gain = 0.0 if days == 7 else sum(deltas[days]) / len(families)
        result["macro"][policy_name(days)] = metrics(0.6 + gain)
    return result


class MathematicalReplayTests(unittest.TestCase):
    def test_iterative_updates_equal_closed_form_event_sum(self):
        offsets = [0, 0, 43200, 2 * DAY, 9 * DAY, 35 * DAY]
        for days in (1, 3, 7, 14, 30, 60):
            ranker = study.Ranker(days)
            for index, offset in enumerate(offsets):
                ranker.record("d0", NOW + offset)
                expected = math.fsum(
                    2 ** (-(offset - old) / (days * DAY)) for old in offsets[:index + 1]
                )
                state = ranker.snapshot()["d0"]
                self.assertEqual(state["count"], index + 1)
                self.assertEqual(state["last_seen"], NOW + offset)
                self.assertAlmostEqual(state["weight"], expected, places=12)

    def test_half_life_aggregate_cannot_identify_another_half_life(self):
        histories = ([21, 21, 7, 0], [14, 14, 14, 0])
        weights = {}
        for days in (7, 14):
            weights[days] = []
            for ages in histories:
                ranker = study.Ranker(days)
                for age in ages:
                    ranker.record("d0", NOW - age * DAY)
                row = ranker.snapshot()["d0"]
                self.assertEqual((row["count"], row["last_seen"]), (4, NOW))
                weights[days].append(row["weight"])
        self.assertEqual(weights[7], [1.75, 1.75])
        self.assertAlmostEqual(weights[14][0], 1 + math.sqrt(2), places=12)
        self.assertAlmostEqual(weights[14][1], 2.5, places=12)
        self.assertNotEqual(weights[14][0], weights[14][1])

    def test_clock_rollback_does_not_move_anchor_or_decay_twice(self):
        ranker = study.Ranker(7)
        ranker.record("d0", NOW)
        ranker.record("d0", NOW - DAY)
        self.assertEqual(ranker.snapshot()["d0"], {"count": 2, "last_seen": NOW, "weight": 2.0})
        ranker.record("d0", NOW + 7 * DAY)
        self.assertEqual(ranker.snapshot()["d0"], {"count": 3, "last_seen": NOW + 7 * DAY, "weight": 2.0})

    def test_idle_ranking_is_invariant_and_ranking_does_not_mutate_history(self):
        ranker = study.Ranker(7)
        ranker.record("d0", NOW - 7 * DAY)
        ranker.record("d0", NOW - 7 * DAY)
        ranker.record("d0", NOW - 7 * DAY)
        ranker.record("d1", NOW)
        before = ranker.snapshot()
        for elapsed in (0, DAY, 100 * DAY, 10000 * DAY):
            self.assertEqual(ranker.rank(NOW + elapsed), ["d0", "d1"])
            self.assertEqual(ranker.snapshot(), before)

    def test_ties_use_recency_then_path_and_only_seen_targets_compete(self):
        ranker = study.Ranker(7)
        self.assertEqual(ranker.rank(NOW), [])
        ranker.record("d1", NOW)
        self.assertEqual(ranker.rank(NOW), ["d1"])
        ranker.record("d0", NOW)
        self.assertEqual(ranker.rank(NOW), ["d0", "d1"])

    def test_frequency_and_recency_controls_have_distinct_contracts(self):
        frequency, recency = study.Ranker("frequency"), study.Ranker("recency")
        for ranker in (frequency, recency):
            ranker.record("d0", NOW)
            ranker.record("d0", NOW + 1)
            ranker.record("d1", NOW + 2)
        self.assertEqual(frequency.rank(NOW + 3), ["d0", "d1"])
        self.assertEqual(recency.rank(NOW + 3), ["d1", "d0"])

    def test_snapshot_is_detached_from_live_state(self):
        ranker = study.Ranker(7)
        ranker.record("d0", NOW)
        snapshot = ranker.snapshot()
        snapshot["d0"]["weight"] = 999
        self.assertEqual(ranker.snapshot()["d0"]["weight"], 1.0)


class ProspectiveEvaluationTests(unittest.TestCase):
    def test_prediction_precedes_update_and_cold_target_is_recorded(self):
        # First B is cold and excluded but recorded. Second B loses the A/B
        # frequency tie; third B wins. Updating before scoring would score 1.0.
        tiny = trace([
            event("d0", NOW, False, "warmup"),
            event("d1", NOW, False, "first_visit"),
            event("d1", NOW),
            event("d1", NOW),
        ])
        result = study.evaluate_traces([tiny], [7])
        observed = result["traces"][0]["policies"]["7d"]
        self.assertEqual(observed["n"], 2)
        self.assertEqual(observed["top1"], 0.5)
        self.assertEqual(observed["mrr"], 0.75)

    def test_unscored_probe_updates_state_before_ordinary_predictions(self):
        tiny = trace([
            event("d0", NOW, False, "warmup"),
            event("d1", NOW + 100 * DAY, False, "probe"),
            event("d1", NOW + 100 * DAY + 60),
        ], family="holiday")
        result = study.evaluate_traces([tiny], [7])
        observed = result["traces"][0]["policies"]["7d"]
        self.assertEqual(observed["n"], 1)
        self.assertEqual(observed["top1"], 1.0)

    def test_input_hash_is_independent_of_policies_and_binds_actual_events(self):
        tiny = trace([event("d0", NOW, False, "warmup"), event("d0", NOW + 1)])
        one = study.evaluate_traces([tiny], [7])
        two = study.evaluate_traces([copy.deepcopy(tiny)], [3, 7])
        self.assertEqual(one["input_sha256"], two["input_sha256"])
        changed = copy.deepcopy(tiny)
        changed["events"][-1]["now"] += 1
        other = study.evaluate_traces([changed], [7])
        self.assertNotEqual(one["input_sha256"], other["input_sha256"])
        self.assertEqual(tiny["events"][-1]["now"], NOW + 1)


class TinyCalibrationGeneratorTests(unittest.TestCase):
    def tiny_protocol(self, family):
        protocol = study.load_protocol()
        protocol["warmup_days"] = 0
        protocol["evaluation_days"] = 2
        protocol["execution_details"]["family_order"] = [family]
        protocol["splits"]["calibration"] = {"rates_per_active_day": [3], "seeds": [123]}
        protocol["families"][family]["calibration"] = [1.0 if family == "stable" else 2]
        return protocol

    def test_tiny_calibration_times_first_visit_and_reproducibility(self):
        protocol = self.tiny_protocol("stable")
        first = list(study.generate_traces("calibration", protocol))
        self.assertEqual(first, list(study.generate_traces("calibration", copy.deepcopy(protocol))))
        self.assertEqual(len(first), 1)
        events = first[0]["events"]
        self.assertEqual([e["now"] - NOW for e in events],
                         [28800, 46800, 64800, DAY + 28800, DAY + 46800, DAY + 64800])
        self.assertEqual(events[0]["exclusion"], "first_visit")
        self.assertFalse(events[0]["scored"])
        self.assertTrue(all(e["scored"] for e in events[1:]))
        self.assertTrue(all(e["target"] == events[0]["target"] for e in events))

    def test_holiday_probe_precedes_return_events_after_empty_gap(self):
        protocol = self.tiny_protocol("holiday")
        protocol["warmup_days"] = 1
        events = list(study.generate_traces("calibration", protocol))[0]["events"]
        relative = [e["now"] - NOW for e in events]
        self.assertFalse(any(DAY <= second < 3 * DAY for second in relative))
        probe = next(e for e in events if e["exclusion"] == "probe")
        self.assertEqual(probe["now"], NOW + 3 * DAY + 28800)
        self.assertFalse(probe["scored"])
        returned = [e for e in events if 3 * DAY <= e["now"] - NOW < 4 * DAY and e is not probe]
        self.assertEqual([e["now"] - NOW - 3 * DAY for e in returned], [28860, 46830, 64800])
        self.assertTrue(all(e["aux"] == ["holiday"] for e in returned if e["scored"]))


class DecisionTests(unittest.TestCase):
    def setUp(self):
        self.protocol = study.load_protocol()
        self.families = self.protocol["execution_details"]["family_order"]

    def deltas(self, value=-0.03):
        return {days: [value] * len(self.families) for days in self.protocol["half_lives_days"] if days != 7}

    def test_quantiles_use_declared_linear_interpolation(self):
        self.assertEqual(study.quantile([30, 0, 20, 10], 0.25), 7.5)
        self.assertEqual(study.quantile([30, 0, 20, 10], 0), 0)
        self.assertEqual(study.quantile([30, 0, 20, 10], 1), 30)



    def test_calibration_rejects_high_mean_with_excessive_family_loss(self):
        deltas = self.deltas()
        deltas[1] = [-0.02, 0.06, 0.06, 0.06, 0.06, 0.06]
        deltas[3] = [0.02] * 6
        deltas[14] = [0.01] * 6
        decision = study.select_challenger(calibration_result(deltas), self.protocol)
        self.assertTrue(decision["eligible"])
        self.assertEqual(decision["selected_days"], 3)

    def test_no_eligible_challenger_is_diagnostic_only(self):
        decision = study.select_challenger(calibration_result(self.deltas()), self.protocol)
        self.assertFalse(decision["eligible"])
        self.assertNotEqual(decision["selected_days"], 7)

    def test_near_macro_ties_prefer_nearest_incumbent_half_life(self):
        deltas = self.deltas()
        deltas[3] = [0.01 + 5e-13] * 6
        deltas[14] = [0.01] * 6
        decision = study.select_challenger(calibration_result(deltas), self.protocol)
        self.assertEqual(decision["selected_days"], 14)
        deltas[3] = [0.01 + 2e-12] * 6
        decision = study.select_challenger(calibration_result(deltas), self.protocol)
        self.assertEqual(decision["selected_days"], 3)

    def test_each_switch_gate_is_required_with_unrounded_boundary_values(self):
        base = {"macro_delta": 0.01, "macro_lower": 1e-9,
                "family_deltas": {family: 0.01 for family in self.families},
                "family_lowers": {family: -0.01 for family in self.families}}
        self.assertTrue(study.decision_gates(base, True, True, self.protocol)["switch_recommended"])
        for field, value in [("macro_delta", 0.01 - 1e-9), ("macro_lower", 0.0)]:
            summary = copy.deepcopy(base)
            summary[field] = value
            self.assertFalse(study.decision_gates(summary, True, True, self.protocol)["switch_recommended"])
        summary = copy.deepcopy(base)
        summary["family_lowers"][self.families[0]] = -0.01 - 1e-9
        self.assertFalse(study.decision_gates(summary, True, True, self.protocol)["switch_recommended"])
        self.assertFalse(study.decision_gates(base, False, True, self.protocol)["switch_recommended"])
        self.assertFalse(study.decision_gates(base, True, False, self.protocol)["switch_recommended"])

    def test_bootstrap_is_paired_reproducible_and_equally_weights_families(self):
        traces = []
        for index, family in enumerate(self.families):
            delta = (0.12 if index == 0 else -0.06 if index == 1 else 0.0)
            for seed in range(2 if index == 0 else 8):
                traces.append({"id": f"{family}/{seed}", "family": family,
                               "policies": {"7d": metrics(0.5), "14d": metrics(0.5 + delta)}})
        result = {"traces": traces}
        protocol = copy.deepcopy(self.protocol)
        protocol["decision"]["bootstrap_replicates"] = 50
        first = study.paired_bootstrap(result, 14, protocol)
        second = study.paired_bootstrap(copy.deepcopy(result), 14, protocol)
        self.assertEqual(first, second)
        self.assertAlmostEqual(first["macro_delta"], 0.01, places=12)
        self.assertAlmostEqual(first["macro_lower"], 0.01, places=12)
        self.assertAlmostEqual(first["family_lowers"][self.families[0]], 0.12, places=12)
        self.assertAlmostEqual(first["family_lowers"][self.families[1]], -0.06, places=12)


class HoldoutBoundaryTests(unittest.TestCase):
    def fixture(self, root):
        """Mock all generator calls: even guard tests never touch holdout data."""
        tiny = trace([event("d0", NOW, False, "warmup"), event("d0", NOW + 1)])
        manifest, digest = study.input_manifest([tiny])
        deltas = {days: [0.01] * 6 for days in (1, 3, 14, 30, 60)}
        calibration = calibration_result(deltas)
        calibration.update(study.bindings(), split="calibration", manifest=manifest, input_sha256=digest)
        calibration_path, checks_path, decision_path = [root / name for name in ("calibration.json", "checks.json", "decision.json")]
        evidence_path = root / "toy-evidence.txt"
        evidence_path.write_text("hand-written fixture only\n", encoding="utf-8")
        study.write_new(calibration_path, calibration)
        study.write_new(checks_path, dict(study.bindings(), passed=True,
                         evidence=[dict(path=str(evidence_path), sha256=study.sha(evidence_path), kind="toy", result="pass")]))
        with mock.patch.object(study, "generate_traces", return_value=iter([tiny])) as generator:
            study.freeze_decision(calibration_path, checks_path, decision_path)
            generator.assert_called_once_with("calibration")
        return decision_path, evidence_path

    def test_missing_decision_is_rejected_before_generator_can_run(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            with mock.patch.object(study, "generate_traces", side_effect=AssertionError("holdout generation is prohibited in tests")) as generator:
                with self.assertRaises((OSError, ValueError, RuntimeError)):
                    study.run_holdout(root / "missing.json", root / "out.json")
                generator.assert_not_called()
            self.assertFalse((root / "out.json").exists())

    def test_changed_evidence_is_rejected_before_generator_can_run(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            decision, evidence = self.fixture(root)
            evidence.write_text("changed\n", encoding="utf-8")
            with mock.patch.object(study, "generate_traces", side_effect=AssertionError("forbidden")) as generator:
                with self.assertRaises(ValueError):
                    study.run_holdout(decision, root / "out.json")
                generator.assert_not_called()
            self.assertFalse(Path(str(decision) + ".holdout-claim.json").exists())

    def test_changed_selection_is_rejected_before_generator_can_run(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            decision, _ = self.fixture(root)
            value = study._read(decision)
            value["selection"]["selected_days"] = 60
            decision.write_text(study.canonical(value), encoding="utf-8")
            with mock.patch.object(study, "generate_traces", side_effect=AssertionError("forbidden")) as generator:
                with self.assertRaises(ValueError):
                    study.run_holdout(decision, root / "out.json")
                generator.assert_not_called()

    def test_holdout_claim_precedes_generation_and_blocks_retry_after_failure(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            decision, _ = self.fixture(root)
            claim = Path(str(decision) + ".holdout-claim.json")
            def stop_instead_of_generating(*args, **kwargs):
                self.assertTrue(claim.exists())
                self.assertEqual(args[0], "holdout")
                self.assertTrue(kwargs["allow_holdout"])
                raise RuntimeError("intentional mock stop; no holdout generated")
            with mock.patch.object(study, "generate_traces", side_effect=stop_instead_of_generating) as generator:
                with self.assertRaisesRegex(RuntimeError, "intentional mock"):
                    study.run_holdout(decision, root / "out.json")
                with self.assertRaises(FileExistsError):
                    study.run_holdout(decision, root / "retry.json")
                self.assertEqual(generator.call_count, 1)
            self.assertFalse((root / "out.json").exists())
            self.assertFalse((root / "retry.json").exists())

    def test_evidence_publication_does_not_overwrite(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "evidence.json"
            study.write_new(path, {"original": True})
            with self.assertRaises(FileExistsError):
                study.write_new(path, {"replacement": True})
            self.assertEqual(study._read(path), {"original": True})


if __name__ == "__main__":
    unittest.main()
