#!/usr/bin/env python3
"""Source-pinned Windows observations leave the runtime and profile unchanged."""
import importlib.util
from pathlib import Path
import tempfile
import hashlib
import json
import struct
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('runner', Path(__file__).with_name('run-qualification.py'))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class Policy(unittest.TestCase):
    def test_private_environment_policy_requires_exact_host_platform_mode_and_bootstrap(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            helper = root / 'ownership-helper'
            helper.write_text('synthetic')
            source = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Windows',
                          NANH_DESKTOP_QUALIFICATION_MODE='startup-baseline',
                          NANH_CLAUDE_WINDOWS_PROFILE_POLICY='private-env',
                          FEASIBILITY_WINDOWS_PROOF_PYTHON=str(helper), FEASIBILITY_WINDOWS_PROOF_SCRIPT=str(helper))
            def environment(app='claude-desktop', values=None):
                return runner.qualification_environment(app, root, helper, root / 'app/Claude.exe', values or source)
            with patch.object(runner, 'validate_claude_windows_bundle') as validate:
                result = environment()
                validate.assert_called_once_with(root / 'app/Claude.exe')
                self.assertEqual(result['NANH_CLAUDE_WINDOWS_PROFILE_POLICY'], 'private-env')
                self.assertNotIn('CLAUDE_USER_DATA_DIR', result)
                for changes in ({'RUNNER_OS':'macOS'}, {'NANH_DESKTOP_QUALIFICATION_MODE':'renderer'},
                                {'NANH_CLAUDE_WINDOWS_PROFILE_POLICY':'native'}, {'GITHUB_ACTIONS':'false'}):
                    with self.assertRaises(ValueError):
                        environment(values={**source, **changes})
                with self.assertRaises(ValueError):
                    environment(app='chatgpt-desktop')
            with self.assertRaises(ValueError):
                environment()

    def test_bounded_asar_reader_binds_the_bootstrap_and_rejects_truncation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            executable = root / 'app/Claude.exe'
            executable.parent.mkdir()
            executable.write_bytes(b'synthetic-executable')
            asar = root / 'app/resources/app.asar'
            asar.parent.mkdir()
            data = b'synthetic-public-bootstrap'
            index = json.dumps({'files': {'.vite': {'files': {'build': {'files': {
                'index.pre.js': {'offset': '0', 'size': len(data)}}}}}}}).encode()
            header_size = len(index) + 8
            archive = struct.pack('<4I', 4, header_size, len(index) + 4, len(index)) + index + data
            asar.write_bytes(archive)
            with patch.object(runner, 'CLAUDE_WINDOWS_BOOTSTRAP_SHA256', hashlib.sha256(data).hexdigest()):
                runner.validate_claude_windows_bundle(executable)
                asar.write_bytes(archive[:-1])
                with self.assertRaises(ValueError):
                    runner.validate_claude_windows_bundle(executable)
            asar.write_bytes(archive)
            with self.assertRaises(ValueError):
                runner.validate_claude_windows_bundle(executable)


if __name__ == '__main__':
    unittest.main()
