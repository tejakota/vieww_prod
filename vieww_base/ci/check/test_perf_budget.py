"""Tests for perf_budget.py: python3 -I ci/check/test_perf_budget.py"""

import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import perf_budget as pb  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))

RECEIPT = {
    "schema": "vieww.bench-receipt/1",
    "mode": "gallery",
    "machine": {"os": "Ubuntu 24.04", "arch": "x86_64", "cpu_model": "Intel i7-8550U"},
    "fixtures": [
        {"name": "editor", "cpu_p50_ms": 4.0},
        {"name": "list", "cpu_p50_ms": 2.0, "gpu_best_ms": 1.0},
    ],
}

SUMMARY = {"devices": [{"device": "LG Stylo 5", "result": {
    "model": "LM-Q720",
    "gpu_frame_time": "median 16.437136ms, worst 22.85ms, budget 16.66ms (240 samples)",
    "auto_frame_time": "median 348.803µs, worst 2.4ms, budget 8.3ms (240 samples)",
}}, {"device": "no result", "result": None}]}


def write(obj):
    fd, path = tempfile.mkstemp(suffix=".json")
    with os.fdopen(fd, "w") as f:
        json.dump(obj, f)
    return path


class PerfBudget(unittest.TestCase):
    def test_the_committed_budgets_file_is_valid(self):
        pb.validate(pb.load_budgets(os.path.join(HERE, "perf-budgets.json")))

    def test_receipt_and_summary_are_read(self):
        m = pb.measurements(RECEIPT)
        cls = "ubuntu-24-04/x86-64/intel-i7-8550u"
        self.assertEqual(m[cls]["gallery/editor/cpu_p50_ms"], 4.0)
        self.assertEqual(m[cls]["gallery/list/gpu_best_ms"], 1.0)
        s = pb.measurements(SUMMARY)
        self.assertAlmostEqual(s["phone-lm-q720"]["gpu_frame_time_p50_ms"], 16.437136)
        self.assertAlmostEqual(s["phone-lm-q720"]["auto_frame_time_p50_ms"], 0.348803)
        self.assertNotIn("cpu_frame_time_p50_ms", s["phone-lm-q720"], "the phones' cpu pass is not gated")
        self.assertEqual(list(pb.measurements(RECEIPT, "kabylake-linux")), ["kabylake-linux"])

    def test_record_then_check_then_a_regression_fails(self):
        budgets = write({"schema": pb.SCHEMA, "tolerance_percent": 10, "classes": {}})
        receipt = write(RECEIPT)
        self.assertEqual(pb.main(["record", budgets, receipt, "--class", "kabylake-linux"]), 0)
        self.assertEqual(pb.main(["check", budgets, receipt, "--class", "kabylake-linux"]), 0)
        slower = json.loads(json.dumps(RECEIPT))
        slower["fixtures"][0]["cpu_p50_ms"] = 5.2  # +30%, +1.2 ms
        self.assertEqual(pb.main(["check", budgets, write(slower), "--class", "kabylake-linux"]), 1)
        within = json.loads(json.dumps(RECEIPT))
        within["fixtures"][0]["cpu_p50_ms"] = 4.3  # +7.5%
        self.assertEqual(pb.main(["check", budgets, write(within), "--class", "kabylake-linux"]), 0)

    def test_a_small_absolute_change_is_noise(self):
        budgets = write({"schema": pb.SCHEMA, "tolerance_percent": 10, "min_delta_ms": 1.0,
                         "classes": {"phone-x": {"gpu_frame_time_p50_ms": 0.35}}})
        fast = {"devices": [{"device": "x", "result": {
            "model": "x", "gpu_frame_time": "median 0.9ms, worst 2ms"}}]}
        self.assertEqual(pb.main(["check", budgets, write(fast)]), 0, "+0.55 ms is under the floor")
        slow = {"devices": [{"device": "x", "result": {
            "model": "x", "gpu_frame_time": "median 1.6ms, worst 2ms"}}]}
        self.assertEqual(pb.main(["check", budgets, write(slow)]), 1, "+1.25 ms is over it")

    def test_an_unknown_class_is_reported_not_failed(self):
        budgets = write({"schema": pb.SCHEMA, "tolerance_percent": 10, "classes": {}})
        self.assertEqual(pb.main(["check", budgets, write(SUMMARY)]), 0)

    def test_a_new_class_is_held_to_its_last_run(self):
        budgets = write({"schema": pb.SCHEMA, "tolerance_percent": 10, "classes": {}})
        receipt = write(RECEIPT)
        last = tempfile.mktemp(suffix=".json")
        # First run: nothing to compare with, figures saved for the next.
        self.assertEqual(pb.main(["check", budgets, receipt, "--baseline", last, "--write-measured", last]), 0)
        self.assertTrue(os.path.exists(last))
        # Second run, 15% slower: inside the last-run tolerance (25%).
        a_bit = json.loads(json.dumps(RECEIPT))
        a_bit["fixtures"][0]["cpu_p50_ms"] = 4.6
        self.assertEqual(pb.main(["check", budgets, write(a_bit), "--baseline", last]), 0)
        # 60% slower: over it, and failed...
        slow = json.loads(json.dumps(RECEIPT))
        slow["fixtures"][0]["cpu_p50_ms"] = 6.4
        self.assertEqual(pb.main(["check", budgets, write(slow), "--baseline", last]), 1)
        # ...unless advisory, as on a hosted runner.
        self.assertEqual(pb.main(["check", budgets, write(slow), "--baseline", last, "--advisory"]), 0)
        # A committed budget wins over the last run.
        pb.main(["record", budgets, write(slow)])
        self.assertEqual(pb.main(["check", budgets, write(slow), "--baseline", last]), 0)

    def test_a_missing_or_foreign_baseline_is_ignored(self):
        budgets = write({"schema": pb.SCHEMA, "tolerance_percent": 10, "classes": {}})
        self.assertEqual(pb.main(["check", budgets, write(RECEIPT), "--baseline", "/no/such.json"]), 0)
        self.assertEqual(pb.main(["check", budgets, write(RECEIPT), "--baseline", write({"x": 1})]), 0)

    def test_a_broken_budgets_file_is_refused(self):
        for bad in (
            {"schema": "other", "tolerance_percent": 10, "classes": {}},
            {"schema": pb.SCHEMA, "tolerance_percent": 0, "classes": {}},
            {"schema": pb.SCHEMA, "tolerance_percent": 10, "classes": {"x": {"m": -1}}},
            {"schema": pb.SCHEMA, "tolerance_percent": 10, "classes": {"x": {}}},
        ):
            with self.assertRaises(ValueError):
                pb.validate(bad)

    def test_a_broken_known_limitations_entry_is_refused(self):
        good_classes = {"x": {"gallery/f/cpu_p50_ms": 23.8}}
        for bad in (
            # no budget class to drift-gate against
            {"schema": pb.SCHEMA, "tolerance_percent": 10, "classes": {},
             "known_limitations": {"x": {"gallery/f/cpu_p50_ms": {"target_ms": 16.7, "reason": "r"}}}},
            # no budget figure for that metric
            {"schema": pb.SCHEMA, "tolerance_percent": 10, "classes": {"x": {"other": 1.0}},
             "known_limitations": {"x": {"gallery/f/cpu_p50_ms": {"target_ms": 16.7, "reason": "r"}}}},
            # no target
            {"schema": pb.SCHEMA, "tolerance_percent": 10, "classes": good_classes,
             "known_limitations": {"x": {"gallery/f/cpu_p50_ms": {"reason": "r"}}}},
            # no reason naming the decision
            {"schema": pb.SCHEMA, "tolerance_percent": 10, "classes": good_classes,
             "known_limitations": {"x": {"gallery/f/cpu_p50_ms": {"target_ms": 16.7}}}},
        ):
            with self.assertRaises(ValueError):
                pb.validate(bad)

    def test_the_committed_known_limitations_name_their_decision(self):
        b = pb.load_budgets(os.path.join(HERE, "perf-budgets.json"))
        self.assertTrue(b.get("known_limitations"), "FW-07's two over-budget fixtures are recorded")
        for cls, metrics in b["known_limitations"].items():
            for name, entry in metrics.items():
                self.assertIn(name, b["classes"][cls],
                              f"{cls} {name}: a limitation must drift-gate a committed figure")
                self.assertGreater(entry["target_ms"], 0)
                self.assertTrue(entry["reason"].strip())

    def a_limitation_fixture(self):
        """Budgets with one known limitation, and a receipt factory for it."""
        budgets = write({
            "schema": pb.SCHEMA, "tolerance_percent": 10, "min_delta_ms": 1.0,
            "classes": {"kabylake-linux": {"gallery/00-layers-nested/cpu_p50_ms": 23.8}},
            "known_limitations": {"kabylake-linux": {
                "gallery/00-layers-nested/cpu_p50_ms": {
                    "target_ms": 16.7, "since": "2026-10-15",
                    "reason": "accepted miss; closing it needs the f16 fidelity ruling"}}},
        })
        nested = {"schema": "vieww.bench-receipt/1", "mode": "gallery",
                  "machine": {"os": "o", "arch": "a", "cpu_model": "c"},
                  "fixtures": [{"name": "00-layers-nested", "cpu_p50_ms": 23.8}]}
        return budgets, nested

    def test_a_known_limitation_does_not_license_drift(self):
        budgets, nested = self.a_limitation_fixture()
        slower = json.loads(json.dumps(nested))
        slower["fixtures"][0]["cpu_p50_ms"] = 27.5  # +15.5%, +3.7 ms
        self.assertEqual(
            pb.main(["check", budgets, write(slower), "--class", "kabylake-linux"]), 1,
            "over the recorded figure by tolerance AND floor fails, limitation or not")

    def test_a_known_limitation_under_target_is_flagged_stale_not_passed(self):
        budgets, nested = self.a_limitation_fixture()
        won = json.loads(json.dumps(nested))
        won["fixtures"][0]["cpu_p50_ms"] = 15.9  # under the 16.7 ms target at last
        rows, failures, unknown, stale = pb.check(pb.load_budgets(budgets), pb.gather([write(won)], "kabylake-linux"))
        self.assertEqual(failures, [])
        self.assertEqual(unknown, [])
        self.assertEqual(stale, [("kabylake-linux", "gallery/00-layers-nested/cpu_p50_ms", 16.7, 15.9)])
        self.assertIn("stale limitation", rows[0][5])
        # ...and the run itself exits 0: good news is not a failure, only loud.
        self.assertEqual(pb.main(["check", budgets, write(won), "--class", "kabylake-linux"]), 0)

    def test_recording_under_a_limitation_target_warns_while_the_file_is_open(self):
        budgets, nested = self.a_limitation_fixture()
        won = json.loads(json.dumps(nested))
        won["fixtures"][0]["cpu_p50_ms"] = 15.9
        self.assertEqual(pb.main(["record", budgets, write(won), "--class", "kabylake-linux"]), 0)
        with open(budgets) as f:
            reread = json.load(f)
        self.assertEqual(reread["classes"]["kabylake-linux"]["gallery/00-layers-nested/cpu_p50_ms"], 15.9)
        # The stale limitation survives the rewrite for a human to remove
        # deliberately: `record` never edits the decision record.
        self.assertIn("gallery/00-layers-nested/cpu_p50_ms", reread["known_limitations"]["kabylake-linux"])

    def test_something_else_is_refused(self):
        with self.assertRaises(ValueError):
            pb.measurements({"hello": 1})


if __name__ == "__main__":
    unittest.main()
