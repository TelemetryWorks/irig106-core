"""CI whole-reader RSS/Windows private-commit gates. Never measures Cargo/Python."""
import json
import os
from pathlib import Path
import queue
import signal
import subprocess
import sys
import tempfile
import threading

CASES = {"bogus": 32, "admit": 80, "setup": 144, "refuse": 32, "pressure": 32}
PHASES = {"bogus": ["cold", "short"], "admit": ["cold", "accepted", "trimmed"],
          "setup": ["cold", "accepted", "trimmed"], "refuse": ["cold", "refused"],
          "pressure": ["cold", "blocked", "trimmed"]}


class MemoryCeilingExceeded(AssertionError):
    pass


def validate(sample, memory_kib, ceiling_kib):
    if memory_kib > ceiling_kib:
        raise MemoryCeilingExceeded((memory_kib, ceiling_kib))
    assert memory_kib > 0, memory_kib
    assert 0 <= int(sample["used"]) <= int(sample["budget"]), sample
    blocks, idle = int(sample["blocks"]), int(sample["idle"])
    assert 0 <= idle <= blocks, sample
    assert int(sample["used"]) >= blocks * 524288, sample
    phase = sample["phase"]
    if phase in ["cold", "refused"]:
        assert blocks == 0, sample
    elif phase == "short":
        assert blocks <= 1, sample
    elif phase == "blocked":
        assert blocks == 31 and idle == 0, sample
    elif phase == "trimmed":
        assert blocks == idle and idle <= 8, sample


def measure(binary, case):
    ceiling = CASES[case] * 1024
    with tempfile.TemporaryDirectory(prefix="core-whole-memory-") as tmp:
        report = Path(tmp) / "rss.txt"
        command = [str(binary), case]
        if sys.platform == "linux":
            command = ["/usr/bin/time", "-f", "%M", "-o", str(report)] + command
        p = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, text=True)
        lines = queue.Queue()
        def collect():
            for line in p.stdout:
                lines.put(line)
            lines.put(None)
        collector = threading.Thread(target=collect, daemon=True)
        collector.start()
        samples, peak = [], 0
        try:
            while (line := lines.get(timeout=40)) is not None:
                sample = dict(field.split("=", 1) for field in line.split())
                samples.append(sample)
                # GNU time supplies whole-lifetime RSS after exit; validate counts now.
                memory = 1
                if sys.platform == "win32":
                    pid = int(sample["pid"])
                    script = (f"$p=Get-Process -Id {pid}; $p.Refresh(); "
                              "@{private=$p.PrivateMemorySize64; working=$p.WorkingSet64; "
                              "peak_commit=$p.PeakPagedMemorySize64; peak_working=$p.PeakWorkingSet64} "
                              "| ConvertTo-Json -Compress")
                    r = subprocess.run(["powershell", "-NoProfile", "-Command", script],
                                       capture_output=True, text=True, timeout=15, check=True)
                    counters = json.loads(r.stdout)
                    assert all(int(n) > 0 for n in counters.values()), counters
                    memory = (max(counters.values()) + 1023) // 1024
                    print(f"case={case} phase={sample['phase']} windows_bytes={counters}", flush=True)
                validate(sample, memory, ceiling)
                peak = max(peak, memory)
                print(line.strip(), flush=True)
                p.stdin.write("\n")
                p.stdin.flush()
            assert p.wait(timeout=10) == 0, p.stderr.read()
            assert not p.stderr.read()
            assert [s["phase"] for s in samples] == PHASES[case], samples
            if sys.platform == "linux":
                peak = int(report.read_text().strip())
                for sample in samples:
                    validate(sample, peak, ceiling)
            assert sys.platform in ["linux", "win32"], "measurement unavailable"
            print(f"case={case} peak_kib={peak} ceiling_kib={ceiling}", flush=True)
        finally:
            if p.poll() is None:
                if sys.platform == "linux" and samples:
                    try:
                        os.kill(int(samples[0]["pid"]), signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                p.kill()
                p.wait(timeout=5)
            collector.join(timeout=5)
            assert not collector.is_alive()


def main(binary):
    if os.environ.get("CI") != "true":
        raise SystemExit("CI-only measurements; use test_whole_resources.py locally")
    for case in sys.argv[2:] or CASES:
        measure(binary, case)


if __name__ == "__main__":
    main(Path(sys.argv[1]).resolve())
