#!/usr/bin/env python3
"""Contracts shared by CLI, desktop and latest-version dispatchers."""

from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "actions"))
import selection


class SelectionTests(unittest.TestCase):
    def test_complete_suites_have_three_sequential_platform_jobs(self):
        for suite, count in (("cli", 15), ("desktop", 5)):
            result = selection.select_suite(suite)
            self.assertEqual([job["system"] for job in result["platforms"]],
                             ["linux", "macos", "windows"])
            self.assertTrue(all(len(job["harnesses"]) == count for job in result["platforms"]))

    def test_partial_selection_is_canonical_and_does_not_expand_coverage(self):
        result = selection.select_suite("cli", "windows, linux", "opencode, codex", "live")
        self.assertEqual(result["harnesses"], ["codex", "opencode"])
        self.assertEqual([job["system"] for job in result["platforms"]], ["linux", "windows"])
        self.assertEqual(result["mode"], "live")

    def test_invalid_selections_fail_before_launch(self):
        for value in ("", "codex,", "codex,codex", "all,codex", "unknown", "zed-desktop"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                selection.select_suite("cli", harnesses=value)
        for value in ("", "linux,linux", "all,windows", "plan9"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                selection.select_suite("cli", platforms=value)
        with self.assertRaises(ValueError):
            selection.select_suite("unknown")
        with self.assertRaises(ValueError):
            selection.select_suite("cli", mode="publish")

    def test_model_precedence_and_validation(self):
        with patch.dict("os.environ", {"CANARY_MODEL": "configured-model"}):
            self.assertEqual(selection.resolve_model(), "configured-model")
            self.assertEqual(selection.resolve_model("selected-model"), "selected-model")
        self.assertEqual(selection.resolve_model(configured=""), "qwen3.6")
        for value in (" ", "x\ny", "$(command)", "-option", "x" * 129):
            with self.subTest(value=value), self.assertRaises(ValueError):
                selection.resolve_model(value)

    def test_platform_identity_preserves_native_architectures(self):
        self.assertEqual(selection.select_suite("cli", "linux")["platforms"][0]["architecture"],
                         "aarch64")
        self.assertEqual(selection.select_suite("desktop", "linux")["platforms"][0]["architecture"],
                         "x86_64")
        windows = selection.select_suite("cli", "windows")["platforms"][0]
        self.assertEqual((windows["architecture"], windows["target"]), ("x86_64", "pc-windows-msvc"))


if __name__ == "__main__":
    unittest.main()
