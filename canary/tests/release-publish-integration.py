#!/usr/bin/env python3
"""Exercise the release publisher through its real shell publication boundaries."""

import importlib.util
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[2]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


gate = load("release_gate_integration", ROOT / "canary/actions/release_gate.py")
publisher = load("release_publish_integration", ROOT / "canary/actions/release_publish.py")


FAKE_GH = r'''#!/usr/bin/env python3
import json, os, shutil, sys
from pathlib import Path

root = Path(os.environ["FAKE_GH_STATE"])
log = root / "calls.jsonl"
args = sys.argv[1:]
write_ops = {
    ("release", "upload"), ("release", "create"), ("release", "edit"),
    ("release", "delete-asset"),
}
write = tuple(args[:2]) in write_ops
with log.open("a") as stream:
    stream.write(json.dumps({"args": args, "write": write}) + "\n")

def release_dir(tag):
    path = root / "releases" / tag / "assets"
    path.mkdir(parents=True, exist_ok=True)
    return path

def parse(name):
    values = {}
    i = 0
    while i < len(args):
        if args[i].startswith("--") and i + 1 < len(args) and not args[i + 1].startswith("--"):
            values[args[i]] = args[i + 1]; i += 2
        else:
            i += 1
    return values

def assets(tag):
    directory = release_dir(tag)
    return [{"name": item.name, "createdAt": "2026-09-14T00:00:00Z"}
            for item in sorted(directory.iterdir()) if item.is_file()]

if args[:2] == ["release", "view"]:
    tag = args[2]
    if not (root / "releases" / tag / "state").exists(): sys.exit(1)
    state = (root / "releases" / tag / "state").read_text().strip()
    if "--json" in args:
        value = {"tagName": tag, "isDraft": state == "draft", "isPrerelease": state == "prerelease",
                 "assets": assets(tag)}
        print(json.dumps(value))
    sys.exit(0)

if args[:2] == ["release", "download"]:
    tag = args[2]; values = parse("")
    pattern = values.get("--pattern")
    source_dir = release_dir(tag)
    if not source_dir.exists(): sys.exit(1)
    if "--dir" in args:
        target = Path(values["--dir"]); target.mkdir(parents=True, exist_ok=True)
        selected = [p for p in source_dir.iterdir() if p.is_file() and (pattern is None or p.name == pattern)]
        if not selected: sys.exit(1)
        for source in selected: shutil.copyfile(source, target / source.name)
    else:
        if pattern is None or pattern not in {p.name for p in source_dir.iterdir()}:
            sys.exit(1)
        target = Path(values["--output"]); shutil.copyfile(source_dir / pattern, target)
    sys.exit(0)

if args[:2] == ["release", "upload"]:
    tag = args[2]; source = next((Path(value) for value in args[3:] if Path(value).is_file()), None)
    if source is None: sys.exit(1)
    shutil.copyfile(source, release_dir(tag) / source.name)
    sys.exit(0)

if args[:2] == ["release", "create"]:
    tag = args[2]; directory = release_dir(tag)
    (root / "releases" / tag / "state").write_text("prerelease" if "--prerelease" in args else "public")
    sources = [Path(value) for value in args[3:] if Path(value).is_file()]
    for source in sources: shutil.copyfile(source, directory / source.name)
    sys.exit(0)

if args[:2] == ["release", "delete-asset"]:
    tag, name = args[2], args[3]
    target = release_dir(tag) / name
    if not target.exists(): sys.exit(1)
    target.unlink(); sys.exit(0)

if args[:2] == ["release", "edit"]:
    tag = args[2]
    if "--draft=false" in args: (root / "releases" / tag / "state").write_text("public")
    if "--latest" in args: (root / "latest").write_text(tag)
    sys.exit(0)

if args[:1] == ["attestation"]: sys.exit(0)

if args[:1] == ["api"]:
    path = args[1]
    if "/git/ref/tags/" in path:
        value = {"object": {"type": "commit", "sha": (root / "tag-commit").read_text().strip()}}
        if "--jq" in args: print("commit\\t" + value["object"]["sha"])
        else: print(json.dumps(value))
        sys.exit(0)
    if path.endswith("/releases/latest"):
        latest = root / "latest"
        if latest.exists(): print(json.dumps({"tag_name": latest.read_text().strip()})); sys.exit(0)
        print("404 Not Found", file=sys.stderr); sys.exit(1)
    if "/releases/tags/" in path:
        tag = path.rsplit("/", 1)[1]
        if (root / "releases" / tag / "state").exists():
            body = json.dumps({"name": tag, "assets": assets(tag)})
            print("HTTP/2 200 OK\n\n" + body, end="")
            sys.exit(0)
        print("HTTP/2 404 Not Found\n\n{}", end="")
        sys.exit(1)
sys.exit(1)
'''


class ReleasePublishIntegrationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.assets = self.root / "assets"
        self.reports = self.root / "reports"
        self.remote = self.root / "remote"
        self.bin = self.root / "bin"
        for path in (self.assets, self.reports, self.remote / "releases", self.bin): path.mkdir(parents=True)
        self.repository = "Acme/Fork"
        self.tag = "v1.2.3"
        self.tag_commit = "a" * 40
        self.workflow_commit = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT,
                                              text=True, capture_output=True, check=True).stdout.strip()
        (self.remote / "tag-commit").write_text(self.tag_commit)
        fake = self.bin / "gh"
        fake.write_text(FAKE_GH)
        fake.chmod(fake.stat().st_mode | stat.S_IXUSR)
        for name in gate.ASSETS:
            (self.assets / name).write_bytes((name + "-payload").encode())
        version = self.tag[1:]
        manifest = {"schemaVersion": 1, "version": version, "notesUrl": "https://example.test/", "artifacts": []}
        for name in ("nan-harness-aarch64-unknown-linux-musl", "nan-harness-aarch64-apple-darwin"):
            manifest["artifacts"].append({"target": name, "sha256": gate.digest(self.assets / name),
                "url": f"https://github.com/{self.repository}/releases/download/{self.tag}/{name}"})
        (self.assets / "update-manifest.json").write_text(json.dumps(manifest) + "\n")
        sums = [f"{gate.digest(self.assets / name)}  {name}" for name in gate.ASSETS]
        sums.append(f"{gate.digest(self.assets / 'update-manifest.json')}  update-manifest.json")
        (self.assets / "SHA256SUMS").write_text("\n".join(sums) + "\n")
        for identity in sorted(gate.expected_identities()):
            system, harness = identity.split("-", 1)
            self._write_report(system, harness)
        self.handoff = self.root / "handoff.json"
        gate.build_manifest(type("Args", (), {"repository": self.repository, "tag": self.tag,
            "tag_commit": self.tag_commit, "workflow_commit": self.workflow_commit, "run_id": "run-1",
            "reports_dir": self.reports, "assets_dir": self.assets, "output": self.handoff})())
        release = self.remote / "releases" / self.tag
        (release / "assets").mkdir(parents=True)
        (release / "state").write_text("draft")
        for source in self.assets.iterdir(): shutil.copyfile(source, release / "assets" / source.name)

    def tearDown(self): self.temp.cleanup()

    def _write_report(self, system, harness):
        asset = gate.PLATFORM_ASSETS[system]["harness"]
        report = {"schemaVersion": 2, "runId": "run-1", "cellId": f"{system}-{harness}",
            "specSha256": "c" * 64, "trigger": "release", "tier": "release-gate",
            "scenario": "synthetic-release-gate", "startedAt": "2026-09-14T00:00:00Z",
            "completedAt": "2026-09-14T00:00:01Z", "durationMilliseconds": 1000,
            "nanHarness": {"version": "1.2.3", "source": f"commit:{self.tag_commit}",
                           "sha256": gate.digest(self.assets / asset)},
            "environment": {"operatingSystem": system,
                             "architecture": gate.PLATFORMS[system]["architecture"], "image": "synthetic",
                             "profile": "release", "runtimes": []},
            "harness": {"id": harness, "version": "9999.0.0"},
            "checks": [{"name": name, "status": "passed", "durationMilliseconds": 1, "attempts": 1}
                       for name in ("install-and-diagnose", "deterministic-conformance", "live-tool")],
            "outcome": "passed"}
        architecture = gate.PLATFORMS[system]["architecture"]
        (self.reports / f"{system}-{architecture}-{harness}.json").write_text(json.dumps(report, sort_keys=True) + "\n")

    def _env(self):
        env = dict(os.environ)
        env.update({"FAKE_GH_STATE": str(self.remote), "PATH": str(self.bin) + os.pathsep + env["PATH"],
                    "NAN_CANARY_RETRY_DELAY_SECONDS": "0", "NAN_CANARY_STATE_DIR": str(self.root / "state")})
        return env

    def _run(self, *extra):
        return subprocess.run([os.environ.get("PYTHON", "python3"), str(ROOT / "canary/actions/release_publish.py"),
                               "--repository", self.repository,
                               "--tag", self.tag, "--handoff", str(self.handoff), "--assets-dir", str(self.assets),
                               "--reports-dir", str(self.reports), "--report-validator",
                               str(ROOT / "target/debug/nan-harness-canary"), *extra], cwd=ROOT,
                              env=self._env(), text=True, capture_output=True)

    def _calls(self): return [json.loads(line) for line in (self.remote / "calls.jsonl").read_text().splitlines()]

    def test_publish_recommend_and_verify_only_use_real_boundaries(self):
        published = self._run("--publish")
        self.assertEqual(published.returncode, 0, published.stderr)
        self.assertEqual((self.remote / "releases" / self.tag / "state").read_text(), "public")
        compatibility = self.remote / "releases" / "compatibility" / "assets" / "compatibility-v3.json"
        available = self.remote / "releases" / "available" / "assets" / "update-manifest.json"
        self.assertIn('"nanHarnessVersion": "1.2.3"', compatibility.read_text())
        self.assertEqual(json.loads(available.read_text())["version"], "1.2.3")
        calls = self._calls()
        writes = [call["args"] for call in calls if call["write"]]
        evidence = next(i for i, args in enumerate(writes) if "release-gate-evidence-1.2.3.json" in " ".join(args))
        receipt = next(i for i, args in enumerate(writes) if "release-publication-receipt-1.2.3.json" in " ".join(args))
        edit = next(i for i, args in enumerate(writes) if args[:2] == ["release", "edit"] and "--draft=false" in args)
        self.assertIn("--latest=false", writes[edit])
        available_write = next(i for i, args in enumerate(writes)
                               if "update-manifest.json" in " ".join(args)
                               and args[:2] in (["release", "upload"], ["release", "create"]))
        self.assertLess(evidence, receipt)
        self.assertLess(receipt, edit)
        self.assertLess(edit, available_write)

        before_verify = len(self._calls())
        verify_only = self._run()
        self.assertEqual(verify_only.returncode, 0, verify_only.stderr)
        after = self._calls()
        self.assertTrue(after[before_verify:])
        self.assertFalse(any(call["write"] for call in after[before_verify:]))

        # Recommendation trusts the current publisher checkout while retaining
        # the immutable workflow/run binding from the original evidence.
        old_workflow = "b" * 40
        handoff = json.loads(self.handoff.read_text())
        handoff["workflowCommit"] = old_workflow
        self.handoff.write_text(json.dumps(handoff, sort_keys=True) + "\n")
        validated = publisher.validate_handoff(self.handoff, self.assets, self.reports, False)
        evidence_value = publisher.build_evidence(self.handoff, validated, self.reports)
        evidence_path = self.root / "replacement-evidence.json"
        evidence_path.write_text(json.dumps(evidence_value, sort_keys=True, separators=(",", ":")) + "\n")
        release_assets = self.remote / "releases" / self.tag / "assets"
        durable_evidence = release_assets / "release-gate-evidence-1.2.3.json"
        shutil.copyfile(evidence_path, durable_evidence)
        receipt_path = release_assets / "release-publication-receipt-1.2.3.json"
        receipt = json.loads(receipt_path.read_text())
        receipt["workflowCommit"] = old_workflow
        receipt["handoffSha256"] = publisher._canonical_digest(handoff)
        receipt["evidenceSha256"] = publisher.digest(durable_evidence)
        receipt_path.write_text(json.dumps(receipt, sort_keys=True, indent=2) + "\n")

        recommended = self._run("--recommend", "--workflow-commit", self.workflow_commit, "--run-id", "recommend-1")
        self.assertEqual(recommended.returncode, 0, recommended.stderr)
        self.assertEqual((self.remote / "latest").read_text(), self.tag)
        recommendation_receipt = json.loads((release_assets / "release-recommendation-receipt-1.2.3.json").read_text())
        self.assertEqual(recommendation_receipt["workflowCommit"], self.workflow_commit)
        self.assertEqual(recommendation_receipt["runId"], "recommend-1")
        self.assertEqual(recommendation_receipt["evidenceWorkflowCommit"], old_workflow)
        self.assertEqual(recommendation_receipt["evidenceRunId"], "run-1")
        self.assertTrue(any(args[:2] == ["release", "edit"] and "--latest" in args
                            for args in (call["args"] for call in self._calls())))

    def test_resume_reconstructs_hosted_report_names_from_evidence(self):
        published = self._run("--publish")
        self.assertEqual(published.returncode, 0, published.stderr)
        receipt_path = self.remote / "releases" / self.tag / "assets" / "release-publication-receipt-1.2.3.json"
        receipt = json.loads(receipt_path.read_text())
        receipt["phases"].update({phase: False for phase in publisher.PHASES[3:]})
        receipt_path.write_text(json.dumps(receipt, sort_keys=True, indent=2) + "\n")
        (self.remote / "releases" / self.tag / "state").write_text("draft")
        self.reports.rename(self.root / "reports-unavailable")

        resumed = self._run("--publish")
        self.assertEqual(resumed.returncode, 0, resumed.stderr)
        self.assertEqual((self.remote / "releases" / self.tag / "state").read_text(), "public")


if __name__ == "__main__":
    unittest.main()
