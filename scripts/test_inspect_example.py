"""Execute the built public example with small independent stdin fixtures."""
from pathlib import Path
import subprocess
import sys


def frame(sequence):
    b = bytearray.fromhex(
        "25 eb 01 00 1c 00 00 00 04 00 00 00 00 00 00 09 "
        "00 00 00 00 00 00 00 00 01 02 03 04"
    )
    b[13] = sequence
    checksum = sum(int.from_bytes(b[i:i + 2], "little") for i in range(0, 22, 2))
    b[22:24] = (checksum & 0xFFFF).to_bytes(2, "little")
    return bytes(b)


def main(binary):
    for name, data, code, output, diagnostic in [
        ("wrap", frame(255) + frame(0), 0,
         ["packets=2 data_bytes=8 packet_bytes=56", "status=Complete"], None),
        ("recovery", frame(8) + b"\xff" * 4096 + frame(9), 0,
         ["packets=2", "status=Complete", "recording_index: Unknown", "recording_events: Unknown"],
         "Unverified"),
        ("empty", b"", 3, ["packets=0", "status=Incomplete"], "no complete Chapter 10 packet"),
    ]:
        result = subprocess.run([str(binary)], input=data, capture_output=True, timeout=10)
        stdout, stderr = result.stdout.decode(), result.stderr.decode()
        assert result.returncode == code, (name, result.returncode, stdout, stderr)
        assert all(text in stdout for text in output), (name, stdout)
        assert diagnostic in stderr if diagnostic else not stderr, (name, stderr)
        print(f"{name}: exit code, output and findings pass")


if __name__ == "__main__":
    main(Path(sys.argv[1]).resolve())
