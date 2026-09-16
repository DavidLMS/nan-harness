#!/usr/bin/env python3
"""Static and portable contracts for the native Windows probe.

The native runner is Windows-only; these checks still exercise its closed report
inputs and command contracts on every development host.
"""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
PROBE = ROOT / "canary/guest/probe-harness.ps1"
CELL_SPEC = importlib.util.spec_from_file_location("probe_cell", ROOT / "canary/actions/cell.py")
CELL = importlib.util.module_from_spec(CELL_SPEC)
CELL_SPEC.loader.exec_module(CELL)


class WindowsProbeContracts(unittest.TestCase):
    def _run_live_fixture(self, harness="fx", *, exit_code=0, missing_child=False):
        pwsh = shutil.which("pwsh")
        if not pwsh:
            self.skipTest("pwsh unavailable; live PowerShell fixture deferred to Windows")
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            marker = root / "probe-result.json"
            child = root / "synthetic-live-child.ps1"
            child.write_text(
                "$prompt = [string]$args[-1]\n"
                "if ($prompt -match \"read '([^']+)'\") {\n"
                "  Write-Output ('Reading ' + $Matches[1])\n"
                "  Get-Content -Raw -LiteralPath $Matches[1]\n"
                "}\n"
                "if ($prompt -match 'powershell -NoProfile -Command \"([^\"]+)\"') {\n"
                "  & pwsh -NoProfile -NonInteractive -Command $Matches[1]\n"
                "  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }\n"
                "}\n"
                "Set-Content -NoNewline -LiteralPath $env:NAN_HARNESS_INTERNAL_CANARY_USAGE_FILE "
                "-Value '{\"schemaVersion\":1,\"status\":\"observed\"}'\n"
                "Write-Output 'NAN_CANARY_OK'\n"
                "Write-Output 'NaN usage (synthetic)'\n"
                f"exit {exit_code}\n",
                encoding="utf-8",
            )
            command = [pwsh, "-NoProfile", "-NonInteractive", "-File", str(PROBE),
                       "-Harness", harness, "-Stage", "live-tool", "-NanBinary",
                       str(root / "missing-child.ps1" if missing_child else child),
                       "-Canary", str(child), "-Version", "1.2.3"]
            env = dict(os.environ, NAN_API_KEY="synthetic", NAN_CANARY_PROBE_RESULT=str(marker))
            run = subprocess.run(command, env=env, capture_output=True, text=True, timeout=30)
            try:
                value = json.loads(marker.read_text(encoding="utf-8-sig"))
            except (OSError, json.JSONDecodeError) as error:
                self.fail(f"probe result unavailable: {type(error).__name__}")
            expected_success = not exit_code and not missing_child
            if expected_success:
                self.assertEqual(run.returncode, 0, f"probe_result={value}")
            else:
                self.assertNotEqual(run.returncode, 0, f"probe_result={value}")
            return value

    def test_real_pwsh_live_success_records_zero_exit(self):
        value = self._run_live_fixture()
        self.assertEqual(value, {"schemaVersion": 2, "stage": "complete", "status": "passed",
                                 "diagnostics": [], "exitCode": 0})

    def test_real_pwsh_live_nonzero_records_actual_exit(self):
        value = self._run_live_fixture(exit_code=7)
        self.assertEqual(value["stage"], "harness-run")
        self.assertEqual(value["status"], "failed")
        self.assertEqual(value["exitCode"], 7)
        self.assertEqual(value["diagnostics"], ["live-exit-nonzero"])

    def test_real_pwsh_live_launch_failure_records_safe_sentinel_exit(self):
        value = self._run_live_fixture(missing_child=True)
        self.assertEqual(value["stage"], "harness-run")
        self.assertEqual(value["status"], "failed")
        self.assertEqual(value["exitCode"], -1)
        self.assertEqual(value["diagnostics"], ["live-child-launch"])

    def test_real_pwsh_codex_prompt_uses_explicit_windows_writer(self):
        value = self._run_live_fixture(harness="codex")
        self.assertEqual(value["status"], "passed")
        self.assertEqual(value["exitCode"], 0)

    def test_real_pwsh_doctor_json_integer_types(self):
        """Run the actual probe against producer-shaped JSON when pwsh exists."""
        pwsh = shutil.which("pwsh")
        if not pwsh:
            self.skipTest("pwsh is unavailable; native PowerShell fixture deferred to Windows")
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            marker = root / "probe-result.json"
            producer = root / "doctor-producer.ps1"
            producer.write_text(
                "@'\n"
                '{"schemaVersion":8,"offline":true,"harness":"fx","level":"ok",'
                '"installed":true,"version":"1.2.3",'
                '"minimumSupportedVersion":"1.0.0",'
                '"lastCompatibleVersion":"1.2.3",'
                '"compatibleAt":"2026-09-07T02:40:14.144121Z",'
                '"lastLiveVerifiedVersion":"1.2.3",'
                '"liveVerifiedAt":"2026-09-07T02:40:14.144121Z",'
                '"compatibility":"tested",'
                '"warnings":[],"safeToShare":true}\n'
                "'@\n"
                "exit 0\n",
                encoding="utf-8",
            )
            command = [pwsh, "-NoProfile", "-NonInteractive", "-File", str(PROBE),
                       "-Harness", "fx", "-Stage", "version-doctor", "-NanBinary", str(producer),
                       "-Canary", str(producer), "-Version", "1.2.3"]
            env = dict(__import__("os").environ, NAN_CANARY_PROBE_RESULT=str(marker))
            run = subprocess.run(command, env=env, capture_output=True, text=True, timeout=30)
            self.assertEqual(run.returncode, 0, run.stderr)
            value = __import__("json").loads(marker.read_text(encoding="utf-8-sig"))
            self.assertEqual(value["status"], "passed")
            self.assertEqual(value["exitCode"], 0)
            for invalid in ("true", "8.5", '"8"', "9223372036854775808"):
                producer.write_text(
                    "Write-Output '{\"schemaVersion\":" + invalid + ",\"offline\":true,"
                    "\"harness\":\"fx\",\"level\":\"ok\",\"installed\":true,"
                    "\"version\":\"1.2.3\",\"warnings\":[],\"safeToShare\":true}'\nexit 0\n",
                    encoding="utf-8",
                )
                run = subprocess.run(command, env=env, capture_output=True, text=True, timeout=30)
                self.assertNotEqual(run.returncode, 0, invalid)
                invalid_value = __import__("json").loads(marker.read_text(encoding="utf-8-sig"))
                self.assertIn("doctor-schema-invalid", invalid_value["diagnostics"])
            for field in ("compatibleAt", "liveVerifiedAt"):
                for invalid_timestamp in ("123", "true", "{}"):  # timestamps must remain Rust strings
                    producer.write_text(
                        "Write-Output '{\"schemaVersion\":8,\"offline\":true,"
                        "\"harness\":\"fx\",\"level\":\"ok\",\"installed\":true,"
                        "\"version\":\"1.2.3\",\"" + field + "\":" + invalid_timestamp + ","
                        "\"warnings\":[],\"safeToShare\":true}'\nexit 0\n",
                        encoding="utf-8",
                    )
                    run = subprocess.run(command, env=env, capture_output=True, text=True, timeout=30)
                    self.assertNotEqual(run.returncode, 0, (field, invalid_timestamp))
                    invalid_value = __import__("json").loads(marker.read_text(encoding="utf-8-sig"))
                    self.assertIn("doctor-schema-invalid", invalid_value["diagnostics"])

    def test_real_pwsh_inventory_cleanup_contract(self):
        """Exercise cleanup-error validation through the actual PowerShell reader."""
        pwsh = shutil.which("pwsh")
        if not pwsh:
            self.skipTest("pwsh is unavailable; native PowerShell fixture deferred to Windows")
        scenarios = [
            {"name": name, "status": "passed", "checks": [{"name": "contract", "status": "passed", "durationMilliseconds": 0}],
             "durationMilliseconds": 0}
            for name in ("inventory", "tool-round-trip", "sentinel", "external-prerequisite")
        ]
        base = {"schemaVersion": 2, "harness": "fx", "outcome": "passed", "scenarios": scenarios,
                "durationMilliseconds": 0,
                "inventoryProcess": {"status": "cleanup-error", "cleanupStage": "wait-timeout",
                                     "cleanupStream": "stderr", "osErrorCode": 232}}
        invalid = (
            {"status": "cleanup-error", "cleanupStage": "wait-timeout", "cleanupStream": "stderr", "exitCode": 1},
            {"status": "cleanup-error", "cleanupStage": "wait-timeout", "cleanupStream": "stderr", "timeoutMilliseconds": 1},
            {"status": "cleanup-error", "cleanupStage": "unknown", "cleanupStream": "stderr"},
            {"status": "cleanup-error", "cleanupStage": ["wait-timeout"], "cleanupStream": "stderr"},
            {"status": "cleanup-error", "cleanupStage": "wait-timeout", "cleanupStream": ["stderr"]},
        )
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); marker = root / "probe-result.json"; producer = root / "conformance-producer.ps1"
            producer.write_text("Write-Output $env:NAN_CONFORMANCE_FIXTURE_JSON\nexit 0\n", encoding="utf-8")
            command = [pwsh, "-NoProfile", "-NonInteractive", "-File", str(PROBE), "-Harness", "fx",
                       "-Stage", "deterministic-contract", "-NanBinary", str(producer), "-Canary", str(producer),
                       "-Version", "1.2.3"]
            env = dict(os.environ, NAN_CANARY_PROBE_RESULT=str(marker))
            env["NAN_CONFORMANCE_FIXTURE_JSON"] = json.dumps(base, separators=(",", ":"))
            run = subprocess.run(command, env=env, capture_output=True, text=True, timeout=30)
            self.assertEqual(run.returncode, 0, run.stderr)
            value = json.loads(marker.read_text(encoding="utf-8-sig"))
            self.assertEqual(value["status"], "passed")
            self.assertEqual(value["inventoryProcess"], base["inventoryProcess"])
            for evidence in invalid:
                env["NAN_CONFORMANCE_FIXTURE_JSON"] = json.dumps({**base, "inventoryProcess": evidence}, separators=(",", ":"))
                run = subprocess.run(command, env=env, capture_output=True, text=True, timeout=30)
                self.assertNotEqual(run.returncode, 0, evidence)
                value = json.loads(marker.read_text(encoding="utf-8-sig"))
                self.assertIn("conformance-schema-invalid", value["diagnostics"])

    def test_doctor_and_conformance_use_real_closed_schemas(self):
        source = PROBE.read_text(encoding="utf-8")
        self.assertIn("'doctor' $Harness '--allow-unsupported' '--allow-untested' '--json'", source)
        self.assertIn("$value.version", source)
        self.assertIn("function Is-Integer", source)
        self.assertIn("function Is-BoundedInteger", source)
        self.assertIn("function Is-SignedInt32", source)
        self.assertIn("$Value.status -isnot [string]", source)
        self.assertNotIn("$value.schemaVersion -isnot [int]", source)
        self.assertIn("Is-BoundedInteger $Value.schemaVersion 2", source)
        self.assertIn("Is-BoundedInteger $value.schemaVersion 8", source)
        self.assertIn("function Is-DoctorOptionalString", source)
        self.assertIn("compatibleAt','liveVerifiedAt", source)
        self.assertIn("'inventoryFailureReasons'", source)
        self.assertIn("provider-shutdown-failed", source)
        self.assertIn("inventoryFailed", source)
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
        self.assertIn("doctorSchemaReason = $doctorSchemaReason", source)
        self.assertIn("discoveryCode = $discoveryCode", source)
        self.assertIn("inventoryFailureReasons = @($inventoryFailureReasons)", source)
        self.assertIn("inventoryProcess = $inventoryProcess", source)
        self.assertIn("$valueValid -and $null -ne $value.inventoryProcess", source)
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

    def test_contract_failure_publishes_closed_scenario_names(self):
        # The hosted diagnostic can only attribute a conformance failure when the probe
        # publishes which contract failed, and it must stay a closed vocabulary.
        producer = PROBE.read_text(encoding="utf-8")
        self.assertIn("$failedScenarios", producer)
        self.assertIn("$value.failedScenarios = @($failedScenarios)", producer)
        for name in ("external-prerequisite", "inventory", "sentinel", "tool-round-trip"):
            self.assertIn("'" + name + "'", producer)
        self.assertIn("Add-Diagnostic 'probe-unexpected-failure'", producer)

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
