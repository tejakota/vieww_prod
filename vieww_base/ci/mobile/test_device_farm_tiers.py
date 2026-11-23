"""The phone picker's tiers: `--count` spreads, `--tiers` keeps.

    python3 -I ci/mobile/test_device_farm_tiers.py
"""
import importlib.util
import pathlib
import sys
import unittest

spec = importlib.util.spec_from_file_location("device_farm", pathlib.Path(__file__).with_name("device_farm.py"))
device_farm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(device_farm)


def phones(n):
    return [{"name": f"P{i}", "availability": "HIGHLY_AVAILABLE", "os": "14", "memory": i,
             "cpu": {"architecture": "arm64", "clock": 1}} for i in range(n)]


def fake_aws(devices):
    return lambda *args: {"devices": devices}


class Tiers(unittest.TestCase):
    def pick(self, count, tiers=("low", "mid", "high"), n=10):
        return [(d["name"], d["tier"]) for d in device_farm.pick_devices(fake_aws(phones(n)), [], count, "ANDROID", tiers)]

    def test_default_is_one_of_each(self):
        self.assertEqual(self.pick(3), [("P0", "low"), ("P4", "mid"), ("P9", "high")])

    def test_low_and_high_skip_the_middle(self):
        self.assertEqual(self.pick(3, ("low", "high")), [("P0", "low"), ("P9", "high")])

    def test_only_high_is_the_most_capable(self):
        self.assertEqual(self.pick(3, ("high",)), [("P9", "high")])

    def test_more_phones_widen_the_middle(self):
        self.assertEqual([t for _, t in self.pick(5, ("mid",))], ["mid", "mid", "mid"])

    def test_a_tier_the_spread_lacks_is_refused(self):
        with self.assertRaises(SystemExit) as e:
            self.pick(2, ("mid",))
        self.assertIn("--count 3", str(e.exception))


class OneTierSource(unittest.TestCase):
    """PERF-03: the table and the budgets read the same tier."""

    def test_the_phones_own_tier_wins_over_the_spread_label(self):
        # An iPhone 13 picked as the "low" of a spread whose app resolved
        # `tier medium`: the table says medium, as its cache budgets do.
        result = {"gpu_device": "tier medium, 4.0 GB, 6 cores, Apple A15"}
        self.assertEqual(device_farm.reported_tier(result), "medium")

    def test_a_run_that_reported_nothing_falls_back_to_the_ram_policy_or_none(self):
        self.assertEqual(device_farm.reported_tier({"mem_total_kb": "1913472", "cpu_cores": "8"}), "low")
        self.assertIsNone(device_farm.reported_tier({}))
        self.assertIsNone(device_farm.reported_tier(None))

    def test_medium_and_mid_sort_as_the_same_rung(self):
        order = device_farm.TIER_ORDER
        self.assertEqual(order["medium"], order["mid"])
        self.assertLess(order["low"], order["medium"])
        self.assertLess(order["medium"], order["high"])


if __name__ == "__main__":
    sys.exit(unittest.main())
