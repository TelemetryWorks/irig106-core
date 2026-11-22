"""Slice 1 layout/packaging contract; no parser behavior is claimed."""
import hashlib
import json
from pathlib import Path
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[1]
CRATE = ROOT / "crates/irig106-core"


class WorkspaceContract(unittest.TestCase):
    def metadata(self):
        return json.loads(subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--locked", "--format-version=1"],
            cwd=ROOT, text=True,
        ))

    def test_virtual_root_and_single_library_member(self):
        """Owner layout decision; no orphan root package or duplicate library."""
        data = self.metadata()
        self.assertIsNone(data["resolve"])
        self.assertEqual(len(data["workspace_members"]), 1)
        package, = data["packages"]
        self.assertEqual(package["manifest_path"], str(CRATE / "Cargo.toml"))
        self.assertFalse((ROOT / "src").exists())
        self.assertEqual(package["targets"][0]["src_path"], str(CRATE / "src/lib.rs"))
        self.assertEqual(data["workspace_members"], data["workspace_default_members"])

    def test_placeholder_and_metadata_are_preserved(self):
        """Slice 1 moves the scaffold; it does not implement or release 0.1."""
        package, = self.metadata()["packages"]
        self.assertEqual(package["name"], "irig106-core")
        self.assertEqual(package["version"], "0.0.1")
        self.assertEqual(package["edition"], "2024")
        self.assertEqual(package["rust_version"], "1.85")
        self.assertEqual(package["license"], "Apache-2.0")
        self.assertEqual(package["dependencies"], [])
        # SHA-256 of src/lib.rs at base 38ac070; works in shallow CI checkouts.
        self.assertEqual(
            hashlib.sha256((CRATE / "src/lib.rs").read_bytes()).hexdigest(),
            "c04c976b49d9540ebad87db788a0887ce2806ffef3f1ea64736bb845e6c7568b",
        )

    def test_package_includes_documentation_and_license(self):
        """A nested crate must ship its own README and unchanged license."""
        files = subprocess.check_output(
            ["cargo", "package", "-p", "irig106-core", "--list", "--locked",
             "--allow-dirty"], cwd=ROOT, text=True,
        ).splitlines()
        for name in ["Cargo.toml", "Cargo.lock", "src/lib.rs", "README.md", "LICENSE"]:
            self.assertIn(name, files)
        self.assertEqual(
            hashlib.sha256((ROOT / "LICENSE").read_bytes()).digest(),
            hashlib.sha256((CRATE / "LICENSE").read_bytes()).digest(),
        )


if __name__ == "__main__":
    unittest.main()
