"""Workspace, dependency and packaging contracts; parser behavior has Rust tests."""
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

    def test_metadata_and_dependency_boundary(self):
        """Preserve package metadata and use only the approved registry dependency."""
        package, = self.metadata()["packages"]
        self.assertEqual(package["name"], "irig106-core")
        self.assertEqual(package["version"], "0.1.0")
        self.assertEqual(package["edition"], "2024")
        self.assertEqual(package["rust_version"], "1.85")
        self.assertEqual(package["license"], "Apache-2.0")
        dependency, = package["dependencies"]
        self.assertEqual(dependency["name"], "irig106-types")
        self.assertEqual(dependency["req"], "^0.1")
        self.assertIsNone(dependency.get("path"))
        self.assertTrue(dependency["source"].startswith("registry+"))
        self.assertFalse(dependency["uses_default_features"])
        self.assertEqual(dependency["features"], [])

    def test_package_includes_documentation_and_license(self):
        """A nested crate must ship its own README and unchanged license."""
        files = subprocess.check_output(
            ["cargo", "package", "-p", "irig106-core", "--list", "--locked",
             "--allow-dirty"], cwd=ROOT, text=True,
        ).splitlines()
        expected = {".cargo_vcs_info.json", "Cargo.toml", "Cargo.toml.orig", "Cargo.lock", "README.md", "LICENSE"}
        expected.update(f"src/{name}.rs" for name in
                        ["lib", "header", "reader", "error", "integrity", "inspection", "sequence", "pool", "admission", "whole"])
        expected.update(f"tests/{name}.rs" for name in
                        ["primary_header", "reader", "completion", "integrity", "inspection", "sequence", "pool", "whole"])
        expected.update(["examples/inspect.rs", "examples/rss_probe.rs"])
        self.assertEqual(set(files), expected)
        self.assertEqual(
            hashlib.sha256((ROOT / "LICENSE").read_bytes()).digest(),
            hashlib.sha256((CRATE / "LICENSE").read_bytes()).digest(),
        )


if __name__ == "__main__":
    unittest.main()
