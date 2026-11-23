"""CI-only isolated core RSS: GNU time wraps the prebuilt Rust process alone."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

PACKETS_PER_SEGMENT = 1_048_576
PADDING_BYTES = 16 * 1024 * 1024
RECOVERY_BYTES = 4096
LARGE_PACKET_BYTES = 64 * 1024 * 1024
LARGE_GAP_BYTES = 48 * 1024 * 1024
CEILING_KIB = 32 * 1024


def write_fill(out, value, count):
    chunk = bytes([value]) * (64 * 1024)
    chunks, remainder = divmod(count, len(chunk))
    for _ in range(chunks):
        out.write(chunk)
    out.write(chunk[:remainder])


def make_fixture(path, packets_per_segment, padding, gap, large_packet, large_gap):
    frames = []
    for sequence in range(256):
        b = bytearray([0x25, 0xEB, 0, 0, 28, 0, 0, 0, 4, 0, 0, 0,
                       0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4])
        b[13] = sequence
        checksum = sum(int.from_bytes(b[i:i + 2], "little") for i in range(0, 22, 2))
        b[22:24] = (checksum & 0xFFFF).to_bytes(2, "little")
        frames.append(bytes(b))
    cycle = b"".join(frames)
    chunk = cycle * 64  # 448 KiB, never a whole-recording allocation.
    with path.open("wb") as out:
        large = bytearray(frames[255])  # First dense packet's sequence0 follows 255.
        large[4:8] = large_packet.to_bytes(4, "little")
        large[8:12] = (large_packet - 24).to_bytes(4, "little")
        checksum = sum(int.from_bytes(large[i:i + 2], "little") for i in range(0, 22, 2))
        large[22:24] = (checksum & 0xFFFF).to_bytes(2, "little")
        out.write(large)  # Header plus first four DATA bytes.
        write_fill(out, 0, large_packet - 28)  # Complete, physically written DATA.
        for segment in range(4):
            if segment:
                write_fill(out, 0xFF, large_gap if segment == 1 else gap)
            chunks, remainder = divmod(packets_per_segment, 256 * 64)
            for _ in range(chunks):
                out.write(chunk)
            cycles, remainder = divmod(remainder, 256)
            out.write(cycle * cycles + cycle[:remainder * 28])
        write_fill(out, 0, padding)


def expected_output(packets, padding, gap, large_packet, large_gap):
    return (f"packets={4 * packets + 1} data_bytes={16 * packets + large_packet - 24} "
            f"packet_bytes={112 * packets + large_packet} "
            f"recoveries=3 recovered={large_gap + 2 * gap} tail={padding} tail_scanned={padding - 24} "
            f"unverified=3 max_packet={large_packet} status=Incomplete")


def validate(stdout, report, packets, padding, gap, large_packet, large_gap):
    assert stdout.strip() == expected_output(packets, padding, gap, large_packet, large_gap), stdout
    rss_kib = int(report.strip())
    assert 0 < rss_kib <= CEILING_KIB, f"peak RSS {rss_kib} KiB exceeds/invalidates ceiling {CEILING_KIB} KiB"
    return rss_kib


def main(binary):
    if os.environ.get("CI") != "true":
        raise SystemExit("Large RSS measurement is CI-only; use test_rss_probe.py locally")
    assert LARGE_PACKET_BYTES > CEILING_KIB * 1024
    assert LARGE_GAP_BYTES > CEILING_KIB * 1024
    params = (PACKETS_PER_SEGMENT, PADDING_BYTES, RECOVERY_BYTES, LARGE_PACKET_BYTES, LARGE_GAP_BYTES)
    with tempfile.TemporaryDirectory(prefix="irig106-core-rss-") as tmp:
        root = Path(tmp)
        fixture, report = root / "dense.ch10", root / "rss-kib.txt"
        size = 112 * PACKETS_PER_SEGMENT + LARGE_PACKET_BYTES + LARGE_GAP_BYTES + 2 * RECOVERY_BYTES + PADDING_BYTES
        assert shutil.disk_usage(root).free > size + 1024 * 1024, "insufficient fixture space"
        make_fixture(fixture, *params)
        assert fixture.stat().st_size == size
        result = subprocess.run(
            ["/usr/bin/time", "-f", "%M", "-o", str(report), str(binary), str(fixture)],
            capture_output=True, text=True, timeout=180,
        )
        assert result.returncode == 0, (result.returncode, result.stdout, result.stderr)
        assert not result.stderr, result.stderr
        print(result.stdout.strip(), flush=True)
        print(f"fixture_bytes={size} peak_rss_kib={report.read_text().strip()} ceiling_kib={CEILING_KIB}", flush=True)
        validate(result.stdout, report.read_text(), *params)


if __name__ == "__main__":
    main(Path(sys.argv[1]).resolve())
