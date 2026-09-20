#!/usr/bin/env python3
"""Offline contracts for the trusted release publisher handoff."""

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import textwrap
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("release_publish", ROOT / "canary/actions/release_publish.py")
publisher = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(publisher)


class ReleasePublishTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.run_id = "run-123"
        self.tag_commit = "a" * 40
        self.workflow_commit = "b" * 40
        self.assets = self.root / "assets"
        self.assets.mkdir()
        asset_entries = []
        for name in publisher.ASSET_NAMES:
            path = self.assets / name
            path.write_bytes(name.encode())
            asset_entries.append({"name": name, "path": f"assets/{name}",
                                  "sha256": publisher.digest(path), "bytes": path.stat().st_size})
        self.manifest = self.root / "SHA256SUMS"
        update_manifest = self.root / "update-manifest.json"
        update_manifest.write_text(json.dumps({"schemaVersion": 1, "version": "1.2.3",
                                                "notesUrl": "https://example.test/", "artifacts": []}) + "\n")
        self.manifest.write_text("\n".join(f"{item['sha256']}  {item['name']}" for item in asset_entries)
                                  + f"\n{publisher.digest(update_manifest)}  update-manifest.json\n")
        reports = []
        for platform in publisher.PLATFORMS:
            for harness in publisher.HARNESSES:
                if f"{platform}/{harness}" not in publisher.REQUIRED_IDENTITIES:
                    continue
                report = {
                    "schemaVersion": 2, "runId": self.run_id, "trigger": "release",
                    "tier": "release-gate",
                    "nanHarness": {"source": f"commit:{self.tag_commit}",
                                    "version": "1.2.3",
                                    "sha256": next(item["sha256"] for item in asset_entries
                                                  if item["name"] == publisher.PLATFORM_ASSETS[platform]["harness"])},
                    "harness": {"id": harness},
                    "environment": {"operatingSystem": platform,
                                    "architecture": publisher.PLATFORMS[platform]["architecture"]},
                    "checks": [{"name": name, "status": "passed"} for name in publisher.REQUIRED_CHECKS],
                    "outcome": "passed",
                }
                path = self.root / f"{platform}-{harness}.json"
                path.write_text(json.dumps(report))
                reports.append({"identity": f"{platform}-{harness}", "path": path.name,
                                "sha256": publisher.digest(path), "sourceSha": self.tag_commit,
                                "runId": self.run_id, "bytes": path.stat().st_size})
        self.handoff = self.root / "handoff.json"
        self.handoff.write_text(json.dumps({
            "schemaVersion": 1, "repository": "Acme/Fork", "tag": "v1.2.3",
            "tagCommit": self.tag_commit, "workflowCommit": self.workflow_commit,
            "runId": self.run_id, "reportCount": 43, "reports": reports,
            "assets": asset_entries, "assetManifest": self.manifest.name,
            "assetManifestSha256": publisher.digest(self.manifest),
            "attestation": {"workflow": "Acme/Fork/.github/workflows/release.yml",
                            "sourceRef": "refs/tags/v1.2.3"},
        }))

    def tearDown(self):
        self.temp.cleanup()

    def test_valid_handoff_has_complete_matrix_and_assets(self):
        value = publisher.validate_handoff(self.handoff)
        self.assertEqual(value["reportCount"], 43)

    def test_child_boundaries_strip_secrets_from_validator(self):
        with patch.dict(os.environ, {"GH_TOKEN": "gh-secret", "GITHUB_TOKEN": "github-secret",
                                     "NAN_API_KEY": "api-secret"}):
            validator_env = publisher._child_env(github=False)
            helper_env = publisher._child_env(github=True)
        self.assertNotIn("GH_TOKEN", validator_env)
        self.assertNotIn("GITHUB_TOKEN", validator_env)
        self.assertNotIn("NAN_API_KEY", validator_env)
        self.assertEqual(helper_env.get("GH_TOKEN"), "gh-secret")
        self.assertNotIn("GITHUB_TOKEN", helper_env)
        self.assertNotIn("NAN_API_KEY", helper_env)

    def test_prerelease_publication_is_rejected_before_remote_access(self):
        args = publisher.parser().parse_args(["--repository", "Acme/Fork", "--tag", "v1.2.3-rc.1",
                                               "--publish"])
        with patch.object(publisher, "_remote_tag_commit") as remote:
            with self.assertRaises(publisher.ContractError):
                publisher.publish(args)
        remote.assert_not_called()

    def test_duplicate_report_identity_is_rejected(self):
        value = json.loads(self.handoff.read_text())
        value["reports"][1]["identity"] = value["reports"][0]["identity"]
        self.handoff.write_text(json.dumps(value))
        with self.assertRaises(publisher.ContractError):
            publisher.validate_handoff(self.handoff)

    def test_cross_source_report_is_rejected(self):
        value = json.loads(self.handoff.read_text())
        value["reports"][0]["sourceSha"] = "c" * 40
        self.handoff.write_text(json.dumps(value))
        with self.assertRaises(publisher.ContractError):
            publisher.validate_handoff(self.handoff)

    def test_tampered_report_is_rejected(self):
        value = json.loads(self.handoff.read_text())
        report_path = self.root / value["reports"][0]["path"]
        report_path.write_text(report_path.read_text() + "tampered")
        with self.assertRaises(publisher.ContractError):
            publisher.validate_handoff(self.handoff)

    def test_missing_required_report_check_is_rejected_locally_and_durably(self):
        valid = publisher.validate_handoff(self.handoff)
        evidence = publisher.build_evidence(self.handoff, valid)
        value = json.loads(self.handoff.read_text())
        report_path = self.root / value["reports"][0]["path"]
        report = json.loads(report_path.read_text())
        report["checks"] = [{"name": "install-and-diagnose", "status": "passed"}]
        report_path.write_text(json.dumps(report))
        value["reports"][0]["sha256"] = publisher.digest(report_path)
        self.handoff.write_text(json.dumps(value))
        with self.assertRaises(publisher.ContractError):
            publisher.validate_handoff(self.handoff)
        evidence["reports"]["linux/claude-code"]["checks"] = [{"name": "install-and-diagnose", "status": "passed"}]
        with self.assertRaises(publisher.ContractError):
            publisher.validate_evidence(evidence, "Acme/Fork", "v1.2.3")

    def test_receipt_rejects_interrupted_phase_order(self):
        handoff = json.loads(self.handoff.read_text())
        handoff["_handoffSha256"] = publisher._canonical_digest(handoff)
        handoff["_evidenceSha256"] = "c" * 64
        handoff["_assetDigests"] = {item["name"]: item["sha256"] for item in handoff["assets"]}
        receipt = {"schemaVersion": 1, **{key: handoff[key] for key in
                   ("repository", "tag", "tagCommit", "workflowCommit", "runId")},
                   "handoffSha256": handoff["_handoffSha256"],
                   "evidenceSha256": handoff["_evidenceSha256"],
                   "assetDigests": handoff["_assetDigests"],
                   "phases": {phase: False for phase in publisher.PHASES}}
        receipt["phases"]["releasePublished"] = True
        with self.assertRaises(publisher.ContractError):
            publisher.validate_receipt(receipt, handoff)

    def test_receipt_accepts_monotonic_resume_state(self):
        handoff = json.loads(self.handoff.read_text())
        handoff["_handoffSha256"] = publisher._canonical_digest(handoff)
        handoff["_evidenceSha256"] = "c" * 64
        handoff["_assetDigests"] = {item["name"]: item["sha256"] for item in handoff["assets"]}
        receipt = {"schemaVersion": 1, **{key: handoff[key] for key in
                   ("repository", "tag", "tagCommit", "workflowCommit", "runId")},
                   "handoffSha256": handoff["_handoffSha256"],
                   "evidenceSha256": handoff["_evidenceSha256"],
                   "assetDigests": handoff["_assetDigests"],
                   "phases": {phase: True for phase in publisher.PHASES[:2]}}
        receipt["phases"].update({phase: False for phase in publisher.PHASES[2:]})
        publisher.validate_receipt(receipt, handoff)

    def test_recommendation_can_use_durable_receipt_without_expired_local_evidence(self):
        for path in self.root.glob("*.json"):
            if path.name != "handoff.json":
                path.unlink()
        for path in self.assets.iterdir():
            path.unlink()
        value = publisher.validate_handoff(self.handoff, self.assets, require_evidence=False)
        self.assertEqual(value["reportCount"], 43)

    def test_durable_evidence_is_strict_and_binds_all_reports(self):
        handoff = publisher.validate_handoff(self.handoff)
        evidence = publisher.build_evidence(self.handoff, handoff)
        publisher.validate_evidence(evidence, "Acme/Fork", "v1.2.3")
        evidence["reports"].pop(next(iter(evidence["reports"])))
        with self.assertRaises(publisher.ContractError):
            publisher.validate_evidence(evidence, "Acme/Fork", "v1.2.3")

    def test_fake_gh_remote_persists_evidence_for_clean_recommendation(self):
        handoff = publisher.validate_handoff(self.handoff)
        evidence = publisher.build_evidence(self.handoff, handoff)
        local = self.root / "release-gate-evidence-1.2.3.json"
        local.write_text(json.dumps(evidence, sort_keys=True, separators=(",", ":")) + "\n")
        remote = self.root / "remote"
        fake_bin = self.root / "bin"
        remote.mkdir()
        fake_bin.mkdir()
        fake = fake_bin / "gh"
        fake.write_text(textwrap.dedent("""
            #!/usr/bin/env python3
            import os, pathlib, shutil, sys
            remote = pathlib.Path(os.environ["FAKE_GH_REMOTE"])
            args = sys.argv[1:]
            if args[:2] == ["release", "upload"]:
                source = next(pathlib.Path(value) for value in args[2:] if pathlib.Path(value).is_file())
                target = remote / source.name
                shutil.copyfile(source, target)
                raise SystemExit(0)
            if args[:2] == ["release", "download"]:
                pattern = args[args.index("--pattern") + 1]
                output = pathlib.Path(args[args.index("--output") + 1])
                source = remote / pattern
                if not source.exists():
                    raise SystemExit(1)
                shutil.copyfile(source, output)
                raise SystemExit(0)
            raise SystemExit(1)
        """).lstrip())
        fake.chmod(fake.stat().st_mode | stat.S_IXUSR)
        clean = self.root / "clean"
        clean.mkdir()
        fetched = clean / local.name
        with patch.dict(os.environ, {"FAKE_GH_REMOTE": str(remote), "PATH": str(fake_bin) + os.pathsep + os.environ["PATH"]}):
            publisher._upload_evidence("Acme/Fork", "v1.2.3", local)
            self.assertTrue(publisher._evidence_asset("Acme/Fork", "v1.2.3", fetched))
        self.assertEqual(publisher.digest(local), publisher.digest(fetched))
        publisher.validate_evidence(json.loads(fetched.read_text()), "Acme/Fork", "v1.2.3")
        handoff["_evidenceSha256"] = publisher.digest(fetched)
        handoff["_assetDigests"] = {item["name"]: item["sha256"] for item in handoff["assets"]}
        receipt = self.root / "release-publication-receipt-1.2.3.json"
        receipt.write_text(json.dumps({"schemaVersion": 1, **{key: handoff[key] for key in
                          ("repository", "tag", "tagCommit", "workflowCommit", "runId")},
                          "handoffSha256": handoff["_handoffSha256"],
                          "evidenceSha256": handoff["_evidenceSha256"],
                          "assetDigests": handoff["_assetDigests"],
                          "phases": {phase: True for phase in publisher.PHASES}}))
        with patch.dict(os.environ, {"FAKE_GH_REMOTE": str(remote), "PATH": str(fake_bin) + os.pathsep + os.environ["PATH"]}):
            publisher._upload_receipt("Acme/Fork", "v1.2.3", receipt)
            clean_receipt = self.root / "clean-receipt.json"
            self.assertTrue(publisher._receipt_asset("Acme/Fork", "v1.2.3", clean_receipt))
        publisher.validate_receipt(json.loads(clean_receipt.read_text()), handoff)

    def test_tampered_fake_remote_evidence_is_rejected_before_use(self):
        handoff = publisher.validate_handoff(self.handoff)
        evidence = publisher.build_evidence(self.handoff, handoff)
        evidence["reports"]["linux/claude-code"]["outcome"] = "failed"
        with self.assertRaises(publisher.ContractError):
            publisher.validate_evidence(evidence, "Acme/Fork", "v1.2.3")

    def test_main_recommendation_self_fetches_from_empty_workspace(self):
        handoff = publisher.validate_handoff(self.handoff)
        current_commit = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True,
                                        check=True).stdout.strip()
        raw = json.loads(self.handoff.read_text())
        raw["workflowCommit"] = current_commit
        self.handoff.write_text(json.dumps(raw))
        handoff = publisher.validate_handoff(self.handoff)
        evidence = publisher.build_evidence(self.handoff, handoff)
        evidence_path = self.root / "release-gate-evidence-1.2.3.json"
        evidence_path.write_text(json.dumps(evidence, sort_keys=True, separators=(",", ":")) + "\n")
        remote = self.root / "remote-main"
        remote.mkdir()
        for path in self.assets.iterdir():
            (remote / path.name).write_bytes(path.read_bytes())
        (remote / "SHA256SUMS").write_bytes(self.manifest.read_bytes())
        (remote / "release-gate-evidence-1.2.3.json").write_bytes(evidence_path.read_bytes())
        receipt = {
            "schemaVersion": 1, **{key: raw[key] for key in ("repository", "tag", "tagCommit", "workflowCommit", "runId")},
            "handoffSha256": publisher._canonical_digest(raw), "evidenceSha256": publisher.digest(evidence_path),
            "assetDigests": {item["name"]: item["sha256"] for item in raw["assets"]},
            "phases": {phase: True for phase in publisher.PHASES},
        }
        (remote / "release-publication-receipt-1.2.3.json").write_text(json.dumps(receipt))
        (remote / "available.json").write_text('{"version":"1.2.3"}')
        (remote / "compatibility.json").write_text('{"schemaVersion":3,"releases":[{"nanHarnessVersion":"1.2.3"}]}')
        fake_bin = self.root / "main-bin"
        fake_bin.mkdir()
        fake = fake_bin / "gh"
        fake.write_text(textwrap.dedent("""
            #!/usr/bin/env python3
            import json, os, pathlib, shutil, sys
            remote = pathlib.Path(os.environ["FAKE_GH_REMOTE"])
            args = sys.argv[1:]
            if args[:2] == ["release", "download"]:
                pattern = args[args.index("--pattern") + 1] if "--pattern" in args else None
                source = remote / ({"update-manifest.json":"available.json", "compatibility-v3.json":"compatibility.json"}.get(pattern, pattern or ""))
                if "--dir" in args:
                    output_dir = pathlib.Path(args[args.index("--dir") + 1])
                    output_dir.mkdir(parents=True, exist_ok=True)
                    if pattern is None:
                        for item in remote.iterdir():
                            if item.is_file() and item.name not in ("available.json", "compatibility.json", "latest-mutated"):
                                shutil.copyfile(item, output_dir / item.name)
                        raise SystemExit(0)
                    output = output_dir / source.name
                else:
                    output = pathlib.Path(args[args.index("--output") + 1])
                if not source.exists(): raise SystemExit(1)
                shutil.copyfile(source, output)
                raise SystemExit(0)
            if args[:2] == ["release", "upload"]:
                source = next(pathlib.Path(value) for value in args[2:] if pathlib.Path(value).is_file())
                shutil.copyfile(source, remote / source.name)
                raise SystemExit(0)
            if args[:2] == ["release", "view"]:
                print(json.dumps({"tagName":"v1.2.3","isDraft":False,"isPrerelease":False}))
                raise SystemExit(0)
            if args[:2] == ["release", "edit"]:
                (remote / "latest-mutated").write_text("1")
                raise SystemExit(0)
            if args[0:1] == ["attestation"]:
                raise SystemExit(0)
            if args[0:1] == ["api"] and "releases/latest" not in args[1]:
                print(json.dumps({"object":{"type":"commit","sha":"a"*40}}))
                raise SystemExit(0)
            if args[0:1] == ["api"]:
                print("404 Not Found", file=sys.stderr)
                raise SystemExit(1)
            raise SystemExit(1)
        """).lstrip())
        fake.chmod(fake.stat().st_mode | stat.S_IXUSR)
        validator = fake_bin / "trusted-validator"
        validator.write_text("#!/bin/sh\nexit 0\n")
        validator.chmod(validator.stat().st_mode | stat.S_IXUSR)
        clean = self.root / "empty-workspace"
        clean.mkdir()
        with patch.dict(os.environ, {"FAKE_GH_REMOTE": str(remote),
                                     "NAN_TRUSTED_REPORT_VALIDATOR": str(validator),
                                     "PATH": str(fake_bin) + os.pathsep + os.environ["PATH"]}), patch("os.getcwd", return_value=str(clean)):
            self.assertEqual(publisher.main(["--repository", "Acme/Fork", "--tag", "v1.2.3", "--workflow-commit",
                                             current_commit, "--run-id", "recommend-1", "--recommend"]), 0)
        self.assertTrue((remote / "latest-mutated").exists())
        (remote / "latest-mutated").unlink()
        (remote / "release-gate-evidence-1.2.3.json").write_text("{\"tampered\":true}\n")
        with patch.dict(os.environ, {"FAKE_GH_REMOTE": str(remote),
                                     "NAN_TRUSTED_REPORT_VALIDATOR": str(validator),
                                     "PATH": str(fake_bin) + os.pathsep + os.environ["PATH"]}), patch("os.getcwd", return_value=str(clean)):
            self.assertNotEqual(publisher.main(["--repository", "Acme/Fork", "--tag", "v1.2.3", "--workflow-commit",
                                                current_commit, "--run-id", "recommend-2", "--recommend"]), 0)
        self.assertFalse((remote / "latest-mutated").exists())


if __name__ == "__main__":
    unittest.main()
