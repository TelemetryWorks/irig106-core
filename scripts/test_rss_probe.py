"""Small fixture and ceiling controls only; never measures RSS locally."""
from pathlib import Path
import tempfile
import unittest

from check_rss import make_fixture, expected_output, validate


class RssContract(unittest.TestCase):
    def test_small_fixture_has_dense_packets_gaps_and_tail(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "small.ch10"
            make_fixture(path, 256, 32, 3, 64, 7)
            data = path.read_bytes()  # Small unit fixture only.
        self.assertEqual(len(data), 28781)
        self.assertEqual(data[:24].hex(), "25eb0000400000002800000000ff00090000000000008df3")
        self.assertEqual(data[24:28], bytes([1, 2, 3, 4]))
        self.assertEqual(data[28:64], bytes(36))  # Complete declared DATA, not a sparse/truncated packet.
        self.assertEqual(data[64:88].hex(), "25eb00001c000000040000000000000900000000000045f4")
        self.assertEqual(data[64 + 255 * 28 + 13], 255)
        self.assertEqual(data[64 + 255 * 28 + 22:64 + 255 * 28 + 24], bytes.fromhex("45f3"))
        self.assertEqual(data[64 + 256 * 28:64 + 256 * 28 + 7], b"\xff" * 7)
        self.assertEqual(data[64 + 256 * 28 + 7 + 13], 0)
        self.assertEqual(data[-32:], b"\0" * 32)

    def test_exact_rss_boundary_and_invalid_measurements(self):
        output = expected_output(256, 32, 3, 64, 7)
        self.assertEqual(output, "packets=1025 data_bytes=4136 packet_bytes=28736 recoveries=3 recovered=13 tail=32 tail_scanned=8 unverified=3 max_packet=64 status=Incomplete")
        self.assertEqual(validate(output, "32768\n", 256, 32, 3, 64, 7), 32768)
        for report in ["32769", "0", "-1", "", "not GNU time"]:
            with self.subTest(report=report), self.assertRaises((AssertionError, ValueError)):
                validate(output, report, 256, 32, 3, 64, 7)

    def test_early_stop_cannot_pass_low_rss(self):
        with self.assertRaises(AssertionError):
            validate("packets=1", "2048", 256, 32, 3, 64, 7)


if __name__ == "__main__":
    unittest.main()
