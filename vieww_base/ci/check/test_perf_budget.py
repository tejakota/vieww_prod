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

    def test_a_broken_budgets_file_is_refused(self):
        for bad in (
            {"schema": "other", "tolerance_percent": 10, "classes": {}},
            {"schema": pb.SCHEMA, "tolerance_percent": 0, "classes": {}},
            {"schema": pb.SCHEMA, "tolerance_percent": 10, "classes": {"x": {"m": -1}}},
            {"schema": pb.SCHEMA, "tolerance_percent": 10, "classes": {"x": {}}},
        ):
            with self.assertRaises(ValueError):
                pb.validate(bad)

    def test_something_else_is_refused(self):
        with self.assertRaises(ValueError):
            pb.measurements({"hello": 1})


if __name__ == "__main__":
    unittest.main()
