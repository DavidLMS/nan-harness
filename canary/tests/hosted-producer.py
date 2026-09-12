#!/usr/bin/env python3
"""Producer and ingress agree on exact native release evidence."""

import datetime
import copy
import io
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "actions"))
import evidence
import hosted
import hosted_publication
from state import StateError, canonical

COMMIT = "a" * 40
RELEASE = "b" * 40
SPEC = "c" * 64
BINARY = "d" * 64


def report():
    return {"nanHarness": {"version": "1.2.3", "sha256": BINARY, "source": "commit:" + RELEASE},
            "harness": {"id": "codex", "version": "2.0.0"},
            "environment": {"operatingSystem": "linux", "architecture": "aarch64"},
            "checks": [{"name": name, "status": "passed"} for name in
                       ("install-and-diagnose", "deterministic-conformance", "live-tool")],
            "model": "selected-model", "outcome": "passed", "completedAt": "2026-09-12T00:00:00Z"}


def desktop_report(platform="linux", architecture="x86_64"):
    probe = {"status": "passed", "steps": ["launched", "input-submitted", "response-verified",
                                              "tool-verified", "error-recovered"],
             "inputMode": "accessibility", "responseVerification": "accessibility",
             "durationMilliseconds": 1}
    return {"schemaVersion": 3, "checkerVersion": "0.1.0", "runId": "a" * 32,
            "startedAt": "2026-09-12T00:00:00Z", "platform": platform, "architecture": architecture,
            "model": "selected-model", "nanHarness": {"version": "1.2.3", "sha256": BINARY},
            "cleanup": "passed", "results": [{"app": "chatgpt-desktop", "appVersion": "26.9.0",
                "runtimeVersion": "0.1.0", "deterministic": [copy.deepcopy(probe) for _ in range(3)],
                "live": {**probe, "steps": probe["steps"][:-1]}, "cleanup": "passed"}]}


