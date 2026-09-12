#!/usr/bin/env python3
"""Behavioral contracts for selective, model-scoped compatibility updates."""

import copy
import datetime
import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "actions"))
import hosted

SPEC = "b" * 64


def report():
    return {"nanHarness": {"version": "0.1.6", "sha256": "a" * 64},
            "harness": {"id": "codex", "version": "0.155.0"},
            "environment": {"operatingSystem": "linux", "architecture": "aarch64"},
            "completedAt": "2026-09-12T00:00:00Z", "model": "qwen3.6", "outcome": "passed",
            "checks": [{"name": name, "status": "passed"} for name in
                       ("install-and-diagnose", "deterministic-conformance", "live-tool")]}


def update(value=None):
    return hosted.report_update("cli", json.dumps(value or report()).encode(), SPEC, 1234)


def desktop_split_report(live=False, runtime="0.1.0", app="chatgpt-desktop"):
    probe = {"status": "passed", "steps": ["launched", "input-submitted",
             "response-verified", "tool-verified", "error-recovered"]}
    deterministic = [copy.deepcopy(probe) for _ in range(3)]
    if live:
        deterministic = [{"status": "blocked", "reason": "not-run"} for _ in range(3)]
    return {"schemaVersion": 3, "startedAt":
            "2026-09-12T00:00:01Z" if live else "2026-09-12T00:00:00Z",
            "platform": "linux", "architecture": "x86_64", "model": "qwen3.6",
            "nanHarness": {"version": "0.1.6", "sha256": "a" * 64},
            "cleanup": "passed", "results": [{"app": app, "appVersion": "26.9.0",
                "runtimeVersion": runtime, "deterministic": deterministic,
                "live": copy.deepcopy(probe) if live else {"status": "skipped"},
                "cleanup": "passed"}]}


