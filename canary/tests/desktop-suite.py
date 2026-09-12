#!/usr/bin/env python3
"""Deterministic contracts for the modular desktop suite runner."""

import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("desktop_suite", ROOT / "canary/actions/desktop_suite.py")
SUITE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SUITE)


def selection(apps=None):
    requested = apps or list(SUITE.DESKTOP_HARNESSES)
    apps = [app for app in SUITE.DESKTOP_HARNESSES if app in requested]
    return {"suite": "desktop", "mode": "deterministic", "harnesses": apps,
            "platforms": [{"system": system, "runner": "runner", "architecture": "x86_64",
                           "target": "target", "harnesses": apps} for system in ("linux", "macos", "windows")]}


class DesktopSuiteTests(unittest.TestCase):
    def test_one_cell_keeps_apps_sequential_and_canonical(self):
        cell = SUITE.suite_cell(selection(["zed-desktop", "chatgpt-desktop"]), "linux",
                                "branch", "a" * 40, "selected-model")
        self.assertEqual(cell["apps"], ["chatgpt-desktop", "zed-desktop"])
        self.assertEqual(cell["sourceSha"], "a" * 40)

    def test_branch_and_release_identity_are_distinct(self):
        with self.assertRaises(ValueError):
            SUITE.validate_identity("release", "a" * 40, "model", ["zed-desktop"], "linux", "v0.1.0")
        self.assertEqual(SUITE.validate_identity("release", "b" * 64, "model",
                                                  ["zed-desktop"], "linux", "v0.1.0"), ("zed-desktop",))

    def test_command_passes_model_and_all_apps_without_shell(self):
        cell = SUITE.suite_cell(selection(), "windows", "branch", "a" * 40, "model/x")
        command = SUITE.checker_command("checker", "live", cell, "receipt", "report", "nanh")
        self.assertEqual(command[0:7], ["checker", "run", "--yes", "--non-interactive", "--ephemeral", "--mode", "live"])
        self.assertIn("--model", command)
        self.assertIn("model/x", command)
        self.assertEqual(command[-2:], ["--session", "github-hosted"])

    def test_initial_state_contains_only_hashes_for_binaries(self):
        with tempfile.TemporaryDirectory() as directory:
            checker = Path(directory) / "checker"
            nanh = Path(directory) / "nanh"
            checker.write_bytes(b"checker")
            nanh.write_bytes(b"nanh")
            cell = SUITE.suite_cell(selection(["zed-desktop"]), "linux", "branch", "a" * 40, "model")
            state = SUITE.initial_state(cell, checker, nanh)
            self.assertEqual(state["checkerSha256"], SUITE.digest(checker))
            self.assertNotIn("output", state)
            self.assertEqual(state["outcome"], "blocked")


if __name__ == "__main__":
    unittest.main()
