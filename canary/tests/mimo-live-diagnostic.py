#!/usr/bin/env python3
"""One live call, immutable assets, and advisory closed observations."""
import importlib.util
import json
import os
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("mimo_live_diagnostic", Path(__file__).resolve().parents[1] / "actions/mimo_live_diagnostic.py")
diagnostic = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(diagnostic)


def observation(stage="tool-evidence"):
    stream = dict(status="empty", events=dict(toolUse=0, read=0, text=0, error=0, other=0),
                  readMarker=False, completionMarker=False, bridgeSentinel=False)
    return dict(schemaVersion=1, stage=stage, stdout=dict(stream), stderr=dict(stream), usage="missing")


class LiveDiagnosticTests(unittest.TestCase):
    def inputs(self, root):
        binary, canary = root / "release-cli", root / "release-canary"
        binary.write_bytes(b"synthetic immutable CLI")
        canary.write_bytes(b"synthetic immutable canary")
        receipt = dict(schemaVersion=1, controllerSha="a"*40, releaseTag=diagnostic.TAG,
            releaseCommit=diagnostic.COMMIT, assetRunId=40520278155, assetWorkflowSha=diagnostic.COMMIT,
            artifactId=1, artifactSha256="b"*64, manifestSha256=diagnostic.MANIFEST,
            nanHarnessSha256=diagnostic.inventory.digest(binary), canarySha256=diagnostic.inventory.digest(canary))
        provenance, manifest = root / "provenance.json", root / "manifest.json"
        provenance.write_text(json.dumps(receipt))
        manifest.write_text(json.dumps({"harnesses": [dict(harness="mimo-code", version="0.1.16", system="linux",
            architecture="aarch64", source="github:XiaomiMiMo/MiMo-Code", package="", model="qwen3.6", ref="")]}))
        return SimpleNamespace(binary=binary, canary=canary, provenance=provenance, manifest=manifest,
            source_sha="a"*40, directory=root / "cli-cell", output=root / "public.json")

    def collect(self, args, value=None, outcome="failed", mutate=None):
        calls = []
        cli_sha, canary_sha = diagnostic.inventory.digest(args.binary), diagnostic.inventory.digest(args.canary)
        def install(command, state):
            calls.append("install")
            self.assertNotIn("NAN_API_KEY", os.environ)
            self.assertNotIn("GITHUB_TOKEN", os.environ)
            self.assertEqual(command.directory, args.directory / "mimo-code")
            self.assertEqual(command.harness_version, "0.1.16")
            self.assertEqual(command.model, "qwen3.6")
            state["harness"]["version"] = "0.1.16"
            if mutate == "install": args.canary.write_bytes(b"changed")
            if mutate == "installer-failed": raise RuntimeError("PRIVATE installer payload")
        def live(command, state, observe=None):
            calls.append("live")
            self.assertEqual(os.environ["NAN_API_KEY"], "PRIVATE_PROVIDER_KEY")
            self.assertNotIn("GITHUB_TOKEN", os.environ)
            self.assertEqual(command.mode, "live")
            self.assertEqual(command.trigger, "release")
            self.assertEqual(state["harness"]["version"], "0.1.16")
            observe(value)
            if mutate == "repeat": observe(value)
            calls.append("after-observer")
            if mutate == "live": args.binary.write_bytes(b"changed")
            if outcome == "cleanup": raise diagnostic.cell.ProbeCleanupError("PRIVATE cleanup payload")
            if outcome == "timeout": raise diagnostic.cell.StageTimeout("PRIVATE timeout payload")
            if outcome == "failed": raise diagnostic.cell.ProbeFailure("tool-evidence", 1)
            return 1
        with patch.multiple(diagnostic, CLI_SHA=cli_sha, CANARY_SHA=canary_sha), \
                patch.dict(os.environ, {"GITHUB_SHA": args.source_sha, "GITHUB_REF": "refs/heads/main",
                    "RUNNER_TEMP": str(args.directory.parent), "NAN_API_KEY": "PRIVATE_PROVIDER_KEY", "GITHUB_TOKEN": "PRIVATE_READ_TOKEN"}), \
                patch.object(diagnostic.cell, "detected_identity", return_value=("linux", "aarch64")), \
                patch.object(diagnostic.cell, "install", side_effect=install), \
                patch.object(diagnostic.cell, "live", side_effect=live):
            if mutate == "before": args.binary.write_bytes(b"changed")
            report = diagnostic.collect(args)
            self.assertEqual(os.environ["NAN_API_KEY"], "PRIVATE_PROVIDER_KEY")
            self.assertEqual(os.environ["GITHUB_TOKEN"], "PRIVATE_READ_TOKEN")
        self.assertNotIn("PRIVATE", json.dumps(report))
        self.assertNotIn(str(args.directory.parent), json.dumps(report))
        return report, calls

    def test_original_failed_verdict_and_code_survive_valid_observation_without_retry(self):
        with tempfile.TemporaryDirectory() as tmp:
            report, calls = self.collect(self.inputs(Path(tmp)), observation())
        self.assertEqual(calls, ["install", "live", "after-observer"])
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["originalLiveStatus"], "failed")
        self.assertEqual(report["originalLiveStage"], "tool-evidence")
        self.assertEqual(report["originalLiveCode"], "live-tool-evidence-exit-1")
        self.assertEqual(report["originalLiveExitCode"], 1)
        self.assertEqual(report["observationStatus"], "valid")
        self.assertEqual(report["observation"], observation())
        self.assertFalse(report["qualification"])
        self.assertTrue(report["diagnosticsOnly"])

    def test_passed_original_probe_does_not_become_qualification(self):
        with tempfile.TemporaryDirectory() as tmp:
            report, calls = self.collect(self.inputs(Path(tmp)), observation("usage-summary"), outcome="passed")
        self.assertEqual(calls, ["install", "live", "after-observer"])
        self.assertEqual(report["status"], "observed")
        self.assertEqual(report["originalLiveStatus"], "passed")
        self.assertIsNone(report["originalLiveCode"])
        self.assertFalse(report["qualification"])

    def test_missing_invalid_or_repeated_observation_never_throws_into_original_probe(self):
        for value, mutation, status in ((None, None, "missing"), ({"rawText": "PRIVATE"}, None, "invalid"),
                                        (observation(), "repeat", "invalid")):
            for outcome in ("passed", "failed"):
                with self.subTest(status=status, outcome=outcome), tempfile.TemporaryDirectory() as tmp:
                    report, calls = self.collect(self.inputs(Path(tmp)), value, outcome=outcome, mutate=mutation)
                self.assertEqual(calls[-1], "after-observer")
                self.assertEqual(report["originalLiveStatus"], outcome)
                self.assertEqual(report["status"], "failed")
                self.assertEqual(report["observationStatus"], status)
                self.assertIsNone(report["observation"])
                if outcome == "failed": self.assertEqual(report["originalLiveCode"], "live-tool-evidence-exit-1")

    def test_binding_changes_suppress_diagnostics_and_prevent_unattributed_live_acceptance(self):
        for phase, calls_expected in (("before", []), ("install", ["install"]), ("live", ["install", "live", "after-observer"])):
            with self.subTest(phase=phase), tempfile.TemporaryDirectory() as tmp:
                report, calls = self.collect(self.inputs(Path(tmp)), observation(), mutate=phase)
            self.assertEqual(calls, calls_expected)
            self.assertEqual(report["status"], "failed")
            self.assertIsNone(report["observation"])
            self.assertEqual(report["reason"], "provenance-invalid" if phase == "before" else "binding-changed")

    def test_manifest_drift_is_rejected_before_install_or_live(self):
        for key, value in (("version", "0.1.17"), ("architecture", "x86_64"), ("source", "PRIVATE"), ("ref", "a"*40)):
            with self.subTest(key=key), tempfile.TemporaryDirectory() as tmp:
                args = self.inputs(Path(tmp)); manifest = json.loads(args.manifest.read_text())
                manifest["harnesses"][0][key] = value; args.manifest.write_text(json.dumps(manifest))
                report, calls = self.collect(args, observation())
            self.assertEqual(calls, [])
            self.assertEqual(report["reason"], "manifest-invalid")

    def test_closed_cleanup_and_timeout_failures_remain_failures(self):
        for outcome, expected in (("cleanup", "cleanup-unproven"), ("timeout", "live-timeout")):
            with self.subTest(outcome=outcome), tempfile.TemporaryDirectory() as tmp:
                report, calls = self.collect(self.inputs(Path(tmp)), observation(), outcome=outcome)
            self.assertEqual(report["reason"], expected)
            self.assertEqual(report["originalLiveStatus"], "failed")
            self.assertEqual(calls, ["install", "live", "after-observer"])

    def test_untrusted_branch_cannot_start_a_cell(self):
        with tempfile.TemporaryDirectory() as tmp:
            args = self.inputs(Path(tmp))
            with patch.dict(os.environ, {"GITHUB_SHA": args.source_sha, "GITHUB_REF": "refs/heads/untrusted"}), \
                    patch.object(diagnostic.cell, "install") as install, patch.object(diagnostic.cell, "live") as live:
                report = diagnostic.collect(args)
            install.assert_not_called(); live.assert_not_called()
        self.assertEqual(report["reason"], "source-invalid")

    def test_output_is_exclusive_and_protected_before_any_bytes(self):
        with tempfile.TemporaryDirectory() as tmp:
            args = self.inputs(Path(tmp)); argv = ["diagnostic", "--source-sha", args.source_sha]
            for name in ("binary", "canary", "provenance", "manifest", "directory", "output"):
                argv += ["--" + name, str(getattr(args, name))]
            def refuse(path):
                self.assertEqual(path.read_bytes(), b""); raise RuntimeError("PRIVATE protection failure")
            with patch.object(diagnostic.sys, "argv", argv), patch.object(diagnostic, "collect", return_value={"status":"failed"}), \
                    patch.object(diagnostic.cell, "protect_private", side_effect=refuse), patch("builtins.print") as output:
                self.assertEqual(diagnostic.main(), 1)
            self.assertFalse(args.output.exists())
            self.assertNotIn("PRIVATE", str(output.call_args))


if __name__ == "__main__":
    unittest.main()
