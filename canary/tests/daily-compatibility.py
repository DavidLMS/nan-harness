#!/usr/bin/env python3
"""Offline selection, provenance and real feed publication contracts."""

import copy
import datetime
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import unittest
from unittest.mock import patch
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "canary/actions"))
import daily_evidence as evidence
import daily_compatibility as daily
from selection import PLATFORM_ASSETS, PLATFORMS, supported_platforms


def fixture():
    started = datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(minutes=2)
    completed = started + datetime.timedelta(seconds=1)
    release = {"tag": "v1.2.3", "version": "1.2.3", "commit": "a" * 40,
               "digests": {name: "b" * 64 for pair in PLATFORM_ASSETS.values() for name in pair.values()}}
    plan = {"runId": "123-1", "model": "qwen3.6", "specSha256": "c" * 64,
            "startedAt": started.isoformat(), "cells": [], "releases": [release], "results": []}
    reports = {}
    for harness in ("codex", "claude-code"):
        for system in supported_platforms(harness):
            name = f"v1.2.3-{system}-{harness}.json"
            cell = {"tag": release["tag"], "system": system, "harness": harness,
                    "version": "9999.0.0", "report": name}
            plan["cells"].append(cell)
            reports[name] = {
                "schemaVersion": 2, "runId": plan["runId"], "cellId": system + "-" + harness,
                "specSha256": plan["specSha256"], "trigger": "daily", "tier": "live-core",
                "scenario": "synthetic-daily", "startedAt": started.isoformat(),
                "completedAt": completed.isoformat(), "durationMilliseconds": 1000,
                "nanHarness": {"version": release["version"], "source": "commit:" + release["commit"],
                               "sha256": "b" * 64},
                "environment": {"operatingSystem": system, "architecture": PLATFORMS[system]["architecture"],
                                "image": "synthetic", "profile": "daily", "runtimes": []},
                "harness": {"id": harness, "version": cell["version"]}, "model": plan["model"],
                "checks": [{"name": n, "status": "passed", "durationMilliseconds": 1, "attempts": 1}
                           for n in evidence.CHECKS], "outcome": "passed"}
    return plan, release, reports


