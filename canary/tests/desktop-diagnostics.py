#!/usr/bin/env python3
"""Synthetic privacy, bounds and containment contracts for diagnostics."""

import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "canary/actions"))
import desktop_diagnostics as D

SHA = "a" * 40


def install():
    return {"schema_version": 1, "app": "hermes-desktop", "stage": "hermes_build",
            "operation": "pip_install", "failure": "nonzero_exit", "return_code": 17}


def native():
    return {"schemaVersion": 1, "app": "pen-desktop", "probeIndex": 0, "mode": "deterministic",
            "launchStage": "window-unavailable", "composer": [], "truncated": False,
            "guiAcquisition": {"stage": "window-ownership", "errorCategory": "other", "reason": "isolation-unavailable"},
            "cleanup": {"stage": "stop", "originalReason": "isolation-unavailable", "reason": "cleanup-failed"}}


def line(record, prefix=b"DESKTOP_INSTALL_DIAGNOSTIC:"):
    return prefix + json.dumps(record).encode() + b"\n"


class DiagnosticTests(unittest.TestCase):
    def test_unpositioned_marker_is_a_closed_failure_not_success(self):
        record = {**native(), "launchStage": "window-acquired",
                  "composer": [{"operation": "verify-response-visual",
                                "errorCategory": "marker-without-composer-anchor"}],
                  "resultReason": "selector-not-matched"}
        D.validate_native(record)
        record["composer"][0]["marker"] = "private"
        with self.assertRaises(ValueError):
            D.validate_native(record)

    def test_startup_facts_are_closed_numeric_and_scoped_to_chatgpt(self):
        record = {**native(), "app": "chatgpt-desktop", "launchFailure": "native-app-exited",
                  "startup": {"exit": {"signal": 6}, "hint": "no-usable-sandbox"}}
        capture = D.Capture()
        capture.observe(io.BytesIO(line(record, b"DESKTOP_DIAGNOSTIC:")))
        self.assertEqual(capture.events, [{"kind": "native", "record": record}])
        for startup in ({"exit": {"code": 1, "signal": 6}, "hint": "unknown"},
                        {"exit": {"signal": 0}, "hint": "unknown"},
                        {"exit": {"code": True}, "hint": "unknown"},
                        {"hint": "private stderr"}, {"hint": "unknown", "stderr": "private"}):
            with self.subTest(startup=startup), self.assertRaises(ValueError):
                D.validate_native({**record, "startup": startup})
        with self.assertRaises(ValueError):
            D.validate_native({**record, "app": "hermes-desktop"})

    def test_claude_process_observation_is_closed_and_app_scoped(self):
        record = {**native(), "app": "claude-desktop",
                  "nativeProcessObservation": {
                      "state": "matching-process-absent", "everObservedPresent": True}}
        D.validate_native(record)
        for invalid in (
            {"state": "private", "everObservedPresent": True},
            {"state": "query-failed", "everObservedPresent": 1},
            {"state": "matching-process-present", "everObservedPresent": True, "pid": 7},
        ):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                D.validate_native({**record, "nativeProcessObservation": invalid})
        with self.assertRaises(ValueError):
            D.validate_native({**record, "app": "pen-desktop"})

    def test_shared_native_and_installer_fixture(self):
        path = Path(__file__).resolve().parent / "fixtures/desktop-diagnostics.json"
        self.assertEqual(len(D.validate_bundle(path, SHA, "macos")["events"]), 3)

    def test_actual_timeout_preserves_diagnostics_without_leaving_a_running_child(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "diagnostic.json"
            script = "import sys,time; print(" + repr(line(install()).decode().strip()) + ",file=sys.stderr,flush=True); time.sleep(30)"
            with self.assertRaises(D.StageTimeout):
                D.run([sys.executable, "-c", script], output, SHA, "macos", timeout=1, directory=Path(directory))
            self.assertEqual(len(D.validate_bundle(output, SHA, "macos")["events"]), 1)
            self.assertEqual(list(Path(directory).iterdir()), [output])

    def test_closed_events_exclude_unstructured_output(self):
        capture = D.Capture()
        capture.observe(io.BytesIO(b"private prompt and key\n" + line(install()) + line(native(), b"DESKTOP_DIAGNOSTIC:")))
        self.assertEqual([event["kind"] for event in capture.events], ["install", "native"])
        self.assertEqual(capture.invalid, 0)
        self.assertNotIn("private", json.dumps(capture.events))

    def test_unknown_fields_enums_and_types_are_rejected(self):
        for key, value in (("operation", "private"), ("operation", []), ("return_code", True),
                           ("return_code", 2**40), ("app", {}), ("secret", "private")):
            capture = D.Capture()
            record = install()
            record[key] = value
            capture.observe(io.BytesIO(line(record)))
            self.assertEqual(capture.events, [])
            self.assertEqual(capture.invalid, 1)

    def test_windows_exit_codes_are_numeric_not_truncated(self):
        for code in (3221225477, -1073741819):
            D.validate_install({**install(), "return_code": code})

    def test_preparation_events_are_closed_and_have_no_private_inventory(self):
        record = {"schemaVersion": 1, "app": "chatgpt-desktop", "stage": "root-enumeration",
                  "errorCategory": "unreadable", "reason": "installation-unreadable", "osError": 5}
        capture = D.Capture()
        capture.observe(io.BytesIO(line(record, b"DESKTOP_PREPARE_DIAGNOSTIC:")))
        self.assertEqual(capture.events, [{"kind": "prepare", "record": record}])
        for key, value in (("path", "/private"), ("stage", "private stage"), ("errorCategory", "private error"),
                           ("osError", True), ("osError", 2**31)):
            with self.subTest(key=key), self.assertRaises(ValueError):
                D.validate_prepare({**record, key: value})

    def test_transport_events_require_closed_operation_and_observed_status(self):
        record = {"schemaVersion": 1, "app": "claude-desktop", "stage": "frozen-resolution",
                  "errorCategory": "resolution-transport", "reason": "version-unknown",
                  "transportCategory": "http-status", "operation": "artifact", "httpStatus": 403}
        D.validate_prepare(record)
        for update in ({"httpStatus": True}, {"httpStatus": 600}, {"operation": "private-url"},
                       {"transportCategory": "connect"}, {"osError": 5}, {"url": "private"}):
            with self.subTest(update=update), self.assertRaises(ValueError):
                D.validate_prepare({**record, **update})
        without_status = {key: value for key, value in record.items() if key != "httpStatus"}
        with self.assertRaises(ValueError):
            D.validate_prepare(without_status)
        D.validate_prepare({**without_status, "transportCategory": "timeout"})

    def test_version_failure_facts_are_closed_and_observed(self):
        record = {"schemaVersion": 1, "app": "chatgpt-desktop", "source": "runtime-version-command",
                  "failure": "nonzero-exit", "exitCode": 7}
        capture = D.Capture()
        capture.observe(io.BytesIO(line(record, b"DESKTOP_VERSION_DIAGNOSTIC:")))
        self.assertEqual(capture.events, [{"kind": "version", "record": record}])
        for update in ({"path": "/private"}, {"exitCode": True}, {"exitCode": 2**31},
                       {"failure": "timeout"}, {"osError": 5}, {"source": "private"}):
            with self.subTest(update=update), self.assertRaises(ValueError):
                D.validate_version({**record, **update})
        access = {"schemaVersion": 1, "app": "chatgpt-desktop",
                  "source": "runtime-version-command", "failure": "spawn", "osError": 5,
                  "runtimeReadAccess": "other-error"}
        D.validate_version(access)
        for update in ({"app": "pen-desktop"}, {"failure": "wait"}, {"osError": 2},
                       {"runtimeReadAccess": "private"}, {"path": "/private"}):
            with self.subTest(update=update), self.assertRaises(ValueError):
                D.validate_version({**access, **update})

    def test_windows_package_enumeration_failures_remain_closed(self):
        base = {"schemaVersion": 1, "app": "chatgpt-desktop",
                "source": "windows-package-enumeration"}
        for failure, extra in (("spawn", {"osError": 2}),
                               ("nonzero-exit", {"exitCode": -1073741819}),
                               ("encoding", {}),
                               ("invalid-metadata", {})):
            with self.subTest(failure=failure):
                D.validate_version({**base, "failure": failure, **extra})
        with self.assertRaises(ValueError):
            D.validate_version({**base, "failure": "encoding", "exitCode": 1})

    def test_spawn_facts_are_numeric_and_operation_scoped(self):
        record = {**install(), "operation": "npm_ci", "failure": "spawn", "os_error": 2,
                  "win_error": 2, "npm_resolution": "cmd"}
        D.validate_install(record)
        for key, value in (("os_error", True), ("win_error", 2**32), ("npm_resolution", "/private/npm.cmd"),
                           ("failure", "timeout"), ("operation", "pip_install")):
            with self.subTest(key=key), self.assertRaises(ValueError):
                D.validate_install({**record, key: value})

    def test_stop_facts_attribute_each_error_and_reject_private_fields(self):
        stop = {name: {"outcome": "timed-out"} for name in ("initialWait", "graceWait", "kill", "finalWait")}
        stop["kill"] = {"outcome": "failed", "osError": -5}
        record = native()
        record["cleanup"]["stop"] = stop
        D.validate_native(record)
        for key, value in (("osError", True), ("osError", 2**31), ("pid", 42), ("outcome", "private")):
            invalid = {**stop, "kill": {**stop["kill"], key: value}}
            with self.subTest(key=key), self.assertRaises(ValueError):
                D.validate_stop(invalid)
        for operation in ("verify-input-accessibility", "verify-input-visual"):
            record["composer"] = [{"operation": operation, "errorCategory": "input-mismatch"}]
            D.validate_native(record)

    def test_observed_window_stages_and_foreground_relations_remain_closed(self):
        for stage in ("window-inventory-empty", "window-candidates-empty", "window-candidates-too-small"):
            record = native()
            record["guiAcquisition"]["stage"] = stage
            D.validate_native(record)
        for category in ("foreground-process-different", "foreground-window-different", "foreground-identity-unavailable"):
            record = native()
            record["composer"] = [{"operation": "guard", "errorCategory": category,
                                    "guardContext": "before-send"}]
            D.validate_native(record)
        for context in ("reacquisition", "before-input", "before-select-all", "before-type",
                        "before-send", "before-response"):
            record = native()
            record["composer"] = [{"operation": "guard", "errorCategory": "focus-changed",
                                    "guardContext": context}]
            D.validate_native(record)
        invalid_context = native()
        invalid_context["composer"] = [{"operation": "type-text", "errorCategory": "focus-changed",
                                         "guardContext": "before-type"}]
        with self.assertRaises(ValueError):
            D.validate_native(invalid_context)
        record["composer"][0]["errorCategory"] = "private process name"
        with self.assertRaises(ValueError):
            D.validate_native(record)

    def test_window_ownership_causes_remain_distinct_closed_categories(self):
        for category in ("ownership-owner-group-lookup-unavailable",
                         "ownership-candidate-group-lookup-unavailable", "ownership-different-group"):
            record = native()
            record["guiAcquisition"]["errorCategory"] = category
            D.validate_native(record)

    def test_pip_facts_are_closed_bounded_and_operation_scoped(self):
        record = {**install(), "pip_failure_hint": "dependency_resolution",
                  "python_major": 3, "python_minor": 12, "pip_major": 25, "pip_minor": 1}
        capture = D.Capture()
        capture.observe(io.BytesIO(line(record)))
        self.assertEqual(capture.events, [{"kind": "install", "record": record}])
        D.validate_install({**record, "pip_failure_hint": "wheel_build"})
        for key, invalid in (("python_major", True), ("python_minor", 100), ("pip_major", -1),
                             ("pip_minor", "private"), ("pip_failure_hint", "https://private.invalid"),
                             ("operation", "npm_ci"), ("app", "pen-desktop"), ("stage", "artifact")):
            with self.subTest(key=key), self.assertRaises(ValueError):
                D.validate_install({**record, key: invalid})

    def test_duplicate_deep_invalid_utf8_and_oversize_records_are_rejected(self):
        for payload in (b'{"app":1,"app":2}', b"[" * 1500 + b"]" * 1500,
                        b'"\xff"', b"x" * (D.MAX_EVENT + 1)):
            capture = D.Capture()
            capture.observe(io.BytesIO(b"DESKTOP_DIAGNOSTIC:" + payload + b"\n"))
            self.assertEqual(capture.events, [])
            self.assertEqual(capture.invalid, 1)

    def test_log_is_read_with_a_bound_and_event_count_is_bounded(self):
        class BoundedLog(io.BytesIO):
            def read(self, size=-1):
                self.asserted_size = size
                if size != D.MAX_STREAM + 1:
                    raise AssertionError("unbounded read")
                return super().read(size)
        log = BoundedLog(line(install()) * (D.MAX_EVENTS + 2))
        capture = D.Capture()
        capture.observe(log)
        self.assertEqual(log.asserted_size, D.MAX_STREAM + 1)
        self.assertEqual(len(capture.events), D.MAX_EVENTS)
        self.assertEqual(capture.invalid, 2)

    def test_wrapper_reuses_private_executor_and_validates_saved_sidecar(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "diagnostic.json"
            def command(*args, **kwargs):
                kwargs["diagnostic_callback"](io.BytesIO(line(install())))
                return 0
            with patch.object(D, "private_command", side_effect=command) as executor:
                self.assertTrue(D.run(["synthetic"], output, SHA, "windows"))
            self.assertEqual(executor.call_args.kwargs["allow_failure"], True)
            self.assertNotIn("live", executor.call_args.kwargs)
            value = D.validate_bundle(output, SHA, "windows")
            self.assertEqual(len(value["events"]), 1)
            if os.name != "nt":
                self.assertEqual(output.stat().st_mode & 0o777, 0o600)

    def test_cleanup_failure_is_preserved_even_if_sidecar_write_fails(self):
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(D, "private_command", side_effect=D.CleanupError("synthetic")), \
                patch.object(D, "write_json", side_effect=OSError("synthetic")):
            with self.assertRaises(D.CleanupError):
                D.run(["synthetic"], Path(directory) / "out", SHA, "linux")

    def test_actual_noisy_child_cannot_publish_raw_output(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "diagnostic.json"
            script = "import sys; print('private-stdout'); print('private-stderr', file=sys.stderr); print(" + repr(line(install()).decode().strip()) + ", file=sys.stderr); sys.exit(7)"
            self.assertFalse(D.run([sys.executable, "-c", script], output, SHA, "macos", directory=Path(directory)))
            self.assertNotIn("private-", output.read_text())
            self.assertEqual(len(D.validate_bundle(output, SHA, "macos")["events"]), 1)

    def test_sidecar_validation_rejects_unbound_or_injected_fields(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "out"
            D.write_json(output, {"schemaVersion": 1, "sourceSha": SHA, "platform": "linux", "events": [], "invalidEvents": 0})
            with self.assertRaises(ValueError):
                D.validate_bundle(output, "b" * 40, "linux")
            value = json.loads(output.read_text())
            value["raw"] = "private"
            D.write_json(output, value)
            with self.assertRaises(ValueError):
                D.validate_bundle(output, SHA, "linux")


if __name__ == "__main__":
    unittest.main()
