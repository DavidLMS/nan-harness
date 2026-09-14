#!/usr/bin/env python3
"""Static contracts for the manual native Windows diagnostic workflow."""

from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = (ROOT / ".github/workflows/windows-cli-diagnostic.yml").read_text()


class WindowsDiagnosticWorkflowTests(unittest.TestCase):
    def test_manual_windows_only_and_least_privilege(self):
        self.assertIn("workflow_dispatch:", WORKFLOW)
        self.assertNotIn("schedule:", WORKFLOW)
        self.assertNotIn("workflow_call:", WORKFLOW)
        self.assertIn("runs-on: windows-2025", WORKFLOW)
        self.assertNotIn("wsl", WORKFLOW.lower())
        self.assertIn("permissions:\n  contents: read", WORKFLOW)
        self.assertIn("persist-credentials: false", WORKFLOW)
        self.assertIn("fetch --no-tags --depth=1 origin $source", WORKFLOW)
        self.assertIn("rev-parse HEAD) -ne $source", WORKFLOW)

    def test_bounded_inputs_and_single_batch_entrypoint(self):
        for token in ("harnesses:", "mode:", "model:", "source_sha:",
                      "options: [native-diagnostic, deterministic, live]", "--harnesses",
                      "--mode", "--model", "--source-sha", "--binary",
                      "--canary", "--output"):
            self.assertIn(token, WORKFLOW)
        self.assertEqual(WORKFLOW.count("canary\\windows-diagnostic.cmd"), 1)
        self.assertIn("if: always()", WORKFLOW)

    def test_setup_is_advisory_and_versions_are_fixed(self):
        self.assertIn("continue-on-error: true", WORKFLOW)
        self.assertIn("node-version: 24.20.0", WORKFLOW)
        self.assertIn("python-version: '3.12'", WORKFLOW)
        self.assertIn("rustup show", WORKFLOW)
        self.assertIn("cargo build --locked --release --package nan-harness-cli", WORKFLOW)
        self.assertIn("cargo build --locked --release --package nan-harness-canary", WORKFLOW)
        self.assertIn("--target x86_64-pc-windows-msvc", WORKFLOW)
        self.assertIn("NAN_DIAGNOSTIC_BINARY", WORKFLOW)
        self.assertIn("NAN_DIAGNOSTIC_CANARY", WORKFLOW)
        self.assertIn("timeout-minutes: 120", WORKFLOW)

    def test_live_secret_is_scoped_and_report_is_always_uploaded(self):
        self.assertIn("inputs.mode == 'live' && secrets.NAN_API_KEY || ''", WORKFLOW)
        self.assertIn("name: Upload diagnostic report", WORKFLOW)
        self.assertIn("if-no-files-found: warn", WORKFLOW)
        self.assertNotIn("echo $env:NAN_API_KEY", WORKFLOW)
        self.assertNotIn("Write-Host $env:NAN_API_KEY", WORKFLOW)
        self.assertIn("windows-cli-diagnostic\\report.json", WORKFLOW)
        self.assertIn("windows-cli-diagnostic\\summary.md", WORKFLOW)
        self.assertNotIn("windows-cli-diagnostic\\*", WORKFLOW)
        self.assertIn("Fail for failed, blocked, or unsupported", WORKFLOW)
        for stage in ("CHECKOUT", "NODE", "PYTHON", "RUST", "SOURCE", "BUILD"):
            self.assertIn(f"NAN_DIAGNOSTIC_SETUP_{stage}", WORKFLOW)


if __name__ == "__main__":
    unittest.main()
