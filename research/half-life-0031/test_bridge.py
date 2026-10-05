"""Parity-harness checks; no generated trace or quality evaluation is run."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("production_parity_verifier", HERE / "verify_production.py")
bridge = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bridge)
BINARY = os.environ.get("JJ_PRODUCTION_BRIDGE")


class ReferenceChecks(unittest.TestCase):
    def test_integer_age_saturates_like_rust(self):
        self.assertEqual(bridge.age(bridge.I64_MAX, -bridge.I64_MAX - 1), bridge.I64_MAX)
        self.assertEqual(bridge.age(-bridge.I64_MAX - 1, bridge.I64_MAX), 0)

    def test_half_life_and_rollback_anchor(self):
        paths = {"d0": "/study/d0/api"}
        reference = bridge.EdgeReference(paths, [dict(id="d0", count=8, last_seen=0, weight=8.0)])
        reference.record("d0", 604800)
        self.assertEqual(reference.snapshot()["d0"], dict(count=9, last_seen=604800, weight=5.0))
        reference.record("d0", 0)
        self.assertEqual(reference.snapshot()["d0"], dict(count=10, last_seen=604800, weight=6.0))

    def test_parity_rejects_an_incorrect_weight(self):
        verifier = object.__new__(bridge.Verifier)
        verifier.max_abs_error = verifier.max_relative_error = 0.0
        verifier.weight_checks = 0
        with self.assertRaises(AssertionError):
            verifier.check_row(dict(id="d0", path="/study/d0/api", count=1, last_seen=0, weight=2.0),
                               dict(count=1, last_seen=0, weight=1.0), "d0", {"d0": "/study/d0/api"})

    def test_existing_report_is_refused_before_any_build(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "evidence.json"
            report.write_text("kept", encoding="utf-8")
            build = Path(directory) / "must-not-build"
            result = subprocess.run([sys.executable, str(HERE / "verify_production.py"), "--smoke",
                                     "--out", str(report), "--build-dir", str(build)],
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            self.assertEqual(result.returncode, 2)
            self.assertEqual(report.read_text(encoding="utf-8"), "kept")
            self.assertFalse(build.exists())


@unittest.skipUnless(BINARY, "set JJ_PRODUCTION_BRIDGE to the source-bound research executable")
class ProductionBridgeChecks(unittest.TestCase):
    def test_multiple_edges_and_study_reference_share_the_stream(self):
        verifier = bridge.Verifier(Path(BINARY))
        try:
            for trace in bridge.edge_traces():
                verifier.trace(trace, bridge.EdgeReference(trace["paths"], trace["initial"]))
            # Handwritten fixture verifies the evaluate.py API; no generator or
            # parameter comparison is invoked by this unit test.
            evaluate = bridge.load_evaluator()
            trace = dict(id="api-fixture", paths={"d0": "/study/d0/api", "d1": "/study/d1/api"},
                         events=[dict(now=0, target="d1"), dict(now=604800, target="d0"),
                                 dict(now=1209600, target="d1")])
            verifier.trace(trace, evaluate.Ranker(7))
            verifier.close()
        except BaseException:
            verifier.abort()
            raise

    def test_invalid_initial_weight_is_refused(self):
        line = dict(id="invalid", paths={"d0": "/study/d0/api"}, events=[],
                    initial=[dict(id="d0", path="/study/d0/api", count=1, last_seen=0, weight=-1)])
        result = subprocess.run([BINARY], input=bridge.canonical_bytes(line) + b"\n",
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"")


if __name__ == "__main__":
    unittest.main()
