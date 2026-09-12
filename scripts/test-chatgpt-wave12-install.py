#!/usr/bin/env python3
"""Synthetic installer contracts. No test executes a package or host command."""

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "official_install", Path(__file__).with_name("chatgpt-wave12-install.py"))
installer = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(installer)


class InstallerTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="chatgpt-install-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.output = self.root / "new"
        self.exe = self.root / "ChatGPT"
        self.profile = self.root / "profile"
        self.restriction = self.root / "restriction"
        self.restriction.write_text("1\n")
        self.package = b"synthetic package"
        self.calls = []
        self.change = None
        self.fields = {"Package": "chatgpt", "Version": installer.VERSION, "Architecture": "amd64"}
        for name, value in (("EXECUTABLE", self.exe), ("PROFILE", self.profile),
                            ("RESTRICTION", self.restriction),
                            ("PACKAGE_SHA256", hashlib.sha256(self.package).hexdigest())):
            self.enterContext(patch.object(installer, name, value))
        # Any accidental external command, including sudo, fails this test.
        self.enterContext(patch.object(installer.subprocess, "run",
                                       side_effect=AssertionError("host command forbidden")))

    def fake_command(self, args, **kwargs):
        self.calls.append(args)
        if args[0] == "curl":
            Path(args[args.index("--output") + 1]).write_bytes(self.package)
        elif args[0] == "dpkg-deb":
            return self.fields[args[-1]]
        elif args[:4] == ["sudo", "-n", "apt-get", "install"]:
            self.exe.write_bytes(b"synthetic executable")
            self.profile.write_text(installer.OFFICIAL_PROFILE)
            if self.change:
                self.change()
        elif args[0] == "dpkg-query":
            return installer.VERSION
        else:
            self.fail(f"unexpected command: {args[0]}")
        return ""

    def install(self, loaded=True):
        with patch.object(installer, "check_runner"), \
                patch.object(installer, "command", side_effect=self.fake_command), \
                patch.object(installer, "profile_loaded", return_value=loaded), \
                patch.object(installer, "root_owned_file"):
            installer.install(self.output)

    def test_pinned_install_emits_only_closed_evidence(self):
        self.install()
        result = json.loads((self.output / "sandbox-setup.json").read_text())
        self.assertEqual(set(result), {"schemaVersion", "kind", "installation", "packageVersion",
                                      "packageSha256", "executableSha256", "profileSha256",
                                      "profileLoaded", "userNamespaceRestrictionBefore",
                                      "userNamespaceRestrictionAfter"})
        self.assertEqual(result["userNamespaceRestrictionAfter"], 1)
        self.assertTrue(result["profileLoaded"])
        self.assertEqual(result["executableSha256"], installer.digest(self.exe))
        apt = [call for call in self.calls if "apt-get" in call]
        self.assertEqual(apt, [["sudo", "-n", "apt-get", "install", "--yes",
                               "--no-install-recommends", str(self.output / "chatgpt.deb")]])
        self.assertIn(installer.PACKAGE_URL, self.calls[0])

    def test_wrong_digest_never_installs(self):
        self.package = b"different package"
        with self.assertRaisesRegex(ValueError, "digest mismatch"):
            self.install()
        self.assertEqual(len(self.calls), 1)
        self.assertFalse(self.exe.exists())

    def test_wrong_package_identity_never_installs(self):
        for field in self.fields:
            with self.subTest(field=field):
                old = self.fields[field]
                self.fields[field] = "unexpected"
                self.output = self.root / field
                with self.assertRaisesRegex(ValueError, "identity mismatch"):
                    self.install()
                self.fields[field] = old
        self.assertFalse(any("apt-get" in call for call in self.calls))

    def test_missing_profile_activation_refuses_evidence(self):
        with self.assertRaisesRegex(ValueError, "not loaded"):
            self.install(loaded=False)
        self.assertFalse((self.output / "sandbox-setup.json").exists())

    def test_package_install_failure_stops_before_evidence(self):
        self.change = lambda: installer.require(False, "runner command failed")
        with self.assertRaisesRegex(ValueError, "runner command failed"):
            self.install()
        self.assertFalse((self.output / "sandbox-setup.json").exists())

    def test_changed_profile_or_global_restriction_refuses_evidence(self):
        for name, path, content in (("profile", self.profile, "different profile"),
                                    ("restriction", self.restriction, "0\n")):
            with self.subTest(name=name):
                self.output = self.root / ("case-" + name) / "output"
                self.output.parent.mkdir(exist_ok=True)
                self.change = lambda: path.write_text(content)
                with self.assertRaises(ValueError):
                    self.install()
                self.assertFalse((self.output / "sandbox-setup.json").exists())

    def test_existing_output_is_preserved(self):
        self.output.mkdir()
        sentinel = self.output / "sentinel"
        sentinel.write_text("keep")
        with self.assertRaises(FileExistsError):
            self.install()
        self.assertEqual(sentinel.read_text(), "keep")
        self.assertEqual(self.calls, [])

    def test_runner_gate_rejects_local_execution_without_commands(self):
        with patch.dict(os.environ, {}, clear=True), self.assertRaises(ValueError):
            installer.check_runner()

    def test_runner_gate_rejects_keys_and_preexisting_state(self):
        runner = {"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted",
                  "RUNNER_OS": "Linux", "RUNNER_ARCH": "X64"}
        with patch.dict(os.environ, runner, clear=True), \
                patch.object(installer.platform, "system", return_value="Linux"), \
                patch.object(installer.platform, "machine", return_value="x86_64"), \
                patch.object(installer.os, "getuid", return_value=1001), \
                patch.object(installer, "command", return_value="") as command, \
                patch.object(installer, "profile_loaded", return_value=False):
            for key in ("NAN_API_KEY", "OPENAI_API_KEY", "CODEX_API_KEY"):
                with patch.dict(os.environ, {key: "synthetic"}), self.assertRaises(ValueError):
                    installer.check_runner()
            command.assert_not_called()
            # The fake executable parent already exists, so nothing is overwritten.
            with self.assertRaisesRegex(ValueError, "must be preserved"):
                installer.check_runner()
            self.assertFalse(any(call.args[0][0] == "dpkg-query" for call in command.call_args_list))

    def test_runner_gate_requires_restriction_and_preserves_loaded_profile(self):
        runner = {"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted",
                  "RUNNER_OS": "Linux", "RUNNER_ARCH": "X64"}
        with patch.dict(os.environ, runner, clear=True), \
                patch.object(installer.platform, "system", return_value="Linux"), \
                patch.object(installer.platform, "machine", return_value="x86_64"), \
                patch.object(installer.os, "getuid", return_value=1001), \
                patch.object(installer, "command", return_value="") as command:
            self.restriction.write_text("0\n")
            with self.assertRaisesRegex(ValueError, "must remain enabled"):
                installer.check_runner()
            command.assert_not_called()
            self.restriction.write_text("1\n")
            with patch.object(installer, "profile_loaded", return_value=True), \
                    self.assertRaisesRegex(ValueError, "profile must be preserved"):
                installer.check_runner()

    def test_root_owned_file_rejects_writable_or_symlinked_paths(self):
        for mode, owner in ((stat.S_IFREG | 0o666, 0), (stat.S_IFLNK | 0o777, 0),
                            (stat.S_IFREG | 0o755, 1001)):
            with self.subTest(mode=mode, owner=owner), \
                    patch.object(Path, "is_file", return_value=True), \
                    patch.object(Path, "lstat", return_value=SimpleNamespace(st_uid=owner, st_mode=mode)), \
                    self.assertRaises(ValueError):
                installer.root_owned_file(self.exe)

    def test_command_failures_do_not_expose_captured_output(self):
        result = SimpleNamespace(returncode=1, stdout="synthetic private output", stderr="synthetic")
        with patch.object(installer.subprocess, "run", return_value=result), \
                self.assertRaisesRegex(ValueError, "^runner command failed$"):
            installer.command(["synthetic"])
        with patch.object(installer.subprocess, "run", side_effect=subprocess.TimeoutExpired("synthetic", 1)), \
                self.assertRaises(subprocess.TimeoutExpired):
            installer.command(["synthetic"])


if __name__ == "__main__":
    unittest.main()
