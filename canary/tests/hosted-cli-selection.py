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
    def test_windows_unavailable_harnesses_are_explicit_skips_only_on_windows(self):
        result = selection.select_cli("all", "all", "live")
        self.assertEqual(len(result["cells"]), 43)
        self.assertEqual({item["harness"] for item in result["skipped"]}, {"prime-agent", "fx"})
        self.assertTrue(all(item["system"] == "windows" for item in result["skipped"]))
        for harness in ("prime-agent", "fx"):
            self.assertEqual({cell["system"] for cell in result["cells"]
                              if cell["harness"] == harness}, {"linux", "macos"})
        only_skips = selection.select_cli("windows", "fx,prime-agent")
        self.assertEqual(only_skips["cells"], [])
        self.assertEqual(len(only_skips["skipped"]), 2)

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

    def test_native_windows_is_an_explicit_hosted_platform(self):
        result = selection.select_cli("windows", "codex")
        self.assertEqual(result["cells"], [{"system": "windows", "runner": "windows-2025",
                                            "architecture": "x86_64",
                                            "target": "x86_64-pc-windows-msvc",
                                            "harness": "codex", "mode": "deterministic"}])
        every = selection.select_cli("all", "codex")
        self.assertEqual({cell["system"] for cell in every["cells"]},
                         {"linux", "macos", "windows"})
        self.assertEqual(len(every["cells"]), 3)

    def test_platform_and_architecture_pairs_are_closed(self):
        self.assertEqual(selection.identity("windows", "x86_64")["runner"], "windows-2025")
        for system, architecture in (("windows", "aarch64"), ("linux", "x86_64"),
                                     ("solaris", "x86_64")):
            with self.subTest(system=system, architecture=architecture), self.assertRaises(ValueError):
                selection.identity(system, architecture)

    def test_every_harness_requires_the_hosted_arm64_platforms(self):
        self.assertEqual(set(selection.HARNESS_PLATFORMS), set(selection.CLI_HARNESSES))
        self.assertEqual(len(selection.qualified_identities()), 30)
        self.assertEqual(selection.qualified_platforms(), ("linux", "macos"))
        self.assertEqual(len(selection.required_assets()), 4)
        for harness in selection.CLI_HARNESSES:
            self.assertEqual(selection.supported_platforms(harness), ("linux", "macos"))
        with self.assertRaises(ValueError):
            selection.supported_platforms("claude-desktop")

    def test_invalid_duplicates_and_mode_fail(self):
        for platforms, harnesses, mode in (("linux,linux", "all", "deterministic"),
                                           ("linux", "codex,codex", "deterministic"),
                                           ("linux", "claude-desktop", "deterministic"),
                                           ("linux", "all", "publish")):
            with self.subTest(platforms=platforms, harnesses=harnesses, mode=mode), self.assertRaises(ValueError):
                selection.select_cli(platforms, harnesses, mode)


if __name__ == "__main__":
    unittest.main()
