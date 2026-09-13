#!/usr/bin/env python3
"""Focused offline contracts for the hosted CLI execution pair."""

import importlib.util
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
