#!/usr/bin/env python3
"""Deterministic contracts for the modular desktop suite runner."""

import importlib.util
from pathlib import Path
import tempfile
import unittest
import os
from unittest.mock import patch

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

    def test_release_manifest_rejects_tag_or_digest_mismatch(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            asset = root / "nanh"
            asset.write_bytes(b"verified")
            manifest = root / "SHA256SUMS"
            manifest.write_text(f"{SUITE.digest(asset)}  nanh\n")
            command = SUITE.validate_release_assets(manifest, {"nanh": asset}, "v1.2.3", "a" * 40)
            self.assertEqual(command[-4:], ["--source-ref", "refs/tags/v1.2.3", "--source-digest", "a" * 40])
            with self.assertRaises(ValueError):
                SUITE.validate_release_assets(manifest, {"nanh": asset}, "v1.2.4", "a" * 39)
            asset.write_bytes(b"tampered")
            with self.assertRaises(ValueError):
                SUITE.validate_release_assets(manifest, {"nanh": asset}, "v1.2.3", "a" * 40)

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
            prepared = Path(directory) / "prepared"
            prepared.write_bytes(b"receipt")
            state = SUITE.initial_state(cell, checker, nanh, prepared)
            self.assertEqual(state["checkerSha256"], SUITE.digest(checker))
            self.assertNotIn("output", state)
            self.assertEqual(state["outcome"], "blocked")

    def test_fake_stage_is_bounded_and_drops_key_for_deterministic(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "marker"
            fake = Path(directory) / "checker"
            fake.write_text("#!/bin/sh\nprintf '%s' \"${NAN_API_KEY-unset}\" > \"$MARKER\"\nexit 0\n")
            fake.chmod(0o700)
            old = os.environ.get("MARKER")
            os.environ["MARKER"] = str(marker)
            try:
                with patch.dict(os.environ, {"NAN_API_KEY": "synthetic"}):
                    self.assertTrue(SUITE.run_stage([str(fake)], live=False, timeout=5))
                self.assertEqual(marker.read_text(), "unset")
            finally:
                if old is None:
                    os.environ.pop("MARKER", None)
                else:
                    os.environ["MARKER"] = old

    def test_state_identity_mismatch_refuses_before_fake_checker(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            checker = root / "checker"
            nanh = root / "nanh"
            prepared = root / "prepared"
            for path in (checker, nanh, prepared):
                path.write_bytes(path.name.encode())
            cell = SUITE.suite_cell(selection(["zed-desktop"]), "linux", "branch", "a" * 40, "model")
            state = SUITE.initial_state(cell, checker, nanh, prepared)
            state["model"] = "different-model"
            self.assertNotEqual(state["model"], cell["model"])


if __name__ == "__main__":
    unittest.main()
