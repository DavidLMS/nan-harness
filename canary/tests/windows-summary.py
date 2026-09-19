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
    def test_generated_collector_reports_are_accepted(self):
        with tempfile.TemporaryDirectory() as directory:
            for index, value in enumerate((report(), report(sourceSha="b" * 40))):
                path = Path(directory) / f"collector-{index}.json"
                path.write_text(json.dumps(value), encoding="utf-8")
                with self.subTest(path=path):
                    view = summary.safe_view(json.loads(path.read_text(encoding="utf-8")))
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

    def test_inventory_process_evidence_is_allowlisted_and_rendered(self):
        value = report()
        value["harnesses"][0]["phases"]["deterministic-contract"]["diagnostic"] = {
            "inventoryProcess": {"status": "launch-error", "osErrorCode": 2}}
        rendered = summary.render(summary.safe_view(value))
        self.assertIn("inventoryProcess=status=launch-error,osErrorCode=2", rendered)
        value["harnesses"][0]["phases"]["deterministic-contract"]["diagnostic"]["inventoryProcess"] = {
            "status": "cleanup-error", "cleanupStage": "wait-timeout", "cleanupStream": "stderr", "osErrorCode": 232}
        rendered = summary.render(summary.safe_view(value))
        self.assertIn("inventoryProcess=status=cleanup-error,osErrorCode=232,cleanupStage=wait-timeout,cleanupStream=stderr", rendered)
        for bad in ({"status": "nonzero-exit", "exitCode": 2147483648},
                    {"status": "nonzero-exit", "exitCode": -2147483649},
                    {"status": "nonzero-exit", "exitCode": True},
                    {"status": "nonzero-exit", "exitCode": 1.5},
                    {"status": "cleanup-error", "cleanupStage": "capture-timeout"},
                    {"status": "cleanup-error", "cleanupStage": "capture-timeout", "cleanupStream": "secret"},
                    {"status": "cleanup-error", "cleanupStage": "capture-timeout", "cleanupStream": "stdout", "exitCode": 1},
                    {"status": "cleanup-error", "cleanupStage": "capture-timeout", "cleanupStream": "stdout", "timeoutMilliseconds": 1},
                    {"status": "cleanup-error", "cleanupStage": "unknown", "cleanupStream": "stdout"},
                    {"status": "cleanup-error", "cleanupStage": ["capture-timeout"], "cleanupStream": "stdout"},
                    {"status": "cleanup-error", "cleanupStage": "capture-timeout", "cleanupStream": ["stdout"]},
                    {"status": "launch-error", "SECRET": "secret"}):
            value["harnesses"][0]["phases"]["deterministic-contract"]["diagnostic"]["inventoryProcess"] = bad
            with self.assertRaises(summary.UnsafeReport) as raised:
                summary.safe_view(value)
            self.assertNotIn("secret", str(raised.exception))

    def test_rejected_marker_field_names_are_allowlisted_and_rendered(self):
        value = report()
        value["harnesses"][0]["phases"]["version-doctor"]["diagnostic"] = {
            "markerState": "invalid", "markerFields": ["schemaVersion", "stage", "status"],
            "unexpectedFieldCount": 1}
        rendered = summary.render(summary.safe_view(value))
        self.assertIn("markerState=invalid", rendered)
        self.assertIn("unexpectedFieldCount=1", rendered)
        for bad_fields in (["secret"], ["schemaVersion", "secret"], "schemaVersion",
                           [f"field{index}" for index in range(17)]):
            value["harnesses"][0]["phases"]["version-doctor"]["diagnostic"]["markerFields"] = bad_fields
            with self.assertRaises(summary.UnsafeReport) as raised:
                summary.safe_view(value)
            self.assertNotIn("secret", str(raised.exception))
        value["harnesses"][0]["phases"]["version-doctor"]["diagnostic"]["markerFields"] = ["schemaVersion"]
        value["harnesses"][0]["phases"]["version-doctor"]["diagnostic"]["unexpectedFieldCount"] = 1025
        with self.assertRaises(summary.UnsafeReport) as raised:
            summary.safe_view(value)
        self.assertNotIn("secret", str(raised.exception))

    def test_assertion_codes_are_allowlisted_and_rendered(self):
        # A failed conformance contract publishes the closed code of the assertion that failed.
        value = report()
        value["harnesses"][0]["phases"]["deterministic-contract"]["diagnostic"] = {
            "assertions": {"assertionStatus": "valid", "assertions": ["side-effect-missing"]}}
        rendered = summary.render(summary.safe_view(value))
        self.assertIn("assertions=side-effect-missing", rendered)
        for bad in ({"assertionStatus": "valid", "assertions": ["secret"]},
                    {"assertionStatus": "valid", "assertions": []},
                    {"assertionStatus": "valid", "assertions": "side-effect-missing"},
                    {"assertionStatus": "absent", "assertions": ["process-failed"]},
                    {"assertionStatus": "secret"}):
            value["harnesses"][0]["phases"]["deterministic-contract"]["diagnostic"] = {"assertions": bad}
            with self.assertRaises(summary.UnsafeReport) as raised:
                summary.safe_view(value)
            self.assertNotIn("secret", str(raised.exception))

    def test_nested_installer_failure_class_is_allowlisted(self):
        value = report()
        value["harnesses"][0]["phases"]["install"]["diagnostic"] = {
            "subphase": "install", "executable": "pwsh", "exitCode": 1,
            "processCategory": "network-timeout"}
        view = summary.safe_view(value)
        self.assertEqual(view["harnesses"][0]["phases"]["install"]["diagnostic"]["processCategory"],
                         "network-timeout")
        value["harnesses"][0]["phases"]["install"]["diagnostic"]["processCategory"] = "secret"
        with self.assertRaises(summary.UnsafeReport) as raised:
            summary.safe_view(value)
        self.assertNotIn("secret", str(raised.exception))

    def test_failed_scenarios_are_allowlisted(self):
        # A conformance failure publishes the closed names of the scenarios that failed.
        value = report()
        value["harnesses"][0]["phases"]["deterministic-contract"]["diagnostic"] = {
            "failedScenarios": ["tool-round-trip"]}
        view = summary.safe_view(value)
        self.assertEqual(view["harnesses"][0]["phases"]["deterministic-contract"]["diagnostic"],
                         {"failedScenarios": ["tool-round-trip"]})
        for bad in ("tool-round-trip", ["secret"], ["tool-round-trip", "tool-round-trip"], []):
            value["harnesses"][0]["phases"]["deterministic-contract"]["diagnostic"] = {
                "failedScenarios": bad}
            with self.assertRaises(summary.UnsafeReport) as raised:
                summary.safe_view(value)
            self.assertNotIn("secret", str(raised.exception))

    def test_per_scenario_progress_is_allowlisted_and_rendered(self):
        value = report()
        value["harnesses"][0]["phases"]["deterministic-contract"]["diagnostic"] = {
            "progress": {
                "progressStatus": "valid",
                "progress": {"schema_version": 1, "scenario": "sentinel", "stage": "scenario",
                             "status": "started", "elapsed_milliseconds": 95},
                "progressScenarios": [
                    {"schema_version": 1, "scenario": "tool-round-trip", "stage": "process",
                     "status": "failed", "elapsed_milliseconds": 90},
                    {"schema_version": 1, "scenario": "sentinel", "stage": "scenario",
                     "status": "started", "elapsed_milliseconds": 95}]}}
        rendered = summary.render(summary.safe_view(value))
        self.assertIn("progress[tool-round-trip]=process=failed", rendered)
        for bad in ([{"schema_version": 1, "scenario": "secret", "stage": "process",
                      "status": "failed", "elapsed_milliseconds": 1}],
                    "secret",
                    [{"schema_version": 1, "scenario": "sentinel", "stage": "process",
                      "status": "secret", "elapsed_milliseconds": 1}],
                    [{"schema_version": 1, "scenario": "sentinel", "stage": "process",
                      "status": "failed", "elapsed_milliseconds": 1}] * 5):
            value["harnesses"][0]["phases"]["deterministic-contract"]["diagnostic"]["progress"]["progressScenarios"] = bad
            with self.assertRaises(summary.UnsafeReport) as raised:
                summary.safe_view(value)
            self.assertNotIn("secret", str(raised.exception))

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

    def test_unhashable_marker_state_is_rejected_without_leaking_values(self):
        for marker_state in (["secret"], {"secret": "token"}):
            value = report()
            value["harnesses"][0]["phases"]["deterministic-contract"]["diagnostic"] = {
                "markerState": marker_state}
            with self.assertRaises(summary.UnsafeReport) as raised:
                summary.render(summary.safe_view(value))
            self.assertNotIn("secret", str(raised.exception))

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
