"""Independent stdin fixtures for the bounded whole-packet consumer example."""
from pathlib import Path
import subprocess
import sys


def frame(channel=1, kind=9, sequence=0, length=28):
    b = bytearray.fromhex(
        "25 eb 01 00 1c 00 00 00 04 00 00 00 00 00 00 09 "
        "00 00 00 00 00 00 00 00 01 02 03 04"
    )
    b[2:4] = channel.to_bytes(2, "little")
    b[4:8] = length.to_bytes(4, "little")
    b[8:12] = (length - 24).to_bytes(4, "little")
    b[13], b[15] = sequence, kind
    checksum = sum(int.from_bytes(b[i:i + 2], "little") for i in range(0, 22, 2))
    b[22:24] = (checksum & 0xFFFF).to_bytes(2, "little")
    return bytes(b) + bytes(max(0, length - 28))


def run(binary, name, data, code, expected, diagnostic=None):
    r = subprocess.run([str(binary)], input=data, capture_output=True, timeout=15)
    out, err = r.stdout.decode(), r.stderr.decode()
    assert r.returncode == code, (name, r.returncode, out, err)
    assert all(text in out for text in expected), (name, out)
    assert diagnostic in err if diagnostic else not err, (name, err)
    return out


def main(binary):
    # Setup/time notifications precede dispatch; revisions are carried by value.
    data = (frame(0, 1) + frame(1, 9, 255) + frame(2, 0x11)
            + frame(1, 9, 0) + frame(2, 9, 1))
    out = run(binary, "ordered", data, 0, [
        "control offset=0 revision=1", "control offset=56 revision=2",
        "dispatch offset=28 channel=1 worker=1 revision=1 sequence=First",
        "dispatch offset=84 channel=1 worker=1 revision=2 sequence=Contiguous",
        "worker=0 packets=3 data_bytes=12 revision=2",
        "worker=1 packets=2 data_bytes=8 revision=2",
        "packets=5 status=Complete", "recording_index: Absent",
    ])
    assert out.index("control offset=56") < out.index("dispatch offset=56")
    run(binary, "burst", b"".join(frame(i % 2, 9, i // 2) for i in range(64)), 0,
        ["worker=0 packets=32", "worker=1 packets=32", "packets=64 status=Complete"])
    run(binary, "recovery", frame(sequence=8) + b"\xff" * 4096 + frame(sequence=9), 0,
        ["sequence=Unverified", "packets=2 status=Complete", "recording_index: Unknown"])
    run(binary, "tail", frame() + b"\xff", 0,
        ["worker=1 packets=1", "packets=1 status=Incomplete", "recording_index: Unknown"],
        "terminal tail")
    run(binary, "empty", b"", 3, ["packets=0 status=Incomplete"], "no complete Chapter 10 packet")
    run(binary, "truncated", frame() + frame(sequence=1)[:26], 0,
        ["packets=1 status=Incomplete"], "terminal tail")
    # A multi-block oversoft stream is not authorized merely by using workers.
    run(binary, "oversoft", frame() + frame(length=524292), 3,
        ["packets=1 status=Incomplete", "worker=1 packets=1"], "corroboration")
    print("ordered revisions, affine routing, bounded bursts, recovery, tails and refusal pass")


if __name__ == "__main__":
    main(Path(sys.argv[1]).resolve())
