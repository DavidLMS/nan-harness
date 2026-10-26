#!/usr/bin/env python3
"""Closed qualification receipts and the managed-profile file boundary."""
import importlib.util
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
from types import ModuleType, SimpleNamespace
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "canary/actions"))
import cell

spec = importlib.util.spec_from_file_location("managed_hermes_profile", ROOT / "crates/nan-harness-runtime/src/prepared/hermes/nanh_profile.py")
profile = importlib.util.module_from_spec(spec)
spec.loader.exec_module(profile)
spec = importlib.util.spec_from_file_location("managed_hermes_main", ROOT / "crates/nan-harness-runtime/src/prepared/hermes/main.py")
runtime = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runtime)
sys.path.insert(0, str(ROOT / "canary/hermes"))
spec = importlib.util.spec_from_file_location("native_hermes_qualification", ROOT / "canary/hermes/check.py")
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)
spec = importlib.util.spec_from_file_location("hermes_frame_diagnostics", ROOT / "crates/nan-harness-runtime/src/prepared/hermes/frame_diagnostics.py")
frame_diagnostics = importlib.util.module_from_spec(spec)
spec.loader.exec_module(frame_diagnostics)


class Qualification(unittest.TestCase):
    def test_readiness_failure_survives_missing_control_and_cleanup_context(self):
        from hermes_diagnostics import managed_namespace_diagnostics
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory)
            home = config / "hermes-profiles/owned"
            home.mkdir(parents=True)
            identity = "b" * 32
            with patch.dict(os.environ, {"NAN_HARNESS_HERMES_DIAGNOSTICS": identity}):
                frame_diagnostics.readiness_failure(home, runtime.READINESS_EXIT_CODES["identity-supervisor"])
            self.assertEqual(managed_namespace_diagnostics(config, identity),
                             ["NH-HERMES-READINESS-IDENTITY-SUPERVISOR"])
            self.assertEqual(managed_namespace_diagnostics(config, "a" * 32), [])
            path = home / ".nanh-readiness-failure.json"
            value = json.loads(path.read_text())
            path.write_text(json.dumps({**value, "exitCode": "private payload"}))
            self.assertEqual(managed_namespace_diagnostics(config, identity), [])

    def test_expiry_preserves_owner_stack_before_cleanup_can_replace_it(self):
        from hermes_diagnostics import managed_profile_frames
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory)
            home = config / "hermes-profiles/owned"
            home.mkdir(parents=True)
            identity = "f" * 32
            value = {"schemaVersion": 1, "identity": identity, "action": "owner", "sequence": 4,
                     "stacks": [[{"module": "gateway_bootstrap", "line": 814}]]}
            frame_diagnostics.private_receipt(home, ".nanh-frames-owner.json", value)
            with patch.dict(os.environ, {"NAN_HARNESS_HERMES_DIAGNOSTICS": identity}):
                frame_diagnostics.readiness_state(home, "waiting")
                self.assertEqual(frame_diagnostics.current_readiness_state(home, identity), "waiting")
                frame_diagnostics.readiness_state(home, "expired")
                frame_diagnostics.readiness_state(home, "host-closed")
                self.assertEqual(frame_diagnostics.current_readiness_state(home, identity), "host-closed")
                self.assertEqual(frame_diagnostics.current_readiness_state(home, "a" * 32), "unobserved")
            frame_diagnostics.private_receipt(home, ".nanh-frames-owner.json", {**value, "sequence": 9})
            records = managed_profile_frames(config, identity)
            self.assertEqual(records, [{key: value[key] for key in ("action", "sequence", "stacks")}])

    def test_namespace_observation_preserves_native_names_without_exporting_home(self):
        from hermes_diagnostics import managed_namespace_diagnostics
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory)
            home = config / "hermes-profiles/owned"
            home.mkdir(parents=True)
            control = ModuleType("gateway.control_socket")
            original = lambda path: "synthetic-pipe-" + str(path)
            control.windows_pipe_name = original
            gateway = ModuleType("gateway")
            gateway.control_socket = control
            identity = "d" * 32
            calls = {"control": control}
            exec(compile("def _create_pipe(home):\n    return control.windows_pipe_name(home)\n"
                         "def query_runtime_control(home):\n    return control.windows_pipe_name(home)\n",
                         "/private/synthetic/gateway/runtime_bootstrap_windows.py", "exec"), calls)
            with patch.dict(sys.modules, {"gateway": gateway, "gateway.control_socket": control}), \
                    patch.dict(os.environ, {"NAN_HARNESS_HERMES_DIAGNOSTICS": identity}):
                frame_diagnostics.trace_control_namespace(home, "owner")
                self.assertEqual(calls["_create_pipe"](home), original(home))
                # A fresh process gets a fresh module; restore the original for this fixture.
                control.windows_pipe_name = original
                frame_diagnostics.trace_control_namespace(home, "ready")
                self.assertEqual(calls["query_runtime_control"](home), original(home))
                values = managed_namespace_diagnostics(config, identity)
                self.assertIn("NH-HERMES-CONTROL-READY-NAMESPACE-MATCH", values)
                self.assertIn("NH-HERMES-CONTROL-OWNER-HOME-MATCH", values)
                self.assertTrue(set(values) <= set(cell.HERMES_PROFILE_DIAGNOSTICS))
                self.assertNotIn(str(home), json.dumps(values))
                with patch.object(frame_diagnostics, "private_receipt", side_effect=OSError("private payload")):
                    self.assertEqual(calls["query_runtime_control"](config), original(config))

    def test_namespace_timing_is_closed_and_unknown_metadata_is_rejected(self):
        from hermes_diagnostics import managed_namespace_diagnostics
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory)
            home = config / "hermes-profiles/owned"
            home.mkdir(parents=True)
            identity = "e" * 32
            base = {"schemaVersion": 1, "identity": identity, "matchesManagedName": True,
                    "samePhysicalHome": True, "verbatimHome": False, "observedNs": 100_000_000_000,
                    "readinessState": "expired"}
            for action, role, digest in (("owner", "server", "a" * 64), ("ready", "client", "b" * 64)):
                (home / (".nanh-namespace-" + action + "-" + role + ".json")).write_text(json.dumps(
                    {**base, "action": action, "role": role, "digest": digest}))
            error = {"schemaVersion": 1, "identity": identity, "kind": "missing", "errno": 2, "winerror": 2,
                     "frames": [], "deadlineNs": 98_000_000_000, "observedNs": 38_000_000_000}
            (home / ".nanh-control-error.json").write_text(json.dumps(error))
            values = managed_namespace_diagnostics(config, identity)
            self.assertIn("NH-HERMES-CONTROL-READY-NAMESPACE-MISMATCH", values)
            self.assertIn("NH-HERMES-CONTROL-SERVER-NAME-LATE-UNDER-5S", values)
            self.assertTrue(set(values) <= set(cell.HERMES_PROFILE_DIAGNOSTICS))
            path = home / ".nanh-namespace-owner-server.json"
            invalid = json.loads(path.read_text())
            invalid["private"] = "payload"
            path.write_text(json.dumps(invalid))
            self.assertNotIn("NH-HERMES-CONTROL-READY-NAMESPACE-MISMATCH", managed_namespace_diagnostics(config, identity))

    def test_control_exception_preserves_the_throw_site_and_only_numeric_os_codes(self):
        from hermes_diagnostics import managed_profile_frames, safe_frames
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory)
            home = config / "hermes-profiles/owned"
            home.mkdir(parents=True)
            identity = "c" * 32
            with patch.dict(os.environ, {"NAN_HARNESS_HERMES_DIAGNOSTICS": identity,
                                        "NAN_HARNESS_HERMES_MANAGED_HOME": str(home)}):
                try:
                    exec(compile("raise FileNotFoundError(2, 'private synthetic message and path')",
                                 "/private/synthetic/gateway/runtime_bootstrap_windows.py", "exec"))
                except FileNotFoundError as error:
                    frame_diagnostics.record_exception(error)
                values = managed_profile_frames(config, identity)
                self.assertEqual(len(values), 1)
                self.assertEqual(values[0]["action"], "control-error")
                self.assertEqual(values[0]["errorKind"], "missing")
                self.assertEqual(values[0]["errno"], 2)
                self.assertEqual(values[0]["stacks"], [[{"module": "runtime_windows", "line": 1}]])
                self.assertNotIn("private", json.dumps(values))
                self.assertNotIn("synthetic", json.dumps(values))
                self.assertEqual(safe_frames([{**values[0], "winerror": True}]), [])
                self.assertEqual(safe_frames([{**values[0], "errno": "private"}]), [])
                frame_diagnostics.clear_exception()
                self.assertEqual(managed_profile_frames(config, identity), [])

    def test_source_positions_contain_only_known_module_identifiers_and_lines(self):
        namespace = {"sys": sys}
        exec(compile("def held():\n    private = 'sensitive synthetic payload'\n    return sys._getframe()\n",
                     "/private/synthetic/user/hermes_cli/main.py", "exec"), namespace)
        frame = namespace["held"]()
        try:
            positions = frame_diagnostics.public_frames(frame)
            self.assertEqual(positions, [{"module": "cli_main", "line": 3}])
            self.assertNotIn("private", json.dumps(positions))
            self.assertNotIn("sensitive", json.dumps(positions))
        finally:
            frame.clear()

    def test_frame_receipts_are_current_bounded_and_closed(self):
        from hermes_diagnostics import managed_profile_frames, safe_frames
        positions = [{"action": "owner", "sequence": 20, "stacks": [[{"module": "cli_main", "line": 12}]]}]
        self.assertEqual(safe_frames(positions), positions)
        for bad in [{"module": "private payload", "line": 12}, {"module": "cli_main", "line": True},
                    {"module": "cli_main", "line": 100001}, {"module": "cli_main", "line": 12, "private": "payload"}]:
            self.assertEqual(safe_frames([{**positions[0], "stacks": [[bad]]}]), [])
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory)
            profile_home = config / "hermes-profiles/owned"
            profile_home.mkdir(parents=True)
            identity = "a" * 32
            receipt = profile_home / ".nanh-frames-owner.json"
            receipt.write_text(json.dumps({"schemaVersion": 1, "identity": identity, **positions[0]}))
            self.assertEqual(managed_profile_frames(config, identity), positions)
            self.assertEqual(managed_profile_frames(config, "b" * 32), [])
            if os.name != "nt":
                receipt.unlink()
                foreign = config / "native-receipt.json"
                foreign.write_text(json.dumps({"schemaVersion": 1, "identity": identity, **positions[0]}))
                receipt.symlink_to(foreign)
                self.assertEqual(managed_profile_frames(config, identity), [])

    def test_position_watch_is_opt_in_and_cannot_follow_existing_output_links(self):
        with patch.dict(os.environ, {"NAN_HARNESS_HERMES_DIAGNOSTICS": ""}), \
                patch.object(frame_diagnostics.threading, "Thread") as thread:
            with frame_diagnostics.watch("owner"):
                pass
            thread.assert_not_called()
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            foreign = home / "foreign"
            foreign.write_text("preserve")
            receipt = home / ".nanh-frames-owner.json"
            if os.name != "nt":
                receipt.symlink_to(foreign)
            frame_diagnostics.snapshot(home, "owner", "a" * 32, 1)
            self.assertEqual(foreign.read_text(), "preserve")
            self.assertFalse(receipt.is_symlink())
            value = json.loads(receipt.read_text())
            self.assertEqual(value["identity"], "a" * 32)
            self.assertEqual(value["sequence"], 1)

    def test_native_frontend_diagnostics_never_export_private_text(self):
        self.assertEqual(native.closed_frontend_diagnostics("private synthetic payload"), [])
        self.assertEqual(native.closed_frontend_diagnostics(
            "invalid uri authority: private synthetic path\nNoConsoleScreenBufferError: private synthetic payload"),
            ["NH-HERMES-FRONTEND-CONSOLE", "NH-HERMES-FRONTEND-URI"])
        self.assertTrue(set(native.closed_frontend_diagnostics("Gateway bootstrap timed out"))
                        <= set(cell.HERMES_PROFILE_DIAGNOSTICS))
        self.assertEqual(native.closed_frontend_diagnostics("[WinError 9999] PrivateError: private value"), [])
        codes = native.closed_frontend_diagnostics("FileNotFoundError: [WinError 2] private path; managed_worker_hello_timeout")
        self.assertEqual(codes, ["NH-HERMES-FRONTEND-WINERROR-2", "NH-HERMES-FRONTEND-EXCEPTION-PATH",
                                 "NH-HERMES-FRONTEND-RPC-MANAGED-WORKER-HELLO-TIMEOUT"])
        self.assertTrue(set(codes) <= set(cell.HERMES_PROFILE_DIAGNOSTICS))

    def test_only_bounded_declared_managed_profile_logs_are_classified(self):
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / "config"
            logs = config / "hermes-profiles/owned/logs"
            logs.mkdir(parents=True)
            # The diagnostic at the head exceeds the read budget and must be absent.
            (logs / "managed-worker.log").write_bytes(b"AttributeError: private\n" + b"x" * 65536 + b"\n[WinError 2] private path")
            (logs / "undeclared.log").write_text("PermissionError: private path")
            foreign = Path(directory) / "native-errors.log"
            foreign.write_text("[WinError 6] private native state")
            if os.name != "nt":
                (logs / "errors.log").symlink_to(foreign)
            codes = native.managed_profile_diagnostics(config)
            self.assertEqual(codes, ["NH-HERMES-FRONTEND-WINERROR-2"])
            self.assertTrue(set(codes) <= set(cell.HERMES_PROFILE_DIAGNOSTICS))

    def test_machine_frontend_progress_is_a_closed_fact(self):
        for output, expected in [("private synthetic text", "NO-INIT"),
                                 ('{"type":"system","subtype":"init"}', "INIT-ONLY"),
                                 ('{"type":"result","session_id":""}', "RESULT-UNBOUND"),
                                 ('{"type":"result","session_id":"private synthetic id"}', "RESULT-BOUND")]:
            with self.subTest(expected=expected):
                self.assertEqual(native.frontend_phase(output), "NH-HERMES-FRONTEND-" + expected)

    def test_cleanup_priority_retains_every_closed_cause_without_private_values(self):
        codes = ["NH-HERMES-CLEANUP", "NH-HERMES-FRONTEND-WINERROR-2", "private synthetic payload"]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args = SimpleNamespace(directory=root, binary=root / "nanh", canary=root / "canary", harness="hermes",
                                   stage="conformance", trigger="manual", output=root / "report.json")
            (root / "state.json").write_text(json.dumps({"startedAt": cell.timestamp(),
                "durationMilliseconds": 0, "checks": [], "outcome": "passed"}))
            frames = [{"action": "owner", "sequence": 1, "stacks": [[{"module": "cli_main", "line": 12}]]}]
            failure = cell.HermesConformanceFailure("conformance-hermes-history-create-cleanup-exit-nonzero", codes, "proven", frames)
            with patch.object(cell, "private_command", return_value=0):
                cell.failed_report(args, failure)
            value = json.loads(args.output.read_text())["failure"]
            self.assertEqual(value["code"], failure.code)
            for code in codes[:-1]:
                self.assertIn(code, value["summary"])
            self.assertIn('"module":"cli_main","line":12', value["summary"])
            self.assertNotIn("private synthetic", args.output.read_text())
            self.assertEqual(cell.HermesConformanceFailure(failure.code, codes, {"private": "value"}).cleanup, "unknown")
            self.assertEqual(cell.HermesConformanceFailure(failure.code, codes, "proven", [{"private": "payload"}]).frames, [])

    def test_machine_frontend_is_selected_from_the_installed_version(self):
        for version, modern in [("0.21.0", False), ("0.22.0", True),
                                ("Hermes 0.22.5", True), ("unknown", False)]:
            with self.subTest(version=version), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                args = SimpleNamespace(directory=root, binary=root / "nanh",
                                       canary=root / "canary", harness="hermes")
                def command(arguments, _directory, *, output, **_kwargs):
                    self.assertEqual("--hermes-stream-json" in arguments, modern)
                    output.write_text(json.dumps({"schemaVersion": 2, "harness": "hermes", "outcome": "passed",
                        "scenarios": [{"name": name, "status": "passed", "checks": ["synthetic"],
                                       "durationMilliseconds": 0} for name in cell.CONFORMANCE_SCENARIOS]}))
                    return 0
                with patch.object(cell, "cell_environment", return_value={}), \
                        patch.object(cell, "hermes_profile_probe") as profile_probe, \
                        patch.object(cell, "private_command", side_effect=command):
                    self.assertEqual(cell.conformance(args, {"harness": {"version": version}}), 1)
                    self.assertEqual(profile_probe.call_count, int(modern))
                self.assertFalse((root / "conformance-private.json").exists())

    def test_launch_wrapper_waits_for_the_child_and_preserves_its_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "child-completed"
            child = ("import os,sys,time; from pathlib import Path; time.sleep(0.1); "
                     "assert os.environ['NAN_API_KEY']==sys.argv[2]; "
                     "Path(sys.argv[1]).write_text('completed'); sys.exit(7)")
            result = subprocess.run([sys.executable, "-c", native.LAUNCH_WRAPPER,
                                     sys.executable, "-c", child, str(marker), native.KEY],
                                    capture_output=True, timeout=10, check=False)
            self.assertEqual(result.returncode, 7)
            self.assertEqual(marker.read_text(), "completed")

    def probe(self, value, status=0, failure=None):
        with tempfile.TemporaryDirectory() as directory:
            args = SimpleNamespace(directory=Path(directory), binary=Path("synthetic-nanh"))
            marker = args.directory / "hermes-profile-private.json"

            def command(*_args, **_kwargs):
                if failure:
                    raise failure
                marker.write_text(json.dumps(value))
                return status

            with patch.object(cell.shutil, "which", return_value="synthetic-hermes"), patch.object(cell, "private_command", side_effect=command):
                try:
                    cell.hermes_profile_probe(args, {"PATH": "synthetic"})
                finally:
                    self.assertFalse(marker.exists())

    def test_requires_the_complete_exact_positive_receipt(self):
        valid = {"schemaVersion": 1, "status": "passed", "checks": cell.HERMES_PROFILE_CHECKS}
        self.probe(valid)
        for value, status in [(dict(valid, checks=valid["checks"][:-1]), 0), (dict(valid, extra="synthetic"), 0),
                              (dict(valid, schemaVersion=True), 0), (valid, 1)]:
            with self.subTest(value=value, status=status), self.assertRaises(cell.ConformanceFailure):
                self.probe(value, status)

    def test_exposes_only_closed_failure_phases_and_prioritizes_cleanup(self):
        for phase, cleanup, expected in [("history-resume", "proven", "history-resume"),
                                         ("reasoning-effort", "unproven", "cleanup")]:
            with self.subTest(phase=phase), self.assertRaises(cell.ConformanceFailure) as caught:
                self.probe({"schemaVersion": 1, "status": "failed", "phase": phase, "cleanup": cleanup}, 1)
            self.assertEqual(caught.exception.code, "conformance-hermes-" + expected + "-exit-nonzero")
        with self.assertRaises(cell.ConformanceFailure) as caught:
            self.probe({"schemaVersion": 1, "status": "failed", "phase": "private synthetic payload"}, 1)
        self.assertEqual(caught.exception.code, "conformance-hermes-report-invalid")
        with self.assertRaises(cell.ConformanceFailure) as caught:
            self.probe({"schemaVersion": 1, "status": "failed", "phase": "history-create", "cleanup": "proven",
                        "diagnostics": ["NH-HERMES-READINESS", "private synthetic payload"]}, 1)
        self.assertEqual(caught.exception.code, "conformance-hermes-history-create-readiness-exit-nonzero")

    def test_missing_and_timed_out_probes_cannot_qualify(self):
        for failure, code in [(OSError(), "unavailable"), (cell.StageTimeout(), "timeout")]:
            with self.subTest(code=code), self.assertRaises(cell.ConformanceFailure) as caught:
                self.probe(None, failure=failure)
            self.assertEqual(caught.exception.code, "conformance-hermes-" + code)


