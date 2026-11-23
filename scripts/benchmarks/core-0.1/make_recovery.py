"""Create the measured 8 MiB gap + 8 MiB trailing-padding control."""
from pathlib import Path
import shutil
import sys
root = Path(sys.argv[1])
with (root / "recording.ch10").open("rb") as src, (root / "recovery.ch10").open("wb") as dst:
    dst.write(src.read(1048576))
    dst.write(bytes([0x55]) * 8388608)
    shutil.copyfileobj(src, dst, 1024 * 1024)
    dst.write(bytes([0x55]) * 8388608)
