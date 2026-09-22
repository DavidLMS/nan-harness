#!/usr/bin/env python3
"""Offline contracts for the trusted hosted release gate handoff."""

import importlib.util
import json
import os
import re
import shlex
import sys
import subprocess
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
    def test_release_tag_guard_rejects_duplicates_across_pages_and_api_failures(self):
        script = ROOT / ".github/scripts/check-release-tag.sh"
        with tempfile.TemporaryDirectory() as directory:
            mock = Path(directory) / "gh"
            mock.write_text('#!/bin/sh\ncat "$RELEASE_FIXTURE"\nexit "${API_EXIT:-0}"\n')
            mock.chmod(0o755)
            fixture = Path(directory) / "releases.json"
            env = {**os.environ, "PATH": directory + os.pathsep + os.environ["PATH"],
                   "RELEASE_FIXTURE": str(fixture), "API_EXIT": "0"}
            for pages, mode, succeeds in [
                ([[]], "absent", True),
                ([[]], "unique", False),
                ([[{"tag_name": "v1.2.3"}]], "absent", False),
                ([[{"tag_name": "v0.1.0"}], [{"tag_name": "v1.2.3"}]], "unique", True),
                ([[{"tag_name": "v1.2.3", "draft": True}],
                  [{"tag_name": "v1.2.3", "draft": True}]], "unique", False),
                ([[{"tag_name": "v1.2.3", "draft": False}]], "absent", False),
            ]:
                fixture.write_text(json.dumps(pages))
                result = subprocess.run(["bash", str(script), "Acme/Fork", "v1.2.3", mode],
                                        env=env, capture_output=True)
                self.assertEqual(result.returncode == 0, succeeds, (pages, mode))
            fixture.write_text("[[]]")
            env["API_EXIT"] = "1"
            result = subprocess.run(["bash", str(script), "Acme/Fork", "v1.2.3", "absent"],
                                    env=env, capture_output=True)
            self.assertNotEqual(result.returncode, 0)

    def test_release_workflows_require_unambiguous_tags_and_exact_source_digest(self):
        release = (ROOT / ".github/workflows/release.yml").read_text()
        gate = (ROOT / ".github/workflows/release-gate.yml").read_text()
        guard = 'bash .github/scripts/check-release-tag.sh'
        self.assertLess(release.index(guard), release.index('gh release create'))
        self.assertIn('"$GITHUB_REF_NAME" absent', release)
        self.assertLess(gate.index(guard), gate.index('gh release view'))
        self.assertIn('"$TAG" unique', gate)
        self.assertIn('TAG_COMMIT: ${{ steps.identity.outputs.tag_commit }}', gate)
        self.assertIn('--source-digest "$TAG_COMMIT"', gate)

    def test_qualified_platforms_drive_matrix_assets_and_identities(self):
        # The support list is shared data: adding a platform to a harness must change
        # the required identities, and a platform without a canary asset must fail
        # closed instead of collecting a cell whose evidence cannot exist.
        selection = sys.modules["selection"]
        original = dict(selection.HARNESS_PLATFORMS)
        original_windows_asset = dict(selection.PLATFORM_ASSETS["windows"])
        try:
            self.assertEqual(len(release_gate.expected_identities()), 43)
            self.assertEqual(len(release_gate.ASSETS), 6)
            self.assertIn("windows-codex", release_gate.expected_identities())
            self.assertNotIn("windows-prime-agent", release_gate.expected_identities())
            self.assertNotIn("windows-fx", release_gate.expected_identities())
            selection.PLATFORM_ASSETS["windows"]["canary"] = None
            with self.assertRaisesRegex(ValueError, "published canary release asset"):
                release_gate.matrix(None)
        finally:
            selection.HARNESS_PLATFORMS.clear()
            selection.HARNESS_PLATFORMS.update(original)
            selection.PLATFORM_ASSETS["windows"] = original_windows_asset

    def test_workflows_are_manual_serialized_and_trusted(self):
        gate = (ROOT / ".github/workflows/release-gate.yml").read_text()
        recommend = (ROOT / ".github/workflows/recommend-release.yml").read_text()
        modular = (ROOT / ".github/workflows/cli-release-gate.yml").read_text()
        self.assertIn('name: cli-cell-${{ matrix.system }}-${{ matrix.harness }}\n          overwrite: true', modular)
        for name in ('release-gate-assets', 'release-gate-report-${{ matrix.system }}-${{ matrix.harness }}',
                     'release-gate-handoff'):
            self.assertIn(f'name: {name}\n          overwrite: true', gate)
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
        self.assertNotIn('jq -r .isDraft <<<"$release_json")" = false', gate)
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

    def _fixture(self, count=43):
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
                "outcome": "passed", "environment": {"operatingSystem": system,
                    "architecture": release_gate.PLATFORMS[system]["architecture"]},
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

    def test_manifest_contains_exact_full43_and_asset_provenance(self):
        root, reports, assets = self._fixture()
        manifest = release_gate.build_manifest(self._args(root, reports, assets))
        self.assertEqual(manifest["reportCount"], 43)
        self.assertEqual(len(manifest["reports"]), 43)
        self.assertEqual(len(manifest["assets"]), 6)
        self.assertEqual(manifest["attestation"]["sourceRef"], "refs/tags/v1.2.3")

    def test_generated_handoff_is_accepted_by_actual_publisher_validator(self):
        root, reports, assets = self._fixture()
        args = self._args(root, reports, assets)
        release_gate.build_manifest(args)
        value = publisher.validate_handoff(args.output, assets, reports)
        self.assertEqual(value["tagCommit"], args.tag_commit)
        self.assertEqual(value["reportCount"], 43)

    def test_missing_or_cross_source_report_is_rejected(self):
        root, reports, assets = self._fixture(42)
        with self.assertRaisesRegex(ValueError, "exactly 43"):
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

    def test_linux_macos_only_cannot_qualify_a_release(self):
        root, reports, assets = self._fixture(30)
        with self.assertRaisesRegex(ValueError, "exactly 43"):
            release_gate.build_manifest(self._args(root, reports, assets))

    def test_windows_architecture_and_binary_digest_are_required(self):
        for field in ("architecture", "sha256"):
            root, reports, assets = self._fixture()
            path = reports / "windows-cline.json"
            value = json.loads(path.read_text())
            if field == "architecture":
                value["environment"][field] = "aarch64"
            else:
                value["nanHarness"][field] = release_gate.digest(
                    assets / release_gate.PLATFORM_ASSETS["linux"]["harness"])
            path.write_text(json.dumps(value))
            with self.subTest(field=field), self.assertRaises(ValueError):
                release_gate.build_manifest(self._args(root, reports, assets))

    def test_deterministic_evidence_cannot_authorize_publication(self):
        root, reports, assets = self._fixture()
        for path in reports.glob("*.json"):
            value = json.loads(path.read_text())
            value["checks"] = value["checks"][:2]
            path.write_text(json.dumps(value))
        args = self._args(root, reports, assets)
        with self.assertRaisesRegex(ValueError, "live-tool"):
            release_gate.build_manifest(args)
        args.mode = "deterministic"
        self.assertEqual(release_gate.build_manifest(args)["reportCount"], 43)
        with self.assertRaises(publisher.ContractError):
            publisher.validate_handoff(args.output, assets, reports)

    def test_release_dispatches_verification_only_after_draft_creation(self):
        release = (ROOT / ".github/workflows/release.yml").read_text()
        qualify = release.split("  qualify:\n", 1)[1]
        self.assertIn("needs: publish", qualify)
        self.assertIn("actions: write", qualify)
        self.assertIn('gh workflow run release-gate.yml --repo "$GITHUB_REPOSITORY"', qualify)
        self.assertIn('--ref "$DEFAULT_BRANCH"', qualify)
        self.assertIn('-f tag_commit="$GITHUB_SHA"', qualify)
        self.assertIn("-f mode=live -f verification_only=true", qualify)
        self.assertNotIn("checkout", qualify)
        gate = (ROOT / ".github/workflows/release-gate.yml").read_text()
        self.assertIn('windows) test "$(uname -m)" = x86_64', gate)
        self.assertIn('--architecture "$ARCHITECTURE"', gate)
        self.assertIn('"$PYTHON" canary/actions/cli-suite.py', gate)
        for asset in release_gate.ASSETS:
            self.assertIn(asset, gate)

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
