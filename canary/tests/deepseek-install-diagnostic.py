#!/usr/bin/env python3
"""Ensure raw npm details cannot escape the diagnostic report."""

import importlib.util
import io
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "actions"))
spec = importlib.util.spec_from_file_location(
    "deepseek_diagnostic", Path(__file__).resolve().parents[1] / "actions/deepseek-install-diagnostic.py")
diagnostic = importlib.util.module_from_spec(spec)
spec.loader.exec_module(diagnostic)


class DiagnosticTests(unittest.TestCase):
    def test_missing_version_projects_only_reviewed_identity_and_status(self):
        raw = f"npm error code ETARGET\nnpm error notarget {diagnostic.MISSING}\nsecret-token /private/path"
        self.assertEqual(diagnostic.inspect_failure(io.BytesIO(raw.encode()), 1),
                         {"exitCode": 1, "missingRc3Dependency": True})

    def test_unrelated_error_does_not_claim_the_same_cause(self):
        for raw in (b"npm error code E404", b"npm error code ETARGET unrelated-package",
                    diagnostic.MISSING.encode()):
            self.assertEqual(diagnostic.inspect_failure(io.BytesIO(raw), 1),
                             {"exitCode": 1, "missingRc3Dependency": False})


if __name__ == "__main__":
    unittest.main()