class DailyEvidenceTests(unittest.TestCase):
    def test_spec_digest_survives_windows_checkout(self):
        with tempfile.TemporaryDirectory() as tmp:
            checkout = Path(tmp)
            env = dict(os.environ, GIT_INDEX_FILE=str(checkout / "index"))
            subprocess.run(["git", "read-tree", "HEAD"], cwd=ROOT, env=env, check=True)
            source = "canary/actions/cell.py"
            # Exercise checkout conversion of the current source, including
            # uncommitted edits, without touching the user's real index.
            blob = subprocess.run(["git", "hash-object", "-w", source], cwd=ROOT,
                                  check=True, capture_output=True, text=True).stdout.strip()
            subprocess.run(["git", "update-index", "--cacheinfo", "100644", blob, source],
                           cwd=ROOT, env=env, check=True)
            subprocess.run(["git", "-c", "core.autocrlf=true", "checkout-index",
                            "--prefix=" + str(checkout) + "/", source],
                           cwd=ROOT, env=env, check=True)
            self.assertEqual(daily.digest(checkout / source), daily.digest(ROOT / source))

    def test_channels_are_deduplicated_and_must_be_stable(self):
        self.assertEqual(evidence.release_tags({"version": "1.2.3"}, {"tag_name": "v1.2.3"}), ["v1.2.3"])
        self.assertEqual(evidence.release_tags({"version": "1.3.0"}, {"tag_name": "v1.2.3"}),
                         ["v1.3.0", "v1.2.3"])
        for version in ("1.2.3-rc.1", "01.2.3", "../../x"):
            with self.assertRaises(ValueError):
                evidence.release_tags({"version": version}, {"tag_name": "v1.2.3"})

    def test_pending_uses_release_scoped_live_semver_evidence(self):
        entry = {"id": "codex", "lastCompatibleVersion": "1.0.0", "lastLiveVerifiedVersion": "1.0.0-rc.9"}
        feed = {"releases": [{"nanHarnessVersion": "1.2.3", "verifications": [entry]}]}
        self.assertTrue(evidence.pending(feed, "1.2.3", "codex", "1.0.0-rc.10"))
        self.assertFalse(evidence.pending(feed, "1.2.3", "codex", "1.0.0-rc.8"))
        self.assertTrue(evidence.pending(feed, "1.2.3", "codex", "1.0.0"))
        entry["lastLiveVerifiedVersion"] = "1.0.0+old"
        self.assertFalse(evidence.pending(feed, "1.2.3", "codex", "1.0.0+new"))
        self.assertTrue(evidence.pending(feed, "1.2.3", "codex", "1.0.0", True))
        self.assertTrue(evidence.pending(feed, "1.2.4", "codex", "1.0.0"))
        entry.pop("lastLiveVerifiedVersion")
        self.assertTrue(evidence.pending(feed, "1.2.3", "codex", "1.0.0"))

    def collect(self, plan, release, reports):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name, report in reports.items():
                (root / name).write_text(json.dumps(report))
            return evidence.collect_release(plan, release, root, lambda path: None)

    def test_windows_failure_or_missing_report_only_blocks_its_harness(self):
        for missing in (False, True):
            plan, release, reports = fixture()
            name = "v1.2.3-windows-claude-code.json"
            if missing:
                del reports[name]
            else:
                reports[name]["outcome"] = "infrastructure-failure"
            updates, results = self.collect(plan, release, reports)
            self.assertEqual([u["id"] for u in updates], ["codex"])
            self.assertIn({"tag": "v1.2.3", "harness": "claude-code", "status": "pending"}, results)
            # There is no persistent failure cache: a fresh complete retry qualifies both.
            retry_plan, retry_release, retry_reports = fixture()
            self.assertEqual(len(self.collect(retry_plan, retry_release, retry_reports)[0]), 2)

    def test_provenance_and_incomplete_live_contract_reject_entire_release(self):
        changes = [
            ("runId", "other-run"), ("specSha256", "d" * 64), ("trigger", "release"),
            ("tier", "deterministic"), ("model", "other"), ("checks", []),
            ("completedAt", "2099-01-01T00:00:00Z"),
            ("nanHarness", {"version": "1.2.4", "source": "commit:" + "a" * 40, "sha256": "b" * 64}),
            ("harness", {"id": "codex", "version": "9998.0.0"}),
            ("environment", {"operatingSystem": "windows", "architecture": "aarch64"}),
            ("nanHarness", {"version": "1.2.3", "source": "commit:" + "a" * 40, "sha256": "d" * 64}),
        ]
        for field, value in changes:
            with self.subTest(field=field):
                plan, release, reports = fixture()
                reports["v1.2.3-windows-codex.json"][field] = value
                with self.assertRaises(ValueError):
                    self.collect(plan, release, reports)
        plan, release, reports = fixture()
        reports["v1.2.3-duplicate.json"] = copy.deepcopy(next(iter(reports.values())))
        with self.assertRaises(ValueError):
            self.collect(plan, release, reports)

    def test_platform_selection_must_be_complete_and_versions_must_agree(self):
        plan, release, reports = fixture()
        plan["cells"][0]["version"] = "9998.0.0"
        with self.assertRaises(ValueError):
            self.collect(plan, release, reports)
        plan, release, reports = fixture()
        cell = plan["cells"].pop()
        del reports[cell["report"]]
        with self.assertRaises(ValueError):
            self.collect(plan, release, reports)

    def test_resolver_failure_does_not_discard_independent_selection(self):
        plan, release, _ = fixture()
        plan["cells"] = []
        versions = {system: {} for system in PLATFORMS}
        for system in PLATFORMS:
            versions[system]["codex"] = {"harness": "codex", "system": system, "version": "1.0.0"}
        daily.select_cells(plan, release, versions, {"releases": []}, False)
        self.assertEqual(len(plan["cells"]), 3)
        self.assertTrue(all(c["harness"] == "codex" for c in plan["cells"]))
        self.assertTrue(any(r["status"] == "unresolved" for r in plan["results"]))

    def test_signed_asset_verification_requires_exact_commit(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(daily, "release_identity", return_value="a" * 40), \
                patch.object(daily, "download"), patch.object(daily, "gh") as gh, \
                patch.object(daily, "_asset_entries", return_value=([], "b" * 64)):
            daily.release_assets("Acme/Fork", "v1.2.3", Path(tmp))
            call = gh.call_args.args
            self.assertIn("--deny-self-hosted-runners", call)
            self.assertEqual(call[call.index("--source-digest") + 1], "a" * 40)
            self.assertEqual(call[call.index("--source-ref") + 1], "refs/tags/v1.2.3")

    def test_reports_pass_the_real_typed_validator(self):
        plan, release, reports = fixture()
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            for name, report in reports.items():
                (directory / name).write_text(json.dumps(report))
            validator = ROOT / "target/debug/nan-harness-canary"
            updates, _ = evidence.collect_release(plan, release, directory,
                lambda path: subprocess.run([str(validator), "validate-report", str(path)],
                                            check=True, capture_output=True))
            self.assertEqual(len(updates), 2)

    def test_prepare_keeps_other_release_when_assets_are_missing(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            args = SimpleNamespace(directory=directory, repository="Acme/Fork", run_id="123-1",
                                   workflow_commit="a" * 40, force=False)
            def download(*call):
                call[-1].write_text(json.dumps({"version": "1.3.0"}))
            _, release, _ = fixture()
            with patch.object(daily, "download", side_effect=download), \
                    patch.object(daily, "gh", return_value=b'{"tag_name":"v1.2.3"}'), \
                    patch.object(daily, "read_feed", return_value={"releases": []}), \
                    patch.object(daily, "frozen_versions", return_value={s: {} for s in PLATFORMS}), \
                    patch.object(daily, "release_assets", side_effect=[ValueError("missing"), release]), \
                    patch.dict(os.environ, {"GITHUB_OUTPUT": str(directory / "outputs")}):
                daily.prepare(args)
            plan = json.loads((directory / "plan.json").read_bytes())
            self.assertEqual(plan["releases"], [release])
            self.assertEqual(plan["results"][0]["status"], "assets-unavailable")

    def test_no_pending_versions_selects_no_cells(self):
        plan, release, _ = fixture()
        plan["cells"] = []
        versions = {s: {h: {"system": s, "harness": h, "version": "9999.0.0"}
                        for h in daily.CLI_HARNESSES if s in supported_platforms(h)} for s in PLATFORMS}
        feed = {"releases": [{"nanHarnessVersion": release["version"], "verifications": [
            {"id": h, "lastLiveVerifiedVersion": "9999.0.0"} for h in daily.CLI_HARNESSES]}]}
        daily.select_cells(plan, release, versions, feed, False)
        self.assertEqual(plan["cells"], [])
        self.assertEqual({r["status"] for r in plan["results"]}, {"current"})

    def test_aggregate_publishes_independent_success_then_reports_failure(self):
        plan, release, reports = fixture()
        del reports["v1.2.3-windows-claude-code.json"]
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            for name, report in reports.items():
                (directory / name).write_text(json.dumps(report))
            args = SimpleNamespace(directory=directory, reports=directory, repository="Acme/Fork",
                                   validator="fixture-validator", publish=True)
            with patch.object(daily, "load_plan", return_value=plan), \
                    patch.object(daily, "release_identity", return_value=release["commit"]), \
                    patch.object(daily, "command"), patch.object(daily, "publish_release") as publish, \
                    patch.dict(os.environ, {"GITHUB_STEP_SUMMARY": str(directory / "summary.md")}):
                self.assertEqual(daily.aggregate(args), 1)
            self.assertEqual([u["id"] for u in publish.call_args.args[2]], ["codex"])
            result = json.loads((directory / "summary.json").read_bytes())
            self.assertEqual({r["status"] for r in result}, {"published", "pending"})

    def test_workflow_schedule_is_serialized_and_publication_is_isolated(self):
        workflow = (ROOT / ".github/workflows/harness-canary.yml").read_text()
        self.assertIn('cron: "0 5 * * *"\n      timezone: Europe/Madrid', workflow)
        self.assertIn("group: release-channel-${{ github.repository }}", workflow)
        self.assertIn("cancel-in-progress: false", workflow)
        self.assertIn("fail-fast: false", workflow)
        self.assertEqual(workflow.count("contents: write"), 1)
        self.assertEqual(workflow.count("NAN_API_KEY:"), 1)
        self.assertIn("github.event_name == 'schedule' || !inputs.verification_only", workflow)
        self.assertIn("always() && !cancelled() && needs.prepare.result == 'success'", workflow)
        self.assertNotIn("run-source-main-detector", workflow)
        self.assertFalse((ROOT / ".github/scripts/run-source-main-detector.sh").exists())


class DailyPublicationTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        spec = importlib.util.spec_from_file_location("publication_fixture", ROOT / "canary/tests/release-publish-integration.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        self.remote = self.root / "remote"
        self.assets = self.remote / "releases/compatibility/assets"
        self.assets.mkdir(parents=True)
        (self.assets.parent / "state").write_text("prerelease")
        bin_dir = self.root / "bin"
        bin_dir.mkdir()
        (bin_dir / "gh").write_text(module.FAKE_GH)
        (bin_dir / "gh").chmod(0o755)
        self.env = {**os.environ, "PATH": str(bin_dir) + os.pathsep + os.environ["PATH"],
                    "FAKE_GH_STATE": str(self.remote), "NAN_CANARY_RETRY_DELAY_SECONDS": "0"}
        self.version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
        self.updates = self.root / "updates"
        self.updates.mkdir()
        self.entry = {"id": "codex", "lastCompatibleVersion": "9998.0.0",
                      "compatibleAt": "2026-09-20T00:00:00Z", "lastLiveVerifiedVersion": "9998.0.0",
                      "liveVerifiedAt": "2026-09-20T00:00:00Z"}
        self.base = {"schemaVersion": 3, "releases": [
            {"nanHarnessVersion": self.version, "verifications": [self.entry]},
            {"nanHarnessVersion": "0.0.1", "verifications": [self.entry]}]}
        for schema, name in ((2, "compatibility.json"), (3, "compatibility-v3.json")):
            feed = {**self.base, "schemaVersion": schema}
            (self.assets / name).write_text(json.dumps(feed))
        update = {**self.entry, "nanHarnessVersion": self.version,
                  "lastCompatibleVersion": "9999.0.0", "lastLiveVerifiedVersion": "9999.0.0",
                  "compatibleAt": "2026-09-21T00:00:00Z", "liveVerifiedAt": "2026-09-21T00:00:00Z"}
        (self.updates / "codex.json").write_text(json.dumps(update))

    def run_publisher(self, publish=False, **env):
        output = self.root / ("output-" + str(len(list(self.root.glob("output-*")))))
        args = ["bash", str(ROOT / "canary/host/publish-compatibility.sh"), "--trigger", "daily",
                "--nan-harness-version", self.version, "--release-tag", "v" + self.version,
                "--reports", str(self.root), "--output-dir", str(output), "--state-dir", str(self.root / "state"),
                "--report-validator", "/usr/bin/true", "--repository", "Acme/Fork",
                "--verified-updates", str(self.updates)]
        if publish:
            args.append("--publish-feed")
        return subprocess.run(args, env={**self.env, **env}, cwd=ROOT, text=True, capture_output=True), output

    def calls(self):
        return [json.loads(line) for line in (self.remote / "calls.jsonl").read_text().splitlines()]

    def test_partial_publication_preserves_unobserved_history_and_absent_desktop(self):
        result, _ = self.run_publisher(True)
        self.assertEqual(result.returncode, 0, result.stderr)
        for name in ("compatibility.json", "compatibility-v3.json"):
            feed = json.loads((self.assets / name).read_bytes())
            self.assertEqual(feed["releases"][1], self.base["releases"][1])
            current = feed["releases"][0]
            self.assertNotIn("desktopVerifications", current)
            self.assertEqual(len(current["verifications"]), 1)
            self.assertEqual(current["verifications"][0]["lastCompatibleVersion"], "9999.0.0")
        before = len(self.calls())
        repeated, _ = self.run_publisher(True)
        self.assertEqual(repeated.returncode, 0, repeated.stderr)
        self.assertFalse(any(c["write"] for c in self.calls()[before:]))

    def test_dry_run_recovers_backup_locally_without_writing_remote(self):
        for name in ("compatibility.json", "compatibility-v3.json"):
            (self.assets / name).rename(self.assets / (name + ".backup.fixture"))
        result, output = self.run_publisher()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((output / "compatibility-v3.json").is_file())
        self.assertFalse(any(c["write"] for c in self.calls()))

    def test_existing_desktop_and_unselected_cli_are_preserved(self):
        desktop = {"id": "pen-desktop", "platform": "macos", "evidence": "live-verified",
                   "lastCompatibleAppVersion": "1.2.7", "compatibleAt": "2026-09-01T00:00:00Z"}
        feed = copy.deepcopy(self.base)
        feed["releases"][0]["desktopVerifications"] = [desktop]
        other = {**self.entry, "id": "claude-code"}
        feed["releases"][0]["verifications"].append(other)
        (self.assets / "compatibility-v3.json").write_text(json.dumps(feed))
        result, _ = self.run_publisher(True)
        self.assertEqual(result.returncode, 0, result.stderr)
        updated = json.loads((self.assets / "compatibility-v3.json").read_bytes())["releases"][0]
        self.assertEqual(updated["desktopVerifications"], [desktop])
        self.assertEqual(updated["verifications"][1], other)

    def test_daily_updates_cannot_bypass_release_matrix_or_modify_other_releases(self):
        path = self.updates / "codex.json"
        update = json.loads(path.read_bytes())
        update["nanHarnessVersion"] = "99.0.0"
        path.write_text(json.dumps(update))
        result, _ = self.run_publisher(True)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.remote / "calls.jsonl").exists())

    def test_failed_swap_restores_previous_feed_and_retry_succeeds(self):
        result, _ = self.run_publisher(True, NAN_CANARY_PUBLICATION_FAIL_PHASE="after-stable-delete",
                                       NAN_CANARY_PUBLICATION_FAIL_ASSET="compatibility-v3.json")
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(list(self.assets.glob("compatibility-v3.json.backup.*")))
        result, _ = self.run_publisher(True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads((self.assets / "compatibility-v3.json").read_bytes())
                         ["releases"][0]["verifications"][0]["lastCompatibleVersion"], "9999.0.0")


if __name__ == "__main__":
    unittest.main()
