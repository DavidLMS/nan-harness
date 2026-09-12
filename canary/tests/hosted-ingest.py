#!/usr/bin/env python3
"""Automatic ingestion authenticates data before any durable state mutation."""

import hashlib
import io
import json
from pathlib import Path
import stat
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "actions"))
import hosted_publication as ingest
import publication
from state import StateError, canonical

COMMIT = "a" * 40
SPEC = "b" * 64


def bundle():
    report = {"nanHarness": {"version": "0.1.6", "sha256": "c" * 64},
              "environment": {"operatingSystem": "linux", "architecture": "aarch64"},
              "checks": [{"name": "live-tool"}], "model": "chosen-model"}
    return {"schemaVersion": 1, "suite": "cli", "platform": "linux", "architecture": "aarch64",
            "sourceCommit": COMMIT, "releaseTag": "v0.1.6", "releaseCommit": "d" * 40,
            "model": "chosen-model", "specSha256": SPEC, "reports": [report]}


def archive(value, name="evidence.json", symlink=False, extra=False):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w") as target:
        entry = zipfile.ZipInfo(name)
        if symlink:
            entry.external_attr = (stat.S_IFLNK | 0o777) << 16
        target.writestr(entry, canonical(value))
        if extra:
            target.writestr("payload.py", "raise RuntimeError('never execute')")
    return output.getvalue()


class IngestTests(unittest.TestCase):
    def test_archive_is_exactly_one_bounded_regular_data_file(self):
        self.assertEqual(ingest.artifact_bundle(archive(bundle())), bundle())
        for raw in (archive(bundle(), "../evidence.json"), archive(bundle(), symlink=True),
                    archive(bundle(), extra=True), b"invalid", b"x" * (ingest.LIMIT + 1)):
            with self.assertRaises(StateError):
                ingest.artifact_bundle(raw)

    def test_artifacts_bind_api_run_name_and_digest(self):
        raw = archive(bundle())
        artifact = {"id": 1, "name": "hosted-evidence-cli-linux", "expired": False,
                    "workflow_run": {"id": 12}, "size_in_bytes": len(raw),
                    "digest": "sha256:" + hashlib.sha256(raw).hexdigest()}
        store = Mock(repository="owner/repository")
        store.call.return_value = {"total_count": 1, "artifacts": [artifact]}
        self.assertEqual(list(ingest.source_artifacts(store, 12, Mock(return_value=raw))), [bundle()])
        for field, value in (("digest", "sha256:" + "0" * 64), ("workflow_run", {"id": 13}),
                             ("expired", True), ("size_in_bytes", ingest.LIMIT + 1),
                             ("name", "hosted-evidence-desktop-linux")):
            changed = {**artifact, field: value}
            store.call.return_value = {"total_count": 1, "artifacts": [changed]}
            with self.assertRaises(StateError, msg=field):
                list(ingest.source_artifacts(store, 12, Mock(return_value=raw)))

    def test_release_identity_and_report_validation_precede_conversion(self):
        digests = {"nan-harness-aarch64-unknown-linux-musl": "c" * 64}
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(ingest, "specification_digest", return_value=SPEC), \
                patch.object(ingest, "attested_release", return_value=(digests, b"{}")):
            command = Mock()
            args = (Mock(), bundle(), COMMIT, Path(directory), Path(directory),
                    {"cli": "trusted-validator"}, command, Mock())
            reports, _registry = ingest.bundle_updates(*args)
            self.assertEqual(json.loads(reports[0])["model"], "chosen-model")
            self.assertEqual(command.call_args.args[0][:2], ["trusted-validator", "validate-report"])
            for field, value in (("sourceCommit", "e" * 40), ("specSha256", "f" * 64),
                                 ("architecture", "x86_64"), ("releaseTag", "feature"),
                                 ("model", "other-model")):
                changed = {**bundle(), field: value}
                with self.assertRaises(StateError, msg=field):
                    ingest.bundle_updates(args[0], changed, *args[2:])
            command.side_effect = StateError("validator rejected report")
            with self.assertRaises(StateError):
                ingest.bundle_updates(*args)

    def test_attestation_checks_exact_tag_commit_before_registry_read(self):
        calls = []
        def command(argv):
            calls.append(argv)
            if argv[:3] == ["gh", "release", "download"]:
                Path(argv[-1]).write_text("c" * 64 + "  native-binary\n")
            if argv[:2] == ["git", "show"]:
                return b"{}"
            return b""
        store = Mock(repository="owner/repository")
        with tempfile.TemporaryDirectory() as directory:
            digests, _ = ingest.attested_release(store, bundle(), Path(directory), command,
                                                Mock(return_value="d" * 40))
            self.assertEqual(digests["native-binary"], "c" * 64)
            verify = calls[1]
            self.assertIn("refs/tags/v0.1.6", verify)
            self.assertIn("d" * 40, verify)
            self.assertIn("--deny-self-hosted-runners", verify)
            calls.clear()
            with self.assertRaises(StateError):
                ingest.attested_release(store, bundle(), Path(directory), command,
                                        Mock(return_value="e" * 40))
            self.assertEqual(calls, [])

    def test_untrusted_or_invalid_data_never_enters_queue(self):
        args = SimpleNamespace(run="12", validator="validator", checker="checker")
        store = Mock()
        with patch.object(ingest, "trusted_run", side_effect=StateError("untrusted")):
            with self.assertRaises(StateError):
                ingest.enqueue(args, store, Path("/unused"), Mock(), Mock())
        store.enqueue.assert_not_called()
        with patch.object(ingest, "trusted_run", return_value=COMMIT), \
                patch.object(ingest, "source_artifacts", return_value=[bundle()]), \
                patch.object(ingest, "bundle_updates", side_effect=StateError("invalid report")):
            with self.assertRaises(StateError):
                ingest.enqueue(args, store, Path("/unused"), Mock(), Mock())
        store.enqueue.assert_not_called()

    def test_enqueue_replay_has_one_content_identity(self):
        args = SimpleNamespace(run="12", validator="validator", checker="checker")
        requests = {}
        def enqueue(request):
            identity = hashlib.sha256(canonical(request)).hexdigest()
            requests[identity] = request
            return identity
        store = Mock()
        store.enqueue.side_effect = enqueue
        with patch.object(ingest, "trusted_run", return_value=COMMIT), \
                patch.object(ingest, "source_artifacts", return_value=[bundle()]), \
                patch.object(ingest, "bundle_updates", return_value=([], b"{}")):
            first = ingest.enqueue(args, store, Path("/unused"), Mock(), Mock())
            self.assertEqual(first, ingest.enqueue(args, store, Path("/unused"), Mock(), Mock()))
        self.assertEqual(len(requests), 1)

    def test_failed_publication_stays_pending_and_retry_completes_once(self):
        request = {"schemaVersion": 1, "kind": "hosted", "sourceRun": 12,
                   "sourceCommit": COMMIT, "bundles": [bundle()]}
        store = Mock()
        store.pending.return_value = [("e" * 64, request)]
        with patch.object(ingest, "publish", side_effect=StateError("interrupted")):
            with self.assertRaises(StateError):
                publication.drain(SimpleNamespace(), store)
        store.put.assert_not_called()
        with patch.object(ingest, "publish"):
            publication.drain(SimpleNamespace(), store)
        store.put.assert_called_once_with("completed/" + "e" * 64 + ".json",
                                          canonical({"request": "e" * 64}), immutable=True)


if __name__ == "__main__":
    unittest.main()
