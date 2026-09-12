#!/usr/bin/env python3
"""The Desktop subject is the exact attested native release, not its probe build."""

import hashlib
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "actions"))
import desktop_release
from state import StateError

COMMIT = "a" * 40


class DesktopReleaseTests(unittest.TestCase):
    def run_stage(self, system="windows", architecture="x86_64", corrupt=False, attest=False):
        calls = []
        def command(argv):
            calls.append(argv)
            if argv[:3] == ["gh", "release", "download"]:
                directory = Path(argv[-1])
                asset = argv[-3]
                (directory / asset).write_bytes(b"release-binary")
                digest = "0" * 64 if corrupt else hashlib.sha256(b"release-binary").hexdigest()
                (directory / "SHA256SUMS").write_text(f"{digest}  {asset}\n")
            elif attest:
                raise StateError("attestation failed")
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(desktop_release, "remote_commit", return_value=COMMIT):
            binary = desktop_release.stage(SimpleNamespace(repository="owner/repository"), "v0.1.6",
                                           COMMIT, system, architecture, Path(directory) / "assets", command)
            return binary.name, calls

    def test_native_names_include_architecture_and_windows_extension_exactly_once(self):
        for system, architecture, asset in (
                ("windows", "x86_64", "nan-harness-x86_64-pc-windows-msvc.exe"),
                ("linux", "x86_64", "nan-harness-x86_64-unknown-linux-musl"),
                ("macos", "aarch64", "nan-harness-aarch64-apple-darwin")):
            name, calls = self.run_stage(system, architecture)
            self.assertEqual(name, asset)
            self.assertEqual(calls[1][:3], ["gh", "attestation", "verify"])
            self.assertIn("owner/repository/.github/workflows/release.yml", calls[1])
            self.assertIn("--deny-self-hosted-runners", calls[1])
            self.assertIn(COMMIT, calls[1])

    def test_checksum_or_attestation_failure_cannot_return_an_executable(self):
        with self.assertRaises(ValueError):
            self.run_stage(corrupt=True)
        with self.assertRaises(StateError):
            self.run_stage(attest=True)

    def test_changed_tag_cannot_write_staging_directory(self):
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(desktop_release, "remote_commit", return_value="b" * 40):
            target = Path(directory) / "assets"
            with self.assertRaises(StateError):
                desktop_release.stage(SimpleNamespace(repository="owner/repository"), "v0.1.6",
                                      COMMIT, "windows", "x86_64", target)
            self.assertFalse(target.exists())


if __name__ == "__main__":
    unittest.main()
