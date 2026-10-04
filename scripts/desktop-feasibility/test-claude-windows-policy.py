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

    def test_chat_only_is_explicit_and_does_not_admit_other_hosts_or_modes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            helper = root / 'helper'
            helper.write_text('synthetic')
            source = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Windows',
                          NANH_DESKTOP_QUALIFICATION_MODE='startup-baseline',
                          NANH_CLAUDE_WINDOWS_PROFILE_POLICY='private-env', NANH_CLAUDE_WINDOWS_CHAT_ONLY='1',
                          FEASIBILITY_WINDOWS_PROOF_PYTHON=str(helper), FEASIBILITY_WINDOWS_PROOF_SCRIPT=str(helper))
            with patch.object(runner, 'validate_claude_windows_bundle'):
                def invoke(values=source, app='claude-desktop'):
                    return runner.qualification_environment(app, root, helper, root / 'app/Claude.exe', values)
                self.assertEqual(invoke()['NANH_CLAUDE_WINDOWS_CHAT_ONLY'], '1')
                plain = {key: value for key, value in source.items() if key != 'NANH_CLAUDE_WINDOWS_CHAT_ONLY'}
                self.assertNotIn('NANH_CLAUDE_WINDOWS_CHAT_ONLY', invoke(plain))
                for change in ({'RUNNER_OS': 'macOS'}, {'GITHUB_ACTIONS': 'false'},
                               {'RUNNER_ENVIRONMENT': 'self-hosted'}, {'NANH_CLAUDE_WINDOWS_CHAT_ONLY': '0'},
                               {'NANH_DESKTOP_QUALIFICATION_MODE': 'renderer'},
                               {'NANH_CLAUDE_WINDOWS_PROFILE_POLICY': None}):
                    with self.assertRaises(ValueError): invoke({**source, **change})
                with self.assertRaises(ValueError): invoke(source, 'chatgpt-desktop')

    def test_native_chat_requires_paired_fresh_profile_and_derives_source_authority(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            helper = root / 'helper'
            helper.write_text('synthetic')
            source = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Windows',
                          NANH_DESKTOP_QUALIFICATION_MODE='renderer',
                          NANH_CLAUDE_WINDOWS_PROFILE_POLICY='private-env', NANH_CLAUDE_WINDOWS_CHAT_ONLY='1',
                          NANH_CLAUDE_WINDOWS_NATIVE_CHAT='1', NANH_CLAUDE_WINDOWS_FRESH_PROFILE='1',
                          FEASIBILITY_WINDOWS_PROOF_PYTHON=str(helper), FEASIBILITY_WINDOWS_PROOF_SCRIPT=str(helper))
            with patch.object(runner, 'validate_claude_windows_bundle'):
                def invoke(values=source, app='claude-desktop'):
                    return runner.qualification_environment(app, root, helper, root / 'app/Claude.exe', values)
                result = invoke()
                self.assertEqual(result['NANH_DESKTOP_QUALIFICATION_MODE'], 'startup-baseline')
                self.assertEqual(result['NANH_CLAUDE_WINDOWS_FRESH_PROFILE'], '1')
                self.assertNotIn('NANH_CLAUDE_WINDOWS_SOURCE_POLICY', result)
                trial = {**source, 'NANH_CLAUDE_WINDOWS_PERSIST_POLICY': 'std-rename'}
                self.assertEqual(invoke(trial)['NANH_CLAUDE_WINDOWS_PERSIST_POLICY'], 'std-rename')
                for change in ({'RUNNER_OS': 'macOS'}, {'NANH_CLAUDE_WINDOWS_FRESH_PROFILE': None},
                               {'NANH_CLAUDE_WINDOWS_CHAT_ONLY': None},
                               {'NANH_CLAUDE_WINDOWS_PERSIST_POLICY': 'unknown'}):
                    with self.assertRaises(ValueError): invoke({**trial, **change})
                with self.assertRaises(ValueError): invoke(trial, 'chatgpt-desktop')
                for change in ({'RUNNER_OS': 'macOS'}, {'NANH_CLAUDE_WINDOWS_FRESH_PROFILE': None},
                               {'NANH_CLAUDE_WINDOWS_NATIVE_CHAT': None}, {'NANH_CLAUDE_WINDOWS_CHAT_ONLY': None},
                               {'NANH_CLAUDE_WINDOWS_PROFILE_POLICY': None},
                               {'NANH_CLAUDE_WINDOWS_SOURCE_POLICY': 'official-2.19675.0-97910a066871'}):
                    with self.assertRaises(ValueError): invoke({**source, **change})
                with self.assertRaises(ValueError): invoke(source, 'chatgpt-desktop')

    def test_windows_http_fixture_forwards_only_opt_in_under_native_profile_policy(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            helper = root / 'helper'
            helper.write_text('synthetic')
            source = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Windows',
                          NANH_DESKTOP_QUALIFICATION_MODE='startup-baseline',
                          NANH_CLAUDE_WINDOWS_PROFILE_POLICY='private-env', NANH_CLAUDE_WINDOWS_CHAT_ONLY='1',
                          NANH_CLAUDE_WINDOWS_NATIVE_CHAT='1', NANH_CLAUDE_WINDOWS_FRESH_PROFILE='1',
                          NANH_CLAUDE_WINDOWS_MCP_FIXTURE='read-only',
                          FEASIBILITY_WINDOWS_PROOF_PYTHON=str(helper), FEASIBILITY_WINDOWS_PROOF_SCRIPT=str(helper))
            with patch.object(runner, 'validate_claude_windows_bundle'):
                def invoke(values=source, app='claude-desktop'):
                    return runner.qualification_environment(app, root, helper, root / 'app/Claude.exe', values)
                result = invoke()
                self.assertEqual(result['NANH_CLAUDE_WINDOWS_MCP_FIXTURE'], 'read-only')
                self.assertEqual(result['NANH_DESKTOP_QUALIFICATION_MODE'], 'startup-baseline')
                for key in ('NANH_CLAUDE_WINDOWS_MCP_URL', 'NANH_CLAUDE_MCP_SCRIPT',
                            'NANH_CLAUDE_MCP_PYTHON', 'NANH_CLAUDE_MCP_SOURCE_SHA256'):
                    self.assertNotIn(key, result)
                plain = {key: value for key, value in source.items()
                         if key != 'NANH_CLAUDE_WINDOWS_MCP_FIXTURE'}
                self.assertNotIn('NANH_CLAUDE_WINDOWS_MCP_FIXTURE', invoke(plain))
                # Native policy already maps renderer requests to the startup backend.
                self.assertEqual(invoke({**source, 'NANH_DESKTOP_QUALIFICATION_MODE': 'renderer'})[
                    'NANH_DESKTOP_QUALIFICATION_MODE'], 'startup-baseline')
                for key, value in (
                        ('RUNNER_OS', 'Linux'), ('GITHUB_ACTIONS', 'false'),
                        ('RUNNER_ENVIRONMENT', 'self-hosted'),
                        ('NANH_DESKTOP_QUALIFICATION_MODE', 'unknown'),
                        ('NANH_CLAUDE_WINDOWS_MCP_FIXTURE', 'unknown'),
                        ('NANH_CLAUDE_WINDOWS_FRESH_PROFILE', None),
                        ('NANH_CLAUDE_WINDOWS_NATIVE_CHAT', None),
                        ('NANH_CLAUDE_WINDOWS_CHAT_ONLY', None),
                        ('NANH_CLAUDE_WINDOWS_PROFILE_POLICY', None),
                        ('NANH_CLAUDE_MCP_FIXTURE', 'read-only'),
                        ('NANH_CLAUDE_LINUX_MCP_FIXTURE', 'read-only'),
                        ('NANH_CLAUDE_WINDOWS_MCP_URL', 'http://127.0.0.1:1234/mcp'),
                        ('NANH_CLAUDE_WINDOWS_MCP_URL', '')):
                    with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                        invoke({**source, key: value})
                for app in ('chatgpt-desktop', 'zed-desktop', 'hermes-desktop'):
                    with self.subTest(app=app), self.assertRaises(ValueError):
                        invoke(source, app)
                with self.assertRaisesRegex(ValueError, 'URL must be derived'):
                    invoke({**plain, 'NANH_CLAUDE_WINDOWS_MCP_URL': 'http://127.0.0.1:1234/mcp'})

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
                # Match the actual pinned public MSIX entry, including lowercase.
                official_case = executable.with_name('claude.exe')
                executable.rename(official_case)
                runner.validate_claude_windows_bundle(official_case)
                official_case.rename(executable)
                alias = root / 'alias' / 'app' / 'claude.exe'
                alias.parent.mkdir(parents=True)
                alias.symlink_to(executable)
                with self.assertRaisesRegex(ValueError, '^claude-windows-executable-invalid$'):
                    runner.validate_claude_windows_bundle(alias)
                with self.assertRaisesRegex(ValueError, '^claude-windows-executable-invalid$'):
                    runner.validate_claude_windows_bundle(executable.parent / '..' / 'app' / executable.name)
                asar.write_bytes(archive[:-1])
                with self.assertRaisesRegex(ValueError, '^claude-windows-bootstrap-mismatch$'):
                    runner.validate_claude_windows_bundle(executable)
            asar.write_bytes(archive)
            with self.assertRaisesRegex(ValueError, '^claude-windows-bootstrap-mismatch$'):
                runner.validate_claude_windows_bundle(executable)
            for malformed in (b'bad', struct.pack('<4I', 4, 10, 6, 2) + b'{}'):
                asar.write_bytes(malformed)
                with self.assertRaisesRegex(ValueError, '^claude-windows-bootstrap-invalid$'):
                    runner.validate_claude_windows_bundle(executable)
            with self.assertRaisesRegex(ValueError, '^claude-windows-executable-invalid$'):
                runner.validate_claude_windows_bundle(root / 'Other.exe')


if __name__ == '__main__':
    unittest.main()
