#!/usr/bin/env python3
"""Offline contracts for exact Desktop installer inputs."""

import importlib.util
from pathlib import Path
import tempfile
import unittest
import os
from unittest.mock import patch
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "canary/actions"))
import desktop_suite
if not hasattr(desktop_suite, "read_frozen_manifest"):
    desktop_suite.read_frozen_manifest = lambda *args: {}
SPEC = importlib.util.spec_from_file_location("desktop_install", ROOT / "canary/actions/desktop_install.py")
INSTALL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INSTALL)


def release(app="chatgpt-desktop", digest=None, staged=True):
    return {"app": app, "status": "frozen", "installer": "external", "format": "msix",
            "staged": staged, "digest": "sha256:" + (digest or "a" * 64),
            "url": "https://example.invalid/frozen.msix", "version": "1.2.3"}


class DesktopInstallTests(unittest.TestCase):
    def test_materialization_verifies_copied_bytes_and_refuses_overwrite(self):
        with tempfile.TemporaryDirectory() as directory:
            source, destination = Path(directory) / "source", Path(directory) / "destination"
            source.write_bytes(b"verified")
            expected = __import__("hashlib").sha256(source.read_bytes()).hexdigest()
            INSTALL.materialize_verified(source, destination, expected)
            self.assertEqual(destination.read_bytes(), b"verified")
            with self.assertRaises(FileExistsError):
                INSTALL.materialize_verified(source, destination, expected)
            source.write_bytes(b"tampered")
            with self.assertRaises(RuntimeError):
                INSTALL.materialize_verified(source, Path(directory) / "other", expected)

    def test_tampered_staged_bytes_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / ("chatgpt-desktop-" + "a" * 64)
            path.write_bytes(b"tampered")
            with self.assertRaises(RuntimeError):
                INSTALL.install_entry(release(), "windows", Path(directory), Path(directory) / "work")

    def test_missing_staged_bytes_are_rejected_without_download(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(INSTALL, "_download") as download:
            with self.assertRaises(RuntimeError):
                INSTALL.install_entry(release(), "windows", Path(directory), Path(directory) / "work")
            download.assert_not_called()

    def test_staged_branch_does_not_refetch_moving_url(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / ("chatgpt-desktop-" + "a" * 64)
            payload = b"frozen"
            digest = __import__("hashlib").sha256(payload).hexdigest()
            path.unlink(missing_ok=True)
            release_entry = release(digest=digest)
            path = Path(directory) / ("chatgpt-desktop-" + digest)
            path.write_bytes(payload)
            (Path(directory) / "work").mkdir()
            with patch.object(INSTALL, "_download", side_effect=AssertionError), \
                    patch.object(INSTALL, "_install_windows"):
                self.assertEqual(INSTALL.install_entry(release_entry, "windows", Path(directory), Path(directory) / "work"), "installed")

    def test_manifest_selection_and_identity_are_passed_unchanged(self):
        frozen = {"apps": [release()]}
        with tempfile.TemporaryDirectory() as directory, patch.object(INSTALL, "read_frozen_manifest", return_value=frozen) as reader, \
                patch.object(INSTALL, "install_entry", return_value="installed"):
            self.assertTrue(INSTALL.install(Path(directory) / "manifest", Path(directory) / "artifacts",
                                            "windows", "x86_64", "model/x", "chatgpt-desktop"))
            reader.assert_called_once_with(Path(directory) / "manifest", ["chatgpt-desktop"],
                                           "windows", "x86_64", "model/x")

    def test_invalid_platform_model_and_selection_fail_before_manifest_read(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(INSTALL, "read_frozen_manifest") as reader:
            for platform, model, apps in (("freebsd", "model", "chatgpt-desktop"),
                                          ("windows", "bad model", "chatgpt-desktop"),
                                          ("windows", "model", "chatgpt-desktop,chatgpt-desktop")):
                with self.assertRaises(ValueError):
                    INSTALL.install(Path(directory) / "manifest", Path(directory) / "artifacts",
                                    platform, "x86_64", model, apps)
            reader.assert_not_called()

    def test_one_failed_app_does_not_hide_an_independent_success(self):
        entries = {"apps": [release("chatgpt-desktop"), release("claude-desktop")]}
        with tempfile.TemporaryDirectory() as directory, patch.object(INSTALL, "read_frozen_manifest", return_value=entries), \
                patch.object(INSTALL, "install_entry", side_effect=[RuntimeError("private"), "installed"]):
            self.assertTrue(INSTALL.install(Path(directory) / "manifest", Path(directory) / "artifacts",
                                            "windows", "x86_64", "model", "chatgpt-desktop,claude-desktop"))

    def test_installation_root_and_hermes_exports_are_retained(self):
        entries = {"apps": [{"app": "hermes-desktop", "status": "frozen", "installer": "external",
                              "format": "source", "staged": False, "revision": "b" * 40,
                              "url": "https://github.com/NousResearch/hermes-agent.git", "version": "1.2.3"}]}
        with tempfile.TemporaryDirectory() as directory:
            env_file, path_file = Path(directory) / "env", Path(directory) / "path"
            source = Path(directory) / "artifacts" / "desktop-install" / "linux" / "hermes" / "hermes-agent"
            (source / "venv" / "bin").mkdir(parents=True)
            (source / "venv" / "bin" / "hermes").touch()
            (source / "apps" / "desktop" / "release").mkdir(parents=True)
            with patch.object(INSTALL, "read_frozen_manifest", return_value=entries), \
                    patch.object(INSTALL, "install_entry", return_value="installed"), \
                    patch.dict(os.environ, {"GITHUB_ENV": str(env_file), "GITHUB_PATH": str(path_file)}):
                self.assertTrue(INSTALL.install(Path(directory) / "manifest", Path(directory) / "artifacts",
                                                "linux", "x86_64", "model", "hermes-desktop"))
            root = Path(directory) / "artifacts" / "desktop-install"
            self.assertTrue(root.is_dir())
            self.assertIn("hermes-desktop", (root / "installation.json").read_text())

    def test_windows_installers_keep_nsis_and_inno_switches(self):
        def run_and_install(argv, **_):
            if argv[0] == "powershell":
                Path(directory, "Programs", "Pen").mkdir(parents=True)
            target = next((str(value)[3:] for value in argv if str(value).startswith("/D=")), None)
            target = target or next((str(value)[5:] for value in argv if str(value).startswith("/DIR=")), None)
            if target:
                Path(target).mkdir(parents=True, exist_ok=True)
            return True
        with tempfile.TemporaryDirectory() as directory, patch.object(INSTALL, "_run", side_effect=run_and_install) as run, \
                patch.dict(os.environ, {"LOCALAPPDATA": directory}):
            workspace = Path(directory) / "work"
            workspace.mkdir()
            for app in ("pen-desktop", "zed-desktop"):
                package = workspace / (app + ".exe")
                package.write_bytes(b"package")
                entry = {"app": app, "format": "windows-setup", "version": "1.2.3"}
                INSTALL._install_windows(entry, package, workspace)
            pen_args = run.call_args_list[0].args[0]
            zed_args = run.call_args_list[1].args[0]
            self.assertIn("$s.Arguments='/S /D=" + str(Path(directory) / "Programs" / "Pen") + "'", pen_args[-1])
            self.assertIn("$s.UseShellExecute=$false", pen_args[-1])
            self.assertIn("/VERYSILENT", zed_args)
            self.assertIn("/DIR=" + str(Path(directory) / "Programs" / "Zed"), zed_args)

    def test_msix_binds_signature_and_package_name_without_three_part_version_compare(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(INSTALL, "_run_output", return_value=""), \
                patch.object(INSTALL, "_run", return_value=True) as run:
            workspace = Path(directory)
            package = workspace / "package.msix"
            package.write_bytes(b"package")
            INSTALL._install_windows({"app": "chatgpt-desktop", "format": "msix", "version": "1.2.3"}, package, workspace)
            command = run.call_args.args[0][-1]
            self.assertIn("Get-AppxPackage", command)
            self.assertIn("OpenAI.Codex", command)
            self.assertIn("OpenAI.ChatGPT-Desktop", command)
            self.assertNotIn("1.2.3", command)

    def test_windows_hermes_runtime_paths_use_scripts_exe(self):
        python, launcher, path = INSTALL.hermes_runtime_paths(Path("root"), windows=True)
        self.assertEqual(str(python), "root/venv/Scripts/python.exe")
        self.assertEqual(str(launcher), "root/venv/Scripts/hermes.exe")
        self.assertEqual(str(path), "root/venv/Scripts")

    def test_hermes_commands_pin_revision_not_main_or_date_tag(self):
        item = {"app": "hermes-desktop", "url": "https://github.com/NousResearch/hermes-agent.git",
                "revision": "b" * 40}
        commands = [argv for argv, _ in INSTALL.hermes_source_commands(item, Path("private"))]
        self.assertIn("b" * 40, commands[2])
        self.assertNotIn("main", commands[2])
        self.assertNotIn("latest", commands[2])

    def test_hermes_bootstrap_uses_running_python_on_every_host(self):
        item = {"app": "hermes-desktop", "url": "https://github.com/NousResearch/hermes-agent.git",
                "revision": "b" * 40}
        commands = [argv for argv, _ in INSTALL.hermes_source_commands(item, Path("private"))]
        self.assertEqual(commands[4][0], sys.executable)


if __name__ == "__main__":
    unittest.main()
