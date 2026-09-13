#!/usr/bin/env python3
"""Structural contracts for the opt-in hosted CLI workflow."""

from pathlib import Path
import unittest


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


if __name__ == "__main__":
    unittest.main()
