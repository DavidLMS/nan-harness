"""Portable contract tests for the Codex diagnostic report reader."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("codex_diagnostic", ROOT / "actions" / "codex_diagnostic.py")
diagnostic = importlib.util.module_from_spec(spec)
spec.loader.exec_module(diagnostic)


def report():
    evidence = {"event": "exited", "capture": "pipe", "rootExit": {"kind": "code", "value": 0},
                "stdout": {"eof": "observed", "atMilliseconds": 1},
                "stderr": {"eof": "observed", "atMilliseconds": 1},
                "termination": {"before": {"attempted": False, "result": "not_needed"},
                                "after": {"attempted": False, "result": "not_needed"}},
                "cleanup": {"stage": "none"}, "marker": {"observed": True},
                "provider": {"requests": 0, "roundTrip": False},
                "readers": {"stdout": "eof", "stderr": "eof"},
                "afterCleanup": {"stdout": {"eof": "unknown", "atMilliseconds": None},
                                 "stderr": {"eof": "unknown", "atMilliseconds": None}},
                "survivors": {"atFailure": {"scan": "not_needed", "names": [], "count": 0},
                              "residual": {"scan": "available", "names": [], "count": 0}}}
    cases = [{"id": case, "status": "passed", "reason": "none", "durationMilliseconds": 1,
              "evidence": evidence} for case in diagnostic.CASE_IDS]
    return {"schemaVersion": 1, "harness": "codex", "cases": cases,
            "overall": {"executed": 11, "passed": 11, "failed": 0, "blocked": 0},
            "durationMilliseconds": 11}


class CodexDiagnosticTests(unittest.TestCase):
    def test_status_reason_and_missing_evidence_fail_closed(self):
        for change in ({"reason": "launch_failed"}, {"status": "failed"}):
            value = report()
            value["cases"][0].update(change)
            with self.assertRaises(diagnostic.UnsafeReport):
                diagnostic.safe_view(value)
        value = report()
        del value["cases"][0]["evidence"]
        with self.assertRaises(diagnostic.UnsafeReport):
            diagnostic.safe_view(value)

    def test_prepare_requires_success_marker_and_unique_private_native_executable(self):
        with tempfile.TemporaryDirectory() as directory:
            cell = Path(directory)
            setup = cell / "setup.json"
            native = cell / "home/.npm-global/node_modules/@openai/codex/vendor/codex.exe"
            native.parent.mkdir(parents=True)
            native.touch()
            marker = cell / "installer-result.json"
            module = SimpleNamespace(isolated_environment=lambda _: {}, resolver_module=lambda: None,
                                     resolve_one=lambda *a, **k: (SimpleNamespace(version="0.1.0"), None),
                                     run_bounded=lambda *a, **k: (0, "exit"))
            with patch.object(diagnostic, "_windows_diagnostic_module", return_value=module):
                for status, expected in (("failed", 2), ("passed", 0)):
                    marker.write_text(json.dumps({"status": status}), encoding="utf-8")
                    self.assertEqual(diagnostic.prepare_codex(cell, setup), expected)
                self.assertEqual(json.loads(setup.read_text())["executable"], str(native.resolve()))
                second = native.parent / "other/codex.exe"
                second.parent.mkdir()
                second.touch()
                self.assertEqual(diagnostic.prepare_codex(cell, setup), 2)

    def test_all_fixed_cases_are_required_and_rendered(self):
        view = diagnostic.safe_view(report())
        text = diagnostic.render(view)
        for case in diagnostic.CASE_IDS:
            self.assertIn(case, text)

    def test_individual_failure_does_not_hide_later_cases(self):
        value = report()
        value["cases"][0].update(status="failed", reason="launch_failed")
        value["cases"][1].update(status="blocked", reason="deadline_exceeded")
        value["overall"] = {"executed": 10, "passed": 9, "failed": 1, "blocked": 1}
        view = diagnostic.safe_view(value)
        self.assertEqual([item["id"] for item in view["cases"]], list(diagnostic.CASE_IDS))

    def test_missing_case_duplicate_case_and_bad_totals_rejected(self):
        value = report()
        value["cases"][-1]["id"] = value["cases"][0]["id"]
        with self.assertRaises(diagnostic.UnsafeReport):
            diagnostic.safe_view(value)
        value = report()
        value["overall"]["passed"] = 10
        with self.assertRaises(diagnostic.UnsafeReport):
            diagnostic.safe_view(value)

    def test_duplicate_json_fields_rejected(self):
        raw = '{"schemaVersion":1,"harness":"codex","harness":"codex","cases":[]}'
        with self.assertRaises(diagnostic.UnsafeReport):
            diagnostic.load_report(Path(self._write(raw)))

    def test_nonfinite_json_numbers_rejected(self):
        raw = '{"schemaVersion":1,"harness":"codex","cases":[],"overall":{},"durationMilliseconds":NaN}'
        with self.assertRaises(diagnostic.UnsafeReport):
            diagnostic.load_report(Path(self._write(raw)))

    def test_boolean_schema_version_rejected(self):
        value = report()
        value["schemaVersion"] = True
        with self.assertRaises(diagnostic.UnsafeReport):
            diagnostic.safe_view(value)

    def test_typed_evidence_rejects_unknown_fields_arrays_and_secrets(self):
        for bad in ({"message": "secret"}, {"observed": ["secret"]}, {"observed": True, "path": "secret"}):
            value = report()
            value["cases"][0]["evidence"] = {"marker": bad}
            with self.assertRaises(diagnostic.UnsafeReport) as raised:
                diagnostic.safe_view(value)
            self.assertNotIn("secret", str(raised.exception))

    def test_attribution_evidence_is_closed_and_rendered(self):
        value = report()
        value["cases"][3]["evidence"].update(
            readers={"stdout": "open", "stderr": "open"},
            afterCleanup={"stdout": {"eof": "observed", "atMilliseconds": 4200},
                          "stderr": {"eof": "observed", "atMilliseconds": 4201}},
            survivors={"atFailure": {"scan": "available", "names": ["codex.exe"], "count": 1},
                       "residual": {"scan": "available", "names": [], "count": 0}})
        text = diagnostic.render(diagnostic.safe_view(value))
        self.assertIn("readers=stdout:open,stderr:open", text)
        self.assertIn("afterCleanup=stdout:observed,stderr:observed", text)
        self.assertIn("survivors.atFailure=available:1(codex.exe)", text)
        self.assertIn("survivors.residual=available:0(-)", text)

    def test_attribution_evidence_rejects_paths_unknown_states_and_bad_counts(self):
        for change in ({"readers": {"stdout": "waiting", "stderr": "eof"}},
                       {"readers": {"stdout": "eof"}},
                       {"survivors": {"atFailure": {"scan": "unknown", "names": [], "count": 0},
                                      "residual": {"scan": "available", "names": [], "count": 0}}},
                       {"survivors": {"atFailure": {"scan": "available",
                                                    "names": ["C:\\Users\\runner\\codex.exe"], "count": 1},
                                      "residual": {"scan": "available", "names": [], "count": 0}}},
                       {"survivors": {"atFailure": {"scan": "available", "names": ["codex.exe"], "count": 0},
                                      "residual": {"scan": "available", "names": [], "count": 0}}},
                       {"afterCleanup": {"stdout": {"eof": "observed", "atMilliseconds": None},
                                         "stderr": {"eof": "unknown", "atMilliseconds": None}}}):
            value = report()
            value["cases"][0]["evidence"].update(change)
            with self.assertRaises(diagnostic.UnsafeReport) as raised:
                diagnostic.safe_view(value)
            self.assertNotIn("runner", str(raised.exception))

    def test_timeout_checkpoint_evidence_is_closed(self):
        value = report()
        value["cases"][9].update(status="blocked", reason="deadline_exceeded")
        value["overall"] = {"executed": 10, "passed": 10, "failed": 0, "blocked": 1}
        value["cases"][9]["evidence"] = {"event": "timed_out", "capture": "pipe", "rootExit": {"kind": "unavailable"},
            "stdout": {"eof": "not_observed", "atMilliseconds": None}, "stderr": {"eof": "unknown", "atMilliseconds": None},
            "termination": {"before": {"attempted": True, "result": "succeeded"},
                            "after": {"attempted": True, "result": "succeeded"}},
            "cleanup": {"stage": "capture_timeout", "osErrorCode": 232},
            "marker": {"observed": False}, "provider": {"requests": 0, "roundTrip": False},
                "readers": {"stdout": "open", "stderr": "open"},
                "afterCleanup": {"stdout": {"eof": "unknown", "atMilliseconds": None},
                                 "stderr": {"eof": "unknown", "atMilliseconds": None}},
                "survivors": {"atFailure": {"scan": "available", "names": ["ping.exe"], "count": 1},
                              "residual": {"scan": "available", "names": [], "count": 0}}}
        view = diagnostic.safe_view(value)
        self.assertEqual(view["cases"][9]["evidence"]["cleanup"]["stage"], "capture_timeout")

    def test_main_keeps_malformed_report_private(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            report_path = root / "report.json"
            output = root / "summary.md"
            report_path.write_text(json.dumps({"secret": "prompt"}), encoding="utf-8")
            self.assertEqual(diagnostic.main(["--report", str(report_path), "--output", str(output)]), 2)
            self.assertNotIn("prompt", output.read_text(encoding="utf-8"))

    @staticmethod
    def _write(text):
        handle = tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False)
        handle.write(text)
        handle.close()
        return handle.name


if __name__ == "__main__":
    unittest.main()
