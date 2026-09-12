#!/usr/bin/env python3
"""Deterministic contracts for the modular desktop suite runner."""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import os
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("desktop_suite", ROOT / "canary/actions/desktop_suite.py")
SUITE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SUITE)


def selection(apps=None):
    requested = apps or list(SUITE.DESKTOP_HARNESSES)
    apps = [app for app in SUITE.DESKTOP_HARNESSES if app in requested]
    return {"suite": "desktop", "mode": "deterministic", "harnesses": apps,
            "platforms": [{"system": system, "runner": "runner", "architecture": "x86_64",
                           "target": "target", "harnesses": apps} for system in ("linux", "macos", "windows")]}


class DesktopSuiteTests(unittest.TestCase):
    def frozen_manifest(self, app="zed-desktop", platform="linux", architecture="x86_64", model="model"):
        return {"schemaVersion": 1, "suite": "desktop", "platform": platform,
                "architecture": architecture, "model": model, "apps": [{
                    "status": "frozen", "app": app, "version": "1.2.3",
                    "channel": "github-release:zed-industries/zed", "url": "https://github.com/zed-industries/zed/releases/download/v1.2.3/zed-linux-x86_64.tar.gz",
                    "format": "tar-gz", "digest": "sha256:" + "a" * 64,
                    "staged": False, "installer": "checker"}]}

    def test_read_frozen_manifest_returns_exact_requested_order(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            value = self.frozen_manifest()
            value["apps"] = [self.frozen_manifest("zed-desktop")["apps"][0],
                             {"status": "blocked", "app": "chatgpt-desktop", "reason": "resolution-failed",
                              "evidence": "https://persistent.oaistatic.com/codex-app-prod/linux/deb/"}]
            value["apps"][0]["app"] = "zed-desktop"
            path.write_text(json.dumps(value))
            result = SUITE.read_frozen_manifest(path, ["chatgpt-desktop", "zed-desktop"], "linux", "x86_64", "model")
            self.assertEqual([entry["app"] for entry in result["apps"]], ["chatgpt-desktop", "zed-desktop"])

    def test_read_frozen_manifest_rejects_drift_and_unknown_fields(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            value = self.frozen_manifest()
            value["apps"][0]["staged"] = True
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                SUITE.read_frozen_manifest(path, ["zed-desktop"], "linux", "x86_64", "model")

    def test_manifest_rejects_same_host_attacker_url_and_allows_runtime_prerelease(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            value = self.frozen_manifest()
            value["apps"][0]["runtimeVersion"] = "0.154.0-alpha.6.2"
            value["apps"][0]["url"] = "https://github.com/attacker/zed/releases/download/v1.2.3/zed-linux-x86_64.tar.gz"
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                SUITE.read_frozen_manifest(path, ["zed-desktop"], "linux", "x86_64", "model")

    def test_manifest_rejects_extra_frozen_reason_and_bad_types(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            value = self.frozen_manifest()
            value["apps"][0]["reason"] = "resolution-failed"
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                SUITE.read_frozen_manifest(path, ["zed-desktop"], "linux", "x86_64", "model")
            value = self.frozen_manifest()
            value["apps"][0]["staged"] = "false"
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                SUITE.read_frozen_manifest(path, ["zed-desktop"], "linux", "x86_64", "model")
            value = self.frozen_manifest()
            value["unexpected"] = True
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                SUITE.read_frozen_manifest(path, ["zed-desktop"], "linux", "x86_64", "model")

    def test_one_cell_keeps_apps_sequential_and_canonical(self):
        cell = SUITE.suite_cell(selection(["zed-desktop", "chatgpt-desktop"]), "linux",
                                "branch", "a" * 40, "selected-model")
        self.assertEqual(cell["apps"], ["chatgpt-desktop", "zed-desktop"])
        self.assertEqual(cell["sourceSha"], "a" * 40)

    def test_branch_and_release_identity_are_distinct(self):
        with self.assertRaises(ValueError):
            SUITE.validate_identity("release", "a" * 39, "model", ["zed-desktop"], "linux", "v0.1.0")
        self.assertEqual(SUITE.validate_identity("release", "b" * 40, "model",
                                                  ["zed-desktop"], "linux", "v0.1.0"), ("zed-desktop",))

    def test_release_manifest_rejects_tag_or_digest_mismatch(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            asset = root / "nanh"
            asset.write_bytes(b"verified")
            manifest = root / "SHA256SUMS"
            manifest.write_text(f"{SUITE.digest(asset)}  nanh\n")
            command = SUITE.validate_release_assets(manifest, {"nanh": asset}, "v1.2.3", "a" * 40)
            self.assertEqual(command[-4:], ["--source-ref", "refs/tags/v1.2.3", "--source-digest", "a" * 40])
            with self.assertRaises(ValueError):
                SUITE.validate_release_assets(manifest, {"nanh": asset}, "v1.2.4", "a" * 39)
            asset.write_bytes(b"tampered")
            with self.assertRaises(ValueError):
                SUITE.validate_release_assets(manifest, {"nanh": asset}, "v1.2.3", "a" * 40)

    def test_workflow_has_reusable_attestation_gate_before_preparation(self):
        workflow = (ROOT / ".github/workflows/desktop-check-suite.yml").read_text()
        self.assertIn("workflow_call:", workflow)
        self.assertIn("type: choice\n        options: [branch, release]", workflow)
        self.assertIn("python3 canary/actions/desktop_release.py", workflow)
        self.assertLess(workflow.index("python3 canary/actions/desktop_release.py"),
                        workflow.index("Prepare apps and private receipt"))
        self.assertIn("fail-fast: false", workflow)
        self.assertIn("hosted_evidence:", workflow)
        self.assertIn("hosted evidence requires release source", workflow)
        self.assertIn("python3 canary/actions/detector.py feed", workflow)
        self.assertIn("python3 canary/actions/evidence.py select-desktop", workflow)
        self.assertLess(workflow.index("Resolve exact frozen Desktop releases"),
                        workflow.index("Install exact external Desktop applications"))
        self.assertIn("--model '${{ needs.select.outputs.model }}'", workflow)
        self.assertIn("--platform '${{ matrix.system }}'", workflow)
        self.assertIn("if: steps.pending.outputs.harnesses != '' || !inputs.hosted_evidence", workflow)
        self.assertIn("name: hosted-evidence-desktop-${{ matrix.system }}", workflow)
        self.assertNotIn("prepare-hermes-desktop.sh", workflow)

    def test_standalone_uses_exact_installer_after_resolve_on_every_platform(self):
        workflow = (ROOT / ".github/workflows/desktop-check.yml").read_text()
        self.assertIn("name: Install exact Desktop application from frozen manifest", workflow)
        self.assertNotIn("prepare-hermes-desktop.sh", workflow)
        self.assertLess(workflow.index("Freeze official Desktop release inputs"),
                        workflow.index("Install exact Desktop application from frozen manifest"))

    def test_command_passes_model_and_all_apps_without_shell(self):
        cell = SUITE.suite_cell(selection(), "windows", "branch", "a" * 40, "model/x")
        command = SUITE.checker_command("checker", "live", cell, "receipt", "report", "nanh")
        self.assertEqual(command[0:7], ["checker", "run", "--yes", "--non-interactive", "--ephemeral", "--mode", "live"])
        self.assertIn("--model", command)
        self.assertIn("model/x", command)
        self.assertEqual(command[-2:], ["--session", "github-hosted"])

    def test_initial_state_contains_only_hashes_for_binaries(self):
        with tempfile.TemporaryDirectory() as directory:
            checker = Path(directory) / "checker"
            nanh = Path(directory) / "nanh"
            checker.write_bytes(b"checker")
            nanh.write_bytes(b"nanh")
            cell = SUITE.suite_cell(selection(["zed-desktop"]), "linux", "branch", "a" * 40, "model")
            prepared = Path(directory) / "prepared"
            prepared.write_bytes(b"receipt")
            state = SUITE.initial_state(cell, checker, nanh, prepared)
            self.assertEqual(state["checkerSha256"], SUITE.digest(checker))
            self.assertNotIn("output", state)
            self.assertEqual(state["outcome"], "blocked")

    def test_fake_stage_is_bounded_and_drops_key_for_deterministic(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "marker"
            fake = Path(directory) / "checker"
            fake.write_text("#!/bin/sh\nprintf '%s' \"${NAN_API_KEY-unset}\" > \"$MARKER\"\nexit 0\n")
            fake.chmod(0o700)
            old = os.environ.get("MARKER")
            os.environ["MARKER"] = str(marker)
            try:
                with patch.dict(os.environ, {"NAN_API_KEY": "synthetic"}):
                    self.assertTrue(SUITE.run_stage([str(fake)], live=False, timeout=5))
                self.assertEqual(marker.read_text(), "unset")
            finally:
                if old is None:
                    os.environ.pop("MARKER", None)
                else:
                    os.environ["MARKER"] = old

    def test_state_identity_mismatch_refuses_before_fake_checker(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            checker = root / "checker"
            nanh = root / "nanh"
            prepared = root / "prepared"
            for path in (checker, nanh, prepared):
                path.write_bytes(path.name.encode())
            cell = SUITE.suite_cell(selection(["zed-desktop"]), "linux", "branch", "a" * 40, "model")
            state = SUITE.initial_state(cell, checker, nanh, prepared)
            state["model"] = "different-model"
            self.assertNotEqual(state["model"], cell["model"])


if __name__ == "__main__":
    unittest.main()
