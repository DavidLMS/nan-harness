#!/usr/bin/env python3
"""Structural contracts for the opt-in hosted CLI workflow."""

from pathlib import Path
import importlib.util
import tempfile
import sys
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = (ROOT / ".github/workflows/cli-hosted.yml").read_text()


class HostedCliWorkflowTests(unittest.TestCase):
    def test_manual_and_reusable_inputs_are_explicit(self):
        for text in ("workflow_call:", "workflow_dispatch:", "platforms:", "harnesses:", "mode:", "source_ref:"):
            self.assertIn(text, WORKFLOW)
        self.assertIn("options: [deterministic, live]", WORKFLOW)

    def test_matrix_is_independent_and_target_is_arm64(self):
        self.assertIn("fail-fast: false", WORKFLOW)
        self.assertIn("max-parallel: 3", WORKFLOW)
        self.assertIn("needs: select", WORKFLOW)
        self.assertIn("matrix: ${{ fromJSON(needs.select.outputs.matrix).cells }}", WORKFLOW)
        self.assertIn("ubuntu-24.04-arm", (ROOT / "canary/actions/selection.py").read_text())
        self.assertIn('"aarch64"', (ROOT / "canary/actions/selection.py").read_text())

    def test_secret_is_live_only_and_checkout_has_no_credentials(self):
        self.assertGreaterEqual(WORKFLOW.count("persist-credentials: false"), 2)
        self.assertIn("if [ \"$MODE\" = deterministic ]; then unset NAN_API_KEY; fi", WORKFLOW)
        self.assertIn("secrets.NAN_API_KEY", WORKFLOW)
        self.assertIn("git rev-parse --verify HEAD", WORKFLOW)
        self.assertNotIn("schedule:", WORKFLOW)
        self.assertNotIn("apt-get", WORKFLOW)
        self.assertNotIn("sudo ", WORKFLOW)

    def test_real_runner_contract_receives_all_required_provenance(self):
        for argument in ("--tag", "--binary", "--canary", "--directory", "--output",
                         "--system", "--architecture", "--source-kind", "--source-sha",
                         "--nan-version", "--manifest", "--run-id"):
            self.assertIn(argument, WORKFLOW)
        self.assertIn("cli-suite.py resolve", WORKFLOW)
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


if __name__ == "__main__":
    unittest.main()
