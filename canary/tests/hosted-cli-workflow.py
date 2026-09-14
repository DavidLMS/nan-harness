#!/usr/bin/env python3
"""Structural contracts for the opt-in hosted CLI workflow."""

from pathlib import Path
import importlib.util
import json
import shlex
import subprocess
import tempfile
import sys
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = (ROOT / ".github/workflows/cli-release-gate.yml").read_text()


class HostedCliWorkflowTests(unittest.TestCase):
    def test_workflow_version_command_executes_against_cargo_metadata(self):
        line = next(line.strip() for line in WORKFLOW.splitlines()
                    if line.strip().startswith('nan_version="$('))
        command = shlex.split(line[len('nan_version="$('):-2])
        self.assertEqual(command[:2], ["python3", "-c"])
        with tempfile.TemporaryDirectory() as directory:
            metadata = Path(directory) / "metadata.json"
            metadata.write_text(json.dumps({"packages": [
                {"name": "other-crate", "version": "9.9.9"},
                {"name": "nan-harness-cli", "version": "1.2.3"},
            ]}))
            result = subprocess.run([sys.executable, *command[1:-1], str(metadata)],
                                    capture_output=True, text=True, check=True)
            self.assertEqual(result.stdout.strip(), "1.2.3")

    def test_manual_inputs_are_explicit_and_os_is_a_choice(self):
        for text in ("workflow_dispatch:", "platforms:", "harnesses:", "mode:", "source_ref:"):
            self.assertIn(text, WORKFLOW)
        self.assertNotIn("workflow_call:", WORKFLOW)
        self.assertIn("type: choice\n        options: [linux, macos, both]", WORKFLOW)
        self.assertIn("options: [deterministic, live]", WORKFLOW)

    def test_matrix_is_independent_and_target_is_arm64(self):
        self.assertIn("fail-fast: false", WORKFLOW)
        self.assertIn("max-parallel: 3", WORKFLOW)
        self.assertIn("needs: select", WORKFLOW)
        self.assertIn("matrix: ${{ fromJSON(needs.select.outputs.matrix) }}", WORKFLOW)
        self.assertIn("ubuntu-24.04-arm", (ROOT / "canary/actions/selection.py").read_text())
        self.assertIn('"aarch64"', (ROOT / "canary/actions/selection.py").read_text())

    def test_secret_is_live_only_and_checkout_has_no_credentials(self):
        self.assertGreaterEqual(WORKFLOW.count("persist-credentials: false"), 2)
        self.assertIn("name: Verify live credential is available", WORKFLOW)
        self.assertIn("if: matrix.mode == 'live'", WORKFLOW)
        self.assertIn("if [ \"$MODE\" = deterministic ]; then unset NAN_API_KEY; fi", WORKFLOW)
        self.assertIn("secrets.NAN_API_KEY", WORKFLOW)
        self.assertIn("git rev-parse --verify HEAD", WORKFLOW)
        self.assertIn("node-version: 24.20.0", WORKFLOW)
        self.assertNotIn("schedule:", WORKFLOW)
        self.assertNotIn("apt-get", WORKFLOW)
        self.assertNotIn("sudo ", WORKFLOW)

    def test_live_credential_check_precedes_expensive_build(self):
        credential = WORKFLOW.index("      - name: Verify live credential is available")
        build = WORKFLOW.index("cargo build --locked --release --package nan-harness-cli")
        self.assertLess(credential, build)

    def test_github_token_is_resolver_step_only(self):
        self.assertIn("GITHUB_TOKEN: ${{ github.token }}", WORKFLOW)
        resolver_start = WORKFLOW.index("      - name: Resolve official CLI metadata")
        resolver_end = WORKFLOW.index("      - name: Run isolated CLI cell")
        resolver_step = WORKFLOW[resolver_start:resolver_end]
        self.assertIn("GITHUB_TOKEN: ${{ github.token }}", resolver_step)
        self.assertNotIn("GITHUB_TOKEN", WORKFLOW[resolver_end:])
        self.assertIn("permissions:\n  contents: read", WORKFLOW)

    def test_real_runner_contract_receives_all_required_provenance(self):
        for argument in ("--tag", "--binary", "--canary", "--directory", "--output",
                         "--system", "--architecture", "--source-kind", "--source-sha",
                         "--nan-version", "--manifest", "--run-id"):
            self.assertIn(argument, WORKFLOW)
        self.assertIn("cli-suite.py resolve", WORKFLOW)
        self.assertIn('--tag "v${NAN_VERSION}"', WORKFLOW)
        self.assertIn("cargo build --locked --release --package nan-harness-cli", WORKFLOW)

    def test_synthetic_runner_invocation_uses_real_argparse_contract(self):
        action = ROOT / "canary/actions/cli-suite.py"
        spec = importlib.util.spec_from_file_location("hosted_cli_suite_contract", action)
        suite = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(suite)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            frozen = suite.FrozenHarness("codex", "1.2.3", "linux", "aarch64", "npm:@openai/codex",
                                        "@openai/codex", "qwen3.6")
            argv = ["cli-suite.py", "--harnesses", "codex", "--mode", "deterministic",
                    "--trigger", "manual", "--tag", "source-tag", "--model", "qwen3.6",
                    "--source-kind", "branch", "--source-sha", "a" * 40, "--nan-version", "0.1.6",
                    "--system", "linux", "--architecture", "aarch64", "--manifest", str(root / "manifest"),
                    "--binary", str(root / "nan-harness"), "--canary", str(root / "canary"),
                    "--directory", str(root / "cells"), "--output", str(root / "reports"), "--run-id", "synthetic"]
            with patch.object(suite, "_load_manifest", return_value=([frozen], [])), \
                    patch.object(suite.subprocess, "run", return_value=type("Result", (), {"returncode": 0})()), \
                    patch.object(sys, "argv", argv):
                self.assertEqual(suite.main(), 0)

    def test_cell_accepts_version_derived_release_tag_and_rejects_malformed_tag(self):
        action = ROOT / "canary/actions/cell.py"
        spec = importlib.util.spec_from_file_location("hosted_cli_cell_contract", action)
        cell = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cell)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            argv = ["cell.py", "install", "--harness", "codex", "--trigger", "manual",
                    "--tag", "v1.2.3", "--binary", str(root / "nan-harness"),
                    "--canary", str(root / "canary"), "--directory", str(root / "cell"),
                    "--output", str(root / "report.json"), "--run-id", "synthetic",
                    "--model", "qwen3.6", "--mode", "deterministic", "--system", "linux",
                    "--architecture", "aarch64", "--source-kind", "branch", "--source-sha", "a" * 40,
                    "--nan-version", "1.2.3", "--harness-version", "1.2.3"]
            with patch.object(cell, "ensure_private_directory"), patch.object(cell, "run"), \
                    patch.object(sys, "argv", argv):
                self.assertEqual(cell.main(), 0)
            with patch.object(sys, "argv", [*argv[:7], "not-a-semver", *argv[8:]]), \
                    self.assertRaises(SystemExit) as error:
                cell.main()
            self.assertEqual(error.exception.code, 2)


if __name__ == "__main__":
    unittest.main()
