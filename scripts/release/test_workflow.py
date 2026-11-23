"""Source-level workflow contracts supplement actionlint and runtime controls."""
from pathlib import Path
import re
import unittest

WORKFLOW = Path(__file__).resolve().parents[2] / ".github/workflows/trusted-publishing.yml"


class WorkflowContract(unittest.TestCase):
    def setUp(self):
        self.text = WORKFLOW.read_text()
        self.jobs = dict(re.findall(r"^  ([a-z-]+):\n(.*?)(?=^  [a-z-]+:|\Z)",
                                    self.text.split("jobs:\n", 1)[1], re.M | re.S))

    def test_base_jobs_and_privileged_independent_guards(self):
        self.assertEqual(set(self.jobs), {"gate", "build", "attest", "publish", "github-release", "verify", "publish-path-check"})
        for job in ["attest", "publish", "github-release", "verify"]:
            source = self.jobs[job]
            self.assertIn("needs.gate.outputs.publish == 'true'", source)
            for term in ["github.event_name == 'push'", "github.ref_type == 'tag'", "startsWith(github.ref, 'refs/tags/v')"]:
                self.assertIn(term, source.split("steps:", 1)[0])
        self.assertEqual(self.text.count("ref: ${{ github.sha }}"), self.text.count("uses: actions/checkout@"))
        self.assertNotIn("ref: ${{ needs.gate", self.text)

    def test_dry_route_cannot_sign_publish_or_enter_environment(self):
        source = self.jobs["publish-path-check"]
        self.assertIn("needs: [gate, build]", source)
        self.assertIn("if: needs.gate.outputs.publish != 'true'", source)
        self.assertIn("contents: read", source)
        for forbidden in ["environment:", "id-token:", "attestations:", "crates-io-auth", "cargo publish", "attest-build"]:
            self.assertNotIn(forbidden, source)
        self.assertIn("needs.build.outputs.rust-version", source)
        self.assertIn("pipeline.py known-good", source)

    def test_sidecar_selection_and_verification_before_authentication(self):
        for job in ["publish", "github-release", "verify"]:
            self.assertIn("pattern: release-${{ needs.gate.outputs.version }}{,-attestations}", self.jobs[job])
            self.assertIn("merge-multiple: true", self.jobs[job])
        publish = self.jobs["publish"]
        self.assertLess(publish.index("pipeline.py prepare"), publish.index("uses: rust-lang/crates-io-auth-action@"))
        self.assertIn("environment: crates-io", publish)
        self.assertNotIn("overwrite:", self.text)
        self.assertEqual(self.text.count("environment: crates-io"), 1)

    def test_exact_job_permissions_and_dependencies(self):
        expected = {"gate": {"contents: read", "actions: read"}, "build": {"contents: read"},
                    "attest": {"contents: read", "id-token: write", "attestations: write"},
                    "publish": {"contents: read", "id-token: write"},
                    "github-release": {"contents: write"}, "verify": {"contents: read"},
                    "publish-path-check": {"contents: read"}}
        for job, permissions in expected.items():
            block = re.search(r"    permissions:\n((?:      .*\n)+)", self.jobs[job])[1]
            self.assertEqual({line.split("#", 1)[0].strip() for line in block.splitlines()}, permissions)
        for job, needs in {"build": "gate", "attest": "[gate, build]", "publish": "[gate, build, attest]",
                           "github-release": "[gate, publish]", "verify": "[gate, github-release]"}.items():
            self.assertIn(f"needs: {needs}\n", self.jobs[job])


if __name__ == "__main__":
    unittest.main()