class PrivateFiles(unittest.TestCase):
    def test_native_owner_stdin_is_eof_while_host_lifetime_remains_open(self):
        script = """
import importlib.util, os, sys, threading, types
os.environ.pop('NAN_HARNESS_HERMES_DIAGNOSTICS', None)
os.environ['NAN_HARNESS_HERMES_MANAGED_HOME'] = os.getcwd()
spec = importlib.util.spec_from_file_location('owner_fixture', sys.argv[1])
runtime = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runtime)
stopped = threading.Event()
gateway, status = types.ModuleType('gateway'), types.ModuleType('gateway.status')
def stop(pid):
    assert pid == os.getpid()
    stopped.set()
    return True
status.write_planned_stop_marker = stop
sys.modules.update({'gateway': gateway, 'gateway.status': status})
def native(*_args, **_kwargs):
    assert not sys.stdin.isatty()
    assert sys.stdin.buffer.read(1) == b''
    if os.name == 'nt':
        import ctypes, _winapi
        # DLL initialization can probe the OS standard handle, not the CRT fd.
        info = ctypes.create_string_buffer(128)
        ctypes.windll.kernel32.GetFileInformationByHandle(
            ctypes.c_void_p(_winapi.GetStdHandle(-10)), info)
    print('native-ready', flush=True)
    assert stopped.wait(5)
runtime.runpy.run_module = native
runtime.owner()
print('host-stopped', flush=True)
"""
        child = subprocess.Popen([sys.executable, "-c", script, runtime.__file__],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 stderr=subprocess.DEVNULL, text=True)
        lines = queue.Queue()
        def read():
            for line in child.stdout:
                lines.put(line.strip())
        reader = threading.Thread(target=read, daemon=True)
        reader.start()
        try:
            self.assertEqual(lines.get(timeout=5), "native-ready")
            self.assertIsNone(child.poll())
            child.stdin.close()
            self.assertEqual(lines.get(timeout=5), "host-stopped")
            self.assertEqual(child.wait(timeout=5), 0)
        finally:
            if child.poll() is None:
                child.kill()
                child.wait()
            reader.join(timeout=5)
            child.stdout.close()
            if not child.stdin.closed:
                child.stdin.close()

    def test_readiness_preserves_closed_control_and_runtime_failure_categories(self):
        gateway = ModuleType("hermes_cli.gateway_runtime")
        discovery = ModuleType("hermes_cli.gateway_runtime_discovery")
        gateway.discover_gateway_endpoint = lambda *_args, **_kwargs: SimpleNamespace(
            state="incompatible", endpoint=None, reason_code="runtime_protocol")
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory).resolve()
            discovery.query_identify = lambda *_args, **_kwargs: dict(
                pid=123, hermes_home=str(home), supervisor="external")
            modules = {"hermes_cli": ModuleType("hermes_cli"),
                       "hermes_cli.gateway_runtime": gateway,
                       "hermes_cli.gateway_runtime_discovery": discovery}
            with patch.dict(sys.modules, modules), patch.object(runtime.time, "monotonic", side_effect=[0, 1, runtime.READINESS_TIMEOUT_SECONDS + 1]), patch.object(runtime.time, "sleep"):
                with self.assertRaises(runtime.ReadinessFailure) as caught:
                    runtime.ready(home, 123)
                self.assertEqual(caught.exception.exit_code, runtime.READINESS_EXIT_CODES["runtime-protocol"])
            def absent(*_args, **_kwargs):
                raise FileNotFoundError("private synthetic path")
            discovery.query_identify = absent
            with patch.dict(sys.modules, modules), patch.object(runtime.time, "monotonic", side_effect=[0, 1, runtime.READINESS_TIMEOUT_SECONDS + 1]), patch.object(runtime.time, "sleep"):
                with self.assertRaises(runtime.ReadinessFailure) as caught:
                    runtime.ready(home, 123)
                self.assertEqual(caught.exception.exit_code, runtime.READINESS_EXIT_CODES["control-absent"])
                self.assertNotIn("private", str(caught.exception))

    def test_routing_cannot_be_overridden_or_bypassed_after_native_parsing(self):
        runtime.require_creation_routing(SimpleNamespace(reasoning="high"))
        for field, value in [("provider", "other"), ("model", "other"), ("api_key", "synthetic"),
                             ("base_url", "http://127.0.0.1:1"), ("safe_mode", True), ("ignore_user_config", True)]:
            with self.subTest(field=field), self.assertRaises(RuntimeError):
                runtime.require_creation_routing(SimpleNamespace(**{field: value}))
    def test_update_requires_an_existing_regular_file(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "config.yaml"
            with self.assertRaises(RuntimeError):
                profile.private_write(path, "synthetic")
            path.write_text("old")
            path.chmod(0o600)
            profile.private_write(path, "new")
            self.assertEqual(path.read_text(), "new")
            if os.name != "nt":
                self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    @unittest.skipIf(os.name == "nt", "native Windows ACL coverage runs in the hosted cell")
    def test_update_never_follows_a_link_into_native_state(self):
        with tempfile.TemporaryDirectory() as directory:
            original = Path(directory) / "native"
            original.write_text("preserve")
            link = Path(directory) / "config.yaml"
            link.symlink_to(original)
            with self.assertRaises(RuntimeError):
                profile.private_write(link, "replace")
            self.assertEqual(original.read_text(), "preserve")


if __name__ == "__main__":
    unittest.main()