class ProducerTests(unittest.TestCase):
    def test_cli_selector_preserves_unresolved_entries_in_a_consumable_manifest(self):
        resolved = {"harness": "codex", "version": "2.0.0", "system": "linux",
                    "architecture": "aarch64", "source": "npm:@openai/codex",
                    "package": "@openai/codex", "model": "selected-model", "ref": ""}
        unresolved = {"harness": "hermes", "system": "linux", "architecture": "aarch64",
                      "source": "github:NousResearch/hermes-agent", "package": "", "model": "selected-model"}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest, output, feed = (root / name for name in ("manifest.json", "selected.json", "feed.json"))
            manifest.write_bytes(canonical({"harnesses": [resolved], "unresolved": [unresolved]}))
            feed.write_bytes(canonical({"schemaVersion": 5, "releases": []}))
            argv = ["evidence.py", "select-cli", "--manifest", str(manifest), "--feed", str(feed),
                    "--feed-validator", "validator", "--harnesses", "codex,hermes", "--mode", "live",
                    "--platform", "linux", "--architecture", "aarch64", "--model", "selected-model",
                    "--source-commit", COMMIT, "--release-tag", "v1.2.3", "--binary", "binary",
                    "--output", str(output)]
            with mock.patch.object(sys, "argv", argv), mock.patch.object(evidence, "validate"), \
                    mock.patch.object(evidence, "digest", return_value=BINARY), \
                    mock.patch.object(evidence, "specification_digest", return_value=SPEC), \
                    mock.patch("sys.stdout", new_callable=io.StringIO) as stdout:
                self.assertEqual(evidence.main(), 0)
            self.assertEqual(stdout.getvalue().strip(), "codex,hermes")
            self.assertEqual(json.loads(output.read_bytes()), {"harnesses": [resolved], "unresolved": [unresolved]})
            module = sys.modules["cli_suite"]
            self.assertEqual(module.read_unresolved_manifest(output, ["codex", "hermes"],
                "linux", "aarch64", "selected-model")[0].as_dict(), unresolved)

    def test_desktop_envelopes_preserve_native_model_and_runtime_without_private_manifests(self):
        for platform, architecture in (("linux", "x86_64"), ("macos", "aarch64"), ("windows", "x86_64")):
            value = desktop_report(platform, architecture)
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "report.json"
                path.write_bytes(canonical(value))
                bundle = evidence.pack_reports([path], "checker", "desktop", platform, architecture,
                    COMMIT, "v1.2.3", RELEASE, "selected-model", BINARY, SPEC, lambda _: None)
            self.assertEqual(hosted_publication.validate_bundle(bundle, COMMIT)["architecture"], architecture)
            projected = hosted.report_update("desktop", canonical(bundle["reports"][0]), SPEC, 123)
            self.assertEqual(projected["hostedChecks"][1]["model"], "selected-model")
            self.assertEqual(projected["hostedChecks"][1]["runtimeVersion"], "0.1.0")
            self.assertNotIn("apps", bundle)
            self.assertNotIn("url", canonical(bundle).decode())

    def test_desktop_selection_never_borrows_a_historical_runtime_or_discards_unresolved_apps(self):
        frozen = {"schemaVersion": 1, "suite": "desktop", "platform": "linux", "architecture": "x86_64",
                  "model": "selected-model", "apps": [{"app": "chatgpt-desktop", "status": "frozen",
                                                          "version": "26.9.0", "runtimeVersion": "0.1.0"}]}
        update = hosted.report_update("desktop", canonical(desktop_report()), SPEC, 123)
        feed = {"schemaVersion": 5, "releases": [update]}
        now = hosted.instant("2026-09-12T12:00:00Z")
        def selected(manifest=frozen, when=now):
            return evidence.pending_desktop(feed, manifest, "1.2.3", BINARY, SPEC, "live", when)
        self.assertEqual(selected(), [])
        for field, replacement in (("version", "26.10.0"), ("runtimeVersion", "0.2.0")):
            changed = copy.deepcopy(frozen)
            changed["apps"][0][field] = replacement
            self.assertEqual(selected(changed), changed["apps"])
        unknown = copy.deepcopy(frozen)
        del unknown["apps"][0]["runtimeVersion"]
        self.assertEqual(selected(unknown), unknown["apps"])
        unresolved = {"app": "pen-desktop", "status": "blocked", "reason": "resolution-failed"}
        self.assertEqual(selected({**frozen, "apps": frozen["apps"] + [unresolved]}), [unresolved])
        update["hostedChecks"][-1]["outcome"] = "blocked"
        self.assertEqual(selected(), [])
        self.assertEqual(selected(when=now + datetime.timedelta(days=1)), frozen["apps"])

    def test_desktop_runtime_prerelease_is_preserved_as_an_exact_identity(self):
        value = desktop_report()
        runtime = "0.154.0-alpha.6.2"
        value["results"][0]["runtimeVersion"] = runtime
        update = hosted.report_update("desktop", canonical(value), SPEC, 123)
        self.assertEqual([check["runtimeVersion"] for check in update["hostedChecks"]], [runtime, runtime])
        manifest = {"platform": "linux", "architecture": "x86_64", "model": "selected-model",
                    "apps": [{"app": "chatgpt-desktop", "status": "frozen", "version": "26.9.0",
                              "runtimeVersion": runtime}]}
        self.assertEqual(evidence.pending_desktop({"schemaVersion": 5, "releases": [update]}, manifest,
            "1.2.3", BINARY, SPEC, "live", hosted.instant("2026-09-12T12:00:00Z")), [])

    def pack(self, value, command=lambda _: None):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.json"
            path.write_bytes(canonical(value))
            return evidence.pack_reports([path], "validator", "cli", "linux", "aarch64",
                                         COMMIT, "v1.2.3", RELEASE, "selected-model", BINARY, SPEC, command)

    def test_data_only_envelope_round_trips_through_actual_ingress(self):
        observed = []
        def validate(argv):
            self.assertEqual(argv[:2], ["validator", "validate-report"])
            observed.append(json.loads(Path(argv[-1]).read_bytes()))
        value = self.pack(report(), validate)
        self.assertEqual(observed, [report()])
        archive = io.BytesIO()
        with zipfile.ZipFile(archive, "w") as output:
            output.writestr("evidence.json", canonical(value))
        received = hosted_publication.artifact_bundle(archive.getvalue())
        self.assertEqual(received, value)
        self.assertEqual(hosted_publication.validate_bundle(received, COMMIT)["architecture"], "aarch64")

    def test_report_identity_drift_and_validator_failure_cannot_produce_an_envelope(self):
        for section, field, replacement in (("nanHarness", "sha256", "e" * 64),
                                             ("nanHarness", "source", "commit:" + COMMIT),
                                             ("environment", "architecture", "x86_64")):
            value = report()
            value[section][field] = replacement
            with self.assertRaises(StateError):
                self.pack(value)
        value = report()
        value["model"] = "another-model"
        with self.assertRaises(StateError):
            self.pack(value)
        def reject(_argv):
            raise StateError("invalid report")
        with self.assertRaises(StateError):
            self.pack(report(), reject)

    def test_selective_driver_requires_both_deterministic_and_selected_live_identity(self):
        item = SimpleNamespace(harness="codex", version="2.0.0", system="linux",
                               architecture="aarch64", model="selected-model")
        updates = hosted.report_update("cli", canonical(report()), SPEC, 123)
        feed = {"schemaVersion": 5, "releases": [updates]}
        now = hosted.instant("2026-09-12T12:00:00Z")
        self.assertEqual(evidence.pending_harnesses(feed, [item], "1.2.3", BINARY, SPEC, "live", now), [])
        item.model = "another-model"
        self.assertEqual(evidence.pending_harnesses(feed, [item], "1.2.3", BINARY, SPEC, "live", now), [item])
        self.assertEqual(evidence.pending_harnesses(feed, [item], "1.2.3", BINARY, SPEC, "deterministic", now), [])
        item.model = "selected-model"
        updates["hostedChecks"][-1]["outcome"] = "blocked"
        self.assertEqual(evidence.pending_harnesses(feed, [item], "1.2.3", BINARY, SPEC, "live", now), [])
        self.assertEqual(evidence.pending_harnesses(feed, [item], "1.2.3", BINARY, SPEC, "live",
                                                  now + datetime.timedelta(days=1)), [item])


if __name__ == "__main__":
    unittest.main()
