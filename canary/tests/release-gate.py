#!/usr/bin/env python3
"""Offline contracts for the trusted hosted release gate handoff."""

import importlib.util
import json
import re
import shlex
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("release_gate", ROOT / "canary/actions/release_gate.py")
release_gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release_gate)
PUBLISHER_SPEC = importlib.util.spec_from_file_location("release_publish", ROOT / "canary/actions/release_publish.py")
publisher = importlib.util.module_from_spec(PUBLISHER_SPEC)
PUBLISHER_SPEC.loader.exec_module(publisher)


class ReleaseGateTests(unittest.TestCase):
    def test_qualified_platforms_drive_matrix_assets_and_identities(self):
        # The support list is shared data: adding a platform to a harness must change
        # the required identities, and a platform without a canary asset must fail
        # closed instead of collecting a cell whose evidence cannot exist.
        selection = sys.modules["selection"]
        original = dict(selection.HARNESS_PLATFORMS)
        try:
            self.assertEqual(len(release_gate.expected_identities()), 30)
            self.assertEqual(len(release_gate.ASSETS), 4)
            selection.HARNESS_PLATFORMS["codex"] = ("linux", "macos", "windows")
            self.assertIn("windows-codex", release_gate.expected_identities())
            with self.assertRaisesRegex(ValueError, "published canary release asset"):
                release_gate.matrix(None)
        finally:
            selection.HARNESS_PLATFORMS.clear()
            selection.HARNESS_PLATFORMS.update(original)

    def test_workflows_are_manual_serialized_and_trusted(self):
        gate = (ROOT / ".github/workflows/release-gate.yml").read_text()
        recommend = (ROOT / ".github/workflows/recommend-release.yml").read_text()
        for workflow in (gate, recommend):
            self.assertIn("workflow_dispatch:", workflow)
            self.assertIn("cancel-in-progress: false", workflow)
            self.assertIn("github.ref_name == github.event.repository.default_branch", workflow)
            self.assertIn("ref: ${{ github.sha }}", workflow)
            self.assertIn("persist-credentials: false", workflow)
            self.assertIn("group: release-channel-${{ github.repository }}", workflow)
            self.assertNotIn("group: release-channel-${{ github.repository }}-${{ inputs.tag }}", workflow)
        self.assertIn("reportCount", (ROOT / "canary/actions/release_gate.py").read_text())
        self.assertIn("verification_only", gate)
        self.assertIn("contents: write", gate)
        self.assertIn("inputs.mode == 'live' && inputs.verification_only == false", gate)
        self.assertIn('if [ "$VERIFICATION_ONLY" = true ]; then', gate)
        self.assertIn('jq -r .isDraft <<<"$release_json")" = false', gate)
        self.assertIn('jq -r .isDraft <<<"$release_json")" = true', gate)
        self.assertIn("isPrerelease", gate)
        self.assertIn("environment: release-publication", gate)
        self.assertIn("environment: release-publication", recommend)
        self.assertIn("permissions:\n  contents: read", recommend)
        self.assertNotIn("ref: ${{ inputs.tag }}", gate)
        # The hosted runner labels live in the hosted platform table, which every
        # consumer shares.
        self.assertIn('"macos-14"', (ROOT / "canary/actions/selection.py").read_text())
        self.assertIn('test "$(uname -m)" = arm64', gate)
        self.assertIn("--reports-dir reports", gate)
        self.assertNotIn("gh run download", recommend)
        self.assertNotIn("Download durable release evidence", recommend)
        self.assertNotIn("--handoff", recommend)
        self.assertNotIn("--assets-dir", recommend)
        self.assertNotIn("empty-assets", recommend)
        self.assertNotIn("gate_run_id", recommend)
        self.assertIn('--repository "$GITHUB_REPOSITORY"', recommend)
        self.assertIn('--tag "$TAG"', recommend)
        self.assertIn('--tag-commit "$TAG_COMMIT"', recommend)
        self.assertIn('--workflow-commit "$EXPECTED_WORKFLOW_COMMIT"', recommend)
        self.assertIn('--run-id "$CURRENT_RUN_ID"', recommend)
        self.assertIn("--recommend", recommend)

    def test_shell_bodies_have_no_direct_input_interpolation(self):
        gate = (ROOT / ".github/workflows/release-gate.yml").read_text()
        recommend = (ROOT / ".github/workflows/recommend-release.yml").read_text()
        for workflow in (gate, recommend):
            bodies = re.findall(r"(?ms)^\s+run: \|\n(.*?)(?=^\s{6}\S|\Z)", workflow)
            shell = "\n".join(bodies)
            for expression in ("${{ inputs.", "${{ matrix.", "${{ needs."):
                self.assertNotIn(expression, shell)
        malicious = "$(touch /tmp/should-not-run);`echo bad`"
        self.assertNotIn(malicious, gate + recommend)

    def test_workflow_invocations_parse_with_actual_publisher_parser(self):
        publisher.parser().parse_args(shlex.split(
            '--repository Acme/Fork --tag v1.2.3 --tag-commit ' + 'a' * 40
            + ' --workflow-commit ' + 'b' * 40 + ' --run-id run-1 --reports-manifest handoff.json '
              '--assets-dir assets --reports-dir reports --publish'))
        publisher.parser().parse_args(shlex.split(
            '--repository Acme/Fork --tag v1.2.3 --tag-commit ' + 'a' * 40
            + ' --workflow-commit ' + 'b' * 40 + ' --run-id current-1 --recommend'))

    def _fixture(self, count=30):
        directory = Path(tempfile.mkdtemp())
        reports = directory / "reports"
        assets = directory / "assets"
        reports.mkdir()
        assets.mkdir()
        for name in release_gate.ASSETS:
            (assets / name).write_bytes(name.encode())
        sums = []
        for name in release_gate.ASSETS:
            sums.append(f"{release_gate.digest(assets / name)}  {name}")
        (assets / "update-manifest.json").write_text(json.dumps({"schemaVersion": 1,
            "version": "1.2.3", "notesUrl": "https://example.test/", "artifacts": []}) + "\n")
        sums.append(f"{release_gate.digest(assets / 'update-manifest.json')}  update-manifest.json")
        (assets / "SHA256SUMS").write_text("\n".join(sums) + "\n")
        identities = sorted(release_gate.expected_identities())
        for index, identity in enumerate(identities[:count]):
            system, harness = identity.split("-", 1)
            report = {
                "schemaVersion": 2, "runId": "run-1", "trigger": "release", "tier": "release-gate",
                "outcome": "passed", "environment": {"operatingSystem": system, "architecture": "aarch64"},
                "harness": {"id": harness}, "nanHarness": {
                    "source": "commit:" + "a" * 40, "version": "1.2.3",
                    "sha256": release_gate.digest(assets / release_gate.PLATFORM_ASSETS[system]["harness"]),
                }, "checks": [{"name": name, "status": "passed"}
                              for name in release_gate.REQUIRED_CHECKS],
            }
            (reports / f"{system}-{harness}.json").write_text(json.dumps(report))
        return directory, reports, assets

    def _args(self, root, reports, assets):
        return SimpleNamespace(repository="Acme/Fork", tag="v1.2.3", tag_commit="a" * 40,
                               workflow_commit="b" * 40, run_id="run-1", reports_dir=reports,
                               assets_dir=assets, output=root / "handoff.json")

    def test_manifest_contains_exact_full30_and_asset_provenance(self):
        root, reports, assets = self._fixture()
        manifest = release_gate.build_manifest(self._args(root, reports, assets))
        self.assertEqual(manifest["reportCount"], 30)
        self.assertEqual(len(manifest["reports"]), 30)
        self.assertEqual(len(manifest["assets"]), 4)
        self.assertEqual(manifest["attestation"]["sourceRef"], "refs/tags/v1.2.3")

    def test_generated_handoff_is_accepted_by_actual_publisher_validator(self):
        root, reports, assets = self._fixture()
        args = self._args(root, reports, assets)
        release_gate.build_manifest(args)
        value = publisher.validate_handoff(args.output, assets, reports)
        self.assertEqual(value["tagCommit"], args.tag_commit)
        self.assertEqual(value["reportCount"], 30)

    def test_missing_or_cross_source_report_is_rejected(self):
        root, reports, assets = self._fixture(29)
        with self.assertRaisesRegex(ValueError, "exactly 30"):
            release_gate.build_manifest(self._args(root, reports, assets))
        root, reports, assets = self._fixture()
        path = next(reports.glob("*.json"))
        report = json.loads(path.read_text())
        report["nanHarness"]["source"] = "commit:" + "c" * 40
        path.write_text(json.dumps(report))
        with self.assertRaisesRegex(ValueError, "provenance"):
            release_gate.build_manifest(self._args(root, reports, assets))

    def test_tampered_asset_is_rejected(self):
        root, reports, assets = self._fixture()
        (assets / release_gate.ASSETS[0]).write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "asset checksum"):
            release_gate.build_manifest(self._args(root, reports, assets))

    def test_full_release_checksum_manifest_is_preserved(self):
        root, reports, assets = self._fixture()
        value = release_gate.build_manifest(self._args(root, reports, assets))
        self.assertEqual(value["assetManifestSha256"], release_gate.digest(assets / "SHA256SUMS"))

    def test_required_report_checks_are_publishable_semantics(self):
        root, reports, assets = self._fixture()
        path = next(reports.glob("*.json"))
        report = json.loads(path.read_text())
        report["checks"] = [{"name": "install-and-diagnose", "status": "passed"}]
        path.write_text(json.dumps(report))
        with self.assertRaisesRegex(ValueError, "deterministic-conformance"):
            release_gate.build_manifest(self._args(root, reports, assets))

    def test_duplicate_required_checksum_is_rejected(self):
        root, reports, assets = self._fixture()
        with (assets / "SHA256SUMS").open("a") as stream:
            stream.write((assets / "SHA256SUMS").read_text().splitlines()[0] + "\n")
        with self.assertRaisesRegex(ValueError, "duplicate checksum"):
            release_gate.build_manifest(self._args(root, reports, assets))


if __name__ == "__main__":
    unittest.main()
