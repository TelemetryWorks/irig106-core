"""Small validation oracles; no large measurements on the development machine."""
import unittest
from check_whole_resources import CASES, MemoryCeilingExceeded, validate


class WholeResources(unittest.TestCase):
    def test_reviewer_ceiling_policy_and_boundaries(self):
        sample = dict(phase="cold", pid="1", budget="16777216", used="100", blocks="0", idle="0")
        for case in ["bogus", "refuse", "pressure-baseline", "recovery"]:
            with self.subTest(case=case):
                self.assertEqual(CASES[case], 12)
                validate(sample, 12288, CASES[case] * 1024)
                with self.assertRaises(MemoryCeilingExceeded):
                    validate(sample, 12289, CASES[case] * 1024)
        self.assertEqual(CASES["pressure"], 16 + 8)
        validate(sample, 24576, CASES["pressure"] * 1024)
        with self.assertRaises(MemoryCeilingExceeded):
            validate(sample, 24577, CASES["pressure"] * 1024)
        self.assertEqual((CASES["admit"], CASES["setup"]), (80, 144))

    def test_macos_byte_units_and_fail_closed_reports(self):
        from check_whole_resources import unix_peak_kib
        report = "  0.01 real 0.00 user 0.00 sys\n   12582912  maximum resident set size\n"
        self.assertEqual(unix_peak_kib(report, "darwin"), 12288)
        self.assertEqual(unix_peak_kib(report.replace("12582912", "12582913"), "darwin"), 12289)
        self.assertEqual(unix_peak_kib("12288\n", "linux"), 12288)
        for bad in ["", "not a report", report + report]:
            with self.subTest(bad=bad), self.assertRaises(AssertionError):
                unix_peak_kib(bad, "darwin")

    def test_exact_ceiling_and_fail_closed(self):
        sample = dict(phase="cold", pid="1", budget="16777216", used="100", blocks="0", idle="0")
        validate(sample, 12 * 1024, 12 * 1024)
        for changed, rss in [({"used": "16777217"}, 1), ({"blocks": "1"}, 1), ({}, 12289), ({}, 0)]:
            with self.subTest(changed=changed, rss=rss), self.assertRaises(AssertionError):
                validate(sample | changed, rss, 12288)

    def test_recovery_keeps_one_block(self):
        sample = dict(phase="recovered", pid="1", budget="268435456", used="600000", blocks="1", idle="0")
        validate(sample, 3000, 12288)
        validate(sample | dict(phase="tail", idle="1"), 3000, 12288)
        with self.assertRaises(AssertionError):
            validate(sample | dict(blocks="2", used="1200000"), 3000, 12288)

    def test_coalesced_capacity_is_part_of_used_bytes(self):
        sample = dict(phase="coalesced", pid="1", budget=str(272*1024*1024),
                      blocks="256", idle="0", coalesced=str(128*1024*1024+256),
                      used=str(256*1024*1024+512))
        validate(sample, 260*1024, 272*1024)
        for changed in [dict(coalesced="0"), dict(used=str(128*1024*1024))]:
            with self.subTest(changed=changed), self.assertRaises(AssertionError):
                validate(sample | changed, 260*1024, 272*1024)

    def test_pressure_requires_full_budget_and_trim(self):
        sample = dict(phase="blocked", pid="1", budget="16777216", used="16254000", blocks="31", idle="0")
        validate(sample, 20000, 24576)
        validate(sample | dict(phase="trimmed", blocks="8", idle="8", used="4194400"), 20000, 24576)
        for changed in [{"blocks": "30"}, {"idle": "1"}, {"phase": "trimmed", "idle": "9"}]:
            with self.subTest(changed=changed), self.assertRaises(AssertionError):
                validate(sample | changed, 20000, 24576)


if __name__ == "__main__":
    unittest.main()
