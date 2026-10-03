#!/usr/bin/env python3
"""Matrix completeness and fail-closed full-acceptance reducer contracts."""
import copy
import importlib.util
import argparse
from pathlib import Path
import sys
import json
import hashlib
import struct
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'canary/actions'))
import desktop_qualification as q


spec = importlib.util.spec_from_file_location('qualification_runner', ROOT / 'scripts/desktop-feasibility/run-qualification.py')
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class CodexProjectPreflightTests(unittest.TestCase):
    def test_rejected_stage_is_closed_and_never_qualifies(self):
        value = dict(schemaVersion=1, mechanism='codex-project-preflight', diagnosticsOnly=True, stage='workspace')
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'facts.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(tmp, 'chatgpt-desktop'), [value])
            for changes in ({'stage': 'PRIVATE'}, {'stage': None}, {'path': 'PRIVATE'}, {'diagnosticsOnly': False}):
                path.write_text(json.dumps({**value, **changes}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(tmp, 'chatgpt-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(tmp, 'claude-desktop')


class CodexDriverFactsTests(unittest.TestCase):
    def test_closed_driver_facts_reject_private_payloads_and_unproved_retry(self):
        flags = 'endpointOwned targetVerified attached bindingVerified auxiliaryInert codingComposerReady uniqueComposer inputReadback inputSubmitted userTurnObserved responseVerified errorObserved retryControl retryAttempted retryCompleted providerResponseVerified'.split()
        value = {key: False for key in flags}
        value.update(schemaVersion=1, mechanism='codex-renderer-qualification', diagnosticsOnly=True,
                     assistantTurnCount=0, providerGenerationCount=None, errorCategory='composer-unavailable')
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'facts.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(tmp, 'chatgpt-desktop'), [value])
            for changes in ({'assistantText': 'PRIVATE'}, {'errorCategory': 'PRIVATE'},
                            {'assistantTurnCount': True}, {'providerGenerationCount': 4097},
                            {'retryCompleted': True}, {'bindingVerified': 1}):
                path.write_text(json.dumps({**value, **changes}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(tmp, 'chatgpt-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(tmp, 'claude-desktop')


class RunnerTests(unittest.TestCase):
    def test_zed_panel_zoom_diagnostic_is_explicit_linux_only_and_private(self):
        source = {key: 'synthetic' for key in runner.ZED_HELPERS}
        source.update(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted',
                      RUNNER_OS='Linux', NANH_ZED_PANEL_ZOOM='observe',
                      FEASIBILITY_ZED_INPUT_DRIVER_MODE='paste',
                      FEASIBILITY_ZED_RESPONSE_METHOD='thread-export', OPENAI_API_KEY='PRIVATE')
        environment = runner.qualification_environment('zed-desktop', Path('/tmp'), Path('/tmp/nanh'),
                                                       '/tmp/zed', source)
        self.assertEqual(environment['NANH_ZED_PANEL_ZOOM'], 'observe')
        self.assertNotIn('OPENAI_API_KEY', environment)
        source.pop('NANH_ZED_PANEL_ZOOM')
        self.assertNotIn('NANH_ZED_PANEL_ZOOM', runner.qualification_environment(
            'zed-desktop', Path('/tmp'), Path('/tmp/nanh'), '/tmp/zed', source))
        for app, changes in (('zed-desktop', {'RUNNER_OS': 'macOS'}),
                             ('zed-desktop', {'RUNNER_OS': 'Windows'}),
                             ('hermes-desktop', {}), ('zed-desktop', {'NANH_ZED_PANEL_ZOOM': 'activate'})):
            with self.assertRaises(ValueError):
                runner.qualification_environment(app, Path('/tmp'), Path('/tmp/nanh'), '/tmp/zed',
                                                {**source, 'NANH_ZED_PANEL_ZOOM': 'observe', **changes})

    @staticmethod
    def synthetic_claude_bundle(root, payload=b'synthetic bootstrap'):
        contents = root / 'Claude.app/Contents'
        executable = contents / 'MacOS/Claude'
        for document in (executable, contents / 'Info.plist'):
            document.parent.mkdir(parents=True, exist_ok=True)
            document.write_bytes(b'synthetic')
        archive = contents / 'Resources/app.asar'
        archive.parent.mkdir(parents=True, exist_ok=True)
        header = json.dumps({'files': {'.vite': {'files': {'build': {'files': {
            'index.pre.js': {'size': len(payload), 'offset': '0'}}}}}}}, separators=(',', ':')).encode()
        padding = b'\0' * (-len(header) % 4)
        header_size = 8 + len(header) + len(padding)
        archive.write_bytes(struct.pack('<4I', 4, header_size, header_size - 4, len(header))
                            + header + padding + payload)
        return executable, archive

    def test_zed_delivery_policy_is_explicit_and_linux_only(self):
        source = {key: 'synthetic' for key in runner.ZED_HELPERS}
        source.update(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted',
                      RUNNER_OS='Linux', NANH_ZED_XRECORD='1',
                      FEASIBILITY_ZED_INPUT_DRIVER_MODE='paste',
                      FEASIBILITY_ZED_RESPONSE_METHOD='thread-export')
        args = ('zed-desktop', Path('/synthetic/facts'), Path('/synthetic/nanh'), Path('/synthetic/app'))
        env = runner.qualification_environment(*args, inherited=source)
        self.assertEqual(env['NANH_ZED_XRECORD'], '1')
        for changes in ({'RUNNER_OS': 'macOS'}, {'NANH_ZED_XRECORD': 'unknown'}):
            with self.assertRaises(ValueError):
                runner.qualification_environment(*args, inherited={**source, **changes})
        del source['NANH_ZED_XRECORD']
        self.assertNotIn('NANH_ZED_XRECORD', runner.qualification_environment(*args, inherited=source))

    def test_claude_mac_profile_trial_requires_direct_bundle_and_scoped_policy(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            contents = root / 'Claude.app/Contents'
            executable = contents / 'MacOS/Claude'
            executable, archive = self.synthetic_claude_bundle(root)
            source = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='macOS',
                          NANH_DESKTOP_QUALIFICATION_MODE='startup-baseline',
                          NANH_CLAUDE_MAC_PROFILE_POLICY='electron-user-data-dir',
                          CLAUDE_USER_DATA_DIR='PRIVATE', CLAUDE_CDP_AUTH='PRIVATE')
            with patch.object(runner, 'CLAUDE_BOOTSTRAP_SHA256', hashlib.sha256(b'synthetic bootstrap').hexdigest()):
                env = runner.qualification_environment('claude-desktop', root, root / 'nanh', executable, source)
            self.assertEqual(env['NANH_CLAUDE_MAC_PROFILE_POLICY'], 'electron-user-data-dir')
            self.assertNotIn('PRIVATE', str(env))
            for changes, app in (({'RUNNER_OS': 'Linux'}, 'claude-desktop'),
                                 ({'NANH_DESKTOP_QUALIFICATION_MODE': 'renderer'}, 'claude-desktop'),
                                 ({'NANH_CLAUDE_MAC_PROFILE_POLICY': 'unknown'}, 'claude-desktop'),
                                 ({}, 'pen-desktop')):
                with self.assertRaises(ValueError):
                    runner.qualification_environment(app, root, root / 'nanh', executable, {**source, **changes})
            (contents / 'Resources/app.asar').unlink()
            with self.assertRaises(ValueError):
                runner.qualification_environment('claude-desktop', root, root / 'nanh', executable, source)
            (contents / 'Resources/app.asar').symlink_to(contents / 'Info.plist')
            with self.assertRaises(ValueError):
                runner.validate_claude_bundle(executable)

    def test_claude_bootstrap_uses_padded_asar_structure_and_exact_digest(self):
        with tempfile.TemporaryDirectory() as tmp:
            executable, archive = self.synthetic_claude_bundle(Path(tmp).resolve())
            valid = archive.read_bytes()
            expected = hashlib.sha256(b'synthetic bootstrap').hexdigest()
            with self.assertRaises(ValueError):
                runner.validate_claude_bundle(executable)  # Production pin rejects synthetic bytes.
            with patch.object(runner, 'CLAUDE_BOOTSTRAP_SHA256', expected):
                runner.validate_claude_bundle(executable)
                for data in (valid[:15], valid[:-1], struct.pack('<I', 8) + valid[4:],
                             valid[:8] + struct.pack('<I', 1) + valid[12:],
                             valid[:12] + struct.pack('<I', 2 ** 32 - 1) + valid[16:],
                             valid[:-1] + b'X'):
                    archive.write_bytes(data)
                    with self.assertRaises(ValueError):
                        runner.validate_claude_bundle(executable)

    def test_windows_native_ownership_helpers_survive_the_closed_environment(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            python = root / 'python.exe'
            script = root / 'endpoint-owner-windows.py'
            python.write_text('synthetic interpreter')
            script.write_text('synthetic read-only helper')
            source = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Windows',
                          FEASIBILITY_WINDOWS_PROOF_PYTHON=str(python),
                          FEASIBILITY_WINDOWS_PROOF_SCRIPT=str(script), OPENAI_API_KEY='PRIVATE',
                          FEASIBILITY_RETRY_FORCE='PRIVATE')
            for app in ('hermes-desktop', 'chatgpt-desktop', 'claude-desktop', 'pen-desktop'):
                env = runner.qualification_environment(app, root, python, str(python), source)
                for key in runner.WINDOWS_PROOF:
                    self.assertEqual(env[key], source[key])
                self.assertNotIn('PRIVATE', str(env))
            for value in (None, 'relative.py', str(root), str(root / 'absent')):
                invalid = {**source, 'FEASIBILITY_WINDOWS_PROOF_SCRIPT': value}
                with self.assertRaises(ValueError):
                    runner.qualification_environment('claude-desktop', root, python, str(python), invalid)
            link = root / 'symlink.py'
            link.symlink_to(script)
            with self.assertRaises(ValueError):
                runner.qualification_environment('claude-desktop', root, python, str(python),
                    {**source, 'FEASIBILITY_WINDOWS_PROOF_SCRIPT': str(link)})
            env = runner.qualification_environment('claude-desktop', root, python, str(python),
                    {**source, 'RUNNER_OS': 'Linux'})
            for key in runner.WINDOWS_PROOF:
                self.assertNotIn(key, env)

    def test_public_onboarding_opt_in_requires_inspected_codex_host_and_renderer(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            helper = root / 'helper'
            helper.write_text('synthetic helper')
            source = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Windows',
                          FEASIBILITY_WINDOWS_PROOF_PYTHON=str(helper), FEASIBILITY_WINDOWS_PROOF_SCRIPT=str(helper),
                          NANH_CODEX_PUBLIC_ONBOARDING='engineering')
            env = runner.qualification_environment('chatgpt-desktop', root, helper, str(helper), source)
            self.assertEqual(env['NANH_CODEX_PUBLIC_ONBOARDING'], 'engineering')
            linux = runner.qualification_environment('chatgpt-desktop', root, helper, str(helper), {**source, 'RUNNER_OS': 'Linux'})
            self.assertEqual(linux['NANH_CODEX_PUBLIC_ONBOARDING'], 'engineering')
            for changed, app in (({'NANH_CODEX_PUBLIC_ONBOARDING': 'PRIVATE'}, 'chatgpt-desktop'),
                                 ({'RUNNER_OS': 'FreeBSD'}, 'chatgpt-desktop'),
                                 ({'NANH_DESKTOP_QUALIFICATION_MODE': 'startup-baseline'}, 'chatgpt-desktop'),
                                 ({}, 'claude-desktop'), ({}, 'pen-desktop')):
                with self.assertRaises(ValueError):
                    runner.qualification_environment(app, root, helper, str(helper), {**source, **changed})

    def test_closed_environment_excludes_credentials_and_experiment_opt_ins(self):
        source = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Linux',
                      PATH='/synthetic/bin', NAN_API_KEY='PRIVATE', AWS_SECRET_ACCESS_KEY='PRIVATE',
                      GITHUB_TOKEN='PRIVATE', FEASIBILITY_HERMES_DOM_FACTS='/old-experiment',
                      FEASIBILITY_HERMES_STARTUP_ONLY='1', FEASIBILITY_ZED_NATIVE_COPY_FACTS='/old',
                      HERMES_DESKTOP_HERMES='/frozen/venv/bin/hermes')
        env = runner.qualification_environment('hermes-desktop', Path('/facts'), Path('/nanh'), '/app', source)
        state_source = {**source, 'XDG_STATE_HOME': '/private/checker-state'}
        state_env = runner.qualification_environment('hermes-desktop', Path('/facts'), Path('/nanh'), '/app', state_source)
        self.assertEqual(state_env['XDG_STATE_HOME'], '/private/checker-state')
        self.assertNotIn('PRIVATE', str(env))
        self.assertNotIn('/old', str(env))
        self.assertEqual(env['NANH_DESKTOP_QUALIFICATION_FACTS'], '/facts')
        self.assertEqual(env['FEASIBILITY_HERMES_DOM_INPUT'], '1')
        self.assertEqual(env['FEASIBILITY_HERMES_DOM_DRIVER'], str(ROOT / 'scripts/desktop-feasibility/observe-hermes.cjs'))
        self.assertEqual(env['HERMES_DESKTOP_HERMES'], '/frozen/venv/bin/hermes')
        with self.assertRaises(ValueError):
            runner.qualification_environment('hermes-desktop', Path('/facts'), Path('/nanh'), '/app',
                                             {**source, 'RUNNER_ENVIRONMENT': 'self-hosted'})
        with self.assertRaises(ValueError):
            runner.qualification_environment('hermes-desktop', Path('/facts'), Path('/nanh'), '/app',
                                             {**source, 'FEASIBILITY_HERMES_NAMESPACE_POLICY': 'no-sandbox'})
        with self.assertRaises(ValueError):
            runner.qualification_environment('zed-desktop', Path('/facts'), Path('/nanh'), '/app', source)

    def test_claude_native_chat_preserves_owned_profile_policy(self):
        source = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='macOS',
                      NANH_DESKTOP_QUALIFICATION_MODE='renderer', NANH_CLAUDE_MAC_NATIVE_CHAT='1',
                      NANH_CLAUDE_MAC_PROFILE_POLICY='native-known-folders')
        args = ('claude-desktop', Path('/facts'), Path('/nanh'), '/app')
        with patch.object(runner, 'validate_claude_bundle'):
            environment = runner.qualification_environment(*args, inherited=source)
            self.assertEqual(environment['NANH_CLAUDE_MAC_NATIVE_CHAT'], '1')
            self.assertEqual(environment['NANH_DESKTOP_QUALIFICATION_MODE'], 'startup-baseline')
            for changes in ({'RUNNER_OS': 'Linux'}, {'NANH_CLAUDE_MAC_NATIVE_CHAT': '0'},
                            {'NANH_CLAUDE_MAC_PROFILE_POLICY': 'electron-user-data-dir'},
                            {'RUNNER_ENVIRONMENT': 'self-hosted'}, {'NANH_DESKTOP_QUALIFICATION_MODE': 'PRIVATE'}):
                with self.assertRaises(ValueError):
                    runner.qualification_environment(*args, inherited={**source, **changes})
            with self.assertRaises(ValueError):
                runner.qualification_environment('chatgpt-desktop', *args[1:], inherited=source)

    def test_claude_read_fixture_policy_is_mac_owned_and_source_bound(self):
        source = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='macOS',
                      NANH_DESKTOP_QUALIFICATION_MODE='startup-baseline',
                      NANH_CLAUDE_MAC_PROFILE_POLICY='native-known-folders',
                      NANH_CLAUDE_MAC_CHAT_NAVIGATION='1', NANH_CLAUDE_MCP_FIXTURE='read-only',
                      NANH_CLAUDE_MCP_SCRIPT='/untrusted', NANH_CLAUDE_MCP_PYTHON='/untrusted')
        with patch.object(runner, 'validate_claude_bundle'):
            environment = runner.qualification_environment('claude-desktop', Path('/facts'), Path('/nanh'), '/app', source)
            self.assertEqual(environment['NANH_CLAUDE_MCP_SCRIPT'], str(ROOT / 'scripts/desktop-feasibility/claude-read-fixture.py'))
            self.assertNotIn('/untrusted', str(environment))
            for changes in ({'RUNNER_OS': 'Linux'}, {'NANH_CLAUDE_MCP_FIXTURE': 'arbitrary'},
                            {'NANH_CLAUDE_MAC_CHAT_NAVIGATION': '0'},
                            {'NANH_CLAUDE_MAC_PROFILE_POLICY': 'electron-user-data-dir'}):
                with self.assertRaises(ValueError):
                    runner.qualification_environment('claude-desktop', Path('/facts'), Path('/nanh'), '/app', {**source, **changes})
            with patch.object(runner, 'digest', return_value='0' * 64), self.assertRaises(ValueError):
                runner.qualification_environment('claude-desktop', Path('/facts'), Path('/nanh'), '/app', source)

    def trial(self, mutation=None, report_mutation=None):
        with tempfile.TemporaryDirectory() as root:
            directory = Path(root)
            files = {key: directory / key for key in ('checker', 'real_nanh', 'prepared', 'frozen', 'app')}
            for key, path in files.items():
                path.write_text(key)
            launcher = ROOT / 'scripts/desktop-feasibility/nanh-shim.py'
            receipt = dict(schemaVersion=2, platform='linux', architecture='x86_64',
                           checker={'sha256': q.digest(files['checker'])},
                           nanh={'sha256': q.digest(launcher)},
                           frozen={'sha256': q.digest(files['frozen']), 'model': 'qwen3.6'},
                           apps=[dict(app='hermes-desktop', blocked=None, executable={
                               'path': str(files['app']), 'sha256': q.digest(files['app'])})])
            files['prepared'].write_text(json.dumps(receipt))
            args = argparse.Namespace(app='hermes-desktop', platform='linux', source_sha='a' * 40,
                                      directory=directory / 'output', **{k: v for k, v in files.items() if k != 'app'})
            def execute(command, cwd, **kwargs):
                self.assertIn('--verification', command)
                self.assertIn('semantic-only', command)
                self.assertIn('deterministic', command)
                self.assertEqual(kwargs['timeout'], 1200)
                self.assertTrue(kwargs['allow_failure'])
                self.assertNotIn('NAN_API_KEY', kwargs['environment'])
                facts = Path(kwargs['environment']['NANH_DESKTOP_QUALIFICATION_FACTS'])
                self.assertEqual(facts.stat().st_mode & 0o777, 0o700)
                report = dict(schemaVersion=3, platform='linux', architecture='x86_64',
                              nanHarness={'sha256': q.digest(launcher)}, results=[{'app': args.app}])
                if report_mutation: report_mutation(report)
                (args.directory / 'report.json').write_text(json.dumps(report))
                if mutation: mutation(files)
                return 0
            env = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Linux', PATH='/bin', NAN_API_KEY='PRIVATE')
            with patch.dict(runner.os.environ, env, clear=True), patch.object(runner.sys, 'platform', 'linux'), \
                 patch.object(runner, 'read_frozen_manifest', return_value={'apps': [{'status': 'frozen'}]}), patch.object(runner, 'private_command', side_effect=execute), \
                 patch.object(runner.subprocess, 'run') as validate:
                result = runner.run(args)
                self.assertEqual(validate.call_args.kwargs['timeout'], 30)
                self.assertNotIn('NAN_API_KEY', validate.call_args.kwargs['env'])
                return result

    def test_bounded_semantic_only_run_and_frozen_identity(self):
        self.assertEqual(self.trial(), 0)
        with self.assertRaises(ValueError):
            self.trial(lambda files: files['frozen'].write_text('changed'))
        with self.assertRaises(ValueError):
            self.trial(report_mutation=lambda report: report.update(platform='macos'))


class QualificationTests(unittest.TestCase):
    def test_failure_policy_requires_a_disclosed_status_and_explicit_ui_action(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'policy.json'
            value = dict(schemaVersion=1, mechanism='semantic-failure-policy',
                         failureStatus=400, recoveryAction='explicit-ui-retry')
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(Path(root), 'zed-desktop'), [value])
            for changed in ({**value, 'failureStatus': True}, {**value, 'failureStatus': 200},
                            {**value, 'recoveryAction': 'automatic'}, {**value, 'text': 'PRIVATE'}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(Path(root), 'zed-desktop')

    def test_windows_proof_diagnostics_remain_closed(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'proof.json'
            value = dict(schemaVersion=1, mechanism='windows-endpoint-proof', diagnosticsOnly=True,
                         category='transport-timeout')
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(Path(root), 'hermes-desktop'), [value])
            for changed in ({**value, 'category': ['PRIVATE']}, {**value, 'stderr': 'PRIVATE'},
                            {**value, 'category': None}, {**value, 'diagnosticsOnly': False}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(Path(root), 'hermes-desktop')

    def test_backend_failure_categories_never_export_error_text(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'backend.json'
            value = dict(schemaVersion=1, mechanism='hermes-backend-failure', diagnosticsOnly=True,
                         category='python-import-failure', assistantTurnCount=2)
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(Path(root), 'hermes-desktop'), [value])
            for changed in ({**value, 'category': 'PRIVATE'}, {**value, 'text': 'PRIVATE'},
                            {**value, 'assistantTurnCount': True}, {**value, 'diagnosticsOnly': False}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(Path(root), 'hermes-desktop')

    def test_baseline_is_diagnostic_and_cannot_claim_renderer_instrumentation(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'baseline.json'
            value = dict(schemaVersion=1, mechanism='renderer-startup-baseline', diagnosticsOnly=True,
                         windowAcquired=True, rendererInstrumented=False)
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [value])
            for changed in ({**value, 'windowAcquired': False}, {**value, 'rendererInstrumented': True},
                            {**value, 'html': 'PRIVATE'}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')

    def test_baseline_accessibility_inventory_keeps_only_closed_counts(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inventory = dict(appPresent=True, editableCount=1, retryCount=0, loginCount=None)
            value = dict(schemaVersion=1, mechanism='renderer-startup-baseline', diagnosticsOnly=True,
                         windowAcquired=True, rendererInstrumented=False, accessibilityInventory=inventory)
            path = root / 'baseline.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [value])
            for changed in ({**inventory, 'label': 'PRIVATE'}, {**inventory, 'editableCount': True},
                            {**inventory, 'retryCount': 4097}):
                path.write_text(json.dumps({**value, 'accessibilityInventory': changed}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')

    def test_child_startup_receipt_rejects_private_output_and_wrong_app(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'renderer-startup.json'
            value = dict(schemaVersion=1, mechanism='renderer-startup', diagnosticsOnly=True,
                         app='claude-desktop', exitCode=0, stderrPresent=True,
                         captureTruncated=False, startupCategory='unclassified')
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [value])
            for changed in ({**value, 'stderr': 'PRIVATE'}, {**value, 'app': 'pen-desktop'},
                            {**value, 'exitCode': True}, {**value, 'startupCategory': 'PRIVATE'}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')

    def test_stability_counts_are_closed_and_never_accept_window_metadata(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'stability.json'
            counts = dict(observations=2, candidatesPresent=1, candidatesAbsent=1,
                          identityChanges=0, boundsChanges=0, nameChanges=0, stablePairs=0)
            value = dict(schemaVersion=1, mechanism='native-window-stability', diagnosticsOnly=True, counts=counts)
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [value])
            observed = {**value, 'counts': {**counts, 'lastWindowState': 'minimized'}}
            path.write_text(json.dumps(observed))
            self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [observed])
            for changed in ({**value, 'counts': {**counts, 'lastWindowState': 'PRIVATE'}}, {**value, 'title': 'PRIVATE'}, {**value, 'counts': {**counts, 'nameChanges': True}},
                            {**value, 'counts': {**counts, 'candidatesAbsent': 0}}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')

    def test_deferred_pen_scope_is_explicit_and_cannot_hide_missing_active_cells(self):
        cells = q.matrix(['pen-desktop'])['include']
        self.assertEqual(len(cells), 12)
        self.assertEqual({c['app'] for c in cells}, set(q.APPS) - {'pen-desktop'})
        for invalid in (['unknown'], ['pen-desktop', 'pen-desktop'], list(q.APPS)):
            with self.assertRaises(ValueError):
                q.matrix(invalid)
        with tempfile.TemporaryDirectory() as root:
            for index, item in enumerate(cells):
                directory = Path(root) / str(index)
                directory.mkdir()
                (directory / 'qualification.json').write_text(json.dumps(q.envelope(
                    item['app'], item['platform'], item['architecture'], 'a' * 40)))
            result = q.aggregate(root, 'a' * 40, ['pen-desktop'])
            self.assertEqual(result['excludedApps'], ['pen-desktop'])
            self.assertEqual(result['qualification'], 'incomplete')
            with self.assertRaises(ValueError):
                q.aggregate(root, 'a' * 40)
            (Path(root) / '0/qualification.json').unlink()
            with self.assertRaises(ValueError):
                q.aggregate(root, 'a' * 40, ['pen-desktop'])

    def test_runner_failure_keeps_pending_verdict_and_never_exports_exception_text(self):
        with tempfile.TemporaryDirectory() as root:
            directory = Path(root).resolve()
            args = argparse.Namespace(directory=directory, app='chatgpt-desktop',
                                      platform='windows', source_sha='a' * 40)
            value = q.envelope(args.app, args.platform, 'x86_64', args.source_sha)
            output = directory / 'qualification.json'
            output.write_text(json.dumps(value))
            env = {'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted'}
            with patch.dict(runner.os.environ, env, clear=True):
                runner.publish_runner_failure(args, 'PRIVATE_EXCEPTION')
                self.assertEqual(json.loads(output.read_text()), value)
                runner.publish_runner_failure(args, 'codex-project-release-mismatch')
            observed = json.loads(output.read_text())
            self.assertEqual(observed['qualification'], 'unqualified')
            self.assertEqual(observed['reason'], 'not-run')
            self.assertEqual(observed['semanticObservations'][0]['errorCategory'], 'codex-project-release-mismatch')
            self.assertNotIn('PRIVATE_EXCEPTION', output.read_text())
            with patch.dict(runner.os.environ, env, clear=True):
                runner.publish_runner_failure(args, 'execution-failed')
            self.assertEqual(json.loads(output.read_text()), observed)
            facts = directory / 'facts'
            facts.mkdir()
            fact = facts / 'runner.json'
            fact.write_text(json.dumps(observed['semanticObservations'][0]))
            self.assertEqual(q.semantic_observations(facts, args.app), observed['semanticObservations'])
            fact.write_text(json.dumps({**observed['semanticObservations'][0], 'errorCategory': 'PRIVATE_EXCEPTION'}))
            with self.assertRaises(ValueError):
                q.semantic_observations(facts, args.app)

    def test_exact_initial_matrix_separates_scenario_backends_and_inventories(self):
        cells = q.matrix()['include']
        self.assertEqual(len(cells), 15)
        self.assertEqual(len({(c['app'], c['platform'], c['architecture']) for c in cells}), 15)
        self.assertEqual(sum(c['backend'] != 'renderer-inventory' for c in cells), 10)
        self.assertEqual({(c['platform'], c['architecture']) for c in cells},
                         {('linux', 'x86_64'), ('macos', 'aarch64'), ('windows', 'x86_64')})
        with self.assertRaises(ValueError):
            q.envelope('zed-desktop', 'macos', 'x86_64', 'a' * 40)

    def test_pending_is_explicit_unqualified_and_never_publishes_payloads(self):
        value = q.envelope('pen-desktop', 'windows', 'x86_64', 'a' * 40)
        self.assertEqual(value['outcome'], 'blocked')
        self.assertEqual(value['qualification'], 'unqualified')
        self.assertIsNone(value['applicationSha256'])
        self.assertIsNone(value['nativeDiagnosticInvalidEvents'])
        with self.assertRaises(ValueError):
            q.envelope('pen-desktop', 'windows', 'x86_64', 'private source payload')

    def test_aggregate_rejects_missing_duplicate_and_foreign_source(self):
        with tempfile.TemporaryDirectory() as root:
            for index, item in enumerate(q.matrix()['include']):
                directory = Path(root) / str(index)
                directory.mkdir()
                (directory / 'qualification.json').write_text(json.dumps(q.envelope(
                    item['app'], item['platform'], item['architecture'], 'a' * 40)))
            self.assertEqual(q.aggregate(root, 'a' * 40)['qualification'], 'incomplete')
            first = Path(root) / '0/qualification.json'
            value = json.loads(first.read_text())
            for invalid in (True, -1, 'PRIVATE', 4 * 1024 * 1024 + 2):
                first.write_text(json.dumps({**value, 'nativeDiagnosticInvalidEvents': invalid}))
                with self.assertRaises(ValueError):
                    q.aggregate(root, 'a' * 40)
            first.write_text(json.dumps({**value, 'nativeDiagnosticInvalidEvents': 1}))
            cells = q.aggregate(root, 'a' * 40)['cells']
            self.assertEqual(sum(item['nativeDiagnosticInvalidEvents'] == 1 for item in cells), 1)
            first.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.aggregate(root, 'b' * 40)
            (Path(root) / '0/qualification.json').unlink()
            with self.assertRaises(ValueError):
                q.aggregate(root, 'a' * 40)

    def test_semantic_observations_publish_only_closed_progress(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'observation.json'
            value = dict(schemaVersion=1, mechanism='hermes-renderer-qualification',
                         errorCategory='response-timeout', responseVerified=False,
                         providerResponseVerified=True, errorObserved=True, retryControl=True,
                         uniqueComposer=True, inputSubmitted=True, observedRuntimeVersion='144.0.7559.236',
                         privatePrompt='PRIVATE_SYNTHETIC', rawError='PRIVATE_SYNTHETIC')
            path.write_text(json.dumps(value))
            (Path(root) / 'connection-123.json').write_text('PRIVATE_SOCKET_PROTOCOL')
            (Path(root) / 'startup-123.json').write_text('PRIVATE_STARTUP')
            public = q.semantic_observations(root, 'hermes-desktop')
            self.assertNotIn('PRIVATE', str(public))
            self.assertEqual(public[0]['errorCategory'], 'response-timeout')
            self.assertTrue(public[0]['retryControl'])
            for field, invalid in [('errorCategory', 'PRIVATE_SYNTHETIC'), ('uniqueComposer', 1),
                                   ('observedRuntimeVersion', 'PRIVATE_SYNTHETIC')]:
                path.write_text(json.dumps({**value, field: invalid}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'hermes-desktop')
            native = dict(schemaVersion=1, mechanism='zed-native-copy', stage='response-control',
                          substage='retry-control-query', guardKind=None, guardCategory=None,
                          blocker='selector-not-matched', clipboardCleanup='passed',
                          retryControlCount=0, retrySelector='retry-name-or-description', retryActionReceipt='completion-unknown',
                          input={'submitted': True, 'clipboardVerified': True, 'private': 'PRIVATE'},
                          response={'clipboardVerified': False, 'providerVerified': True})
            path.write_text(json.dumps(native))
            public = q.semantic_observations(root, 'zed-desktop')
            self.assertEqual(public[0]['substage'], 'retry-control-query')
            self.assertTrue(public[0]['inputSubmitted'])
            self.assertEqual(public[0]['retryControlCount'], 0)
            self.assertEqual(public[0]['retrySelector'], 'retry-name-or-description')
            for field, invalid in [('retryControlCount', True), ('retryControlCount', 4097),
                                   ('retrySelector', 'PRIVATE_SYNTHETIC'), ('retryActionReceipt', 'PRIVATE')]:
                path.write_text(json.dumps({**native, field: invalid}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')
            path.write_text(json.dumps(native))
            self.assertNotIn('PRIVATE', str(public))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'hermes-desktop')

    def test_native_icon_calibration_prunes_pixels_and_rejects_unbounded_metrics(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'icons.json'
            metrics = dict(contrastPositions=10, foregroundPositions=4, maxCorrelationMilli=980,
                           maxContrastMilli=120000, maxSpreadMilli=24000, pixels='PRIVATE')
            calibration = dict(scaleMilli=1000, grayRange=200, grayStdMilli=25000,
                               retry=metrics, copy=metrics, close=metrics, width=123, pixels='PRIVATE')
            value = dict(schemaVersion=1, mechanism='zed-native-icons', diagnosticsOnly=True,
                         status='complete', stage='completed', reason=None, templateSide=14,
                         calibration=calibration)
            path.write_text(json.dumps(value))
            public = q.semantic_observations(root, 'zed-desktop')[0]['calibration']
            self.assertEqual(public['retry']['maxCorrelationMilli'], 980)
            self.assertNotIn('PRIVATE', str(public))
            self.assertNotIn('width', public)
            for changed in ({**calibration, 'scaleMilli': 1500},
                            {**calibration, 'grayRange': True},
                            {**calibration, 'retry': {**metrics, 'contrastPositions': 4194305}},
                            {**calibration, 'copy': {**metrics, 'maxCorrelationMilli': 1001}}):
                path.write_text(json.dumps({**value, 'calibration': changed}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')

    def test_provider_oracle_and_retry_tooltip_diagnostics_are_closed(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'provider.json'
            oracle = dict(schemaVersion=1, mechanism='semantic-provider-oracle', stage='tool',
                          toolCompleted=True, toolRecordingBounded=True, toolVerified=False,
                          fixtureResponseVerified=True, failureObserved=False, private='PRIVATE')
            path.write_text(json.dumps(oracle))
            for app in ('hermes-desktop', 'zed-desktop'):
                public = q.semantic_observations(root, app)
                self.assertTrue(public[0]['toolCompleted'])
                self.assertFalse(public[0]['toolVerified'])
                self.assertNotIn('PRIVATE', str(public))
            for field, invalid in [('stage', 'PRIVATE'), ('toolVerified', 'PRIVATE'), ('failureObserved', 0)]:
                path.write_text(json.dumps({**oracle, field: invalid}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')
            native = dict(schemaVersion=1, mechanism='zed-native-copy', substage='retry-tooltip-query',
                          retrySelector='retry-tooltip', retryTitleCount=0, retryCandidateCount=6,
                          retryTooltipCount=1, retryLabelCount=None, activationAttempted=True,
                          activationSucceeded=False, privateRole='PRIVATE', privatePid=123, retryInventoryStatus='complete',
                          retryInventoryTotal=20, retryInventoryButtons=3, retryInventoryStaticText=4,
                          retryInventoryTitleMatches=1, retryInventoryGenerationMatches=0, retryInventoryRetryMatches=0)
            path.write_text(json.dumps(native))
            public = q.semantic_observations(root, 'zed-desktop')[0]
            self.assertEqual(public['retryTooltipCount'], 1)
            self.assertEqual(public['retryCandidateCount'], 6)
            self.assertEqual(public['retrySelector'], 'retry-tooltip')
            self.assertEqual(public['retryInventoryStatus'], 'complete')
            self.assertEqual(public['retryInventoryTotal'], 20)
            self.assertTrue(public['activationAttempted'])
            self.assertFalse(public['activationSucceeded'])
            self.assertNotIn('PRIVATE', str(public))
            self.assertNotIn('privatePid', public)
            for field in ('activationAttempted', 'activationSucceeded'):
                path.write_text(json.dumps({**native, field: 1}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')
            for field in ('retryTitleCount', 'retryCandidateCount', 'retryTooltipCount', 'retryLabelCount',
                          'retryInventoryTotal', 'retryInventoryButtons', 'retryInventoryStaticText',
                          'retryInventoryTitleMatches', 'retryInventoryGenerationMatches', 'retryInventoryRetryMatches'):
                for invalid in (True, -1, 4097):
                    path.write_text(json.dumps({**native, field: invalid}))
                    with self.assertRaises(ValueError):
                        q.semantic_observations(root, 'zed-desktop')

    def test_initial_decision_requires_closed_category_from_same_snapshot(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'focus.json'
            value = dict(schemaVersion=1, mechanism='claude-window-focus', diagnosticsOnly=True,
                         status='query-error', nativeForegroundWindowMatchedHeld=None,
                         phase='initial-decision', guardCategory='bounds-changed')
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(Path(root), 'claude-desktop'), [value])
            for change in ({'guardCategory': 'PRIVATE'}, {'guardCategory': True}, {'phase': 'initial'}):
                path.write_text(json.dumps({**value, **change}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(Path(root), 'claude-desktop')
            del value['guardCategory']
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(Path(root), 'claude-desktop')

    def test_main_aux_correlation_is_diagnostic_closed_and_requires_full_proof(self):
        counts = dict(roleLegend=0, roleRadios=0, engineering=0, dialog=0, quickChatComposer=0, editable=0)
        value = dict(schemaVersion=1, mechanism='codex-main-aux-correlation', diagnosticsOnly=True,
                     status='observed', totalPages=2, stableSamples=2,
                     heldMainUnchanged=True, auxRouteMatched=True, mainScopeUnique=True,
                     auxMainControlsAbsent=True, auxComposerAbsent=True, guarded=True,
                     mainDocumentFocused=True, auxDocumentFocused=False,
                     main={**counts, 'roleLegend': 1, 'roleRadios': 11, 'engineering': 1, 'dialog': 1}, aux=counts)
        self.assertEqual(q.main_aux_correlation(value, 'chatgpt-desktop'), value)
        for dialogs in (0, 2):
            separate = {**value, 'main': {**value['main'], 'dialog': dialogs}}
            self.assertEqual(q.main_aux_correlation(separate, 'chatgpt-desktop'), separate)
        for change in ({'status': 'PRIVATE'}, {'stableSamples': True}, {'stableSamples': 1},
                       {'guarded': False}, {'auxDocumentFocused': True}, {'main': None},
                       {'targetId': 'PRIVATE'}, {'aux': {**counts, 'editable': 4097}}):
            with self.assertRaises(ValueError):
                q.main_aux_correlation({**value, **change}, 'chatgpt-desktop')
        with self.assertRaises(ValueError):
            q.main_aux_correlation(value, 'pen-desktop')

    def test_claude_focus_identity_is_diagnostic_and_unknown_is_not_false(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'focus.json'
            value = dict(schemaVersion=1, mechanism='claude-window-focus', diagnosticsOnly=True,
                         status='proved', nativeForegroundWindowMatchedHeld=True)
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(Path(root), 'claude-desktop'), [value])
            for change in ({'status': 'ambiguous'}, {'nativeForegroundWindowMatchedHeld': None},
                           {'windowId': 1}, {'diagnosticsOnly': False}, {'status': 'guessed'}):
                path.write_text(json.dumps({**value, **change}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(Path(root), 'claude-desktop')
            for status in ('no-match', 'ambiguous', 'untrusted', 'not-standard', 'query-error', 'focus-mismatch'):
                closed = {**value, 'status': status, 'nativeForegroundWindowMatchedHeld': None}
                path.write_text(json.dumps(closed))
                self.assertEqual(q.semantic_observations(Path(root), 'claude-desktop'), [closed])
            unknown = {**value, 'status': 'identity-changed', 'nativeForegroundWindowMatchedHeld': None}
            path.write_text(json.dumps(unknown))
            self.assertEqual(q.semantic_observations(Path(root), 'claude-desktop'), [unknown])
            with self.assertRaises(ValueError):
                q.semantic_observations(Path(root), 'zed-desktop')

    def test_claude_focus_query_diagnostics_preserve_failure_and_reject_private_data(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'focus.json'
            value = dict(schemaVersion=1, mechanism='claude-window-focus', diagnosticsOnly=True,
                         status='query-error', nativeForegroundWindowMatchedHeld=None,
                         query=dict(phase='before', stage='input-window', error='attribute-unsupported'))
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(Path(root), 'claude-desktop'), [value])
            for change in ({'phase': 'after'}, {'stage': 'private-label'}, {'error': 123},
                           {'path': '/private/profile'}):
                path.write_text(json.dumps({**value, 'query': {**value['query'], **change}}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(Path(root), 'claude-desktop')
            value['status'] = 'identity-changed'
            value['query']['phase'] = 'after'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(Path(root), 'claude-desktop'), [value])

    def test_focus_agreement_reasons_require_the_corresponding_identity_failure(self):
        focus = dict(schemaVersion=1, mechanism='claude-window-focus', diagnosticsOnly=True,
                     status='identity-changed', nativeForegroundWindowMatchedHeld=None,
                     windowOnlyStatus='identity-changed', windowOnlyMatchedHeld=None)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'facts.json'
            for key, status_key in (('agreement', 'status'), ('windowOnlyAgreement', 'windowOnlyStatus')):
                for reason in ('foreground-changed', 'after-proof-unready', 'window-element-changed', 'geometry-changed'):
                    value = {**focus, key: reason}
                    path.write_text(json.dumps(value))
                    self.assertEqual(q.semantic_observations(tmp, 'claude-desktop'), [value])
                    for change in ({key: 'PRIVATE'}, {key: None}, {key: True},
                                   {status_key: 'query-error'}, {'privatePath': 'PRIVATE'}):
                        path.write_text(json.dumps({**value, **change}))
                        with self.assertRaises(ValueError):
                            q.semantic_observations(tmp, 'claude-desktop')
                value = {**focus, key: 'geometry-changed'}
                del value[status_key]
                path.write_text(json.dumps(value))
                with self.assertRaises(ValueError):
                    q.semantic_observations(tmp, 'claude-desktop')

    def test_independent_window_query_and_owned_stop_are_closed(self):
        focus = dict(schemaVersion=1, mechanism='claude-window-focus', diagnosticsOnly=True,
                     status='proved', nativeForegroundWindowMatchedHeld=True,
                     windowOnlyStatus='query-error', windowOnlyMatchedHeld=None,
                     windowOnlyQuery=dict(phase='before', stage='focused-window', error='cannot-complete'))
        stop = dict(schemaVersion=1, mechanism='windows-owned-stop', diagnosticsOnly=True,
                    wrapperPresent=True, launcherHandleAvailable=True, terminateResult='issued', jobClosed=True)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'facts.json'
            for value in (focus, stop):
                path.write_text(json.dumps(value))
                self.assertEqual(q.semantic_observations(tmp, 'claude-desktop'), [value])
                for changed in ({**value, 'privatePath': 'PRIVATE'}, {**value, 'diagnosticsOnly': False}):
                    path.write_text(json.dumps(changed))
                    with self.assertRaises(ValueError):
                        q.semantic_observations(tmp, 'claude-desktop')
            for changed in ({**focus, 'windowOnlyStatus': 'proved'},
                            {**focus, 'windowOnlyQuery': {'phase': 'before', 'stage': 'PRIVATE', 'error': 'cannot-complete'}},
                            {**stop, 'wrapperPresent': False}, {**stop, 'terminateResult': 'PRIVATE'},
                            {**stop, 'jobClosed': 1}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(tmp, 'claude-desktop')

    def test_claude_focus_phases_are_optional_closed_and_never_promote_failure(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'focus.json'
            value = dict(schemaVersion=1, mechanism='claude-window-focus', diagnosticsOnly=True,
                         status='query-error', nativeForegroundWindowMatchedHeld=None,
                         query=dict(phase='before', stage='focused-element', error='no-value'),
                         windowOnlyStatus='proved', windowOnlyMatchedHeld=True)
            for phase in (None, 'initial', 'final-stability'):
                receipt = value if phase is None else {**value, 'phase': phase}
                path.write_text(json.dumps(receipt))
                self.assertEqual(q.semantic_observations(Path(root), 'claude-desktop'), [receipt])
            for phase in ('PRIVATE', True, [], None):
                path.write_text(json.dumps({**value, 'phase': phase}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(Path(root), 'claude-desktop')
            proved = {**value, 'status': 'proved', 'nativeForegroundWindowMatchedHeld': True,
                      'query': None, 'phase': 'final-stability'}
            path.write_text(json.dumps(proved))
            self.assertEqual(q.semantic_observations(Path(root), 'claude-desktop'), [proved])
            with self.assertRaises(ValueError):
                q.semantic_observations(Path(root), 'zed-desktop')

    def test_claude_window_only_proof_does_not_promote_full_focus_failure(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'focus.json'
            value = dict(schemaVersion=1, mechanism='claude-window-focus', diagnosticsOnly=True,
                         status='query-error', nativeForegroundWindowMatchedHeld=None,
                         query=dict(phase='before', stage='focused-element', error='no-value'),
                         windowOnlyStatus='proved', windowOnlyMatchedHeld=True)
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(Path(root), 'claude-desktop'), [value])
            for change in ({'windowOnlyStatus': 'ambiguous'}, {'windowOnlyMatchedHeld': None},
                           {'windowOnlyStatus': 'PRIVATE'}, {'windowOnlyMatchedHeld': 1}):
                path.write_text(json.dumps({**value, **change}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(Path(root), 'claude-desktop')
            del value['windowOnlyMatchedHeld']
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(Path(root), 'claude-desktop')

    def test_claude_stack_counts_are_closed_and_overflow_is_unknown(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'stack.json'
            value = dict(schemaVersion=1, mechanism='claude-window-stack', diagnosticsOnly=True,
                         status='complete', samePidAheadCount=2, samePidAheadEligibleCount=1,
                         samePidAheadIntersectsHeldCount=0, samePidAheadNormalLayerCount=1,
                         samePidAheadOtherLayerCount=1, foregroundPidMatchesHeld=True,
                         frontmostWindowSamePid=True)
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(Path(root), 'claude-desktop'), [value])
            for change in ({'samePidAheadCount': True}, {'samePidAheadOtherLayerCount': 2},
                           {'samePidAheadEligibleCount': 3}, {'windowId': 1}, {'status': 'overflow'}):
                path.write_text(json.dumps({**value, **change}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(Path(root), 'claude-desktop')
            unknown = {**value, 'status': 'overflow'}
            for key in list(unknown):
                if key.startswith('samePidAhead'): unknown[key] = None
            path.write_text(json.dumps(unknown))
            self.assertEqual(q.semantic_observations(Path(root), 'claude-desktop'), [unknown])
            with self.assertRaises(ValueError):
                q.semantic_observations(Path(root), 'zed-desktop')

    def test_private_retry_policy_has_only_fixed_settings_and_hashes(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'policy.json'
            policy = dict(schemaVersion=1, mechanism='hermes-retry-policy', policy='explicit-ui-retry',
                          autoRecoveryCycles=0, apiMaxRetries=3, configBeforeSha256='a' * 64,
                          configAfterSha256='b' * 64, privateYaml='PRIVATE')
            path.write_text(json.dumps(policy))
            public = q.semantic_observations(root, 'hermes-desktop')[0]
            self.assertEqual(public['autoRecoveryCycles'], 0)
            self.assertEqual(public['apiMaxRetries'], 3)
            self.assertNotIn('PRIVATE', str(public))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'zed-desktop')
            for field, invalid in [('policy', 'PRIVATE'), ('autoRecoveryCycles', False),
                                   ('apiMaxRetries', 1), ('configAfterSha256', 'PRIVATE')]:
                path.write_text(json.dumps({**policy, field: invalid}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'hermes-desktop')

    def test_semantic_inventory_exposes_counts_without_tool_names(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'inventory.json'
            value = dict(schemaVersion=1, mechanism='semantic-inventory', requestCount=1,
                         toolCount=12, knownReadToolCount=0, readToolSelected=False,
                         privateToolNames=['PRIVATE'])
            path.write_text(json.dumps(value))
            for app in ('hermes-desktop', 'zed-desktop'):
                public = q.semantic_observations(root, app)[0]
                self.assertEqual(public['knownReadToolCount'], 0)
                self.assertFalse(public['readToolSelected'])
                self.assertNotIn('PRIVATE', str(public))
            for field, invalid in [('requestCount', True), ('toolCount', -1),
                                   ('knownReadToolCount', 4097), ('readToolSelected', 0)]:
                path.write_text(json.dumps({**value, field: invalid}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'hermes-desktop')

    def test_semantic_observation_bounds_and_symlink_rejection(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'observation.json'
            path.write_text(' ' * 8193)
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'hermes-desktop')
            path.unlink()
            path.symlink_to(Path(root) / 'absent')
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'hermes-desktop')
            path.unlink()
            for index in range(33):
                (Path(root) / f'connection-{index}.json').write_text('{}')
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'hermes-desktop')

    def trial(self, mutate=lambda report: None, policy_count=3):
        with tempfile.TemporaryDirectory() as root:
            paths = {key: Path(root) / key for key in ('checker', 'launcher', 'real_nanh', 'prepared', 'frozen', 'report')}
            for key, path in paths.items():
                path.write_text(key)
            probe = dict(status='passed', steps=sorted(q.STEPS), inputMode='renderer-dom-and-keyboard', responseVerification='renderer-dom')
            report = dict(schemaVersion=3, platform='linux', architecture='x86_64',
                          nanHarness={'sha256': q.digest(paths['launcher'])}, cleanup='passed',
                          results=[dict(app='hermes-desktop', appVersion='0.17.6', cleanup='passed',
                                        deterministic=[copy.deepcopy(probe) for _ in range(3)])])
            mutate(report)
            manifest = {'apps': [dict(status='frozen', app='hermes-desktop', version='0.17.6', revision='b' * 40)]}
            receipt = dict(schemaVersion=2, platform='linux', architecture='x86_64',
                           checker={'sha256': q.digest(paths['checker'])},
                           nanh={'sha256': q.digest(paths['launcher'])},
                           frozen={'sha256': q.digest(paths['frozen']), 'model': 'qwen3.6'}, apps= [dict(app='hermes-desktop', executable={'sha256': 'c' * 64})])
            with patch.object(q, 'read_frozen_manifest', return_value=manifest), \
                 patch.object(q, 'bounded_json', return_value=receipt), \
                 patch.object(q, 'semantic_observations', return_value=[dict(
                     schemaVersion=1, mechanism='hermes-retry-policy', policy='explicit-ui-retry',
                     autoRecoveryCycles=0, apiMaxRetries=3, configBeforeSha256='e' * 64,
                     configAfterSha256='f' * 64) for _ in range(policy_count)]), \
                 patch.object(q, 'validated_report', return_value=(report, 'd' * 64)):
                return q.reduce_report(app='hermes-desktop', platform='linux', architecture='x86_64',
                                       source_sha='a' * 40, model='qwen3.6', **paths)

    def test_full_three_probes_and_cleanup_are_required(self):
        self.assertEqual(self.trial()['qualification'], 'deterministic-full')
        for count in (0, 1, 2, 4):
            self.assertEqual(self.trial(policy_count=count)['qualification'], 'unqualified')
        changes = [lambda r: r['results'][0]['deterministic'].pop(),
                   lambda r: r['results'][0]['deterministic'][0]['steps'].remove('tool-verified'),
                   lambda r: r['results'][0]['deterministic'][0].update(status='skipped'),
                   lambda r: r['results'][0]['deterministic'][0].update(responseVerification='local-ocr'),
                   lambda r: r.update(cleanup='failed'),
                   lambda r: r['results'][0]['deterministic'][0].update(inputMode='native-clipboard-and-keyboard')]
        for change in changes:
            self.assertEqual(self.trial(change)['qualification'], 'unqualified')

    def test_identity_mismatch_rejected_and_private_fields_not_copied(self):
        with self.assertRaises(ValueError):
            self.trial(lambda r: r['nanHarness'].update(sha256='f' * 64))
        with self.assertRaises(ValueError):
            self.trial(lambda r: r['results'][0].update(appVersion='0.17.7'))
        value = self.trial(lambda r: r['results'][0]['deterministic'][0].update(private='PRIVATE_SYNTHETIC'))
        self.assertNotIn('PRIVATE_SYNTHETIC', str(value))


    def test_native_icon_diagnostics_remain_closed_and_cannot_claim_qualification(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'icon.json'
            value = dict(schemaVersion=1, mechanism='zed-native-icons', diagnosticsOnly=True,
                         status='complete', stage='matching', reason=None, templateSide=28, retryMatches=1,
                         copyMatches=1, closeMatches=1, baselineClusters=0, firstClusters=1,
                         secondClusters=1, newStableClusters=1, privateFrame='PRIVATE', x=123)
            path.write_text(json.dumps(value))
            public = q.semantic_observations(root, 'zed-desktop')[0]
            self.assertTrue(public['diagnosticsOnly'])
            self.assertEqual(public['newStableClusters'], 1)
            self.assertEqual(public['stage'], 'matching')
            self.assertNotIn('PRIVATE', str(public))
            self.assertNotIn('x', public)
            for key, invalid in [('diagnosticsOnly', False), ('templateSide', 27),
                                 ('retryMatches', 4097), ('status', 'PRIVATE'), ('stage', 'PRIVATE')]:
                path.write_text(json.dumps({**value, key: invalid}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'hermes-desktop')

    def test_retry_overlay_diagnostics_use_only_closed_categories(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'dom.json'
            value = dict(schemaVersion=1, mechanism='hermes-renderer-qualification',
                         retryRectInViewport=True, retryAncestorClipped=True,
                         retryPointerEventsNone=False, retryHitOwned=False, retryHitOwnedPoints=0, retryPointStable=False,
                         retrySampleStatus="no-owned-point", retryReveal="onboarding-skipped", retryHitAncestor=False,
                         retryHitSharesTurnPair=False, retryHitContainsComposer=False,
                         retryHitTarget='other', retryHitTag='div',
                         retryHitRegion='thread-viewport', className='PRIVATE', rectangle=[1, 2, 3, 4])
            path.write_text(json.dumps(value))
            public = q.semantic_observations(root, 'hermes-desktop')[0]
            self.assertTrue(public['retryAncestorClipped'])
            self.assertEqual(public['retryHitRegion'], 'thread-viewport')
            self.assertNotIn('PRIVATE', str(public))
            self.assertNotIn('rectangle', public)
            for key, invalid in [('retryRectInViewport', 1), ('retryAncestorClipped', 'true'),
                                 ('retryPointerEventsNone', None), ('retryHitTag', 'custom-private-tag'),
                                 ('retryHitRegion', 'PRIVATE'), ('retryPointStable', 1),
                                 ('retryHitOwnedPoints', True), ('retryHitOwnedPoints', 10),
                                 ('retryHitOwnedPoints', -1), ('retrySampleStatus', 'PRIVATE'), ('retryReveal', 'PRIVATE'),
                                 ('retryHitAncestor', 1), ('retryHitContainsComposer', None)]:
                path.write_text(json.dumps({**value, key: invalid}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'hermes-desktop')

    def test_semantic_file_budget_excludes_private_connection_metadata(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            value = dict(schemaVersion=1, mechanism='semantic-inventory', requestCount=1,
                         toolCount=1, knownReadToolCount=1, readToolSelected=True)
            for index in range(30):
                (root / f'fact-{index}.json').write_text(json.dumps(value))
            for prefix in ('connection-', 'startup-'):
                for index in range(3):
                    (root / f'{prefix}{index}.json').write_text('PRIVATE_INVALID_JSON')
            self.assertEqual(len(q.semantic_observations(root, 'hermes-desktop')), 30)
            for index in range(30, 64):
                (root / f'fact-{index}.json').write_text(json.dumps(value))
            self.assertEqual(len(q.semantic_observations(root, 'hermes-desktop')), 64)
            for index in range(64, 65):
                (root / f'fact-{index}.json').write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'hermes-desktop')

    def test_source_fingerprints_publish_bounded_hashes_without_dom_attributes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'front.json'
            value = dict(schemaVersion=1, mechanism='hermes-front-source', diagnosticsOnly=True,
                         levels=[dict(level=0, tokenCount=1, tokenHashes=['a' * 64], className='PRIVATE')],
                         rawText='PRIVATE', coordinates=[1, 2])
            path.write_text(json.dumps(value))
            public = q.semantic_observations(root, 'hermes-desktop')[0]
            self.assertEqual(public['levels'][0]['tokenHashes'], ['a' * 64])
            self.assertNotIn('PRIVATE', str(public))
            self.assertNotIn('coordinates', public)
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'zed-desktop')
            for invalid in [dict(level=1, tokenCount=1, tokenHashes=['a' * 64]),
                            dict(level=0, tokenCount=True, tokenHashes=['a' * 64]),
                            dict(level=0, tokenCount=1, tokenHashes=['PRIVATE']),
                            dict(level=0, tokenCount=2, tokenHashes=['a' * 64, 'a' * 64]),
                            dict(level=0, tokenCount=25, tokenHashes=['a' * 64] * 25)]:
                path.write_text(json.dumps({**value, 'levels': [invalid]}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'hermes-desktop')
            too_many = [dict(level=i, tokenCount=24, tokenHashes=[f'{j:064x}' for j in range(24)]) for i in range(3)]
            path.write_text(json.dumps({**value, 'levels': too_many}))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'hermes-desktop')

    def test_retry_focus_diagnostics_cannot_publish_active_editor_contents(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'dom.json'
            value = dict(schemaVersion=1, mechanism='hermes-renderer-qualification',
                         retryFocusAfterAcquire=False, retryFocusBeforeAction=False,
                         retryButtonConnected=True, retryAncestorHidden=False,
                         retryAncestorInert=False, retryFieldsetDisabled=False, retryDocumentFocused=True,
                         retryActiveTag='div', retryActiveRegion='composer-root', activeText='PRIVATE',
                         activeAttributes={'PRIVATE': 'PRIVATE'})
            path.write_text(json.dumps(value))
            public = q.semantic_observations(root, 'hermes-desktop')[0]
            self.assertEqual(public['retryActiveRegion'], 'composer-root')
            self.assertFalse(public['retryFocusAfterAcquire'])
            self.assertNotIn('PRIVATE', str(public))
            for key, invalid in [('retryFocusAfterAcquire', 1), ('retryDocumentFocused', None),
                                 ('retryActiveTag', 'private-tag'), ('retryActiveRegion', 'PRIVATE')]:
                path.write_text(json.dumps({**value, key: invalid}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'hermes-desktop')

    def test_owned_codex_relaunch_receipts_are_diagnostics_only_and_closed(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'relaunch.json'
            value = dict(schemaVersion=1, mechanism='codex-owned-relaunch', diagnosticsOnly=True, stage='restarted')
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop'), [value])
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'hermes-desktop')
            for changed in ({**value, 'stage': 'PRIVATE'}, {**value, 'marker': 'PRIVATE'},
                            {**value, 'diagnosticsOnly': False}, {**value, 'stage': None}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'chatgpt-desktop')

    def test_startup_facts_reject_raw_fields(self):
        import desktop_startup
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            value = dict(schemaVersion=1, mechanism='hermes-startup', startupCategory='unclassified',
                         namespacePolicy='default', disableSetuidSandbox=False, stderrPresent=True,
                         captureTruncated=False, drainComplete=True, launcherExitCode=1,
                         effectiveUserIsRoot=False, apparmor_restrict_unprivileged_userns=None,
                         unprivileged_userns_clone=None, sandboxHelperPresent=None,
                         sandboxHelperOwnerIsRoot=None, sandboxHelperModeIs4755=None)
            path = root / 'closed-startup-1.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'hermes-desktop'), [value])
            for changed in ({**value, 'stderr': 'PRIVATE'}, {**value, 'launcherExitCode': True},
                            {**value, 'startupCategory': 'PRIVATE'}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'hermes-desktop')

    def test_hermes_action_diagnostics_reject_private_or_untyped_values(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'readiness.json'
            value = dict(schemaVersion=1, mechanism='hermes-windows-catalog-readiness', diagnosticsOnly=True,
                         stage='menu', errorCategory='menu-unavailable', menuOpened=False,
                         refreshAttempted=False, catalogVerified=False, modelRowVerified=False,
                         menuDismissed=False, composerReverified=False,
                         composerObservation=None, guardFailure=None)
            action = dict(action='menu', sampleStatus='no-owned-point', blocker='onboarding')
            item = {**value, 'actionObservation': action}
            path.write_text(json.dumps(item))
            self.assertEqual(q.semantic_observations(root, 'hermes-desktop'), [item])
            skipped = {**item, 'onboardingSkipped': True}
            path.write_text(json.dumps(skipped))
            self.assertEqual(q.semantic_observations(root, 'hermes-desktop'), [skipped])
            path.write_text(json.dumps({**item, 'onboardingSkipped': 'PRIVATE'}))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'hermes-desktop')
            for changed in ({**action, 'label': 'PRIVATE'}, {**action, 'action': 'PRIVATE'},
                            {**action, 'sampleStatus': True}, {**action, 'blocker': 'PRIVATE'},
                            {key: val for key, val in action.items() if key != 'blocker'}, None):
                path.write_text(json.dumps({**value, 'actionObservation': changed}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'hermes-desktop')

    def test_atspi_geometry_diagnostics_reject_private_or_inconsistent_counts(self):
        names = 'sampledButtons identityRejected stateRejected stabilityRejected containmentRejected offsetExpected offsetMissing offsetInconsistent toggleOn toggleOff toggleUnknown'.split()
        value = dict(schemaVersion=1, mechanism='zed-atspi-geometry', diagnosticsOnly=True,
                     status='observed', phase='pre-retry', **dict.fromkeys(names, 0))
        value.update(sampledButtons=1, offsetMissing=1, toggleOn=1)
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'observation.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [value])
            path.write_text(json.dumps({**value, 'phase': 'pre-send'}))
            self.assertEqual(q.semantic_observations(root, 'zed-desktop')[0]['phase'], 'pre-send')
            for changed in ({**value, 'phase': 'PRIVATE'}, {**value, 'phase': True},
                            {**value, 'bus': 'PRIVATE'}, {**value, 'status': 'PRIVATE'},
                            {**value, 'sampledButtons': 65}, {**value, 'toggleOn': False},
                            {**value, 'offsetMissing': 0}, {**value, 'diagnosticsOnly': False}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')

    def test_native_root_preflight_and_zoom_observations_are_closed(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'observation.json'
            preflight = dict(schemaVersion=1, mechanism='claude-native-root-preflight',
                             diagnosticsOnly=True, stage='roots-created', failure=None)
            zoom = dict(schemaVersion=1, mechanism='zed-panel-zoom', diagnosticsOnly=True,
                        status='observed', maximizeMatches=1, minimizeMatches=0,
                        stableMaximizeMatches=1, stableMinimizeMatches=0, correlatedButtons=1,
                        checkedState='unavailable', uniqueCorrelation=True, activationAttempted=False)
            for item, app, invalid in ((preflight, 'claude-desktop', (
                    {**preflight, 'stage': 'PRIVATE'}, {**preflight, 'stage': 'foundation-query'},
                    {**preflight, 'failure': 'PRIVATE'}, {**preflight, 'path': 'PRIVATE'})),
                    (zoom, 'zed-desktop', ({**zoom, 'activationAttempted': True},
                    {**zoom, 'maximizeMatches': 65}, {**zoom, 'checkedState': 'PRIVATE'},
                    {**zoom, 'correlatedButtons': 2}, {**zoom, 'label': 'PRIVATE'}))):
                path.write_text(json.dumps(item))
                self.assertEqual(q.semantic_observations(root, app), [item])
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'hermes-desktop')
                for changed in invalid:
                    path.write_text(json.dumps(changed))
                    with self.assertRaises(ValueError):
                        q.semantic_observations(root, app)

    def test_post_stop_process_observation_cannot_waive_cleanup_or_leak_identity(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'post-stop.json'
            value = dict(schemaVersion=1, mechanism='windows-post-stop-process', diagnosticsOnly=True,
                phase='first-accessibility-rejection', state='absent')
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [value])
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'chatgpt-desktop')
            for changed in ({**value, 'pid': 'PRIVATE'}, {**value, 'state': 'PRIVATE'},
                {**value, 'phase': 'PRIVATE'}, {**value, 'diagnosticsOnly': False}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')

    def test_zoom_raw_role_group_rejects_partial_inconsistent_and_private_values(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'zoom.json'
            value = dict(schemaVersion=1, mechanism='zed-panel-zoom', diagnosticsOnly=True,
                status='observed', maximizeMatches=1, minimizeMatches=0,
                stableMaximizeMatches=1, stableMinimizeMatches=0, correlatedButtons=1,
                checkedState='unavailable', uniqueCorrelation=True, activationAttempted=False,
                matchedPushButtons=1, matchedToggleButtons=0, nestedContainingControls=0,
                matchedRole='push-button')
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [value])
            partial = dict(value)
            del partial['matchedRole']
            for changed in (partial, {**value, 'matchedRole': 'PRIVATE'},
                {**value, 'matchedToggleButtons': True}, {**value, 'matchedPushButtons': 65},
                {**value, 'matchedToggleButtons': 1}, {**value, 'nestedContainingControls': 2},
                {**value, 'checkedState': 'on'}, {**value, 'rawRole': 'PRIVATE'}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')
            mixed = {**value, 'correlatedButtons': 2, 'uniqueCorrelation': False,
                'matchedToggleButtons': 1, 'matchedRole': 'mixed',
                'nestedContainingControls': 1, 'checkedState': 'ambiguous'}
            path.write_text(json.dumps(mixed))
            self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [mixed])

    def test_renderer_document_diagnostics_reject_text_and_partial_records(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            value = dict(schemaVersion=1, mechanism='renderer-inventory', diagnosticsOnly=True,
                         app='pen-desktop', endpointOwned=True, launcherOwned=True, attached=True,
                         pageCount=1, textareaCount=0, editableCount=0, sendCount=0,
                         retryCount=0, newThreadCount=0, loginCount=0, dialogCount=0, errorCategory=None)
            state = dict(readyState='complete', targetKind='file', bodyPresent=True,
                         elementCount=10, visibleElementCount=2, inputCount=0, frameCount=1, pageErrorCount=0)
            path = root / 'inventory.json'
            value['documentState'] = state
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'pen-desktop'), [value])
            for changed in ({**state, 'url': 'PRIVATE'}, {**state, 'targetKind': 'PRIVATE'},
                            {**state, 'inputCount': True}, {**state, 'readyState': None},
                            {key: item for key, item in state.items() if key != 'frameCount'}):
                path.write_text(json.dumps({**value, 'documentState': changed}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'pen-desktop')

    def test_windows_process_absence_error_is_closed_and_bound_to_application(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            value = dict(schemaVersion=1, mechanism='windows-process-absence', diagnosticsOnly=True,
                         app='claude-desktop', stage='deadline')
            path = root / 'process.json'
            for stage in ('deadline', 'system-root', 'private-output', 'spawn', 'exit', 'read', 'schema', 'oversize', 'snapshot', 'first', 'next'):
                record = {**value, 'stage': stage}
                path.write_text(json.dumps(record))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [record])
            for changed in ({**value, 'stage': 'PRIVATE'}, {**value, 'stage': []},
                            {**value, 'app': 'chatgpt-desktop'}, {**value, 'pid': 42},
                            {**value, 'diagnosticsOnly': False}, {**value, 'schemaVersion': True}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'pen-desktop')

    def test_claude_restore_receipt_preserves_failure_boundaries_without_details(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            value = dict(schemaVersion=1, mechanism='claude-restore', diagnosticsOnly=True,
                         stage='process-check', outcome='rejected', errorCategory='app-running')
            path = root / 'restore.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [value])
            for changed in ({**value, 'errorCategory': 'PRIVATE'}, {**value, 'stage': 'receipt'},
                            {**value, 'outcome': 'restored'}, {**value, 'message': 'PRIVATE'},
                            {**value, 'diagnosticsOnly': False}, {**value, 'stage': []},
                            {**value, 'errorCategory': []}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            for outcome, category in [('restored', None), ('nothing-to-restore', 'no-receipt'),
                                      ('rejected', 'document-restore')]:
                receipt = {**value, 'stage': 'receipt', 'outcome': outcome, 'errorCategory': category}
                path.write_text(json.dumps(receipt))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [receipt])
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')

    def test_claude_configuration_presence_is_closed_diagnostic_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            value = dict(schemaVersion=1, mechanism='claude-owned-configuration', diagnosticsOnly=True,
                         configurationPresent=True, objectSchema=True, deploymentModeMatches=True,
                         profileMatches=True, providerGateway=True, loopbackBaseUrlMatches=True,
                         authMatches=True, modelDiscoveryEnabled=True, chatEnabled=True, chooserDisabled=True,
                         nativePathAlignment=False, configurationConsumed=None, modelDiscoverySeen=None)
            path = root / 'configuration.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [value])
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'zed-desktop')
            for changed in ({**value, 'token': 'PRIVATE'}, {**value, 'nativePathAlignment': 'PRIVATE'},
                            {**value, 'authMatches': None}, {**value, 'configurationConsumed': 1},
                            {**value, 'diagnosticsOnly': False}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')

    def test_hermes_composer_readiness_diagnostics_are_closed_and_advisory(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            value = dict(schemaVersion=1, mechanism='hermes-windows-catalog-readiness', diagnosticsOnly=True,
                         stage='composer', errorCategory='composer-unavailable', menuOpened=False,
                         refreshAttempted=False, catalogVerified=False, modelRowVerified=False,
                         menuDismissed=False, composerReverified=False)
            path = root / 'readiness.json'
            observation = dict(roots=1, editors=1, expectedModelPills=0, modelPills=0,
                               pickerButtons=1, switchButtons=0, readyState='complete')
            for snapshot in (None, observation):
                item = {**value, 'composerObservation': snapshot, 'guardFailure': 'deadline-expired'}
                path.write_text(json.dumps(item))
                self.assertEqual(q.semantic_observations(root, 'hermes-desktop'), [item])
            item = {**value, 'composerObservation': observation, 'guardFailure': 'ownership-lost'}
            invalid = [{**item, 'guardFailure': 'PRIVATE'},
                       {**item, 'composerObservation': {**observation, 'label': 'PRIVATE'}},
                       {**item, 'composerObservation': {**observation, 'roots': True}},
                       {**item, 'composerObservation': {**observation, 'roots': 65}},
                       {**item, 'composerObservation': {**observation, 'readyState': 'PRIVATE'}},
                       {key: val for key, val in item.items() if key != 'guardFailure'}]
            for changed in invalid:
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'hermes-desktop')
            path.write_text(json.dumps(item))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'zed-desktop')
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'hermes-desktop'), [value])

    def test_pointer_observation_never_accepts_private_native_details(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            value = dict(schemaVersion=1, mechanism='zed-pointer-observation', diagnosticsOnly=True,
                         maximizedHorizontal=True, maximizedVertical=True, enabled=True, sensitive=True,
                         showing=None, visible=None, defunct=False, retryContains=True, pointerTarget='client', pointerChild='client')
            path = root / 'pointer.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [value])
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'hermes-desktop')
            measured = {**value, 'clientOriginVerified': True, 'retryOffsetRelation': 'missing-origin'}
            path.write_text(json.dumps(measured))
            self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [measured])
            corrected = {**measured, 'clientOriginVerified': False, 'coordinatePackage': 'noble-5build1',
                         'coordinateRelation': 'parent-offset', 'coordinateAuthority': 'verified-xtranslate'}
            path.write_text(json.dumps(corrected))
            self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [corrected])
            for changed in ({**corrected, 'coordinatePackage': 'PRIVATE'},
                            {**corrected, 'coordinatePackage': 'unverified'},
                            {**corrected, 'coordinateRelation': 'other'},
                            {**corrected, 'coordinateAuthority': True},
                            {**corrected, 'clientOriginVerified': True},
                            {key: item for key, item in corrected.items() if key != 'coordinateRelation'}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')
            for changed in ({**measured, 'clientOriginVerified': 1},
                            {**measured, 'retryOffsetRelation': 'PRIVATE'},
                            {**measured, 'retryOffsetRelation': False},
                            {key: item for key, item in measured.items() if key != 'clientOriginVerified'}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')
            for changed in ({**value, 'x': 100}, {**value, 'pointerTarget': 'PRIVATE'}, {**value, 'pointerChild': 'PRIVATE'},
                            {**value, 'enabled': 1}, {**value, 'diagnosticsOnly': False},
                            {key: item for key, item in value.items() if key != 'visible'}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')

    def test_claude_model_discovery_is_positive_only_and_payload_free(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'discovery.json'
            value = dict(schemaVersion=1, mechanism='claude-model-discovery', diagnosticsOnly=True,
                         authenticatedModelsCount=1, complete=True, modelDiscoverySeen=True)
            for count, seen, complete in ((0, None, True), (1, True, True), (32, True, False)):
                current = {**value, 'authenticatedModelsCount': count, 'modelDiscoverySeen': seen, 'complete': complete}
                path.write_text(json.dumps(current))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [current])
            for changed in ({**value, 'authenticatedModelsCount': 0}, {**value, 'authenticatedModelsCount': True},
                            {**value, 'authenticatedModelsCount': 33}, {**value, 'modelDiscoverySeen': False},
                            {**value, 'complete': 1}, {**value, 'modelIds': ['PRIVATE']},
                            {**value, 'diagnosticsOnly': False}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'chatgpt-desktop')

    def test_windows_claude_process_baseline_is_closed_and_initial_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'process-baseline.json'
            value = dict(schemaVersion=1, mechanism='windows-process-baseline', diagnosticsOnly=True,
                         app='claude-desktop', phase='before-launch', state='absent')
            for state in ('absent', 'present', 'query-failed'):
                current = {**value, 'state': state}
                path.write_text(json.dumps(current))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [current])
            for changed in ({**value, 'pid': 22}, {**value, 'state': True},
                            {**value, 'state': 'PRIVATE'}, {**value, 'phase': 'after-stop'},
                            {**value, 'diagnosticsOnly': False}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'chatgpt-desktop')

    def test_zed_retry_visual_is_advisory_closed_and_consistent(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'retry-visual.json'
            value = dict(schemaVersion=1, mechanism='zed-retry-visual', diagnosticsOnly=True,
                         status='complete', reason=None, templateSide=14, copyMatches=1, closeMatches=1,
                         baselinePairs=0, firstPairs=1, secondPairs=1, newStablePairs=1,
                         retryCorrelations=1, relation='left-same-row')
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [value])
            for changed in ({**value, 'x': 20}, {**value, 'templateSide': True},
                            {**value, 'newStablePairs': 2}, {**value, 'retryCorrelations': 0},
                            {**value, 'relation': 'PRIVATE'}, {**value, 'copyMatches': 33},
                            {**value, 'diagnosticsOnly': False}, {**value, 'status': 'unsupported'}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'claude-desktop')

    def test_claude_final_candidate_state_is_closed_and_diagnostic_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'focus.json'
            value = dict(schemaVersion=1, mechanism='claude-window-focus', diagnosticsOnly=True,
                         status='proved', nativeForegroundWindowMatchedHeld=True,
                         phase='final-stability', candidateState='off-display')
            for state in ('absent', 'ambiguous', 'identity-changed', 'bounds-changed', 'focus-unproved',
                          'same-process-window', 'off-display', 'occluded', 'proved'):
                current = {**value, 'candidateState': state}
                path.write_text(json.dumps(current))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [current])
            for changed in ({**value, 'candidateState': 'PRIVATE'}, {**value, 'candidateState': True},
                            {**value, 'phase': 'initial'}, {**value, 'windowId': 45}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')

    def test_claude_native_storage_preserves_unavailable_and_rejects_false_freshness(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            flags = dict(claudeLocalState=False, claudePreferences=False,
                         thirdPartyLocalState=False, thirdPartyPreferences=False)
            base = dict(schemaVersion=1, mechanism='claude-native-storage', diagnosticsOnly=True,
                        freshBefore=None, observationValid=False, before=None, after=None)
            path = root / 'native-storage.json'
            valid = [base, {**base, 'before': flags, 'freshBefore': True},
                     {**base, 'before': flags, 'after': flags, 'freshBefore': True, 'observationValid': True},
                     {**base, 'before': {**flags, 'claudeLocalState': True}, 'freshBefore': False}]
            for value in valid:
                path.write_text(json.dumps(value))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [value])
            for value in ({**base, 'freshBefore': True}, {**base, 'observationValid': True},
                          {**base, 'rawPath': 'PRIVATE'}, {**base, 'diagnosticsOnly': False},
                          {**base, 'before': {**flags, 'claudeLocalState': 1}},
                          {**valid[2], 'freshBefore': False}, {**base, 'freshBefore': 0}):
                path.write_text(json.dumps(value))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps(base))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'chatgpt-desktop')

    def test_claude_chat_navigation_is_one_observed_action_not_qualification(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            base = dict(schemaVersion=1, mechanism='claude-chat-navigation', diagnosticsOnly=True,
                        phase='preflight', actionStatus='not-attempted', preconditionsVerified=False,
                        pressAttempted=False, chatPostconditionVerified=False, nativeGuardVerified=True)
            path = root / 'chat-navigation.json'
            complete = {**base, 'phase': 'completed', 'actionStatus': 'completed',
                        'preconditionsVerified': True, 'pressAttempted': True, 'chatPostconditionVerified': True}
            counts = dict.fromkeys('classicEditable classicVisible modernMessageEditable sendMessageVisible sendMessageEnabled startTaskVisible modeGroupVisible modeChatVisible modeChatEnabled modeCoworkVisible'.split(), 0)
            for value in (base, complete, {**complete, 'postconditionCounts': counts},
                          {**base, 'nativePressStage': 'chat'},
                          {**base, 'nativePressStage': 'current-chat', 'preconditionsVerified': True},
                          {**complete, 'nativePressStage': 'completed'},
                          {**complete, 'actionStatus': 'uncertain'},
                          {**complete, 'phase': 'postcondition', 'chatPostconditionVerified': False}):
                path.write_text(json.dumps(value))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [value])
            for changed in ({**complete, 'preconditionsVerified': False}, {**complete, 'nativeGuardVerified': False},
                            {**base, 'pressAttempted': True}, {**complete, 'actionStatus': 'not-attempted'},
                            {**base, 'chatPostconditionVerified': True}, {**complete, 'windowId': 5},
                            {**complete, 'diagnosticsOnly': False}, {**complete, 'actionStatus': 'PRIVATE'},
                            {**base, 'nativePressStage': 'completed'}, {**complete, 'nativePressStage': 'tree'},
                            {**base, 'nativePressStage': 'PRIVATE'},
                            {**base, 'nativePressStage': 'current-chat'},
                            {**complete, 'nativePressStage': 'current-chat'}, {**base, 'postconditionCounts': counts},
                            {**complete, 'postconditionCounts': {**counts, 'prompt': 'PRIVATE'}},
                            {**complete, 'postconditionCounts': {**counts, 'classicEditable': True}},
                            {**complete, 'postconditionCounts': {**counts, 'classicEditable': 4097}}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')

    def test_windows_process_settlement_preserves_unknown_and_ordered_results(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            base = dict(schemaVersion=1, mechanism='windows-process-settlement', diagnosticsOnly=True,
                        firstState='present', lastState='query-failed', queryCount=2)
            path = root / 'settlement.json'
            for value in (base, {**base, 'firstState': 'not-queried', 'lastState': 'not-queried', 'queryCount': 0},
                          {**base, 'firstState': 'absent', 'lastState': 'absent', 'queryCount': 1},
                          {**base, 'queryCount': None}):
                path.write_text(json.dumps(value))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [value])
            for changed in ({**base, 'queryCount': True}, {**base, 'queryCount': 129},
                            {**base, 'queryCount': 0}, {**base, 'queryCount': 1},
                            {**base, 'firstState': 'not-queried'}, {**base, 'lastState': 'PRIVATE'},
                            {**base, 'pid': 99}, {**base, 'diagnosticsOnly': False}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')

    def test_claude_window_fit_stages_reject_unclosed_identity_and_success_claims(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            value = dict(schemaVersion=1, mechanism='claude-window-fit', diagnosticsOnly=True, stage='postcondition')
            path = root / 'window-fit.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [value])
            for changed in ({**value, 'stage': 'PRIVATE'}, {**value, 'stage': True},
                            {**value, 'windowId': 10}, {**value, 'qualified': True},
                            {**value, 'diagnosticsOnly': False}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')

    def test_claude_private_storage_stages_reject_paths_and_wrong_phase(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            value = dict(schemaVersion=1, mechanism='claude-private-storage-stage', diagnosticsOnly=True,
                         phase='before-launch', stage='environment-unbound')
            path = root / 'private-storage-stage.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [value])
            for changed in ({**value, 'stage': 'checkpoint-read-failed'}, {**value, 'phase': 'PRIVATE'},
                            {**value, 'diagnosticsOnly': False}, {**value, 'path': 'PRIVATE'},
                            {**value, 'stage': True}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')

    def test_claude_native_composer_is_source_bound_and_never_accepts_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            counts = dict(classicEditable=1, classicVisible=1, modernMessageEditable=0,
                          sendMessageVisible=1, sendMessageEnabled=0, startTaskVisible=None)
            value = dict(schemaVersion=1, mechanism='claude-native-composer', diagnosticsOnly=True,
                         sourceVersion='2.19675.0', sourceCount=counts,
                         classicSourceSha256='6e6be632eb7adc0e66c1bb795448269d6c1f3ffe8821bea59d9e9374671cf0ea',
                         sendSourceSha256='69d43f83ac78605402b590559cfb9bd355215336a193cedf80cc30b246c1db60',
                         modernSourceSha256='a9f54a8a154e19f86a9d9d696b808bd693904b5e47ec63517abb635003a4244d')
            path = root / 'composer.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [value])
            mode = {**value, 'modeSourceSha256': '0d16680f19e10d03bc11e7797d842d01159da37b5ab410cad9b7307f7eeef3aa',
                    'sourceCount': {**counts, 'modeGroupVisible': 1, 'modeChatVisible': 1,
                                    'modeChatEnabled': 1, 'modeCoworkVisible': 1}}
            path.write_text(json.dumps(mode))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [mode])
            linux = {**mode, 'sourceVersion': '2.9939.4',
                     'classicSourceSha256': '26f823bafc90cff4a749bfad6916ee69e4c3189f18b54a4e958ca387939c1181',
                     'sendSourceSha256': 'd076b2f208fc5e572d0f3cd39aba35c6bacbe100a82db569851a0ce2317fa05c',
                     'modernSourceSha256': '5d1afc949ac69080ef6fe15491137ca0c3d2056991a9581537cba2bcc3724287',
                     'modeSourceSha256': '62ffbc1b8a3e4440ae77a33be142afd1914796f945bcd75d58cfe73679925f61'}
            path.write_text(json.dumps(linux))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [linux])
            for mixed in ({**linux, 'modeSourceSha256': mode['modeSourceSha256']},
                          {**linux, 'classicSourceSha256': mode['classicSourceSha256']},
                          {**mode, 'sourceVersion': linux['sourceVersion']}):
                path.write_text(json.dumps(mixed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            for invalid in ({**mode, 'modeSourceSha256': '0' * 64}, {**mode, 'sourceCount': counts}):
                path.write_text(json.dumps(invalid))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            for changed in ({**value, 'sourceVersion': 'other'}, {**value, 'classicSourceSha256': '0' * 64},
                            {**value, 'diagnosticsOnly': False}, {**value, 'text': 'PRIVATE'},
                            {**value, 'sourceCount': {**counts, 'classicEditable': True}},
                            {**value, 'sourceCount': {**counts, 'classicEditable': 4097}},
                            {**value, 'sourceCount': {**counts, 'selector': 'PRIVATE'}}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'chatgpt-desktop')

    def test_claude_storage_use_is_closed_and_cannot_certify_consumption(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            flags = dict(claudeLocalState=False, claudePreferences=False,
                         thirdPartyLocalState=False, thirdPartyPreferences=False)
            value = dict(schemaVersion=1, mechanism='claude-storage-use', diagnosticsOnly=True,
                         freshBefore=True, observationValid=True, before=flags,
                         after={**flags, 'thirdPartyPreferences': True})
            path = root / 'storage.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop')[0], value)
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'chatgpt-desktop')
            for changed in ({**value, 'rawPath': 'PRIVATE'}, {**value, 'diagnosticsOnly': False},
                            {**value, 'freshBefore': False}, {**value, 'configurationConsumed': True},
                            {**value, 'after': {**flags, 'thirdPartyPreferences': 1}},
                            {**value, 'before': {**flags, 'claudeLocalState': True}}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')

    def test_zed_published_ancestor_bounds_are_closed_and_do_not_certify_hitbox(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            base = dict(schemaVersion=1, mechanism='zed-pointer-observation', diagnosticsOnly=True,
                        maximizedHorizontal=True, maximizedVertical=True, enabled=True, sensitive=True,
                        showing=None, visible=None, defunct=False, retryContains=True,
                        pointerTarget='client', pointerChild='client')
            path = root / 'ancestor.json'
            valid = [dict(ancestorBoundsStatus='complete', checkedAncestorCount=3,
                          centerWithinPublishedAncestors=within) for within in (True, False)]
            valid += [dict(ancestorBoundsStatus=state, checkedAncestorCount=count,
                           centerWithinPublishedAncestors=None)
                      for state, count in (('unavailable', 0), ('cycle', 2), ('limit', 64))]
            historical = list(valid)
            valid.extend({**extension, 'ancestorQueryStage': 'complete' if extension['ancestorBoundsStatus'] == 'complete'
                          else 'chain' if extension['ancestorBoundsStatus'] in ('cycle', 'limit') else 'ancestor-bounds'}
                         for extension in historical)
            for extension in valid:
                value = {**base, **extension}
                path.write_text(json.dumps(value))
                self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [value])
            for extension in ({**valid[0], 'centerWithinPublishedAncestors': 1},
                              {**valid[0], 'checkedAncestorCount': True},
                              {**valid[0], 'ancestorBoundsStatus': 'PRIVATE'},
                              {**valid[0], 'ancestorQueryStage': 'PRIVATE'},
                              {**valid[0], 'ancestorQueryStage': 'ancestor-bounds'},
                              {**valid[2], 'ancestorQueryStage': 'complete'},
                              {**valid[2], 'centerWithinPublishedAncestors': True},
                              {**valid[-1], 'checkedAncestorCount': 63},
                              {'ancestorBoundsStatus': 'complete'},
                              {**valid[0], 'bounds': [0, 0, 1, 1]}):
                path.write_text(json.dumps({**base, **extension}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')

    def test_input_delivery_is_advisory_closed_and_distinguishes_unmeasured(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            value = dict(schemaVersion=1, mechanism='zed-pointer-observation', diagnosticsOnly=True,
                         maximizedHorizontal=True, maximizedVertical=True, enabled=True, sensitive=True,
                         showing=None, visible=None, defunct=False, retryContains=True,
                         pointerTarget='client', pointerChild='client')
            path = root / 'pointer.json'
            valid = [dict(status='complete', pressCount=1, releaseCount=1, orderedPair=True),
                     dict(status='complete', pressCount=0, releaseCount=0, orderedPair=False)]
            valid.extend(dict(status=status, pressCount=None, releaseCount=None, orderedPair=None)
                         for status in ('unavailable', 'timeout', 'query-failed', 'identity-failed'))
            valid.extend(dict(status='unavailable', pressCount=None, releaseCount=None, orderedPair=None, stage=stage)
                         for stage in ('policy', 'budget-insufficient', 'request', 'library', 'display',
                                       'record-version', 'xres-version', 'xinput-extension', 'client-query',
                                       'client-identity', 'context', 'enable', 'identity-recheck',
                                       'armed', 'observation', 'cleanup'))
            for delivery in valid:
                path.write_text(json.dumps({**value, 'inputDelivery': delivery}))
                observed = q.semantic_observations(root, 'zed-desktop')[0]
                self.assertEqual(observed['inputDelivery'], delivery)
                self.assertIs(observed['diagnosticsOnly'], True)
            invalid = [dict(status='PRIVATE', pressCount=None, releaseCount=None, orderedPair=None),
                       dict(status='complete', pressCount=0, releaseCount=1, orderedPair=True),
                       dict(status='complete', pressCount=True, releaseCount=1, orderedPair=False),
                       dict(status='complete', pressCount=3, releaseCount=1, orderedPair=False),
                       dict(status='timeout', pressCount=0, releaseCount=None, orderedPair=None),
                       {**valid[0], 'window': 10}, {**valid[0], 'stage': 'PRIVATE'},
                       {**valid[0], 'stage': None}, {'status': 'complete'}, None]
            for delivery in invalid:
                path.write_text(json.dumps({**value, 'inputDelivery': delivery}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')

    def test_pointer_diagnostic_is_closed_and_app_scoped(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            value = dict(schemaVersion=1, mechanism='zed-pointer-transport',
                         diagnosticsOnly=True, stage='movement-failed')
            path = root / 'pointer.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [value])
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'hermes-desktop')
            for changed in ({**value, 'stage': 'PRIVATE'}, {**value, 'pid': 40},
                            {**value, 'diagnosticsOnly': False}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')

    def test_clipboard_failures_reject_private_fields_and_invalid_categories(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'clipboard.json'
            value = dict(schemaVersion=1, mechanism='zed-clipboard-transport',
                         diagnosticsOnly=True, operation='clear', stage='wait-timeout', elapsed='at-least-3s')
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [value])
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'hermes-desktop')
            for changed in ({**value, 'text': 'PRIVATE'}, {**value, 'operation': None},
                            {**value, 'stage': 'PRIVATE'}, {**value, 'elapsed': True},
                            {**value, 'diagnosticsOnly': False}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')

    def test_policy_preparation_failures_keep_only_the_closed_stage(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            value = dict(schemaVersion=1, mechanism='hermes-policy-preparation',
                         diagnosticsOnly=True, stage='ownership')
            path = root / 'failure.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'hermes-desktop'), [value])
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'pen-desktop')
            for changed in ({**value, 'stage': 'PRIVATE'}, {**value, 'path': 'PRIVATE'},
                            {**value, 'diagnosticsOnly': False}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'hermes-desktop')

    def test_zed_tooltip_zoom_proof_has_closed_bounded_statuses(self):
        value = dict(schemaVersion=1, mechanism='zed-panel-zoom', diagnosticsOnly=True,
                     status='observed', maximizeMatches=0, minimizeMatches=0,
                     stableMaximizeMatches=0, stableMinimizeMatches=0, correlatedButtons=0,
                     checkedState='unavailable', uniqueCorrelation=False, activationAttempted=False)
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'zoom.json'
            for status, candidates, matches in (('unmeasured', 0, 0), ('unavailable', 0, 0),
                                                ('missing', 3, 0), ('proved', 3, 1), ('ambiguous', 3, 2)):
                item = {**value, 'tooltipStatus': status, 'tooltipCandidates': candidates, 'tooltipMatches': matches}
                path.write_text(json.dumps(item))
                self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [item])
            progress = {**value, 'tooltipStartRemainingMs': 2200, 'tooltipEndRemainingMs': 0,
                        'tooltipPhase': 'initial-clear'}
            path.write_text(json.dumps(progress))
            self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [progress])
            bounded = {**progress, 'tooltipStartRemainingMs': 30000, 'tooltipEndRemainingMs': 2200,
                       'tooltipPhase': 'hover'}
            path.write_text(json.dumps(bounded))
            self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [bounded])
            metrics = dict(contrastPositions=20, foregroundPositions=10, maxCorrelationMilli=999,
                           maxContrastMilli=255000, maxSpreadMilli=255000)
            calibration = dict(templateSide=14, scaleMilli=1000, maximize=metrics, minimize=metrics)
            measured = {**value, 'iconCalibration': calibration}
            path.write_text(json.dumps(measured))
            self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [measured])
            for invalid in (None, {**calibration, 'templateSide': True},
                            {**calibration, 'scaleMilli': 2000}, {**calibration, 'pixels': 'PRIVATE'},
                            {**calibration, 'minimize': {**metrics, 'maxCorrelationMilli': 1001}},
                            {**calibration, 'maximize': {**metrics, 'foregroundPositions': 21}},
                            {**calibration, 'maximize': {**metrics, 'maxContrastMilli': True}},
                            {**calibration, 'maximize': {**metrics, 'label': 'PRIVATE'}}):
                path.write_text(json.dumps({**value, 'iconCalibration': invalid}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')
            for changed in ({**progress, 'tooltipEndRemainingMs': 2201},
                            {**progress, 'tooltipStartRemainingMs': True},
                            {**progress, 'tooltipPhase': 'PRIVATE'},
                            {**progress, 'tooltipStartRemainingMs': 30001},
                            {key: item for key, item in progress.items() if key != 'tooltipPhase'}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')
            proof = {**value, 'tooltipStatus': 'proved', 'tooltipCandidates': 3, 'tooltipMatches': 1}
            for changed in ({**proof, 'tooltip': 'PRIVATE'}, {**proof, 'tooltipStatus': 'PRIVATE'},
                            {**proof, 'tooltipCandidates': 4}, {**proof, 'tooltipMatches': True},
                            {**proof, 'tooltipMatches': 2}, {**proof, 'tooltipCandidates': 0},
                            {**proof, 'tooltipStatus': 'missing'}, {**proof, 'tooltipStatus': 'unavailable'},
                            {key: item for key, item in proof.items() if key != 'tooltipMatches'}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')

    def test_windows_cleanup_preflight_failure_is_closed_and_not_acceptance(self):
        value = dict(schemaVersion=1, mechanism='windows-owned-cleanup-preflight',
                     diagnosticsOnly=True, stage='deadline')
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'failure.json'
            for stage in 'request path file-open file-hash file-identity process-open snapshot inspector-parent ancestry target-open target-identity owner-recheck deadline transport'.split():
                item = {**value, 'stage': stage}
                path.write_text(json.dumps(item))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [item])
            for changed in ({**value, 'stage': 'PRIVATE'}, {**value, 'path': 'PRIVATE'},
                            {**value, 'diagnosticsOnly': False}, {**value, 'stage': None}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'chatgpt-desktop')
            self.assertEqual(q.envelope('claude-desktop', 'windows', 'x86_64', 'a' * 40)['qualification'], 'unqualified')

    def test_claude_native_chat_receipt_is_closed_and_not_qualification(self):
        value = dict(schemaVersion=1, mechanism='claude-native-chat', diagnosticsOnly=True,
                     stage='completed', submittedTurns=3, inputVerifiedTurns=3, copiedResponses=3,
                     retryAttempted=True, clipboardCleared=True)
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'native-chat.json'
            for item in (value, {**value, 'stage': 'deadline', 'copiedResponses': 0}):
                path.write_text(json.dumps(item))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [item])
                self.assertEqual(q.envelope('claude-desktop', 'macos', 'aarch64', 'a' * 40)['qualification'], 'unqualified')
            for stage in ('tree-query', 'tree-duplicate', 'tree-type', 'tree-limit', 'tree-pid',
                          'tree-focus', 'tree-window', 'input-initial-unavailable', 'input-initial-nonempty',
                          'input-clipboard-mismatch', 'input-value-mismatch'):
                item = {**value, 'stage': stage, 'submittedTurns': 0, 'inputVerifiedTurns': 0,
                        'copiedResponses': 0, 'retryAttempted': False}
                path.write_text(json.dumps(item))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [item])
            for changed in ({**value, 'prompt': 'PRIVATE'}, {**value, 'stage': 'PRIVATE'},
                            {**value, 'submittedTurns': True}, {**value, 'copiedResponses': 4},
                            {**value, 'inputVerifiedTurns': 2}, {**value, 'retryAttempted': 1},
                            {**value, 'diagnosticsOnly': False},
                            {**value, 'submittedTurns': 0, 'copiedResponses': 2}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'chatgpt-desktop')

    def test_windows_owned_descendant_cleanup_does_not_replace_absence(self):
        counts = set('retainedCount alreadyExitedCount targetedCount exitedCount rejectedCount'.split())
        flags = set('triggerAttempted expectedExecutableVerified historicalOwnershipVerified'.split())
        value = dict(schemaVersion=1, mechanism='windows-owned-descendant-cleanup', diagnosticsOnly=True,
                     status='completed', retainedCount=5, alreadyExitedCount=1, targetedCount=4,
                     exitedCount=4, rejectedCount=0, **dict.fromkeys(flags, True))
        unavailable = {**value, 'status': 'unavailable', **dict.fromkeys(counts), **dict.fromkeys(flags, False)}
        deadline = {**value, 'status': 'deadline', **dict.fromkeys(counts - {'retainedCount'})}
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'owned-cleanup.json'
            for item in (value, unavailable, deadline, {**deadline, 'status': 'uncertain'}, {**unavailable, 'status': 'deadline'},
                         {**value, 'status': 'partial', 'exitedCount': 3}):
                path.write_text(json.dumps(item))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [item])
                self.assertEqual(q.envelope('claude-desktop', 'windows', 'x86_64', 'a' * 40)['qualification'], 'unqualified')
            for changed in ({**value, 'pid': 7}, {**value, 'executable': 'PRIVATE'},
                            {**value, 'triggerAttempted': False}, {**value, 'expectedExecutableVerified': False},
                            {**value, 'historicalOwnershipVerified': False}, {**value, 'retainedCount': True},
                            {**value, 'retainedCount': 65}, {**value, 'targetedCount': 5},
                            {**value, 'exitedCount': 5}, {**value, 'status': 'partial'},
                            {**value, 'retainedCount': None}, {**value, 'diagnosticsOnly': False},
                            {**unavailable, 'triggerAttempted': True}, {**unavailable, 'retainedCount': 0},
                            {**deadline, 'targetedCount': 1}, {**deadline, 'retainedCount': None},
                            {**deadline, 'historicalOwnershipVerified': False}, {**value, 'status': 'PRIVATE'}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'chatgpt-desktop')

    def test_windows_process_correlation_is_closed_and_not_job_membership(self):
        value = dict(schemaVersion=1, mechanism='windows-process-correlation', diagnosticsOnly=True,
                     status='observed', sameLauncherSurvives=False, verifiedDescendantsPresent=True,
                     unlinkedMatchesPresent=True, matchedCount=2, verifiedDescendantCount=1, unlinkedCount=1)
        evidence = ('sameLauncherSurvives', 'verifiedDescendantsPresent', 'unlinkedMatchesPresent',
                    'matchedCount', 'verifiedDescendantCount', 'unlinkedCount')
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'correlation.json'
            for item in (value, {**value, 'status': 'unavailable', **dict.fromkeys(evidence)},
                         {**value, 'status': 'deadline', **dict.fromkeys(evidence)}):
                path.write_text(json.dumps(item))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [item])
            for changed in ({**value, 'pid': 7}, {**value, 'jobMembership': True},
                            {**value, 'sameLauncherSurvives': 1}, {**value, 'matchedCount': True},
                            {**value, 'matchedCount': 1}, {**value, 'verifiedDescendantsPresent': False},
                            {**value, 'unlinkedMatchesPresent': False}, {**value, 'status': 'PRIVATE'},
                            {**value, 'status': 'unavailable'}, {**value, 'matchedCount': 65}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'chatgpt-desktop')

    def test_codex_initial_binding_diagnostics_are_closed_and_not_acceptance(self):
        value = dict(schemaVersion=1, mechanism='renderer-inventory', diagnosticsOnly=True,
                     app='chatgpt-desktop', endpointOwned=True, launcherOwned=True, attached=True,
                     pageCount=1, textareaCount=0, editableCount=0, sendCount=0,
                     retryCount=0, newThreadCount=0, loginCount=0, dialogCount=0, errorCategory=None)
        binding = dict(status='captured', route='primary', targetPresent=True,
                       framePresent=True, loaderPresent=True)
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'inventory.json'
            for item in (binding, {**binding, 'status': 'route-rejected', 'route': 'primary-query'},
                         {**binding, 'status': 'deadline', 'loaderPresent': False}):
                path.write_text(json.dumps({**value, 'initialMainBinding': item}))
                self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['initialMainBinding'], item)
            self.assertEqual(q.envelope('chatgpt-desktop', 'linux', 'x86_64', 'a' * 40)['qualification'], 'unqualified')
            for changed in ({**binding, 'url': 'PRIVATE'}, {**binding, 'status': 'PRIVATE'},
                            {**binding, 'route': 'PRIVATE'}, {**binding, 'loaderPresent': 1},
                            {**binding, 'loaderPresent': False}, {**binding, 'route': 'primary-query'},
                            {key: item for key, item in binding.items() if key != 'framePresent'}):
                path.write_text(json.dumps({**value, 'initialMainBinding': changed}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'chatgpt-desktop')

    def test_windows_endpoint_counts_preserve_earlier_failures_without_private_identity(self):
        allowed = 'owned process-budget ancestry-cycle process-unavailable parent-unavailable parent-reused session-mismatch ancestry-limit listener-unavailable query-failed transport-timeout transport-failed unclassified'.split()
        value = dict(schemaVersion=1, mechanism='windows-endpoint-proof', diagnosticsOnly=True,
                     category='owned', categoryCounts={**dict.fromkeys(allowed, 0), 'owned': 2, 'parent-reused': 1})
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'proof.json'
            for item in (value, {key: item for key, item in value.items() if key != 'categoryCounts'},
                         {**value, 'categoryCounts': {**value['categoryCounts'], 'owned': 4096}}):
                path.write_text(json.dumps(item))
                self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop'), [item])
            for counts in ({**value['categoryCounts'], 'pid': 7},
                           {**value['categoryCounts'], 'owned': 0},
                           {**value['categoryCounts'], 'owned': True},
                           {**value['categoryCounts'], 'owned': 4097},
                           {key: count for key, count in value['categoryCounts'].items() if key != 'query-failed'}):
                path.write_text(json.dumps({**value, 'categoryCounts': counts}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'chatgpt-desktop')
            self.assertEqual(q.envelope('chatgpt-desktop', 'windows', 'x86_64', 'a' * 40)['qualification'], 'unqualified')

    def test_codex_managed_sign_in_observation_cannot_authorize_input(self):
        value = dict(schemaVersion=1, mechanism='renderer-inventory', diagnosticsOnly=True,
                     app='chatgpt-desktop', endpointOwned=True, launcherOwned=True, attached=True,
                     pageCount=1, textareaCount=0, editableCount=0, sendCount=0,
                     retryCount=0, newThreadCount=0, loginCount=0, dialogCount=0, errorCategory=None)
        empty = dict.fromkeys('loading unsupported disabled error chatgptChoice apiKeyChoice'.split(), 0)
        observations = [dict(status=key, counts={**empty, key: 1})
                        for key in ('loading', 'unsupported', 'disabled', 'error')]
        observations.extend([dict(status='unknown', counts=empty),
                             dict(status='sign-in-options', counts={**empty, 'apiKeyChoice': 1}),
                             dict(status='ambiguous', counts={**empty, 'loading': 1, 'apiKeyChoice': 1}),
                             dict(status='ambiguous', counts={**empty, 'loading': 2})])
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'inventory.json'
            for item in observations:
                path.write_text(json.dumps({**value, 'managedSignIn': item}))
                self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['managedSignIn'], item)
            item = observations[0]
            for changed in ({**item, 'status': 'PRIVATE'}, {**item, 'url': 'PRIVATE'},
                            {**item, 'status': 'unknown'}, {**item, 'counts': {**empty, 'loading': True}},
                            {**item, 'counts': {**empty, 'loading': 33}},
                            {**item, 'counts': {**empty, 'content': 'PRIVATE'}}):
                path.write_text(json.dumps({**value, 'managedSignIn': changed}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'chatgpt-desktop')
            path.write_text(json.dumps({**value, 'app': 'claude-desktop', 'managedSignIn': item}))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'claude-desktop')
            self.assertEqual(q.envelope('chatgpt-desktop', 'linux', 'x86_64', 'a' * 40)['qualification'], 'unqualified')

    def test_codex_source_screen_is_closed_passive_and_consistent(self):
        value = dict(schemaVersion=1, mechanism='renderer-inventory', diagnosticsOnly=True,
                     app='chatgpt-desktop', endpointOwned=True, launcherOwned=True, attached=True,
                     pageCount=1, textareaCount=0, editableCount=0, sendCount=0,
                     retryCount=0, newThreadCount=0, loginCount=0, dialogCount=0, errorCategory=None)
        headings = dict(gatewayHeading='gateway-connect', recoveryHeading='app-recovery',
                        importHeading='external-import', allSetHeading='all-set',
                        permissionHeading='permission-setup')
        empty = dict.fromkeys((*headings, 'continueSignIn'), 0)
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'inventory.json'
            screens = [dict(status='unknown', counts={**empty, 'continueSignIn': 1}),
                       dict(status='ambiguous', counts={**empty, 'gatewayHeading': 2}),
                       dict(status='ambiguous', counts={**empty, 'gatewayHeading': 1, 'allSetHeading': 1})]
            screens.extend(dict(status=status, counts={**empty, key: 1}) for key, status in headings.items())
            for screen in screens:
                path.write_text(json.dumps({**value, 'sourceScreen': screen}))
                self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['sourceScreen'], screen)
            screen = dict(status='unknown', counts=empty)
            for changed in ({**screen, 'title': 'PRIVATE'}, {**screen, 'status': 'PRIVATE'},
                            {**screen, 'status': 'gateway-connect'},
                            {**screen, 'counts': {**empty, 'gatewayHeading': True}},
                            {**screen, 'counts': {**empty, 'gatewayHeading': 33}},
                            {**screen, 'counts': {**empty, 'url': 'PRIVATE'}},
                            {**screen, 'counts': {key: count for key, count in empty.items() if key != 'continueSignIn'}}):
                path.write_text(json.dumps({**value, 'sourceScreen': changed}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'chatgpt-desktop')
            path.write_text(json.dumps({**value, 'app': 'claude-desktop', 'sourceScreen': screen}))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'claude-desktop')
            self.assertEqual(q.envelope('chatgpt-desktop', 'linux', 'x86_64', 'a' * 40)['qualification'], 'unqualified')

    def test_codex_main_confirmation_preserves_failed_guard_and_privacy(self):
        value = dict(schemaVersion=1, mechanism='renderer-inventory', diagnosticsOnly=True,
                     app='chatgpt-desktop', endpointOwned=True, launcherOwned=True, attached=True,
                     pageCount=1, textareaCount=0, editableCount=0, sendCount=0,
                     retryCount=0, newThreadCount=0, loginCount=0, dialogCount=0, errorCategory=None)
        counts = dict(roleLegend=1, roleRadios=11, engineering=1, dialog=0, quickChatComposer=0, editable=0)
        facts = dict(status='confirmed', identityUnchanged=True, mainScopeUnique=True,
                     documentFocused=True, counts=counts)
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'main.json'
            for item in (facts, {**facts, 'status': 'document-unfocused', 'documentFocused': False},
                         dict(status='initial-missing', identityUnchanged=None, mainScopeUnique=None,
                              documentFocused=None, counts=None)):
                path.write_text(json.dumps({**value, 'initialMainConfirmation': item}))
                self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['initialMainConfirmation'], item)
            for changed in ({**facts, 'status': 'PRIVATE'}, {**facts, 'targetId': 'PRIVATE'},
                            {**facts, 'identityUnchanged': False}, {**facts, 'documentFocused': 1},
                            {**facts, 'counts': {**counts, 'roleRadios': True}},
                            {**facts, 'counts': {**counts, 'roleRadios': 4097}},
                            {**facts, 'counts': {**counts, 'roleRadios': 10}}):
                path.write_text(json.dumps({**value, 'initialMainConfirmation': changed}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'chatgpt-desktop')

    def test_renderer_inventory_is_closed_and_cannot_claim_acceptance(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            value = dict(schemaVersion=1, mechanism='renderer-inventory', diagnosticsOnly=True,
                         app='pen-desktop', endpointOwned=True, launcherOwned=True, attached=True,
                         pageCount=1, textareaCount=1, editableCount=0, sendCount=1,
                         retryCount=0, newThreadCount=0, loginCount=0, dialogCount=0, errorCategory=None)
            path = root / 'inventory.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'pen-desktop'), [value])
            self.assertEqual(q.envelope('pen-desktop', 'linux', 'x86_64', 'a' * 40)['qualification'], 'unqualified')
            value['landingCounts'] = dict(importHeading=1, importDismiss=1, createProject=0,
                                         sourceFolders=0, projectName=0)
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'pen-desktop'), [value])
            for counts in ({**value['landingCounts'], 'PRIVATE': 1},
                           {**value['landingCounts'], 'importHeading': True},
                           {**value['landingCounts'], 'importHeading': 4097}):
                path.write_text(json.dumps({**value, 'landingCounts': counts}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'pen-desktop')
            value['startupScreen'] = 'gpu-unavailable'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'pen-desktop'), [value])
            for changed in ({**value, 'html': 'PRIVATE'}, {**value, 'sendCount': True},
                            {**value, 'app': 'claude-desktop'}, {**value, 'startupScreen': 'PRIVATE'},
                            {**value, 'startupScreen': None}, {**value, 'startupScreen': []},
                            {**value, 'startupScreen': 'cli-connection-failed'}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'pen-desktop')
            codex = {**value, 'app': 'chatgpt-desktop', 'startupScreen': 'cli-connection-failed'}
            codex['onboardingCounts'] = dict(roleRadios=11, roleLegend=1, workHeading=0, suggestionsCheckbox=1)
            path.write_text(json.dumps(codex))
            self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop'), [codex])
            for counts in ({**codex['onboardingCounts'], 'label': 'PRIVATE'},
                           {**codex['onboardingCounts'], 'roleRadios': True},
                           {**codex['onboardingCounts'], 'roleLegend': 4097}):
                path.write_text(json.dumps({**codex, 'onboardingCounts': counts}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'chatgpt-desktop')

    def test_codex_restore_facts_are_typed_private_diagnostics(self):
        value = dict(schemaVersion=1, mechanism='codex-restore', diagnosticsOnly=True,
                     stage='restore', cause='backup-mismatch')
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'restore.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop'), [value])
            for changed in ({**value, 'stage': 'PRIVATE'}, {**value, 'cause': []},
                            {**value, 'diagnosticsOnly': False}, {**value, 'schemaVersion': True},
                            {**value, 'rawError': 'PRIVATE'},
                            {key: item for key, item in value.items() if key != 'cause'}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'chatgpt-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'hermes-desktop')

    def test_public_onboarding_receipts_reject_private_and_untyped_data(self):
        setup = dict(schemaVersion=1, mechanism='codex-public-onboarding', diagnosticsOnly=True,
                     stage='stopped-after-role', errorCategory=None, conversationalScope=True,
                     engineeringControl=True, roleClickAttempted=True, roleClickCompleted=True,
                     engineeringChecked=True, continueControl=True, continueClickAttempted=True,
                     continueClickCompleted=True, roleScopeAbsent=True, roleProofFailure='unmeasured', sessionProofFailure='unmeasured')
        self.assertEqual(q.public_onboarding(setup, 'chatgpt-desktop'), setup)
        for reason in ('deadline', 'native-ownership', 'page-set', 'main-identity', 'main-focus', 'main-scope',
                       'auxiliary-route', 'auxiliary-identity', 'auxiliary-focus', 'auxiliary-controls',
                       'query-failed', 'unmeasured'):
            measured = {**setup, 'mainGuardFailure': reason}
            self.assertEqual(q.public_onboarding(measured, 'chatgpt-desktop'), measured)
        for invalid in ('PRIVATE', None, True, {'path': 'PRIVATE'}):
            with self.assertRaises(ValueError):
                q.public_onboarding({**setup, 'mainGuardFailure': invalid}, 'chatgpt-desktop')
        details = dict(reason='after-sample-changed', initialCount=1, currentCount=2, heldPresent=True)
        page_failure = {**setup, 'mainGuardFailure': 'page-set', 'pageSetFailure': details}
        self.assertEqual(q.public_onboarding(page_failure, 'chatgpt-desktop'), page_failure)
        for invalid in ({**details, 'reason': 'PRIVATE'}, {**details, 'initialCount': True},
                        {**details, 'currentCount': 33}, {**details, 'heldPresent': 1},
                        {**details, 'path': 'PRIVATE'}, {**details, 'reason': []}):
            with self.assertRaises(ValueError):
                q.public_onboarding({**page_failure, 'pageSetFailure': invalid}, 'chatgpt-desktop')
        with self.assertRaises(ValueError):
            q.public_onboarding({**page_failure, 'mainGuardFailure': 'deadline'}, 'chatgpt-desktop')
        overflow = {**details, 'initialCount': None, 'currentCount': None}
        self.assertEqual(q.public_onboarding({**page_failure, 'pageSetFailure': overflow}, 'chatgpt-desktop')['pageSetFailure'], overflow)
        inventory = dict(status='complete', total=2, held=1, app=1, blank=1, devtools=0, other=0)
        self.assertEqual(q.public_onboarding({**setup, 'rejectedPageInventory': inventory}, 'chatgpt-desktop')['rejectedPageInventory'], inventory)
        for invalid in ({**inventory, 'total': 1}, {**inventory, 'held': 2},
                        {**inventory, 'blank': True}, {**inventory, 'url': 'PRIVATE'},
                        {**inventory, 'status': []}, {'status': 'overflow', 'total': 33}):
            with self.assertRaises(ValueError):
                q.public_onboarding({**setup, 'rejectedPageInventory': invalid}, 'chatgpt-desktop')

        route_counts = dict.fromkeys(('avatarOverlay', 'hotkeyWindow', 'quickChat', 'quickChatPrewarm',
                                      'detachedWindow', 'globalDictation', 'debug', 'unknown'), 0)
        route_counts.update(quickChatPrewarm=1, unknown=1)
        source = dict(status='complete', routes=route_counts, visibility=dict(visible=1, hidden=1, unavailable=0))
        measured = {**inventory, 'source': source}
        self.assertEqual(q.public_onboarding({**setup, 'rejectedPageInventory': measured}, 'chatgpt-desktop')['rejectedPageInventory'], measured)
        for invalid in ({**source, 'url': 'PRIVATE'}, {**source, 'status': []},
                        {**source, 'routes': {**route_counts, 'unknown': True}},
                        {**source, 'visibility': dict(visible=2, hidden=1, unavailable=0)}):
            with self.assertRaises(ValueError):
                q.public_onboarding({**setup, 'rejectedPageInventory': {**inventory, 'source': invalid}}, 'chatgpt-desktop')

        blocked = {**setup, 'roleProofFailure': 'control-not-actionable',
                   'actionabilityFailure': 'foreign-overlay'}
        self.assertEqual(q.public_onboarding(blocked, 'chatgpt-desktop'), blocked)
        fingerprint = {**blocked, 'foreignOverlay': 'chatgpt-onboarding-complete'}
        self.assertEqual(q.public_onboarding(fingerprint, 'chatgpt-desktop'), fingerprint)
        proved = {**fingerprint, 'foreignOverlayProof': 'classified'}
        self.assertEqual(q.public_onboarding(proved, 'chatgpt-desktop'), proved)
        surface = {**proved, 'foreignOverlaySurface': 'separate-dialog', 'foreignOverlayFingerprint': 'matched'}
        self.assertEqual(q.public_onboarding(surface, 'chatgpt-desktop'), surface)
        for heading in ('all-set', 'external-import', 'skip-confirmation', 'unknown', 'ambiguous'):
            measured = {**surface, 'foreignOverlayHeading': heading}
            self.assertEqual(q.public_onboarding(measured, 'chatgpt-desktop'), measured)
        for invalid in ('PRIVATE heading', True, None):
            with self.assertRaises(ValueError):
                q.public_onboarding({**surface, 'foreignOverlayHeading': invalid}, 'chatgpt-desktop')
        with self.assertRaises(ValueError):
            q.public_onboarding({**blocked, 'foreignOverlayHeading': 'unknown'}, 'chatgpt-desktop')

        for changed in ({**surface, 'foreignOverlaySurface': 'PRIVATE'},
                        {**surface, 'foreignOverlayFingerprint': True},
                        {**surface, 'foreignOverlaySurface': 'enclosing-role-dialog'},
                        {**surface, 'foreignOverlayProof': 'query-failed'},
                        {**surface, 'rawDialog': 'PRIVATE'},
                        {key: value for key, value in surface.items() if key != 'foreignOverlaySurface'}):
            with self.assertRaises(ValueError):
                q.public_onboarding(changed, 'chatgpt-desktop')

        for role_failure, proof in [('legend-count', 'role-proof-rejected'), ('scope-count', 'role-proof-rejected'),
                                    ('deadline-expired', 'deadline-expired'), ('ownership-lost', 'ownership-lost'),
                                    ('page-count', 'ownership-lost')]:
            specific = {**blocked, 'roleProofFailure': role_failure,
                        'foreignOverlay': 'guard-rejected', 'foreignOverlayProof': proof}
            self.assertEqual(q.public_onboarding(specific, 'chatgpt-desktop'), specific)
            for change in ({'foreignOverlay': 'other'}, {'actionabilityFailure': 'hidden'},
                           {'foreignOverlayProof': 'classified'}, {'foreignOverlayProof': 'document-replaced'}):
                with self.assertRaises(ValueError):
                    q.public_onboarding({**specific, **change}, 'chatgpt-desktop')
        for failure, proof in [('legend-count', 'deadline-expired'), ('deadline-expired', 'ownership-lost'),
                               ('ownership-lost', 'role-proof-rejected')]:
            with self.assertRaises(ValueError):
                q.public_onboarding({**blocked, 'roleProofFailure': failure,
                    'foreignOverlay': 'guard-rejected', 'foreignOverlayProof': proof}, 'chatgpt-desktop')
        rejected = {**blocked, 'foreignOverlay': 'guard-rejected', 'foreignOverlayProof': 'query-failed'}
        self.assertEqual(q.public_onboarding(rejected, 'chatgpt-desktop'), rejected)
        for changed in ({**proved, 'foreignOverlayProof': 'PRIVATE'},
                        {**proved, 'foreignOverlayProof': True},
                        {**proved, 'foreignOverlayProof': 'query-failed'},
                        {**rejected, 'foreignOverlayProof': 'classified'},
                        {**setup, 'foreignOverlayProof': 'query-failed'}):
            with self.assertRaises(ValueError):
                q.public_onboarding(changed, 'chatgpt-desktop')
        for changed in ({**fingerprint, 'foreignOverlay': 'PRIVATE'},
                        {**fingerprint, 'foreignOverlay': True},
                        {**fingerprint, 'rawHeading': "You're all set"},
                        {**fingerprint, 'actionabilityFailure': 'hidden'},
                        {key: value for key, value in fingerprint.items() if key != 'actionabilityFailure'}):
            with self.assertRaises(ValueError):
                q.public_onboarding(changed, 'chatgpt-desktop')
        for changed in ({**blocked, 'actionabilityFailure': 'PRIVATE'},
                        {**blocked, 'actionabilityFailure': True},
                        {**blocked, 'roleProofFailure': 'unmeasured'}):
            with self.assertRaises(ValueError):
                q.public_onboarding(changed, 'chatgpt-desktop')
        for changed in ({**setup, 'label': 'PRIVATE'}, {**setup, 'stage': 'PRIVATE'},
                        {**setup, 'errorCategory': []}, {**setup, 'engineeringChecked': 1},
                        {**setup, 'roleProofFailure': 'PRIVATE'}, {**setup, 'roleProofFailure': []}, {**setup, 'sessionProofFailure': 'PRIVATE'}, {**setup, 'sessionProofFailure': None},
                        {**setup, 'schemaVersion': True}, {**setup, 'diagnosticsOnly': False},
                        {key: value for key, value in setup.items() if key != 'roleScopeAbsent'}):
            with self.assertRaises(ValueError):
                q.public_onboarding(changed, 'chatgpt-desktop')
        with self.assertRaises(ValueError):
            q.public_onboarding(setup, 'pen-desktop')
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            value = dict(schemaVersion=1, mechanism='renderer-inventory', diagnosticsOnly=True,
                         app='chatgpt-desktop', endpointOwned=True, launcherOwned=True, attached=True,
                         pageCount=1, textareaCount=0, editableCount=0, sendCount=0,
                         retryCount=0, newThreadCount=0, loginCount=0, dialogCount=0,
                         errorCategory=None, publicOnboarding=setup)
            (root / 'inventory.json').write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop'), [value])
            self.assertEqual(q.envelope('chatgpt-desktop', 'windows', 'x86_64', 'a' * 40)['qualification'], 'unqualified')

class HermesReadinessTests(unittest.TestCase):
    def test_policy_is_windows_hosted_only_and_profile_is_not_inherited(self):
        with tempfile.TemporaryDirectory() as tmp:
            helper = Path(tmp) / 'helper'
            helper.write_text('synthetic')
            source = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Windows',
                          FEASIBILITY_WINDOWS_PROOF_PYTHON=str(helper), FEASIBILITY_WINDOWS_PROOF_SCRIPT=str(helper),
                          FEASIBILITY_HERMES_READINESS_POLICY='current-catalog',
                          FEASIBILITY_HERMES_CATALOG_PROFILE='foreign')
            env = runner.qualification_environment('hermes-desktop', Path('/facts'), Path('/nanh'), '/app', source)
            self.assertEqual(env['FEASIBILITY_HERMES_CATALOG_PROFILE'], 'nan')
            self.assertEqual(env['FEASIBILITY_HERMES_READINESS_POLICY'], 'current-catalog')
            for changed in ({'RUNNER_OS': 'Linux'}, {'RUNNER_OS': 'macOS'},
                            {'RUNNER_ENVIRONMENT': 'self-hosted'}, {'FEASIBILITY_HERMES_READINESS_POLICY': 'unknown'}):
                with self.assertRaises(ValueError):
                    runner.qualification_environment('hermes-desktop', Path('/facts'), Path('/nanh'), '/app', {**source, **changed})

    def test_readiness_receipt_is_closed_and_never_qualifies_a_cell(self):
        value = dict(schemaVersion=1, mechanism='hermes-windows-catalog-readiness', diagnosticsOnly=True,
                     stage='ready', errorCategory=None, menuOpened=True, refreshAttempted=True,
                     catalogVerified=True, modelRowVerified=True, menuDismissed=True, composerReverified=True)
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'ready.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'hermes-desktop'), [value])
            self.assertEqual(q.envelope('hermes-desktop', 'windows', 'x86_64', 'a' * 40)['qualification'], 'unqualified')
            for changed in ({'diagnosticsOnly': False}, {'catalogVerified': 1}, {'composerReverified': False},
                            {'stage': 'PRIVATE'}, {'errorCategory': 'PRIVATE'}, {'rawFrame': 'PRIVATE'}):
                path.write_text(json.dumps({**value, **changed}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'hermes-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'zed-desktop')

    def test_zed_cursor_counters_are_bounded_and_private(self):
        value = dict(schemaVersion=1, mechanism='zed-pointer-observation', diagnosticsOnly=True,
                     maximizedHorizontal=True, maximizedVertical=True, enabled=True, sensitive=True,
                     showing=True, visible=True, defunct=False, retryContains=True,
                     pointerTarget='client', pointerChild='client')
        selection = dict(status='no-hit', sampledPoints=9, exactPointerMatched=False,
                         accessibleHitVerified=False, guardBeforeVerified=9, guardAfterVerified=9,
                         accessibleChecks=9, accessibleExactMatches=9, cursorChecks=9,
                         cursorExactMatches=0, failureReason='cursor-unmatched')
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'pointer.json'
            path.write_text(json.dumps({**value, 'cursorSelection': selection}))
            self.assertEqual(q.semantic_observations(tmp, 'zed-desktop')[0]['cursorSelection'], selection)
            classified = {**selection, 'cursorClasses': dict(hand=0, arrow=9, notallowed=0, transparent=0, unknown=0),
                          'cursorSizeSource': 'screen'}
            path.write_text(json.dumps({**value, 'cursorSelection': classified}))
            self.assertEqual(q.semantic_observations(tmp, 'zed-desktop')[0]['cursorSelection'], classified)
            sampled = {**classified, 'pointerChecks': 9, 'pointerPositionMatches': 9, 'pointerChildMatches': 9}
            path.write_text(json.dumps({**value, 'cursorSelection': sampled}))
            self.assertEqual(q.semantic_observations(tmp, 'zed-desktop')[0]['cursorSelection'], sampled)
            for changes in ({'pointerChecks': 46}, {'pointerChecks': True},
                            {'pointerPositionMatches': 10}, {'pointerChildMatches': 8},
                            {'pointerChildMatches': 'PRIVATE'}, {'rawPosition': [1, 2]}):
                path.write_text(json.dumps({**value, 'cursorSelection': {**sampled, **changes}}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(tmp, 'zed-desktop')
            for reason in ('pointer-position', 'pointer-child'):
                rejected = {**selection, 'status': 'identity-rejected', 'failureReason': reason,
                            'cursorChecks': 0, 'pointerChecks': 1, 'pointerPositionMatches': 0, 'pointerChildMatches': 0}
                path.write_text(json.dumps({**value, 'cursorSelection': rejected}))
                self.assertEqual(q.semantic_observations(tmp, 'zed-desktop')[0]['cursorSelection'], rejected)
            for changes in ({'cursorSizeSource': 'PRIVATE'}, {'cursorClasses': {'rawPixels': 'PRIVATE'}},
                            {'cursorSizeSource': None}, {'cursorChecks': 8}):
                path.write_text(json.dumps({**value, 'cursorSelection': {**classified, **changes}}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(tmp, 'zed-desktop')
            for changes in ({'failureReason': 'PRIVATE'}, {'cursorChecks': 20},
                            {'accessibleChecks': True}, {'accessibleExactMatches': 10},
                            {'cursorExactMatches': 10}, {'rawPixels': 'PRIVATE'},
                            {'guardBeforeVerified': 8}):
                path.write_text(json.dumps({**value, 'cursorSelection': {**selection, **changes}}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(tmp, 'zed-desktop')

    def test_zed_pointer_modifier_receipts_are_atomic_and_closed(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            value = dict(schemaVersion=1, mechanism='zed-pointer-observation', diagnosticsOnly=True,
                         maximizedHorizontal=True, maximizedVertical=True, enabled=True, sensitive=True,
                         showing=True, visible=True, defunct=False, retryContains=True,
                         pointerTarget='client', pointerChild='client')
            path = root / 'pointer.json'
            for state in ('none', 'shift', 'control', 'lock', 'other-modifier', 'mixed', 'unknown'):
                measured = {**value, 'modifierState': state, 'buttonsHeld': None if state == 'unknown' else False}
                path.write_text(json.dumps(measured))
                self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [measured])
            for changed in ({'modifierState': 'PRIVATE', 'buttonsHeld': False},
                            {'modifierState': 'none'}, {'buttonsHeld': True},
                            {'modifierState': 'unknown', 'buttonsHeld': True},
                            {'modifierState': 'control', 'buttonsHeld': 1},
                            {'modifierState': 'none', 'buttonsHeld': False, 'rawMask': 4}):
                path.write_text(json.dumps({**value, **changed}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')

if __name__ == '__main__':
    unittest.main()
