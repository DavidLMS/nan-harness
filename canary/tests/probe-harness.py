#!/usr/bin/env python3
"""Synthetic contracts for the guest live probe wrapper."""

from pathlib import Path
import json
import importlib.util
import os
import re
import stat
import subprocess
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
PROBE = ROOT / "canary/guest/probe-harness.sh"
CELL_SPEC = importlib.util.spec_from_file_location("probe_cell_contract", ROOT / "canary/actions/cell.py")
CELL = importlib.util.module_from_spec(CELL_SPEC)
CELL_SPEC.loader.exec_module(CELL)
HARNESSES = (
    "claude-code", "codex", "opencode", "hermes", "pi", "omp", "prime-agent",
    "deepseek-harness", "openclaw", "cline", "qwen-code", "kimi-code", "aider",
    "goose", "fx",
)


FAKE_NANH = r'''#!/usr/bin/env python3
import json
import os
import re
import sys
from pathlib import Path

args = sys.argv[1:]
if args and args[0] == "__media":
    if os.environ.get("NAN_CANARY_FAKE_MODE") == "media-" + args[1]:
        raise SystemExit(1)
    if "--output" in args:
        output = Path(args[args.index("--output") + 1])
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_bytes(b"synthetic media output")
    raise SystemExit(0)
model = args[args.index("--model") + 1]
with open(os.environ["NAN_CANARY_MODEL_LOG"], "a", encoding="utf-8") as log:
    log.write(model + "\n")
if "--dry-run" in args:
    if os.environ.get("NAN_CANARY_FAKE_MODE") == "media-plan":
        raise SystemExit(1)
    print(json.dumps({"media": ["nan-whisper", "nan-kokoro", "image_gen/nan_harness"]}))
    raise SystemExit(0)
if os.environ.get("NAN_CANARY_FAKE_MODE") == "providerfailure":
    print("synthetic secret should be redacted", file=sys.stderr)
    raise SystemExit(17)
subcommand = args[0]
if subcommand == "aider":
    Path("edit-target.txt").write_text("AIDER_CANARY_TOOL_OK\n", encoding="utf-8")
    if os.environ.get("NAN_CANARY_FAKE_MODE", "").startswith("aidercategory-stdout-nonempty"):
        print("synthetic private output")
if subcommand in {"codex", "hermes", "prime", "dsh"}:
    text = " ".join(args)
    match = re.search(r"(?:> '|> |create '|to ')(/[^ '\"]+)", text)
    if match:
        target = Path(match.group(1))
        value = {"codex": "NAN_CODEX_TOOL_OK", "hermes": "NAN_HERMES_TOOL_OK",
                 "prime": "NAN_PRIME_TOOL_OK", "dsh": "NAN_DEEPSEEK_TOOL_OK"}[subcommand]
        target.write_text(value + "\n", encoding="utf-8")
if subcommand == "openclaw":
    print('{')
    print('"meta":{"toolSummary":{"calls":1,"failures":0,"tools":["read"]}}')
    print('}')
    print("NAN_CANARY_OK")
elif os.environ.get("NAN_CANARY_FAKE_MODE") != "toolfailure":
    outputs = {
        "claude": '{"name":"Read"}', "opencode": '{"tool":"read"}',
        "pi": '{"toolName":"read"}', "omp": '{"toolName":"read"}',
        "cline": "read_files", "qwen": '{"name":"read_file"}',
        "kimi": "Read", "goose": '{"name":"shell"}',
    }
    if subcommand in outputs:
        print(outputs[subcommand])
    if subcommand == "fx":
        target = re.search(r"(/[^ ']+/read-target\.txt)", " ".join(args))
        print("Reading " + (target.group(1) if target else "synthetic target"))
    target = re.search(r"(/[^ ']+/read-target\.txt)", " ".join(args))
    if target:
        print(Path(target.group(1)).read_text(encoding="utf-8").strip())
    if not (subcommand == "aider" and
            (os.environ.get("NAN_CANARY_FAKE_MODE", "").startswith("aidercategory") or
             os.environ.get("NAN_CANARY_FAKE_MODE", "") == "aidercompletionmissing")):
        print("NAN_CANARY_OK")
usage = Path(os.environ["NAN_HARNESS_INTERNAL_CANARY_USAGE_FILE"])
usage.parent.mkdir(parents=True, exist_ok=True)
usage.write_text('{"schemaVersion":1,"status":"observed"}\n', encoding="utf-8")
if os.environ.get("NAN_CANARY_FAKE_MODE", "") not in {
        "aidercategory-stdout-empty-stderr-empty",
        "aidercategory-stdout-nonempty-stderr-empty"}:
    print("NaN usage (synthetic)", file=sys.stderr)
'''


