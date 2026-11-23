"""Small fixture and ceiling controls only; never measures RSS locally."""
from pathlib import Path
import tempfile
import unittest

from check_rss import make_fixture, expected_output, validate


class RssContract(unittest.TestCase):
    def test_small_fixture_has_dense_packets_gaps_and_tail(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "small.ch10"
            make_fixture(path, 256, 32, 3)
            data = path.read_bytes()  # Small unit fixture only.
        self.assertEqual(len(data), 4 * 256 * 28 + 9 + 32)
        self.assertEqual(data[:24].hex(), "25eb00001c0000000400000000000009000000000000000045f4")
        self.assertEqual(data[255 * 28 + 13], 255)
        self.assertEqual(data[255 * 28 + 22:255 * 28 + 24], bytes.fromhex("45f3"))
        self.assertEqual(data[256 * 28:256 * 28 + 3], b"\xff" * 3)
        self.assertEqual(data[256 * 28 + 3 + 13], 0)
        self.assertEqual(data[-32:], b"\0" * 32)

    def test_exact_rss_boundary_and_invalid_measurements(self):
        output = expected_output(256, 32, 3)
        self.assertEqual(validate(output, "32768\n", 256, 32, 3), 32768)
        for report in ["32769", "0", "-1", "", "not GNU time"]:
            with self.subTest(report=report), self.assertRaises((AssertionError, ValueError)):
                validate(output, report, 256, 32, 3)

    def test_early_stop_cannot_pass_low_rss(self):
        with self.assertRaises(AssertionError):
            validate("packets=1", "2048", 256, 32, 3)


if __name__ == "__main__":
    unittest.main()
