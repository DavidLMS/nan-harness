#!/usr/bin/env python3
"""Static and portable contracts for the native Windows probe.

The native runner is Windows-only; these checks still exercise its closed report
inputs and command contracts on every development host.
"""
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[2]
PROBE = ROOT / "canary/guest/probe-harness.ps1"
CELL_SPEC = importlib.util.spec_from_file_location("probe_cell", ROOT / "canary/actions/cell.py")
CELL = importlib.util.module_from_spec(CELL_SPEC)
CELL_SPEC.loader.exec_module(CELL)


class WindowsProbeContracts(unittest.TestCase):
    def test_doctor_and_conformance_use_real_closed_schemas(self):
        source = PROBE.read_text(encoding="utf-8")
        self.assertIn("'doctor' $Harness '--allow-unsupported' '--allow-untested' '--json'", source)
        self.assertIn("$value.version", source)
        self.assertIn("'conformance' '--nan-harness' $NanBinary '--harness' $Harness '--json'", source)
        self.assertIn("Validate-Conformance $value $Harness", source)
        self.assertIn("diagnostics = @($diagnostics.ToArray())", source)
        self.assertIn("$value.exitCode", source)
        for diagnostic in ("doctor-output-invalid", "doctor-schema-invalid", "doctor-version-missing",
                           "doctor-version-invalid", "doctor-version-mismatch",
                           "conformance-output-invalid", "conformance-schema-invalid",
                           "conformance-scenario-failed", "conformance-inventory-failed",
                           "conformance-inventory-operational-failed",
                           "conformance-check-invalid", "conformance-child-launch"):
            with self.subTest(diagnostic=diagnostic):
                self.assertIn(diagnostic, source)
        self.assertNotIn("Fail 'conformance command failed'", source)

    def test_doctor_and_inventory_failures_have_bounded_distinctions(self):
        source = PROBE.read_text(encoding="utf-8")
        self.assertIn("doctor-version-missing", source)
        self.assertIn("doctor-version-invalid", source)
        self.assertIn("[string]$value.version -cne $Version", source)
        self.assertIn("doctorVersion = $doctorVersion", source)
        self.assertIn("doctorExpectedVersion = $doctorExpectedVersion", source)
        self.assertIn("doctorReason = $doctorReason", source)
        self.assertIn("discoveryCode = $discoveryCode", source)
        self.assertIn("inventoryFailureReasons = @($inventoryFailureReasons)", source)
        for reason in ("process-failed", "marker-missing", "provider-failed",
                       "provider-shutdown-failed", "daemon-cleanup-failed"):
            self.assertIn(reason, source)
        for code in ("NH-DISCOVERY-001", "NH-DISCOVERY-002", "NH-DISCOVERY-003",
                     "NH-DISCOVERY-004", "NH-DISCOVERY-005", "NH-DISCOVERY-006",
                     "NH-DISCOVERY-007"):
            self.assertIn(code, source)
        # Rust marks inventory failed only when the operational contract failed;
        # inventory drift is an observation on an otherwise passed report.
        self.assertIn("conformance-inventory-operational-failed", source)

    def test_all_fifteen_variants_have_real_launcher_and_tool_contracts(self):
        source = PROBE.read_text(encoding="utf-8")
        for launcher in ("claude", "codex", "opencode", "hermes", "pi", "omp", "prime", "dsh",
                         "openclaw", "cline", "qwen", "kimi", "aider", "goose", "fx"):
            with self.subTest(launcher=launcher):
                self.assertIn("'" + launcher + "'", source)
        for evidence in ("NAN_CODEX_TOOL_OK", "NAN_HERMES_TOOL_OK", "NAN_PRIME_TOOL_OK",
                         "NAN_DEEPSEEK_TOOL_OK", '"name":"Read"', '"name":"read_file"',
                         "read_files", '"toolName"\\s*:\\s*"read"',
                         '"name"\\s*:\\s*"shell"', "NAN_CANARY_OK"):
            self.assertIn(evidence, source)
        self.assertIn("usage-evidence", source)
        self.assertIn("usage-summary", source)

    def test_synthetic_real_reports_accept_and_reject_without_false_passes(self):
        base = {"schemaVersion": 1, "harness": "fx", "outcome": "passed", "scenarios": [
            {"name": name, "status": "passed" if name != "external-prerequisite" else "skipped",
             "checks": [{"name": "contract", "status": "passed"}], "durationMilliseconds": 0}
            for name in ("inventory", "tool-round-trip", "sentinel", "external-prerequisite")
        ]}
        self.assertEqual(CELL.conformance_result(json.loads(json.dumps(base)), "fx"), {})
        for mutation in (
            lambda x: x.update(harness="wrong"),
            lambda x: x.update(outcome="failed"),
            lambda x: x["scenarios"].pop(),
            lambda x: x["scenarios"][0].update(name="missing-tool"),
            lambda x: x["scenarios"][1].update(checks=[]),
        ):
            value = json.loads(json.dumps(base)); mutation(value)
            with self.assertRaises(ValueError): CELL.conformance_result(value, "fx")

    def test_synthetic_report_keeps_all_failed_scenarios_visible_to_validator(self):
        value = {"schemaVersion": 2, "harness": "fx", "outcome": "failed", "scenarios": [
            {"name": name, "status": "failed" if name in ("inventory", "sentinel") else
             ("skipped" if name == "external-prerequisite" else "passed"),
             "checks": [{"name": "contract", "status": "failed" if name in ("inventory", "sentinel") else
                         ("skipped" if name == "external-prerequisite" else "passed"),
                         "durationMilliseconds": 0}], "durationMilliseconds": 0}
            for name in ("inventory", "tool-round-trip", "sentinel", "external-prerequisite")
        ]}
        failed = CELL.conformance_result(value, "fx")
        self.assertEqual(set(failed), {"sentinel"})

    def test_pwsh_parser_is_run_when_available(self):
        pwsh = shutil.which("pwsh")
        if not pwsh:
            self.skipTest("pwsh unavailable on this development host")
        result = subprocess.run([pwsh, "-NoProfile", "-NonInteractive", "-Command",
                                 f"[scriptblock]::Create((Get-Content -Raw '{PROBE}')) | Out-Null"],
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
