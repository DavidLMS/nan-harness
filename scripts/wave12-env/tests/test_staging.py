"""Staging-only contracts: no environment driver, GUI, or real checker runs."""

import hashlib
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "staging", Path(__file__).resolve().parents[1] / "stage_artifacts.py")
staging = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(staging)


class StagingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.source = self.root / "source"
        self.source.mkdir()
        self.destination = self.root / "published"
        self.checker = self.root / "synthetic-checker"
        self.fields = dict(source_commit="unknown", condition="baseline",
                           run_url="none", probe_exit="1", report="validated",
                           occlusion="absent", docks_changed="0",
                           prior_state="none", restore_status="none")
        self.payload = b'{"synthetic":true}\n'
        self.bind_report(self.payload)
        (self.source / "probe-status.txt").write_text("probe_exit=1\n")
        self.write_metadata()

    def bind_report(self, payload):
        digest = hashlib.sha256(payload).hexdigest()
        (self.source / "report.json").write_bytes(payload)
        (self.source / "report.sha256").write_text(digest)
        self.fields["report_sha256"] = digest

    def write_metadata(self):
        (self.source / "evidence.txt").write_text(
            "".join(key + "=" + value + "\n" for key, value in self.fields.items()))
        (self.source / "probe-status.txt").write_text(
            "probe_exit=" + self.fields.get("probe_exit", "1") + "\n")

    def validator(self, args, **kwargs):
        self.assertEqual(args[:2], [str(self.checker), "validate-report"])
        snapshot = Path(args[2])
        self.assertNotEqual(snapshot.parent, self.source)
        self.assertEqual(snapshot.parent.stat().st_mode & 0o777, 0o700)
        self.assertEqual(kwargs["timeout"], 30)
        self.assertEqual(kwargs["stderr"], subprocess.DEVNULL)
        payload = snapshot.read_bytes()
        if payload != self.payload:
            return subprocess.CompletedProcess(args, 1)
        return subprocess.CompletedProcess(
            args, 0, stdout=hashlib.sha256(payload).hexdigest().encode() + b"\n")

    def run_stage(self):
        staging.stage(self.source, self.destination, self.checker)

    def refused(self):
        with patch.object(staging.subprocess, "run", side_effect=self.validator):
            with self.assertRaises((ValueError, OSError, subprocess.SubprocessError)):
                self.run_stage()
        self.assertFalse(self.destination.exists())
        self.assertEqual(list(self.root.glob(".wave12-stage-*")), [])

    def test_validated_snapshot_and_no_private_path_in_manifest(self):
        with patch.object(staging.subprocess, "run", side_effect=self.validator) as call:
            self.run_stage()
        call.assert_called_once()
        self.assertEqual((self.destination / "report.json").read_bytes(), self.payload)
        self.assertNotIn(str(self.source), (self.destination / "staged.txt").read_text())

    def test_source_changed_after_snapshot_does_not_change_publication(self):
        def change_source(args, **kwargs):
            (self.source / "report.json").write_text("synthetic-private-marker")
            return self.validator(args, **kwargs)
        with patch.object(staging.subprocess, "run", side_effect=change_source):
            self.run_stage()
        self.assertEqual((self.destination / "report.json").read_bytes(), self.payload)

    def test_stale_validated_label_does_not_authorize_changed_bytes(self):
        (self.source / "report.status").write_text("validated\n")
        (self.source / "report.json").write_text("synthetic-private-marker")
        self.refused()

    def test_matching_hashes_do_not_replace_schema_validation(self):
        self.bind_report(b"synthetic-private-marker")
        self.write_metadata()
        self.refused()

    def test_private_metadata_and_duplicate_fields_refused(self):
        for extra in ("raw=synthetic-private-marker\n", "probe_exit=1\n",
                      "run_url=https://github.com/a/b/actions/runs/1?secret=value\n"):
            with self.subTest(extra=extra):
                self.write_metadata()
                with (self.source / "evidence.txt").open("a") as stream:
                    stream.write(extra)
                self.refused()

    def test_private_value_in_allowed_field_refused(self):
        self.fields["run_url"] = "https://synthetic-private-marker.invalid"
        self.write_metadata()
        self.refused()

    def test_shell_entry_point_does_not_echo_rejected_metadata(self):
        self.fields["run_url"] = "https://synthetic-private-marker.invalid"
        self.write_metadata()
        wrapper = Path(__file__).resolve().parents[1] / "stage-artifacts.sh"
        result = subprocess.run(
            ["bash", str(wrapper), str(self.source), str(self.destination), str(self.checker)],
            capture_output=True, timeout=5, check=False)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, b"")
        self.assertEqual(result.stderr, b"wave12 artifact staging refused\n")
        self.assertFalse(self.destination.exists())

    def test_probe_mismatch_refused(self):
        (self.source / "probe-status.txt").write_text("probe_exit=0\n")
        self.refused()

    def test_digest_file_mismatch_refused(self):
        (self.source / "report.sha256").write_text("0" * 64)
        self.refused()

    def test_missing_digest_refused(self):
        (self.source / "report.sha256").unlink()
        self.refused()

    def test_symlinked_artifact_refused(self):
        report = self.source / "report.json"
        report.unlink()
        report.symlink_to(self.root / "absent")
        self.refused()

    def test_fifo_refused_without_waiting_for_a_writer(self):
        report = self.source / "report.json"
        report.unlink()
        os.mkfifo(report)
        self.refused()

    def test_missing_source_with_symlinked_destination_refused(self):
        self.source = self.root / "absent"
        self.destination.symlink_to(self.root / "other-absent")
        self.refused()

    def test_existing_destination_preserved(self):
        self.destination.mkdir()
        marker = self.destination / "existing"
        marker.write_text("keep")
        with self.assertRaises(ValueError):
            self.run_stage()
        self.assertEqual(marker.read_text(), "keep")

    def test_timeout_publishes_nothing(self):
        with patch.object(staging.subprocess, "run",
                          side_effect=subprocess.TimeoutExpired("synthetic", 30)):
            with self.assertRaises(subprocess.TimeoutExpired):
                self.run_stage()
        self.assertFalse(self.destination.exists())

    def test_invalid_optional_diagnostic_prevents_partial_publication(self):
        payload = b"synthetic-private-marker"
        digest = hashlib.sha256(payload).hexdigest()
        self.fields.update(occlusion="validated", occlusion_sha256=digest)
        self.write_metadata()
        (self.source / "occlusion.json").write_bytes(payload)
        (self.source / "occlusion.sha256").write_text(digest)
        commands = []

        def reject_diagnostic(args, **kwargs):
            commands.append(args[1])
            if args[1] == "validate-occlusion":
                return subprocess.CompletedProcess(args, 1)
            return self.validator(args, **kwargs)

        with patch.object(staging.subprocess, "run", side_effect=reject_diagnostic):
            with self.assertRaises(ValueError):
                self.run_stage()
        self.assertEqual(commands, ["validate-report", "validate-occlusion"])
        self.assertFalse(self.destination.exists())
        self.assertEqual(list(self.root.glob(".wave12-stage-*")), [])

    def test_absent_source_has_empty_manifest(self):
        self.source = self.root / "absent"
        with patch.object(staging.subprocess, "run") as call:
            self.run_stage()
        call.assert_not_called()
        self.assertEqual([p.name for p in self.destination.iterdir()], ["staged.txt"])

    def test_invalid_artifact_is_not_read_or_staged(self):
        self.fields["report"] = "invalid"
        del self.fields["report_sha256"]
        self.write_metadata()
        (self.source / "report.json").write_text("synthetic-private-marker")
        with patch.object(staging.subprocess, "run") as call:
            self.run_stage()
        call.assert_not_called()
        self.assertFalse((self.destination / "report.json").exists())

    def test_restoration_failure_is_published_as_uncertainty(self):
        self.fields.update(condition="dock-hidden", docks_changed="1",
                           prior_state="false", restore_status="failed")
        self.write_metadata()
        with patch.object(staging.subprocess, "run", side_effect=self.validator):
            self.run_stage()
        evidence = (self.destination / "evidence.txt").read_text()
        self.assertIn("restore_status=failed\n", evidence)
        self.assertIn("prior_state=false\n", evidence)

    def test_unknown_prior_state_publishes_evidence_only(self):
        self.fields.update(condition="dock-hidden", prior_state="unreadable",
                           probe_exit="not-run", report="absent")
        del self.fields["report_sha256"]
        self.write_metadata()
        with patch.object(staging.subprocess, "run") as call:
            self.run_stage()
        call.assert_not_called()
        self.assertEqual(sorted(path.name for path in self.destination.iterdir()),
                         ["evidence.txt", "probe-status.txt", "staged.txt"])
        self.assertIn("prior_state=unreadable\n",
                      (self.destination / "evidence.txt").read_text())

    def test_contradictory_closed_facts_refused(self):
        original = dict(self.fields)
        for case in (
                dict(docks_changed="1", restore_status="ok"),
                dict(restore_status="ok"),
                dict(condition="dock-hidden"),
                dict(condition="dock-hidden", prior_state="false"),
                dict(condition="dock-hidden", prior_state="false", docks_changed="1"),
                dict(condition="dock-hidden", prior_state="unreadable",
                     docks_changed="1", restore_status="ok"),
                dict(probe_exit="not-run")):
            with self.subTest(case=case):
                self.fields = dict(original, **case)
                self.write_metadata()
                self.refused()

    def test_missing_restoration_facts_refused(self):
        original = dict(self.fields)
        for key in ("prior_state", "restore_status"):
            with self.subTest(key=key):
                self.fields = dict(original)
                del self.fields[key]
                self.write_metadata()
                self.refused()


if __name__ == "__main__":
    os.umask(0o077)
    unittest.main()
