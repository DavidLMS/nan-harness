#!/usr/bin/env python3
"""Offline contracts for exact Desktop installer inputs."""

import importlib.util
import contextlib
import io
import json
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
    def _diagnostic(self, callback, app="chatgpt-desktop"):
        stream = io.StringIO()
        token = INSTALL._APP_CONTEXT.set(app)
        try:
            with contextlib.redirect_stderr(stream):
                with self.assertRaises(RuntimeError):
                    callback()
        finally:
            INSTALL._APP_CONTEXT.reset(token)
        lines = [line for line in stream.getvalue().splitlines()
                 if line.startswith("DESKTOP_INSTALL_DIAGNOSTIC: ")]
        self.assertEqual(len(lines), 1)
        return json.loads(lines[0].split(": ", 1)[1])

    def test_subprocess_timeout_is_bounded_diagnostic(self):
        with patch.object(INSTALL, "private_command", side_effect=INSTALL.StageTimeout("fixture")):
            diagnostic = self._diagnostic(lambda: INSTALL._run(("synthetic",), stage="hermes_build",
                                                                operation="npm_ci"))
        self.assertEqual(diagnostic, {"app": "chatgpt-desktop", "failure": "timeout", "operation": "npm_ci",
                                      "schema_version": 1, "stage": "hermes_build"})

    def test_subprocess_spawn_is_bounded_diagnostic(self):
        with patch.object(INSTALL, "private_command", side_effect=FileNotFoundError("secret path")):
            diagnostic = self._diagnostic(lambda: INSTALL._run(("synthetic",), stage="download",
                                                                operation="fetch_artifact"))
        self.assertEqual(diagnostic["failure"], "spawn")
        self.assertNotIn("secret", json.dumps(diagnostic))

    def test_subprocess_nonzero_includes_only_numeric_return_code(self):
        with patch.object(INSTALL, "private_command", return_value=23):
            diagnostic = self._diagnostic(lambda: INSTALL._run(("synthetic",), stage="installer",
                                                                operation="run_installer"))
        self.assertEqual(diagnostic, {"app": "chatgpt-desktop", "failure": "nonzero_exit", "operation": "run_installer",
                                      "return_code": 23, "schema_version": 1, "stage": "installer"})

    def test_npm_spawn_records_codes_and_resolution_without_paths(self):
        for executable, category in ((None, "missing"), ("/private/npm.cmd", "cmd"), ("/private/npm.exe", "exe")):
            error = FileNotFoundError(2, "private message", "/private/path")
            error.winerror = 2
            with patch.object(INSTALL, "private_command", side_effect=error), patch.object(INSTALL.shutil, "which", return_value=executable):
                diagnostic = self._diagnostic(lambda: INSTALL._run(("npm", "ci"), stage="hermes_build", operation="npm_ci"), app="hermes-desktop")
            self.assertEqual(diagnostic["os_error"], 2)
            self.assertEqual(diagnostic["win_error"], 2)
            self.assertEqual(diagnostic["npm_resolution"], category)
            self.assertNotIn("private", json.dumps(diagnostic))

    def test_diagnostic_details_are_strictly_closed_and_bounded(self):
        token = INSTALL._APP_CONTEXT.set("chatgpt-desktop")
        try:
            for details in ({"secret": "private"}, {"pip_failure_hint": "private"},
                            {"python_major": True}, {"pip_minor": 100}):
                with self.assertRaises(ValueError):
                    INSTALL._emit_diagnostic("hermes_build", "pip_install", "nonzero_exit", 1, details)
        finally:
            INSTALL._APP_CONTEXT.reset(token)

    def test_pip_failure_has_bounded_runtime_facts_and_closed_hint(self):
        with patch.object(INSTALL, "private_command", return_value=1):
            diagnostic = self._diagnostic(lambda: INSTALL._run(("synthetic",), stage="hermes_build",
                                                                operation="pip_install", pip_facts=None), app="hermes-desktop")
        self.assertEqual(diagnostic["pip_failure_hint"], "other")
        self.assertNotIn("python_major", diagnostic)
        self.assertNotIn("pip_major", diagnostic)

        facts = {"python_major": 3, "python_minor": 12, "pip_major": 25, "pip_minor": 1}
        with patch.object(INSTALL, "private_command", return_value=1):
            diagnostic = self._diagnostic(lambda: INSTALL._run(("synthetic",), stage="hermes_build",
                                                                operation="pip_install", pip_facts=facts), app="hermes-desktop")
        for key, value in facts.items():
            self.assertEqual(diagnostic[key], value)
        self.assertEqual(diagnostic["pip_failure_hint"], "other")

    def test_runtime_facts_are_structured_and_failed_preflight_is_optional(self):
        facts = {"python_major": 3, "python_minor": 12, "pip_major": 25, "pip_minor": 1}
        def command(*args, **kwargs):
            kwargs["diagnostic_callback"](io.BytesIO(json.dumps(facts).encode()))
            return 0
        with tempfile.TemporaryDirectory() as directory, patch.object(INSTALL, "private_command", side_effect=command):
            self.assertEqual(INSTALL._runtime_facts("synthetic-python", Path(directory)), facts)
        with patch.object(INSTALL, "private_command", return_value=1):
            self.assertIsNone(INSTALL._runtime_facts("synthetic-python", Path(directory)))

    def test_pip_hint_requires_one_bounded_signature_and_discards_private_text(self):
        signatures = {
            "interpreter_compatibility": b"ERROR: package requires-python >=3.12\n",
            "dependency_resolution": b"ERROR: ResolutionImpossible\n",
            "build_prerequisite": b"error: subprocess-exited-with-error\n",
            "network": b"Could not fetch URL https://secret.invalid/token\n",
        }
        for expected, payload in signatures.items():
            self.assertEqual(INSTALL._pip_failure_hint(io.BytesIO(payload)), expected)
        self.assertEqual(INSTALL._pip_failure_hint(io.BytesIO(
            signatures["network"] + signatures["dependency_resolution"])), "other")
        self.assertEqual(INSTALL._pip_failure_hint(io.BytesIO(b"x" * (64 * 1024 + 1))), "other")
        self.assertEqual(INSTALL._pip_failure_hint(io.BytesIO(b"ERROR: ResolutionImpossible\n\xff")), "other")

    def test_runtime_fact_cleanup_failure_stops_preflight(self):
        for error in (INSTALL.CleanupError("private"), INSTALL.StageTimeout("private")):
            with patch.object(INSTALL, "private_command", side_effect=error), self.assertRaises(INSTALL.CleanupUncertain):
                INSTALL._runtime_facts("synthetic", Path("."))

    def test_runtime_fact_reader_is_bounded_and_rejects_invalid_payloads(self):
        class Bounded(io.BytesIO):
            def read(self, size=-1):
                if size != 4097:
                    raise AssertionError("unbounded runtime fact read")
                return super().read(size)
        for raw in (b"x" * 4097, b"\xff", b'{"python_major":true}', b'{"private":"token"}'):
            def command(*args, **kwargs):
                kwargs["diagnostic_callback"](Bounded(raw))
                return 0
            with patch.object(INSTALL, "private_command", side_effect=command):
                self.assertIsNone(INSTALL._runtime_facts("synthetic", Path(".")))

    def test_pip_failure_callback_attaches_only_the_closed_hint(self):
        def command(*args, **kwargs):
            kwargs["diagnostic_callback"](io.BytesIO(
                b"ERROR: ResolutionImpossible\nprivate-token=https://secret.invalid/x\n"))
            return 1
        with patch.object(INSTALL, "private_command", side_effect=command):
            diagnostic = self._diagnostic(lambda: INSTALL._run(("synthetic",), stage="hermes_build",
                                                                operation="pip_install",
                                                                pip_facts={"python_major": 3}), app="hermes-desktop")
        self.assertEqual(diagnostic["pip_failure_hint"], "dependency_resolution")
        self.assertNotIn("secret.invalid", json.dumps(diagnostic))
        self.assertNotIn("private-token", json.dumps(diagnostic))

    def test_pip_callback_failure_does_not_mask_cleanup_failure(self):
        with patch.object(INSTALL, "_pip_failure_hint", side_effect=RuntimeError("private")):
            holder = ["other"]
            INSTALL._pip_diagnostic_callback(holder, io.BytesIO(b"private"))
            self.assertEqual(holder, ["other"])
        with patch.object(INSTALL, "private_command", side_effect=INSTALL.CleanupError("private cleanup")):
            diagnostic = self._diagnostic(lambda: INSTALL._run(("synthetic",), stage="hermes_build",
                                                                operation="pip_install", pip_facts={"python_major": 3}), app="hermes-desktop")
        self.assertEqual(diagnostic["failure"], "cleanup_uncertain")

    def test_successful_subprocess_has_no_diagnostic(self):
        token = INSTALL._APP_CONTEXT.set("chatgpt-desktop")
        try:
            with patch.object(INSTALL, "private_command", return_value=0):
                stream = io.StringIO()
                with contextlib.redirect_stderr(stream):
                    self.assertTrue(INSTALL._run(("synthetic",), stage="hermes_build", operation="npm_pack"))
        finally:
            INSTALL._APP_CONTEXT.reset(token)
        self.assertEqual(stream.getvalue(), "")

    def test_hermes_commands_have_exact_operation_names(self):
        item = {"app": "hermes-desktop", "url": "https://github.com/NousResearch/hermes-agent.git",
                "revision": "b" * 40}
        commands = [argv for argv, _ in INSTALL.hermes_source_commands(item, Path("private"))]
        self.assertEqual([INSTALL._hermes_operation(command) for command in commands],
                         ["git_init", "git_remote_add", "git_fetch", "git_checkout", "venv_create",
                          "pip_install", "npm_ci", "npm_pack"])
        for command in commands:
            operation = INSTALL._hermes_operation(command)
            with patch.object(INSTALL, "private_command", return_value=11):
                diagnostic = self._diagnostic(lambda: INSTALL._run(command, stage="hermes_build", operation=operation), app="hermes-desktop")
            self.assertEqual(diagnostic["operation"], operation)
            self.assertEqual(diagnostic["return_code"], 11)
            if operation == "pip_install":
                self.assertEqual(diagnostic["pip_failure_hint"], "other")
                self.assertNotIn("python_major", diagnostic)

    def test_cleanup_uncertain_is_reported_without_private_details(self):
        with patch.object(INSTALL, "private_command", side_effect=INSTALL.CleanupError("private output")):
            diagnostic = self._diagnostic(lambda: INSTALL._run(("synthetic",), stage="installer",
                                                                operation="install"))
        self.assertEqual(diagnostic["failure"], "cleanup_uncertain")
        self.assertNotIn("private output", json.dumps(diagnostic))

    def test_app_context_is_reset_and_direct_failure_does_not_invent_identity(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(INSTALL, "_download", return_value=False):
            with self.assertRaises(RuntimeError):
                INSTALL.install_entry(release("claude-desktop", staged=False), "windows",
                                      Path(directory), Path(directory) / "work")
            self.assertIsNone(INSTALL._APP_CONTEXT.get())
        with patch.object(INSTALL, "private_command", return_value=7):
            with self.assertRaises(ValueError):
                INSTALL._run(("synthetic",), stage="installer", operation="install")
        self.assertIsNone(INSTALL._APP_CONTEXT.get())

    def test_diagnostic_privacy_excludes_command_paths_urls_and_environment(self):
        with patch.object(INSTALL, "private_command", return_value=9) as command, \
                patch.dict(os.environ, {"NAN_API_KEY": "fixture-secret"}):
            diagnostic = self._diagnostic(lambda: INSTALL._run(("https://secret.invalid", "/private/path"),
                                                                cwd="/private/work", stage="download",
                                                                operation="fetch_artifact"))
        self.assertEqual(set(diagnostic), {"schema_version", "app", "stage", "operation", "failure", "return_code"})
        self.assertNotIn("fixture-secret", json.dumps(diagnostic))
        command.assert_called_once()

    def test_materialization_verifies_copied_bytes_and_refuses_overwrite(self):
        token = INSTALL._APP_CONTEXT.set("chatgpt-desktop")
        try:
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
        finally:
            INSTALL._APP_CONTEXT.reset(token)

    def test_tampered_staged_bytes_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / ("chatgpt-desktop-" + "a" * 64)
            path.write_bytes(b"tampered")
            with self.assertRaises(RuntimeError):
                INSTALL.install_entry(release(), "windows", Path(directory), Path(directory) / "work")

    def test_missing_staged_bytes_are_rejected_without_download(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(INSTALL, "_download") as download:
            diagnostic = self._diagnostic(lambda: INSTALL.install_entry(
                release(), "windows", Path(directory), Path(directory) / "work"))
            self.assertEqual(diagnostic["failure"], "missing_artifact")
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

    def test_msix_refuses_existing_installation_or_failed_inventory_before_registration(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(INSTALL, "_run_output", return_value=None), \
                patch.object(INSTALL, "_run") as register:
            workspace = Path(directory)
            with self.assertRaises(RuntimeError):
                INSTALL._install_windows(release(), workspace / "package.msix", workspace)
            register.assert_not_called()

    def test_msix_registration_failure_is_not_reported_as_installed(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(INSTALL, "_run_output", return_value=""), \
                patch.object(INSTALL, "_run", return_value=False):
            workspace = Path(directory)
            with self.assertRaises(RuntimeError):
                INSTALL._install_windows(release(), workspace / "package.msix", workspace)

    def test_msix_unknown_identity_never_invokes_powershell(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(INSTALL, "_run_output") as inventory, \
                patch.object(INSTALL, "_run") as register:
            workspace = Path(directory)
            with self.assertRaises(ValueError):
                INSTALL._install_windows(release("unknown-desktop"), workspace / "package.msix", workspace)
            inventory.assert_not_called()
            register.assert_not_called()

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

    def test_hermes_build_sequence_installs_root_workspace_then_packs_desktop(self):
        item = {"app": "hermes-desktop", "url": "https://github.com/NousResearch/hermes-agent.git",
                "revision": "b" * 40}
        sequence = INSTALL.hermes_source_commands(item, Path("private"))
        self.assertEqual(sequence[-2], (("npm", "ci", "--no-audit", "--no-fund"), Path("private") / "hermes-agent"))
        self.assertEqual(sequence[-1], (("npm", "run", "pack"), Path("private") / "hermes-agent" / "apps" / "desktop"))
        self.assertLess(sequence.index(sequence[-2]), sequence.index(sequence[-1]))


if __name__ == "__main__":
    unittest.main()
