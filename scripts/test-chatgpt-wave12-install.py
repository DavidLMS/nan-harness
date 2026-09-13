#!/usr/bin/env python3
"""Synthetic installer contracts. No test executes a package or host command."""

from contextlib import nullcontext
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import select
import stat
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "official_install", Path(__file__).with_name("chatgpt-wave12-install.py"))
installer = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(installer)


class InstallerTests(unittest.TestCase):
    @staticmethod
    def checker_report(cleanup="passed"):
        return json.dumps({"schemaVersion": 3, "checkerVersion": "0.1.0", "runId": "a" * 32,
                           "startedAt": "2026-09-13T00:00:00Z", "platform": "linux",
                           "architecture": "x86_64", "cleanup": cleanup,
                           "results": [{"app": "chatgpt-desktop", "cleanup": cleanup}]})

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
        self.package_has_profile = True
        self.fields = {"Package": "chatgpt", "Version": installer.VERSION, "Architecture": "amd64"}
        for name, value in (("EXECUTABLE", self.exe), ("PROFILE", self.profile),
                            ("RESTRICTION", self.restriction),
                            ("PACKAGE_SHA256", hashlib.sha256(self.package).hexdigest())):
            self.enterContext(patch.object(installer, name, value))
    def fake_command(self, args, **kwargs):
        self.calls.append(args)
        if args[0] == "curl":
            Path(args[args.index("--output") + 1]).write_bytes(self.package)
        elif args[:2] == ["dpkg-deb", "--contents"]:
            return ("-rw-r--r-- root/root 178 2026-09-13 00:00 ./etc/apparmor.d/chatgpt\n"
                    if self.package_has_profile else
                    "-rwxr-xr-x root/root 178 2026-09-13 00:00 ./usr/lib/chatgpt/ChatGPT\n")
        elif args[0] == "dpkg-deb":
            return self.fields[args[-1]]
        elif args[:4] == ["sudo", "-n", "apt-get", "install"]:
            self.exe.write_bytes(b"synthetic executable")
            if self.package_has_profile:
                self.profile.write_text(installer.CUSTOM_PROFILE)
            if self.change:
                self.change()
        elif args[:4] == ["sudo", "-n", "apt-get", "purge"]:
            self.exe.unlink(missing_ok=True)
            self.profile.unlink(missing_ok=True)
        elif args[:4] == ["sudo", "-n", "apparmor_parser", "-R"]:
            self.profile.unlink(missing_ok=True)
        elif args[:4] == ["sudo", "-n", "apparmor_parser", "-r"]:
            self.profile.write_text(Path(args[-1]).read_text())
        elif args[:5] == ["sudo", "-n", "install", "-o", "root"]:
            self.profile.write_text(Path(args[-2]).read_text())
        elif args[:4] == ["sudo", "-n", "rm", "--"]:
            self.profile.unlink(missing_ok=True)
        elif args[:2] == ["pgrep", "-x"]:
            return ""
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
                                      "packageProfile", "profileLoaded", "profileAttachmentObservation",
                                      "userNamespaceRestrictionBefore",
                                      "userNamespaceRestrictionAfter"})
        self.assertEqual(result["userNamespaceRestrictionAfter"], 1)
        self.assertTrue(result["profileLoaded"])
        self.assertEqual(result["packageProfile"], "package-profile-present")
        self.assertEqual(result["profileAttachmentObservation"], "listed")
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

    def test_missing_package_profile_uses_exact_custom_attachment(self):
        self.package_has_profile = False
        self.install()
        result = json.loads((self.output / "sandbox-setup.json").read_text())
        self.assertEqual(result["packageProfile"], "package-profile-absent")
        self.assertEqual(self.profile.read_text(), installer.CUSTOM_PROFILE)
        self.assertIn(["sudo", "-n", "install", "-o", "root", "-g", "root", "-m", "644",
                       str(self.output / "chatgpt-apparmor-profile"), str(self.profile)], self.calls)
        self.assertIn(["sudo", "-n", "apparmor_parser", "-r", str(self.profile)], self.calls)

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
        with self.assertRaisesRegex(ValueError, "^runner command failed$"):
            installer.command(["sh", "-c", "printf synthetic >&2; exit 1"])

    def test_command_success_and_output_overflow(self):
        self.assertEqual(installer.command(["sh", "-c", "printf success"]), "success")
        with self.assertRaisesRegex(ValueError, "output too large"):
            installer.command(["sh", "-c", "head -c 65537 /dev/zero"])

    @unittest.skipUnless(sys.platform.startswith("linux"), "process-group contract is Linux-only")
    def test_command_timeout_and_descendant_held_pipes_are_bounded(self):
        pid_file = self.root / "descendant.pid"
        child = f"import os,time; open({str(pid_file)!r}, 'w').write(str(os.getpid())); time.sleep(30)"
        script = ("import subprocess,sys,time; "
                  f"subprocess.Popen([sys.executable, '-c', {child!r}]); "
                  "time.sleep(.2); print('parent-exited', flush=True)")
        outer = ("import importlib.util, subprocess; "
                 f"s=importlib.util.spec_from_file_location('i', {str(Path(installer.__file__)).__repr__()}); "
                 "m=importlib.util.module_from_spec(s); s.loader.exec_module(m); "
                 f"\ntry: m.command([sys.executable, '-c', {script!r}], timeout=1)\n"
                 f"except subprocess.TimeoutExpired:\n"
                 f" import os,time; time.sleep(.1); pid=int(open({str(pid_file)!r}).read()); "
                 "\n try: state=open(f'/proc/{pid}/stat').read().split()[2]\n"
                 " except FileNotFoundError: state='gone'\n"
                 " if state not in ('Z', 'gone'): raise SystemExit('descendant leaked')\n"
                 " print('bounded')")
        result = subprocess.run([sys.executable, "-c", "import sys; " + outer],
                                timeout=5, check=True, capture_output=True, text=True)
        self.assertEqual(result.stdout.strip(), "bounded")

    def test_command_primary_timeout_is_bounded(self):
        with self.assertRaises(subprocess.TimeoutExpired):
            installer.command(["sh", "-c", "sleep 30"], timeout=0.1)

    @unittest.skipUnless(sys.platform.startswith("linux"), "process cleanup contract is Linux-only")
    def test_command_setup_register_and_read_failures_reap_owned_process(self):
        def assert_stopped(pid_file):
            pid = int(pid_file.read_text())
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                return
            try:
                state = Path(f"/proc/{pid}/stat").read_text().split()[2]
            except FileNotFoundError:
                return
            self.assertEqual(state, "Z", f"owned process {pid} was not reaped")

        class FailingSelector:
            def register(self, *_args):
                raise OSError("synthetic register failure")

            def get_map(self):
                return {}

            def close(self):
                return None

        cases = (("setup", patch.object(installer.selectors, "DefaultSelector",
                                         side_effect=OSError("synthetic setup failure"))),
                 ("register", patch.object(installer.selectors, "DefaultSelector",
                                            return_value=FailingSelector())),
                 ("read", None))
        for name, failure in cases:
            with self.subTest(name=name):
                pid_file = self.root / (name + ".pid")
                ready_file = self.root / (name + ".ready")
                read_fault = {"enabled": False}
                read_phases = []
                real_os_read = os.read

                def controlled_read(fd, size):
                    read_phases.append("command" if read_fault["enabled"] else "launch")
                    if read_fault["enabled"]:
                        raise OSError("synthetic read failure")
                    return real_os_read(fd, size)

                real_popen = installer.subprocess.Popen

                def launch(*args, **kwargs):
                    process = real_popen(*args, **kwargs)
                    deadline = time.monotonic() + 1
                    while not ready_file.exists() and time.monotonic() < deadline:
                        select.select([], [], [], 0.01)
                    if not ready_file.exists():
                        try:
                            process.kill()
                        except ProcessLookupError:
                            pass
                        process.wait(timeout=1)
                        raise AssertionError("synthetic child readiness handshake failed")
                    pid_file.write_text(str(process.pid))
                    if name == "read":
                        read_fault["enabled"] = True
                    return process

                read_context = (patch.object(installer.os, "read", side_effect=controlled_read)
                                if name == "read" else nullcontext())
                failure_context = failure or nullcontext()
                with patch.object(installer.subprocess, "Popen", side_effect=launch), \
                        failure_context, read_context, self.assertRaises(OSError):
                    child = ("from pathlib import Path; "
                             f"Path({str(ready_file)!r}).write_text('ready'); "
                             "import time; time.sleep(30)")
                    installer.command([sys.executable, "-c", child], timeout=1)
                if name == "read":
                    self.assertIn("launch", read_phases)
                    self.assertIn("command", read_phases)
                assert_stopped(pid_file)

    def test_file_and_report_reads_are_bounded(self):
        self.restriction.write_bytes(b"1" * (installer.MAX_RESTRICTION_BYTES + 1))
        with self.assertRaisesRegex(ValueError, "too large"):
            installer.restriction_value()
        self.profile.write_bytes(b"x" * (installer.MAX_PROFILE_BYTES + 1))
        with self.assertRaisesRegex(ValueError, "too large"):
            installer.profile_text()
        report = self.root / "oversized.json"
        report.write_bytes(b"x" * (installer.MAX_JOURNAL_BYTES + 1))
        with self.assertRaisesRegex(ValueError, "too large"):
            installer.bounded_json(report)

    def test_cleanup_requires_process_stop_and_records_success(self):
        self.exe.write_bytes(b"synthetic executable")
        self.profile.write_text(installer.CUSTOM_PROFILE)
        report = self.root / "report.json"
        report.write_text(self.checker_report())
        self.output.mkdir()
        (self.output / "sandbox-journal.json").write_text(json.dumps(
            {"schemaVersion": 1, "kind": "chatgpt-sandbox-journal",
             "packageSha256": installer.PACKAGE_SHA256, "packageVersion": installer.VERSION,
             "createdProfile": False, "phase": "installed", "launchState": "prelaunch",
             "executableSha256": installer.digest(self.exe),
             "profileSha256": installer.digest(self.profile)}))
        with patch.object(installer, "command", side_effect=self.fake_command), \
                patch.object(installer, "profile_loaded", return_value=True), \
                patch.object(installer, "root_owned_file"):
            installer.cleanup(self.output, report)
        result = json.loads((self.output / "sandbox-cleanup.json").read_text())
        self.assertEqual(result, {"kind": "chatgpt-official-sandbox-cleanup",
                                  "outcome": "cleaned", "packageRemoved": True,
                                  "profileRemoved": True, "restrictionAfter": "1",
                                  "restrictionBefore": "1", "schemaVersion": 1})

    def test_cleanup_failure_is_recorded_without_claiming_success(self):
        self.output.mkdir()
        journal = {"schemaVersion": 1, "kind": "chatgpt-sandbox-journal",
                   "packageSha256": installer.PACKAGE_SHA256, "packageVersion": installer.VERSION,
                   "phase": "installed", "launchState": "prelaunch"}
        (self.output / "sandbox-journal.json").write_text(json.dumps(journal))
        report = self.root / "report.json"
        report.write_text(self.checker_report())
        with patch.object(installer, "command", side_effect=ValueError("synthetic")), \
                self.assertRaisesRegex(ValueError, "cleanup failed"):
            installer.cleanup(self.output, report)
        result = json.loads((self.output / "sandbox-cleanup.json").read_text())
        self.assertEqual(result["outcome"], "cleanup-failed")
        self.assertFalse(result["packageRemoved"])

    def test_cleanup_rejects_failed_checker_report_before_mutation(self):
        self.output.mkdir()
        journal = {"schemaVersion": 1, "kind": "chatgpt-sandbox-journal",
                   "packageSha256": installer.PACKAGE_SHA256, "packageVersion": installer.VERSION,
                   "phase": "installed", "launchState": "prelaunch"}
        (self.output / "sandbox-journal.json").write_text(json.dumps(journal))
        report = self.root / "failed-report.json"
        report.write_text(self.checker_report(cleanup="failed"))
        def process_only(args, **kwargs):
            if args[:2] == ["pgrep", "-x"]:
                return ""
            raise AssertionError("privileged mutation")
        with patch.object(installer, "command", side_effect=process_only), \
                self.assertRaisesRegex(ValueError, "cleanup failed"):
            installer.cleanup(self.output, report)
        result = json.loads((self.output / "sandbox-cleanup.json").read_text())
        self.assertEqual(result["outcome"], "cleanup-failed")
        self.assertFalse(result["profileRemoved"])

    def test_prelaunch_partial_package_cleanup_needs_no_checker_report(self):
        self.output.mkdir()
        package = self.output / "chatgpt.deb"
        package.write_bytes(self.package)
        (self.output / "sandbox-journal.json").write_text(json.dumps(
            {"schemaVersion": 1, "kind": "chatgpt-sandbox-journal",
             "packageSha256": installer.PACKAGE_SHA256, "packageVersion": installer.VERSION,
             "phase": "package-verified", "launchState": "prelaunch"}))
        with patch.object(installer, "command", side_effect=self.fake_command):
            installer.cleanup(self.output, None)
        result = json.loads((self.output / "sandbox-cleanup.json").read_text())
        self.assertEqual(result["outcome"], "cleaned")
        self.assertTrue(result["packageRemoved"])
        self.assertFalse(package.exists())

    def test_launch_mark_and_live_process_fail_closed(self):
        self.output.mkdir()
        (self.output / "sandbox-journal.json").write_text(json.dumps(
            {"schemaVersion": 1, "kind": "chatgpt-sandbox-journal",
             "packageSha256": installer.PACKAGE_SHA256, "packageVersion": installer.VERSION,
             "phase": "installed", "launchState": "prelaunch"}))
        installer.mark_launch(self.output)
        journal = json.loads((self.output / "sandbox-journal.json").read_text())
        self.assertEqual(journal["launchState"], "launched")
        report = self.root / "report.json"
        report.write_text(self.checker_report())
        with patch.object(installer, "command", return_value="123"), \
                self.assertRaisesRegex(ValueError, "cleanup failed"):
            installer.cleanup(self.output, report)
        result = json.loads((self.output / "sandbox-cleanup.json").read_text())
        self.assertEqual(result["outcome"], "cleanup-failed")

    def test_invalid_phase_emits_uncertain_cleanup_evidence(self):
        self.output.mkdir()
        (self.output / "sandbox-journal.json").write_text(json.dumps(
            {"schemaVersion": 1, "kind": "chatgpt-sandbox-journal",
             "packageSha256": installer.PACKAGE_SHA256, "packageVersion": installer.VERSION,
             "phase": "download-started", "launchState": "prelaunch"}))
        with self.assertRaisesRegex(ValueError, "cleanup failed"):
            installer.cleanup(self.output, None)
        result = json.loads((self.output / "sandbox-cleanup.json").read_text())
        self.assertEqual(result["outcome"], "cleanup-uncertain")
        self.assertFalse(result["profileRemoved"])


if __name__ == "__main__":
    unittest.main()