class HostedEvidenceTests(unittest.TestCase):
    def test_split_desktop_reports_project_each_raw_source_without_merging_reports(self):
        deterministic = json.dumps(desktop_split_report()).encode()
        live = json.dumps(desktop_split_report(live=True)).encode()
        projected = hosted.desktop_batch_update([deterministic, live], SPEC, 1234)
        checks = projected["hostedChecks"]
        self.assertEqual([check["outcome"] for check in checks], ["passed", "passed"])
        self.assertEqual([check["evidenceSha256"] for check in checks], [
            hashlib.sha256(deterministic).hexdigest(), hashlib.sha256(live).hexdigest()])
        self.assertEqual([check["checkedAt"] for check in checks],
                         ["2026-09-12T00:00:00Z", "2026-09-12T00:00:01Z"])
        self.assertEqual(json.dumps(desktop_split_report()).encode(), deterministic)
        self.assertEqual(json.dumps(desktop_split_report(live=True)).encode(), live)

    def test_split_desktop_reports_require_exact_runtime_and_keep_apps_independent(self):
        deterministic = desktop_split_report()
        live = desktop_split_report(live=True, runtime="0.2.0")
        live["results"].append(desktop_split_report(live=True, app="zed-desktop")["results"][0])
        checks = hosted.desktop_batch_checks([deterministic, live], ["b" * 64, "c" * 64], SPEC, 1234)
        by_app = {}
        for check in checks:
            by_app.setdefault(check["id"], []).append(check)
        self.assertEqual([check["outcome"] for check in by_app["chatgpt-desktop"]],
                         ["passed", "blocked", "blocked"])
        self.assertEqual([check["outcome"] for check in by_app["zed-desktop"]],
                         ["blocked", "blocked"])

    def test_split_desktop_live_requires_passing_deterministic_and_clean_reports(self):
        deterministic = desktop_split_report()
        live = desktop_split_report(live=True)
        deterministic["results"][0]["deterministic"][0]["status"] = "failed"
        checks = hosted.desktop_batch_checks([deterministic, live], ["b" * 64, "c" * 64], SPEC, 1234)
        self.assertEqual([check["outcome"] for check in checks], ["blocked", "blocked"])
        deterministic = desktop_split_report()
        live = desktop_split_report(live=True)
        live["cleanup"] = "failed"
        checks = hosted.desktop_batch_checks([deterministic, live], ["b" * 64, "c" * 64], SPEC, 1234)
        self.assertEqual([check["outcome"] for check in checks], ["passed", "blocked"])

    def test_full_desktop_report_keeps_combined_api_compatibility(self):
        value = desktop_split_report(live=True)
        value["results"][0]["deterministic"] = [
            {"status": "passed"}, {"status": "passed"}, {"status": "passed"}]
        checks = hosted.desktop_batch_checks([value], ["b" * 64], SPEC, 1234)
        self.assertEqual([check["outcome"] for check in checks], ["passed", "passed"])

    def test_batch_rejects_legacy_live_model_and_ambiguous_or_time_reversed_inputs(self):
        legacy = desktop_split_report(live=True)
        legacy["schemaVersion"] = 2
        with self.assertRaises(ValueError):
            hosted.desktop_batch_checks([legacy], ["b" * 64], SPEC, 1234)
        deterministic = desktop_split_report()
        duplicate = copy.deepcopy(deterministic)
        with self.assertRaises(ValueError):
            hosted.desktop_batch_checks([deterministic, duplicate], ["b" * 64, "c" * 64], SPEC, 1234)
        live = desktop_split_report(live=True)
        deterministic["startedAt"] = "2026-09-12T00:00:02Z"
        with self.assertRaises(ValueError):
            hosted.desktop_batch_checks([deterministic, live], ["b" * 64, "c" * 64], SPEC, 1234)

    def test_batch_rejects_empty_or_unidentified_reports(self):
        with self.assertRaises(ValueError):
            hosted.desktop_batch_update([], SPEC, 1234)
        value = desktop_split_report()
        value["nanHarness"] = None
        with self.assertRaises(ValueError):
            hosted.desktop_batch_update([json.dumps(value).encode()], SPEC, 1234)
        with self.assertRaises(ValueError):
            hosted.desktop_batch_checks([desktop_split_report()], [], SPEC, 1234)

    def test_live_evidence_keeps_model_and_never_updates_global_legacy_fields(self):
        value = update()
        self.assertEqual(value["verifications"], [])
        self.assertEqual(len(value["hostedChecks"]), 2)
        self.assertNotIn("model", value["hostedChecks"][0])
        self.assertEqual(value["hostedChecks"][1]["model"], "qwen3.6")
        self.assertTrue(all(check["outcome"] == "passed" for check in value["hostedChecks"]))

    def test_failed_live_or_cleanup_never_promotes_a_success(self):
        value = report()
        value["outcome"] = "failed"
        value["failure"] = {"class": "provider"}
        value["checks"][-1]["status"] = "failed"
        self.assertTrue(all(check["outcome"] == "blocked" for check in update(value)["hostedChecks"]))

    def test_live_provider_failure_retains_validated_deterministic_evidence(self):
        value = report()
        value["outcome"] = "infrastructure-failure"
        value["failure"] = {"class": "infrastructure", "phase": "live-tool",
                             "summary": "provider unavailable", "fingerprint": "c" * 64}
        value["checks"][-1]["status"] = "failed"
        checks = update(value)["hostedChecks"]
        self.assertEqual([check["outcome"] for check in checks], ["passed", "blocked"])
        self.assertEqual(checks[1]["model"], "qwen3.6")

    def test_cleanup_failure_blocks_earlier_stages(self):
        value = report()
        value["outcome"] = "infrastructure-failure"
        value["failure"] = {"class": "infrastructure", "phase": "cleanup",
                             "summary": "cleanup was unproven", "fingerprint": "d" * 64}
        value["checks"][-1]["status"] = "failed"
        self.assertTrue(all(check["outcome"] == "blocked" for check in update(value)["hostedChecks"]))

    def test_unknown_installed_version_is_not_a_certification(self):
        value = report()
        value["harness"]["version"] = "unknown"
        self.assertEqual(update(value)["hostedChecks"], [])

    def test_selective_checks_bind_every_identity_dimension(self):
        value = update()
        feed = {"schemaVersion": 5, "releases": [value]}
        target = value["hostedChecks"][1]
        now = hosted.instant("2026-09-12T12:00:00Z")
        self.assertFalse(hosted.should_probe(feed, "0.1.6", target, now))
        for field, replacement in (("model", "another-model"), ("harnessVersion", "0.156.0"),
                                   ("platform", "windows"), ("architecture", "x86_64"),
                                   ("nanHarnessSha256", "d" * 64), ("specSha256", "e" * 64)):
            different = {**target, field: replacement}
            self.assertTrue(hosted.should_probe(feed, "0.1.6", different, now), field)
        self.assertTrue(hosted.should_probe(feed, "0.1.7", target, now))

    def test_infrastructure_retry_is_bounded_and_known_failure_waits_for_change(self):
        value = update()
        target = value["hostedChecks"][1]
        target["outcome"] = "blocked"
        feed = {"schemaVersion": 5, "releases": [value]}
        now = hosted.instant(target["checkedAt"])
        self.assertFalse(hosted.should_probe(feed, "0.1.6", target, now + datetime.timedelta(hours=23)))
        self.assertTrue(hosted.should_probe(feed, "0.1.6", target, now + datetime.timedelta(days=1)))
        target["outcome"] = "failed"
        self.assertFalse(hosted.should_probe(feed, "0.1.6", target, now + datetime.timedelta(days=5)))

    def test_old_desktop_live_reports_cannot_invent_a_model(self):
        probe = {"status": "passed"}
        value = {"schemaVersion": 2, "nanHarness": report()["nanHarness"], "platform": "linux",
                 "architecture": "x86_64", "startedAt": "2026-09-12T00:00:00Z", "cleanup": "passed",
                 "results": [{"app": "zed-desktop", "appVersion": "0.204.0", "cleanup": "passed",
                              "deterministic": [probe] * 3, "live": probe}]}
        with self.assertRaises(ValueError):
            hosted.report_update("desktop", json.dumps(value).encode(), SPEC, 1234)
        value["schemaVersion"], value["model"] = 3, "selected-model"
        result = hosted.report_update("desktop", json.dumps(value).encode(), SPEC, 1234)
        self.assertEqual(result["hostedChecks"][1]["model"], "selected-model")
        bad = copy.deepcopy(value)
        bad["cleanup"] = "failed"
        result = hosted.report_update("desktop", json.dumps(bad).encode(), SPEC, 1234)
        self.assertTrue(all(check["outcome"] == "blocked" for check in result["hostedChecks"]))


if __name__ == "__main__":
    unittest.main()
