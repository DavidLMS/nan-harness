#!/usr/bin/env python3
"""Typed CLI failure classification, partial official resolution and exact installers.

Set NAN_CANARY_VALIDATOR to a built nan-harness-canary binary to also validate the
closed reports with the actual Rust report validator.
"""

import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "actions"))
import cell
import hosted

_SUITE_SPEC = importlib.util.spec_from_file_location(
    "cli_suite", Path(__file__).resolve().parents[1] / "actions/cli-suite.py")
cli_suite = importlib.util.module_from_spec(_SUITE_SPEC)
sys.modules["cli_suite"] = cli_suite
_SUITE_SPEC.loader.exec_module(cli_suite)

COMMIT = "939e45c91d751fadd94dcd1b873ac3cb44846213"
VALIDATOR = os.environ.get("NAN_CANARY_VALIDATOR")
# cell.main routes these to a closed infrastructure report.
RETRYABLE = (OSError, ValueError, RuntimeError)


def conformance_report(harness="codex", failed=(), duration=1_000, **statuses):
    names = ("inventory", "tool-round-trip", "sentinel", "external-prerequisite")
    scenarios = []
    for name in names:
        status = statuses.get(name.replace("-", "_"), "failed" if name in failed else "passed")
        scenarios.append({"name": name, "status": status, "durationMilliseconds": duration,
                          "checks": [{"name": "contract", "status": status,
                                      "durationMilliseconds": duration}]})
    return {"schemaVersion": 2, "harness": harness, "scenarios": scenarios,
            "outcome": "failed" if failed else "passed", "durationMilliseconds": duration}


def cell_state(binary, harness="codex", checks=("install-and-diagnose", "deterministic-conformance"),
               version="1.0.0"):
    return {"schemaVersion": 2, "runId": "run", "cellId": f"linux-{harness}-manual",
            "specSha256": "c" * 64, "trigger": "manual", "tier": "deterministic",
            "scenario": "hosted-clean-install-deterministic-and-live-tool",
            "startedAt": cell.timestamp(), "completedAt": cell.timestamp(), "durationMilliseconds": 0,
            "nanHarness": {"version": "1.2.3", "source": "commit:" + "a" * 40,
                           "sha256": cell.digest(binary)},
            "environment": {"operatingSystem": "linux", "architecture": "aarch64",
                            "image": "github-hosted", "profile": "clean-linux", "runtimes": []},
            "harness": {"id": harness, "version": version},
            "checks": [{"name": name, "status": "passed", "durationMilliseconds": 1, "attempts": 1}
                       for name in checks],
            "outcome": "passed"}


