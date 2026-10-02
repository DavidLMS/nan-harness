"""Offline contracts for immutable ZCode builds and their complete probe path."""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("zcode_source", ROOT / "canary/guest/zcode-source.py")
source = importlib.util.module_from_spec(spec)
spec.loader.exec_module(source)


class SourceContracts(unittest.TestCase):
    def test_binding_preserves_startup_and_refuses_source_drift(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            entries = root / "apps/zcode-cli/packages/cli/src"
            entries.mkdir(parents=True)
            main = entries / "main.ts"
            headless = entries / "prompt-command.ts"
            main.write_text('const exitCode = await run(context, {\n});\nvoid main();\n')
            headless.write_text('      env: appEnv,\n')
            source.bind(root)
            self.assertIn('else {\n  void main();', main.read_text())
            self.assertIn('NAN_HARNESS_ZCODE_PROJECT_CONFIG_FILE', main.read_text())
            self.assertIn('projectConfigPath: deps.projectConfigPath', headless.read_text())
            main.write_text('changed startup')
            with self.assertRaises(ValueError):
                source.bind(root)

    def test_utf8_source_binding_survives_windows_legacy_default_encoding(self):
        read, write = Path.read_text, Path.write_text
        def windows_read(path, encoding=None, **kwargs):
            return read(path, encoding=encoding or "cp1252", **kwargs)
        def windows_write(path, content, encoding=None, **kwargs):
            return write(path, content, encoding=encoding or "cp1252", **kwargs)
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            entries = root / "apps/zcode-cli/packages/cli/src"
            entries.mkdir(parents=True)
            original = "// synthetic UTF-8 ⚁\n"
            (entries / "main.ts").write_text(original + "const exitCode = await run(context, {\n});\nvoid main();\n", encoding="utf-8")
            (entries / "prompt-command.ts").write_text(original + "      env: appEnv,\n", encoding="utf-8")
            with patch.object(Path, "read_text", windows_read), patch.object(Path, "write_text", windows_write):
                source.bind(root)
            for name in ("main.ts", "prompt-command.ts"):
                self.assertEqual((entries / name).read_bytes().decode("utf-8").splitlines()[0], original.strip())

    def test_source_command_finishes_when_a_descendant_retains_standard_handles(self):
        import os
        import signal
        import sys
        import time
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            command = ("import subprocess,sys; child=subprocess.Popen([sys.executable,'-c',"
                       "'import time; time.sleep(10)']); print(child.pid, flush=True)")
            started = time.monotonic()
            output = source.run([sys.executable, "-c", command], root)
            elapsed = time.monotonic() - started
            try:
                self.assertLess(elapsed, 5)
            finally:
                try:
                    os.kill(int(output), signal.SIGTERM)
                except ProcessLookupError:
                    pass

    def test_child_failures_keep_only_closed_stage_and_reason(self):
        import subprocess
        failure = subprocess.CalledProcessError(1, ["private-path"], output="private-output",
                                                stderr="private-secret")
        with patch.object(source.subprocess, "run", side_effect=failure):
            with self.assertRaises(source.SourceFailure) as caught:
                source.run(["private-path"], ROOT, stage="node-install")
        self.assertEqual(caught.exception.stage, "node-install")
        self.assertEqual(caught.exception.reason, "exit-nonzero")
        self.assertNotIn("private", str(caught.exception))

    def test_bad_identity_never_starts_an_installer(self):
        for version, ref in (('latest', 'a' * 40), ('0.16.9', 'main'),
                             ('../0.16.9', 'a' * 40)):
            with patch.object(source, 'run') as run, tempfile.TemporaryDirectory() as tmp:
                with self.assertRaises(ValueError):
                    source.install(version, ref, Path(tmp))
                run.assert_not_called()

    def test_declared_version_and_checked_out_commit_must_match(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(source, 'run', return_value='b' * 40) as run:
            with self.assertRaises(ValueError):
                source.install('0.16.9', 'a' * 40, Path(tmp))
            self.assertFalse(any('npm' in str(call) for call in run.call_args_list))

    def test_check_runs_protocol_sessions_tui_managed_native_and_search_probes_without_live_key(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(source, 'run') as run, \
                patch.dict(source.os.environ, {'NAN_API_KEY': 'synthetic-secret'}):
            root = Path(tmp)
            (root / 'zcode-source.json').write_text(json.dumps({'source': str(root / 'source'),
                                                              'node': str(root / 'node')}))
            source.check(root, root / 'nanh')
            calls = run.call_args_list
            self.assertEqual(len(calls), 7)
            commands = [[str(arg) for arg in call.args[0]] for call in calls]
            self.assertEqual([command[-1] for command in commands[4:6]], ['sessions', 'tui'])
            self.assertIn('integration_probe.py', ' '.join(commands[-1]))
            for call in calls:
                self.assertNotIn('NAN_API_KEY', call.args[2])


if __name__ == '__main__':
    unittest.main()
