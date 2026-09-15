#!/usr/bin/env python3
"""Failure-injection contracts for the native Windows diagnostic collector."""
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("windows_diagnostic", ROOT / "actions" / "windows_diagnostic.py")
diagnostic = importlib.util.module_from_spec(spec); spec.loader.exec_module(diagnostic)
summary_spec = importlib.util.spec_from_file_location("windows_summary", ROOT / "actions" / "windows_summary.py")
summary = importlib.util.module_from_spec(summary_spec); summary_spec.loader.exec_module(summary)


class Item:
    def __init__(self, harness): self.harness, self.version, self.ref = harness, "1.2.3", ("a" * 40 if harness == "hermes" else "")


class Resolver:
    def resolve_manifest(self, harnesses, *_args): return [Item(harnesses[0])], []


class WindowsOsProxy:
    """Exercise Windows branches without mutating the process-wide os module."""
    def __init__(self, real_os): self._real_os, self.name = real_os, "nt"
    def __getattr__(self, name): return getattr(self._real_os, name)


class WindowsDiagnosticTests(unittest.TestCase):
    def args(self, mode="deterministic"):
        return type("Args", (), {"source": "a" * 40, "mode": mode, "model": "test/model", "python_version": "3.12", "timeout": 1,
                                  "binary": Path("nanh.exe"), "canary": Path("nan-harness-canary.exe")})()

    def binaries(self, args, directory):
        args.binary = directory / "nanh.exe"; args.canary = directory / "nan-harness-canary.exe"
        args.binary.write_bytes(b"binary"); args.canary.write_bytes(b"canary")

    def test_windows_native_preflight_fx_doctor_is_exercised_on_this_host(self):
        calls = []
        def fake(argv, cwd, env, timeout):
            calls.append(argv)
            if "-Stage" in argv:
                Path(env["NAN_CANARY_PROBE_RESULT"]).write_text(
                    '{"schemaVersion":2,"stage":"complete","status":"passed","diagnostics":[],"exitCode":0}')
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, \
             patch.object(diagnostic, "os", WindowsOsProxy(diagnostic.os)), \
             patch.object(diagnostic.shutil, "which", return_value="x"), \
             patch.object(diagnostic, "protect_private"), \
             patch.object(diagnostic, "run_bounded", side_effect=fake), \
             patch.object(diagnostic, "run_bounded_command_line", side_effect=fake):
            cell = Path(tmp)
            result = diagnostic.native_prerequisite_self_test(
                cell, {"ComSpec": r"C:\Windows\System32\cmd.exe", "PATH": "safe"}, 1
            )
        self.assertEqual(result["checks"]["doctor-json"]["status"], "PASS")
        self.assertTrue(
            any(
                "-Harness" in call and call[call.index("-Harness") + 1] == "fx"
                and "-Stage" in call and call[call.index("-Stage") + 1] == "version-doctor"
                for call in calls
            )
        )

    def test_budget_uses_monotonic_clock_and_reserves_cleanup(self):
        now = [100.0]
        budget = diagnostic.BatchBudget(40, lambda: now[0])
        self.assertEqual(budget.child_timeout(900), 20.0)
        now[0] = 141.0
        self.assertEqual(budget.child_timeout(900), 0.0)
        self.assertTrue(budget.exhausted())

    def test_progress_reader_preserves_last_valid_record_and_rejects_bad_data(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "progress.jsonl"
            valid = {"schema_version": 1, "scenario": "inventory", "stage": "provider-shutdown",
                     "status": "started", "elapsed_milliseconds": 12}
            path.write_text(json.dumps(valid) + "\n{" , encoding="utf-8")
            self.assertEqual(diagnostic.read_progress(path), {"progressStatus": "valid", "progress": valid})
            path.write_text(json.dumps({**valid, "secret": "token"}) + "\n", encoding="utf-8")
            self.assertEqual(diagnostic.read_progress(path), {"progressStatus": "corrupt"})
            path.write_text("x" * (diagnostic.PROGRESS_MAX_LINE + 1), encoding="utf-8")
            self.assertEqual(diagnostic.read_progress(path), {"progressStatus": "corrupt"})

    def test_progress_reader_bounds_binary_read_before_rejecting_oversize_input(self):
        class Stream:
            def __init__(self): self.requested = None
            def __enter__(self): return self
            def __exit__(self, *_args): return False
            def read(self, amount):
                self.requested = amount
                return b"x" * amount
        class FakePath:
            def __init__(self): self.stream = Stream()
            def exists(self): return True
            def open(self, mode):
                self.mode = mode
                return self.stream
        path = FakePath()
        self.assertEqual(diagnostic.read_progress(path), {"progressStatus": "corrupt"})
        self.assertEqual(path.mode, "rb")
        self.assertEqual(path.stream.requested, diagnostic.PROGRESS_MAX_LINE * diagnostic.PROGRESS_MAX_RECORDS + 1)

    def test_progress_paths_are_fresh_and_absent_is_explicit(self):
        with tempfile.TemporaryDirectory() as tmp:
            cell = Path(tmp)
            first, second = diagnostic.progress_path(cell), diagnostic.progress_path(cell)
            self.assertNotEqual(first, second)
            self.assertEqual(diagnostic.read_progress(first), {"progressStatus": "absent"})

    def test_timeout_exposes_last_progress_without_child_output(self):
        args = self.args(); seen = []
        def fake(argv, cwd, env, timeout):
            if "deterministic-contract" in argv:
                seen.append(env.get("NAN_HARNESS_CONFORMANCE_PROGRESS"))
                event = {"schema_version": 1, "scenario": "inventory", "stage": "process",
                         "status": "started", "elapsed_milliseconds": 17}
                Path(env["NAN_HARNESS_CONFORMANCE_PROGRESS"]).write_text(json.dumps(event) + "\n", encoding="utf-8")
                return (None, "timeout")
            if "-Stage" in argv:
                Path(env["NAN_CANARY_PROBE_RESULT"]).write_text(
                    '{"schemaVersion":2,"stage":"complete","status":"passed","diagnostics":[],"exitCode":0}')
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertTrue(failed); self.assertEqual(len(seen), 1)
        progress = report["harnesses"][0]["phases"]["deterministic-contract"]["diagnostic"]["progress"]
        self.assertEqual(progress["progress"]["stage"], "process")

    def test_workflow_budget_math_reserves_startup_and_finalization(self):
        self.assertEqual(diagnostic.diagnostic_batch_budget(8 * 60 + 36), 5964)
        self.assertEqual(diagnostic.diagnostic_batch_budget(110 * 60), 0 + 1)
        self.assertEqual(diagnostic.diagnostic_batch_budget(0, job_seconds=120, finalization_seconds=10,
                                                            startup_slack_seconds=5, cap_seconds=100), 100)

    def test_metadata_resolution_receives_remaining_bounded_timeout(self):
        args = self.args(); seen = []
        class TimedResolver:
            def resolve_manifest(self, harnesses, *_args, timeout=0):
                seen.append(timeout); return [Item(harnesses[0])], []
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=TimedResolver()), \
             patch.object(diagnostic.shutil, "which", return_value=None):
            args.budget_seconds = 50; args.clock = lambda: 0.0; self.binaries(args, Path(tmp))
            diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertEqual(seen, [20])

    def test_checkpoint_snapshot_is_pure_and_recomputes_active_outcome(self):
        args = self.args()
        reports = []
        active = {"harness": "codex", "phase": "deterministic-contract", "phases": {"metadata": diagnostic.phase("PASS"),
                                                    "prerequisites": diagnostic.phase("PASS"),
                                                    "install": diagnostic.phase("PASS"),
                                                    "version-doctor": diagnostic.phase("PASS"),
                                                    "deterministic-contract": diagnostic.phase("BLOCKED", "unfinished")}}
        snapshot = diagnostic._report(args, reports, {"status": "NOT_RUN"}, ["codex"], active=active)
        self.assertEqual(snapshot["harnesses"][0]["outcome"], "blocked")
        self.assertEqual(reports, [])
        active["phases"]["deterministic-contract"] = diagnostic.phase("PASS")
        snapshot = diagnostic._report(args, reports, {"status": "NOT_RUN"}, ["codex"], active=active)
        self.assertEqual(snapshot["harnesses"][0]["outcome"], "passed")

    def test_interruption_checkpoint_preserves_install_and_unfinished_doctor(self):
        args = self.args(); calls = [0]
        fx_doctor_seen = []
        def interrupted(argv, cwd, env, timeout):
            calls[0] += 1
            harness = argv[argv.index("-Harness") + 1] if "-Harness" in argv else None
            stage = argv[argv.index("-Stage") + 1] if "-Stage" in argv else None
            if harness == "fx" and stage == "version-doctor":
                fx_doctor_seen.append(argv)
            if harness == "codex" and stage == "version-doctor":
                raise KeyboardInterrupt
            if harness == "codex" and stage is not None:
                Path(env["NAN_CANARY_PROBE_RESULT"]).write_text(
                    '{"schemaVersion":2,"stage":"complete","status":"passed","diagnostics":[],"exitCode":0}')
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "os", WindowsOsProxy(diagnostic.os)), \
             patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=interrupted):
            self.binaries(args, Path(tmp)); output = Path(tmp)
            with self.assertRaises(KeyboardInterrupt):
                diagnostic.collect(args, ["codex"], output)
            persisted = json.loads((output / "report.json").read_text())
        item = persisted["harnesses"][0]
        self.assertEqual(item["phases"]["install"]["status"], "PASS")
        self.assertEqual(item["phases"]["version-doctor"]["reason"], "unfinished")
        self.assertEqual(item["phases"]["deterministic-contract"]["reason"], "not-started")
        self.assertEqual(item["outcome"], "blocked")
        self.assertEqual(len(fx_doctor_seen), 1)

    def test_deadline_checkpoint_preserves_partial_cells_and_explicit_not_started(self):
        args = self.args(); now = [0.0]
        fx_doctor_seen = []
        def fake(argv, cwd, env, timeout):
            # Windows runs the native prerequisite self-test before harness
            # cells; only the synthetic installer should consume this test's
            # deadline, or the preflight would make the assertion
            # platform-dependent.
            harness = argv[argv.index("-Harness") + 1] if "-Harness" in argv else None
            stage = argv[argv.index("-Stage") + 1] if "-Stage" in argv else None
            if harness == "fx" and stage == "version-doctor":
                fx_doctor_seen.append(argv)
            if harness in {"claude-code", "codex", "opencode"} and stage is None:
                now[0] = 20.0
            return (1, "nonzero")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "os", WindowsOsProxy(diagnostic.os)), \
             patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            args.budget_seconds = 30; args.clock = lambda: now[0]; self.binaries(args, Path(tmp))
            report, failed = diagnostic.collect(args, ["claude-code", "codex", "opencode"], Path(tmp))
            persisted = json.loads((Path(tmp) / "report.json").read_text())
        self.assertTrue(failed); self.assertEqual(report["totals"]["selected"], 3)
        self.assertEqual(persisted["harnesses"][0]["outcome"], "failed")
        self.assertEqual(persisted["harnesses"][2]["phases"]["install"]["reason"], "not-started")
        self.assertEqual(persisted["harnesses"][2]["outcome"], "blocked")
        self.assertEqual(persisted, report)
        self.assertEqual(len(fx_doctor_seen), 1)

    def test_known_deadline_is_persisted_before_unattempted_metadata(self):
        args = self.args(); args.budget_seconds = 1; args.clock = lambda: 0.0
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
            persisted = json.loads((Path(tmp) / "report.json").read_text())
        self.assertTrue(failed); self.assertEqual(persisted, report)
        self.assertEqual(report["harnesses"][0]["phases"]["metadata"]["reason"], "deadline-exhausted")

    def test_cleanup_failure_is_explicit_and_does_not_publish_child_text(self):
        args = self.args()
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", return_value=(None, "timeout")), \
             patch.object(diagnostic, "cleanup_installer_artifacts", return_value=False):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertTrue(failed)
        self.assertEqual(report["harnesses"][0]["phases"]["install"]["reason"], "installer-timeout")
        self.assertEqual(report["harnesses"][0]["phases"]["install"]["causeDetails"],
                         {"parentReason": "timeout", "installerReason": "installer-failed", "cleanupReason": "cleanup-failed"})

    def write_probe_success(self, argv, env):
        if "-Stage" in argv:
            Path(env["NAN_CANARY_PROBE_RESULT"]).write_text(
                '{"schemaVersion":2,"stage":"complete","status":"passed",'
                '"diagnostics":[],"exitCode":0}')

    def test_all_fifteen_have_every_phase_exclusive_totals(self):
        args = self.args()
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", return_value=(None, "timeout")):
            self.binaries(args, Path(tmp))
            report, failed = diagnostic.collect(args, list(diagnostic.HARNESSES), Path(tmp))
        self.assertTrue(failed); self.assertEqual(report["totals"]["selected"], 15)
        self.assertEqual(report["totals"]["passed"] + report["totals"]["failed"] + report["totals"]["blocked"], 15)
        for item in report["harnesses"]:
            self.assertEqual(set(item["phases"]), set(diagnostic.PHASES)); self.assertEqual(item["outcome"], "failed")

    def test_first_middle_timeout_does_not_stop_later_harnesses(self):
        args = self.args("live"); calls = []
        def fake(argv, cwd, env, timeout):
            calls.append((cwd.name, argv))
            self.write_probe_success(argv, env)
            return (None, "timeout") if cwd.name == "codex" else (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake), \
             patch.dict(diagnostic.os.environ, {"NAN_API_KEY": "synthetic"}):
            self.binaries(args, Path(tmp))
            report, failed = diagnostic.collect(args, ["claude-code", "codex", "opencode"], Path(tmp))
        self.assertTrue(failed); self.assertEqual([x["harness"] for x in report["harnesses"]], ["claude-code", "codex", "opencode"])
        self.assertEqual(report["harnesses"][2]["outcome"], "passed")
        self.assertTrue(any("deterministic-contract" in call[1] for call in calls))
        self.assertTrue(any("live-tool" in call[1] for call in calls))
        self.assertTrue(any(any("nanh.exe" in str(arg) for arg in call[1]) and
                            any("nan-harness-canary.exe" in str(arg) for arg in call[1]) for call in calls))

    def test_environment_strips_credentials_and_isolates_npm(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(diagnostic.os.environ, {"NAN_API_KEY": "secret", "GITHUB_TOKEN": "secret"}):
            env = diagnostic.isolated_environment(Path(tmp))
        self.assertNotIn("NAN_API_KEY", env); self.assertNotIn("GITHUB_TOKEN", env)
        self.assertIn("NPM_CONFIG_PREFIX", env); self.assertIn(str(Path(tmp) / "bin"), env["PATH"])

    def test_native_self_test_reports_each_independent_boundary(self):
        with tempfile.TemporaryDirectory() as tmp:
            cell = Path(tmp)
            with patch.object(diagnostic.os, "name", "nt"), patch.object(diagnostic, "protect_private"), \
                 patch.object(diagnostic, "run_bounded", return_value=(1, "nonzero")), \
                 patch.object(diagnostic, "run_bounded_command_line", return_value=(1, "nonzero")):
                result = diagnostic.native_prerequisite_self_test(cell, {"ComSpec": r"C:\Windows\System32\cmd.exe"}, 1)
        self.assertEqual(result["status"], "FAIL")
        self.assertEqual(set(result["checks"]), {
            "pwsh-parser", "cmd-node-npm", "npm-registry", "npm-isolation", "python-venv", "python-pip", "git-bash", "job-dacl", "doctor-json", "cmd-argument-roundtrip"})
        self.assertTrue(all(value["status"] == "FAIL" for value in result["checks"].values()))

    def test_native_collect_preserves_setup_for_safe_report_and_gate(self):
        args = self.args("native-diagnostic")
        setup = {
            "NAN_DIAGNOSTIC_SETUP_CHECKOUT": "success",
            "NAN_DIAGNOSTIC_SETUP_NODE": "success",
            "NAN_DIAGNOSTIC_SETUP_PYTHON": "success",
            "NAN_DIAGNOSTIC_SETUP_RUST": "success",
            "NAN_DIAGNOSTIC_SETUP_SOURCE": "success",
            "NAN_DIAGNOSTIC_SETUP_FIXTURES": "failed-python-regressions",
            "NAN_DIAGNOSTIC_SETUP_RUST_FIXTURE": "success",
            "NAN_DIAGNOSTIC_SETUP_BUILD": "success",
        }
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp)
            with patch.object(diagnostic.os, "name", "nt"), \
             patch.object(diagnostic, "protect_private"), \
             patch.object(diagnostic, "native_prerequisite_self_test", return_value={"status": "PASS", "checks": {}}), \
             patch.dict(diagnostic.os.environ, setup, clear=False):
                report, failed = diagnostic.collect(args, ["codex"], output)
                persisted = json.loads((output / "report.json").read_text())
                view = summary.safe_view(persisted)
                summary_path = output / "safe-summary.md"
            self.assertEqual(summary.main(["--report", str(output / "report.json"), "--output", str(summary_path)]), 0)
            rendered_summary = summary_path.read_text()
        self.assertTrue(failed)
        self.assertEqual(report, persisted)
        self.assertEqual(view["totals"]["selected"], 0)
        self.assertEqual(report["nativePrerequisites"]["status"], "PASS")
        self.assertEqual(report["setup"]["fixtures"], "failed-python-regressions")
        required = ("checkout", "node", "python", "rust", "source", "fixtures", "rust_fixture", "build")
        missing = [name for name in required if not report["setup"].get(name)]
        setup_bad = [name for name in required if report["setup"].get(name) not in ("success", "")]
        self.assertEqual(missing, [])
        self.assertEqual(setup_bad, ["fixtures"])
        self.assertIn("failed-python-regressions", rendered_summary)

    def test_native_collect_without_ci_setup_remains_standalone(self):
        args = self.args("native-diagnostic")
        keys = ("NAN_DIAGNOSTIC_SETUP_CHECKOUT", "NAN_DIAGNOSTIC_SETUP_NODE",
                "NAN_DIAGNOSTIC_SETUP_PYTHON", "NAN_DIAGNOSTIC_SETUP_RUST",
                "NAN_DIAGNOSTIC_SETUP_SOURCE", "NAN_DIAGNOSTIC_SETUP_FIXTURES",
                "NAN_DIAGNOSTIC_SETUP_RUST_FIXTURE", "NAN_DIAGNOSTIC_SETUP_BUILD")
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp)
            with patch.object(diagnostic.os, "name", "nt"), \
                 patch.object(diagnostic, "protect_private"), \
                 patch.object(diagnostic, "native_prerequisite_self_test", return_value={"status": "PASS", "checks": {}}), \
                 patch.dict(diagnostic.os.environ, {key: "" for key in keys}, clear=False):
                report, failed = diagnostic.collect(args, ["codex"], output)
        self.assertFalse(failed)
        self.assertEqual(report["harnesses"], [])
        self.assertTrue(all(report["setup"][key.removeprefix("NAN_DIAGNOSTIC_SETUP_").lower()] == "" for key in keys))

    def test_native_self_test_uses_cmd_argument_boundary_and_safe_reasons(self):
        calls = []
        def fake(argv, cwd, env, timeout):
            calls.append(argv)
            return (1, "nonzero")
        with tempfile.TemporaryDirectory() as tmp:
            cell = Path(tmp)
            with patch.object(diagnostic.os, "name", "nt"), \
                 patch.object(diagnostic, "protect_private"), patch.object(diagnostic.shutil, "which", return_value="resolved"), \
                 patch.object(diagnostic, "run_bounded", side_effect=fake), \
                 patch.object(diagnostic, "run_bounded_command_line", side_effect=fake):
                result = diagnostic.native_prerequisite_self_test(cell, {"ComSpec": r"C:\\Windows\\System32\\cmd.exe", "PATH": "safe"}, 1)
        cmd = next(argv for argv in calls if isinstance(argv, str) and "cmd.exe" in argv and "npm.cmd --version" in argv)
        self.assertIn(" /d /c ", cmd)
        self.assertIn('native self-test space', cmd)
        self.assertEqual(result["checks"]["cmd-node-npm"]["reason"], "version-probe-failed")
        self.assertEqual(result["checks"]["npm-registry"]["reason"], "registry-probe-failed")
        self.assertEqual(result["checks"]["cmd-argument-roundtrip"]["reason"], "argument-roundtrip-failed")
        self.assertIn('NAN_CMD_ARG_OK', next(argv for argv in calls if isinstance(argv, str) and "NAN_CMD_ARG_OK" in argv))

    def test_native_python_probe_selects_configured_minor(self):
        calls = []
        with tempfile.TemporaryDirectory() as tmp:
            cell = Path(tmp)
            with patch.object(diagnostic.os, "name", "nt"), patch.object(diagnostic, "protect_private"), \
                 patch.object(diagnostic.shutil, "which", return_value="resolved"), \
                 patch.object(diagnostic, "run_bounded", side_effect=lambda argv, *rest: (calls.append(argv) or (0, "exit"))), \
                 patch.object(diagnostic, "run_bounded_command_line", return_value=(0, "exit")):
                diagnostic.native_prerequisite_self_test(cell, {"ComSpec": r"C:\\Windows\\System32\\cmd.exe", "PATH": "safe"}, 1, "3.12")
        self.assertIn(["py.exe", "-3.12", "-m", "venv"], [argv[:4] for argv in calls if isinstance(argv, list)])

    def test_native_self_test_distinguishes_missing_path_tool(self):
        with tempfile.TemporaryDirectory() as tmp:
            cell = Path(tmp)
            with patch.object(diagnostic.os, "name", "nt"), \
                 patch.object(diagnostic, "protect_private"), patch.object(diagnostic.shutil, "which", return_value=None), \
                 patch.object(diagnostic, "run_bounded") as run, patch.object(diagnostic, "run_bounded_command_line") as raw_run:
                result = diagnostic.native_prerequisite_self_test(cell, {"ComSpec": r"C:\\Windows\\System32\\cmd.exe", "PATH": "safe"}, 1)
        self.assertEqual(result["checks"]["cmd-node-npm"]["reason"], "executable-missing")
        self.assertEqual(result["checks"]["git-bash"]["reason"], "git-for-windows-missing")
        self.assertEqual(result["checks"]["npm-registry"]["reason"], "executable-missing")
        self.assertNotEqual(run.call_count + raw_run.call_count, 0)

    def test_native_checkpoint_preserves_first_result_before_second_child_interrupts(self):
        snapshots = []
        calls = [0]
        def fake(*_args):
            calls[0] += 1
            if calls[0] == 2:
                raise KeyboardInterrupt
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp:
            cell = Path(tmp)
            with patch.object(diagnostic.os, "name", "nt"), patch.object(diagnostic, "protect_private"), \
                 patch.object(diagnostic.shutil, "which", return_value="resolved"), \
                 patch.object(diagnostic, "run_bounded", side_effect=fake), \
                 patch.object(diagnostic, "run_bounded_command_line", side_effect=fake):
                with self.assertRaises(KeyboardInterrupt):
                    diagnostic.native_prerequisite_self_test(
                        cell, {"ComSpec": r"C:\\Windows\\System32\\cmd.exe", "PATH": "safe"}, 1,
                        checkpoint=lambda name, checks, complete: snapshots.append((name, checks, complete)))
        self.assertEqual(snapshots[0][0], "pwsh-parser")
        self.assertFalse(snapshots[0][2]); self.assertEqual(snapshots[1][0], "pwsh-parser")
        self.assertEqual(snapshots[1][1]["pwsh-parser"]["status"], "PASS")
        self.assertEqual(snapshots[2][0], "cmd-node-npm")
        self.assertEqual(snapshots[2][1]["cmd-node-npm"]["reason"], "unfinished")

    def test_git_bash_resolves_only_git_for_windows_bundle(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); git = root / "cmd" / "git.exe"; bundled = root / "usr" / "bin" / "bash.exe"
            git.parent.mkdir(parents=True); bundled.parent.mkdir(parents=True); git.write_bytes(b""); bundled.write_bytes(b"")
            with patch.object(diagnostic.shutil, "which", return_value=str(git)):
                self.assertEqual(diagnostic._git_for_windows_bash({"PATH": "safe"}), str(bundled))

    def test_grouped_install_causes_are_shared_but_keep_harness_ids(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            (cwd / "installer-result.json").write_text(
                '{"schemaVersion":2,"status":"failed","reason":"installer-failed",'
                '"diagnostic":{"subphase":"install","executable":"npm-cmd",'
                '"exitCode":1,"npmCode":"registry-dns","processReason":"exit-nonzero"}}')
            return (1, "nonzero")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, _ = diagnostic.collect(args, ["codex", "qwen-code"], Path(tmp))
        groups = [value for value in report["groupedCauses"].values() if value["phase"] == "install"]
        self.assertEqual(len(groups), 1); self.assertEqual(groups[0]["count"], 2)
        self.assertEqual(groups[0]["harnesses"], ["codex", "qwen-code"])

    def test_live_without_credentials_is_blocked_and_required(self):
        args = self.args("live")
        def fake(argv, cwd, env, timeout):
            self.write_probe_success(argv, env); return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake), \
             patch.dict(diagnostic.os.environ, {}, clear=True):
            self.binaries(args, Path(tmp))
            report, failed = diagnostic.collect(args, ["claude-code"], Path(tmp))
        self.assertTrue(failed); self.assertEqual(report["harnesses"][0]["phases"]["live-tool"]["status"], "BLOCKED")

    def test_missing_build_still_reports_all_ids_and_blocks_nan_phases(self):
        args = self.args()
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"):
            report, failed = diagnostic.collect(args, list(diagnostic.HARNESSES), Path(tmp))
        self.assertTrue(failed); self.assertEqual(len(report["harnesses"]), 15)
        self.assertTrue(all(item["phases"]["version-doctor"]["status"] == "BLOCKED" for item in report["harnesses"]))

    def test_output_redacts_child_text_and_groups_causal_ids(self):
        args = self.args()
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value=None):
            report, _ = diagnostic.collect(args, ["claude-code"], Path(tmp)); diagnostic.write_outputs(report, Path(tmp))
            payload = (Path(tmp) / "report.json").read_text()
        self.assertNotIn("secret", payload.lower()); self.assertTrue(report["groupedCauses"])

    def test_install_argv_keeps_frozen_hermes_ref_and_private_tool_paths(self):
        args = self.args(); calls = []
        def fake(argv, cwd, env, timeout):
            calls.append((argv, env)); self.write_probe_success(argv, env); return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake), \
             patch.dict(diagnostic.os.environ, {"NAN_API_KEY": "secret", "GITHUB_TOKEN": "secret"}):
            self.binaries(args, Path(tmp)); report, _ = diagnostic.collect(args, ["hermes"], Path(tmp))
        install = next(call for call in calls if "-Harness" in call[0] and "hermes" in call[0])
        self.assertEqual(install[0][0], "pwsh"); self.assertIn("-Ref", install[0]); self.assertIn("a" * 40, install[0])
        self.assertIn(("-PythonVersion", "3.12"), list(zip(install[0], install[0][1:])))
        self.assertNotIn("NAN_API_KEY", install[1]); self.assertNotIn("GITHUB_TOKEN", install[1])
        self.assertIn(str(Path(tmp) / "cells/hermes/hermes/bin"), install[1]["PATH"])
        self.assertIn(str(Path(tmp) / "cells/hermes/home/.nan-harness-canary-venv/Scripts"), install[1]["PATH"])

    def test_closed_installer_reason_reaches_phase_without_raw_message(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            if "-Harness" in argv and "hermes" in argv:
                (cwd / "installer-result.json").write_text(
                    '{"schemaVersion":1,"status":"failed","reason":"capability-not-implemented"}')
                return (1, "nonzero")
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["hermes"], Path(tmp))
        self.assertTrue(failed); install = report["harnesses"][0]["phases"]["install"]
        self.assertEqual(install["reason"], "installer-nonzero")
        self.assertEqual(install["causeDetails"]["parentReason"], "nonzero")
        self.assertEqual(install["causeDetails"]["installerReason"], "capability-not-implemented")
        self.assertNotIn("raw", str(report))

    def test_v2_installer_diagnostic_reaches_phase_with_closed_values(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            if "-Harness" in argv and "codex" in argv:
                (cwd / "installer-result.json").write_text(
                    '{"schemaVersion":2,"status":"failed","reason":"installer-failed",'
                    '"diagnostic":{"subphase":"install","executable":"npm-cmd",'
                    '"exitCode":1,"npmCode":"registry-dns","processReason":"exit-nonzero"}}')
                return (1, "nonzero")
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertTrue(failed)
        install = report["harnesses"][0]["phases"]["install"]
        self.assertEqual(install["reason"], "installer-nonzero")
        self.assertEqual(install["causeDetails"]["parentReason"], "nonzero")
        self.assertEqual(install["causeDetails"]["installerReason"], "installer-failed")
        self.assertEqual(install["diagnostic"], {"subphase": "install", "executable": "npm-cmd", "exitCode": 1,
                                                  "npmCode": "registry-dns", "processReason": "exit-nonzero"})

    def test_invalid_v2_installer_diagnostic_falls_back_without_raw_values(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            (cwd / "installer-result.json").write_text(
                '{"schemaVersion":2,"status":"failed","reason":"installer-failed",'
                '"diagnostic":{"subphase":"install","executable":"not-a-real-tool",'
                '"exitCode":1,"message":"secret"}}')
            return (1, "nonzero")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertTrue(failed)
        install = report["harnesses"][0]["phases"]["install"]
        self.assertNotIn("diagnostic", install)
        self.assertNotIn("secret", str(report))

    def test_probe_v2_diagnostics_and_exit_code_are_closed_in_phase(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            if "-Stage" in argv and "version-doctor" in argv:
                Path(env["NAN_CANARY_PROBE_RESULT"]).write_text(
                    '{"schemaVersion":2,"stage":"version-doctor","status":"failed",'
                    '"diagnostics":["doctor-exit-nonzero","doctor-schema-invalid"],"exitCode":17,'
                    '"doctorSchemaReason":"field-type"}')
                return (1, "nonzero")
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertTrue(failed)
        doctor = report["harnesses"][0]["phases"]["version-doctor"]
        self.assertEqual(doctor["reason"], "probe-nonzero")
        self.assertEqual(doctor["diagnostic"], {"markerState": "valid", "stage": "version-doctor", "diagnostics": ["doctor-exit-nonzero", "doctor-schema-invalid"], "exitCode": 17, "doctorSchemaReason": "field-type"})

    def test_nonzero_conformance_parent_preserves_closed_marker_fields(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            if "-Stage" in argv and "deterministic-contract" in argv:
                Path(env["NAN_CANARY_PROBE_RESULT"]).write_text(
                    '{"schemaVersion":2,"stage":"deterministic-contract","status":"failed",'
                    '"diagnostics":["conformance-exit-nonzero","conformance-inventory-failed"],'
                    '"exitCode":23,"inventoryFailureReasons":["provider-failed"],'
                    '"inventoryProcess":{"status":"nonzero-exit","exitCode":23}}')
                return (23, "nonzero")
            if "-Stage" in argv:
                self.write_probe_success(argv, env)
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertTrue(failed)
        contract = report["harnesses"][0]["phases"]["deterministic-contract"]
        self.assertEqual(contract["reason"], "probe-nonzero")
        self.assertEqual(contract["diagnostic"], {
            "markerState": "valid",
            "stage": "deterministic-contract",
            "diagnostics": ["conformance-exit-nonzero", "conformance-inventory-failed"],
            "exitCode": 23,
            "inventoryFailureReasons": ["provider-failed"],
            "inventoryProcess": {"status": "nonzero-exit", "exitCode": 23},
            "progress": {"progressStatus": "absent"},
        })

    def test_nonzero_parent_without_probe_marker_keeps_generic_reason(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            if "-Stage" in argv and "deterministic-contract" in argv:
                return (23, "nonzero")
            if "-Stage" in argv:
                self.write_probe_success(argv, env)
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertTrue(failed)
        contract = report["harnesses"][0]["phases"]["deterministic-contract"]
        self.assertEqual(contract["reason"], "probe-nonzero")
        self.assertEqual(contract["diagnostic"], {"markerState": "absent", "progress": {"progressStatus": "absent"}})

    def test_nonzero_parent_cannot_be_overridden_by_success_marker(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            if "-Stage" in argv and "deterministic-contract" in argv:
                Path(env["NAN_CANARY_PROBE_RESULT"]).write_text(
                    '{"schemaVersion":2,"stage":"complete","status":"passed",'
                    '"diagnostics":[],"exitCode":0}')
                return (23, "nonzero")
            if "-Stage" in argv:
                self.write_probe_success(argv, env)
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertTrue(failed)
        contract = report["harnesses"][0]["phases"]["deterministic-contract"]
        self.assertEqual(contract["status"], "FAIL")
        self.assertEqual(contract["reason"], "probe-nonzero")

    def test_real_pwsh_producer_marker_survives_reader_and_summary(self):
        pwsh = shutil.which("pwsh")
        if pwsh is None:
            self.skipTest("portable pwsh is required for the producer integration test")
        payload = json.dumps({
            "schemaVersion": 2,
            "harness": "codex",
            "outcome": "failed",
            "durationMilliseconds": 7,
            "scenarios": [
                {"name": "external-prerequisite", "status": "skipped", "checks": [{"name": "auth", "status": "skipped", "durationMilliseconds": 0}], "durationMilliseconds": 0},
                {"name": "inventory", "status": "failed", "checks": [{"name": "registry", "status": "failed", "durationMilliseconds": 1}], "durationMilliseconds": 1},
                {"name": "sentinel", "status": "passed", "checks": [{"name": "sentinel", "status": "passed", "durationMilliseconds": 1}], "durationMilliseconds": 1},
                {"name": "tool-round-trip", "status": "passed", "checks": [{"name": "tool", "status": "passed", "durationMilliseconds": 1}], "durationMilliseconds": 1},
            ],
            "inventoryFailureReasons": ["provider-failed"],
            "inventoryProcess": {"status": "nonzero-exit", "exitCode": 1},
        })
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            marker = root / "probe-result.json"
            fake_canary = root / "fake-canary.ps1"
            fake_canary.write_text(f"Write-Output '{payload}'; exit 1\n", encoding="utf-8")
            env = os.environ.copy()
            env["NAN_CANARY_PROBE_RESULT"] = str(marker)
            probe = ROOT / "guest" / "probe-harness.ps1"
            result = subprocess.run(
                [pwsh, "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", str(probe),
                 "-Harness", "codex", "-Stage", "deterministic-contract", "-NanBinary", str(fake_canary),
                 "-Canary", str(fake_canary), "-Version", "1.2.3"],
                env=env, capture_output=True, text=True, timeout=30,
            )
            self.assertNotEqual(result.returncode, 0)
            parsed = diagnostic.probe_diagnostic(root, "deterministic-contract")
            self.assertEqual(parsed["stage"], "deterministic-contract")
            self.assertEqual(parsed["exitCode"], 1)
            self.assertEqual(parsed["inventoryProcess"], {"status": "nonzero-exit", "exitCode": 1})
            self.assertIn("conformance-inventory-operational-failed", parsed["diagnostics"])
            diagnostic_value = {key: value for key, value in parsed.items() if key != "status"}
            phases = {name: {"status": "NOT_REQUESTED", "reason": "not-started"} for name in summary.PHASES}
            phases["deterministic-contract"] = {"status": "FAIL", "reason": "probe-nonzero", "diagnostic": diagnostic_value}
            report = {"schemaVersion": 1, "mode": "deterministic", "sourceSha": "a" * 40,
                      "harnesses": [{"harness": "codex", "outcome": "failed", "phases": phases}],
                      "totals": {"selected": 1, "passed": 0, "failed": 1, "blocked": 0}}
            rendered = summary.render(summary.safe_view(report))
            self.assertIn("conformance-inventory-operational-failed", rendered)
            invalid_root = root / "invalid"
            invalid_root.mkdir()
            invalid_marker = invalid_root / "probe-result.json"
            fake_canary.write_text(
                f"Write-Output '{json.dumps({**json.loads(payload), 'inventoryProcess': {'status': 'nonzero-exit', 'exitCode': 1, 'SECRET': 'SECRET'}})}'; exit 1\n",
                encoding="utf-8",
            )
            invalid_env = dict(env)
            invalid_env["NAN_CANARY_PROBE_RESULT"] = str(invalid_marker)
            invalid_result = subprocess.run(
                [pwsh, "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", str(probe),
                 "-Harness", "codex", "-Stage", "deterministic-contract", "-NanBinary", str(fake_canary),
                 "-Canary", str(fake_canary), "-Version", "1.2.3"],
                env=invalid_env, capture_output=True, text=True, timeout=30,
            )
            self.assertNotEqual(invalid_result.returncode, 0)
            emitted = invalid_marker.read_text(encoding="utf-8-sig")
            self.assertNotIn("SECRET", emitted)
            invalid_parsed = diagnostic.probe_diagnostic(invalid_root, "deterministic-contract")
            self.assertEqual(invalid_parsed["markerState"], "valid")
            self.assertIn("conformance-schema-invalid", invalid_parsed["diagnostics"])

    def test_probe_diagnostic_allowlists_match_powershell_producer(self):
        producer = (ROOT / "guest" / "probe-harness.ps1").read_text(encoding="utf-8")
        match = re.search(r"\$knownDiagnostics\s*=\s*@\((.*?)\)", producer, re.DOTALL)
        self.assertIsNotNone(match)
        emitted = set(re.findall(r"'([^']+)'", match.group(1)))
        self.assertTrue(emitted <= diagnostic._PROBE_DIAGNOSTICS)
        self.assertTrue(emitted <= summary.PROBE_DIAGNOSTICS)

    def test_invalid_probe_diagnostics_do_not_escape_as_raw_data(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            if "-Stage" in argv and "version-doctor" in argv:
                Path(env["NAN_CANARY_PROBE_RESULT"]).write_text(
                    '{"schemaVersion":2,"stage":"version-doctor","status":"failed",'
                    '"diagnostics":["secret"],"exitCode":999999}')
                return (1, "nonzero")
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertTrue(failed)
        doctor = report["harnesses"][0]["phases"]["version-doctor"]
        self.assertEqual(doctor["diagnostic"], {"markerState": "invalid"})
        self.assertNotIn("secret", str(report))

    def test_probe_marker_states_distinguish_absent_and_rejected_markers(self):
        with tempfile.TemporaryDirectory() as tmp:
            cell = Path(tmp)
            self.assertEqual(diagnostic.probe_diagnostic(cell, "version-doctor"),
                             {"status": "failed", "markerState": "absent"})
            marker = cell / "probe-result.json"
            for value in ("not-json", {"schemaVersion": 2, "stage": "version-doctor",
                                        "status": "failed", "diagnostics": ["unknown-code"], "exitCode": 1},
                          {"schemaVersion": 2, "stage": "version-doctor", "status": "failed",
                           "diagnostics": ["doctor-version-mismatch"], "exitCode": {"secret": "value"}},
                          {"schemaVersion": 2, "stage": "deterministic-contract", "status": "failed",
                           "diagnostics": ["conformance-inventory-failed"], "exitCode": 1,
                           "inventoryFailureReasons": [{"secret": "value"}]}):
                marker.write_text(value if isinstance(value, str) else json.dumps(value), encoding="utf-8")
                parsed = diagnostic.probe_diagnostic(cell, "version-doctor")
                self.assertEqual(parsed, {"status": "failed", "markerState": "invalid"})
            marker.write_text("x" * (diagnostic.PROBE_MARKER_MAX_BYTES + 1), encoding="utf-8")
            self.assertEqual(diagnostic.probe_diagnostic(cell, "version-doctor"),
                             {"status": "failed", "markerState": "invalid"})

    def test_invalid_marker_state_is_safe_in_summary(self):
        value = report = {"schemaVersion": 1, "mode": "deterministic", "sourceSha": "a" * 40,
                          "harnesses": [{"harness": "codex", "outcome": "failed", "phases": {
                              phase: {"status": "NOT_REQUESTED", "reason": "not-started"}
                              for phase in summary.PHASES}}],
                          "totals": {"selected": 1, "passed": 0, "failed": 1, "blocked": 0}}
        value["harnesses"][0]["phases"]["version-doctor"] = {
            "status": "FAIL", "reason": "probe-nonzero",
            "diagnostic": {"markerState": "invalid"}}
        rendered = summary.render(summary.safe_view(report))
        self.assertIn("markerState=invalid", rendered)
        value["harnesses"][0]["phases"]["version-doctor"]["diagnostic"]["markerState"] = "secret"
        with self.assertRaises(summary.UnsafeReport):
            summary.safe_view(value)

    def test_probe_v2_success_requires_complete_zero_exit_and_empty_diagnostics(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            self.write_probe_success(argv, env)
            if "-Stage" in argv and "version-doctor" in argv:
                Path(env["NAN_CANARY_PROBE_RESULT"]).write_text(
                    '{"schemaVersion":2,"stage":"complete","status":"passed",'
                    '"diagnostics":[],"exitCode":0}')
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertFalse(failed)
        self.assertEqual(report["harnesses"][0]["phases"]["version-doctor"]["status"], "PASS")

    def test_probe_v2_success_rejects_missing_exit_evidence(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            if "-Stage" in argv and "version-doctor" in argv:
                Path(env["NAN_CANARY_PROBE_RESULT"]).write_text(
                    '{"schemaVersion":2,"stage":"complete","status":"passed",'
                    '"diagnostics":[]}')
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertTrue(failed)
        self.assertEqual(report["harnesses"][0]["phases"]["version-doctor"]["status"], "FAIL")

    def test_probe_producer_examples_are_consumable_by_collector(self):
        producer = (ROOT / "guest" / "probe-harness.ps1").read_text(encoding="utf-8")
        self.assertIn("diagnostics = @($diagnostics.ToArray())", producer)
        self.assertIn("$value.exitCode = [int]$exitCode", producer)
        examples = (
            {"schemaVersion": 2, "stage": "complete", "status": "passed", "diagnostics": [], "exitCode": 0},
            {"schemaVersion": 2, "stage": "completion-marker", "status": "failed",
             "diagnostics": ["live-completion-marker-missing"]},
        )
        with tempfile.TemporaryDirectory() as tmp:
            marker = Path(tmp) / "probe-result.json"
            for example in examples:
                marker.write_text(json.dumps(example))
                parsed = diagnostic.probe_diagnostic(Path(tmp), "live-tool")
                self.assertEqual(parsed["status"], example["status"])

    def test_probe_v2_doctor_and_inventory_fields_are_closed(self):
        reasons = ("process-failed", "marker-missing", "provider-failed",
                   "provider-shutdown-failed", "daemon-cleanup-failed")
        with tempfile.TemporaryDirectory() as tmp:
            marker = Path(tmp) / "probe-result.json"
            for reason in reasons:
                marker.write_text(json.dumps({
                    "schemaVersion": 2, "stage": "deterministic-contract", "status": "failed",
                    "diagnostics": ["conformance-inventory-failed"], "exitCode": 1,
                    "inventoryFailureReasons": [reason]}))
                parsed = diagnostic.probe_diagnostic(Path(tmp), "deterministic-contract")
                self.assertEqual(parsed["inventoryFailureReasons"], [reason])
            marker.write_text(json.dumps({
                "schemaVersion": 2, "stage": "version-doctor", "status": "failed",
                "diagnostics": ["doctor-version-mismatch"], "exitCode": 1,
                "doctorVersion": "1.2.3", "doctorExpectedVersion": "1.2.4",
                "doctorReason": "mismatch"}))
            parsed = diagnostic.probe_diagnostic(Path(tmp), "version-doctor")
            self.assertEqual(parsed["doctorVersion"], "1.2.3")
            self.assertEqual(parsed["doctorReason"], "mismatch")

    def test_probe_v2_inventory_process_evidence_is_closed(self):
        with tempfile.TemporaryDirectory() as tmp:
            marker = Path(tmp) / "probe-result.json"
            base = {"schemaVersion": 2, "stage": "deterministic-contract", "status": "failed",
                    "diagnostics": ["conformance-inventory-failed"], "exitCode": 1,
                    "inventoryFailureReasons": ["process-failed"]}
            for evidence in ({"status": "nonzero-exit", "exitCode": -1073741819},
                             {"status": "nonzero-exit", "exitCode": -2147483648},
                             {"status": "nonzero-exit", "exitCode": 2147483647},
                             {"status": "launch-error", "osErrorCode": 2},
                             {"status": "environment-error", "osErrorCode": 5},
                             {"status": "timeout", "timeoutMilliseconds": 90000},
                             {"status": "capture-error"}, {"status": "cleanup-error"}):
                marker.write_text(json.dumps({**base, "inventoryProcess": evidence}), encoding="utf-8")
                self.assertEqual(diagnostic.probe_diagnostic(Path(tmp), "deterministic-contract")["inventoryProcess"], evidence)
            for bad in ({"status": "nonzero-exit", "exitCode": 2147483648},
                        {"status": "nonzero-exit", "exitCode": -2147483649},
                        {"status": "nonzero-exit", "exitCode": True},
                        {"status": "nonzero-exit", "exitCode": 1.5},
                        {"status": "nonzero-exit", "exitCode": "secret"}):
                marker.write_text(json.dumps({**base, "inventoryProcess": bad}), encoding="utf-8")
                self.assertEqual(diagnostic.probe_diagnostic(Path(tmp), "deterministic-contract"),
                                 {"status": "failed", "markerState": "invalid"})

    def test_probe_v2_rejects_bad_optional_fields_and_unknown_inventory_text(self):
        with tempfile.TemporaryDirectory() as tmp:
            marker = Path(tmp) / "probe-result.json"
            base = {"schemaVersion": 2, "stage": "version-doctor", "status": "failed",
                    "diagnostics": ["doctor-version-mismatch"], "exitCode": 1}
            for field, value in (("doctorVersion", "1"), ("doctorReason", "secret"),
                                 ("discoveryCode", "NH-DISCOVERY-999"),
                                 ("inventoryFailureReasons", ["raw"]),
                                 ("unknown", "value")):
                marker.write_text(json.dumps({**base, field: value}))
                self.assertEqual(diagnostic.probe_diagnostic(Path(tmp), "version-doctor")["status"], "failed")

    def test_live_failure_marker_may_omit_exit_when_no_subprocess_ran(self):
        args = self.args("live")
        def fake(argv, cwd, env, timeout):
            self.write_probe_success(argv, env)
            if "-Stage" in argv and "live-tool" in argv:
                Path(env["NAN_CANARY_PROBE_RESULT"]).write_text(
                    '{"schemaVersion":2,"stage":"completion-marker","status":"failed",'
                    '"diagnostics":["live-completion-marker-missing"]}')
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake), \
             patch.dict(diagnostic.os.environ, {"NAN_API_KEY": "synthetic"}):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertTrue(failed)
        live = report["harnesses"][0]["phases"]["live-tool"]
        self.assertEqual(live["status"], "FAIL")
        self.assertEqual(live["diagnostic"]["stage"], "completion-marker")


if __name__ == "__main__": unittest.main()