class Cell:
    """A private temporary cell with a synthetic binary and closed state."""

    def __init__(self, root, stage, harness="codex", **state):
        self.root = Path(root)
        binary = self.root / "binary"
        binary.write_bytes(b"synthetic binary")
        self.args = argparse.Namespace(directory=self.root, output=self.root / "report.json",
                                       canary=binary, binary=binary, harness=harness,
                                       model="selected-model", trigger="manual", stage=stage)
        cell.write_json(self.root / "state.json", cell_state(binary, harness, **state))

    def fail(self, error):
        with patch.object(cell, "private_command"):
            cell.failed_report(self.args, error)
        raw = self.args.output.read_bytes()
        if VALIDATOR:
            subprocess.run([VALIDATOR, "validate-report", str(self.args.output)], check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return json.loads(raw), hosted.report_update("cli", raw, "a" * 64, 123)["hostedChecks"]


def scripted_conformance(reports):
    """Replace the canary process with closed reports, one per attempt."""
    pending = list(reports)

    def command(argv, _directory, output=None, **_kwargs):
        value = pending.pop(0)
        if isinstance(value, BaseException):
            raise value
        if value is not None:
            output.write_text(json.dumps(value))
        return 1
    return command


class ConformanceClassificationTests(unittest.TestCase):
    def run_conformance(self, *reports):
        with tempfile.TemporaryDirectory() as temporary:
            state = {"harness": {"id": "codex", "version": "1.0.0"}}
            args = argparse.Namespace(directory=Path(temporary), harness="codex",
                                      canary=Path("canary"), binary=Path("nanh"))
            with patch.object(cell, "private_command", side_effect=scripted_conformance(reports)):
                return cell.conformance(args, state), state

    def test_reproduced_fast_contract_failure_is_not_proof_of_compatibility_mismatch(self):
        with self.assertRaises(RETRYABLE) as failure:
            self.run_conformance(conformance_report(failed=("sentinel", "tool-round-trip")),
                                 conformance_report(failed=("tool-round-trip", "sentinel")))
        self.assertNotIsInstance(failure.exception, cell.CompatibilityMismatch)

    def test_retryable_conformance_results_never_claim_a_mismatch(self):
        # Duration is intentionally irrelevant to mismatch classification.
        slow = 88_000
        cases = {
            "possible wrapper timeout": (conformance_report(failed=("sentinel",), duration=slow),
                                         conformance_report(failed=("sentinel",), duration=slow)),
            "unstable scenario set": (conformance_report(failed=("sentinel",)),
                                      conformance_report(failed=("tool-round-trip",))),
            "no closed report": (None,),
            "foreign harness": (conformance_report(harness="goose"),),
            "unknown status": (conformance_report(sentinel="skipped"),),
            "inconsistent outcome": ({**conformance_report(), "outcome": "failed"},),
            "stage limit": (cell.StageTimeout("stage exceeded its execution limit"),),
        }
        for name, reports in cases.items():
            with self.subTest(name), self.assertRaises(RETRYABLE) as failure:
                self.run_conformance(*reports)
            self.assertNotIsInstance(failure.exception, cell.CompatibilityMismatch)

    def test_kimi_uses_its_shorter_wrapper_budget(self):
        report = conformance_report(harness="kimi-code", failed=("sentinel",), duration=40_000)
        with tempfile.TemporaryDirectory() as temporary:
            args = argparse.Namespace(directory=Path(temporary), harness="kimi-code",
                                      canary=Path("canary"), binary=Path("nanh"))
            with patch.object(cell, "private_command", side_effect=scripted_conformance([report, report])):
                with self.assertRaises(RETRYABLE) as failure:
                    cell.conformance(args, {"harness": {"id": "kimi-code", "version": "1.0.0"}})
        self.assertNotIsInstance(failure.exception, cell.CompatibilityMismatch)

    def test_transient_first_failure_passes_with_its_attempt_count(self):
        attempts, _state = self.run_conformance(conformance_report(failed=("sentinel",)),
                                                conformance_report())
        self.assertEqual(attempts, 2)
        attempts, state = self.run_conformance(conformance_report(failed=("inventory",)))
        self.assertEqual((attempts, state["observations"][0]["kind"]), (1, "inventory-drift"))


class LiveClassificationTests(unittest.TestCase):
    def run_live(self, status, marker):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            args = argparse.Namespace(directory=directory, harness="codex", binary=Path("nanh"),
                                      model="selected-model")
            observed = {}

            def probe(_command, _directory, environment=None, **kwargs):
                path = Path(environment["NAN_CANARY_PROBE_RESULT"])
                observed["inside"] = path.parent == directory and kwargs["live"]
                if marker is not None:
                    path.write_text(marker if isinstance(marker, str) else json.dumps(marker))
                return status
            try:
                with patch.dict(os.environ, {"NAN_API_KEY": "synthetic"}), \
                        patch.object(cell, "private_command", side_effect=probe):
                    return cell.live(args, {})
            finally:
                self.assertTrue(observed["inside"])
                self.assertFalse((directory / "probe-result.json").exists())

    def test_only_a_proven_deterministic_probe_stage_is_a_mismatch(self):
        with self.assertRaises(cell.CompatibilityMismatch) as mismatch:
            self.run_live(1, {"schemaVersion": 1, "stage": "usage-summary", "status": "failed"})
        self.assertEqual(mismatch.exception.code, "live:usage-summary")
        self.assertEqual(self.run_live(0, {"schemaVersion": 1, "stage": "complete", "status": "passed"}), 1)

    def test_provider_auth_tool_and_marker_gaps_remain_retryable(self):
        failed = lambda stage: {"schemaVersion": 1, "stage": stage, "status": "failed"}
        cases = {
            "provider or auth exit": (1, failed("harness-run")),
            "model skipped the tool": (1, failed("tool-evidence")),
            "usage not observed": (1, failed("usage-evidence")),
            "bridge diagnostic": (1, failed("bridge-sentinel")),
            "killed before marker": (1, None),
            "raw output instead of marker": (1, "live probe failed during usage-summary"),
            "open marker fields": (1, {**failed("usage-summary"), "detail": "x"}),
            "success exit without passed marker": (0, failed("usage-summary")),
            "unknown stage": (1, failed("provider-said-mismatch")),
        }
        for name, (status, marker) in cases.items():
            with self.subTest(name), self.assertRaises(RETRYABLE) as failure:
                self.run_live(status, marker)
            self.assertNotIsInstance(failure.exception, cell.CompatibilityMismatch)


class RetainedReportTests(unittest.TestCase):
    def test_typed_conformance_mismatch_is_failed_compatibility(self):
        with tempfile.TemporaryDirectory() as temporary:
            report, checks = Cell(temporary, "conformance", checks=("install-and-diagnose",)).fail(
                cell.CompatibilityMismatch("conformance:sentinel"))
        self.assertEqual((report["failure"]["class"], report["failure"]["code"]), ("harness", "conformance:sentinel"))
        self.assertEqual([check["status"] for check in report["checks"]], ["passed", "failed"])
        self.assertEqual(report["checks"][-1]["attempts"], cell.CONFORMANCE_ATTEMPTS)
        self.assertEqual([check["outcome"] for check in checks], ["failed"])

    def test_live_failures_keep_model_and_earlier_passed_checks(self):
        for error, live_outcome in ((cell.CompatibilityMismatch("live:usage-summary"), "failed"),
                                    (RuntimeError("live probe did not pass"), "blocked"),
                                    (cell.StageTimeout("stage exceeded its execution limit"), "blocked")):
            with self.subTest(type(error).__name__), tempfile.TemporaryDirectory() as temporary:
                report, checks = Cell(temporary, "live").fail(error)
                self.assertEqual(report["model"], "selected-model")
                self.assertEqual([check["status"] for check in report["checks"]],
                                 ["passed", "passed", "failed"])
                self.assertEqual(checks[-1]["model"], "selected-model")
                self.assertEqual(checks[-1]["outcome"], live_outcome)
                if isinstance(error, RuntimeError):
                    self.assertEqual(checks[0]["outcome"], "passed")

    def test_mismatch_outside_evidence_stages_cannot_fail_compatibility(self):
        with tempfile.TemporaryDirectory() as temporary:
            report, _checks = Cell(temporary, "install", checks=()).fail(
                cell.CompatibilityMismatch("conformance:sentinel"))
        self.assertEqual(report["failure"]["class"], "installation")
        self.assertNotIn("code", report["failure"])

    def test_report_stage_failure_after_live_keeps_selected_model(self):
        with tempfile.TemporaryDirectory() as temporary:
            report, _checks = Cell(temporary, "report", checks=(
                "install-and-diagnose", "deterministic-conformance", "live-tool")).fail(RuntimeError())
        self.assertEqual(report["model"], "selected-model")

    def test_unproven_live_cleanup_blocks_deterministic_evidence(self):
        with tempfile.TemporaryDirectory() as temporary:
            report, checks = Cell(temporary, "live").fail(cell.ProbeCleanupError())
        self.assertEqual(report["failure"]["phase"], "cleanup")
        self.assertTrue(checks)
        self.assertTrue(all(check["outcome"] == "blocked" for check in checks))

    def test_unresolved_metadata_report_claims_no_version_or_observation(self):
        with tempfile.TemporaryDirectory() as temporary:
            report, checks = Cell(temporary, "unresolved", checks=(), version="unknown").fail(RuntimeError())
        self.assertEqual(report["harness"]["version"], "unknown")
        self.assertEqual((report["outcome"], report["failure"]["phase"]),
                         ("infrastructure-failure", "resolve-official-version"))
        self.assertEqual(checks, [])


def fake_metadata(unavailable=()):
    documents = {
        "https://registry.npmjs.org/@openai/codex/latest": {"version": "1.2.3"},
        "https://api.github.com/repos/NousResearch/hermes-agent/releases/latest": {"tag_name": "v2026.9.11"},
        "https://api.github.com/repos/NousResearch/hermes-agent/commits/v2026.9.11": {"sha": COMMIT},
        "https://api.github.com/repos/block/goose/releases/latest": {"tag_name": "v1.50.0"},
    }
    texts = {
        f"https://raw.githubusercontent.com/NousResearch/hermes-agent/{COMMIT}/pyproject.toml":
            '[project]\nname = "hermes-agent"\nversion = "0.21.2"\n',
        "https://code.kimi.com/kimi-code/latest": "0.42.0",
    }

    def lookup(table):
        def fetch(url):
            if any(marker in url for marker in unavailable):
                raise OSError("synthetic upstream outage")
            return table[url]
        return fetch
    return {"fetch_json": lookup(documents), "fetch_text": lookup(texts), "fetch_document": lookup(texts)}


class ResolutionTests(unittest.TestCase):
    def test_hermes_freezes_release_commit_and_its_project_version(self):
        resolved, unresolved = cli_suite.resolve_manifest(["hermes"], "windows", "x86_64", "m", **fake_metadata())
        self.assertEqual(unresolved, [])
        self.assertEqual((resolved[0].version, resolved[0].ref), ("0.21.2", COMMIT))

    def test_one_unavailable_upstream_keeps_independent_identities(self):
        resolved, unresolved = cli_suite.resolve_manifest(
            ["codex", "hermes", "kimi-code"], "linux", "aarch64", "m", **fake_metadata(("hermes-agent",)))
        self.assertEqual([item.harness for item in resolved], ["codex", "kimi-code"])
        self.assertEqual([item.as_dict() for item in unresolved], [{
            "harness": "hermes", "system": "linux", "architecture": "aarch64",
            "source": "github:NousResearch/hermes-agent", "package": "", "model": "m"}])
        with self.assertRaises(ValueError):
            cli_suite.resolve_frozen_versions(["codex", "hermes"], "linux", "aarch64", "m",
                                              **fake_metadata(("hermes-agent",)))

    def test_all_unavailable_and_malformed_metadata_invent_no_version(self):
        metadata = fake_metadata(("registry", "api.github", "kimi"))
        resolved, unresolved = cli_suite.resolve_manifest(["codex", "goose", "kimi-code"], "linux",
                                                          "aarch64", "m", **metadata)
        self.assertEqual((resolved, [item.harness for item in unresolved]), ([], ["codex", "goose", "kimi-code"]))
        self.assertFalse(any("version" in item.as_dict() for item in unresolved))
        bad_commit = fake_metadata()
        original = bad_commit["fetch_json"]
        bad_commit["fetch_json"] = lambda url: {"sha": "v2026.9.11"} if "/commits/" in url else original(url)
        self.assertEqual(cli_suite.resolve_manifest(["hermes"], "linux", "aarch64", "m", **bad_commit)[0], [])
        with self.assertRaises(ValueError):
            cli_suite.resolve_manifest(["not-a-harness"], "linux", "aarch64", "m", **fake_metadata())

    def manifest(self, root, document):
        path = Path(root) / "versions.json"
        path.write_text(json.dumps(document))
        return path

    def test_manifest_partitions_the_request_exactly(self):
        resolved, unresolved = cli_suite.resolve_manifest(
            ["codex", "hermes", "kimi-code"], "linux", "aarch64", "m", **fake_metadata(("kimi",)))
        entries = [item.as_dict() for item in resolved]
        missing = [item.as_dict() for item in unresolved]
        with tempfile.TemporaryDirectory() as root:
            valid = self.manifest(root, {"harnesses": entries, "unresolved": missing})
            request = ["codex", "hermes", "kimi-code"]
            self.assertEqual([item.harness for item in cli_suite.read_frozen_manifest(
                valid, request, "linux", "aarch64", "m")], ["codex", "hermes"])
            self.assertEqual([item.harness for item in cli_suite.read_unresolved_manifest(
                valid, request, "linux", "aarch64", "m")], ["kimi-code"])
            invalid = {
                "duplicate": {"harnesses": entries + entries[:1], "unresolved": missing},
                "crossed": {"harnesses": entries, "unresolved": missing + [
                    {key: entries[0][key] for key in missing[0]}]},
                "missing": {"harnesses": entries},
                "reordered": {"harnesses": entries[::-1], "unresolved": missing},
                "unknown field": {"harnesses": entries, "unresolved": missing, "extra": []},
                "unresolved version": {"harnesses": entries, "unresolved": [{**missing[0], "version": "0.42.0"}]},
                "untrusted source": {"harnesses": entries, "unresolved": [{**missing[0], "source": "https://x"}]},
                "hermes branch ref": {"harnesses": [entries[0], {**entries[1], "ref": "v2026.9.11"}],
                                      "unresolved": missing},
                "hermes without ref": {"harnesses": [entries[0], {**entries[1], "ref": ""}], "unresolved": missing},
                "ref on npm harness": {"harnesses": [{**entries[0], "ref": COMMIT}, entries[1]],
                                       "unresolved": missing},
            }
            for name, document in invalid.items():
                with self.subTest(name), self.assertRaises(ValueError):
                    cli_suite.read_frozen_manifest(self.manifest(root, document), request, "linux", "aarch64", "m")
            with self.assertRaises(ValueError):
                cli_suite.read_frozen_manifest(valid, ["codex", "hermes"], "linux", "aarch64", "m")

    def test_selected_manifest_without_unresolved_entries_remains_valid(self):
        resolved, _ = cli_suite.resolve_manifest(["codex"], "linux", "aarch64", "m", **fake_metadata())
        with tempfile.TemporaryDirectory() as root:
            path = self.manifest(root, {"harnesses": [item.as_dict() for item in resolved]})
            self.assertEqual(cli_suite.read_frozen_manifest(path, ["codex"], "linux", "aarch64", "m"), resolved)


class SiblingCellIsolationTests(unittest.TestCase):
    def test_later_cell_cannot_discover_an_earlier_cell_installation(self):
        with tempfile.TemporaryDirectory() as temporary:
            cells = Path(temporary) / "cells"
            first = cell.cell_environment(cells / "codex")
            leaked = os.pathsep.join([first["PATH"], os.environ["PATH"]])
            with patch.dict(os.environ, {"PATH": leaked}):
                second = cell.cell_environment(cells / "goose")
            entries = [Path(entry).resolve() for entry in second["PATH"].split(os.pathsep)]
            self.assertFalse(any(entry.is_relative_to((cells / "codex").resolve()) for entry in entries))
            self.assertTrue(any(entry.is_relative_to((cells / "goose").resolve()) for entry in entries))
            for key in ("HERMES_HOME", "KIMI_INSTALL_DIR", "UV_TOOL_BIN_DIR", "NPM_CONFIG_PREFIX", "LOCALAPPDATA"):
                self.assertTrue(Path(second[key]).is_relative_to(cells / "goose"), key)


class InstallerSelectionTests(unittest.TestCase):
    def test_exact_installer_argv_carries_only_the_frozen_ref(self):
        with patch.object(cell.os, "name", "posix"):
            self.assertEqual(cell.installer_command("hermes", "0.21.2", COMMIT)[2:], ["hermes", "0.21.2", COMMIT])
            self.assertEqual(cell.installer_command("codex", "1.2.3")[2:], ["codex", "1.2.3"])
        with patch.object(cell.os, "name", "nt"):
            command = cell.installer_command("hermes", "0.21.2", COMMIT)
            self.assertEqual(command[-6:], ["-Harness", "hermes", "-Version", "0.21.2", "-Ref", COMMIT])
            self.assertNotIn("-Ref", cell.installer_command("goose", "1.50.0"))

    def test_suite_driver_reports_unresolved_cells_and_passes_frozen_refs(self):
        resolved, unresolved = cli_suite.resolve_manifest(
            ["hermes", "kimi-code"], "linux", "aarch64", "model-x", **fake_metadata(("kimi",)))
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            log = root / "argv.log"
            fake = root / "fake-cell.py"
            fake.write_text("import json, os, sys\n"
                            "open(os.environ['ARGV_LOG'], 'a').write(json.dumps(sys.argv[1:]) + '\\n')\n"
                            "raise SystemExit(1 if sys.argv[1] == 'unresolved' else 0)\n")
            manifest = root / "versions.json"
            manifest.write_text(json.dumps({"harnesses": [item.as_dict() for item in resolved],
                                            "unresolved": [item.as_dict() for item in unresolved]}))
            command = [sys.executable, str(Path(__file__).resolve().parents[1] / "actions/cli-suite.py"),
                       "--harnesses", "hermes,kimi-code", "--trigger", "manual", "--tag", "v1.2.3",
                       "--model", "model-x", "--system", "linux", "--architecture", "aarch64",
                       "--source-kind", "branch", "--source-sha", "a" * 40, "--nan-version", "1.2.3",
                       "--binary", str(root / "nanh"), "--canary", str(root / "canary"),
                       "--directory", str(root / "cells"), "--output", str(root / "reports"),
                       "--run-id", "run", "--manifest", str(manifest), "--cell-script", str(fake)]
            result = subprocess.run(command, env=dict(os.environ, ARGV_LOG=str(log)),
                                    capture_output=True, check=False)
            calls = [json.loads(line) for line in log.read_text().splitlines()]
        self.assertEqual(result.returncode, 1)
        self.assertEqual([call[0] for call in calls], ["install", "conformance", "report", "unresolved"])
        install = calls[0]
        self.assertEqual(install[install.index("--harness-ref") + 1], COMMIT)
        self.assertEqual(install[install.index("--harness-version") + 1], "0.21.2")
        self.assertNotIn("--harness-version", calls[-1])


if __name__ == "__main__":
    unittest.main()
