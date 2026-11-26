"""Small validation oracles; no large measurements on the development machine."""
import unittest
from check_whole_resources import validate


class WholeResources(unittest.TestCase):
    def test_exact_ceiling_and_fail_closed(self):
        sample = dict(phase="cold", pid="1", budget="16777216", used="100", blocks="0", idle="0")
        validate(sample, 32 * 1024, 32 * 1024)
        for changed, rss in [({"used": "16777217"}, 1), ({"blocks": "1"}, 1), ({}, 32769), ({}, 0)]:
            with self.subTest(changed=changed, rss=rss), self.assertRaises(AssertionError):
                validate(sample | changed, rss, 32768)

    def test_recovery_keeps_one_block(self):
        sample = dict(phase="recovered", pid="1", budget="268435456", used="600000", blocks="1", idle="0")
        validate(sample, 3000, 32768)
        validate(sample | dict(phase="tail", idle="1"), 3000, 32768)
        with self.assertRaises(AssertionError):
            validate(sample | dict(blocks="2", used="1200000"), 3000, 32768)

    def test_pressure_requires_full_budget_and_trim(self):
        sample = dict(phase="blocked", pid="1", budget="16777216", used="16254000", blocks="31", idle="0")
        validate(sample, 20000, 32768)
        validate(sample | dict(phase="trimmed", blocks="8", idle="8", used="4194400"), 20000, 32768)
        for changed in [{"blocks": "30"}, {"idle": "1"}, {"phase": "trimmed", "idle": "9"}]:
            with self.subTest(changed=changed), self.assertRaises(AssertionError):
                validate(sample | changed, 20000, 32768)


if __name__ == "__main__":
    unittest.main()
