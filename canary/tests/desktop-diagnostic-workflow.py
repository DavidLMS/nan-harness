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


class DiagnosticWorkflowTests(unittest.TestCase):
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
        self.assertIn("if: ${{ !inputs.desktop_diagnostics }}", workflow)
        self.assertIn("max-parallel: 3", WORKFLOW)
        self.assertNotIn("secrets:", diagnostic)


if __name__ == "__main__":
    unittest.main()
