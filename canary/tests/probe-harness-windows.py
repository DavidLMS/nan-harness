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
import re
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
PROBE = ROOT / "canary/guest/probe-harness.ps1"
CURRENT_DOCTOR_SCHEMA = 9
CELL_SPEC = importlib.util.spec_from_file_location("probe_cell", ROOT / "canary/actions/cell.py")
CELL = importlib.util.module_from_spec(CELL_SPEC)
CELL_SPEC.loader.exec_module(CELL)


class WindowsProbeContracts(unittest.TestCase):
    def test_native_doctor_fixture_tracks_the_actual_cli_producer_schema(self):
        models = (ROOT / 'crates/nan-harness-cli/src/commands/doctor/models.rs').read_text()
        version = re.search(r'const DOCTOR_SCHEMA_VERSION: u8 = ([0-9]+);', models)
        self.assertIsNotNone(version)
        self.assertEqual(int(version.group(1)), CURRENT_DOCTOR_SCHEMA)
        source = PROBE.read_text()
        self.assertIn(f'Is-BoundedInteger $value.schemaVersion {CURRENT_DOCTOR_SCHEMA}', source)
        self.assertIn('$value.schemaVersion -notin @(8,9)', source)

    def test_codex_conformance_requires_both_reasoning_checks(self):
        source = PROBE.read_text(encoding="utf-8")
        self.assertIn("@('reasoning-qwen38','reasoning-deepseek-v4')", source)
        self.assertIn("$matching.Count -ne 1 -or $matching[0].status -cne 'passed'", source)

    def test_real_pwsh_codex_reasoning_evidence_fails_closed(self):
        pwsh = shutil.which("pwsh")
        if not pwsh:
            self.skipTest("pwsh unavailable; actual producer is verified on Windows")
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            marker = root / "probe-result.json"
            producer = root / "synthetic-canary.ps1"
            checks = [{"name": name, "status": "passed", "durationMilliseconds": 1}
                      for name in ("contract", "reasoning-qwen38", "reasoning-deepseek-v4")]
            scenarios = [{"name": name, "status": "passed", "durationMilliseconds": 1,
                          "checks": checks if name == "tool-round-trip" else checks[:1]}
                         for name in ("inventory", "tool-round-trip", "sentinel", "external-prerequisite")]
            report = {"schemaVersion": 2, "harness": "codex", "outcome": "passed",
                      "scenarios": scenarios, "durationMilliseconds": 4}
            for mode in ("passed", "missing", "failed", "duplicate"):
                candidate = json.loads(json.dumps(report))
                selected = candidate["scenarios"][1]["checks"]
                if mode == "missing":
                    selected.pop()
                elif mode == "failed":
                    selected[-1]["status"] = "failed"
                elif mode == "duplicate":
                    selected.append(dict(selected[-1]))
                producer.write_text("Write-Output '" + json.dumps(candidate) + "'; exit 0\n")
                env = dict(os.environ, NAN_CANARY_PROBE_RESULT=str(marker))
                result = subprocess.run([pwsh, "-NoProfile", "-NonInteractive", "-File", str(PROBE),
                    "-Harness", "codex", "-Stage", "deterministic-contract", "-NanBinary", str(producer),
                    "-Canary", str(producer), "-Version", "0.153.4"],
                    env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
                observed = json.loads(marker.read_text(encoding="utf-8-sig"))
                with self.subTest(mode=mode):
                    self.assertEqual(result.returncode == 0, mode == "passed")
                    self.assertEqual(observed["status"], "passed" if mode == "passed" else "failed")
                    if mode != "passed":
                        self.assertIn("conformance-check-invalid", observed["diagnostics"])

    def test_real_pwsh_mimo_checks_project_actual_report_and_preserve_exit_verdict(self):
        pwsh = shutil.which("pwsh")
        if not pwsh:
            self.skipTest("pwsh unavailable; actual check producer is verified on Windows")
        for statuses in [("passed", "failed"), ("failed", "passed"), ("passed", "passed")]:
            with self.subTest(statuses=statuses), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                marker = root / "probe-result.json"
                checks = [{"name": name, "status": status, "durationMilliseconds": 12 + index}
                          for index, (name, status) in enumerate(zip(("contract", "native-configuration"), statuses))]
                failed = "failed" in statuses
                scenarios = [{"name": name, "status": "failed" if failed and name == "tool-round-trip" else "passed",
                              "checks": checks if name == "tool-round-trip" else [{"name": "contract", "status": "passed", "durationMilliseconds": 1}],
                              "durationMilliseconds": 25 if name == "tool-round-trip" else 1}
                             for name in ("inventory", "tool-round-trip", "sentinel", "external-prerequisite")]
                report = {"schemaVersion": 2, "harness": "mimo-code", "outcome": "failed" if failed else "passed",
                          "scenarios": scenarios, "durationMilliseconds": 28}
                producer = root / "synthetic-canary.ps1"
                def run(value):
                    producer.write_text("Write-Output '" + json.dumps(value) + "'; exit " + str(int(failed)) + "\n")
                    env = dict(os.environ, NAN_CANARY_PROBE_RESULT=str(marker))
                    result = subprocess.run([pwsh, "-NoProfile", "-NonInteractive", "-File", str(PROBE),
                        "-Harness", "mimo-code", "-Stage", "deterministic-contract", "-NanBinary", str(producer),
                        "-Canary", str(producer), "-Version", "0.1.16"],
                        env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
                    return result.returncode, json.loads(marker.read_text(encoding="utf-8-sig"))
                code, observed = run(report)
                self.assertEqual(code, int(failed))
                self.assertEqual(observed["status"], "failed" if failed else "passed")
                self.assertEqual(observed["conformanceChecks"], checks)
                self.assertEqual(observed["exitCode"], int(failed))
                for invalid in (checks[:1], [{**checks[0], "name": "PRIVATE"}, checks[1]],
                                [{**checks[0], "output": "PRIVATE"}, checks[1]],
                                [{**checks[0], "durationMilliseconds": True}, checks[1]]):
                    scenarios[1]["checks"] = invalid
                    _, observed = run(report)
                    self.assertNotIn("conformanceChecks", observed)
                    self.assertNotIn("PRIVATE", json.dumps(observed))

    def test_live_marker_accepts_only_closed_diagnostic_categories(self):
        with tempfile.TemporaryDirectory() as tmp:
            marker = Path(tmp) / "marker.json"
            value = {"schemaVersion": 2, "stage": "harness-run", "status": "failed",
                     "diagnostics": ["live-child-launch"], "exitCode": -1}
            marker.write_text(json.dumps(value))
            self.assertEqual(CELL.windows_probe_result(marker)["diagnostics"], ["live-child-launch"])
            value["diagnostics"] = ["private provider message"]
            marker.write_text(json.dumps(value))
            self.assertIsNone(CELL.windows_probe_result(marker))

    def _run_live_fixture(self, harness="fx", *, exit_code=0, missing_child=False,
                          shell="pwsh", native_warning=False, failure_text="",
                          failure_model=None, quota_model=None,
                          usage_payload='{"schemaVersion":1,"status":"observed"}'):
        usage_command = ""
        if usage_payload == "unreadable":
            usage_command = "New-Item -ItemType Directory -Path $env:NAN_HARNESS_INTERNAL_CANARY_USAGE_FILE | Out-Null\n"
        elif usage_payload is not None:
            usage_command = ("Set-Content -NoNewline -LiteralPath $env:NAN_HARNESS_INTERNAL_CANARY_USAGE_FILE "
                             "-Value '" + usage_payload.replace("'", "''") + "'\n")
        pwsh = shutil.which(shell)
        if not pwsh:
            self.skipTest("pwsh unavailable; live PowerShell fixture deferred to Windows")
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            marker = root / "probe-result.json"
            child = root / "synthetic-live-child.ps1"
            child.write_text(
                ("if ($args -contains '" + quota_model + "') { Set-Content -NoNewline -LiteralPath $env:NAN_HARNESS_INTERNAL_CANARY_USAGE_FILE -Value '{\"schemaVersion\":1,\"status\":\"" +
                 ("quota-qwen38" if quota_model == "qwen3.8-flash" else "quota-deepseek-v4") + "\"}'; exit 7 }\n" if quota_model else "") +
                ("if ($args -contains '" + failure_model + "') { exit 7 }\n" if failure_model else "") +
                ("Write-Output '" + failure_text.replace("'", "''") + "'\n" if failure_text else "") +
                ("[Console]::Error.WriteLine('synthetic native warning')\n" if native_warning else "") +
                "$prompt = [string]$args[-1]\n"
                "if ($prompt -match \"read '([^']+)'\") {\n"
                "  Write-Output ('Reading ' + $Matches[1])\n"
                "  Get-Content -Raw -LiteralPath $Matches[1]\n"
                "  Write-Output 'read_files'\n"
                "}\n"
                "if ($prompt -match 'powershell -NoProfile -Command \"([^\"]+)\"') {\n"
                "  & pwsh -NoProfile -NonInteractive -Command $Matches[1]\n"
                "  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }\n"
                "}\n"
                + usage_command + "Write-Output 'NAN_CANARY_OK'\n" +
                ("[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)\n"
                 "[Console]::Error.WriteLine(([char]::ConvertFromUtf32(0x1F525) + ' Tokens burned ' + [char]0x2014 + ' this session'))\n"
                 if native_warning else "Write-Output 'NaN usage (synthetic)'\n") +
                f"exit {exit_code}\n",
                encoding="utf-8",
            )
            if native_warning:
                wrapper = root / "synthetic-native.cmd"
                wrapper.write_text(
                    f'@"{pwsh}" -NoProfile -NonInteractive -File "{child}" %*\n'
                    '@exit /b %errorlevel%\n', encoding="utf-8")
                child = wrapper
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
            expected_success = (not exit_code and not missing_child and not failure_model
                                and usage_payload == '{"schemaVersion":1,"status":"observed"}')
            if expected_success:
                self.assertEqual(run.returncode, 0, f"probe_result={value}")
            else:
                self.assertNotEqual(run.returncode, 0, f"probe_result={value}")
            return value

    @unittest.skipUnless(os.name == "nt", "Windows PowerShell native stderr contract")
    def test_windows_powershell_native_warning_preserves_success_and_actual_failure(self):
        for code in (0, 7):
            with self.subTest(exit_code=code):
                value = self._run_live_fixture(
                    harness="cline", shell="powershell.exe", native_warning=True, exit_code=code)
                self.assertEqual(value["exitCode"], code)
                self.assertEqual(value["status"], "failed" if code else "passed")

    def test_real_pwsh_live_success_records_zero_exit(self):
        value = self._run_live_fixture()
        self.assertEqual(value, {"schemaVersion": 2, "stage": "complete", "status": "passed",
                                 "diagnostics": [], "exitCode": 0})

    def test_usage_failures_keep_closed_categories_through_the_daily_reader(self):
        cases = [(None, "missing"), ("unreadable", "unreadable"),
                 ("{PRIVATE_SENTINEL", "malformed"),
                 ('{"schemaVersion":1,"status":"not-observed"}', "not-observed"),
                 ('{"schemaVersion":1,"status":"unsupported"}', "unsupported")]
        cases += [(payload, "schema-invalid") for payload in (
            "null", "[]", '[{"schemaVersion":1,"status":"observed"}]',
            '{"schemaVersion":true,"status":"observed"}',
            '{"schemaVersion":2,"status":"observed"}',
            '{"schemaVersion":1,"status":"PRIVATE_SENTINEL"}',
            '{"schemaVersion":1,"status":"OBSERVED"}',
            '{"schemaVersion":1,"status":"observed","extra":"PRIVATE_SENTINEL"}')]
        shells = ("pwsh", "powershell.exe") if os.name == "nt" else ("pwsh",)
        for shell in shells:
            for payload, reason in cases:
                with self.subTest(shell=shell, reason=reason, payload=payload):
                    result = self._run_live_fixture(shell=shell, usage_payload=payload)
                    self.assertEqual(result["stage"], "usage-evidence")
                    self.assertEqual(result["exitCode"], 0)
                    self.assertEqual(result["diagnostics"], ["live-usage-" + reason])
                    self.assertNotIn("PRIVATE_SENTINEL", json.dumps(result))
                    with tempfile.TemporaryDirectory() as tmp:
                        marker = Path(tmp) / "probe.json"
                        marker.write_text(json.dumps(result))
                        self.assertEqual(CELL.windows_probe_result(marker), result)

    def test_real_pwsh_live_nonzero_records_actual_exit(self):
        value = self._run_live_fixture(exit_code=7)
        self.assertEqual(value["stage"], "harness-run")
        self.assertEqual(value["status"], "failed")
        self.assertEqual(value["exitCode"], 7)
        self.assertEqual(value["diagnostics"], ["live-exit-nonzero"])

    def test_real_pwsh_failure_categories_never_copy_private_text(self):
        for message, code in (("401 Unauthorized PRIVATE_SENTINEL", "live-error-auth"),
                              ("unexpected argument PRIVATE_SENTINEL", "live-error-arguments"),
                              ("Permission denied PRIVATE_SENTINEL", "live-error-permission"),
                              ("stream disconnected PRIVATE_SENTINEL", "live-error-provider"),
                              ("unclassified PRIVATE_SENTINEL", "live-exit-nonzero")):
            with self.subTest(code=code):
                value = self._run_live_fixture(exit_code=7, failure_text=message)
                self.assertEqual(value["diagnostics"], [code])
                self.assertNotIn("PRIVATE_SENTINEL", json.dumps(value))

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
        self.assertEqual(value["evidence"], CELL.CODEX_REASONING_EVIDENCE)

    def test_real_pwsh_codex_live_models_are_individually_required(self):
        for model, stage in (("qwen3.8-flash", "codex-qwen38"), ("deepseek-v4-flash", "codex-deepseek-v4")):
            with self.subTest(model=model):
                value = self._run_live_fixture(harness="codex", failure_model=model)
                self.assertEqual(value["stage"], stage)
                self.assertEqual(value["status"], "failed")
                self.assertEqual(value["exitCode"], 7)
                self.assertNotIn("evidence", value)

    def test_real_pwsh_codex_quota_is_recorded_separately(self):
        for model, evidence in (("qwen3.8-flash", "codex-always-on-models-v2-quota-ok"),
                                ("deepseek-v4-flash", "codex-always-on-models-v2-ok-quota")):
            with self.subTest(model=model):
                value = self._run_live_fixture(harness="codex", quota_model=model)
                self.assertEqual(value["evidence"], evidence)
                self.assertIn("402", CELL.CODEX_REASONING_DETAILS[evidence])

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
                f'{{"schemaVersion":{CURRENT_DOCTOR_SCHEMA},"offline":true,"harness":"fx","level":"ok",'
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
            current = producer.read_text()
            producer.write_text(current.replace(f'"schemaVersion":{CURRENT_DOCTOR_SCHEMA}', '"schemaVersion":8'))
            legacy = subprocess.run(command, env=env, capture_output=True, text=True, timeout=30)
            self.assertEqual(legacy.returncode, 0)
            self.assertEqual(json.loads(marker.read_text(encoding="utf-8-sig"))["status"], "passed")
            for invalid in ("true", "9.5", '"9"', "7", "10", "9223372036854775808"):
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
                        f"Write-Output '{{\"schemaVersion\":{CURRENT_DOCTOR_SCHEMA},\"offline\":true,"
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

    def test_real_pwsh_selects_the_native_hermes_machine_frontend(self):
        pwsh = shutil.which("pwsh")
        if not pwsh:
            self.skipTest("pwsh unavailable; version selection deferred to Windows")
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            producer = root / "conformance.ps1"
            producer.write_text("if (($args -contains '--hermes-stream-json') -ne ($env:EXPECT_MACHINE -eq '1')) { exit 12 }\n"
                                "Write-Output $env:FIXTURE_REPORT\nexit 0\n", encoding="utf-8")
            for harness, version, modern in [("hermes", "0.21.0", False), ("hermes", "0.22.5", True),
                                             ("hermes", "unknown", False), ("cline", "1.3.0", False)]:
                with self.subTest(harness=harness, version=version):
                    report = {"schemaVersion": 2, "harness": harness, "outcome": "passed", "durationMilliseconds": 0,
                              "scenarios": [{"name": name, "status": "passed", "durationMilliseconds": 0,
                                  "checks": [{"name": "contract", "status": "passed", "durationMilliseconds": 0}]}
                                  for name in CELL.CONFORMANCE_SCENARIOS]}
                    marker = root / "result.json"
                    env = dict(os.environ, EXPECT_MACHINE=str(int(modern)), FIXTURE_REPORT=json.dumps(report),
                               NAN_CANARY_PROBE_RESULT=str(marker))
                    run = subprocess.run([pwsh, "-NoProfile", "-NonInteractive", "-File", str(PROBE),
                        "-Harness", harness, "-Stage", "deterministic-contract", "-NanBinary", str(producer),
                        "-Canary", str(producer), "-Version", version], env=env, capture_output=True, timeout=30)
                    self.assertEqual(run.returncode, 0)
                    self.assertEqual(json.loads(marker.read_text(encoding="utf-8-sig"))["status"], "passed")

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
        self.assertIn(f"Is-BoundedInteger $value.schemaVersion {CURRENT_DOCTOR_SCHEMA}", source)
        self.assertIn("function Is-DoctorOptionalString", source)
        self.assertIn("compatibleAt','liveVerifiedAt", source)
        self.assertIn("'inventoryFailureReasons'", source)
        self.assertIn("provider-shutdown-failed", source)
        self.assertIn("inventoryFailed", source)
        self.assertIn("@('conformance','--nan-harness',$NanBinary,'--harness',$Harness,'--json')", source)
        self.assertIn("& $Canary @conformanceArguments", source)
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

    def test_all_sixteen_variants_have_real_launcher_and_tool_contracts(self):
        source = PROBE.read_text(encoding="utf-8")
        for launcher in ("claude", "codex", "opencode", "mimo", "hermes", "pi", "omp", "prime", "dsh",
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
