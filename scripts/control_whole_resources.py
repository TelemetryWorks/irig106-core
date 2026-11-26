"""One-off real library mutants; isolated build, restore source, require RSS failure."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from check_whole_resources import measure, MemoryCeilingExceeded

assert os.environ.get("CI") == "true" and sys.platform == "linux"
source = Path("crates/irig106-core/src/whole.rs")
original = source.read_text()
cases = [
    ("declared_preallocation", "bogus", "        let p = header.packet_length();", '''
        let p = header.packet_length();
        let mut regression = Vec::new();
        regression.try_reserve_exact(p as usize).map_err(PoolError::from)?;
        regression.resize(p as usize, 0x5a);
        std::hint::black_box(&regression);
'''),
    ("unbudgeted_retention", "admit", "        self.info = None;", '''
        let mut regression = Vec::new();
        for span in self.frame_spans() { regression.extend_from_slice(span); }
        std::mem::forget(regression);
        self.info = None;
'''),
]
try:
    for name, case, old, new in cases:
        assert original.count(old) == 1
        source.write_text(original.replace(old, new))
        with tempfile.TemporaryDirectory(prefix="core-control-target-") as tmp:
            subprocess.run(["cargo", "build", "--release", "--locked", "-p", "irig106-core",
                            "--example", "whole_resources", "--target-dir", tmp],
                           check=True, timeout=180)
            try:
                measure(Path(tmp) / "release/examples/whole_resources", case)
            except MemoryCeilingExceeded as error:
                print(f"control={name} KILLED peak_and_ceiling_kib={error}", flush=True)
            else:
                raise AssertionError(f"control {name} survived")
finally:
    source.write_text(original)
