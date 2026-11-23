"""Release controls: reject invalid evidence without registry mutations."""

import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("pipeline", Path(__file__).with_name("pipeline.py"))
pipeline = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pipeline)


class ReleaseControls(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.previous = Path.cwd()
        os.chdir(self.root)
        self.addCleanup(os.chdir, self.previous)
        self.addCleanup(self.temp.cleanup)
        self.env = patch.dict(os.environ, {"GITHUB_OUTPUT": "", "GITHUB_STEP_SUMMARY": ""})
        self.env.start()
        self.addCleanup(self.env.stop)
        self.identity = patch.object(pipeline, "check_source_identity")
        self.identity.start()
        self.addCleanup(self.identity.stop)
        self.value = "0.2.0"
        self.crate = f"irig106-core-{self.value}.crate"
        self.artifacts = self.root / "artifacts"
        self.artifacts.mkdir()
        (self.artifacts / self.crate).write_bytes(b"frozen crate")
        (self.artifacts / "sbom.cdx.json").write_text('{"bomFormat":"CycloneDX"}')
        self.manifest()

    def manifest(self):
        self.lines = [f"{pipeline.digest(self.artifacts / name)}  {name}" for name in sorted(pipeline.expected_files(self.value))]
        (self.artifacts / "SHA256SUMS").write_text("\n".join(self.lines) + "\n")

    def test_valid_manifest(self):
        self.assertEqual(set(pipeline.validate_bundle(self.artifacts, self.value)), {self.crate, "sbom.cdx.json"})

    def test_gate_event_and_checkout_identity_must_match(self):
        self.identity.stop()
        with patch.dict(os.environ, {"RELEASE_COMMIT": "a" * 40, "GITHUB_SHA": "b" * 40}):
            with patch.object(pipeline, "run", side_effect=["a" * 40, "a" * 40]):
                pipeline.check_source_identity()
            for results in [["b" * 40], ["a" * 40, "b" * 40]]:
                with patch.object(pipeline, "run", side_effect=results), self.assertRaises(ValueError):
                    pipeline.check_source_identity()

    def test_source_mismatch_stops_signature_cli(self):
        (self.artifacts / f"irig106-core-{self.value}.crates.intoto.jsonl").write_text('{}\n')
        with patch.object(pipeline, "check_source_identity", side_effect=ValueError("source mismatch")), \
             patch.object(pipeline.subprocess, "run") as cli:
            with self.assertRaisesRegex(ValueError, "source mismatch"):
                pipeline.verify_attestations(self.artifacts, self.value)
            cli.assert_not_called()

    def test_checksum_mutation_rejected(self):
        (self.artifacts / self.crate).write_bytes(b"changed")
        with self.assertRaises(ValueError):
            pipeline.validate_bundle(self.artifacts, self.value)

    def test_manifest_adversarial_entries(self):
        for bad in [self.lines[0], "\n".join(self.lines + [self.lines[0]]),
                    self.lines[0].replace(self.crate, "../escape") + "\n" + self.lines[1],
                    "g" * 64 + "  " + self.crate, "\n".join(self.lines) + "\n\n"]:
            with self.subTest(bad=bad):
                (self.artifacts / "SHA256SUMS").write_text(bad)
                with self.assertRaises(ValueError):
                    pipeline.validate_bundle(self.artifacts, self.value)

    def test_missing_file_rejected(self):
        (self.artifacts / "sbom.cdx.json").unlink()
        with self.assertRaises(ValueError):
            pipeline.validate_bundle(self.artifacts, self.value)

    def test_symlink_payload_rejected(self):
        outside = self.root / "outside"
        (self.artifacts / self.crate).rename(outside)
        (self.artifacts / self.crate).symlink_to(outside)
        with self.assertRaises(ValueError):
            pipeline.validate_bundle(self.artifacts, self.value)

    def test_unexpected_payload_rejected(self):
        (self.artifacts / "extra.crate").write_bytes(b"extra")
        with self.assertRaises(ValueError):
            pipeline.validate_bundle(self.artifacts, self.value)

    def test_manifest_symlink_rejected(self):
        path = self.artifacts / "SHA256SUMS"
        path.rename(self.root / "manifest")
        path.symlink_to(self.root / "manifest")
        with self.assertRaises(ValueError):
            pipeline.validate_bundle(self.artifacts, self.value)

    def make_record(self, date="2026-11-22", notes="Fixed a release control."):
        Path("CHANGELOG.md").write_text(f"## [{self.value}] - {date}\n\nFixed a release control.\n\n[0.2.0]: https://example.test\n")
        path = Path(f"docs/releases/v{self.value}.md")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f"# irig106-core {self.value}\n\n{notes}\n")
        return path

    def test_committed_dated_notes(self):
        path = self.make_record()
        with patch.object(pipeline, "run", return_value=path.read_text().strip()):
            self.assertEqual(pipeline.release_record(self.value), (path, f"irig106-core {self.value}"))

    def test_missing_notes(self):
        self.make_record().unlink()
        with self.assertRaises(FileNotFoundError):
            pipeline.release_record(self.value)

    def test_invalid_changelog_date(self):
        self.make_record(date="2026-02-30")
        with self.assertRaises(ValueError):
            pipeline.release_record(self.value)

    def test_notes_must_derive_from_changelog(self):
        self.make_record(notes="Unrelated notes")
        with self.assertRaises(ValueError):
            pipeline.release_record(self.value)

    def test_notes_must_be_committed(self):
        self.make_record()
        with patch.object(pipeline, "run", return_value="different committed body"):
            with self.assertRaises(ValueError):
                pipeline.release_record(self.value)

    def test_ci_pages_and_identity(self):
        good = {"head_sha": "a" * 40, "path": ".github/workflows/ci.yml", "conclusion": "success", "event": "push", "head_branch": "main"}
        pages = [{"workflow_runs": []}, {"workflow_runs": [good]}]
        self.assertTrue(pipeline.ci_succeeded(pages, "a" * 40))
        for key, value in [("head_sha", "b" * 40), ("path", "unrelated.yml"), ("conclusion", "failure"), ("event", "pull_request"), ("head_branch", "other")]:
            with self.subTest(key=key):
                self.assertFalse(pipeline.ci_succeeded([{"workflow_runs": [dict(good, **{key: value})]}], "a" * 40))

    def test_dry_run_routes_never_publish_even_from_tag(self):
        for event in ["pull_request", "workflow_dispatch"]:
            with self.subTest(event=event), patch.dict(os.environ, {"GITHUB_EVENT_NAME": event, "GITHUB_REF": "refs/tags/v0.2.0"}), \
                 patch.object(pipeline, "version", return_value=self.value), patch.object(pipeline, "run", return_value="a" * 40), \
                 patch.object(pipeline, "release_record", side_effect=FileNotFoundError("missing future notes")), \
                 patch.object(pipeline, "output") as out, contextlib.redirect_stdout(io.StringIO()):
                pipeline.gate()
                self.assertEqual(out.call_args.kwargs["publish"], "false")

    def tag_run(self, kind="tag", tag="v0.2.0", ci=True, ancestry=0, notes=True, api_kind="tag", api_commit="a" * 40):
        def fake_run(*args, **kwargs):
            if args[0] == "gh":
                if "/git/ref/tags/" in args[-1]:
                    return json.dumps({"object": {"type": api_kind, "sha": "b" * 40}})
                if "/git/tags/" in args[-1]:
                    return json.dumps({"object": {"type": "commit", "sha": api_commit}})
                return json.dumps([{"workflow_runs": [{"head_sha": "a" * 40, "path": ".github/workflows/ci.yml", "conclusion": "success" if ci else "failure", "event": "push", "head_branch": "main"}]}])
            if args[1] == "cat-file":
                return kind
            return "a" * 40
        with patch.dict(os.environ, {"GITHUB_EVENT_NAME": "push", "GITHUB_REF": "refs/tags/" + tag, "GITHUB_REF_NAME": tag, "GITHUB_SHA": "a" * 40}), \
             patch.object(pipeline, "version", return_value=self.value), patch.object(pipeline, "run", side_effect=fake_run), \
             patch.object(pipeline.subprocess, "run", return_value=subprocess.CompletedProcess([], ancestry)), \
             patch.object(pipeline, "release_record", side_effect=None if notes else FileNotFoundError("missing notes")), \
             patch.object(pipeline, "output") as out, contextlib.redirect_stdout(io.StringIO()):
            pipeline.gate()
            self.assertEqual(out.call_args.kwargs["publish"], "true")

    def test_qualified_tag_gate(self):
        self.tag_run()

    def test_tag_gate_rejections(self):
        for options in [{"kind": "commit"}, {"tag": "v0.3.0"}, {"ci": False}, {"ancestry": 1}, {"notes": False}]:
            with self.subTest(options=options), self.assertRaises(ValueError):
                self.tag_run(**options)

    def test_unexpected_event_rejected(self):
        with patch.dict(os.environ, {"GITHUB_EVENT_NAME": "schedule"}), patch.object(pipeline, "version", return_value=self.value), patch.object(pipeline, "run", return_value="a" * 40):
            with self.assertRaises(ValueError):
                pipeline.gate()

    def test_existing_registry_mismatch_is_not_skipped(self):
        with patch.object(pipeline, "registry_metadata", return_value={"checksum": "a" * 64}), patch.object(pipeline, "http_get", return_value=b"different"):
            with self.assertRaises(ValueError):
                pipeline.registry_compare(self.value, self.artifacts / self.crate)

    def test_registry_byte_identity_and_checksum(self):
        raw = (self.artifacts / self.crate).read_bytes()
        with patch.object(pipeline, "registry_metadata", return_value={"checksum": hashlib.sha256(raw).hexdigest()}), patch.object(pipeline, "http_get", return_value=raw), contextlib.redirect_stdout(io.StringIO()):
            pipeline.registry_compare(self.value, self.artifacts / self.crate)

    def test_registry_checksum_rejected_even_when_archive_bytes_match(self):
        raw = (self.artifacts / self.crate).read_bytes()
        metadata = json.dumps({"version": {"num": self.value, "checksum": "0" * 64}}).encode()
        with patch.object(pipeline, "http_get", side_effect=[metadata, raw]) as requests:
            with self.assertRaisesRegex(ValueError, "does not match registry checksum"):
                pipeline.registry_compare(self.value, self.artifacts / self.crate)
            self.assertEqual([call.args[0] for call in requests.call_args_list], [
                f"https://crates.io/api/v1/crates/{pipeline.NAME}/{self.value}",
                f"https://crates.io/api/v1/crates/{pipeline.NAME}/{self.value}/download",
            ])

    def test_repackage_never_compiles(self):
        candidate = Path(f"target/package/{self.crate}")
        candidate.parent.mkdir(parents=True)
        candidate.write_bytes((self.artifacts / self.crate).read_bytes())
        with patch.object(pipeline, "check_rust"), patch.object(pipeline.subprocess, "run") as call, contextlib.redirect_stdout(io.StringIO()):
            pipeline.repackage(self.artifacts, self.value, "1.99.1")
            self.assertEqual(call.call_args.args[0], ["cargo", "+1.99.1", "package", "-p", "irig106-core", "--locked", "--no-verify"])
        candidate.write_bytes(b"different candidate")
        with patch.object(pipeline, "check_rust"), patch.object(pipeline.subprocess, "run"):
            with self.assertRaises(ValueError):
                pipeline.repackage(self.artifacts, self.value, "1.99.1")

    def test_prepare_skips_only_matching_existing_registry_bytes(self):
        with patch.object(pipeline, "verify_attestations"), patch.object(pipeline, "repackage"), patch.object(pipeline, "registry_metadata", return_value={}), patch.object(pipeline, "registry_compare") as comparison, patch.object(pipeline, "output") as out:
            pipeline.prepare(self.artifacts, self.value, "1.99.1")
            comparison.assert_called_once()
            self.assertEqual(out.call_args.kwargs, {"upload": "false"})

    def test_wrong_exact_rust_version_rejected(self):
        with patch.object(pipeline, "run", return_value="rustc 1.100.0 (wrong)"):
            with self.assertRaises(ValueError):
                pipeline.check_rust("1.99.1")
        with self.assertRaises(ValueError):
            pipeline.check_rust("stable")

    def test_known_good_fixture_is_pinned(self):
        fixture = Path(pipeline.__file__).parent / "fixtures/irig106-core-0.0.1.crate"
        self.assertEqual(pipeline.digest(fixture), "0d532861736df546570291e7d9494869f27cd4e9cd75448d74563373804e73b3")

    def test_known_good_digest_constant_is_pinned(self):
        self.assertEqual(pipeline.KNOWN_SHA256, "0d532861736df546570291e7d9494869f27cd4e9cd75448d74563373804e73b3")
        with patch.object(pipeline, "KNOWN_SHA256", "0" * 64), \
             patch.object(pipeline, "registry_compare") as comparison:
            with self.assertRaisesRegex(ValueError, "Pinned known-good fixture digest mismatch"):
                pipeline.known_good()
            comparison.assert_not_called()

    def test_known_good_rejects_changed_fixture_before_registry_access(self):
        with patch.object(pipeline, "digest", return_value="0" * 64), \
             patch.object(pipeline, "registry_compare") as comparison:
            with self.assertRaisesRegex(ValueError, "Pinned known-good fixture digest mismatch"):
                pipeline.known_good()
            comparison.assert_not_called()

    def test_api_tag_must_be_annotated(self):
        path = f"repos/{pipeline.REPO}/git/ref/tags/v{self.value}"
        with patch.object(pipeline, "gh_api", return_value={"object": {"type": "commit", "sha": "a" * 40}}) as api:
            with self.assertRaisesRegex(ValueError, "must be annotated"):
                pipeline.tag_commit(f"v{self.value}")
            api.assert_called_once_with(path)

    def test_api_annotated_tag_resolves_to_commit(self):
        with patch.object(pipeline, "gh_api", side_effect=[
            {"object": {"type": "tag", "sha": "b" * 40}},
            {"object": {"type": "commit", "sha": "a" * 40}},
        ]) as api:
            self.assertEqual(pipeline.tag_commit(f"v{self.value}"), "a" * 40)
            self.assertEqual([c.args[0] for c in api.call_args_list], [
                f"repos/{pipeline.REPO}/git/ref/tags/v{self.value}",
                f"repos/{pipeline.REPO}/git/tags/{'b' * 40}",
            ])

    def test_gate_rejects_lightweight_api_tag_despite_local_annotation(self):
        with self.assertRaisesRegex(ValueError, "Release gate rejected"):
            self.tag_run(api_kind="commit")

    def test_gate_rejects_api_tag_pointing_to_another_commit(self):
        with self.assertRaisesRegex(ValueError, "Release gate rejected"):
            self.tag_run(api_commit="c" * 40)

    def release_case(self, *, moved=False, wrong_body=False, mismatch=False, missing=False, draft=False, new=False, invalid_assets=None, moved_after_assets=False, other_releases=(), post_create_lag=0, created_id=10):
        bundle = f"irig106-core-{self.value}.crates.intoto.jsonl"
        (self.artifacts / bundle).write_text('{}\n')
        path = self.make_record()
        names = sorted(pipeline.expected_files(self.value) | {"SHA256SUMS", bundle})
        metadata = {"id": 10, "tag_name": f"v{self.value}", "name": f"irig106-core {self.value}",
                    "body": "wrong" if wrong_body else path.read_text(), "draft": draft or new,
                    "assets": [] if missing else [{"name": name, "id": i} for i, name in enumerate(names)]}
        if invalid_assets == "unexpected":
            metadata["assets"].append({"name": "unexpected.crate", "id": 99})
        elif invalid_assets == "duplicate":
            metadata["assets"].append(dict(metadata["assets"][0], id=99))
        reads = []
        self.release_creations = []
        self.release_id_reads = []
        self.release_metadata = metadata
        def fake_run(*args, **kwargs):
            if args[0] == "gh":
                if "-X" in args and args[args.index("-X") + 1] == "POST":
                    self.release_creations.append((args, kwargs))
                    return json.dumps({"id": created_id})
                if "/releases/" in args[-1] and args[-1].rsplit("/", 1)[-1].isdigit():
                    self.release_id_reads.append(args[-1])
                    return json.dumps(metadata)
                reads.append(True)
                return json.dumps([list(other_releases) if new and (len(reads) == 1 or post_create_lag) else [metadata, *other_releases]])
            return path.read_text().strip()
        def asset_bytes(args, **kwargs):
            index = int(args[2].rsplit("/", 1)[1])
            return b"changed" if mismatch else (self.artifacts / names[index]).read_bytes()
        with patch.dict(os.environ, {"RELEASE_COMMIT": "a" * 40}), \
             patch.object(pipeline, "verify_attestations") as signatures, \
             patch.object(pipeline, "tag_commit", return_value=("b" if moved else "a") * 40,
                          side_effect=["a" * 40, "b" * 40] if moved_after_assets else None) as tags, \
             patch.object(pipeline, "run", side_effect=fake_run), \
             patch.object(pipeline.time, "sleep") as sleeps, \
             patch.object(pipeline.subprocess, "check_output", side_effect=asset_bytes), \
             patch.object(pipeline.subprocess, "run") as mutations:
            self.release_mutations = mutations
            self.release_tag_checks = tags
            self.release_sleeps = sleeps
            self.release_reads = reads
            pipeline.release_assets(self.artifacts, self.value)
            signatures.assert_called_once_with(self.artifacts.resolve(), self.value)
            return mutations.call_args_list

    def test_new_release_requires_existing_tag_and_committed_notes(self):
        calls = self.release_case(new=True)
        args, kwargs = self.release_creations[0]
        self.assertEqual(args, ("gh", "api", "-X", "POST", f"repos/{pipeline.REPO}/releases", "--input", "-"))
        self.assertEqual(json.loads(kwargs["input"]), {
            "tag_name": "v0.2.0", "target_commitish": "a" * 40,
            "name": "irig106-core 0.2.0", "body": self.make_record().read_text(),
            "draft": True, "prerelease": False, "make_latest": "false",
        })
        self.assertEqual(self.release_tag_checks.call_count, 2)
        self.assertIn("PATCH", calls[-1].args[0])

    def test_creation_id_works_when_releases_list_stays_stale(self):
        calls = self.release_case(new=True, missing=True, post_create_lag=100)
        self.assertEqual(len(self.release_reads), 2)  # Resume discovery and Latest only.
        self.release_sleeps.assert_not_called()
        self.assertEqual(len(self.release_creations), 1)
        self.assertEqual(self.release_id_reads, [f"repos/{pipeline.REPO}/releases/10"] * 2)
        self.assertEqual(sum("POST" in c.args[0] for c in calls), 4)
        self.assertTrue(all("releases/10/" in c.args[0][4] for c in calls[:-1]))
        self.assertIn("releases/10", calls[-1].args[0][-3])

    def test_missing_or_unsafe_creation_id_stops_before_assets(self):
        for value in [None, "10", True, 0, -1]:
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, "release ID"):
                self.release_case(new=True, missing=True, created_id=value)
            self.assertEqual(len(self.release_creations), 1)
            self.assertEqual(self.release_id_reads, [])
            self.release_mutations.assert_not_called()

    def test_resume_ambiguity_fails_before_creation_or_attachment(self):
        with patch.object(pipeline, "release_list", return_value=[{"tag_name": "v0.2.0"}] * 2), \
             self.assertRaisesRegex(ValueError, "Multiple Releases"):
            self.release_case(new=True, missing=True)
        self.assertEqual(self.release_creations, [])
        self.release_mutations.assert_not_called()

    def test_changed_id_metadata_blocks_finalization(self):
        reads = []
        def fetch(path):
            reads.append(path)
            return dict(self.release_metadata, tag_name="other") if len(reads) == 2 else self.release_metadata
        with patch.object(pipeline, "gh_api", side_effect=fetch), self.assertRaisesRegex(ValueError, "Release metadata"):
            self.release_case(new=True, missing=True)
        self.assertEqual(reads, [f"repos/{pipeline.REPO}/releases/10"] * 2)
        self.assertEqual(len(self.release_mutations.call_args_list), 4)
        self.assertTrue(all("POST" in c.args[0] for c in self.release_mutations.call_args_list))

    def test_id_lookup_rejects_wrong_identity_or_new_release_state(self):
        for change in [{"id": 11}, {"tag_name": "other"}, {"name": "other"},
                       {"body": "other"}, {"draft": False}]:
            with self.subTest(change=change), patch.object(pipeline, "gh_api", side_effect=lambda path: dict(self.release_metadata, **change)), \
                 self.assertRaisesRegex(ValueError, "Release metadata"):
                self.release_case(new=True, missing=True)
            self.assertEqual(len(self.release_creations), 1)
            self.release_mutations.assert_not_called()

    def test_single_crate_manifest_scope(self):
        Path("Cargo.toml").write_text('[workspace]\nmembers=["crates/irig106-core"]\n[workspace.package]\nversion="0.2.0"\n')
        manifest = Path("crates/irig106-core/Cargo.toml")
        manifest.parent.mkdir(parents=True)
        valid = '[package]\nname="irig106-core"\nversion.workspace=true\npublish=["crates-io"]\n'
        manifest.write_text(valid)
        self.assertEqual(pipeline.version(), self.value)
        for text in [valid.replace('publish=["crates-io"]', 'publish=false'),
                     valid.replace('version.workspace=true', 'version="0.3.0"'),
                     valid.replace('name="irig106-core"', 'name="other"')]:
            manifest.write_text(text)
            with self.assertRaises(ValueError):
                pipeline.version()
        manifest.write_text(valid)
        Path("Cargo.toml").write_text('[workspace]\nmembers=["crates/irig106-core","unexpected"]\n[workspace.package]\nversion="0.2.0"\n')
        with self.assertRaises(ValueError):
            pipeline.version()

    def test_existing_release_assets_retained_without_asset_writes(self):
        calls = self.release_case()
        self.assertEqual(len(calls), 1)
        self.assertIn("PATCH", calls[0].args[0])
        self.assertEqual(json.loads(calls[0].kwargs["input"])["make_latest"], "true")

    def test_existing_asset_mismatch_rejected(self):
        with self.assertRaises(ValueError):
            self.release_case(mismatch=True)

    def test_existing_release_notes_mismatch_rejected(self):
        with self.assertRaises(ValueError):
            self.release_case(wrong_body=True)

    def test_moved_remote_tag_rejected(self):
        with self.assertRaises(ValueError):
            self.release_case(moved=True)

    def test_missing_release_assets_uploaded_without_clobber(self):
        calls = self.release_case(missing=True)
        self.assertEqual(len(calls), 5)
        self.assertTrue(all("POST" in call.args[0] and "--clobber" not in call.args[0] for call in calls[:-1]))
        self.assertIn("PATCH", calls[-1].args[0])

    def test_resumed_draft_uses_numeric_api(self):
        calls = self.release_case(draft=True)
        self.release_sleeps.assert_not_called()
        self.assertEqual(self.release_creations, [])
        self.assertEqual(self.release_id_reads, [f"repos/{pipeline.REPO}/releases/10"] * 2)
        self.assertEqual(len(calls), 1)
        self.assertIn("PATCH", calls[0].args[0])
        self.assertIn("releases/10", calls[0].args[0][-3])

    def test_second_tag_move_check_blocks_undrafting(self):
        with self.assertRaisesRegex(ValueError, "Remote tag moved before publishing"):
            self.release_case(draft=True, moved_after_assets=True)
        self.assertEqual(self.release_tag_checks.call_count, 2)
        self.release_mutations.assert_not_called()

    def test_latest_policy_uses_semver_not_creation_time(self):
        stable = lambda tag, **extra: {"tag_name": tag, "draft": False, "prerelease": False, **extra}
        cases = [
            ("0.2.0", [], (False, True)),
            ("1.10.0", [stable("v1.9.9")], (False, True)),
            ("1.9.9", [stable("v1.10.0", created_at="2000-01-01")], (False, False)),
            ("2.0.0-rc.1", [stable("v1.10.0")], (True, False)),
            ("1.10.0", [stable("v9.0.0-rc.1"), stable("v8.0.0", draft=True)], (False, True)),
            ("1.10.0+metadata", [stable("v1.10.0+other"), stable("nightly")], (False, True)),
            ("1.10.0", [stable("v9.0.0", prerelease=True)], (False, True)),
        ]
        for value, releases, expected in cases:
            with self.subTest(value=value, releases=releases):
                self.assertEqual(pipeline.latest_policy(value, releases), expected)

    def test_older_release_does_not_take_latest(self):
        calls = self.release_case(other_releases=[{"tag_name": "v1.0.0", "draft": False, "prerelease": False}])
        self.assertEqual(json.loads(calls[-1].kwargs["input"])["make_latest"], "false")

    def test_prerelease_flag_is_set_and_latest_is_false(self):
        self.value = "0.2.0-rc.1"
        self.crate = f"irig106-core-{self.value}.crate"
        next(self.artifacts.glob('*.crate')).rename(self.artifacts / self.crate)
        self.manifest()
        calls = self.release_case(new=True)
        self.assertTrue(json.loads(self.release_creations[0][1]["input"])["prerelease"])
        payload = json.loads(calls[-1].kwargs["input"])
        self.assertTrue(payload["prerelease"])
        self.assertEqual(payload["make_latest"], "false")

    def test_prepare_verifies_before_repackaging(self):
        steps = []
        with patch.object(pipeline, "verify_attestations", side_effect=lambda *a: steps.append("verify")), \
             patch.object(pipeline, "repackage", side_effect=lambda *a: steps.append("repackage")), \
             patch.object(pipeline, "registry_metadata", return_value=None), patch.object(pipeline, "output") as output:
            pipeline.prepare(self.artifacts, self.value, "1.99.1")
        self.assertEqual(steps, ["verify", "repackage"])
        self.assertEqual(output.call_args.kwargs, {"upload": "true"})

    def test_bad_signature_blocks_prepare_before_upload_decision(self):
        with patch.object(pipeline, "verify_attestations", side_effect=subprocess.CalledProcessError(1, 'gh')), \
             patch.object(pipeline, "repackage") as repackage, patch.object(pipeline, "registry_metadata") as registry, \
             patch.object(pipeline, "output") as output:
            with self.assertRaises(subprocess.CalledProcessError):
                pipeline.prepare(self.artifacts, self.value, "1.99.1")
            repackage.assert_not_called()
            registry.assert_not_called()
            output.assert_not_called()

    def test_bad_signature_blocks_release_creation_and_attachment(self):
        with patch.object(pipeline, "verify_attestations", side_effect=subprocess.CalledProcessError(1, 'gh')), \
             patch.object(pipeline, "tag_commit") as tag, patch.object(pipeline.subprocess, "run") as mutations:
            with self.assertRaises(subprocess.CalledProcessError):
                pipeline.release_assets(self.artifacts, self.value)
            tag.assert_not_called()
            mutations.assert_not_called()

    def test_missing_sidecar_fails_before_signature_cli(self):
        with patch.object(pipeline.subprocess, "run") as verify:
            with self.assertRaisesRegex(ValueError, "Missing frozen provenance bundle"):
                pipeline.verify_attestations(self.artifacts, self.value)
            verify.assert_not_called()

    def test_invalid_existing_asset_names_fail_before_mutation(self):
        for kind in ["unexpected", "duplicate"]:
            with self.subTest(kind=kind):
                with self.assertRaisesRegex(ValueError, "Duplicate/unexpected existing Release assets"):
                    self.release_case(invalid_assets=kind)
                self.release_mutations.assert_not_called()

    def test_verification_binds_each_attested_file_to_release_commit(self):
        (self.artifacts / f"irig106-core-{self.value}.crates.intoto.jsonl").write_text('{}\n')
        commit = "1234" * 10
        with patch.object(pipeline, "registry_compare"), patch.object(pipeline.subprocess, "run") as verify, \
             patch.object(pipeline, "http_get", return_value=b'{"version":"0.2.0","doc_status":true}'), \
             patch.dict(os.environ, {"RELEASE_COMMIT": commit}), contextlib.redirect_stdout(io.StringIO()):
            pipeline.verify_publication(self.artifacts, self.value)
        self.assertEqual(verify.call_count, 2)
        self.assertEqual({Path(c.args[0][3]).name for c in verify.call_args_list}, pipeline.expected_files(self.value))
        for call in verify.call_args_list:
            command = call.args[0]
            self.assertEqual(command[:3], ["gh", "attestation", "verify"])
            self.assertEqual(command.count("--source-digest"), 1)
            self.assertEqual(command[command.index("--source-digest") + 1], commit)
            self.assertEqual(command[command.index("--source-ref") + 1], f"refs/tags/v{self.value}")
            self.assertEqual(command[command.index("--signer-workflow") + 1], pipeline.WORKFLOW)
            self.assertTrue(call.kwargs["check"])

    def test_docsrs_pending_never_passes(self):
        (self.artifacts / f"irig106-core-{self.value}.crates.intoto.jsonl").write_text('{}\n')
        with patch.object(pipeline, "registry_compare"), patch.object(pipeline.subprocess, "run"), \
             patch.object(pipeline, "http_get", return_value=b'{"version":"0.2.0","doc_status":false}'), \
             patch.object(pipeline.time, "sleep"), patch.dict(os.environ, {"RELEASE_COMMIT": "a" * 40}), contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(ValueError, "pending is not passed"):
                pipeline.verify_publication(self.artifacts, self.value)


if __name__ == "__main__":
    unittest.main()
