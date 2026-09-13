#!/usr/bin/env python3
"""Offline contracts for isolated, non-publishing branch diagnostics."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = (ROOT / ".github/workflows/desktop-check-suite.yml").read_text()
SYNTHETIC_WORKFLOW = (ROOT / ".github/workflows/desktop-check-chatgpt-wave29-synthetic.yml").read_text()


class DiagnosticWorkflowTests(unittest.TestCase):
    @staticmethod
    def scoped_block(text, start, end=None):
        block = text.split(start, 1)[1]
        return block.split(end, 1)[0] if end else block

    def matrix(self, diagnostics, source="branch", mode="deterministic", hosted="false"):
        script = WORKFLOW.split('          if [[ "$DIAGNOSTICS" == true ]]; then\n', 1)[1]
        script = 'if [[ "$DIAGNOSTICS" == true ]]; then\n' + script.split("          printf 'model=", 1)[0]
        selected = {"platforms": [
            {"system": system, "runner": "fixture", "harnesses": ["chatgpt-desktop", "pen-desktop"]}
            for system in ("linux", "macos", "windows")
        ]}
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "output"
            result = subprocess.run(["bash", "-c", "set -euo pipefail\n" + script],
                                    env={**os.environ, "DIAGNOSTICS": diagnostics, "SOURCE": source,
                                         "MODE": mode, "HOSTED_EVIDENCE": hosted,
                                         "selected": json.dumps(selected), "GITHUB_OUTPUT": str(output)},
                                    capture_output=True, check=False)
            return result.returncode, json.loads(output.read_text().removeprefix("matrix=")) if output.exists() else None

    def test_diagnostics_isolates_each_selected_application(self):
        code, matrix = self.matrix("true")
        self.assertEqual(code, 0)
        self.assertEqual(len(matrix["include"]), 6)
        for cell in matrix["include"]:
            self.assertEqual(cell["harnesses"], [cell["diagnostic_app"]])
        self.assertEqual(len({(cell["system"], cell["diagnostic_app"]) for cell in matrix["include"]}), 6)

    def test_default_preserves_platform_cells(self):
        code, matrix = self.matrix("false")
        self.assertEqual(code, 0)
        self.assertEqual(len(matrix["include"]), 3)
        self.assertTrue(all(len(cell["harnesses"]) == 2 and "diagnostic_app" not in cell for cell in matrix["include"]))

    def test_diagnostics_rejects_release_live_and_hosted_evidence(self):
        for options in ({"source": "release"}, {"mode": "live"}, {"hosted": "true"}):
            code, matrix = self.matrix("true", **options)
            self.assertNotEqual(code, 0)
            self.assertIsNone(matrix)

    def test_manual_entry_never_reaches_detector_publication(self):
        workflow = (ROOT / ".github/workflows/harness-canary.yml").read_text()
        diagnostic = workflow.split("  desktop-diagnostics:\n", 1)[1].split("  desktop:\n", 1)[0]
        for contract in ("github.event_name == 'workflow_dispatch'", "source: branch", "mode: deterministic",
                         "diagnostics: true", "hosted_evidence: false"):
            self.assertIn(contract, diagnostic)
        self.assertIn("if: ${{ !inputs.desktop_diagnostics && !inputs.chatgpt_sandbox_synthetic_only }}", workflow)
        self.assertIn("max-parallel: 3", WORKFLOW)
        self.assertNotIn("secrets:", diagnostic)

    def assert_synthetic_route(self, workflow):
        inputs = self.scoped_block(workflow, "      chatgpt_sandbox_synthetic_only:\n", "      mode:\n")
        self.assertIn("description: Run only offline Linux subprocess and workflow contracts", inputs)
        self.assertIn("type: boolean", inputs)
        self.assertIn("default: false", inputs)
        self.assertNotIn("default: true", inputs)

        select = self.scoped_block(workflow, "  select:\n", "  desktop-diagnostics:\n")
        desktop_diagnostics = self.scoped_block(workflow, "  desktop-diagnostics:\n", "  chatgpt-sandbox-synthetic:\n")
        synthetic_caller = self.scoped_block(workflow, "  chatgpt-sandbox-synthetic:\n", "  desktop:\n")
        self.assertIn("if: ${{ !inputs.desktop_diagnostics && !inputs.chatgpt_sandbox_synthetic_only }}", select)
        self.assertIn("if: ${{ github.event_name == 'workflow_dispatch' && inputs.desktop_diagnostics && !inputs.chatgpt_sandbox_synthetic_only }}", desktop_diagnostics)
        self.assertIn("if: ${{ github.event_name == 'workflow_dispatch' && inputs.chatgpt_sandbox_synthetic_only }}", synthetic_caller)
        self.assertIn("uses: ./.github/workflows/desktop-check-chatgpt-wave29-synthetic.yml", synthetic_caller)
        self.assertNotIn("secrets:", synthetic_caller)

        for job in (self.scoped_block(workflow, "  desktop:\n", "  cli:\n"),
                    self.scoped_block(workflow, "  cli:\n")):
            self.assertIn("needs: select", job)

        self.assertIn("runs-on: ubuntu-24.04", SYNTHETIC_WORKFLOW)
        self.assertIn("timeout-minutes: 10", SYNTHETIC_WORKFLOW)
        self.assertIn("permissions:\n  contents: read", SYNTHETIC_WORKFLOW)
        synthetic_job = self.scoped_block(SYNTHETIC_WORKFLOW, "  synthetic-only:\n")
        self.assertIn("uses: actions/checkout@", synthetic_job)
        self.assertEqual(synthetic_job.count("run: python3 -B"), 2)
        self.assertIn("run: python3 -B scripts/test-chatgpt-wave12-install.py", synthetic_job)
        self.assertIn("run: python3 -B canary/tests/desktop-diagnostic-workflow.py", synthetic_job)
        self.assertNotIn("secrets:", SYNTHETIC_WORKFLOW)
        for forbidden in ("apt-get", "sudo", "sysctl", "apparmor", "cargo", "provider",
                          "nan-harness-desktop-check", "--mark-launch", "gui", "native"):
            self.assertNotIn(forbidden, SYNTHETIC_WORKFLOW.lower())

    def test_synthetic_route_is_opt_in_and_excludes_normal_jobs(self):
        workflow = (ROOT / ".github/workflows/harness-canary.yml").read_text()
        self.assert_synthetic_route(workflow)

    def test_synthetic_route_gate_mutations_fail_scoped_validation(self):
        workflow = (ROOT / ".github/workflows/harness-canary.yml").read_text()
        inputs = self.scoped_block(workflow, "      chatgpt_sandbox_synthetic_only:\n", "      mode:\n")
        bad_default = workflow.replace(inputs, inputs.replace("default: false", "default: true", 1), 1)
        with self.assertRaises(AssertionError):
            self.assert_synthetic_route(bad_default)

        select_start = "  select:\n"
        select = self.scoped_block(workflow, select_start, "  desktop-diagnostics:\n")
        bad_select = workflow.replace(select, select.replace(" && !inputs.chatgpt_sandbox_synthetic_only", "", 1), 1)
        with self.assertRaises(AssertionError):
            self.assert_synthetic_route(bad_select)

        desktop_start = "  desktop-diagnostics:\n"
        desktop = self.scoped_block(workflow, desktop_start, "  chatgpt-sandbox-synthetic:\n")
        bad_desktop = workflow.replace(desktop, desktop.replace(" && !inputs.chatgpt_sandbox_synthetic_only", "", 1), 1)
        with self.assertRaises(AssertionError):
            self.assert_synthetic_route(bad_desktop)

    def test_diagnostic_upload_requires_validated_exact_files(self):
        block = WORKFLOW.split("      - name: Validate closed diagnostic artifacts", 1)[1].split(
            "      - name: Pack validated hosted evidence", 1)[0]
        self.assertIn("always() && inputs.diagnostics && !inputs.hosted_evidence", block)
        self.assertIn("steps.diagnostics.outputs.validated == 'true'", block)
        self.assertIn("--validate \"$diagnostic\" --source-sha \"$GITHUB_SHA\"", block)
        self.assertIn('[[ "$count" -gt 0 ]]', block)
        self.assertIn("retention-days: 7", block)
        self.assertNotIn("steps.evidence", block)
        upload_paths = block.split("          path: |\n", 1)[1].split("          if-no-files-found:", 1)[0]
        self.assertEqual([line.strip() for line in upload_paths.splitlines()], [
            "${{ runner.temp }}/desktop-suite/diagnostics-resolve.json",
            "${{ runner.temp }}/desktop-suite/diagnostics-install.json",
            "${{ runner.temp }}/desktop-suite/diagnostics-prepare.json",
            "${{ runner.temp }}/desktop-suite/diagnostics-probes.json",
        ])

    def test_resolution_and_preparation_are_captured_before_probes(self):
        for step, filename in (("Resolve exact frozen Desktop releases before preparation", "resolve"),
                               ("Prepare apps and private receipt without credentials", "prepare")):
            block = WORKFLOW.split("      - name: " + step, 1)[1].split("      - name:", 1)[0]
            self.assertIn('if [[ \'${{ inputs.diagnostics }}\' == true ]]', block)
            self.assertIn('canary/actions/desktop_diagnostics.py --output "$RUNNER_TEMP/desktop-suite/diagnostics-' + filename + '.json"', block)
            self.assertIn('--source-sha "$GITHUB_SHA"', block)

    def test_windows_stop_regression_runs_in_the_affected_diagnostic_cell(self):
        block = WORKFLOW.split("      - name: Verify Windows process stop regression", 1)[1].split("      - name:", 1)[0]
        self.assertIn("inputs.diagnostics && matrix.system == 'windows' && matrix.diagnostic_app == 'pen-desktop'", block)
        self.assertIn("cargo test --locked -p nan-harness-desktop-check --all-features stop_", block)

    def test_both_failure_sources_are_connected_to_collector(self):
        install = WORKFLOW.split("      - name: Install exact external Desktop applications", 1)[1].split(
            "      - name: Capture and exercise Linux native helper", 1)[0]
        self.assertIn('installer=(python3 canary/actions/desktop_install.py', install)
        self.assertIn('canary/actions/desktop_diagnostics.py --output "$RUNNER_TEMP/desktop-suite/diagnostics-install.json"', install)
        self.assertIn('--timeout 3600 -- "${installer[@]}"', install)
        probes = WORKFLOW.split("      - name: Run sequential deterministic desktop apps", 1)[1].split(
            "      - name: Run bounded live desktop apps", 1)[0]
        self.assertIn('diagnostic_args=(--diagnostics "$RUNNER_TEMP/desktop-suite/diagnostics-probes.json")', probes)
        self.assertIn('--stage deterministic "${diagnostic_args[@]}"', probes)


if __name__ == "__main__":
    unittest.main()
