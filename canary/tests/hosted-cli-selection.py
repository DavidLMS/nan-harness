#!/usr/bin/env python3
"""Offline contracts for hosted CLI selection."""

from pathlib import Path
import importlib.util
import sys
import unittest

ACTION = Path(__file__).resolve().parents[1] / "actions" / "selection.py"
SPEC = importlib.util.spec_from_file_location("hosted_cli_selection_contract", ACTION)
selection = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(selection)


class HostedCliSelectionTests(unittest.TestCase):
    def test_all_is_fifteen_independent_arm64_cells(self):
        result = selection.select_cli("both", "all")
        self.assertEqual(len(result["cells"]), 30)
        self.assertEqual({cell["architecture"] for cell in result["cells"]}, {"aarch64"})
        self.assertEqual({cell["system"] for cell in result["cells"]}, {"linux", "macos"})

    def test_subset_is_canonical_and_mode_explicit(self):
        result = selection.select_cli("macos,linux", "fx,codex", "live")
        self.assertEqual([(cell["system"], cell["harness"]) for cell in result["cells"]],
                         [("linux", "codex"), ("linux", "fx"), ("macos", "codex"), ("macos", "fx")])
        self.assertEqual(result["mode"], "live")

    def test_invalid_windows_desktop_duplicates_and_mode_fail(self):
        for platforms, harnesses, mode in (("windows", "all", "deterministic"),
                                           ("linux,linux", "all", "deterministic"),
                                           ("linux", "codex,codex", "deterministic"),
                                           ("linux", "claude-desktop", "deterministic"),
                                           ("linux", "all", "publish")):
            with self.subTest(platforms=platforms, harnesses=harnesses, mode=mode), self.assertRaises(ValueError):
                selection.select_cli(platforms, harnesses, mode)


if __name__ == "__main__":
    unittest.main()
