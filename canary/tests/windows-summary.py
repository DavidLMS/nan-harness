#!/usr/bin/env python3
"""Tests for the telemetry-safe Windows Actions summary projection."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("windows_summary", ROOT / "actions" / "windows_summary.py")
summary = importlib.util.module_from_spec(spec); spec.loader.exec_module(summary)

def report(**updates):
    value = {"schemaVersion": 1, "mode": "live", "sourceSha": "a" * 40,
             "harnesses": [{"harness": "omp", "outcome": "failed", "phases": {
                 phase: {"status": "FAIL", "reason": "probe-failed",
                         "causalId": "WIN-LIVE_TOOL-0123456789ab"}
                 for phase in summary.PHASES}}],
             "totals": {"selected": 1, "passed": 0, "failed": 1, "blocked": 0}}
    value.update(updates); return value

class WindowsSummaryTests(unittest.TestCase):
    def test_saved_real_collector_reports_are_accepted(self):
        paths = (Path("/tmp/windows-cli-diagnostic-34846876377-download/report.json"),
                 Path("/tmp/windows-cli-diagnostic-34852283651-download.hF8Uj0/report.json"))
        for path in paths:
            with self.subTest(path=path):
                view = summary.safe_view(json.loads(path.read_text(encoding="utf-8-sig")))
                self.assertRegex(view["sourceSha"], r"^[0-9a-f]{40}$")

    def test_safe_projection_contains_identity_outcomes_and_statuses(self):
        rendered = summary.render(summary.safe_view(report()))
        self.assertIn("Source SHA: `" + "a" * 40 + "`", rendered)
        self.assertIn("`omp` | `failed`", rendered); self.assertIn("live-tool=FAIL", rendered)
        self.assertNotIn("causalId", rendered)

    def test_cause_details_are_allowlisted_and_rendered(self):
        value = report()
        value["harnesses"][0]["phases"]["install"]["causeDetails"] = {
            "executable": "npm-cmd", "exitCode": 1, "npmCode": "npm-unknown",
            "processReason": "exit-nonzero", "subphase": "install"}
        rendered = summary.render(summary.safe_view(value))
        self.assertIn("npmCode=npm-unknown", rendered)
        value["harnesses"][0]["phases"]["install"]["causeDetails"]["message"] = "secret"
        with self.assertRaises(summary.UnsafeReport): summary.safe_view(value)

    def test_diagnostic_and_cause_details_render_without_hiding_either(self):
        value = report()
        phase_value = value["harnesses"][0]["phases"]["install"]
        phase_value["diagnostic"] = {"subphase": "install", "executable": "npm-cmd", "exitCode": 1}
        phase_value["causeDetails"] = {"parentReason": "nonzero", "installerReason": "capability-not-implemented",
                                        "cleanupReason": "cleanup-failed"}
        rendered = summary.render(summary.safe_view(value))
        self.assertIn("diagnostic=subphase=install,executable=npm-cmd,exitCode=1", rendered)
        self.assertIn("cause=parentReason=nonzero,installerReason=capability-not-implemented,cleanupReason=cleanup-failed", rendered)

    def test_progress_evidence_is_allowlisted_and_rendered(self):
        value = report()
        value["harnesses"][0]["phases"]["deterministic-contract"]["diagnostic"] = {
            "progress": {"progressStatus": "valid", "progress": {
                "schema_version": 1, "scenario": "inventory", "stage": "provider-shutdown",
                "status": "started", "elapsed_milliseconds": 42}}}
        rendered = summary.render(summary.safe_view(value))
        self.assertIn("progress=schema_version=1,scenario=inventory,stage=provider-shutdown,status=started,elapsed_milliseconds=42", rendered)
        value["harnesses"][0]["phases"]["deterministic-contract"]["diagnostic"]["progress"]["progress"]["secret"] = "token"
        with self.assertRaises(summary.UnsafeReport): summary.safe_view(value)

    def test_corrupt_or_absent_progress_is_explicit(self):
        for state in ("absent", "corrupt"):
            value = report()
            value["harnesses"][0]["phases"]["deterministic-contract"]["diagnostic"] = {
                "progress": {"progressStatus": state}}
            rendered = summary.render(summary.safe_view(value))
            self.assertIn("progress=" + state, rendered)

    def test_missing_report_writes_explicit_safe_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "summary.md"
            self.assertEqual(summary.main(["--report", str(Path(directory) / "missing.json"), "--output", str(output)]), 2)
            self.assertIn("summary unavailable", output.read_text())

    def test_actual_collector_envelope_and_safe_fallback_are_supported(self):
        value = report(model="qwen3.6", platform={"os": "windows", "architecture": "x64"}, setup={},
                       groupedCauses={}, nativePrerequisites={"status": "PASS"},
                       nanHarness={"sha256": "a" * 64}, canary={"sha256": "b" * 64})
        summary.safe_view(value)
        fallback = {"schemaVersion": 1, "mode": "live", "sourceSha": "b" * 40,
                    "platform": {"os": "windows", "architecture": "x64"}, "harnesses": [],
                    "totals": {"selected": 0, "passed": 0, "failed": 1, "blocked": 1}}
        rendered = summary.render(summary.safe_view(fallback))
        self.assertIn("selected=0", rendered)

    def test_malformed_and_secret_like_input_is_rejected(self):
        values = [report(sourceSha="not-a-sha"), report(mode="live\nsecret"),
                  report(harnesses=[{"harness": "omp", "outcome": "failed", "phases": {},
                                     "message": "token=secret"}]),
                  report(harnesses=[{"harness": "omp", "outcome": "failed", "phases": {
                      "metadata": {"status": "FAIL", "reason": "raw stderr secret"}}}],
                         totals={"selected": 1, "passed": 0, "failed": 1, "blocked": 0})]
        for value in values:
            with self.assertRaises(summary.UnsafeReport): summary.safe_view(value)

    def test_missing_and_arbitrary_cause_fields_are_rejected(self):
        value = report(); del value["sourceSha"]
        with self.assertRaises(summary.UnsafeReport): summary.safe_view(value)
        value = report(); value["harnesses"][0]["phases"]["metadata"]["causeGroup"] = "arbitrary"
        with self.assertRaises(summary.UnsafeReport): summary.safe_view(value)

    def test_workflow_orders_summary_after_fallback_before_upload(self):
        workflow = (ROOT.parent / ".github/workflows/windows-cli-diagnostic.yml").read_text()
        fallback = workflow.index("- name: Ensure safe fallback report exists")
        publish = workflow.index("- name: Publish safe step summary")
        upload = workflow.index("- name: Upload diagnostic report")
        self.assertLess(fallback, publish); self.assertLess(publish, upload)
        self.assertIn("GITHUB_STEP_SUMMARY", workflow)
        self.assertIn("continue-on-error: true", workflow[publish:upload])

if __name__ == "__main__": unittest.main()
