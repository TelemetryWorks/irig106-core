"""Install exact upstream workflow-audit binaries, verifying pinned SHA-256 first."""
import hashlib
from pathlib import Path
import platform
import sys
import tarfile
import urllib.request

ARCHIVES = {
    "x86_64": [
        ("rhysd/actionlint", "v1.7.12", "actionlint_1.7.12_linux_amd64.tar.gz", "8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8", "actionlint"),
        ("zizmorcore/zizmor", "v1.33.1", "zizmor-x86_64-unknown-linux-gnu.tar.gz", "3e6a301c7a820a2501915f5a9de8c0486da82bdec7965f068477538a98743d43", "zizmor"),
    ],
    "aarch64": [
        ("rhysd/actionlint", "v1.7.12", "actionlint_1.7.12_linux_arm64.tar.gz", "325e971b6ba9bfa504672e29be93c24981eeb1c07576d730e9f7c8805afff0c6", "actionlint"),
        ("zizmorcore/zizmor", "v1.33.1", "zizmor-aarch64-unknown-linux-gnu.tar.gz", "aa487c792a7cbbe86e4f8dccd3b66de33b3b51813c090aaa0842d7ea08a3fc35", "zizmor"),
    ],
}


def main(root):
    root.mkdir(parents=True, exist_ok=True)
    for repo, version, name, digest, binary in ARCHIVES[platform.machine()]:
        request = urllib.request.Request(f"https://github.com/{repo}/releases/download/{version}/{name}",
                                         headers={"User-Agent": "TelemetryWorks-workflow-audit"})
        with urllib.request.urlopen(request, timeout=60) as response:
            raw = response.read()
        if hashlib.sha256(raw).hexdigest() != digest:
            raise ValueError(f"Upstream binary checksum mismatch: {name}")
        archive = root / name
        archive.write_bytes(raw)
        with tarfile.open(archive) as tar:
            member, = [m for m in tar.getmembers() if Path(m.name).name == binary and m.isfile()]
            target = root / binary
            target.write_bytes(tar.extractfile(member).read())
            target.chmod(0o755)
        print(f"Verified {binary} {version}: {digest}")


if __name__ == "__main__":
    main(Path(sys.argv[1]))
