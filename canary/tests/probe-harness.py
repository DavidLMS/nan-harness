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
model = args[args.index("--model") + 1]
with open(os.environ["NAN_CANARY_MODEL_LOG"], "a", encoding="utf-8") as log:
    log.write(model + "\n")
if os.environ.get("NAN_CANARY_FAKE_MODE") == "providerfailure":
    print("synthetic secret should be redacted", file=sys.stderr)
    raise SystemExit(17)
subcommand = args[0]
if subcommand == "aider":
    Path("edit-target.txt").write_text("AIDER_CANARY_TOOL_OK\n", encoding="utf-8")
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
    print("NAN_CANARY_OK")
usage = Path(os.environ["NAN_HARNESS_INTERNAL_CANARY_USAGE_FILE"])
usage.parent.mkdir(parents=True, exist_ok=True)
usage.write_text('{"schemaVersion":1,"status":"observed"}\n', encoding="utf-8")
print("NaN usage (synthetic)", file=sys.stderr)
'''


FAKE_JQ = r'''#!/usr/bin/env python3
import json
import sys
path = sys.argv[-1]
value = json.loads(open(path, encoding="utf-8").read())
if "openclaw-output" in path:
    assert value["meta"]["toolSummary"] == {"calls": 1, "failures": 0, "tools": ["read"]}
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
        self.assertEqual((self.root / "models.log").read_text().splitlines(), ["synthetic-model"] * 15)

    def test_failures_close_at_failing_stage_without_raw_output(self):
        for mode, stage in (("providerfailure", "harness-run"), ("toolfailure", "tool-evidence")):
            with self.subTest(mode=mode):
                result, marker = self.run_probe("claude-code", mode=mode)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(json.loads(marker.read_text())["stage"], stage)
                self.assertEqual(json.loads(marker.read_text())["status"], "failed")
                self.assertNotIn("synthetic secret", result.stderr)

    def test_cleanup_failure_has_priority(self):
        rm = self.bin / "rm"
        self._script(rm, "#!/usr/bin/env bash\nexit 1\n")
        sleep = self.bin / "sleep"
        self._script(sleep, "#!/usr/bin/env bash\nexit 0\n")
        result, marker = self.run_probe("claude-code", extra_path=str(self.bin))
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(json.loads(marker.read_text())["stage"], "cleanup")

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


if __name__ == "__main__":
    unittest.main()
