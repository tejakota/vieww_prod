"""python3 -I tools/test_make_scorecard.py"""
import os, sys, unittest
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import make_scorecard as sc

DOC = {"phones": [{"device": "Redmi", "configs": {"vieww-gpu": {
    "quality": "blur off (tier medium)", "cold_start_median": 500.0, "pss_kb": "67566",
    "phases": {
        "scroll2k": {"app": {"work_ms": [5.0, 6.0, 7.0, 20.0], "frames": 4}, "app_fps": 59.0, "app_jank_pct": 0.1},
        "storm2k": {"app": {"work_ms": [9.0, 10.0, 11.0, 12.0], "frames": 3}, "app_fps": 60.0, "app_jank_pct": 0.0},
        "idle2k": {"app": {"work_ms": [0.2], "frames": 1}}}}}}]}


class Scorecard(unittest.TestCase):
    def test_a_row_carries_the_numbers(self):
        r = sc.rows_for("market-watch", DOC, "abc")[0]
        self.assertEqual((r["tier"], r["path"], r["cold_start_ms"], r["idle_frames"]), ("medium", "gpu", 500, 1))
        self.assertEqual(r["pss_mb"], 66.0)
        self.assertEqual(r["storm_fps"], 60.0)
        # work_ms is [p50, p95, p99, max]: p95 is the second value.
        self.assertEqual((r["scroll_p50_ms"], r["scroll_p95_ms"]), (5.0, 6.0))

    def test_missing_tiers_are_listed_not_dropped(self):
        rows = sc.with_missing_tiers("market-watch", sc.rows_for("market-watch", DOC, "abc"), "abc")
        gaps = {r["tier"] for r in rows if r["device"] == "no run"}
        self.assertEqual(gaps, {"low", "high"})
        self.assertIn("**no run**", sc.markdown(rows, "abc"))


if __name__ == "__main__":
    unittest.main()
