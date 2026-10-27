#!/usr/bin/env python3
"""Canonical MiMo diagnostics retain closed evidence without changing its verdict."""
from copy import deepcopy
import importlib.util
import json
import os
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("mimo_conformance_diagnostic", Path(__file__).resolve().parents[1] / "actions/mimo_conformance_diagnostic.py")
observer = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(observer)


def native_result(second="passed"):
    scenarios = [{"name": name, "status": "skipped" if name == "external-prerequisite" else "passed",
                  "durationMilliseconds": 100, "checks": [{"name": "contract", "status": "passed", "durationMilliseconds": 100}]}
                 for name in observer.SCENARIOS]
    scenarios[1]["checks"].append({"name": "native-configuration", "status": second, "durationMilliseconds": 200})
    scenarios[1]["durationMilliseconds"] = 300
    scenarios[1]["status"] = second
    return {"schemaVersion": 2, "harness": "mimo-code", "scenarios": scenarios,
            "outcome": second, "durationMilliseconds": 500}


class CanonicalConformanceObserverTests(unittest.TestCase):
    def inputs(self, root):
        binary, canary = root / "release-cli.exe", root / "release-canary.exe"
        binary.write_bytes(b"immutable release CLI")
        canary.write_bytes(b"immutable release canary")
        provenance = {"schemaVersion": 1, "controllerSha": "a"*40, "releaseTag": "v0.1.26",
                      "releaseCommit": observer.inventory.RELEASE_COMMIT, "assetRunId": 1,
                      "assetWorkflowSha": "b"*40, "artifactId": 2, "artifactSha256": "c"*64,
                      "manifestSha256": "d"*64, "nanHarnessSha256": observer.inventory.digest(binary),
                      "canarySha256": observer.inventory.digest(canary)}
        receipt, manifest = root / "provenance.json", root / "manifest.json"
        receipt.write_text(json.dumps(provenance))
        manifest.write_text(json.dumps({"harnesses": [{"harness": "mimo-code", "version": "0.1.16",
            "system": "windows", "architecture": "x86_64", "source": "github:XiaomiMiMo/MiMo-Code",
            "package": "", "model": "qwen3.6", "ref": ""}]}))
        return SimpleNamespace(binary=binary, canary=canary, provenance=receipt, manifest=manifest,
                               source_sha="a"*40, directory=root / "cli-cell", output=root / "public.json")

    def collect(self, args, results=None, fail=False, mutate=None):
        calls = []
        results = results if results is not None else [native_result()]

        def install(command, state):
            calls.append(("install", command.directory))
            self.assertNotIn("NAN_API_KEY", os.environ)
            self.assertNotIn("GITHUB_TOKEN", os.environ)
            self.assertEqual(command.directory, args.directory / "mimo-code")
            self.assertEqual(command.harness_version, "0.1.16")
            self.assertEqual(command.binary, args.binary)
            self.assertEqual(command.canary, args.canary)
            state["harness"]["version"] = "0.1.16"
            if mutate == "install": args.canary.write_bytes(b"changed")
            if mutate == "install-failed": raise RuntimeError("PRIVATE install output/path/token")

        def conformance(command, state, observe=None):
            calls.append(("conformance", command.directory))
            self.assertNotIn("NAN_API_KEY", os.environ)
            self.assertNotIn("GITHUB_TOKEN", os.environ)
            self.assertEqual(state["harness"]["version"], "0.1.16")
            for attempt, result in enumerate(results, 1):
                observe(attempt, int(result["outcome"] != "passed"), result)
            if mutate == "conformance": args.binary.write_bytes(b"changed")
            if mutate == "cleanup": raise observer.cell.CleanupError("PRIVATE cleanup")
            if mutate == "missing": return 1
            if fail: raise observer.cell.ConformanceFailure("PRIVATE rejected native contract", len(results))
            return len(results)

        with patch.dict(os.environ, {"GITHUB_SHA": args.source_sha, "RUNNER_TEMP": str(args.directory.parent),
                                     "NAN_API_KEY": "PRIVATE_PROVIDER", "GITHUB_TOKEN": "PRIVATE_READ_TOKEN"}), \
                patch.object(observer.cell, "install", side_effect=install), \
                patch.object(observer.cell, "conformance", side_effect=conformance), \
                patch.object(observer, "prerequisites", return_value={"tools": {}, "gitLocation": "missing", "chosenGitBashExists": False}):
            report = observer.collect(args)
            self.assertEqual(os.environ["NAN_API_KEY"], "PRIVATE_PROVIDER")
            self.assertEqual(os.environ["GITHUB_TOKEN"], "PRIVATE_READ_TOKEN")
        return report, calls

    def test_exact_frozen_cell_functions_and_layout_run_once_without_live_tokens(self):
        with tempfile.TemporaryDirectory() as tmp:
            args = self.inputs(Path(tmp))
            report, calls = self.collect(args)
        self.assertEqual([name for name, _ in calls], ["install", "conformance"])
        self.assertEqual(report["status"], "observed")
        self.assertEqual(report["installStatus"], "passed")
        self.assertEqual(report["conformanceStatus"], "passed")
        self.assertFalse(report["qualification"])
        self.assertTrue(report["diagnosticsOnly"])
        self.assertEqual(report["observerKind"], "canonical-controller")
        self.assertEqual(report["attempts"][0]["process"], {"status": "completed", "exitCode": 0})
        self.assertNotIn("PRIVATE", json.dumps(report))
        self.assertNotIn(tmp, json.dumps(report))

    def test_failed_second_subcheck_survives_both_existing_attempts_as_failure(self):
        with tempfile.TemporaryDirectory() as tmp:
            report, calls = self.collect(self.inputs(Path(tmp)), [native_result("failed"), native_result("failed")], fail=True)
        self.assertEqual([name for name, _ in calls], ["install", "conformance"])
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["reason"], "conformance-failed")
        self.assertEqual(len(report["attempts"]), 2)
        for value in report["attempts"]:
            self.assertEqual(value["process"], {"status": "nonzero-exit", "exitCode": 1})
            self.assertEqual([check["status"] for check in value["checks"]], ["passed", "failed"])

    def test_success_after_existing_second_attempt_retains_first_failed_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            report, calls = self.collect(self.inputs(Path(tmp)), [native_result("failed"), native_result()])
        self.assertEqual(len(calls), 2)
        self.assertEqual(report["status"], "observed")
        self.assertEqual([value["attempt"] for value in report["attempts"]], [1, 2])
        self.assertEqual(report["attempts"][0]["checks"][1]["status"], "failed")

    def test_failed_first_subcheck_is_distinct_from_passing_native_configuration(self):
        value = native_result()
        value["scenarios"][1]["checks"][0]["status"] = "failed"
        value["scenarios"][1]["status"] = "failed"
        value["outcome"] = "failed"
        with tempfile.TemporaryDirectory() as tmp:
            report, _ = self.collect(self.inputs(Path(tmp)), [value, deepcopy(value)], fail=True)
        self.assertEqual(report["status"], "failed")
        self.assertEqual([check["status"] for check in report["attempts"][1]["checks"]], ["failed", "passed"])

    def test_cli_and_canary_hashes_are_both_required_before_and_after_stages(self):
        for phase in ("before-cli", "before-canary", "install", "conformance"):
            with self.subTest(phase=phase), tempfile.TemporaryDirectory() as tmp:
                args = self.inputs(Path(tmp))
                if phase == "before-cli": args.binary.write_bytes(b"changed")
                if phase == "before-canary": args.canary.write_bytes(b"changed")
                report, calls = self.collect(args, mutate=phase)
            self.assertEqual(report["status"], "failed")
            self.assertEqual(report["attempts"], [])
            self.assertEqual(len(calls), {"before-cli": 0, "before-canary": 0, "install": 1, "conformance": 2}[phase])
            self.assertEqual(report["reason"], "provenance-invalid" if phase.startswith("before") else "binding-changed")

    def test_frozen_version_platform_source_and_layout_cannot_drift(self):
        for key, value in (("version", "0.1.17"), ("source", "PRIVATE"), ("architecture", "aarch64"), ("ref", "b"*40)):
            with self.subTest(key=key), tempfile.TemporaryDirectory() as tmp:
                args = self.inputs(Path(tmp))
                document = json.loads(args.manifest.read_text()); document["harnesses"][0][key] = value
                args.manifest.write_text(json.dumps(document))
                report, calls = self.collect(args)
            self.assertEqual(calls, [])
            self.assertEqual(report["reason"], "manifest-invalid")
            self.assertNotIn("PRIVATE", json.dumps(report))
        with tempfile.TemporaryDirectory() as tmp:
            args = self.inputs(Path(tmp)); args.directory = Path(tmp) / "output" / "cli-cell"
            with patch.dict(os.environ, {"GITHUB_SHA": "a"*40, "RUNNER_TEMP": tmp}), patch.object(observer.cell, "install") as install:
                report = observer.collect(args)
            install.assert_not_called()
        self.assertEqual(report["reason"], "layout-invalid")

    def test_private_unknown_payloads_are_stripped_from_projected_records(self):
        value = native_result("failed")
        value["PRIVATE"] = "PRIVATE output/token/path"
        value["scenarios"][1]["PRIVATE"] = "PRIVATE output"
        value["scenarios"][1]["checks"][1]["PRIVATE"] = "PRIVATE credential"
        projected = observer.project_attempt(1, 1, value)
        self.assertNotIn("PRIVATE", json.dumps(projected))
        self.assertEqual([check["name"] for check in projected["checks"]], list(observer.CHECKS))

    def test_malformed_types_check_order_bounds_and_contradictions_are_rejected(self):
        for mutation in ("bool-duration", "status", "name", "order", "count", "scenario", "outcome", "object-status"):
            with self.subTest(mutation=mutation):
                value = native_result()
                checks = value["scenarios"][1]["checks"]
                if mutation == "bool-duration": checks[0]["durationMilliseconds"] = True
                if mutation == "status": checks[0]["status"] = "PRIVATE"
                if mutation == "name": checks[0]["name"] = "PRIVATE"
                if mutation == "order": checks.reverse()
                if mutation == "count": checks.append(deepcopy(checks[0]))
                if mutation == "scenario": value["scenarios"][0]["durationMilliseconds"] = observer.MAX_DURATION + 1
                if mutation == "outcome": value["outcome"] = "failed"
                if mutation == "object-status": checks[0]["status"] = {"passed": None}
                with self.assertRaises(ValueError): observer.project_attempt(1, 0, value)
        for attempt, status in ((True, 0), (3, 0), (1, True), (1, 4294967296), (1, 1)):
            with self.assertRaises(ValueError): observer.project_attempt(attempt, status, native_result())

    def test_malformed_second_observation_keeps_only_valid_first_record_and_stays_failed(self):
        malformed = native_result(); malformed["scenarios"][1]["checks"][0]["status"] = "PRIVATE"
        with tempfile.TemporaryDirectory() as tmp:
            report, _ = self.collect(self.inputs(Path(tmp)), [native_result("failed"), malformed])
        self.assertEqual(report["reason"], "observations-invalid")
        self.assertEqual(report["status"], "failed")
        self.assertEqual(len(report["attempts"]), 1)
        self.assertNotIn("PRIVATE", json.dumps(report))

    def test_install_failure_and_cleanup_refusal_do_not_add_retries_or_hide_evidence(self):
        for phase, reason, calls_expected in (("install-failed", "install-failed", 1), ("cleanup", "cleanup-unproven", 2)):
            with self.subTest(phase=phase), tempfile.TemporaryDirectory() as tmp:
                report, calls = self.collect(self.inputs(Path(tmp)), mutate=phase)
            self.assertEqual(len(calls), calls_expected)
            self.assertEqual(report["status"], "failed")
            self.assertEqual(report["reason"], reason)
            self.assertNotIn("PRIVATE", json.dumps(report))

    def test_environment_facts_only_emit_closed_location_and_git_resolution_categories(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            directory = root / "cli-cell" / "mimo-code"
            git = root / "programs/Git/mingw64/bin/git.exe"
            bash = root / "programs/Git/usr/bin/bash.exe"
            bash.parent.mkdir(parents=True); bash.write_bytes(b"synthetic")
            paths = {"node": str(root / "toolcache/node.exe"), "python": str(root / "home/python.exe"),
                     "git.exe": str(git), "bash.exe": None}
            environment = {"PATH": "PRIVATE", "NAN_HARNESS_GIT_BASH": str(bash), "NAN_CANARY_EXPECTED_NODE_VERSION": "24.22.1"}
            with patch.dict(os.environ, {"RUNNER_TEMP": tmp, "RUNNER_TOOL_CACHE": str(root / "toolcache"),
                                        "USERPROFILE": str(root / "home"), "ProgramFiles": str(root / "programs")}), \
                    patch.object(observer.cell, "cell_environment", return_value=environment), \
                    patch.object(observer.shutil, "which", side_effect=lambda name, **_: paths[name]):
                value = observer.prerequisites(directory)
        self.assertEqual(value["gitLocation"], "mingw-bin")
        self.assertTrue(value["chosenGitBashExists"])
        self.assertEqual(value["tools"]["node"], {"found": True, "location": "runner-toolcache"})
        self.assertEqual(value["tools"]["bash"], {"found": False, "location": "missing"})
        self.assertEqual(value["runtimes"]["node"], "24.22.1")
        self.assertNotIn(tmp, json.dumps(value))
        self.assertNotIn("PRIVATE", json.dumps(value))

    def test_output_is_protected_before_writing_and_removed_if_protection_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            args = self.inputs(Path(tmp))
            argv = ["observer", "--source-sha", args.source_sha]
            for name in ("binary", "canary", "provenance", "manifest", "directory", "output"):
                argv.extend(["--" + name, str(getattr(args, name))])
            def fail_before_write(path):
                self.assertEqual(path.read_bytes(), b"")
                raise RuntimeError("PRIVATE ACL failure")
            with patch.object(observer.sys, "argv", argv), patch.object(observer, "collect", return_value={"status": "observed"}), \
                    patch.object(observer.cell, "protect_private", side_effect=fail_before_write), patch("builtins.print") as output:
                self.assertEqual(observer.main(), 1)
            self.assertFalse(args.output.exists())
            self.assertNotIn("PRIVATE", str(output.call_args))
            args.output.write_text("user-owned")
            with patch.object(observer.sys, "argv", argv), patch.object(observer, "collect") as collect, patch("builtins.print"):
                self.assertEqual(observer.main(), 1)
            collect.assert_not_called()
            self.assertEqual(args.output.read_text(), "user-owned")


if __name__ == "__main__":
    unittest.main()
