#!/usr/bin/env python3
"""Synthetic contracts for the credential-free MiMo inventory observer."""
import importlib.util
import json
import os
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("mimo_inventory_diagnostic", Path(__file__).resolve().parents[1] / "actions/mimo_inventory_diagnostic.py")
observer = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(observer)


def record(index=0):
    stream = {"retainedBytes": 32, "truncated": False, "marker": False}
    return {"index": index, "format": observer.FORMATS[index], "setupError": None,
            "process": {"status": "completed", "exitCode": 0},
            "provider": {"requestCount": 1, "completed": True, "bounded": True, "shutdown": True},
            "capture": {"stdout": dict(stream), "stderr": dict(stream),
                        "events": {key: int(key == "text") for key in observer.EVENTS},
                        "decodedTextMarker": True, "orderedTextMarker": True}}


class InventoryObserverTests(unittest.TestCase):
    def inputs(self, root):
        binary = root / "release.exe"
        binary.write_bytes(b"synthetic immutable release binary")
        provenance = {"schemaVersion": 1, "controllerSha": "a"*40, "releaseTag": "v0.1.26",
                      "releaseCommit": observer.RELEASE_COMMIT, "assetRunId": 1, "assetWorkflowSha": "b"*40,
                      "artifactId": 2, "artifactSha256": "c"*64, "manifestSha256": "d"*64,
                      "nanHarnessSha256": observer.digest(binary), "canarySha256": "e"*64}
        receipt = root / "provenance.json"
        receipt.write_text(json.dumps(provenance))
        output = root / "output"
        output.mkdir()
        return SimpleNamespace(binary=binary, provenance=receipt, source_sha="a"*40, directory=output)

    def collect(self, args, values=None, code=0, outcome="exit", version="0.1.16", mutate=None):
        calls = []
        def run(argv, cwd, env, timeout):
            calls.append((argv, cwd, dict(env), timeout))
            if argv[0] == "pwsh":
                if mutate == "install": args.binary.write_bytes(b"changed")
                if mutate == "installer-failure": return 1, "nonzero"
                return 0, "exit"
            self.assertFalse(Path(env["NAN_MIMO_INVENTORY_OUTPUT"]).exists())
            if values is not None:
                Path(env["NAN_MIMO_INVENTORY_OUTPUT"]).write_text(json.dumps(values))
            if mutate == "observer": args.binary.write_bytes(b"changed")
            if mutate == "receipt":
                changed = json.loads(args.provenance.read_text()); changed["controllerSha"] = "f"*40
                args.provenance.write_text(json.dumps(changed))
            if mutate == "exception": raise RuntimeError("PRIVATE exception with a credential/path")
            return code, outcome
        with patch.dict(os.environ, {"GITHUB_SHA": "a"*40, "GITHUB_TOKEN": "PRIVATE_INSTALL_TOKEN",
                                     "GH_TOKEN": "PRIVATE_GH_TOKEN", "github_token": "PRIVATE_MIXED_TOKEN",
                                     "NAN_API_KEY": "PRIVATE_PROVIDER_KEY", "CARGO_HOME": "retained-cargo-home"}), \
                patch.object(observer.windows, "resolver_module", return_value=object()), \
                patch.object(observer.windows, "resolve_one", return_value=(SimpleNamespace(version=version, ref=""), None)), \
                patch.object(observer.windows, "protect_private"), \
                patch.object(observer.windows, "isolated_environment", return_value={"PATH": "private-installed-path", "HOME": "private-home"}), \
                patch.object(observer.windows, "run_bounded", side_effect=run):
            return observer.collect(args), calls

    def test_version_is_checked_before_any_installer_or_observer(self):
        with tempfile.TemporaryDirectory() as tmp:
            result, calls = self.collect(self.inputs(Path(tmp)), version="0.1.17")
        self.assertEqual(calls, [])
        self.assertEqual(result["reason"], "version-mismatch")
        self.assertFalse(result["qualification"])

    def test_failed_installer_is_not_retried_and_never_starts_cargo(self):
        with tempfile.TemporaryDirectory() as tmp:
            result, calls = self.collect(self.inputs(Path(tmp)), mutate="installer-failure")
        self.assertEqual(len(calls), 1)
        self.assertEqual(result["reason"], "installer-failed")
        self.assertEqual(result["observations"], [])

    def test_controller_environment_source_must_match_before_any_child(self):
        with tempfile.TemporaryDirectory() as tmp:
            args = self.inputs(Path(tmp))
            with patch.dict(os.environ, {"GITHUB_SHA": "b"*40}), patch.object(observer.windows, "run_bounded") as run:
                result = observer.collect(args)
            run.assert_not_called()
        self.assertEqual(result["reason"], "source-invalid")

    def test_exact_invocation_and_credentials_are_limited_to_installer(self):
        with tempfile.TemporaryDirectory() as tmp:
            args = self.inputs(Path(tmp))
            result, calls = self.collect(args, [record(index) for index in range(4)])
        self.assertEqual(result["status"], "observed")
        self.assertIsNone(result["reason"])
        self.assertFalse(result["qualification"])
        self.assertTrue(result["diagnosticsOnly"])
        self.assertEqual(result["observerKind"], "source-test")
        self.assertEqual(len(calls), 2)
        install, cargo = calls
        self.assertEqual(install[3], 180); self.assertEqual(cargo[3], 480)
        self.assertEqual(install[0][install[0].index("-Version")+1], "0.1.16")
        self.assertIn("GITHUB_TOKEN", install[2]); self.assertNotIn("NAN_API_KEY", install[2])
        self.assertEqual(cargo[0], ["cargo", "test", "--locked", "-p", "nan-harness-test-support", "--lib",
            "conformance::runner::inventory_diagnostic::native_mimo_inventory_output", "--", "--ignored", "--exact"])
        self.assertEqual(cargo[2]["CARGO_HOME"], "retained-cargo-home")
        self.assertEqual(cargo[2]["PATH"], "private-installed-path")
        self.assertFalse({key.upper() for key in cargo[2]} & {"GH_TOKEN", "GITHUB_TOKEN", "NAN_API_KEY"})
        self.assertEqual(cargo[2]["NAN_MIMO_INVENTORY_BINARY"], str(args.binary.resolve()))
        self.assertEqual(result["observations"][0]["capture"]["stdout"]["marker"], False)
        self.assertTrue(result["observations"][0]["capture"]["decodedTextMarker"])
        self.assertNotIn("PRIVATE", json.dumps(result))
        self.assertNotIn(str(Path(tmp)), json.dumps(result))

    def test_original_release_binding_prevents_launch_and_changed_binding_discards_evidence(self):
        for phase in ("before", "install", "observer", "receipt"):
            with self.subTest(phase=phase), tempfile.TemporaryDirectory() as tmp:
                args = self.inputs(Path(tmp))
                if phase == "before": args.binary.write_bytes(b"changed")
                result, calls = self.collect(args, [record(i) for i in range(4)], mutate=phase)
            self.assertEqual(result["status"], "failed")
            self.assertEqual(result["observations"], [])
            self.assertEqual(len(calls), {"before": 0, "install": 1, "observer": 2, "receipt": 2}[phase])
            self.assertEqual(result["reason"], "provenance-invalid" if phase == "before" else "binding-changed")

    def test_provenance_and_source_are_exact_and_never_echo_untrusted_values(self):
        for key, invalid in (("controllerSha", "b"*40), ("releaseTag", "v0.1.27"), ("releaseCommit", "f"*40),
                             ("artifactId", True), ("canarySha256", "PRIVATE"), ("path", "PRIVATE")):
            with self.subTest(key=key), tempfile.TemporaryDirectory() as tmp:
                args = self.inputs(Path(tmp)); value = json.loads(args.provenance.read_text()); value[key] = invalid
                args.provenance.write_text(json.dumps(value))
                result, calls = self.collect(args)
            self.assertEqual(calls, []); self.assertEqual(result["reason"], "provenance-invalid")
            self.assertIsNone(result["releaseProvenance"]); self.assertNotIn("PRIVATE", json.dumps(result))
        with tempfile.TemporaryDirectory() as tmp:
            args = self.inputs(Path(tmp)); args.source_sha = "PRIVATE"
            result, calls = self.collect(args)
        self.assertEqual(calls, []); self.assertEqual(result["reason"], "source-invalid")
        self.assertIsNone(result["observerSourceSha"])

    def test_valid_partial_observations_survive_outer_failure_without_invented_cases(self):
        for code, outcome, count, reason in ((None, "timeout", 2, "observer-timeout"),
                (1, "nonzero", 4, "observer-nonzero"), (None, "launch-failed", 0, "observer-launch-failed"),
                (0, "exit", 2, "observations-incomplete")):
            with self.subTest(outcome=outcome, count=count), tempfile.TemporaryDirectory() as tmp:
                result, calls = self.collect(self.inputs(Path(tmp)), [record(i) for i in range(count)], code, outcome)
            self.assertEqual(result["status"], "failed"); self.assertEqual(result["reason"], reason)
            self.assertEqual(len(result["observations"]), count); self.assertEqual(len(calls), 2)
        with tempfile.TemporaryDirectory() as tmp:
            result, _ = self.collect(self.inputs(Path(tmp)), mutate="exception")
        self.assertEqual(result["reason"], "observer-failed"); self.assertNotIn("PRIVATE", json.dumps(result))

    def test_closed_record_schema_rejects_wrong_types_raw_fields_and_unavailable_capture(self):
        changes = [("index", True), ("format", "PRIVATE"), ("setupError", "PRIVATE"), ("output", "PRIVATE"),
                   ("provider", {"requestCount": 129, "completed": True, "bounded": True, "shutdown": True}),
                   ("process", {"status": "completed", "exitCode": True}),
                   ("process", {"status": "completed", "exitCode": None}),
                   ("process", {"status": "timeout", "timeoutMilliseconds": 90000}),
                   ("capture", None)]
        for key, invalid in changes:
            with self.subTest(key=key, invalid=invalid), tempfile.TemporaryDirectory() as tmp:
                value = record(); value[key] = invalid
                result, _ = self.collect(self.inputs(Path(tmp)), [value])
            self.assertEqual(result["reason"], "observations-invalid")
            self.assertEqual(result["observations"], []); self.assertNotIn("PRIVATE", json.dumps(result))
        for key, invalid in (("retainedBytes", 196609), ("retainedBytes", True), ("truncated", 0), ("marker", "PRIVATE")):
            with self.subTest(stream=key), tempfile.TemporaryDirectory() as tmp:
                value = record(); value["capture"]["stdout"][key] = invalid
                path = Path(tmp) / "array.json"; path.write_text(json.dumps([value]))
                with self.assertRaises(ValueError): observer.observations(path, partial=True)
        with tempfile.TemporaryDirectory() as tmp:
            value = record(); value["capture"]["events"]["text"] = True
            path = Path(tmp) / "array.json"; path.write_text(json.dumps([value]))
            with self.assertRaises(ValueError): observer.observations(path, partial=True)

    def test_nullable_setup_and_process_errors_preserve_unknown_capture(self):
        values = []
        for index in range(4):
            value = record(index)
            if index < 2:
                value.update(setupError="workspace" if index == 0 else "provider", process=None, provider=None, capture=None)
            else:
                value.update(process={"status": "timeout", "timeoutMilliseconds": 90000}, capture=None)
            values.append(value)
        with tempfile.TemporaryDirectory() as tmp:
            result, _ = self.collect(self.inputs(Path(tmp)), values)
        self.assertEqual(result["status"], "observed")
        self.assertTrue(all(value["capture"] is None for value in result["observations"]))
        self.assertFalse(result["qualification"])

    def test_file_bound_duplicate_keys_and_four_case_order_are_strict(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "array.json"
            for raw in ('[{"index":0,"index":0}]', 'x'*65537, '[{}]', json.dumps([record(1)])):
                path.write_text(raw)
                with self.assertRaises(ValueError): observer.observations(path)
            path.write_text(json.dumps([record(i) for i in range(4)]))
            self.assertEqual(len(observer.observations(path)), 4)
            path.write_text(json.dumps([record(0)]))
            with self.assertRaises(ValueError): observer.observations(path)
            self.assertEqual(len(observer.observations(path, partial=True)), 1)

    def test_main_creates_only_a_new_private_safe_report_and_refuses_reuse(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            args = self.inputs(root)
            args.directory.rmdir()
            report = {"schemaVersion": 1, "diagnosticsOnly": True, "qualification": False,
                      "observerKind": "source-test", "observerSourceSha": "a"*40, "releaseProvenance": None,
                      "stockVersion": "0.1.16", "status": "failed", "reason": "installer-failed", "observations": []}
            argv = ["observer", "--binary", str(args.binary), "--provenance", str(args.provenance),
                    "--source-sha", args.source_sha, "--directory", str(args.directory)]
            with patch.object(observer.sys, "argv", argv), patch.object(observer, "collect", return_value=report), \
                    patch.object(observer.windows, "protect_private") as protect:
                self.assertEqual(observer.main(), 1)
            destination = args.directory / "mimo-inventory-diagnostic.json"
            self.assertEqual(json.loads(destination.read_text()), report)
            self.assertEqual(protect.call_count, 2)
            before = destination.read_bytes()
            with patch.object(observer.sys, "argv", argv), patch.object(observer, "collect") as collect, \
                    patch("builtins.print") as output:
                self.assertEqual(observer.main(), 1)
            collect.assert_not_called()
            self.assertEqual(destination.read_bytes(), before)
            self.assertNotIn(str(root), str(output.call_args))


if __name__ == "__main__":
    unittest.main()
