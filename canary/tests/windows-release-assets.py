#!/usr/bin/env python3
"""Synthetic API/ZIP/checksum/attestation admission tests; no native binary executes."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import stat
from types import SimpleNamespace
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "actions"))
spec = importlib.util.spec_from_file_location("windows_release_assets", ROOT / "actions/windows_release_assets.py")
assets = importlib.util.module_from_spec(spec); spec.loader.exec_module(assets)


class ReleaseAssetsTests(unittest.TestCase):
    def setup_case(self, root):
        self.files = {name: ("synthetic " + name).encode() for name in sorted(assets.PREPARE_MEMBERS - {"SHA256SUMS"})}
        self.files["SHA256SUMS"] = "".join(hashlib.sha256(value).hexdigest() + "  " + name + "\n"
                                            for name, value in self.files.items()).encode()
        self.commands = []
        self.run = {"id": 40475697974, "head_sha": "a112fa319acd94b551b660239b70a950b6cc62f3",
                    "head_branch": "main", "path": ".github/workflows/release-gate.yml", "event": "workflow_dispatch",
                    "repository": {"full_name": assets.REPOSITORY}, "head_repository": {"full_name": assets.REPOSITORY}}
        self.args = SimpleNamespace(repository=assets.REPOSITORY, tag="v0.1.26", commit="0be5d88fdefe020e8c302f6538756869042c01fd",
            controller="b" * 40, run_id=self.run["id"], manifest_sha256=hashlib.sha256(self.files["SHA256SUMS"]).hexdigest(),
            directory=root / "staged")
        self.tag = {"object": {"type": "commit", "sha": self.args.commit}}
        self.archive = self.make_zip(self.files)
        self.artifact = {"id": 123, "name": "release-gate-assets", "expired": False,
            "size_in_bytes": len(self.archive), "digest": "sha256:" + hashlib.sha256(self.archive).hexdigest(),
            "workflow_run": {"id": self.args.run_id, "head_sha": self.run["head_sha"], "head_branch": "main"}}

    @staticmethod
    def make_zip(files):
        output = io.BytesIO()
        with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            for name, value in files.items():
                archive.writestr(name, value)
        return output.getvalue()

    def fetch(self, endpoint):
        if endpoint.endswith(f"/actions/runs/{self.args.run_id}"):
            return self.run
        if endpoint.endswith("/artifacts?per_page=100"):
            return {"total_count": 1, "artifacts": [self.artifact]}
        if endpoint.endswith("/git/ref/tags/" + self.args.tag):
            return self.tag
        raise AssertionError("unexpected metadata request")

    def command(self, argv, output=None):
        self.commands.append(argv)
        if output is not None:
            self.assertEqual(argv, ["gh", "api", f"repos/{assets.REPOSITORY}/actions/artifacts/123/zip"])
            output.write(self.archive)

    def test_exact_pair_is_bound_to_independent_controller_and_release_sources(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.setup_case(Path(tmp))
            cli, canary, receipt = assets.stage(self.args, self.fetch, self.command)
            self.assertEqual(cli.read_bytes(), self.files[cli.name])
            self.assertEqual(canary.read_bytes(), self.files[canary.name])
            self.assertEqual(receipt["controllerSha"], "b" * 40)
            self.assertEqual(receipt["releaseCommit"], self.args.commit)
            self.assertEqual(receipt["assetWorkflowSha"], self.run["head_sha"])
            self.assertEqual(receipt["nanHarnessSha256"], hashlib.sha256(self.files[cli.name]).hexdigest())
            self.assertEqual(receipt["canarySha256"], hashlib.sha256(self.files[canary.name]).hexdigest())
            self.assertNotIn(str(tmp), json.dumps(receipt))
            self.assertEqual(self.commands[-1], ["gh", "attestation", "verify", str(self.args.directory / "SHA256SUMS"),
                "--source-ref", "refs/tags/v0.1.26", "--source-digest", self.args.commit,
                "--repo", assets.REPOSITORY, "--signer-workflow", assets.REPOSITORY + "/.github/workflows/release.yml",
                "--deny-self-hosted-runners"])
            self.assertEqual(len(self.commands), 2)

    def test_full_canonical_eight_member_zip_extracts_only_windows_pair_and_manifest(self):
        producer = (ROOT.parent / ".github/workflows/release-gate.yml").read_text()
        asset_loop = producer.split("for asset in \\\n", 1)[1].split("; do", 1)[0]
        names = {line.strip().rstrip("\\").strip() for line in asset_loop.splitlines()}
        self.assertIn('--pattern SHA256SUMS', producer)
        self.assertEqual(names | {"SHA256SUMS"}, assets.PREPARE_MEMBERS)
        with tempfile.TemporaryDirectory() as tmp:
            self.setup_case(Path(tmp))
            self.assertEqual(len(self.files), 8)
            archive = Path(tmp) / "release.zip"
            archive.write_bytes(self.archive)
            directory = Path(tmp) / "out"; directory.mkdir()
            assets.extract_pair(archive, directory)
            self.assertEqual({path.name for path in directory.iterdir()}, {
                "SHA256SUMS", assets.PLATFORM_ASSETS["windows"]["harness"], assets.PLATFORM_ASSETS["windows"]["canary"]})

    def test_invalid_run_artifact_and_tag_never_reach_extraction_or_attestation(self):
        changes = [("run", "head_branch", "PRIVATE"), ("run", "path", ".github/workflows/release.yml"),
                   ("run", "head_repository", {"full_name": "foreign/repository"}),
                   ("artifact", "expired", True), ("artifact", "digest", "sha256:PRIVATE"),
                   ("artifact", "size_in_bytes", True), ("artifact", "workflow_run", {"id": 1}),
                   ("tag", "object", {"type": "commit", "sha": "c" * 40})]
        for target, key, value in changes:
            with self.subTest(target=target, key=key), tempfile.TemporaryDirectory() as tmp:
                self.setup_case(Path(tmp))
                getattr(self, target)[key] = value
                with self.assertRaises(ValueError):
                    assets.stage(self.args, self.fetch, self.command)
                self.assertEqual(self.commands, [])

    def test_changed_zip_manifest_or_either_binary_is_never_accepted(self):
        for changed in ("archive", "manifest", "cli", "canary", "signature"):
            with self.subTest(changed=changed), tempfile.TemporaryDirectory() as tmp:
                self.setup_case(Path(tmp))
                command = self.command
                if changed == "archive":
                    self.archive += b"changed"
                elif changed == "manifest":
                    self.args.manifest_sha256 = "f" * 64
                elif changed in {"cli", "canary"}:
                    name = assets.PLATFORM_ASSETS["windows"]["harness" if changed == "cli" else "canary"]
                    self.files[name] = b"changed"
                    self.archive = self.make_zip(self.files)
                    self.artifact.update(size_in_bytes=len(self.archive), digest="sha256:" + hashlib.sha256(self.archive).hexdigest())
                else:
                    def command(argv, output=None):
                        self.command(argv, output)
                        if output is None:
                            raise ValueError("signature rejected")
                with self.assertRaises(ValueError):
                    assets.stage(self.args, self.fetch, command)

    def test_unknown_or_duplicate_archive_entries_cannot_escape_the_staging_directory(self):
        for name in ("../outside", "nested/SHA256SUMS", "PRIVATE"):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as tmp:
                self.setup_case(Path(tmp))
                archive = Path(tmp) / "bad.zip"
                archive.write_bytes(self.make_zip({**self.files, name: b"untrusted"}))
                directory = Path(tmp) / "out"; directory.mkdir()
                with self.assertRaises(ValueError):
                    assets.extract_pair(archive, directory)
                self.assertEqual(list(directory.iterdir()), [])
        for duplicate in (True, False):
            with self.subTest(duplicate=duplicate), tempfile.TemporaryDirectory() as tmp:
                self.setup_case(Path(tmp))
                archive = Path(tmp) / "bad.zip"
                with zipfile.ZipFile(archive, "w") as output:
                    for name, value in self.files.items():
                        entry = zipfile.ZipInfo(name)
                        if not duplicate and name == "SHA256SUMS":
                            entry.external_attr = (stat.S_IFLNK | 0o777) << 16
                        output.writestr(entry, value)
                    if duplicate:
                        import warnings
                        with warnings.catch_warnings():
                            warnings.simplefilter("ignore", UserWarning)
                            output.writestr("SHA256SUMS", b"duplicate")
                directory = Path(tmp) / "out"; directory.mkdir()
                with self.assertRaises(ValueError):
                    assets.extract_pair(archive, directory)
                self.assertEqual(list(directory.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
