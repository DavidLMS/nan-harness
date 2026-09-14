#!/usr/bin/env python3
"""Focused offline contracts for the hosted CLI execution pair."""

import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch


ACTION_DIRECTORY = Path(__file__).resolve().parents[1] / "actions"
sys.path.insert(0, str(ACTION_DIRECTORY))

# The selector is workflow-owned and arrives from the parallel selector change;
# keep these execution tests independent of that interface branch.
selection = type(sys)("selection")
selection.CLI_HARNESSES = (
    "claude-code", "codex", "opencode", "hermes", "pi", "omp", "prime-agent",
    "deepseek-harness", "openclaw", "cline", "qwen-code", "kimi-code", "aider",
    "goose", "fx",
)
selection.resolve_model = lambda requested="", configured=None: requested or configured or "qwen3.6"
sys.modules["selection"] = selection


def load(name):
    spec = importlib.util.spec_from_file_location(name, ACTION_DIRECTORY / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


cell = load("cell")
cli_suite = load("cli-suite")


class CliExecutionTests(unittest.TestCase):
    @staticmethod
    def install_args(root):
        return type("Args", (), {
            "directory": root / "cell", "binary": root / "nan-harness",
            "harness": "openclaw", "harness_version": "1.2.3", "harness_ref": "",
        })()

    def test_source_identity_is_exact_and_private(self):
        self.assertEqual(cell.source_identity("a" * 40), "commit:" + "a" * 40)
        for value in ("A" * 40, "a" * 39, "a" * 41, "release"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                cell.source_identity(value)

    def test_initial_state_binds_arm_identity_and_source(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "nan"
            binary.write_bytes(b"binary")
            args = type("Args", (), {
                "system": "linux", "architecture": "aarch64", "harness": "codex",
                "trigger": "manual", "run_id": "run-1", "model": "qwen3.6",
                "tag": "v1.2.3", "binary": binary, "source_sha": "b" * 40,
            })()
            with patch.object(cell.sys, "platform", "linux"), \
                    patch.object(cell.os, "uname", return_value=type("Uname", (), {"machine": "aarch64"})()), \
                    patch.object(cell.subprocess, "run", return_value=type("Result", (), {
                        "stdout": b"24.20.0\n"})()):
                state = cell.initial_state(args)
            self.assertEqual(state["environment"]["operatingSystem"], "linux")
            self.assertEqual(state["environment"]["architecture"], "aarch64")
            self.assertEqual(state["nanHarness"]["source"], "commit:" + "b" * 40)
            self.assertEqual(state["harness"]["id"], "codex")

    def test_initial_state_rejects_non_arm64_or_unsupported_system(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "nan"
            binary.write_bytes(b"binary")
            args = type("Args", (), {
                "system": "linux", "architecture": "x86_64", "harness": "codex",
                "trigger": "manual", "run_id": "run-1", "model": "qwen3.6",
                "tag": "v1.2.3", "binary": binary, "source_sha": "b" * 40,
            })()
            with patch.object(cell.sys, "platform", "linux"), \
                    patch.object(cell.os, "uname", return_value=type("Uname", (), {"machine": "x86_64"})()):
                with self.assertRaises(RuntimeError):
                    cell.initial_state(args)

    def test_install_reports_installer_failure_without_replacing_cleanup(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            args = self.install_args(root)
            state = {"harness": {"version": "unknown"}}
            with patch.object(cell, "private_command", side_effect=RuntimeError("private")), \
                    self.assertRaises(cell.InstallFailure) as error:
                cell.install(args, state)
            self.assertEqual(error.exception.phase, cell.INSTALLER_FAILURE_PHASE)
            with patch.object(cell, "private_command", side_effect=cell.CleanupError("cleanup")), \
                    self.assertRaises(cell.CleanupError):
                cell.install(args, state)

    def test_private_installer_capture_classifies_bounded_private_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "cell"
            cell.ensure_private_directory(root)
            observed = []

            def capture(log, status):
                observed.append((cell.classify_install_failure(log, status), status))

            status = cell.private_command([
                sys.executable, "-c",
                "import sys; print('npm ERR! code EAI_AGAIN secret-token', file=sys.stderr); sys.exit(1)",
            ], root, allow_failure=True, diagnostic_callback=capture)
            self.assertEqual(status, 1)
            self.assertEqual(observed, [("npm-network", 1)])
            self.assertFalse(any(root.iterdir()))

            huge = io.BytesIO(("npm ERR! code EACCES secret-token " + "x" * cell.PRIVATE_DIAGNOSTIC_LIMIT).encode())
            self.assertEqual(cell.classify_install_failure(huge, 1), "npm-permission")
            self.assertEqual(cell.classify_install_failure(
                io.BytesIO(b"npm WARN EBADENGINE /tmp/e404/secret-token\nfatal: unrelated"), 1),
                "diagnostic-unknown")
            self.assertEqual(cell.classify_install_failure(
                io.BytesIO(b"npm WARN EBADENGINE\nnpm ERR! code E404"), 1),
                "npm-package-not-found")
            self.assertEqual(cell.classify_install_failure(
                io.BytesIO(b"npm ERR! code E404\nnpm ERR! code EACCES"), 1),
                "diagnostic-unknown")
            self.assertEqual(cell.classify_install_failure(
                io.BytesIO(b"npm ERR! path /tmp/node_modules/openclaw\n"
                            b"npm ERR! command sh -c node scripts/postinstall-bundled-plugins.mjs"),
                1, "openclaw"), "npm-openclaw-postinstall")
            preinstall = (b"npm ERR! path /tmp/node_modules/openclaw\n"
                          b"npm ERR! command sh -c node scripts/preinstall-package-manager-warning.mjs\n"
                          b"npm ERR! [openclaw] error: this OpenClaw release requires Node >=24.15.0 <25.\n"
                          b"npm ERR! [openclaw] detected Node 24.14.0 (exec: /private/secret/node)\n")
            self.assertEqual(cell.classify_install_failure(io.BytesIO(preinstall), 1, "openclaw"),
                             "npm-openclaw-preinstall-runtime")
            self.assertEqual(cell.classify_install_failure(io.BytesIO(preinstall), -9, "openclaw"),
                             "npm-openclaw-preinstall-runtime-signal")
            self.assertEqual(cell.classify_install_failure(io.BytesIO(
                b"npm ERR! path /tmp/node_modules/openclaw\n"
                b"npm ERR! command sh -c node scripts/preinstall-package-manager-warning.mjs\n"
                b"npm ERR! [openclaw] error: could not remove the legacy package install guard: EACCES\n"),
                1, "openclaw"), "npm-openclaw-preinstall-legacy-guard")
            self.assertEqual(cell.classify_install_failure(io.BytesIO(
                b"npm ERR! path /tmp/node_modules/openclaw\n"
                b"npm ERR! command sh -c node scripts/preinstall-package-manager-warning.mjs\n"
                b"npm ERR! Error [ERR_MODULE_NOT_FOUND]: Cannot find module 'private'\n"),
                1, "openclaw"), "npm-openclaw-preinstall-module")
            self.assertEqual(cell.classify_install_failure(
                io.BytesIO(b"npm ERR! path /tmp/node_modules/@clack/core\n"
                            b"npm ERR! code 1\n"
                            b"npm ERR! command failed\n"
                            b"npm ERR! command sh -c node scripts/install.js"),
                1, "openclaw"), cell.dependency_failure_code("@clack/core", "sh", "exit"))
            self.assertEqual(cell.classify_install_failure(
                io.BytesIO(b"npm ERR! path /tmp/node_modules/openclaw-suffix\n"
                            b"npm ERR! command sh -c node scripts/postinstall-bundled-plugins.mjs"),
                1, "openclaw"), "diagnostic-unknown")
            self.assertEqual(cell.classify_install_failure(
                io.BytesIO(b"npm ERR! path /tmp/node_modules/@clack/core-suffix\n"
                            b"npm ERR! code 1\n"
                            b"npm ERR! command failed\n"
                            b"npm ERR! command sh -c node scripts/install.js"),
                1, "openclaw"), "diagnostic-unknown")
            self.assertEqual(cell.classify_install_failure(
                io.BytesIO(b"npm ERR! path /tmp/node_modules/other\n"
                            b"npm ERR! command sh -c node scripts/postinstall-bundled-plugins.mjs"),
                1, "openclaw"), "diagnostic-unknown")
            self.assertEqual(cell.classify_install_failure(
                io.BytesIO(b"npm ERR! path /tmp/node_modules/@clack/core\n"
                            b"npm ERR! code 1\n"
                            b"npm ERR! command failed\n"
                            b"npm ERR! command sh -c node scripts/install.js\n"
                            b"\n"
                            b"npm ERR! path /tmp/node_modules/tar\n"
                            b"npm ERR! command failed\n"
                            b"npm ERR! command sh -c node scripts/install.js"),
                1, "openclaw"), "diagnostic-unknown")
            self.assertEqual(cell.classify_install_failure(io.BytesIO(b"safe private output"), 1),
                             "diagnostic-unknown")
            self.assertEqual(cell.classify_install_failure(
                io.BytesIO(b"npm ERR! path /tmp/node_modules/@clack/core\n"
                            b"npm ERR! code 1\n"
                            b"npm ERR! command failed\n"
                            b"npm ERR! command sh -c node scripts/install.js"),
                -9, "openclaw"), cell.dependency_failure_code("@clack/core", "sh", "signal"))
            self.assertEqual(cell.classify_install_failure(io.BytesIO(b"secret-token"), 0),
                             "diagnostic-unknown")
            self.assertEqual(cell.dependency_failure_code("@clack/core", "sh", "exit"),
                             "NH-CLI-DEP-S-CLACK-CORE-SH-EXIT")
            self.assertNotEqual(cell.dependency_failure_code("@clack/core", "sh", "exit"),
                                cell.dependency_failure_code("clack-core", "sh", "exit"))
            self.assertIsNone(cell.dependency_failure_code("@clack/core-suffix", "sh", "exit"))
            self.assertIsNone(cell.dependency_failure_code("@clack/core", "python", "exit"))
            all_codes = [cell.dependency_failure_code(package, executable, termination)
                         for package in cell.OPENCLAW_DEPENDENCIES
                         for executable in cell.DEPENDENCY_EXECUTABLES
                         for termination in cell.DEPENDENCY_TERMINATIONS]
            self.assertEqual(len(all_codes), len(set(all_codes)))
            self.assertTrue(all(code in cell.INSTALL_FAILURE_CODES for code in all_codes))

    def test_install_distinguishes_doctor_exit_and_version_mismatch(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            args = self.install_args(root)
            state = {"harness": {"version": "unknown"}}
            with patch.object(cell, "private_command", side_effect=[None, RuntimeError("doctor")]), \
                    self.assertRaises(cell.InstallFailure) as error:
                cell.install(args, state)
            self.assertEqual(error.exception.phase, cell.DOCTOR_FAILURE_PHASE)

            def write_wrong_doctor(_command, _directory, output=None, **_kwargs):
                if output is not None:
                    output.write_text(json.dumps({"version": "9.9.9"}))

            with patch.object(cell, "private_command", side_effect=write_wrong_doctor), \
                    self.assertRaises(cell.InstallFailure) as error:
                cell.install(args, state)
            self.assertEqual(error.exception.phase, cell.DOCTOR_VERSION_FAILURE_PHASE)
            self.assertFalse((args.directory / "doctor.json").exists())

    def test_install_dependency_diagnostic_reaches_final_report(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            args = self.install_args(root)
            args.output = root / "report.json"
            args.stage = "install"
            args.trigger = "manual"
            args.model = "qwen3.6"
            args.canary = root / "canary"
            args.directory.mkdir()
            (args.directory / "state.json").write_text(json.dumps({
                "startedAt": cell.timestamp(), "durationMilliseconds": 0, "checks": [],
                "outcome": "passed",
            }))
            npm_failure = (b"npm ERR! path /private/node_modules/@clack/core\n"
                           b"npm ERR! code 1\n"
                           b"npm ERR! command failed\n"
                           b"npm ERR! command sh -c node scripts/install.js\n"
                           b"secret-token")

            def fake_private(command, _directory, diagnostic_callback=None, **_kwargs):
                if diagnostic_callback is not None:
                    diagnostic_callback(io.BytesIO(npm_failure), 1)
                    return 1
                return 0

            with patch.object(cell, "private_command", side_effect=fake_private), \
                    self.assertRaises(cell.InstallFailure) as error:
                cell.install(args, {"harness": {"version": "unknown"}})
            with patch.object(cell, "private_command", return_value=0):
                cell.failed_report(args, error.exception)
            report = json.loads(args.output.read_text())
            code = report["failure"]["code"]
            self.assertEqual(code, "NH-CLI-DEP-S-CLACK-CORE-SH-EXIT")
            self.assertNotIn("secret-token", args.output.read_text())

    def test_failed_install_report_keeps_closed_subphase(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            args = type("Args", (), {
                "directory": root / "cell", "output": root / "report.json", "stage": "install",
                "trigger": "manual", "model": "qwen3.6", "harness": "openclaw",
                "canary": root / "canary",
            })()
            args.directory.mkdir()
            (args.directory / "state.json").write_text(json.dumps({
                "startedAt": cell.timestamp(), "durationMilliseconds": 0, "checks": [],
                "outcome": "passed",
            }))
            with patch.object(cell, "private_command", return_value=0):
                cell.failed_report(args, cell.InstallFailure(cell.INSTALLER_FAILURE_PHASE, "npm-network"))
            report = json.loads(args.output.read_text())
            self.assertEqual(report["checks"][0]["name"], cell.INSTALLER_FAILURE_PHASE)
            self.assertEqual(report["failure"]["phase"], cell.INSTALLER_FAILURE_PHASE)
            self.assertEqual(report["failure"]["code"], "npm-network")

    def test_selected_harness_runs_only_requested_deterministic_stages_without_key(self):
        calls = []
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "nan"
            canary = root / "canary"
            binary.write_bytes(b"binary")
            canary.write_bytes(b"canary")
            manifest = root / "manifest.json"
            manifest.write_text(json.dumps({"harnesses": [{
                "harness": "codex", "version": "1.2.3", "system": "linux",
                "architecture": "aarch64", "source": "npm:@openai/codex",
                "package": "@openai/codex", "model": "qwen3.6", "ref": "",
            }]}))
            output = root / "reports"
            argv = ["cli-suite.py", "--harnesses", "codex", "--mode", "deterministic",
                    "--trigger", "manual", "--tag", "v1.2.3", "--model", "qwen3.6",
                    "--binary", str(binary), "--canary", str(canary), "--directory", str(root / "cells"),
                    "--output", str(output), "--run-id", "run-1", "--system", "linux",
                    "--architecture", "aarch64", "--source-kind", "branch", "--source-sha", "a" * 40,
                    "--nan-version", "1.2.3", "--manifest", str(manifest)]

            def run(command, **kwargs):
                calls.append((command, kwargs))
                return type("Result", (), {"returncode": 0})()

            with patch.object(sys, "argv", argv), patch.dict(os.environ, {"NAN_API_KEY": "secret"}), \
                    patch.object(cli_suite.subprocess, "run", side_effect=run):
                self.assertEqual(cli_suite.main(), 0)
            self.assertEqual(len(calls), 3)
            self.assertEqual({command[2] for command, _ in calls}, {"install", "conformance", "report"})
            self.assertTrue(all("--harness" in command and command[command.index("--harness") + 1] == "codex"
                                for command, _ in calls))
            self.assertTrue(all("NAN_API_KEY" not in kwargs["env"] for _, kwargs in calls))

    def test_live_mode_has_key_only_for_live_stage(self):
        calls = []
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name, data in (("nan", b"binary"), ("canary", b"canary")):
                (root / name).write_bytes(data)
            manifest = root / "manifest.json"
            manifest.write_text(json.dumps({"harnesses": [{
                "harness": "codex", "version": "1.2.3", "system": "linux",
                "architecture": "aarch64", "source": "npm:@openai/codex",
                "package": "@openai/codex", "model": "qwen3.6", "ref": "",
            }]}))
            argv = ["cli-suite.py", "--harnesses", "codex", "--mode", "live", "--trigger", "manual",
                    "--tag", "v1.2.3", "--model", "qwen3.6", "--binary", str(root / "nan"),
                    "--canary", str(root / "canary"), "--directory", str(root / "cells"),
                    "--output", str(root / "reports"), "--run-id", "run-1", "--system", "linux",
                    "--architecture", "aarch64", "--source-kind", "branch", "--source-sha", "a" * 40,
                    "--nan-version", "1.2.3", "--manifest", str(manifest)]

            def run(command, **kwargs):
                calls.append((command, kwargs))
                return type("Result", (), {"returncode": 0})()

            with patch.object(sys, "argv", argv), patch.dict(os.environ, {"NAN_API_KEY": "secret"}), \
                    patch.object(cli_suite.subprocess, "run", side_effect=run):
                self.assertEqual(cli_suite.main(), 0)
            self.assertEqual(len(calls), 4)
            for command, kwargs in calls:
                stage = command[2]
                self.assertEqual("NAN_API_KEY" in kwargs["env"], stage == "live")


if __name__ == "__main__":
    unittest.main()