FAKE_JQ = r'''#!/usr/bin/env python3
import json
import sys
path = sys.argv[-1]
value = json.loads(open(path, encoding="utf-8").read())
if "openclaw-output" in path:
    assert value["meta"]["toolSummary"] == {"calls": 1, "failures": 0, "tools": ["read"]}
elif "media-plan" in path:
    assert value["media"] == ["nan-whisper", "nan-kokoro", "image_gen/nan_harness"]
else:
    assert value == {"schemaVersion": 1, "status": "observed"}
'''


class ProbeHarnessTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self._script(self.bin / "nanh", FAKE_NANH)
        self._script(self.bin / "jq", FAKE_JQ)

    def tearDown(self):
        self.temp.cleanup()

    def _script(self, path, content):
        path.write_text(content, encoding="utf-8")
        path.chmod(0o700)

    def run_probe(self, harness, *, model="synthetic-model", mode="success", marker=True,
                  extra_path=None):
        marker_path = self.root / "probe-result.json"
        env = os.environ.copy()
        env.update({
            "PATH": str(self.bin) + ":" + (extra_path or "") + ":" + env["PATH"],
            "NAN_CANARY_NAN_COMMAND": str(self.bin / "nanh"),
            "NAN_CANARY_MODEL_LOG": str(self.root / "models.log"),
            "NAN_CANARY_FAKE_MODE": mode,
            "NAN_CANARY_REDACT_FAILURE_OUTPUT": "1",
            "NAN_CANARY_MEDIA_MODE": "weekly",
        })
        if model is not None:
            env["NAN_CANARY_MODEL"] = model
        if marker:
            env["NAN_CANARY_PROBE_RESULT"] = str(marker_path)
        result = subprocess.run(["bash", str(PROBE), harness], env=env,
                                capture_output=True, text=True, timeout=30)
        return result, marker_path

    def test_all_harnesses_pass_with_selected_model_and_closed_marker(self):
        for harness in HARNESSES:
            with self.subTest(harness=harness):
                result, marker = self.run_probe(harness)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(marker.read_text()),
                                 {"schemaVersion": 1, "stage": "complete", "status": "passed"})
                self.assertEqual(CELL.probe_result(marker),
                                 {"schemaVersion": 1, "stage": "complete", "status": "passed"})
                self.assertEqual(stat.S_IMODE(marker.stat().st_mode), stat.S_IWRITE | stat.S_IREAD)
        self.assertEqual((self.root / "models.log").read_text().splitlines(), ["synthetic-model"] * 17)

    def test_failures_close_at_failing_stage_without_raw_output(self):
        for mode, stage in (("providerfailure", "harness-run"), ("toolfailure", "tool-evidence"),
                            ("aidercompletionmissing", "completion-marker")):
            with self.subTest(mode=mode):
                harness = "aider" if mode == "aidercompletionmissing" else "claude-code"
                result, marker = self.run_probe(harness, mode=mode)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(json.loads(marker.read_text())["stage"], stage)
                self.assertEqual(json.loads(marker.read_text())["status"], "failed")
                self.assertNotIn("synthetic secret", result.stderr)

    def test_media_failures_identify_the_operation(self):
        for harness in ("hermes", "openclaw"):
            for stage in ("media-plan", "media-tts", "media-stt", "media-image"):
                with self.subTest(harness=harness, stage=stage):
                    result, marker = self.run_probe(harness, mode=stage)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(CELL.probe_result(marker)["stage"], stage)
                    self.assertIn(stage, CELL.WINDOWS_PROBE_STAGES)

    def test_aider_completion_failure_reports_safe_fixed_diagnostic(self):
        result, marker = self.run_probe("aider", mode="aidercompletionmissing")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("aider-completion-marker-stdout-empty-stderr-nonempty", result.stderr)
        self.assertEqual(json.loads(marker.read_text())["diagnostic"],
                         "aider-completion-marker-stdout-empty-stderr-nonempty")

    def test_probe_diagnostics_are_closed_and_legacy_markers_remain_valid(self):
        valid = tuple(CELL.PROBE_DIAGNOSTICS)
        for diagnostic in valid:
            marker = self.root / (diagnostic + ".json")
            marker.write_text(json.dumps({"schemaVersion": 1, "stage": "completion-marker",
                                          "status": "failed", "diagnostic": diagnostic}))
            self.assertEqual(CELL.probe_result(marker)["diagnostic"], diagnostic)
        malformed = self.root / "malformed.json"
        malformed.write_text(json.dumps({"schemaVersion": 1, "stage": "completion-marker",
                                         "status": "failed", "diagnostic": "secret-fragment"}))
        self.assertIsNone(CELL.probe_result(malformed))
        legacy = self.root / "legacy.json"
        legacy.write_text(json.dumps({"schemaVersion": 1, "stage": "completion-marker",
                                      "status": "failed"}))
        self.assertEqual(CELL.probe_result(legacy)["status"], "failed")
        for missing in ("schemaVersion", "stage", "status"):
            incomplete = {"schemaVersion": 1, "stage": "completion-marker", "status": "failed"}
            incomplete.pop(missing)
            legacy.write_text(json.dumps(incomplete))
            self.assertIsNone(CELL.probe_result(legacy))
        for malformed in (
            {"schemaVersion": 1, "stage": [], "status": "failed"},
            {"schemaVersion": 1, "stage": "completion-marker", "status": {}},
            {"schemaVersion": 1, "stage": "completion-marker", "status": "failed", "diagnostic": {}},
        ):
            legacy.write_text(json.dumps(malformed))
            self.assertIsNone(CELL.probe_result(legacy))

    def test_cleanup_failure_has_priority(self):
        rm = self.bin / "rm"
        self._script(rm, "#!/usr/bin/env bash\nexit 1\n")
        sleep = self.bin / "sleep"
        self._script(sleep, "#!/usr/bin/env bash\nexit 0\n")
        result, marker = self.run_probe("claude-code", extra_path=str(self.bin))
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(json.loads(marker.read_text())["stage"], "cleanup")

    def test_shell_cleanup_failure_after_aider_diagnostic_discards_diagnostic(self):
        rm = self.bin / "rm"
        self._script(rm, "#!/usr/bin/env bash\nexit 1\n")
        sleep = self.bin / "sleep"
        self._script(sleep, "#!/usr/bin/env bash\nexit 0\n")
        result, marker = self.run_probe("aider", mode="aidercompletionmissing",
                                        extra_path=str(self.bin))
        self.assertNotEqual(result.returncode, 0)
        value = json.loads(marker.read_text())
        self.assertEqual(value, {"schemaVersion": 1, "stage": "cleanup", "status": "failed"})

    def test_marker_write_failure_fails_without_marker(self):
        missing_parent = self.root / "missing" / "probe-result.json"
        result, marker = self.run_probe("claude-code")
        marker.unlink()
        env = os.environ.copy()
        env.update({"PATH": str(self.bin) + ":" + env["PATH"],
                    "NAN_CANARY_NAN_COMMAND": str(self.bin / "nanh"),
                    "NAN_CANARY_MODEL_LOG": str(self.root / "models.log"),
                    "NAN_CANARY_PROBE_RESULT": str(missing_parent)})
        failed = subprocess.run(["bash", str(PROBE), "claude-code"], env=env,
                                capture_output=True, text=True, timeout=30)
        self.assertNotEqual(failed.returncode, 0)
        self.assertFalse(missing_parent.exists())

        self._script(self.bin / "mv", "#!/usr/bin/env bash\nexit 1\n")
        result, marker = self.run_probe("claude-code")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(marker.exists())
        self.assertEqual(list(self.root.glob(".probe-result.*")), [])

    def test_hosted_path_keeps_incoming_tools_without_legacy_overrides(self):
        path_log = self.root / "path.log"
        self._script(self.bin / "nanh", FAKE_NANH.replace(
            'args = sys.argv[1:]',
            'args = sys.argv[1:]\nPath(os.environ["NAN_CANARY_PATH_LOG"]).write_text(os.environ["PATH"], encoding="utf-8")'))
        env = os.environ.copy()
        env.update({"PATH": str(self.bin) + ":trusted-node:/usr/bin:/bin",
                    "NAN_CANARY_PATH_LOG": str(path_log),
                    "NAN_CANARY_NAN_COMMAND": str(self.bin / "nanh"),
                    "NAN_CANARY_MODEL_LOG": str(self.root / "models.log"),
                    "NAN_CANARY_HOSTED": "1"})
        result = subprocess.run(["/bin/bash", str(PROBE), "claude-code"], env=env,
                                capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        path = path_log.read_text()
        self.assertIn("trusted-node", path)
        self.assertNotIn("/opt/homebrew/bin", path)
        self.assertNotIn("/usr/local/bin", path)

    def test_tart_path_retains_legacy_overrides(self):
        path_log = self.root / "path.log"
        self._script(self.bin / "nanh", FAKE_NANH.replace(
            'args = sys.argv[1:]',
            'args = sys.argv[1:]\nPath(os.environ["NAN_CANARY_PATH_LOG"]).write_text(os.environ["PATH"], encoding="utf-8")'))
        env = os.environ.copy()
        env.update({"PATH": str(self.bin) + ":trusted-node:/usr/bin:/bin",
                    "NAN_CANARY_PATH_LOG": str(path_log),
                    "NAN_CANARY_NAN_COMMAND": str(self.bin / "nanh"),
                    "NAN_CANARY_MODEL_LOG": str(self.root / "models.log")})
        result = subprocess.run(["/bin/bash", str(PROBE), "claude-code"], env=env,
                                capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        path = path_log.read_text()
        self.assertIn("trusted-node", path)
        self.assertIn("/opt/homebrew/bin", path)
        self.assertIn("/usr/local/bin", path)

    def test_marker_is_optional_and_legacy_model_defaults_are_preserved(self):
        result, marker = self.run_probe("claude-code", model=None, marker=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(marker.exists())
        self.assertEqual((self.root / "models.log").read_text().strip(), "qwen3.6")

    def test_cell_live_preserves_probe_stage_and_exit_status_without_mocks(self):
        cell_root = self.root / "cell"
        cell_root.mkdir()
        fake_key = "synthetic-provider-key"
        env = os.environ.copy()
        env.update({"NAN_API_KEY": fake_key, "NAN_CANARY_FAKE_MODE": "providerfailure",
                    "NAN_CANARY_MODEL_LOG": str(self.root / "models.log")})
        fake = self.bin / "nanh"
        self._script(fake, FAKE_NANH)
        args = type("Args", (), {"directory": cell_root, "binary": fake,
                                  "harness": "codex", "model": "qwen3.6"})()
        previous = os.environ.copy()
        os.environ.update(env)
        try:
            with self.assertRaises(CELL.ProbeFailure) as failure:
                CELL.live(args, {})
        finally:
            os.environ.clear()
            os.environ.update(previous)
        self.assertEqual(failure.exception.stage, "harness-run")
        self.assertEqual(failure.exception.status, 17)

    def test_cell_live_retains_aider_output_category_from_private_marker(self):
        cell_root = self.root / "aider-cell"
        cell_root.mkdir()
        fake_key = "synthetic-provider-key"
        previous = os.environ.copy()
        os.environ.update({"NAN_API_KEY": fake_key, "NAN_CANARY_FAKE_MODE": "aidercompletionmissing",
                           "NAN_CANARY_MODEL_LOG": str(self.root / "aider-models.log")})
        fake = self.bin / "nanh"
        try:
            with self.assertRaises(CELL.ProbeFailure) as failure:
                CELL.live(type("Args", (), {"directory": cell_root, "binary": fake,
                                             "harness": "aider", "model": "qwen3.6"})(), {})
        finally:
            os.environ.clear()
            os.environ.update(previous)
        self.assertEqual(failure.exception.stage, "completion-marker")
        self.assertEqual(failure.exception.diagnostic,
                         "aider-completion-marker-stdout-empty-stderr-nonempty")

    def test_cell_live_and_report_cover_all_aider_output_categories(self):
        categories = {
            "aidercategory-stdout-empty-stderr-empty": "aider-completion-marker-stdout-empty-stderr-empty",
            "aidercategory-stdout-empty-stderr-nonempty": "aider-completion-marker-stdout-empty-stderr-nonempty",
            "aidercategory-stdout-nonempty-stderr-empty": "aider-completion-marker-stdout-nonempty-stderr-empty",
            "aidercategory-stdout-nonempty-stderr-nonempty": "aider-completion-marker-stdout-nonempty-stderr-nonempty",
        }
        for mode, diagnostic in categories.items():
            with self.subTest(mode=mode):
                directory = self.root / mode
                directory.mkdir()
                (directory / "state.json").write_text(json.dumps({
                    "startedAt": CELL.timestamp(), "durationMilliseconds": 0,
                    "checks": [{"name": "install-and-diagnose", "status": "passed"},
                               {"name": "deterministic-conformance", "status": "passed"}],
                    "outcome": "passed"}))
                previous = os.environ.copy()
                os.environ.update({"NAN_API_KEY": "synthetic-provider-key",
                                   "NAN_CANARY_FAKE_MODE": mode,
                                   "NAN_CANARY_MODEL_LOG": str(self.root / (mode + ".log"))})
                try:
                    with self.assertRaises(CELL.ProbeFailure) as failure:
                        CELL.live(type("Args", (), {"directory": directory, "binary": self.bin / "nanh",
                                                     "harness": "aider", "model": "qwen3.6"})(), {})
                finally:
                    os.environ.clear()
                    os.environ.update(previous)
                self.assertEqual(failure.exception.diagnostic, diagnostic)
                args = type("Args", (), {"directory": directory, "output": directory / "report.json",
                                          "stage": "live", "trigger": "manual", "model": "qwen3.6",
                                          "harness": "aider", "canary": self.root / "canary"})()
                with mock.patch.object(CELL, "private_command", return_value=0):
                    CELL.failed_report(args, failure.exception)
                report = json.loads(args.output.read_text())
                self.assertEqual(report["failure"]["code"], f"live-{diagnostic}-exit-1")
                self.assertNotIn("synthetic-provider-key", args.output.read_text())

    def test_failed_report_projects_probe_stage_and_exit_as_safe_code(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            directory = root / "cell"
            directory.mkdir()
            (directory / "state.json").write_text(json.dumps({
                "startedAt": CELL.timestamp(), "durationMilliseconds": 0,
                "checks": [{"name": "install-and-diagnose", "status": "passed"},
                           {"name": "deterministic-conformance", "status": "passed"}],
                "outcome": "passed",
            }))
            args = type("Args", (), {"directory": directory, "output": root / "report.json",
                                      "stage": "live", "trigger": "manual", "model": "qwen3.6",
                                      "harness": "codex", "canary": root / "canary"})()
            with mock.patch.object(CELL, "private_command", return_value=0):
                CELL.failed_report(args, CELL.ProbeFailure("harness-run", 17))
            report = json.loads(args.output.read_text())
            self.assertEqual(report["failure"]["code"], "live-harness-run-exit-17")
            self.assertIn("harness-run", report["failure"]["summary"])
            self.assertIn("exit status 17", report["failure"]["summary"])
            for diagnostic in ("live-child-launch", "live-credential-missing"):
                with mock.patch.object(CELL, "private_command", return_value=0):
                    CELL.failed_report(args, CELL.ProbeFailure("harness-run", 1, diagnostic))
                report = json.loads(args.output.read_text())
                self.assertEqual(report["failure"]["code"], diagnostic + "-exit-1")
            with mock.patch.object(CELL, "private_command", return_value=0):
                CELL.failed_report(args, CELL.ProbeFailure("harness-run", 1, "private-provider-key"))
            self.assertNotIn("private-provider-key", args.output.read_text())

    def test_failed_report_maps_all_aider_output_categories_without_reflection(self):
        categories = tuple(CELL.PROBE_DIAGNOSTICS)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for diagnostic in categories:
                directory = root / diagnostic
                directory.mkdir()
                (directory / "state.json").write_text(json.dumps({
                    "startedAt": CELL.timestamp(), "durationMilliseconds": 0,
                    "checks": [{"name": "install-and-diagnose", "status": "passed"},
                               {"name": "deterministic-conformance", "status": "passed"}],
                    "outcome": "passed",
                }))
                args = type("Args", (), {"directory": directory, "output": directory / "report.json",
                                          "stage": "live", "trigger": "manual", "model": "qwen3.6",
                                          "harness": "aider", "canary": root / "canary"})()
                with mock.patch.object(CELL, "private_command", return_value=0):
                    CELL.failed_report(args, CELL.ProbeFailure("completion-marker", 1, diagnostic))
                report = json.loads(args.output.read_text())
                self.assertEqual(report["failure"]["code"], f"live-{diagnostic}-exit-1")
                self.assertNotIn("secret", args.output.read_text())

    def test_diagnostic_code_requires_aider_completion_exit_one(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            directory = root / "cell"
            directory.mkdir()
            (directory / "state.json").write_text(json.dumps({
                "startedAt": CELL.timestamp(), "durationMilliseconds": 0,
                "checks": [{"name": "install-and-diagnose", "status": "passed"},
                           {"name": "deterministic-conformance", "status": "passed"}],
                "outcome": "passed"}))
            args = type("Args", (), {"directory": directory, "output": root / "report.json",
                                      "stage": "live", "trigger": "manual", "model": "qwen3.6",
                                      "harness": "aider", "canary": root / "canary"})()
            mismatch = CELL.ProbeFailure(
                "completion-marker", 2,
                "aider-completion-marker-stdout-empty-stderr-empty")
            with mock.patch.object(CELL, "private_command", return_value=0):
                CELL.failed_report(args, mismatch)
            self.assertEqual(json.loads(args.output.read_text())["failure"]["code"],
                             "live-completion-marker-exit-2")

    def test_cleanup_failure_overrides_probe_diagnostic(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            directory = root / "cell"
            directory.mkdir()
            (directory / "state.json").write_text(json.dumps({
                "startedAt": CELL.timestamp(), "durationMilliseconds": 0,
                "checks": [{"name": "install-and-diagnose", "status": "passed"},
                           {"name": "deterministic-conformance", "status": "passed"}],
                "outcome": "passed",
            }))
            args = type("Args", (), {"directory": directory, "output": root / "report.json",
                                      "stage": "live", "trigger": "manual", "model": "qwen3.6",
                                      "harness": "aider", "canary": root / "canary"})()
            with mock.patch.object(CELL, "private_command", return_value=0):
                CELL.failed_report(args, CELL.ProbeCleanupError("cleanup"))
            report = json.loads(args.output.read_text())
            self.assertEqual(report["failure"]["phase"], "cleanup")
            self.assertNotIn("code", report["failure"])


if __name__ == "__main__":
    unittest.main()
