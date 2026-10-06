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
    def test_reducer_rejection_publishes_counts_and_never_exception_contents(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            output = root / 'qualification.json'
            facts = root / 'facts'
            facts.mkdir()
            for index in range(97):
                (facts / f'fact-{index}.json').write_text('{}')
            report = root / 'report.json'
            report.write_text('{}')
            pending = q.envelope('claude-desktop', 'windows', 'x86_64', 'a' * 40)
            options = dict(app='claude-desktop', platform='windows', architecture='x86_64',
                source_sha='a' * 40, output=output, facts=facts, report=report, model='qwen3.6',
                frozen=root/'frozen', prepared=root/'prepared', checker=root/'checker',
                launcher=root/'launcher', real_nanh=root/'nanh')
            argv = ['desktop_qualification.py', 'reduce']
            for key, value in options.items():
                argv.extend(['--' + key.replace('_', '-'), str(value)])
            output.write_text(json.dumps(pending))
            with patch.object(sys, 'argv', argv), self.assertRaises(SystemExit):
                q.main()
            result = json.loads(output.read_text())
            fact = result['semanticObservations'][0]
            self.assertEqual(fact['category'], 'observation-budget')
            self.assertEqual(fact['observationCount'], 97)
            self.assertTrue(fact['reportPresent'])
            self.assertEqual(result['qualification'], 'unqualified')
            output.write_text(json.dumps(pending))
            with patch.object(sys, 'argv', argv), patch.object(q, 'reduce_report', side_effect=ValueError('PRIVATE_OUTPUT')), self.assertRaises(SystemExit):
                q.main()
            self.assertNotIn('PRIVATE_OUTPUT', output.read_text())
            self.assertEqual(json.loads(output.read_text())['semanticObservations'][0]['category'], 'invalid-evidence')
            receipt_dir = root / 'receipt'
            receipt_dir.mkdir()
            (receipt_dir / 'failure.json').write_text(json.dumps(fact))
            self.assertEqual(q.semantic_observations(receipt_dir, 'claude-desktop'), [fact])
            for change in ({'sourceSha': 'b' * 40}, {'qualification': 'deterministic-full'},
                           {'semanticObservations': [fact]}):
                retained = {**pending, **change}
                output.write_text(json.dumps(retained))
                q.publish_reduction_failure(argparse.Namespace(**options), 'invalid-evidence')
                self.assertEqual(json.loads(output.read_text()), retained)

    def test_absent_report_retains_closed_preparation_failure_without_qualification(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            output = root / 'qualification.json'
            facts = root / 'facts'
            facts.mkdir()
            pending = q.envelope('claude-desktop', 'windows', 'x86_64', 'a' * 40)
            output.write_text(json.dumps(pending))
            receipt = dict(schemaVersion=1, mechanism='windows-foreground-session',
                diagnosticsOnly=True, stage='restore', originalTimeoutMs=200000,
                prepared=False, restored=True, failureStage='prepare')
            (facts / 'foreground.json').write_text(json.dumps(receipt))
            result = q.diagnose_pending('claude-desktop', 'windows', 'x86_64', 'a' * 40, output, facts)
            self.assertEqual(result['qualification'], 'unqualified')
            self.assertEqual(result['probes'], [])
            self.assertEqual(result['semanticObservations'][1], receipt)
            with self.assertRaises(ValueError):
                q.diagnose_pending('claude-desktop', 'windows', 'x86_64', 'b' * 40, output, facts)
            self.assertEqual(json.loads(output.read_text()), pending)

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


class ClaudeQueryTimingTests(unittest.TestCase):
    def test_bounded_timing_is_advisory_and_rejects_private_fields(self):
        timing = dict(calls=2, elapsedMs=11000, nativeWindowMs=2000, lastMs=9000)
        value = dict(schemaVersion=1, mechanism='claude-linux-native-chat', diagnosticsOnly=True,
                     stage='deadline', submittedTurns=1, inputVerifiedTurns=1, copiedResponses=1,
                     retryAttempted=False, clipboardCleared=True, queryObservation=timing)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'facts.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(tmp, 'claude-desktop'), [value])
            for change in ({'path':'PRIVATE'}, {'calls':True}, {'elapsedMs':600001},
                           {'nativeWindowMs':11001}, {'lastMs':-1}):
                path.write_text(json.dumps({**value, 'queryObservation':{**timing, **change}}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(tmp, 'claude-desktop')


class CodexRetainedCustodyTests(unittest.TestCase):
    def test_rejected_custody_preserves_closed_privacy_and_deadline_causes(self):
        value = dict(schemaVersion=1, mechanism='codex-windows-profile-prepare', diagnosticsOnly=True,
                     stage='retained-custody', cause='privacy', bindingIndex=None,
                     ancestorCount=6, ownedCount=10, privacy=['protected', 'inherited']+[None]*9,
                     emptyRoots=[None, None], codeHomeAbsent=None, completed=False)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'facts.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(tmp, 'chatgpt-desktop'), [value])
            deadline = {**value, 'cause':'original-cutoff', 'privacy':[None]*11}
            path.write_text(json.dumps(deadline))
            self.assertEqual(q.semantic_observations(tmp, 'chatgpt-desktop'), [deadline])
            for change in ({'privacy':['PRIVATE']+[None]*10}, {'path':'PRIVATE'}, {'completed':True}):
                path.write_text(json.dumps({**value, **change}))
                with self.assertRaises(ValueError):q.semantic_observations(tmp, 'chatgpt-desktop')


class RendererCheckpointTests(unittest.TestCase):
    def test_parent_failure_boundaries_are_closed(self):
        value = dict(schemaVersion=1, mechanism='renderer-inventory-failure',
                     diagnosticsOnly=True, stage='process-custody', reason='isolation-unavailable')
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp)/'failure.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(tmp, 'chatgpt-desktop'), [value])
            for change in ({'stage':'PRIVATE'}, {'reason':'PRIVATE'}, {'path':'PRIVATE'},
                           {'diagnosticsOnly':False}):
                path.write_text(json.dumps({**value, **change}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(tmp, 'chatgpt-desktop')

    def test_partial_phase_is_closed_and_cannot_claim_completed_inventory(self):
        value=dict(schemaVersion=1,mechanism='renderer-inventory',diagnosticsOnly=True,
            app='chatgpt-desktop',endpointOwned=True,launcherOwned=True,attached=True,
            pageCount=1,textareaCount=0,editableCount=0,sendCount=0,retryCount=0,
            newThreadCount=0,loginCount=0,dialogCount=0,errorCategory='unclassified',
            observerStage='folder-trust')
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/'facts.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(tmp,'chatgpt-desktop'),[value])
            for phase in ('disconnecting', 'disconnected', 'returned', 'disconnect-timeout'):
                closed = {**value, 'observerShutdown':phase}
                path.write_text(json.dumps(closed))
                self.assertEqual(q.semantic_observations(tmp,'chatgpt-desktop'),[closed])
            for changes in ({'observerShutdown':'PRIVATE'},{'observerShutdown':None},{'observerStage':'PRIVATE'},{'observerStage':None},{'errorCategory':None}):
                path.write_text(json.dumps({**value,**changes}))
                with self.assertRaises(ValueError):q.semantic_observations(tmp,'chatgpt-desktop')


    def test_onboarding_progress_survives_before_later_inventory(self):
        value=dict(schemaVersion=1,mechanism='renderer-inventory',diagnosticsOnly=True,
            app='chatgpt-desktop',endpointOwned=True,launcherOwned=True,attached=True,
            pageCount=1,textareaCount=0,editableCount=0,sendCount=0,retryCount=0,
            newThreadCount=0,loginCount=0,dialogCount=0,errorCategory='unclassified',
            observerStage='folder-trust',sourceDialogPhase='finished',
            folderTrustObservation=dict(phase='guard',status='blocked',clickAttempted=False,
                                        clickCompleted=False))
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/'facts.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(tmp,'chatgpt-desktop'),[value])
            for change in ({'sourceDialogPhase':'PRIVATE'}, {'sourceDialogPhase':None},
                           {'folderTrustObservation':{**value['folderTrustObservation'],'phase':'PRIVATE'}},
                           {'folderTrustObservation':{**value['folderTrustObservation'],'path':'PRIVATE'}},
                           {'folderTrustObservation':{**value['folderTrustObservation'],'status':'completed'}}):
                path.write_text(json.dumps({**value,**change}))
                with self.assertRaises(ValueError):q.semantic_observations(tmp,'chatgpt-desktop')

    def test_guard_progress_is_closed_and_keeps_partial_work_unqualified(self):
        guard = dict(phase='main-identity', elapsedMs=3000, nativeProofCount=2,
                     nativeProofMs=1800, identityCount=1, identityMs=20)
        value = dict(schemaVersion=1, mechanism='renderer-inventory', diagnosticsOnly=True,
            app='chatgpt-desktop', endpointOwned=True, launcherOwned=True, attached=True,
            pageCount=1, textareaCount=0, editableCount=0, sendCount=0, retryCount=0,
            newThreadCount=0, loginCount=0, dialogCount=0, errorCategory='unclassified',
            observerStage='folder-trust', mainGuardObservation=guard)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'facts.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(tmp, 'chatgpt-desktop'), [value])
            for change in ({'phase': 'PRIVATE'}, {'path': 'PRIVATE'}, {'identityMs': True},
                           {'nativeProofCount': 4097}, {'elapsedMs': -1}, {'identityMs': 600001}):
                path.write_text(json.dumps({**value, 'mainGuardObservation': {**guard, **change}}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(tmp, 'chatgpt-desktop')


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
                            {'retryCompleted': True}, {'bindingVerified': 1},
                            {'retryClickPhase':'PRIVATE'}, {'retryClickPhase':True}):
                path.write_text(json.dumps({**value, **changes}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(tmp, 'chatgpt-desktop')
            for failure in ('request-json', 'request-policy', 'connection-read',
                            'binding-read', 'connection-schema', 'binding-schema'):
                receipt = {**value, 'errorCategory': 'invalid-request', 'preAttachFailure': failure}
                path.write_text(json.dumps(receipt))
                self.assertEqual(q.semantic_observations(tmp, 'chatgpt-desktop'), [receipt])
                for change in ({'preAttachFailure': 'PRIVATE'}, {'attached': True},
                               {'errorCategory': None}):
                    path.write_text(json.dumps({**receipt, **change}))
                    with self.assertRaises(ValueError):
                        q.semantic_observations(tmp, 'chatgpt-desktop')
            observed = dict(overflow=False, homeComposerCount=0, pendingTextareaCount=0,
                            proseMirrorEditableCount=0, workspaceControlCount=0, editableCount=1,
                            codexThreadCount=0, classicChatGPTCount=1)
            receipt = {**value, 'composerAdmissionFailure': 'scope-not-ready',
                       'composerReadinessObservation': observed}
            path.write_text(json.dumps(receipt))
            self.assertEqual(q.semantic_observations(tmp, 'chatgpt-desktop'), [receipt])
            for change in ({'composerAdmissionFailure': 'PRIVATE'}, {'errorCategory': None},
                           {'composerReadinessObservation': {**observed, 'rawText': 'PRIVATE'}},
                           {'composerReadinessObservation': {**observed, 'editableCount': 33}},
                           {'composerReadinessObservation': {**observed, 'editableCount': None}}):
                path.write_text(json.dumps({**receipt, **change}))
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
            scoped = source | {'NANH_CODEX_INPUT_CHANNEL': 'cdp-dom'}
            for platform in ('Linux', 'macOS', 'Windows'):
                selected = runner.qualification_environment('chatgpt-desktop', root, helper,
                    str(helper), scoped | {'RUNNER_OS': platform})
                self.assertEqual(selected['NANH_CODEX_INPUT_CHANNEL'], 'cdp-dom')
            for change in [{'NANH_CODEX_INPUT_CHANNEL': 'native'},
                           {'NANH_CODEX_PUBLIC_ONBOARDING': None},
                           {'RUNNER_ENVIRONMENT': 'self-hosted'}]:
                with self.assertRaises(ValueError):
                    runner.qualification_environment('chatgpt-desktop', root, helper,
                        str(helper), scoped | change)
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

    def test_hermes_response_shape_distinguishes_wrong_turn_without_text(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'backend.json'
            shape = dict(exactUserCount=1, markerAssistantCount=1, boundMarkerAssistantCount=0)
            value = dict(schemaVersion=1, mechanism='hermes-backend-failure', diagnosticsOnly=True,
                         category='unclassified', assistantTurnCount=3, responseShape=shape)
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(Path(root), 'hermes-desktop'), [value])
            for changed in ({**shape, 'text': 'PRIVATE'}, {**shape, 'exactUserCount': True},
                            {**shape, 'boundMarkerAssistantCount': 2},
                            {**shape, 'markerAssistantCount': 4},
                            {**shape, 'exactUserCount': 4097}):
                path.write_text(json.dumps({**value, 'responseShape': changed}))
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

    def test_preparation_failure_is_closed_and_cannot_qualify_an_app(self):
        for apps, expected in [([], 'receipt-shape'), ({}, 'receipt-shape'),
                              ([None], 'receipt-shape'),
                              ([{'app': 'zed-desktop'}], 'receipt-shape'),
                              ([{'app': 'chatgpt-desktop', 'blocked': 'unsupported-version'}], 'unsupported-version'),
                              ([{'app': 'chatgpt-desktop', 'blocked': 'PRIVATE'}], 'unclassified'),
                              ([{'app': 'chatgpt-desktop', 'blocked': {}}], 'unclassified')]:
            with self.assertRaises(runner.PreparedAppUnavailable) as caught:
                runner.require_prepared_app(apps, 'chatgpt-desktop')
            self.assertEqual(caught.exception.reason, expected)
            self.assertNotIn('PRIVATE', str(caught.exception))
        runner.require_prepared_app([{'app': 'chatgpt-desktop', 'blocked': None}], 'chatgpt-desktop')
        with tempfile.TemporaryDirectory() as root:
            directory = Path(root).resolve()
            args = argparse.Namespace(directory=directory, app='chatgpt-desktop',
                                      platform='windows', source_sha='a' * 40)
            value = q.envelope(args.app, args.platform, 'x86_64', args.source_sha)
            output = directory / 'qualification.json'
            env = {'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted'}
            for reason in ('PRIVATE', {}, 'unsupported-version'):
                output.write_text(json.dumps(value))
                with patch.dict(runner.os.environ, env, clear=True):
                    runner.publish_runner_failure(args, 'prepared-app-unavailable', reason)
                observed = json.loads(output.read_text())
                if reason != 'unsupported-version':
                    self.assertEqual(observed, value)
                    continue
                self.assertEqual(observed['qualification'], 'unqualified')
                self.assertEqual(observed['probes'], [])
                fact = observed['semanticObservations'][0]
                facts = directory / 'facts'
                facts.mkdir()
                path = facts / 'failure.json'
                path.write_text(json.dumps(fact))
                self.assertEqual(q.semantic_observations(facts, args.app)[0]['preparationReason'], reason)
                for bad in ({**fact, 'preparationReason': 'PRIVATE'},
                            {**fact, 'errorCategory': 'execution-failed'}):
                    path.write_text(json.dumps(bad))
                    with self.assertRaises(ValueError):
                        q.semantic_observations(facts, args.app)

    def test_exact_initial_matrix_separates_scenario_backends_and_inventories(self):
        cells = q.matrix()['include']
        self.assertEqual(len(cells), 15)
        self.assertEqual(len({(c['app'], c['platform'], c['architecture']) for c in cells}), 15)
        self.assertEqual(sum(c['backend'] != 'renderer-inventory' for c in cells), 12)
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

    def test_windows_claude_uia_is_passive_strict_and_private(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'uia.json'
            value = dict(schemaVersion=1, mechanism='claude-windows-uia', diagnosticsOnly=True,
                         phase='post-ready', status='observed', nativeGuardVerified=True, treeComplete=True,
                         nodeCount=12, classicEditorCount=1, modernEditorCount=0, sendControlCount=1,
                         startTaskControlCount=0, assistantHeadingCount=1, copyControlCount=1)
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop')[0]['classicEditorCount'], 1)
            for key, invalid in [('rawName', 'PRIVATE'), ('status', 'PRIVATE'), ('nodeCount', 0),
                                 ('copyControlCount', 13), ('nativeGuardVerified', False), ('classicEditorCount', True)]:
                path.write_text(json.dumps({**value, key: invalid}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'zed-desktop')

    def test_windows_uia_process_failure_stages_remain_private_and_blocked(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'uia.json'
            value = dict(schemaVersion=1, mechanism='claude-windows-uia', diagnosticsOnly=True,
                         phase='post-ready', nativeGuardVerified=False, treeComplete=False,
                         nodeCount=None, classicEditorCount=None, modernEditorCount=None,
                         sendControlCount=None, startTaskControlCount=None,
                         assistantHeadingCount=None, copyControlCount=None)
            for stage in ('root-process-query', 'root-process-mismatch', 'root-process-zero', 'root-process-invalid',
                          'descendant-process-query', 'descendant-process-mismatch', 'descendant-process-zero', 'descendant-process-invalid',
                          'owned-descendant-process', 'foreign-descendant-process', 'descendant-correlation-unavailable'):
                path.write_text(json.dumps({**value, 'status': stage}))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop')[0]['status'], stage)
                for change in ({'processId': 'PRIVATE'}, {'nodeCount': 1},
                               {'nativeGuardVerified': True}, {'status': stage + '-PRIVATE'}):
                    path.write_text(json.dumps({**value, 'status': stage, **change}))
                    with self.assertRaises(ValueError):
                        q.semantic_observations(root, 'claude-desktop')

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
                          retryControlCount=0, retryControlCountAfterActivation=1, retrySelector='retry-name-or-description', retryActionReceipt='completion-unknown',
                          input={'submitted': True, 'clipboardVerified': True, 'private': 'PRIVATE'},
                          response={'clipboardVerified': False, 'providerVerified': True})
            path.write_text(json.dumps(native))
            public = q.semantic_observations(root, 'zed-desktop')
            self.assertEqual(public[0]['substage'], 'retry-control-query')
            self.assertTrue(public[0]['inputSubmitted'])
            self.assertEqual(public[0]['retryControlCount'], 0)
            self.assertEqual(public[0]['retryControlCountAfterActivation'], 1)
            self.assertEqual(public[0]['retrySelector'], 'retry-name-or-description')
            for field, invalid in [('retryControlCount', True), ('retryControlCount', 4097),
                                   ('retryControlCountAfterActivation', True), ('retryControlCountAfterActivation', 4097),
                                   ('retrySelector', 'PRIVATE_SYNTHETIC'), ('retryActionReceipt', 'PRIVATE')]:
                path.write_text(json.dumps({**native, field: invalid}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')
            for field in ('retryControlCountAfterReadback', 'retryErrorTitleCountBeforeActivation',
                          'retryErrorTitleCountAfterActivation', 'retryErrorTitleCountAfterReadback'):
                for count in (None, 0, 1):
                    path.write_text(json.dumps({**native, field: count}))
                    observation = q.semantic_observations(root, 'zed-desktop')[0]
                    self.assertEqual(observation[field], count)
                    self.assertFalse(observation['responseVerified'])
                for invalid in (True, -1, 4097, 'PRIVATE'):
                    path.write_text(json.dumps({**native, field: invalid}))
                    with self.assertRaises(ValueError):
                        q.semantic_observations(root, 'zed-desktop')
            receipt = dict(status='complete', sessionFound=1, sessionMissing=0, resumeMessages=1, ordinarySend=0, turnStarted=1, turnCompleted=0, turnFailed=1, turnCancelled=0)
            path.write_text(json.dumps({**native, 'retryLogObservation': receipt}))
            self.assertEqual(q.semantic_observations(root, 'zed-desktop')[0]['retryLogObservation'], receipt)
            for bad in ({**receipt, 'raw': 'PRIVATE'}, {**receipt, 'status': 'rotated'},
                        {**receipt, 'sessionFound': True},
                        {**receipt, 'sessionFound': 256},
                        {**receipt, 'status': 'PRIVATE'}):
                path.write_text(json.dumps({**native, 'retryLogObservation': bad}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')
            current = {key: item for key, item in receipt.items() if key not in {'resumeMessages', 'ordinarySend'}}
            current.update(messageTotals=1, priorTurnObserved=True)
            path.write_text(json.dumps({**native, 'retryLogObservation': current}))
            self.assertEqual(q.semantic_observations(root, 'zed-desktop')[0]['retryLogObservation'], current)
            for bad in ({**current, 'priorTurnObserved': 1}, {**current, 'resumeMessages': 1},
                        {**current, 'messageTotals': True}, {**current, 'raw': 'PRIVATE'}):
                path.write_text(json.dumps({**native, 'retryLogObservation': bad}))
                with self.assertRaises(ValueError):q.semantic_observations(root, 'zed-desktop')
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

    def test_windows_chat_only_mode_and_capabilities_are_passive_closed(self):
        base = dict(schemaVersion=1, mechanism='claude-windows-uia', diagnosticsOnly=True,
                    phase='post-ready', status='observed', nativeGuardVerified=True, treeComplete=True,
                    nodeCount=107, classicEditorCount=1, modernEditorCount=0, sendControlCount=0,
                    startTaskControlCount=1, assistantHeadingCount=0, copyControlCount=0)
        mode = dict(status='chat', modeGroupCount=1, chatCount=1, coworkCount=0,
                    currentChatCount=1, currentCoworkCount=0)
        capability = dict(status='observed', valuePattern=True, valueReadOnly=False, valueEmpty=True,
                          password=False, keyboardFocusable=True, startTaskInvokePattern=True)
        with tempfile.TemporaryDirectory() as root:
            path=Path(root)/'uia.json'
            for status in ('chat', 'missing'):
                path.write_text(json.dumps({**base, 'currentMode': {**mode, 'status': status}}))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop')[0]['currentMode']['status'], status)
            valid={**base, 'currentMode':mode, 'chatCapability':capability}
            path.write_text(json.dumps(valid))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop')[0]['chatCapability'],capability)
            for change in ({'valueEmpty':'PRIVATE'}, {'status':'changed'}, {'path':'PRIVATE'},
                           {'valuePattern':False}, {'keyboardFocusable':None}):
                path.write_text(json.dumps({**valid,'chatCapability':{**capability,**change}}))
                with self.assertRaises(ValueError): q.semantic_observations(root, 'claude-desktop')
            unknown={key:None for key in capability if key!='status'}
            path.write_text(json.dumps({**valid,'chatCapability':{**unknown,'status':'unavailable'}}))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop')[0]['chatCapability']['status'],'unavailable')
            path.write_text(json.dumps({**valid,'currentMode':{**mode,'status':'missing'}}))
            with self.assertRaises(ValueError): q.semantic_observations(root, 'claude-desktop')

    def test_recovery_provider_receipt_survives_failed_ui_readback(self):
        oracle = dict(schemaVersion=1, mechanism='semantic-provider-oracle', stage='recovery',
                      toolCompleted=True, toolRecordingBounded=True, toolVerified=True,
                      fixtureResponseVerified=False, failureObserved=True, providerGenerationCount=4)
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'provider.json'
            path.write_text(json.dumps(oracle))
            observed = q.semantic_observations(root, 'zed-desktop')[0]
            self.assertEqual(observed['providerGenerationCount'], 4)
            self.assertFalse(observed['fixtureResponseVerified'])
            for invalid in (True, -1, 100001, 'PRIVATE'):
                path.write_text(json.dumps({**oracle, 'providerGenerationCount': invalid}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')

    def test_tool_result_diagnostics_are_closed_and_do_not_certify_file_read(self):
        oracle = dict(schemaVersion=1, mechanism='semantic-provider-oracle', stage='tool',
                      toolCompleted=True, toolRecordingBounded=True, toolVerified=False,
                      fixtureResponseVerified=True, failureObserved=False)
        result = dict(selectedTool='read', resultPresent=True, resultCount=1,
                      status='complete', shape='string', toolErrorDetected=True,
                      errorCategory='file-not-found')
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'provider.json'
            path.write_text(json.dumps({**oracle, 'toolResult': result}))
            public = q.semantic_observations(root, 'claude-desktop')[0]
            self.assertEqual(public['toolResult'], result)
            self.assertFalse(public['toolVerified'])
            for change in ({'selectedTool': 'PRIVATE'}, {'resultCount': True},
                           {'resultCount': 33}, {'resultPresent': False},
                           {'shape': 'absent'}, {'toolErrorDetected': False},
                           {'path': 'PRIVATE'}, {'errorCategory': 'PRIVATE'}):
                path.write_text(json.dumps({**oracle, 'toolResult': {**result, **change}}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps({**oracle, 'stage': 'failure', 'toolResult': result}))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'claude-desktop')
            absent = dict(selectedTool='exec-command', resultPresent=False, resultCount=0,
                          status='limit', shape='absent', toolErrorDetected=False,
                          errorCategory='none')
            path.write_text(json.dumps({**oracle, 'toolResult': absent}))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop')[0]['toolResult'], absent)
            path.write_text(json.dumps(oracle))
            self.assertNotIn('toolResult', q.semantic_observations(root, 'claude-desktop')[0])

    def test_exec_result_is_optional_closed_and_noncertifying(self):
        oracle = dict(schemaVersion=1, mechanism='semantic-provider-oracle', stage='tool',
                      toolCompleted=True, toolRecordingBounded=True, toolVerified=False,
                      fixtureResponseVerified=True, failureObserved=False)
        result = dict(selectedTool='exec-command', resultPresent=True, resultCount=1,
                      status='complete', shape='string', toolErrorDetected=False,
                      errorCategory='none')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / 'semantic.json'
            for state in ['launch-failed', 'exited-zero', 'exited-nonzero', 'running', 'unknown', 'ambiguous']:
                path.write_text(json.dumps({**oracle, 'toolResult': {**result, 'execResult': state}}))
                public = q.semantic_observations(root, 'chatgpt-desktop')[0]
                self.assertEqual(public['toolResult']['execResult'], state)
                self.assertFalse(public['toolVerified'])
            for change in [{'execResult': 'PRIVATE'}, {'execResult': None},
                           {'execResult': 'running', 'selectedTool': 'read'},
                           {'execResult': 'unknown', 'resultPresent': False, 'resultCount': 0, 'shape': 'absent'},
                           {'execResult': 'exited-zero', 'exitCode': 0}]:
                path.write_text(json.dumps({**oracle, 'toolResult': {**result, **change}}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'chatgpt-desktop')

    def test_tool_failure_hint_is_closed_and_requires_failure(self):
        oracle = dict(schemaVersion=1, mechanism='semantic-provider-oracle', stage='tool',
                      toolCompleted=True, toolRecordingBounded=True, toolVerified=False,
                      fixtureResponseVerified=True, failureObserved=False)
        result = dict(selectedTool='exec-command', resultPresent=True, resultCount=1,
                      status='complete', shape='string', toolErrorDetected=False,
                      errorCategory='none', execResult='exited-nonzero')
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'provider.json'
            for hint in ['permission-denied', 'missing-file', 'invalid-path', 'sandbox',
                         'missing-command', 'unsupported', 'unknown', 'ambiguous']:
                path.write_text(json.dumps({**oracle, 'toolResult': {**result, 'failureHint': hint}}))
                public = q.semantic_observations(root, 'chatgpt-desktop')[0]
                self.assertEqual(public['toolResult']['failureHint'], hint)
                self.assertFalse(public['toolVerified'])
            for change in [{'failureHint': 'PRIVATE'}, {'failureHint': None},
                           {'failureHint': 'unknown', 'execResult': 'exited-zero'},
                           {'failureHint': 'unknown', 'execResult': 'running'}]:
                path.write_text(json.dumps({**oracle, 'toolResult': {**result, **change}}))
                with self.assertRaises(ValueError):q.semantic_observations(root, 'chatgpt-desktop')

    def test_tool_error_envelope_is_optional_closed_and_noncertifying(self):
        oracle = dict(schemaVersion=1, mechanism='semantic-provider-oracle', stage='tool',
                      toolCompleted=True, toolRecordingBounded=True, toolVerified=False,
                      fixtureResponseVerified=True, failureObserved=False)
        result = dict(selectedTool='read', resultPresent=True, resultCount=1,
                      status='complete', shape='string', toolErrorDetected=True,
                      errorCategory='unknown')
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'provider.json'
            for envelope in ('single-xml-read-wrapper', 'single-xml-other', 'plain-read-wrapper',
                             'plain-other', 'multiple-or-incomplete-xml', 'mixed-fragments'):
                path.write_text(json.dumps({**oracle, 'toolResult': {**result, 'errorEnvelope': envelope}}))
                public = q.semantic_observations(root, 'claude-desktop')[0]
                self.assertEqual(public['toolResult']['errorEnvelope'], envelope)
                self.assertFalse(public['toolVerified'])
            for change in ({'errorEnvelope': None}, {'errorEnvelope': 'PRIVATE'},
                           {'errorEnvelope': {}}, {'errorEnvelope': True},
                           {'errorEnvelope': 'plain-read-wrapper', 'selectedTool': 'read-file'},
                           {'errorEnvelope': 'plain-other', 'toolErrorDetected': False, 'errorCategory': 'none'}):
                path.write_text(json.dumps({**oracle, 'toolResult': {**result, **change}}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            nonerror = {**result, 'toolErrorDetected': False, 'errorCategory': 'none', 'errorEnvelope': None}
            path.write_text(json.dumps({**oracle, 'toolResult': nonerror}))
            self.assertIsNone(q.semantic_observations(root, 'claude-desktop')[0]['toolResult']['errorEnvelope'])

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

    def test_runtime_rejection_retains_closed_fresh_proofs_and_guard(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'focus.json'
            value = dict(schemaVersion=1, mechanism='claude-window-focus', diagnosticsOnly=True,
                         phase='runtime-rejection', guardCategory='same-process-window',
                         status='proved', nativeForegroundWindowMatchedHeld=True, query=None,
                         windowOnlyStatus='query-error', windowOnlyMatchedHeld=None,
                         windowOnlyQuery=dict(phase='before', stage='main-window', error='cannot-complete'))
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(Path(root), 'claude-desktop'), [value])
            for change in ({'guardCategory': None}, {'guardCategory': 'PRIVATE'},
                           {'guardCategory': True}, {'candidateState': 'proved'},
                           {'windowOnlyMatchedHeld': True}, {'windowId': 'PRIVATE'}):
                path.write_text(json.dumps({**value, **change}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(Path(root), 'claude-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(Path(root), 'zed-desktop')
            del value['guardCategory']
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(Path(root), 'claude-desktop')

    def test_windows_current_mode_is_closed_optional_and_advisory(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'uia.json'
            value = dict(schemaVersion=1, mechanism='claude-windows-uia', diagnosticsOnly=True,
                         phase='post-ready', status='observed', nativeGuardVerified=True, treeComplete=True,
                         nodeCount=141, classicEditorCount=1, modernEditorCount=0, sendControlCount=0,
                         startTaskControlCount=1, assistantHeadingCount=0, copyControlCount=0)
            mode = dict(status='chat', modeGroupCount=1, chatCount=1, coworkCount=1,
                        currentChatCount=1, currentCoworkCount=0)
            path.write_text(json.dumps({**value, 'currentMode': mode}))
            self.assertEqual(q.semantic_observations(Path(root), 'claude-desktop')[0]['currentMode'], mode)
            for status in ('unavailable', 'changed'):
                unknown = {key: None for key in mode if key != 'status'}
                path.write_text(json.dumps({**value, 'currentMode': {**unknown, 'status': status}}))
                q.semantic_observations(Path(root), 'claude-desktop')
            for change in ({'status': 'PRIVATE'}, {'status': 'changed'}, {'currentCoworkCount': 1},
                           {'chatCount': True}, {'modeGroupCount': 142}, {'rawLabel': 'PRIVATE'}):
                path.write_text(json.dumps({**value, 'currentMode': {**mode, **change}}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(Path(root), 'claude-desktop')
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(Path(root), 'claude-desktop')[0], value)

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
        scoped = value | {'inputChannel': 'cdp-dom', 'mainDocumentFocused': False}
        self.assertEqual(q.main_aux_correlation(scoped, 'chatgpt-desktop'), scoped)
        for changes in [{'inputChannel': 'native-focused'}, {'inputChannel': {}},
                        {'auxDocumentFocused': True}, {'heldMainUnchanged': False}]:
            with self.assertRaises(ValueError):
                q.main_aux_correlation(scoped | changes, 'chatgpt-desktop')
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

    def test_owned_read_fixture_offer_count_is_optional_closed_and_claude_only(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'inventory.json'
            value = dict(schemaVersion=1, mechanism='semantic-inventory', requestCount=1,
                         toolCount=31, knownReadToolCount=1, readToolSelected=True)
            for count in (0, 1, None):
                path.write_text(json.dumps({**value, 'ownedReadFixtureToolCount': count}))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop')[0]
                                 ['ownedReadFixtureToolCount'], count)
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')
            for count in (True, -1, 32, 4097, 'PRIVATE', {'name': 'PRIVATE'}):
                path.write_text(json.dumps({**value, 'ownedReadFixtureToolCount': count}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps(value))
            self.assertNotIn('ownedReadFixtureToolCount', q.semantic_observations(root, 'claude-desktop')[0])

    def test_owned_fixture_selection_is_closed_scoped_and_consistent(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'inventory.json'
            value = dict(schemaVersion=1, mechanism='semantic-inventory', requestCount=2,
                         toolCount=2, knownReadToolCount=0, ownedReadFixtureToolCount=2)
            for status in ('selected', 'missing', 'ambiguous', 'schema-mismatch', 'limit'):
                receipt = {**value, 'readToolSelected': status == 'selected', 'ownedReadFixtureSelection': status}
                path.write_text(json.dumps(receipt))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop')[0]['ownedReadFixtureSelection'], status)
                with self.assertRaises(ValueError): q.semantic_observations(root, 'hermes-desktop')
                path.write_text(json.dumps({**receipt, 'readToolSelected': status != 'selected'}))
                with self.assertRaises(ValueError): q.semantic_observations(root, 'claude-desktop')
            for status in (None, True, 'PRIVATE', {'path':'PRIVATE'}):
                path.write_text(json.dumps({**value, 'readToolSelected': False, 'ownedReadFixtureSelection': status}))
                with self.assertRaises(ValueError): q.semantic_observations(root, 'claude-desktop')
            value.pop('ownedReadFixtureToolCount')
            path.write_text(json.dumps({**value, 'readToolSelected': False, 'ownedReadFixtureSelection':'missing'}))
            with self.assertRaises(ValueError): q.semantic_observations(root, 'claude-desktop')

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

    def trial(self, mutate=lambda report: None, policy_count=3, app='hermes-desktop', observations=()):
        with tempfile.TemporaryDirectory() as root:
            paths = {key: Path(root) / key for key in ('checker', 'launcher', 'real_nanh', 'prepared', 'frozen', 'report')}
            for key, path in paths.items():
                path.write_text(key)
            probe = dict(status='passed', steps=sorted(q.STEPS), inputMode='renderer-dom-and-keyboard', responseVerification='renderer-dom')
            report = dict(schemaVersion=3, platform='linux', architecture='x86_64',
                          nanHarness={'sha256': q.digest(paths['launcher'])}, cleanup='passed',
                          results=[dict(app=app, appVersion='0.17.6', cleanup='passed',
                                        deterministic=[copy.deepcopy(probe) for _ in range(3)])])
            if app in {'claude-desktop', 'zed-desktop'}:
                for item in report['results'][0]['deterministic']:
                    item.update(inputMode='native-clipboard-and-keyboard', responseVerification=(
                        'native-thread-export' if app == 'zed-desktop' else 'native-assistant-clipboard'))
            mutate(report)
            manifest = {'apps': [dict(status='frozen', app=app, version='0.17.6', revision='b' * 40)]}
            receipt = dict(schemaVersion=2, platform='linux', architecture='x86_64',
                           checker={'sha256': q.digest(paths['checker'])},
                           nanh={'sha256': q.digest(paths['launcher'])},
                           frozen={'sha256': q.digest(paths['frozen']), 'model': 'qwen3.6'}, apps= [dict(app=app, executable={'sha256': 'c' * 64})])
            with patch.object(q, 'read_frozen_manifest', return_value=manifest), \
                 patch.object(q, 'bounded_json', return_value=receipt), \
                 patch.object(q, 'semantic_observations', return_value=[dict(
                     schemaVersion=1, mechanism='hermes-retry-policy', policy='explicit-ui-retry',
                     autoRecoveryCycles=0, apiMaxRetries=3, configBeforeSha256='e' * 64,
                     configAfterSha256='f' * 64) for _ in range(policy_count)] + list(observations)), \
                 patch.object(q, 'validated_report', return_value=(report, 'd' * 64)):
                return q.reduce_report(app=app, platform='linux', architecture='x86_64',
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

    def test_foreground_preparation_and_restoration_are_required_when_present(self):
        value = dict(schemaVersion=1, mechanism='windows-foreground-session', diagnosticsOnly=True,
                     stage='completed', originalTimeoutMs=200000, prepared=True, restored=True)
        self.assertEqual(self.trial(app='claude-desktop', policy_count=0,
                                   observations=[value])['qualification'], 'deterministic-full')
        for change in ({'stage': 'restore', 'restored': False}, {'stage': 'restore', 'prepared': False}):
            self.assertEqual(self.trial(app='claude-desktop', policy_count=0,
                observations=[{**value, **change}])['qualification'], 'unqualified')
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'foreground.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(directory, 'claude-desktop'), [value])
            for change in ({'originalTimeoutMs': True}, {'originalTimeoutMs': -1},
                           {'originalTimeoutMs': 4294967296}, {'restored': False},
                           {'prepared': False}, {'stage': 'PRIVATE'}, {'pid': 123}):
                path.write_text(json.dumps({**value, **change}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(directory, 'claude-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(directory, 'zed-desktop')

    def test_entry_tracing_never_promotes_a_full_native_success(self):
        value = dict(schemaVersion=1, mechanism='zed-retry-entry-counts', diagnosticsOnly=True,
                     status='complete', stage='complete', cleanup='passed', retryEntries=3, nativeRetryEntries=3,
                     inputDispatchEntries=300)
        self.assertEqual(self.trial(app='zed-desktop', policy_count=0)['qualification'], 'deterministic-full')
        result = self.trial(app='zed-desktop', policy_count=0, observations=[value])
        self.assertEqual(result['qualification'], 'unqualified')
        self.assertEqual(result['reason'], 'instrumented-diagnostic')
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'trace.json'
            unavailable = {**value, 'status': 'unavailable', 'stage': 'attach',
                           'retryEntries': None, 'nativeRetryEntries': None, 'inputDispatchEntries': None}
            for category in ('tracer-error', 'readiness-incomplete', 'tracer-exited'):
                record = {**unavailable, 'attachFailure': category}
                path.write_text(json.dumps(record))
                self.assertEqual(q.semantic_observations(directory, 'zed-desktop'), [record])
            record = {**unavailable, 'stage': 'readback', 'readbackFailure': 'user-memory-read', 'markerReadFaults': 1}
            path.write_text(json.dumps(record))
            self.assertEqual(q.semantic_observations(directory, 'zed-desktop'), [record])
            for change in ({'stage': 'attach'}, {'readbackFailure': 'PRIVATE'},
                           {'markerReadFaults': True}, {'markerReadFaults': 1025}):
                path.write_text(json.dumps({**record, **change}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(directory, 'zed-desktop')
            path.write_text(json.dumps({**unavailable, 'attachFailure': 'PRIVATE diagnostic'}))
            with self.assertRaises(ValueError):
                q.semantic_observations(directory, 'zed-desktop')
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(directory, 'zed-desktop'), [value])
            legacy = {key: entry for key, entry in value.items() if key != 'inputDispatchEntries'}
            path.write_text(json.dumps(legacy))
            self.assertEqual(q.semantic_observations(directory, 'zed-desktop'), [legacy])
            windows = dict(started=3, ended=3, windows=[dict(inputDispatchEntries=2,
                retryEntries=0, nativeRetryEntries=0, errorClearEntries=0) for _ in range(3)])
            path.write_text(json.dumps({**value, 'activationWindows': windows}))
            self.assertEqual(q.semantic_observations(directory, 'zed-desktop')[0]['activationWindows'], windows)
            geometry_item = dict(status='matched', renderedHitboxes=10, boundsMatches=1,
                priorPointerMatches=True, targetMaskContainsPoint=True, blockingHitboxesAhead=1,
                targetWouldBeHovered=False)
            geometry = dict(status='complete', windows=[geometry_item] * 3)
            measured = {**value, 'activationWindows': windows, 'hitTestGeometry': geometry}
            path.write_text(json.dumps(measured))
            self.assertEqual(q.semantic_observations(directory, 'zed-desktop')[0]['hitTestGeometry'], geometry)
            profile = dict(unoccludedGridMask=0, topBlocker=dict(behavior='block-mouse',
                distanceFromTarget=3, distanceFromFront=2, coversTarget=True, coversViewport=False))
            profiled_item = {**geometry_item, 'occlusionProfile': profile}
            profiled_geometry = {**geometry, 'windows': [profiled_item] * 3}
            path.write_text(json.dumps({**measured, 'hitTestGeometry': profiled_geometry}))
            self.assertEqual(q.semantic_observations(directory, 'zed-desktop')[0]['hitTestGeometry'], profiled_geometry)
            for change in ({'unoccludedGridMask': True}, {'unoccludedGridMask': 512},
                           {'unoccludedGridMask': 1}, {'topBlocker': None}, {'bounds': [1, 2, 3, 4]}):
                with self.assertRaises(ValueError):
                    q.validate_occlusion_profile({**profiled_item, 'occlusionProfile': {**profile, **change}})
            for change in ({'behavior': 'PRIVATE'}, {'distanceFromTarget': True},
                           {'distanceFromTarget': 0}, {'distanceFromFront': 7},
                           {'coversViewport': 'PRIVATE'}, {'bounds': [1, 2, 3, 4]}):
                with self.assertRaises(ValueError):
                    q.validate_occlusion_profile({**profiled_item, 'occlusionProfile': {
                        **profile, 'topBlocker': {**profile['topBlocker'], **change}}})
            for change in ({'targetWouldBeHovered': True}, {'boundsMatches': True},
                           {'blockingHitboxesAhead': 10}, {'bounds': [1, 2, 3, 4]},
                           {'priorPointerMatches': 'PRIVATE'}, {'status': 'absent'}):
                altered = {**geometry, 'windows': [{**geometry_item, **change}] * 3}
                path.write_text(json.dumps({**measured, 'hitTestGeometry': altered}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(directory, 'zed-desktop')
            for geometry in (dict(status='complete', windows=[]), dict(status='unavailable', windows=[geometry_item])):
                path.write_text(json.dumps({**measured, 'hitTestGeometry': geometry}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(directory, 'zed-desktop')
            returns = dict(inputDispatchReturns=2, inputPropagationStops=1, inputDefaultPreventions=1,
                           inputInvalidReturns=0, hoverTrueReturns=4, hoverFalseReturns=20, hoverInvalidReturns=0)
            extended = {**windows, 'windows': [{**item, **returns} for item in windows['windows']]}
            path.write_text(json.dumps({**value, 'activationWindows': extended}))
            self.assertEqual(q.semantic_observations(directory, 'zed-desktop')[0]['activationWindows'], extended)
            for change in ({'inputPropagationStops': 3}, {'inputDefaultPreventions': True},
                           {'hoverTrueReturns': -1}, {'hoverFalseReturns': 65537}, {'rawReturn': 'PRIVATE'}):
                invalid = {**extended, 'windows': [{**item, **change} for item in extended['windows']]}
                path.write_text(json.dumps({**value, 'activationWindows': invalid}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(directory, 'zed-desktop')
            incomplete = {**windows, 'windows': [{**item, 'inputDispatchReturns': 2} for item in windows['windows']]}
            path.write_text(json.dumps({**value, 'activationWindows': incomplete}))
            with self.assertRaises(ValueError):
                q.semantic_observations(directory, 'zed-desktop')
            for changes in ({'started': True}, {'ended': 2}, {'windows': []}, {'pid': 5}):
                path.write_text(json.dumps({**value, 'activationWindows': {**windows, **changes}}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(directory, 'zed-desktop')
            for change in ({'retryEntries': True}, {'nativeRetryEntries': -1}, {'pid': 123},
                           {'inputDispatchEntries': True}, {'inputDispatchEntries': -1},
                           {'inputDispatchEntries': 65537}, {'inputDispatchEntries': None},
                           {'status': 'unavailable'}, {'cleanup': 'failed'}, {'diagnosticsOnly': False}):
                path.write_text(json.dumps({**value, **change}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(directory, 'zed-desktop')
            value.update(status='unavailable', stage='attach', retryEntries=None, nativeRetryEntries=None,
                         inputDispatchEntries=None)
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(directory, 'zed-desktop'), [value])
            with self.assertRaises(ValueError):
                q.semantic_observations(directory, 'claude-desktop')

    def test_linux_claude_qualifies_only_complete_native_recovery(self):
        result = self.trial(app='claude-desktop', policy_count=0)
        self.assertEqual(result['backend'], 'native-assistant-clipboard')
        self.assertEqual(result['qualification'], 'deterministic-full')
        for change in (
            lambda r: r['results'][0]['deterministic'][0]['steps'].remove('error-recovered'),
            lambda r: r['results'][0]['deterministic'][0]['steps'].remove('tool-verified'),
            lambda r: r['results'][0]['deterministic'][0].update(responseVerification='local-ocr'),
            lambda r: r['results'][0].update(cleanup='failed'),
            lambda r: r.update(cleanup='failed'),
        ):
            self.assertEqual(self.trial(change, app='claude-desktop', policy_count=0)['qualification'], 'unqualified')

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

    def test_claude_native_campaign_budget_keeps_validation_and_hard_limit(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            value = dict(schemaVersion=1, mechanism='semantic-inventory', requestCount=1,
                         toolCount=1, knownReadToolCount=1, readToolSelected=True)
            for index in range(70):
                (root / f'fact-{index}.json').write_text(json.dumps(value))
            self.assertEqual(len(q.semantic_observations(root, 'claude-desktop')), 70)
            # The higher file budget must not turn malformed evidence into a
            # partial success by dropping records after the former cutoff.
            (root / 'fact-69.json').write_text(json.dumps({**value, 'toolCount': 'PRIVATE'}))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'claude-desktop')
            (root / 'fact-69.json').write_text(json.dumps(value))
            for index in range(70, 96):
                (root / f'fact-{index}.json').write_text(json.dumps(value))
            self.assertEqual(len(q.semantic_observations(root, 'claude-desktop')), 96)
            (root / 'fact-96.json').write_text(json.dumps(value))
            with self.assertRaisesRegex(ValueError, 'too many semantic observations'):
                q.semantic_observations(root, 'claude-desktop')

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
            controls = dict(coverCount=1, choiceCount=0, coverVisible=True,
                            choiceVisible=None, choiceEnabled=None)
            path.write_text(json.dumps({**item, 'onboardingObservation': controls}))
            self.assertEqual(q.semantic_observations(root, 'hermes-desktop')[0]['onboardingObservation'], controls)
            for change in ({'choiceCount': True}, {'coverCount': 65}, {'choiceVisible': 'PRIVATE'}, {'text': 'PRIVATE'}):
                path.write_text(json.dumps({**item, 'onboardingObservation': {**controls, **change}}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'hermes-desktop')
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

    def test_retained_zed_post_target_state_is_advisory_and_closed(self):
        facts = dict(schemaVersion=1, mechanism='zed-atspi-retry', diagnosticsOnly=True,
                     method='atspi-click', stage='postflight', actionAttempted=True, forwarded=True)
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'facts.json'
            for state in ('unchanged', 'defunct', 'changed', 'unavailable'):
                value = {**facts, 'postTargetState': state}
                path.write_text(json.dumps(value))
                self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [value])
            path.write_text(json.dumps(facts))
            self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [facts])
            for changed in ({'postTargetState': None}, {'postTargetState': 'PRIVATE'},
                            {'postTargetState': 'defunct', 'stage': 'action'},
                            {'postTargetState': 'unavailable', 'forwarded': False},
                            {'postTargetState': 'changed', 'detail': 'PRIVATE'}):
                path.write_text(json.dumps({**facts, **changed}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')

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

    def test_transient_dialog_counts_are_advisory_closed_and_partial_unknown(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / 'dialogs.json'
            base = dict(schemaVersion=1, mechanism='zed-pointer-observation', diagnosticsOnly=True,
                pointerTarget='unavailable', pointerChild='unavailable',
                **dict.fromkeys('maximizedHorizontal maximizedVertical enabled sensitive showing visible defunct retryContains'.split(), None))
            for field in ('transientDialogs', 'transientDialogsBeforeHover', 'transientDialogsBeforeDispatch'):
                for state in ('complete','unavailable','query-failed','identity-rejected','deadline','limit'):
                    dialogs = dict(state=state, ownedTransientDialogs=2 if state=='complete' else None,
                                   mappedOwnedTransientDialogs=1 if state=='complete' else None)
                    value = {**base,field:dialogs}
                    path.write_text(json.dumps(value))
                    self.assertEqual(q.semantic_observations(root,'zed-desktop'),[value])
                    with self.assertRaises(ValueError):
                        q.semantic_observations(root,'claude-desktop')
                good = dict(state='complete',ownedTransientDialogs=0,mappedOwnedTransientDialogs=0)
                for invalid in ({**good,'state':'PRIVATE'}, {**good,'text':'PRIVATE'},
                                {**good,'ownedTransientDialogs':True}, {**good,'ownedTransientDialogs':33},
                                {**good,'mappedOwnedTransientDialogs':1}, {**good,'state':'deadline'},
                                {**good,'ownedTransientDialogs':None}, {}, None):
                    path.write_text(json.dumps({**base,field:invalid}))
                    with self.assertRaises(ValueError):
                        q.semantic_observations(root,'zed-desktop')

    def test_xi2_payload_receipt_is_advisory_bounded_and_closed(self):
        value=dict(state='complete',targetPointCount=9,ownedMotionCount=12,motionWithXYCount=10,
            retainedPointMatchedCount=9,noPressedButtons=True,eventRootTranslationMatched=True,
            observerOnly=True,inputAuthorized=False)
        self.assertEqual(q.zed_xi2_motion(value),value)
        for state in ['complete','unavailable','query-failed','identity-rejected','deadline','limit']:
            empty={**value,'state':state,'ownedMotionCount':0,'motionWithXYCount':0,
                'retainedPointMatchedCount':0,'noPressedButtons':None,'eventRootTranslationMatched':None}
            self.assertEqual(q.zed_xi2_motion(empty),empty)
        for change in [{'targetPointCount':10},{'ownedMotionCount':129},{'motionWithXYCount':13},
                       {'retainedPointMatchedCount':10},{'noPressedButtons':None},{'observerOnly':False},
                       {'inputAuthorized':True},{'state':'PRIVATE'},{'rootCoordinates':[1,2]},
                       {'deviceId':42},{'flags':1},{'ownedMotionCount':True}]:
            with self.assertRaises(ValueError):q.zed_xi2_motion({**value,**change})

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

    def test_claude_prelaunch_is_fixed_file_closed_and_diagnostic_only(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'claude-cli-prelaunch.json'
            value = dict(schemaVersion=1, mechanism='claude-cli-prelaunch', diagnosticsOnly=True,
                         phase='prelaunch', stage='configuration', status='failed')
            path.write_text(json.dumps(value))
            records = q.semantic_observations(directory, 'claude-desktop')
            self.assertEqual(records[0]['stage'], 'configuration')
            for patch in ({'stage':'PRIVATE'}, {'stage':True}, {'stage':[]}, {'path':'PRIVATE'},
                          {'status':'success'}, {'diagnosticsOnly':False}):
                path.write_text(json.dumps({**value, **patch}))
                with self.assertRaises(ValueError): q.semantic_observations(directory, 'claude-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError): q.semantic_observations(directory, 'zed-desktop')
            path.rename(Path(directory) / 'wrong.json')
            with self.assertRaises(ValueError): q.semantic_observations(directory, 'claude-desktop')

    def test_claude_configuration_boundary_preserves_legacy_and_rejects_private_payload(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'claude-cli-prelaunch.json'
            base = dict(schemaVersion=1, mechanism='claude-cli-prelaunch', diagnosticsOnly=True,
                        phase='prelaunch', stage='configuration', status='failed')
            for supplement in ({}, {'configurationSubstage': 'windows-policy'},
                               {'configurationSubstage': 'document-read', 'configurationDocument': 'normal-config'},
                               {'configurationSubstage': 'persist', 'configurationDocument': 'profile'},
                               {'configurationSubstage': 'managed-mcp', 'configurationDocument': 'profile'}):
                path.write_text(json.dumps({**base, **supplement}))
                self.assertEqual(q.semantic_observations(directory, 'claude-desktop')[0], {**base, **supplement})
            for supplement in ({'configurationSubstage': 'PRIVATE_SENTINEL'},
                               {'configurationSubstage': 'persist', 'configurationDocument': 'PRIVATE_SENTINEL'},
                               {'configurationSubstage': 'persist', 'configurationDocument': []},
                               {'configurationSubstage': 'persist', 'configurationDocument': {}},
                               {'configurationSubstage': 'persist'}, {'configurationDocument': 'profile'},
                               {'configurationSubstage': 'windows-policy', 'configurationDocument': 'profile'},
                               {'configurationSubstage': 'managed-mcp', 'configurationDocument': 'metadata'},
                               {'configurationSubstage': 'persist', 'configurationDocument': 'profile', 'error': 'PRIVATE_SENTINEL'},
                               {'stage': 'snapshot', 'configurationSubstage': 'windows-policy'}):
                path.write_text(json.dumps({**base, **supplement}))
                with self.assertRaises(ValueError): q.semantic_observations(directory, 'claude-desktop')

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
            for stage in ('pre-resize-identity', 'resize-acknowledgement', 'pre-position-identity'):
                measured = {**value, 'stage': stage}
                path.write_text(json.dumps(measured))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [measured])
            for error in ('cannot-complete','attribute-unsupported','illegal-argument',
                          'invalid-element','api-disabled','failure','other'):
                measured = {**value, 'stage':'position', 'positionError':error}
                path.write_text(json.dumps(measured))
                self.assertEqual(q.semantic_observations(root,'claude-desktop'),[measured])
            for measured in ({**value,'positionError':'failure'},
                             {**value,'stage':'completed','positionError':'failure'},
                             {**value,'stage':'position','positionError':'PRIVATE'},
                             {**value,'stage':'position','positionError':True}):
                path.write_text(json.dumps(measured))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')
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
            partial = dict(ancestorBoundsStatus='unavailable', checkedAncestorCount=2,
                           centerWithinPublishedAncestors=None, ancestorQueryStage='ancestor-bounds')
            for failure in ('component-unavailable', 'query-failed', 'invalid-geometry', 'unmeasured'):
                value = {**base, **partial, 'ancestorBoundsFailure': failure}
                path.write_text(json.dumps(value))
                self.assertEqual(q.semantic_observations(root, 'zed-desktop'), [value])
            for extension in ({**partial, 'ancestorBoundsFailure': 'PRIVATE'},
                              {**partial, 'ancestorBoundsFailure': None},
                              {**partial, 'ancestorBoundsFailure': 'query-failed', 'ancestorQueryStage': 'parent'},
                              {**valid[0], 'ancestorBoundsFailure': 'query-failed'},
                              {'ancestorBoundsFailure': 'component-unavailable'}):
                path.write_text(json.dumps({**base, **extension}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')
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

    def test_entry_crossing_diagnostic_is_optional_closed_and_never_input_authority(self):
        value=dict(schemaVersion=1,mechanism='zed-pointer-observation',diagnosticsOnly=True,
            maximizedHorizontal=None,maximizedVertical=None,enabled=None,sensitive=None,
            showing=None,visible=None,defunct=None,retryContains=None,
            pointerTarget='unavailable',pointerChild='unavailable')
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);path=root/'entry.json'
            entry=dict(stage='candidate',failureReason='decoration-unavailable')
            path.write_text(json.dumps({**value,'entryCrossing':entry}))
            self.assertEqual(q.semantic_observations(root,'zed-desktop')[0]['entryCrossing'],entry)
            for reason in ['top-frame-hit','decoration-child-hit','client-child-hit','pointer-position','pointer-child-current']:
                path.write_text(json.dumps({**value,'entryCrossing':dict(stage='decoration-before',failureReason=reason)}))
                self.assertEqual(q.semantic_observations(root,'zed-desktop')[0]['entryCrossing']['failureReason'],reason)
            path.write_text(json.dumps(value))
            self.assertNotIn('entryCrossing',q.semantic_observations(root,'zed-desktop')[0])
            for change in ({'stage':'PRIVATE'},{'failureReason':[]},{'failureReason':True},
                           {'rawGeometry':'PRIVATE'},{'stage':'client-complete'},
                           {'failureReason':'PRIVATE'}):
                path.write_text(json.dumps({**value,'entryCrossing':{**entry,**change}}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'zed-desktop')
            path.write_text(json.dumps({**value,'entryCrossing':dict(stage='client-complete',failureReason=None)}))
            self.assertIsNone(q.semantic_observations(root,'zed-desktop')[0]['entryCrossing']['failureReason'])

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
            headers=dict(status='observed',ownedNormalEnterCount=0,ownedNonNormalEnterCount=0,
                         ownedNormalLeaveCount=0,ownedMotionCount=9)
            valid.append({**valid[1],'crossingHeaders':headers})
            valid.append({**valid[1],'crossingHeaders':{**headers,'eventOrder':['leave','enter','press','release']}})
            for change in ({'ownedMotionCount':65},{'ownedNormalEnterCount':True},
                           {'rawEvent':'PRIVATE'},{'status':[]},{'ownedNormalLeaveCount':None},
                           {'eventOrder':['PRIVATE']},{'eventOrder':['enter']*129},{'eventOrder':[True]}):
                bad={**valid[1],'crossingHeaders':{**headers,**change}}
                path.write_text(json.dumps({**value,'inputDelivery':bad}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'zed-desktop')
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

    def test_windows_tree_limits_remain_nonaccepting_and_closed(self):
        value = dict(schemaVersion=1, mechanism='claude-windows-native-chat', diagnosticsOnly=True,
                     stage='tree-depth', submittedTurns=1, inputVerifiedTurns=1, copiedResponses=0,
                     retryAttempted=False, clipboardCleared=True)
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'tree.json'
            for stage in 'tree-depth tree-nodes tree-name-limit tree-text-limit tree-window-limit tree-process-limit'.split():
                path.write_text(json.dumps({**value, 'stage':stage}))
                receipt = q.semantic_observations(root, 'claude-desktop')[0]
                self.assertEqual(receipt['stage'], stage)
                self.assertEqual(receipt['copiedResponses'], 0)
            path.write_text(json.dumps({**value, 'rawName':'PRIVATE'}))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'claude-desktop')

    def test_windows_cleanup_preflight_failure_is_closed_and_not_acceptance(self):
        value = dict(schemaVersion=1, mechanism='windows-owned-cleanup-preflight',
                     diagnosticsOnly=True, stage='deadline')
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'failure.json'
            for stage in 'request path file-open file-hash file-identity process-open snapshot inspector-parent ancestry target-open target-identity target-creation target-state target-image owner-recheck deadline transport target-image-query target-image-sharing target-image-access target-image-open target-image-canonical target-image-metadata target-image-path target-image-volume target-image-file-id target-image-file-id-query target-image-size target-image-write-time'.split():
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

    def test_claude_windows_fit_receipt_has_no_identity_or_geometry(self):
        value = dict(schemaVersion=1, mechanism='claude-windows-fit', diagnosticsOnly=True,
                     phase='final-ready', fitAttempted=True, helperSucceeded=True)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'fit.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(tmp, 'claude-desktop'), [value])
            for changed in ({**value, 'phase': 'initial'}, {**value, 'fitAttempted': False},
                            {**value, 'helperSucceeded': 1}, {**value, 'bounds': 'PRIVATE'}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):
                    q.semantic_observations(tmp, 'claude-desktop')

    def test_claude_windows_fit_rejection_is_closed_and_never_readiness(self):
        value = dict(schemaVersion=1, mechanism='claude-windows-fit-rejection', diagnosticsOnly=True,
                     phase='pending-attachment', policyEnabled=True, fitAttempted=False,
                     sourceComposerReady=None, candidateReason='overlap-ahead', guardFailure='off-display',
                     eligibleCount=1, sameProcessAheadCount=0, overlapAheadCount=1)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'fit.json'
            for item in (value, {**value, 'phase': 'final-ready', 'sourceComposerReady': True}):
                path.write_text(json.dumps(item))
                self.assertEqual(q.semantic_observations(tmp, 'claude-desktop'), [item])
            for change in ({'phase': 'PRIVATE'}, {'sourceComposerReady': True}, {'candidateReason': 'PRIVATE'},
                           {'guardFailure': 'PRIVATE'}, {'eligibleCount': True}, {'overlapAheadCount': 65},
                           {'policyEnabled': 1}, {'bounds': [1, 2, 3, 4]}):
                path.write_text(json.dumps({**value, **change}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(tmp, 'claude-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(tmp, 'chatgpt-desktop')

    def test_native_input_boundaries_preserve_terminal_failure_and_privacy(self):
        facts = dict(schemaVersion=1, mechanism='claude-native-chat', diagnosticsOnly=True,
                     submittedTurns=1, inputVerifiedTurns=1, copiedResponses=1,
                     retryAttempted=False, clipboardCleared=True,
                     actionPhase='completed', transportFailure=None)
        stages = ['input-focus-guard', 'input-focus-setting', 'input-focused-identity', 'input-replace-select-key', 'input-prompt-before-guard', 'input-prompt-clipboard', 'input-prompt-after-guard', 'input-paste-key', 'input-readback-before-guard', 'input-sentinel-clipboard', 'input-sentinel-after-guard', 'input-readback-select-key', 'input-readback-select-guard', 'input-readback-copy-key', 'input-collapse-guard', 'input-collapse-key']
        stages += 'clipboard-owner clipboard-allocation clipboard-lock clipboard-empty clipboard-set clipboard-close clipboard-guard-before clipboard-guard-after clipboard-deadline-before clipboard-deadline-after clipboard-open-deadline'.split()
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'facts.json'
            for stage in stages + ['focus', 'failure-details-ready', 'failure-details-opened']:
                path.write_text(json.dumps({**facts, 'stage': stage}))
                value = q.semantic_observations(root, 'claude-desktop')[0]
                self.assertEqual(value['stage'], stage)
                self.assertEqual(value['submittedTurns'], 1)
                self.assertFalse(value['retryAttempted'])
            for stage in ('input-private-path', None, 'input-focus-setting PRIVATE'):
                path.write_text(json.dumps({**facts, 'stage': stage}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps({**facts, 'stage': stages[0], 'detail': 'PRIVATE'}))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps({**facts, 'stage': stages[0]}))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'zed-desktop')

    def test_windows_native_chat_receipt_is_advisory_and_payload_closed(self):
        value = dict(schemaVersion=1, mechanism='claude-windows-native-chat', diagnosticsOnly=True,
                     stage='completed', submittedTurns=3, inputVerifiedTurns=3, copiedResponses=3,
                     retryAttempted=True, clipboardCleared=True)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'chat.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(tmp, 'claude-desktop'), [value])
            self.assertEqual(q.envelope('claude-desktop', 'windows', 'x86_64', 'a' * 40)['qualification'], 'unqualified')
            for change in ({'stage': 'PRIVATE'}, {'prompt': 'PRIVATE'}, {'submittedTurns': 4},
                           {'inputVerifiedTurns': 2}, {'retryAttempted': 1}, {'clipboardCleared': False, 'detail': 'PRIVATE'}):
                path.write_text(json.dumps({**value, **change}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(tmp, 'claude-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(tmp, 'zed-desktop')

    def test_windows_operation_timing_is_closed_and_bounded(self):
        timing = dict(budgetMs=15000, elapsedMs=15001, transportRemainingMs=14000,
                      postGuardRemainingMs=2)
        value = dict(schemaVersion=1, mechanism='claude-windows-native-chat', diagnosticsOnly=True,
                     stage='action-uncertain', submittedTurns=3, inputVerifiedTurns=3, copiedResponses=2,
                     retryAttempted=False, clipboardCleared=True, operationTiming=timing)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'chat.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(tmp, 'claude-desktop'), [value])
            for change in ({'text':'PRIVATE'}, {'budgetMs':True}, {'elapsedMs':600001},
                           {'postGuardRemainingMs':15001}, {'transportRemainingMs':-1}):
                path.write_text(json.dumps({**value, 'operationTiming':{**timing, **change}}))
                with self.assertRaises(ValueError):q.semantic_observations(tmp, 'claude-desktop')

    def test_zed_pointer_target_readback_is_closed_and_passive(self):
        value = dict(schemaVersion=1, mechanism='zed-pointer-observation', diagnosticsOnly=True,
                     maximizedHorizontal=True, maximizedVertical=True, enabled=True, sensitive=True,
                     showing=None, visible=None, defunct=False, retryContains=True,
                     pointerTarget='client', pointerChild='client')
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp)/'pointer.json'
            for state in ('unavailable','defunct','same-source','changed-source'):
                path.write_text(json.dumps({**value,'targetAfterClick':state}))
                self.assertEqual(q.semantic_observations(tmp,'zed-desktop')[0]['targetAfterClick'],state)
            for state in (True,None,'PRIVATE',{'name':'PRIVATE'}):
                path.write_text(json.dumps({**value,'targetAfterClick':state}))
                with self.assertRaises(ValueError):q.semantic_observations(tmp,'zed-desktop')

    def test_windows_failure_scope_counts_are_closed_without_qualifying_retry(self):
        counts = dict(serverErrorCount=1, failedUserHeadingCount=1, failedPromptTextCount=1,
                      retryButtonCount=1, detailsButtonCount=0, exactPromptGroupCount=1,
                      groupRetryButtonCount=1, groupDetailsButtonCount=0)
        value = dict(schemaVersion=1, mechanism='claude-windows-native-chat', diagnosticsOnly=True,
                     stage='scope-control-absent', submittedTurns=3, inputVerifiedTurns=3, copiedResponses=2,
                     retryAttempted=False, clipboardCleared=True, failureScopeCounts=counts)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp)/'chat.json'
            path.write_text(json.dumps(value))
            public = q.semantic_observations(tmp, 'claude-desktop')[0]
            self.assertEqual(public['failureScopeCounts'], counts)
            self.assertFalse(public['retryAttempted'])
            labels = {**counts, 'retryLabelCount':2, 'detailsLabelCount':1}
            path.write_text(json.dumps({**value, 'failureScopeCounts':labels}))
            self.assertEqual(q.semantic_observations(tmp, 'claude-desktop')[0]['failureScopeCounts'], labels)
            unfiltered = {**labels, 'unfilteredRetryLabelCount': 3, 'unfilteredDetailsLabelCount': None}
            path.write_text(json.dumps({**value, 'failureScopeCounts': unfiltered}))
            observed = q.semantic_observations(tmp, 'claude-desktop')[0]
            self.assertEqual(observed['failureScopeCounts'], unfiltered)
            self.assertFalse(observed['retryAttempted'])
            shaped = {**unfiltered, 'buttonShape': [3,1,2,1,2,1]}
            path.write_text(json.dumps({**value, 'failureScopeCounts': shaped}))
            observed = q.semantic_observations(tmp, 'claude-desktop')[0]
            self.assertEqual(observed['failureScopeCounts'], shaped)
            self.assertFalse(observed['retryAttempted'])
            for bad in (None, 'PRIVATE', [3,1], [True,0,0,0,0,0], [1025,0,0,0,0,0],
                        [0,0,0,0,0,0], [3,4,2,1,2,1], [3,1,4,1,2,1], [3,1,2,1,3,1], [3,1,2,1,2,2]):
                path.write_text(json.dumps({**value, 'failureScopeCounts': {**shaped, 'buttonShape':bad}}))
                with self.assertRaises(ValueError):q.semantic_observations(tmp, 'claude-desktop')
            for bad in (True, -1, 1025, 'PRIVATE'):
                path.write_text(json.dumps({**value, 'failureScopeCounts':
                    {**unfiltered, 'unfilteredRetryLabelCount': bad}}))
                with self.assertRaises(ValueError):q.semantic_observations(tmp, 'claude-desktop')
            for change in ({'retryLabelCount':0}, {'detailsLabelCount':True}, {'text':'PRIVATE'}):
                path.write_text(json.dumps({**value, 'failureScopeCounts':{**labels, **change}}))
                with self.assertRaises(ValueError):q.semantic_observations(tmp,'claude-desktop')
            for change in ({'raw':'PRIVATE'}, {'retryButtonCount':True},
                           {'detailsButtonCount':1025}, {'groupDetailsButtonCount':1}):
                path.write_text(json.dumps({**value, 'failureScopeCounts':{**counts,**change}}))
                with self.assertRaises(ValueError):q.semantic_observations(tmp,'claude-desktop')

    def test_claude_native_chat_guard_rejection_is_separate_and_closed(self):
        value = dict(schemaVersion=1, mechanism='claude-native-chat', diagnosticsOnly=True,
                     stage='action-uncertain', submittedTurns=1, inputVerifiedTurns=1,
                     copiedResponses=0, retryAttempted=False, clipboardCleared=True,
                     actionPhase='post-guard', transportFailure=None)
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'chat.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [value])
            for phase in ('before-guard', 'post-guard'):
                for rejection in ('identity-missing', 'bounds-changed', 'foreground-changed',
                                  'same-process-window', 'off-display', 'occluded'):
                    item = {**value, 'actionPhase': phase, 'guardRejection': rejection}
                    path.write_text(json.dumps(item))
                    self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [item])
            for change in ({'guardRejection': 'PRIVATE'}, {'guardRejection': None},
                           {'actionPhase': 'transport'}, {'transportFailure': 'timeout'},
                           {'stage': 'completed'}, {'rawBounds': [1, 2, 3, 4]}):
                item = {**value, 'guardRejection': 'bounds-changed', **change}
                path.write_text(json.dumps(item))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps({**value, 'guardRejection': 'occluded'}))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'zed-desktop')

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
            for deadline in ('deadline-window', 'deadline-tree', 'deadline-focus', 'deadline-input', 'deadline-input-paste', 'deadline-input-readback',
                             'deadline-press', 'deadline-copy', 'deadline-retry-ready', 'deadline-retry'):
                item={**value, 'stage':deadline, 'submittedTurns':0, 'inputVerifiedTurns':0,
                      'copiedResponses':0, 'retryAttempted':False}
                path.write_text(json.dumps(item))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop'),[item])
                path.write_text(json.dumps({**item,'stage':deadline+' PRIVATE'}))
                with self.assertRaises(ValueError): q.semantic_observations(root,'claude-desktop')
            for phase in (None, 'before-guard', 'after-guard', 'transport', 'post-guard', 'completed'):
                item = {**value, 'actionPhase': phase, 'transportFailure': None}
                path.write_text(json.dumps(item))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [item])
            for phase in ('before-guard', 'transport', 'post-guard'):
                item = {**value, 'stage': 'deadline', 'actionPhase': phase, 'transportFailure': 'timeout'}
                path.write_text(json.dumps(item))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [item])
            for observation in (None, dict(generationObserved=True, fixtureResponseVerified=True, failureObserved=False),
                                dict(generationObserved=False, fixtureResponseVerified=False, failureObserved=False)):
                item = {**value, 'providerObservation': observation}
                path.write_text(json.dumps(item))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop'), [item])
            for observation in ('PRIVATE', {'generationObserved': True},
                                dict(generationObserved=1, fixtureResponseVerified=False, failureObserved=False),
                                dict(generationObserved=True, fixtureResponseVerified=False, failureObserved=False, prompt='PRIVATE')):
                path.write_text(json.dumps({**value, 'providerObservation': observation}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            for changes in ({'actionPhase': 'PRIVATE', 'transportFailure': None},
                            {'actionPhase': 'transport', 'transportFailure': 'PRIVATE'},
                            {'actionPhase': None, 'transportFailure': 'output'},
                            {'actionPhase': 'completed', 'transportFailure': 'timeout'},
                            {'actionPhase': 'transport'}, {'transportFailure': None}):
                path.write_text(json.dumps({**value, **changes}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'claude-desktop')
            for stage in ('tree-query', 'tree-duplicate', 'tree-type', 'tree-limit', 'tree-pid',
                          'tree-focus', 'tree-window', 'scope-anchor-absent', 'scope-heading-absent', 'scope-assistant-heading-absent',
                          'scope-marker-heading-absent', 'scope-anchor-ambiguous',
                          'scope-control-absent', 'scope-control-ambiguous', 'scope-heading-ambiguous', 'scope-prompt-mismatch',
                          'input-initial-unavailable', 'input-initial-nonempty',
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

    def test_codex_public_dialog_is_passive_closed_and_consistent(self):
        value = dict(schemaVersion=1, mechanism='renderer-inventory', diagnosticsOnly=True,
                     app='chatgpt-desktop', endpointOwned=True, launcherOwned=True, attached=True,
                     pageCount=1, textareaCount=0, editableCount=0, sendCount=0,
                     retryCount=0, newThreadCount=0, loginCount=0, dialogCount=1, errorCategory=None)
        observed = dict(status='workspace-discovery-failed', counts=dict(dialogs=1, workspaceFailureTitle=1, retryButton=1))
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'inventory.json'
            path.write_text(json.dumps({**value, 'sourceDialog': observed}))
            self.assertEqual(q.semantic_observations(tmp, 'chatgpt-desktop')[0]['sourceDialog'], observed)
            for changed in ({**observed, 'status': 'PRIVATE'}, {**observed, 'status': 'unknown'},
                            {**observed, 'title': 'PRIVATE'}, {**observed, 'counts': {'dialogs': True}},
                            {**observed, 'counts': {**observed['counts'], 'retryButton': 33}}):
                path.write_text(json.dumps({**value, 'sourceDialog': changed}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(tmp, 'chatgpt-desktop')
            path.write_text(json.dumps({**value, 'app': 'claude-desktop', 'sourceDialog': observed}))
            with self.assertRaises(ValueError):
                q.semantic_observations(tmp, 'claude-desktop')

    def test_codex_managed_sign_in_observation_cannot_authorize_input(self):
        value = dict(schemaVersion=1, mechanism='renderer-inventory', diagnosticsOnly=True,
                     app='chatgpt-desktop', endpointOwned=True, launcherOwned=True, attached=True,
                     pageCount=1, textareaCount=0, editableCount=0, sendCount=0,
                     retryCount=0, newThreadCount=0, loginCount=0, dialogCount=0, errorCategory=None)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'inventory.json'
            for reason in ('ancestor-unowned', 'ancestor-query', 'listener-unavailable', 'listener-shape',
                           'listener-unowned', 'listener-query', 'unmeasured'):
                item = {**value, 'nativeOwnershipFailure': reason}
                path.write_text(json.dumps(item))
                self.assertEqual(q.semantic_observations(tmp, 'chatgpt-desktop')[0]['nativeOwnershipFailure'], reason)
            for reason in ('PRIVATE', None, True, {'pid': 1}):
                path.write_text(json.dumps({**value, 'nativeOwnershipFailure': reason}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(tmp, 'chatgpt-desktop')
            shape = dict(reason='multiple-listeners', listenerCount=2, uniquePidCount=1)
            item = {**value, 'nativeOwnershipFailure': 'listener-shape', 'nativeListenerShape': shape}
            path.write_text(json.dumps(item))
            self.assertEqual(q.semantic_observations(tmp, 'chatgpt-desktop'), [item])
            for change in ({'reason': 'PRIVATE'}, {'listenerCount': 4097}, {'listenerCount': True},
                           {'uniquePidCount': 3}, {'endpoint': 'PRIVATE'}):
                path.write_text(json.dumps({**item, 'nativeListenerShape': {**shape, **change}}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(tmp, 'chatgpt-desktop')
            path.write_text(json.dumps({**item, 'nativeOwnershipFailure': 'ancestor-query'}))
            with self.assertRaises(ValueError):
                q.semantic_observations(tmp, 'chatgpt-desktop')
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

    def test_codex_pending_native_stack_is_closed_advisory_only(self):
        value=dict(schemaVersion=1,mechanism='renderer-inventory',diagnosticsOnly=True,
                   app='chatgpt-desktop',endpointOwned=True,launcherOwned=True,attached=True,
                   pageCount=1,textareaCount=0,editableCount=0,sendCount=0,retryCount=0,
                   newThreadCount=0,loginCount=0,dialogCount=0,errorCategory=None)
        stack=dict(sample='after',normalOverlapCount=0,elevatedOverlapCount=1,
                   lowerOverlapCount=0,displayContained=True)
        activation=dict(phase='polling',status='deadline',activationAttempted=True,
                        guardFailure=None,nativePendingStack=stack)
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);path=root/'pending.json'
            path.write_text(json.dumps({**value,'initialMainActivation':activation}))
            self.assertEqual(q.semantic_observations(root,'chatgpt-desktop')[0]['initialMainActivation'],activation)
            for area in (dict(measured=True,windowContained=False,overlapIntersectionCount=0),
                         dict(measured=True,windowContained=True,overlapIntersectionCount=1),
                         dict(measured=False,windowContained=None,overlapIntersectionCount=None)):
                measured={**activation,'nativePendingStack':{**stack,'workArea':area}}
                path.write_text(json.dumps({**value,'initialMainActivation':measured}))
                self.assertEqual(q.semantic_observations(root,'chatgpt-desktop')[0]['initialMainActivation'],measured)
            for area in (dict(measured=True,windowContained=True,overlapIntersectionCount=0),
                         dict(measured=False,windowContained=False,overlapIntersectionCount=0),
                         dict(measured=True,windowContained=True,overlapIntersectionCount=2),
                         dict(measured=True,windowContained=True,overlapIntersectionCount=True),
                         dict(measured=True,windowContained=True,overlapIntersectionCount=1,rawBounds='PRIVATE')):
                bad={**activation,'nativePendingStack':{**stack,'workArea':area}}
                path.write_text(json.dumps({**value,'initialMainActivation':bad}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'chatgpt-desktop')
            kinds=dict.fromkeys(('controlCenter','notificationCenter','systemUIServer','dock',
                'windowServer','launcherOwned','checkerOwned','other','unobserved'),0)
            kinds['unobserved']=1
            measured={**activation,'nativePendingStack':{**stack,'occluderKinds':kinds}}
            path.write_text(json.dumps({**value,'initialMainActivation':measured}))
            self.assertEqual(q.semantic_observations(root,'chatgpt-desktop')[0]['initialMainActivation'],measured)
            public_other=dict(coreServicesUIAgent=1,textInputMenuAgent=0,securityAgent=0)
            other={**kinds,'unobserved':0,'other':1}
            classified={**activation,'nativePendingStack':{**stack,'occluderKinds':other,
                'otherPublicExecutables':public_other}}
            path.write_text(json.dumps({**value,'initialMainActivation':classified}))
            self.assertEqual(q.semantic_observations(root,'chatgpt-desktop')[0]['initialMainActivation'],classified)
            for change in ({'securityAgent':1},{'coreServicesUIAgent':True},
                           {'textInputMenuAgent':1025},{'path':'PRIVATE'},{'pid':1}):
                bad={**classified,'nativePendingStack':{**classified['nativePendingStack'],
                    'otherPublicExecutables':{**public_other,**change}}}
                path.write_text(json.dumps({**value,'initialMainActivation':bad}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'chatgpt-desktop')
            unbound={**activation,'nativePendingStack':{**stack,'otherPublicExecutables':public_other}}
            path.write_text(json.dumps({**value,'initialMainActivation':unbound}))
            with self.assertRaises(ValueError):q.semantic_observations(root,'chatgpt-desktop')
            for change in ({'controlCenter':1},{'unobserved':True},{'other':1025},
                           {'unobserved':None},{'pid':1},{'path':'PRIVATE'}):
                bad={**activation,'nativePendingStack':{**stack,'occluderKinds':{**kinds,**change}}}
                path.write_text(json.dumps({**value,'initialMainActivation':bad}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'chatgpt-desktop')
            levels=dict(menuLevelCount=1,statusLevelCount=0,dockLevelCount=0,otherLevelCount=0)
            measured={**activation,'nativePendingStack':{**stack,'elevatedLevels':levels}}
            path.write_text(json.dumps({**value,'initialMainActivation':measured}))
            self.assertEqual(q.semantic_observations(root,'chatgpt-desktop')[0]['initialMainActivation'],measured)
            for change in ({'menuLevelCount':True},{'statusLevelCount':1},{'otherLevelCount':1025},
                           {'rawWindowLevel':'PRIVATE'}, {'dockLevelCount':None}):
                bad={**measured,'nativePendingStack':{**stack,'elevatedLevels':{**levels,**change}}}
                path.write_text(json.dumps({**value,'initialMainActivation':bad}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'chatgpt-desktop')
            for changed in ({**stack,'normalOverlapCount':True},{**stack,'elevatedOverlapCount':0},
                            {**stack,'normalOverlapCount':1024},{**stack,'displayContained':False},
                            {**stack,'sample':'PRIVATE'},{**stack,'title':'PRIVATE'},None):
                path.write_text(json.dumps({**value,'initialMainActivation':{**activation,'nativePendingStack':changed}}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'chatgpt-desktop')
            path.write_text(json.dumps({**value,'initialMainActivation':{**activation,'activationAttempted':False}}))
            with self.assertRaises(ValueError):q.semantic_observations(root,'chatgpt-desktop')

    def test_codex_activation_diagnostic_preserves_original_failure_and_privacy(self):
        value = dict(schemaVersion=1, mechanism='renderer-inventory', diagnosticsOnly=True,
                     app='chatgpt-desktop', endpointOwned=True, launcherOwned=True, attached=True,
                     pageCount=1, textareaCount=0, editableCount=0, sendCount=0,
                     retryCount=0, newThreadCount=0, loginCount=0, dialogCount=0, errorCategory=None)
        facts = dict(phase='polling', status='deadline', activationAttempted=True, guardFailure=None)
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'main.json'
            for boundary in ('request','cg-inventory-before','ax-main-before','cg-inventory-after',
                             'ax-main-after','identity','trust'):
                item = {**facts, 'phase':'pre-identity', 'status':'query-failed',
                        'activationAttempted':False, 'nativeBoundary':boundary}
                path.write_text(json.dumps({**value,'initialMainActivation':item}))
                self.assertEqual(q.semantic_observations(root,'chatgpt-desktop')[0]['initialMainActivation'],item)
                for invalid in ({**item,'nativeBoundary':'PRIVATE'}, {**item,'nativeBoundary':True},
                                {**item,'phase':'polling'}, {**item,'activationAttempted':True}):
                    path.write_text(json.dumps({**value,'initialMainActivation':invalid}))
                    with self.assertRaises(ValueError):q.semantic_observations(root,'chatgpt-desktop')
            inventory = dict(reason='candidates-missing',candidateCount=0,executableRejectedCount=2,ancestryRejectedCount=1)
            item = dict(phase='pre-identity',status='query-failed',activationAttempted=False,
                        guardFailure=None,nativeBoundary='cg-inventory-before',nativeInventoryFailure=inventory)
            path.write_text(json.dumps({**value,'initialMainActivation':item}))
            self.assertEqual(q.semantic_observations(root,'chatgpt-desktop')[0]['initialMainActivation'],item)
            for invalid in ({**inventory,'reason':'PRIVATE'}, {**inventory,'candidateCount':True},
                            {**inventory,'candidateCount':1025}, {**inventory,'path':'PRIVATE'}):
                path.write_text(json.dumps({**value,'initialMainActivation':{**item,'nativeInventoryFailure':invalid}}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'chatgpt-desktop')
            path.write_text(json.dumps({**value,'initialMainActivation':{**item,'nativeBoundary':'trust'}}))
            with self.assertRaises(ValueError):q.semantic_observations(root,'chatgpt-desktop')
            for phase, outer in [('activation','activation'),('verification','polling')]:
                failure = dict(phase=phase, boundary='app-unfocused')
                item = {**facts, 'phase':outer, 'status':'query-failed', 'nativeActivationFailure':failure}
                path.write_text(json.dumps({**value, 'initialMainActivation':item}))
                self.assertEqual(q.semantic_observations(root,'chatgpt-desktop')[0]['initialMainActivation'],item)
                for invalid in ({**failure,'boundary':'PRIVATE'}, {**failure,'phase':True},
                                {**failure,'path':'PRIVATE'}, {**failure,'inventory':inventory}):
                    path.write_text(json.dumps({**value,'initialMainActivation':{**item,'nativeActivationFailure':invalid}}))
                    with self.assertRaises(ValueError):q.semantic_observations(root,'chatgpt-desktop')
                for invalid in ({**item,'activationAttempted':False},{**item,'phase':'pre-proof'},
                                {**item,'status':'focused'}):
                    path.write_text(json.dumps({**value,'initialMainActivation':invalid}))
                    with self.assertRaises(ValueError):q.semantic_observations(root,'chatgpt-desktop')
                measured = {**item,'nativeActivationFailure':{**failure,'boundary':'cg-inventory-after','inventory':inventory}}
                path.write_text(json.dumps({**value,'initialMainActivation':measured}))
                self.assertEqual(q.semantic_observations(root,'chatgpt-desktop')[0]['initialMainActivation'],measured)
            for item in (facts, {**facts, 'phase': 'final-proof', 'status': 'focused'},
                         {**facts, 'phase': 'pre-proof', 'status': 'rejected',
                          'activationAttempted': False, 'guardFailure': 'native-ownership'}):
                path.write_text(json.dumps({**value, 'initialMainActivation': item}))
                self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['initialMainActivation'], item)
            for item in ({**facts, 'phase': 'PRIVATE'}, {**facts, 'guardFailure': 'PRIVATE'},
                         {**facts, 'status': 'PRIVATE'}, {**facts, 'url': 'PRIVATE'},
                         {**facts, 'activationAttempted': 1}, {**facts, 'phase': 'pre-proof'},
                         {**facts, 'status': 'focused'},
                         {**facts, 'status': 'focused', 'phase': 'final-proof', 'guardFailure': 'deadline'}):
                path.write_text(json.dumps({**value, 'initialMainActivation': item}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'chatgpt-desktop')
            path.write_text(json.dumps({**value, 'app': 'claude-desktop', 'initialMainActivation': facts}))
            with self.assertRaises(ValueError):
                q.semantic_observations(root, 'claude-desktop')

    def test_linux_chat_failure_boundary_is_closed_and_advisory(self):
        facts = dict(schemaVersion=1, mechanism='claude-linux-native-chat', diagnosticsOnly=True,
                     stage='blocked', submittedTurns=0, inputVerifiedTurns=0, copiedResponses=0,
                     retryAttempted=False, clipboardCleared=True)
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'boundary.json'
            for boundary in ('request','policy','native-window','source-owner','tree','tree-cycle','tree-depth','tree-limit','tree-identity','tree-children','response-heading','response-row','response-row-role-limit','response-row-copy-absent','response-row-copy-ambiguous','response-row-headings','response-row-attachment','state','frame','frame-active','frame-state','frame-identity','frame-bounds','frame-ancestry-cycle','frame-ancestry-depth','frame-nested-dialog','frame-nested-frame','frame-nested-window','frame-editor-outside','frame-count','frame-client','client',
                             'mode','focus','input','clipboard','action','response','transport'):
                path.write_text(json.dumps({**facts,'failureBoundary':boundary}))
                record = q.semantic_observations(root,'claude-desktop')[0]
                self.assertEqual(record['failureBoundary'],boundary)
                self.assertEqual(record['submittedTurns'],0)
            path.write_text(json.dumps(facts))
            self.assertNotIn('failureBoundary',q.semantic_observations(root,'claude-desktop')[0])
            for changed in ({**facts,'failureBoundary':'PRIVATE'}, {**facts,'failureBoundary':None},
                            {**facts,'failureBoundary':True}, {**facts,'failureBoundary':{'path':'PRIVATE'}},
                            {**facts,'failureBoundary':'input','rawError':'PRIVATE'},
                            {**facts,'failureBoundary':'input','stage':'copied'},
                            {**facts,'failureBoundary':'input','stage':'sent'},
                            {**facts,'failureBoundary':'input','stage':'recovery-scope-unimplemented'}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')
            path.write_text(json.dumps({**facts,'failureBoundary':'action','stage':'action-uncertain'}))
            with self.assertRaises(ValueError):q.semantic_observations(root,'chatgpt-desktop')

    def test_linux_input_shape_is_closed_and_cannot_prove_a_sent_turn(self):
        facts = dict(schemaVersion=1, mechanism='claude-linux-native-chat', diagnosticsOnly=True,
                     stage='input-not-empty', submittedTurns=0, inputVerifiedTurns=0, copiedResponses=0,
                     retryAttempted=False, clipboardCleared=True)
        shape = dict(charCount=1, onlyLineBreaks=True, onlyWhitespace=True, onlyZeroWidthMarkers=False)
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'shape.json'
            path.write_text(json.dumps({**facts,'inputShape':shape}))
            self.assertEqual(q.semantic_observations(root,'claude-desktop')[0]['inputShape'],shape)
            objects={**shape,'onlyLineBreaks':False,'onlyWhitespace':False,'onlyObjectReplacement':True}
            path.write_text(json.dumps({**facts,'inputShape':objects}))
            self.assertEqual(q.semantic_observations(root,'claude-desktop')[0]['inputShape'],objects)
            for changed in ({**shape,'charCount':True}, {**shape,'charCount':0},
                            {**shape,'charCount':4097}, {**shape,'onlyWhitespace':False},
                            {**shape,'onlyZeroWidthMarkers':True}, {**shape,'onlyObjectReplacement':True},
                            {**objects,'onlyObjectReplacement':None}, {**objects,'onlyObjectReplacement':'PRIVATE'}, {**shape,'value':'PRIVATE'}):
                path.write_text(json.dumps({**facts,'inputShape':changed}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')
            path.write_text(json.dumps({**facts,'inputShape':shape,'stage':'sent'}))
            with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')

    def test_linux_embedded_text_observation_remains_closed_and_blocked(self):
        facts = dict(schemaVersion=1,mechanism='claude-linux-native-chat',diagnosticsOnly=True,
                     stage='input-not-empty',submittedTurns=0,inputVerifiedTurns=0,copiedResponses=0,
                     retryAttempted=False,clipboardCleared=True,
                     inputShape=dict(charCount=1,onlyLineBreaks=True,onlyWhitespace=True,onlyZeroWidthMarkers=False))
        shape=dict(nodeCount=3,paragraphCount=1,literalLfLeafCount=1,brLfLeafCount=1,exactFillerLfLeafCount=1)
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);path=root/'shape.json'
            path.write_text(json.dumps({**facts,'embeddedTextObservation':shape}))
            self.assertEqual(q.semantic_observations(root,'claude-desktop')[0]['embeddedTextObservation'],shape)
            for turns in (1,2):
                completed={**facts,'submittedTurns':turns,'inputVerifiedTurns':turns,'copiedResponses':turns}
                path.write_text(json.dumps({**completed,'embeddedTextObservation':shape}))
                self.assertEqual(q.semantic_observations(root,'claude-desktop')[0]['embeddedTextObservation'],shape)
            for changed in ({**shape,'nodeCount':0},{**shape,'nodeCount':65},{**shape,'nodeCount':True},
                            {**shape,'paragraphCount':4},{**shape,'brLfLeafCount':2},
                            {**shape,'exactFillerLfLeafCount':2},{**shape,'attributes':'PRIVATE'},None):
                path.write_text(json.dumps({**facts,'embeddedTextObservation':changed}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')
            for changed in ({**facts,'stage':'sent'},{**facts,'submittedTurns':1},
                            {**facts,'submittedTurns':3,'inputVerifiedTurns':3,'copiedResponses':3},
                            {key:value for key,value in facts.items() if key!='inputShape'}):
                path.write_text(json.dumps({**changed,'embeddedTextObservation':shape}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')

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
            for item in (facts, facts | {'inputChannel': 'cdp-dom', 'documentFocused': False},
                         {**facts, 'status': 'document-unfocused', 'documentFocused': False},
                         dict(status='initial-missing', identityUnchanged=None, mainScopeUnique=None,
                              documentFocused=None, counts=None)):
                path.write_text(json.dumps({**value, 'initialMainConfirmation': item}))
                self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['initialMainConfirmation'], item)
            for changed in (facts | {'inputChannel': 'native-focused', 'documentFocused': False},
                            facts | {'inputChannel': {}},
                            {**facts, 'status': 'PRIVATE'}, {**facts, 'targetId': 'PRIVATE'},
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

    def test_codex_selected_project_receipts_never_authorize_send_or_export_identity(self):
        setup=dict(schemaVersion=1,mechanism='codex-public-onboarding',diagnosticsOnly=True,
            stage='coding-readiness',errorCategory=None,conversationalScope=True,
            engineeringControl=True,roleClickAttempted=True,roleClickCompleted=True,
            engineeringChecked=True,continueControl=True,continueClickAttempted=True,
            continueClickCompleted=True,roleScopeAbsent=True,roleProofFailure='unmeasured',sessionProofFailure='unmeasured',
            taskScopeProved=True,taskClickAttempted=True,taskClickCompleted=True)
        state=dict(status='observed',diagnosticsOnly=True,statePairStable=True,
            ordinaryLocalProjectObserved=True,selectedIdCorrelated=True,sendAuthorized=False)
        menu=dict(status='observed',diagnosticsOnly=True,clickAttempted=True,clickCompleted=True,
            sendAuthorized=False,profileStateObservation=state)
        context=dict(verified=True,reason='verified',sourcePinned=True,newHomeController=True,
            localContext=True,retainedProject=True,retainedRoot=True,prewarmResolvedSelection=False,
            noPriorReservation=True,inputAuthorized=False)
        selected_state={**state,'ordinarySelectionCompleted':True}
        selected_menu={**menu,'selectionClickAttempted':True,'selectionClickCompleted':True,'profileStateObservation':selected_state}
        self.assertEqual(q.public_onboarding({**setup,'workspaceMenuObservation':selected_menu},'chatgpt-desktop')['workspaceMenuObservation'],selected_menu)
        staged=selected_menu|{'selectionStage':'completed'}
        self.assertEqual(q.public_onboarding(setup|{'workspaceMenuObservation':staged},'chatgpt-desktop')['workspaceMenuObservation'],staged)
        for stage in ['item-click','original-popup-close','closed-source','reopen-click','reopened-popup']:
            pending=dict(status='blocked',diagnosticsOnly=True,clickAttempted=True,clickCompleted=True,
                sendAuthorized=False,selectionClickAttempted=True,selectionStage=stage,reason='guard')
            self.assertEqual(q.public_onboarding(setup|{'workspaceMenuObservation':pending},'chatgpt-desktop')['workspaceMenuObservation'],pending)
        transition=dict(status='blocked',diagnosticsOnly=True,clickAttempted=True,clickCompleted=True,
            sendAuthorized=False,selectionClickAttempted=True,selectionStage='original-popup-close',reason='guard')
        for cause in ['deadline-or-owner','identity-changed','editor-changed','editor-unavailable','runtime-unavailable','source-close-unproved']:
            receipt=transition|{'selectionFailure':cause}
            self.assertEqual(q.public_onboarding(setup|{'workspaceMenuObservation':receipt},'chatgpt-desktop')['workspaceMenuObservation'],receipt)
        for changed in [{'selectionFailure':[]},{'selectionFailure':'PRIVATE'},
            {'selectionFailure':'editor-changed','selectionStage':'item-click'},
            {'selectionFailure':'editor-changed','status':'observed'}]:
            with self.assertRaises(ValueError):q.public_onboarding(setup|{'workspaceMenuObservation':transition|changed},'chatgpt-desktop')
        for changed in ({'selectionStage':[]},{'selectionStage':'PRIVATE'},{'selectionStage':'item-click'}):
            with self.assertRaises(ValueError):q.public_onboarding(setup|{'workspaceMenuObservation':staged|changed},'chatgpt-desktop')
        for change in [{'selectionClickAttempted':False},{'selectionClickCompleted':False},
                       {'selectionClickAttempted':1},{'rawProjectId':'PRIVATE'},
                       {'profileStateObservation':state},
                       {'profileStateObservation':selected_state|{'ordinarySelectionCompleted':False}}]:
            with self.subTest(change=change),self.assertRaises(ValueError):
                q.public_onboarding({**setup,'workspaceMenuObservation':selected_menu|change},'chatgpt-desktop')
        context_state={**state,'prewarmContext':context}
        self.assertEqual(q.public_onboarding({**setup,'workspaceMenuObservation':{**menu,'profileStateObservation':context_state}},'chatgpt-desktop')['workspaceMenuObservation']['profileStateObservation'],context_state)
        for change in ({'inputAuthorized':True},{'sourcePinned':False},{'reason':'PRIVATE'},
                       {'projectId':'PRIVATE'},{'noPriorReservation':False},{'prewarmResolvedSelection':1}):
            with self.assertRaises(ValueError):
                q.codex_prewarm_context({**context,**change})
        self.assertEqual(q.codex_prewarm_context({**context,'prewarmResolvedSelection':True,'noPriorReservation':False})['verified'],True)
        for reason in ['deadline-or-owner','runtime-unavailable','descriptor-unavailable','source-mismatch',
                       'scope-unavailable','root-mismatch','project-mismatch','mode-mismatch','existing-workspace',
                       'remote-override','controller-mismatch','host-or-cwd-mismatch','context-override','follow-up',
                       'prepare-override','reservation-mismatch','reservation-unavailable','reservation-pending',
                       'render-changed','identity-changed','editor-unavailable','editor-changed']:
            failed=dict(verified=False,reason=reason,inputAuthorized=False)
            self.assertEqual(q.codex_prewarm_context(failed),failed)
            with self.assertRaises(ValueError):q.codex_prewarm_context({**failed,'raw':'PRIVATE'})
        self.assertEqual(q.public_onboarding({**setup,'workspaceMenuObservation':menu},'chatgpt-desktop')['workspaceMenuObservation'],menu)
        for change in ({'sendAuthorized':True},{'projectId':'PRIVATE'},{'status':[]},
                       {'statePairStable':False},{'selectedIdCorrelated':1},{'reason':'PRIVATE'}):
            with self.assertRaises(ValueError):
                q.public_onboarding({**setup,'workspaceMenuObservation':{**menu,'profileStateObservation':{**state,**change}}},'chatgpt-desktop')
        for change in ({'clickAttempted':False},{'sendAuthorized':True},{'path':'PRIVATE'},{'status':[]}):
            with self.assertRaises(ValueError):
                q.public_onboarding({**setup,'workspaceMenuObservation':{**menu,**change}},'chatgpt-desktop')
        blocked={**state,'status':'blocked','statePairStable':False,'selectedIdCorrelated':False,'reason':'selected-id'}
        self.assertEqual(q.public_onboarding({**setup,'workspaceMenuObservation':{**menu,'profileStateObservation':blocked}},'chatgpt-desktop')['workspaceMenuObservation']['profileStateObservation'],blocked)
        for reason in ['menu','list','limit','selected-id']:
            selected={'reason':reason}
            if reason=='selected-id':selected.update(selectedItemCount=0,matchingItemCount=1)
            value={**blocked,'selectedProjectObservation':selected}
            self.assertEqual(q.public_onboarding({**setup,'workspaceMenuObservation':{**menu,'profileStateObservation':value}},'chatgpt-desktop')['workspaceMenuObservation']['profileStateObservation'],value)
            for change in ({'reason':'PRIVATE'},{'projectId':'PRIVATE'},{'selectedItemCount':True},{'matchingItemCount':33}):
                with self.assertRaises(ValueError):
                    q.public_onboarding({**setup,'workspaceMenuObservation':{**menu,'profileStateObservation':{**value,'selectedProjectObservation':{**selected,**change}}}},'chatgpt-desktop')
        for tag in ['container','projects-shape','projects-count','project-namespace','record-shape',
                    'record-identity','record-time','record-root','stored-selection']:
            value={**blocked,'reason':'project','projectFailure':tag}
            self.assertEqual(q.public_onboarding({**setup,'workspaceMenuObservation':{**menu,'profileStateObservation':value}},'chatgpt-desktop')['workspaceMenuObservation']['profileStateObservation'],value)
            for change in ({'projectFailure':'PRIVATE'},{'projectFailure':[]},{'reason':'selected-id'},
                           {'status':'observed'},{'projectId':'PRIVATE'}):
                with self.assertRaises(ValueError):
                    q.public_onboarding({**setup,'workspaceMenuObservation':{**menu,'profileStateObservation':{**value,**change}}},'chatgpt-desktop')

    def test_public_onboarding_receipts_reject_private_and_untyped_data(self):
        setup = dict(schemaVersion=1, mechanism='codex-public-onboarding', diagnosticsOnly=True,
                     stage='stopped-after-role', errorCategory=None, conversationalScope=True,
                     engineeringControl=True, roleClickAttempted=True, roleClickCompleted=True,
                     engineeringChecked=True, continueControl=True, continueClickAttempted=True,
                     continueClickCompleted=True, roleScopeAbsent=True, roleProofFailure='unmeasured', sessionProofFailure='unmeasured')
        self.assertEqual(q.public_onboarding(setup, 'chatgpt-desktop'), setup)
        for kind in ('get-started', 'skip-optional-capabilities'):
            bound = {**setup, 'taskScopeProved':True, 'taskControlKind':kind}
            self.assertEqual(q.public_onboarding(bound, 'chatgpt-desktop'), bound)
        for changed in ({'taskControlKind':'PRIVATE'}, {'taskControlKind':[]},
                        {'taskControlKind':'skip-optional-capabilities','taskScopeProved':False},
                        {'taskControlKind':'skip-optional-capabilities','roleScopeAbsent':False}):
            with self.assertRaises(ValueError):
                q.public_onboarding({**setup, 'taskScopeProved':True, **changed}, 'chatgpt-desktop')
        coding = dict(status='observed',composerCount=1,conversationCount=0,modalCount=0,
                      roleRadioCount=0,exactAckLeafCount=0,exactGetStartedCount=0,exactSkipCount=0)
        navigation = dict.fromkeys('codexButtonCount codexLinkCount codexMenuItemCount chatModeTriggerCount codexModeTriggerCount projectSelectorCount newChatCount projectsLinkCount'.split(), 0)
        navigation.update(status='observed', uniqueCodexRole='none', uniqueCodexHitActionable=False)
        editable = dict.fromkeys('codexHomeCount codexThreadCount codexOtherCount classicChatGPTCount genericInputCount genericBodyCount unboundCount editableCount sidebarNewChatCount'.split(), 0)
        editable.update(status='observed', sidebarNewChatHitActionable=False, codexHomeCount=1, editableCount=1)
        home = dict.fromkeys('homeComposerCount pendingTextareaCount pendingGroupCount proseMirrorEditableCount enabledSendCount disabledSendCount workspaceControlCount'.split(), 0)
        home.update(status='observed', homeComposerCount=1, proseMirrorEditableCount=1)
        transition = setup | dict(stage='coding-readiness', taskScopeProved=False, taskClickAttempted=False,
            taskClickCompleted=False, codingComposerReady=True, homeAfterContinueReady=True,
            transitionReadinessObservation=coding,
            transitionPublicDOMObservation=dict(navigation=navigation, editable=editable, home=home))
        self.assertEqual(q.public_onboarding(transition, 'chatgpt-desktop'), transition)
        for changed in [{'taskClickCompleted':True}, {'roleScopeAbsent':False}, {'homeAfterContinueReady':1},
                        {'transitionReadinessObservation':coding | {'rawText':'PRIVATE'}},
                        {'transitionPublicDOMObservation':{'rawDOM':'PRIVATE'}}]:
            with self.assertRaises(ValueError): q.public_onboarding(transition | changed, 'chatgpt-desktop')
        bound = {**setup,'stage':'coding-readiness','taskScopeProved':True,'taskClickAttempted':True,
                 'taskClickCompleted':True,'codingComposerReady':False,'codingReadinessObservation':coding}
        self.assertEqual(q.public_onboarding(bound,'chatgpt-desktop'),bound)
        for changed in ({**coding,'composerCount':True}, {**coding,'conversationCount':33},
                        {**coding,'status':'PRIVATE'}, {**coding,'text':'PRIVATE'},
                        {**coding,'status':'overflow'}, {**coding,'composerCount':None}):
            with self.assertRaises(ValueError):q.public_onboarding({**bound,'codingReadinessObservation':changed},'chatgpt-desktop')
        overflow = {key:None for key in coding if key != 'status'} | {'status':'overflow'}
        self.assertEqual(q.public_onboarding({**bound,'codingReadinessObservation':overflow},'chatgpt-desktop')['codingReadinessObservation'],overflow)
        navigation = dict(status='observed',sourceVersion='26.930.41038',
            sourceSha256='28c6096af241a37a9a33a2e5601f0aa05426910d5c84d08824d852342a2b4d5d',
            codexButtonCount=1,codexLinkCount=0,codexMenuItemCount=0,chatModeTriggerCount=1,
            codexModeTriggerCount=0,projectSelectorCount=0,newChatCount=0,projectsLinkCount=0,
            uniqueCodexRole='button',uniqueCodexHitActionable=True)
        observed = {**bound,'codingNavigationObservation':navigation}
        self.assertEqual(q.public_onboarding(observed,'chatgpt-desktop'),observed)
        for change in ({'sourceVersion':'PRIVATE'},{'codexButtonCount':True},
                       {'codexButtonCount':33},{'uniqueCodexRole':'link'},
                       {'codexLinkCount':1},{'rawName':'PRIVATE'},{'uniqueCodexRole':[]}):
            with self.assertRaises(ValueError):
                q.public_onboarding({**observed,'codingNavigationObservation':{**navigation,**change}},'chatgpt-desktop')
        home = dict(status='observed',sourcePlatform='linux',sourceVersion='26.930.41038',
            composerSourceSha256='7198ee078e78a748d03c3cc96f5c041d056584d762728fd0be23e695bc394da0',
            pageSourceSha256='9c9d0d9247226d43edeb4606a539b518e3be06fb65aa37bd014984fbe3998ba9',
            homeRootCount=1,localHomeComposerCount=1,homeEditableCount=1,homeProseMirrorCount=1,workspaceControlCount=1)
        home_bound={**bound,'codingHomeObservation':home}
        self.assertEqual(q.public_onboarding(home_bound,'chatgpt-desktop'),home_bound)
        for change in ({'sourcePlatform':'windows'},{'sourceVersion':'PRIVATE'},
                       {'composerSourceSha256':'PRIVATE'},{'pageSourceSha256':'PRIVATE'},
                       {'homeRootCount':False},{'homeEditableCount':33},{'rawPath':'PRIVATE'},
                       {'homeProseMirrorCount':2},{'homeRootCount':0},{'localHomeComposerCount':0},
                       {'homeEditableCount':None},{'status':'overflow'}):
            with self.assertRaises(ValueError):
                q.public_onboarding({**home_bound,'codingHomeObservation':{**home,**change}},'chatgpt-desktop')
        overflow_home={**home,'status':'overflow',**dict.fromkeys(['homeRootCount','localHomeComposerCount',
            'homeEditableCount','homeProseMirrorCount','workspaceControlCount'])}
        self.assertEqual(q.public_onboarding({**bound,'codingHomeObservation':overflow_home},'chatgpt-desktop')['codingHomeObservation'],overflow_home)
        for changed in ({'taskClickCompleted':False}, {'taskScopeProved':False}, {'stage':'task-action'}):
            with self.assertRaises(ValueError):q.public_onboarding({**bound,**changed},'chatgpt-desktop')
        task = dict(heldScopeConnected=True, heldScopeVisible=True,
                    roleRadioCount=0, exactAckLeafCount=1, exactGetStartedCount=0)
        measured = {**setup, 'taskScopeObservation':task}
        self.assertEqual(q.public_onboarding(measured, 'chatgpt-desktop'), measured)
        with self.assertRaises(ValueError):
            q.public_onboarding({**setup, 'taskScopeObservation':{**task, 'rawText':'PRIVATE'}}, 'chatgpt-desktop')
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
        actionability = dict(status='observed', dialogOpacityZero=True, ancestorOpacityZero=False,
                             dialogPointerEventsNone=True, ancestorPointerEventsNone=False,
                             inert=True, stateClosed=True, targetOwnedPointCount=3,
                             dialogOwnedPointCount=3, otherPointCount=3)
        self.assertEqual(q.public_onboarding({**surface, 'foreignOverlayActionability': actionability},
                                            'chatgpt-desktop')['foreignOverlayActionability'], actionability)
        unavailable = {key: None for key in actionability}
        unavailable['status'] = 'unavailable'
        self.assertEqual(q.public_onboarding({**surface, 'foreignOverlayActionability': unavailable},
                                            'chatgpt-desktop')['foreignOverlayActionability'], unavailable)
        for reason in ('ancestor-limit', 'ancestor-detached', 'opacity-invalid',
                       'pointer-property-invalid', 'geometry-invalid', 'geometry-outside'):
            typed = {**unavailable, 'unavailableReason': reason}
            self.assertEqual(q.public_onboarding({**surface, 'foreignOverlayActionability': typed},
                                                'chatgpt-desktop')['foreignOverlayActionability'], typed)
        for invalid in ({**actionability, 'unavailableReason': 'geometry-invalid'},
                        {**unavailable, 'unavailableReason': None},
                        {**unavailable, 'unavailableReason': 'PRIVATE'},
                        {**actionability, 'rawStyle': 'PRIVATE'}, {**actionability, 'status': []},
                        {**actionability, 'inert': 1}, {**actionability, 'otherPointCount': 2},
                        {**actionability, 'dialogOwnedPointCount': True},
                        {**unavailable, 'targetOwnedPointCount': 0}):
            with self.assertRaises(ValueError):
                q.public_onboarding({**surface, 'foreignOverlayActionability': invalid}, 'chatgpt-desktop')
        source_counts = dict(computerHistoryTitleCount=1, computerHistoryFormCount=1,
                             computerHistoryNotNowCount=1, computerHistoryCustomizeCount=1, computerHistoryAllowCount=1,
                             projectImportTitleCount=0, projectImportContinueCount=0, projectImportNotNowCount=1)
        history = {**surface, 'foreignOverlay': 'other', 'foreignOverlayFingerprint': 'computer-history-consent',
                   'foreignOverlayHeading': 'computer-history-consent', 'foreignOverlaySourceCounts': source_counts}
        self.assertEqual(q.public_onboarding(history, 'chatgpt-desktop'), history)
        for invalid in ({**source_counts, 'PRIVATE': 'PRIVATE'}, {**source_counts, 'computerHistoryTitleCount': True},
                        {**source_counts, 'projectImportTitleCount': 33}, {**source_counts, 'computerHistoryFormCount': 0}):
            with self.assertRaises(ValueError):
                q.public_onboarding({**history, 'foreignOverlaySourceCounts': invalid}, 'chatgpt-desktop')
        counts = dict(titleCount=1, continueCount=1, notNowCount=1, skipCount=0)
        measured = {**surface, 'foreignOverlayHeading': 'imported-setup',
                    'foreignOverlayImportSetup': counts}
        self.assertEqual(q.public_onboarding(measured, 'chatgpt-desktop'), measured)
        self.assertEqual(q.public_onboarding({**measured, 'foreignOverlayImportSetup': {
            **counts, 'notNowCount': 0, 'skipCount': 1}}, 'chatgpt-desktop')['foreignOverlayHeading'], 'imported-setup')
        for invalid in ({**counts, 'text': 'PRIVATE'}, {**counts, 'titleCount': True},
                        {**counts, 'titleCount': 33}, {**counts, 'continueCount': 0},
                        {**counts, 'skipCount': 1}, {}, None):
            with self.assertRaises(ValueError):
                q.public_onboarding({**measured, 'foreignOverlayImportSetup': invalid}, 'chatgpt-desktop')
        with self.assertRaises(ValueError):
            q.public_onboarding({**surface, 'foreignOverlayHeading': 'imported-setup'}, 'chatgpt-desktop')
        with self.assertRaises(ValueError):
            q.public_onboarding({**measured, 'foreignOverlayProof': 'ownership-lost'}, 'chatgpt-desktop')
        with self.assertRaises(ValueError):
            q.public_onboarding(measured, 'claude-desktop')


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

class TaskScopeObservationTests(unittest.TestCase):
    def test_closed_counts_preserve_pending_and_reject_private_or_inconsistent_shapes(self):
        facts = dict(heldScopeConnected=True, heldScopeVisible=True,
                     roleRadioCount=0, exactAckLeafCount=1, exactGetStartedCount=0)
        self.assertEqual(q.task_scope_observation(facts), facts)
        detached = {**facts, 'heldScopeConnected':False, 'heldScopeVisible':False,
                    'roleRadioCount':None, 'exactAckLeafCount':None, 'exactGetStartedCount':None}
        self.assertEqual(q.task_scope_observation(detached), detached)
        for changed in ({**facts, 'rawText':'PRIVATE'}, {**facts, 'exactAckLeafCount':True},
                        {**facts, 'exactGetStartedCount':4097}, {**facts, 'roleRadioCount':None},
                        {**facts, 'heldScopeConnected':False}, {**facts, 'heldScopeVisible':False},
                        {**facts, 'exactAckLeafCount':'PRIVATE'}):
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                q.task_scope_observation(changed)


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
            boundary = {**sampled, 'guardBeforeVerified': 56, 'guardAfterVerified': 56,
                        'accessibleChecks': 56, 'accessibleExactMatches': 56,
                        'pointerChecks': 45, 'pointerPositionMatches': 45, 'pointerChildMatches': 45,
                        'cursorChecks': 46,
                        'cursorClasses': dict(hand=0, arrow=46, notallowed=0, transparent=0, unknown=0)}
            path.write_text(json.dumps({**value, 'cursorSelection': boundary}))
            self.assertEqual(q.semantic_observations(tmp, 'zed-desktop')[0]['cursorSelection'], boundary)
            for key in ('cursorChecks', 'accessibleChecks', 'guardBeforeVerified'):
                path.write_text(json.dumps({**value, 'cursorSelection': {**boundary, key: boundary[key] + 1}}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(tmp, 'zed-desktop')
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

class ClaudeLinuxModeRolesTests(unittest.TestCase):
    def test_passive_roles_require_exact_source_and_unique_group(self):
        value = dict(schemaVersion=1, mechanism='claude-linux-mode-roles', diagnosticsOnly=True,
                     sourceVersion='2.9939.4', status='observed',
                     modeSourceSha256='62ffbc1b8a3e4440ae77a33be142afd1914796f945bcd75d58cfe73679925f61',
                     segmentedSourceSha256='1fe986422649ab736613079340a52157efd7791b96e0b9c00c46681731b7a4ea',
                     radioSourceSha256='9c6ff87b4eaf0e9ad25e6329536f4337586b015e0f868389e72480c1769920a9',
                     sourceCount=dict(modeGroupVisible=1, chatButtonVisible=0, chatButtonEnabled=0,
                                      chatRadioVisible=1, chatRadioEnabled=1, coworkButtonVisible=0,
                                      coworkButtonEnabled=0, coworkRadioVisible=0, coworkRadioEnabled=0))
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'roles.json'
            path.write_text(json.dumps(value))
            result = q.semantic_observations(root, 'claude-desktop')[0]
            self.assertEqual(result['sourceCount']['chatRadioVisible'], 1)
            self.assertNotIn('Chat', json.dumps(result))
            for change in ({'label': 'PRIVATE'}, {'sourceVersion': '2.19675.0'},
                           {'status': []}, {'segmentedSourceSha256': '0' * 64},
                           {'sourceCount': {**value['sourceCount'], 'modeGroupVisible': 2}},
                           {'sourceCount': {**value['sourceCount'], 'chatRadioVisible': True}},
                           {'sourceCount': {**value['sourceCount'], 'chatRadioVisible': 4097}}):
                path.write_text(json.dumps({**value, **change}))
                with self.assertRaises(ValueError): q.semantic_observations(root, 'claude-desktop')
            shape_keys = 'buttonAll buttonVisible radioAll radioVisible switchAll switchVisible staticTextAll staticTextVisible chatAll chatAwaitingAll chatUnreadAll chatWorkingAll coworkAll coworkAwaitingAll coworkUnreadAll coworkWorkingAll'.split()
            shape_counts = dict.fromkeys(shape_keys, 0)
            shape_counts.update(buttonAll=1, chatAll=1)
            shape = dict(status='observed', counts=shape_counts)
            path.write_text(json.dumps({**value, 'roleShape': shape}))
            observed = q.semantic_observations(root, 'claude-desktop')[0]
            self.assertEqual(observed['roleShape']['counts']['chatAll'], 1)
            self.assertEqual(observed['roleShape']['counts']['buttonVisible'], 0)
            unavailable = dict(status='unavailable', counts=dict.fromkeys(shape_keys))
            path.write_text(json.dumps({**value, 'roleShape': unavailable}))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop')[0]['roleShape'], unavailable)
            for bad in ({**shape, 'name': 'PRIVATE'}, {**shape, 'status': []},
                        {**shape, 'counts': {**shape_counts, 'buttonAll': True}},
                        {**shape, 'counts': {**shape_counts, 'buttonAll': 4097}},
                        {**shape, 'counts': {**shape_counts, 'buttonVisible': 2}},
                        {**shape, 'counts': {**shape_counts, 'raw': 1}},
                        {**unavailable, 'counts': shape_counts}):
                path.write_text(json.dumps({**value, 'roleShape': bad}))
                with self.assertRaises(ValueError): q.semantic_observations(root, 'claude-desktop')
            nulls = dict.fromkeys(value['sourceCount'])
            for status, group in [('group-unavailable', 0), ('group-ambiguous', 2),
                                  ('query-failed', None), ('query-failed', 1)]:
                measured = {**value, 'status': status, 'sourceCount': {**nulls, 'modeGroupVisible': group}}
                path.write_text(json.dumps(measured))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop')[0]['status'], status)
                path.write_text(json.dumps({**measured, 'sourceCount': {**measured['sourceCount'], 'chatRadioVisible': 0}}))
                with self.assertRaises(ValueError): q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError): q.semantic_observations(root, 'chatgpt-desktop')


class CodexLinuxInitialDialogTests(unittest.TestCase):
    def test_exact_pins_passive_counts_and_unavailable_are_closed(self):
        names = ('allSet', 'importedSetup', 'computerHistory', 'projectImport')
        counts = {'dialogCount': 1} | {name + suffix: 0 for name in names for suffix in ('TitleCount', 'MatchCount')}
        value = dict(schemaVersion=1, mechanism='codex-linux-startup-dialog', diagnosticsOnly=True,
                     sourceVersion='26.930.31730', status='other', candidate='unknown', sourceCount=counts,
                     completeSourceSha256='16b6c59e36aa19da0c4ec1560b6cedec43fabffeca2601710cb6f25f22c593cc',
                     onboardingSourceSha256='b8dff84333a6cfb62341d43642087ba8d72dd31225ed2b3b8e29ad7da31372c6',
                     projectSourceSha256='802041599f534cdc852760bcc3eb18bc4bdc2fda523b8983098c5946476504a9')
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'dialog.json'
            for name, candidate in zip(names, ('all-set', 'imported-setup', 'computer-history', 'project-import')):
                measured = {**value, 'status': 'matched', 'candidate': candidate,
                            'sourceCount': {**counts, name + 'TitleCount': 1, name + 'MatchCount': 1}}
                path.write_text(json.dumps(measured))
                self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['candidate'], candidate)
            unavailable = {**value, 'status': 'guard-rejected', 'sourceCount': dict.fromkeys(counts)}
            path.write_text(json.dumps(unavailable))
            self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['status'], 'guard-rejected')
            for change in ({'status': []}, {'candidate': 'PRIVATE'}, {'title': 'PRIVATE'},
                           {'completeSourceSha256': '0' * 64}, {'sourceVersion': 'PRIVATE'},
                           {'sourceCount': {**counts, 'dialogCount': True}},
                           {'sourceCount': {**counts, 'allSetTitleCount': 33}},
                           {'sourceCount': {**counts, 'allSetMatchCount': 1}},
                           {'status': 'matched', 'candidate': 'all-set'}):
                path.write_text(json.dumps({**value, **change}))
                with self.assertRaises(ValueError): q.semantic_observations(root, 'chatgpt-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError): q.semantic_observations(root, 'claude-desktop')


class CodexStaticDialogTitleTests(unittest.TestCase):
    def test_folder_consent_variants_are_closed_passive_identities(self):
        for platform, version, artifact, wrapper, digest in [('windows', '26.930.51102', '12070c9dd6cca622d043abdaf2225406abe6de19e8061024d93b93255478603e', '229a6d36b32d5f610199794dd61ace9cdd1a6c91853119e50d4e5212e6f771cd', 'a2ebf9ee2a78930256ea089e1458fb0856e62bf25f1ec459dc701b17c24e3c1e'), ('linux', '26.930.41038', 'ee7854145554718d7239d01ea37d44f6ba1e0ba4a93f47ac097d6e0f964da47c', 'c3c9a86a6d9c3a2a8cecaf0a6a22527c69f89949cb0d8958896bc86131e9c6c9', 'b6566a8d50edd58ed59e29eb2c9ef9de10d72f6e650f3ee0ec0a50a927106ee0'), ('macos', '26.930.41038', 'f6cf4d2e9b69aeefa33adda4bcd1a2d306357f5253a1ac6049700870c28dd0c7', '0703d0aa97450d6d21346e1c79c887a5bf9062cd0069e8251ec03748a33b6dd0', '82df6ff119bf98beba8ffe1a593aca671119decdbb8f4f3d39e5378e39b02c48')]:
            value = dict(schemaVersion=1, mechanism='codex-static-dialog-title', diagnosticsOnly=True,
                         sourceVersion=version, platform=platform, artifactSha256=artifact,
                         wrapperSourceSha256=wrapper, catalogSha256=digest, status='matched',
                         titleReferenceCount=1, matchCount=1)
            with tempfile.TemporaryDirectory() as root:
                path = Path(root) / 'title.json'
                for identity in ['projectSetup.consent.title.one', 'projectSetup.consent.title.other', 'projectSetup.consent.untrustedTitle.one', 'projectSetup.consent.untrustedTitle.other']:
                    path.write_text(json.dumps({**value, 'sourceTitleIds': [identity]}))
                    self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['sourceTitleIds'], [identity])
                for change in ({'sourceTitleIds': ['projectSetup.consent.title.path.PRIVATE']},
                               {'sourceTitleIds': ['projectSetup.consent.title.one'], 'workspace': 'PRIVATE'}):
                    path.write_text(json.dumps({**value, **change}))
                    with self.assertRaises(ValueError): q.semantic_observations(root, 'chatgpt-desktop')

    def test_catalog_rejection_is_closed_and_does_not_publish_title_identity(self):
        value = dict(schemaVersion=1, mechanism='codex-static-dialog-title', diagnosticsOnly=True,
                     sourceVersion='26.930.51102', platform='windows',
                     artifactSha256='12070c9dd6cca622d043abdaf2225406abe6de19e8061024d93b93255478603e',
                     wrapperSourceSha256='229a6d36b32d5f610199794dd61ace9cdd1a6c91853119e50d4e5212e6f771cd',
                     catalogSha256='a2ebf9ee2a78930256ea089e1458fb0856e62bf25f1ec459dc701b17c24e3c1e',
                     status='guard-rejected', titleReferenceCount=None, matchCount=None,
                     sourceTitleIds=[], sourceTitleEmpty=None, rejectionStage='title-tag')
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'title.json'
            for stage in ('scope', 'deadline', 'dialog-count', 'reference', 'title-count', 'title-tag',
                          'title-text', 'actionability', 'query', 'changed', 'unmeasured'):
                path.write_text(json.dumps({**value, 'rejectionStage': stage}))
                self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['rejectionStage'], stage)
            for change in ({'rejectionStage': 'PRIVATE'}, {'rejectionStage': None},
                           {'sourceTitleIds': ['keyboardShortcutsDialog.title']}, {'privateTag': 'DIV'}):
                path.write_text(json.dumps({**value, **change}))
                with self.assertRaises(ValueError): q.semantic_observations(root, 'chatgpt-desktop')
            observed = {**value, 'status': 'unknown', 'titleReferenceCount': 1, 'matchCount': 0,
                        'sourceTitleEmpty': False, 'rejectionStage': None}
            path.write_text(json.dumps(observed))
            self.assertIsNone(q.semantic_observations(root, 'chatgpt-desktop')[0]['rejectionStage'])
            path.write_text(json.dumps({**observed, 'rejectionStage': 'title-tag'}))
            with self.assertRaises(ValueError): q.semantic_observations(root, 'chatgpt-desktop')

    def test_linux_catalog_binds_new_version_and_exact_public_bytes(self):
        value = dict(schemaVersion=1, mechanism='codex-static-dialog-title', diagnosticsOnly=True,
                     sourceVersion='26.930.41038', platform='linux',
                     artifactSha256='ee7854145554718d7239d01ea37d44f6ba1e0ba4a93f47ac097d6e0f964da47c',
                     wrapperSourceSha256='c3c9a86a6d9c3a2a8cecaf0a6a22527c69f89949cb0d8958896bc86131e9c6c9',
                     catalogSha256='b6566a8d50edd58ed59e29eb2c9ef9de10d72f6e650f3ee0ec0a50a927106ee0',
                     status='matched', titleReferenceCount=1, matchCount=1,
                     sourceTitleIds=['chatgpt.global_search.modal.title'])
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'title.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['sourceVersion'], '26.930.41038')
            for identity in ('workspaceOnboarding.dialogTitle', 'work.onboarding.role.new.question', 'desktop.windowCloseConfirmation.title', 'chatgptConversations.lockdown.dialog.title', 'appHeader.installUpdate.confirmTitle'):
                path.write_text(json.dumps({**value, 'sourceTitleIds': [identity]}))
                self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['sourceTitleIds'], [identity])
                path.write_text(json.dumps({**value, 'platform': 'windows', 'sourceTitleIds': [identity]}))
                with self.assertRaises(ValueError): q.semantic_observations(root, 'chatgpt-desktop')
            for patch in ({'sourceVersion': '26.930.31730'}, {'platform': 'macos'},
                          {'artifactSha256': '0' * 64}, {'wrapperSourceSha256': '0' * 64},
                          {'catalogSha256': '0' * 64}, {'privateText': 'PRIVATE_SENTINEL'}):
                path.write_text(json.dumps({**value, **patch}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'chatgpt-desktop')

    def test_exact_catalog_pins_and_source_ids_are_passive_closed(self):
        value = dict(schemaVersion=1, mechanism='codex-static-dialog-title', diagnosticsOnly=True,
                     sourceVersion='26.930.41038', platform='macos',
                     artifactSha256='f6cf4d2e9b69aeefa33adda4bcd1a2d306357f5253a1ac6049700870c28dd0c7',
                     wrapperSourceSha256='0703d0aa97450d6d21346e1c79c887a5bf9062cd0069e8251ec03748a33b6dd0',
                     catalogSha256='82df6ff119bf98beba8ffe1a593aca671119decdbb8f4f3d39e5378e39b02c48',
                     status='matched', titleReferenceCount=1, matchCount=1,
                     sourceTitleIds=['electron.onboarding.conversationalOnboarding.skipDialog.title'])
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'title.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['sourceTitleIds'], value['sourceTitleIds'])
            windows = {**value, 'platform': 'windows', 'sourceVersion': '26.930.51102',
                       'artifactSha256': '12070c9dd6cca622d043abdaf2225406abe6de19e8061024d93b93255478603e',
                       'wrapperSourceSha256': '229a6d36b32d5f610199794dd61ace9cdd1a6c91853119e50d4e5212e6f771cd',
                       'catalogSha256': 'a2ebf9ee2a78930256ea089e1458fb0856e62bf25f1ec459dc701b17c24e3c1e'}
            path.write_text(json.dumps(windows))
            self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['platform'], 'windows')
            for key in ('artifactSha256', 'wrapperSourceSha256', 'catalogSha256'):
                path.write_text(json.dumps({**windows, key: value[key]}))
                with self.assertRaises(ValueError): q.semantic_observations(root, 'chatgpt-desktop')
            for platform_value in (value, windows):
                for identity in ('chatgpt.global_search.modal.title', 'settings.browserUse.profileImport.title', 'settings.browserUse.profileImport.extensionsConfirmationTitle'):
                    path.write_text(json.dumps({**platform_value, 'sourceTitleIds': [identity], 'sourceTitleEmpty': False}))
                    self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['sourceTitleIds'], [identity])
            empty = {**value, 'status': 'unknown', 'matchCount': 0, 'sourceTitleIds': [], 'sourceTitleEmpty': True}
            path.write_text(json.dumps(empty))
            self.assertTrue(q.semantic_observations(root, 'chatgpt-desktop')[0]['sourceTitleEmpty'])
            for changed in ({**value, 'sourceTitleEmpty': True}, {**empty, 'sourceTitleEmpty': 'PRIVATE'},
                            {**empty, 'sourceTitleEmpty': None}, {**empty, 'sourceTitleEmpty': 1}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError): q.semantic_observations(root, 'chatgpt-desktop')
            for status, count, identities in [('unknown', 0, []), ('guard-rejected', None, []),
                                              ('ambiguous', 2, sorted(['keyboardShortcutsDialog.title', 'plugins.create.title']))]:
                item = {**value, 'status': status, 'matchCount': count, 'sourceTitleIds': identities,
                        'titleReferenceCount': None if count is None else 1}
                path.write_text(json.dumps(item))
                self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['status'], status)
            for identity in ('workspaceOnboarding.dialogTitle', 'work.onboarding.role.new.question', 'desktop.windowCloseConfirmation.title', 'chatgptConversations.lockdown.dialog.title', 'appHeader.installUpdate.confirmTitle'):
                path.write_text(json.dumps({**value, 'sourceTitleIds': [identity]}))
                self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['sourceTitleIds'], [identity])
            for failure in ('held-document', 'page-set', 'native-ownership', 'retained-document', 'document-focus', 'catalog-limit'):
                rejected = {**value, 'status': 'guard-rejected', 'rejectionStage': 'scope', 'guardFailure': failure,
                            'titleReferenceCount': None, 'matchCount': None, 'sourceTitleIds': []}
                path.write_text(json.dumps(rejected))
                self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['guardFailure'], failure)
                for patch in ({'guardFailure': 'PRIVATE'}, {'rejectionStage': 'query'}, {'guardFailure': []}):
                    path.write_text(json.dumps({**rejected, **patch}))
                    with self.assertRaises(ValueError): q.semantic_observations(root, 'chatgpt-desktop')
            shape = dict(pageRoleLegend=1, dialogRoleLegend=0, pageRoleRadios=11, dialogRoleRadios=0,
                         pageEngineering=1, dialogEngineering=0, dialogContinue=1, dialogGetStarted=0)
            path.write_text(json.dumps({**value, 'sourceShape': shape}))
            self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['sourceShape'], shape)
            for changed in ({**shape, 'raw': 'PRIVATE'}, {**shape, 'dialogEngineering': 2},
                            {**shape, 'pageRoleRadios': True}, {**shape, 'dialogContinue': 4097}):
                path.write_text(json.dumps({**value, 'sourceShape': changed}))
                with self.assertRaises(ValueError): q.semantic_observations(root, 'chatgpt-desktop')
            for change in ({'sourceVersion': '26.930.31730'}, {'text': 'PRIVATE'}, {'platform': []}, {'status': []}, {'sourceTitleIds': ['PRIVATE']},
                           {'sourceTitleIds': value['sourceTitleIds'] * 2}, {'titleReferenceCount': True},
                           {'sourceTitleIds': ['unadmitted.static.title']},
                           {'matchCount': True}, {'matchCount': 2}, {'artifactSha256': '0' * 64},
                           {'wrapperSourceSha256': '0' * 64}, {'catalogSha256': '0' * 64},
                           {'status': 'guard-rejected'}, {'status': 'unknown'}, {'sourceVersion': 'PRIVATE'}):
                path.write_text(json.dumps({**value, **change}))
                with self.assertRaises(ValueError): q.semantic_observations(root, 'chatgpt-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError): q.semantic_observations(root, 'claude-desktop')

class CodexCommandMenuShapeTests(unittest.TestCase):
    def test_only_closed_counts_and_exact_platform_catalog_can_be_recorded(self):
        pins = {'windows': ('26.930.51102', '12070c9dd6cca622d043abdaf2225406abe6de19e8061024d93b93255478603e', '229a6d36b32d5f610199794dd61ace9cdd1a6c91853119e50d4e5212e6f771cd', 'a2ebf9ee2a78930256ea089e1458fb0856e62bf25f1ec459dc701b17c24e3c1e'), 'linux': ('26.930.41038', 'ee7854145554718d7239d01ea37d44f6ba1e0ba4a93f47ac097d6e0f964da47c', 'c3c9a86a6d9c3a2a8cecaf0a6a22527c69f89949cb0d8958896bc86131e9c6c9', 'b6566a8d50edd58ed59e29eb2c9ef9de10d72f6e650f3ee0ec0a50a927106ee0'), 'macos': ('26.930.41038', 'f6cf4d2e9b69aeefa33adda4bcd1a2d306357f5253a1ac6049700870c28dd0c7', '0703d0aa97450d6d21346e1c79c887a5bf9062cd0069e8251ec03748a33b6dd0', '82df6ff119bf98beba8ffe1a593aca671119decdbb8f4f3d39e5378e39b02c48')}
        for platform, (version, artifact, wrapper, catalog) in pins.items():
            value = dict(schemaVersion=1, mechanism='codex-static-dialog-title', diagnosticsOnly=True,
                         sourceVersion=version, platform=platform, artifactSha256=artifact,
                         wrapperSourceSha256=wrapper, catalogSha256=catalog, status='matched',
                         titleReferenceCount=1, matchCount=1, sourceTitleIds=['codex.commandMenu.title'],
                         commandMenuShape=dict(dialogMarkerCount=1, globalScopeCount=1,
                                               rootCount=1, inputCount=1, listCount=1))
            with tempfile.TemporaryDirectory() as root:
                path = Path(root) / 'title.json'
                path.write_text(json.dumps(value))
                self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['commandMenuShape'], value['commandMenuShape'])
                # A source-shaped control is advisory even when its title is unknown.
                unknown = {**value, 'status': 'unknown', 'matchCount': 0, 'sourceTitleIds': []}
                path.write_text(json.dumps(unknown))
                self.assertEqual(q.semantic_observations(root, 'chatgpt-desktop')[0]['status'], 'unknown')
                for shape in ({**value['commandMenuShape'], 'label': 'PRIVATE'},
                              {**value['commandMenuShape'], 'inputCount': True},
                              {**value['commandMenuShape'], 'rootCount': 4097},
                              {**value['commandMenuShape'], 'globalScopeCount': 2},
                              {**value['commandMenuShape'], 'listCount': -1}):
                    path.write_text(json.dumps({**value, 'commandMenuShape': shape}))
                    with self.assertRaises(ValueError): q.semantic_observations(root, 'chatgpt-desktop')
                rejected = {**value, 'status': 'guard-rejected', 'titleReferenceCount': None,
                            'matchCount': None, 'sourceTitleIds': []}
                path.write_text(json.dumps(rejected))
                with self.assertRaises(ValueError): q.semantic_observations(root, 'chatgpt-desktop')
                path.write_text(json.dumps({**rejected, 'commandMenuShape': None}))
                self.assertIsNone(q.semantic_observations(root, 'chatgpt-desktop')[0]['commandMenuShape'])

class ClaudeFailureRowShapeTests(unittest.TestCase):
    def test_passive_counts_never_certify_retry_and_reject_private_payloads(self):
        keys = 'sourceRows streamingRows exactUserHeadings exactPromptNodes serverErrorLabels retryControls detailsControls userRows errorRows sharedParentPairs adjacentPairs assistantHeadingsInErrorRows duplicatePositions'.split()
        counts = dict(zip(keys, [2, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0]))
        shape = dict(sourceVersion='2.19675.0', sourceSha256='87e6b710a540352fcd4f9a1f0f6a8f9f9b6377ca676fd99c3e4d8bc87653dceb',
                     navigationSourceSha256='948270963cdf93cc411d95393157f5c7e2c06f18916d8c4ea1971828fb0c677c', phase='pre-disclosure', counts=counts)
        value = dict(schemaVersion=1, mechanism='claude-native-chat', diagnosticsOnly=True,
                     stage='scope-heading-ambiguous', submittedTurns=3, inputVerifiedTurns=3, copiedResponses=2,
                     retryAttempted=False, clipboardCleared=True, rowShape=shape)
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'rows.json'
            path.write_text(json.dumps(value))
            observed = q.semantic_observations(root, 'claude-desktop')[0]
            self.assertEqual(observed['rowShape'], shape)
            self.assertFalse(observed['retryAttempted'])
            for changed in ({**shape, 'label': 'PRIVATE'}, {**shape, 'sourceSha256': '0' * 64},
                            {**shape, 'counts': {**counts, 'sourceRows': 1025}},
                            {**shape, 'counts': {**counts, 'sourceRows': True}},
                            {**shape, 'counts': {**counts, 'adjacentPairs': 2}},
                            {**shape, 'counts': {**counts, 'userRows': 0}},
                            {**shape, 'counts': {**counts, 'position': 5}}, None):
                path.write_text(json.dumps({**value, 'rowShape': changed}))
                with self.assertRaises(ValueError): q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps({key: field for key, field in value.items() if key != 'rowShape'}))
            self.assertNotIn('rowShape', q.semantic_observations(root, 'claude-desktop')[0])

class ClaudeClassicRoleShapeTests(unittest.TestCase):
    def test_hidden_disabled_source_editor_is_advisory_and_privacy_closed(self):
        counts = dict(classicEditable=0, classicVisible=0, modernMessageEditable=0,
                      sendMessageVisible=0, sendMessageEnabled=0, startTaskVisible=0)
        shape = dict(status='observed', counts=dict(textArea=1, textField=0, editableTextArea=0, editableTextField=0))
        value = dict(schemaVersion=1, mechanism='claude-native-composer', diagnosticsOnly=True,
                     sourceVersion='2.9939.4', sourceCount=counts, classicRoleShape=shape,
                     classicSourceSha256='26f823bafc90cff4a749bfad6916ee69e4c3189f18b54a4e958ca387939c1181',
                     sendSourceSha256='d076b2f208fc5e572d0f3cd39aba35c6bacbe100a82db569851a0ce2317fa05c',
                     modernSourceSha256='5d1afc949ac69080ef6fe15491137ca0c3d2056991a9581537cba2bcc3724287')
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'facts.json'
            path.write_text(json.dumps(value))
            observed = q.semantic_observations(root, 'claude-desktop')[0]
            self.assertEqual(observed['classicRoleShape'], shape)
            self.assertEqual(observed['sourceCount']['classicEditable'], 0)
            unavailable = dict(status='unavailable', counts={key: None for key in shape['counts']})
            path.write_text(json.dumps({**value, 'classicRoleShape': unavailable}))
            self.assertEqual(q.semantic_observations(root, 'claude-desktop')[0]['classicRoleShape'], unavailable)
            for invalid in [None, {**shape, 'label': 'PRIVATE'},
                            dict(status='unavailable', counts=shape['counts']),
                            dict(status='observed', counts={**shape['counts'], 'textArea': True}),
                            dict(status='observed', counts={**shape['counts'], 'textArea': 4097}),
                            dict(status='observed', counts={**shape['counts'], 'editableTextArea': 2})]:
                path.write_text(json.dumps({**value, 'classicRoleShape': invalid}))
                with self.assertRaises(ValueError): q.semantic_observations(root, 'claude-desktop')
            legacy = dict(value); del legacy['classicRoleShape']
            path.write_text(json.dumps(legacy))
            self.assertNotIn('classicRoleShape', q.semantic_observations(root, 'claude-desktop')[0])



class ClaudeFailureScopeShapeTests(unittest.TestCase):
    def test_scope_is_advisory_closed_and_requires_legacy_row_receipt(self):
        keys = 'sourceRows streamingRows exactUserHeadings exactPromptNodes serverErrorLabels retryControls detailsControls userRows errorRows sharedParentPairs adjacentPairs assistantHeadingsInErrorRows duplicatePositions'.split()
        counts = dict(zip(keys, [2, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0]))
        shape = dict(sourceVersion='2.19675.0', sourceSha256='87e6b710a540352fcd4f9a1f0f6a8f9f9b6377ca676fd99c3e4d8bc87653dceb',
                     navigationSourceSha256='948270963cdf93cc411d95393157f5c7e2c06f18916d8c4ea1971828fb0c677c', phase='pre-disclosure', counts=counts)
        value = dict(schemaVersion=1, mechanism='claude-native-chat', diagnosticsOnly=True,
                     stage='scope-heading-ambiguous', submittedTurns=3, inputVerifiedTurns=3, copiedResponses=2,
                     retryAttempted=False, clipboardCleared=True, rowShape=shape)
        scope = dict(parentKind='web-area', walkEnd='boundary-web-area', groupAncestorCount=0,
                     sourceRowLabelsAnyRole=0, streamingLabelsAnyRole=0, tryAgainLabelsAnyRole=1,
                     tryAgainButtons=1, viewDetailsLabelsAnyRole=1, viewDetailsButtons=1)
        value['scopeShape'] = scope
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'scope.json'
            path.write_text(json.dumps(value))
            observed = q.semantic_observations(root, 'claude-desktop')[0]
            self.assertEqual(observed['scopeShape'], scope)
            self.assertFalse(observed['retryAttempted'])
            for changes in ({'parentKind': 'PRIVATE'}, {'parentKind': []}, {'walkEnd': 'PRIVATE'},
                            {'groupAncestorCount': 7}, {'sourceRowLabelsAnyRole': 1025},
                            {'tryAgainLabelsAnyRole': 0}, {'viewDetailsLabelsAnyRole': 0},
                            {'groupAncestorCount': True}, {'rawRole': 'PRIVATE'},
                            {'parentKind': 'none', 'walkEnd': 'depth-limit'}):
                path.write_text(json.dumps({**value, 'scopeShape': {**scope, **changes}}))
                with self.assertRaises(ValueError): q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps({key: val for key, val in value.items() if key != 'rowShape'}))
            with self.assertRaises(ValueError): q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps({key: val for key, val in value.items() if key != 'scopeShape'}))
            self.assertNotIn('scopeShape', q.semantic_observations(root, 'claude-desktop')[0])

class ClaudeConfigurationIoFailureTests(unittest.TestCase):
    def test_persist_io_failure_is_optional_typed_and_never_exports_error(self):
        value = dict(schemaVersion=1, mechanism='claude-cli-prelaunch', diagnosticsOnly=True,
                     phase='prelaunch', stage='configuration', status='failed',
                     configurationSubstage='persist', configurationDocument='normal-config')
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'claude-cli-prelaunch.json'
            for category in ('sharing-violation', 'access-denied', 'invalid-name', 'path-not-found',
                             'already-exists', 'invalid-input', 'other'):
                path.write_text(json.dumps({**value, 'configurationIoFailure': category}))
                self.assertEqual(q.semantic_observations(root, 'claude-desktop')[0]['configurationIoFailure'], category)
            for changes in ({'configurationIoFailure': 'PRIVATE'}, {'configurationIoFailure': 32},
                            {'configurationIoFailure': []}, {'configurationIoFailure': None},
                            {'configurationIoFailure': 'other', 'error': 'PRIVATE'},
                            {'configurationIoFailure': 'other', 'configurationSubstage': 'temporary-create'},
                            {'configurationIoFailure': 'other', 'stage': 'snapshot'}):
                path.write_text(json.dumps({**value, **changes}))
                with self.assertRaises(ValueError): q.semantic_observations(root, 'claude-desktop')
            path.write_text(json.dumps(value))
            self.assertNotIn('configurationIoFailure', q.semantic_observations(root, 'claude-desktop')[0])

class ClaudePersistAttributeTests(unittest.TestCase):
    def test_same_file_flags_are_advisory_nullable_and_payload_closed(self):
        value=dict(schemaVersion=1,mechanism='claude-cli-prelaunch',diagnosticsOnly=True,
                   phase='prelaunch',stage='configuration',status='failed',configurationSubstage='persist',
                   configurationDocument='normal-config',configurationIoFailure='sharing-violation')
        flags=dict(temporaryBefore=False,temporaryAfter=True,readonlyBefore=False,readonlyAfter=False)
        with tempfile.TemporaryDirectory() as root:
            path=Path(root)/'claude-cli-prelaunch.json'
            for attributes in (flags,dict.fromkeys(flags,None)):
                path.write_text(json.dumps({**value,'configurationFileAttributes':attributes}))
                self.assertEqual(q.semantic_observations(root,'claude-desktop')[0]['configurationFileAttributes'],attributes)
            for attributes in ([],{**flags,'temporaryBefore':1},{**flags,'path':'PRIVATE'},
                               {**flags,'temporaryBefore':None}):
                path.write_text(json.dumps({**value,'configurationFileAttributes':attributes}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')
            for change in ({'configurationIoFailure':'other'},{'configurationDocument':'profile'},
                           {'configurationSubstage':'temporary-write'}):
                path.write_text(json.dumps({**value,**change,'configurationFileAttributes':flags}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')

class ClaudeLinuxVisibilityTests(unittest.TestCase):
    def test_std_rename_boundaries_are_closed_and_failure_only(self):
        base=dict(schemaVersion=1,mechanism='claude-cli-prelaunch',phase='prelaunch',stage='configuration',
                  status='failed',diagnosticsOnly=True,configurationSubstage='persist',
                  configurationDocument='normal-config',configurationIoFailure='sharing-violation')
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); path=root/'claude-cli-prelaunch.json'
            for boundary in ('original-source','bridge-reader','retained-reader','destination-preflight',
                             'rename-dispatch','destination-identity','private-postcheck','deadline'):
                value={**base,'stdRenameSelected':True,'stdRenameBoundary':boundary}
                path.write_text(json.dumps(value))
                self.assertEqual(q.semantic_observations(root,'claude-desktop')[0],value)
            path.write_text(json.dumps({**base,'stdRenameSelected':False}))
            self.assertFalse(q.semantic_observations(root,'claude-desktop')[0]['stdRenameSelected'])
            for change in ({'stdRenameSelected':True},{'stdRenameSelected':[]},
                           {'stdRenameSelected':True,'stdRenameBoundary':'PRIVATE'},
                           {'stdRenameSelected':False,'stdRenameBoundary':'rename-dispatch'},
                           {'stdRenameSelected':True,'stdRenameBoundary':[]},
                           {'stdRenameSelected':True,'stdRenameBoundary':'rename-dispatch','rawPath':'PRIVATE'}):
                path.write_text(json.dumps({**base,**change}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')

    def test_closed_passive_states_and_failure_privacy(self):
        value = dict(schemaVersion=1, mechanism='claude-linux-classic-visibility', diagnosticsOnly=True,
                     status='complete', stage='complete', visible=False, showing=False, boundsPositive=True,
                     checkedAncestorCount=2, hiddenAncestorCount=1)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'facts.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(tmp, 'claude-desktop')[0], value)
            for change in ({'hiddenAncestorCount': 3}, {'checkedAncestorCount': True}, {'visible': None},
                           {'status': 'unavailable'}, {'bus': 'PRIVATE'}, {'stage': 'PRIVATE'}):
                path.write_text(json.dumps({**value, **change}))
                with self.assertRaises(ValueError): q.semantic_observations(tmp, 'claude-desktop')
            for status, stage in [('unavailable','deadline'), ('changed','identity'), ('limit','parent')]:
                failed = {**value, 'status':status, 'stage':stage,
                          **dict.fromkeys(('visible','showing','boundsPositive','checkedAncestorCount','hiddenAncestorCount'))}
                path.write_text(json.dumps(failed))
                self.assertEqual(q.semantic_observations(tmp, 'claude-desktop')[0], failed)
                with self.assertRaises(ValueError): q.semantic_observations(tmp, 'chatgpt-desktop')

class CodexFolderTrustTests(unittest.TestCase):
    def test_owned_folder_trust_receipt_is_closed_and_preserves_uncertainty(self):
        setup = dict(schemaVersion=1, mechanism='codex-public-onboarding', diagnosticsOnly=True,
                     stage='role-proof', errorCategory='action-blocked', conversationalScope=False,
                     engineeringControl=False, roleClickAttempted=False, roleClickCompleted=False,
                     engineeringChecked=False, continueControl=False, continueClickAttempted=False,
                     continueClickCompleted=False, roleScopeAbsent=False,
                     roleProofFailure='unmeasured', sessionProofFailure='unmeasured')
        for completed in (True,False):
            measured = {**setup,'stage':'coding-readiness','roleScopeAbsent':True,
                        'taskScopeProved':True,'taskControlKind':'skip-optional-capabilities',
                        'taskClickAttempted':True,'taskClickCompleted':True,'codingComposerReady':False,
                        'continueClickAttempted':True,'continueClickCompleted':True,
                        'roleClickAttempted':True,'roleClickCompleted':True,
                        'taskSkipConfirmationAttempted':True,'taskSkipConfirmationCompleted':completed}
            self.assertEqual(q.public_onboarding(measured,'chatgpt-desktop'),measured)
            for proof in ('overlay-count','form','retained-identity','source-controls','pointer-ancestry','heading','subtitle','matched'):
                observed = {**measured,'taskSkipConfirmationProof':proof}
                self.assertEqual(q.public_onboarding(observed,'chatgpt-desktop'),observed)
            for proof in ('PRIVATE',True,None):
                with self.assertRaises(ValueError):q.public_onboarding({**measured,'taskSkipConfirmationProof':proof},'chatgpt-desktop')
            for invalid in ({**measured,'taskSkipConfirmationAttempted':False},
                            {**measured,'taskSkipConfirmationCompleted':1},
                            {**measured,'taskControlKind':'get-started'},
                            {**measured,'taskClickCompleted':False},
                            {key:value for key,value in measured.items() if key!='taskSkipConfirmationCompleted'}):
                with self.assertRaises(ValueError):q.public_onboarding(invalid,'chatgpt-desktop')
        for status, attempted, completed in (('absent',False,False),('blocked',False,False),
                                             ('blocked',True,True),('completed',True,True),
                                             ('action-uncertain',True,False)):
            receipt = dict(status=status,clickAttempted=attempted,clickCompleted=completed)
            value = {**setup,'folderTrust':receipt}
            self.assertEqual(q.public_onboarding(value,'chatgpt-desktop'),value)
        for receipt in (dict(status='PRIVATE',clickAttempted=False,clickCompleted=False),
                        dict(status='completed',clickAttempted=False,clickCompleted=True),
                        dict(status='absent',clickAttempted=True,clickCompleted=False),
                        dict(status='action-uncertain',clickAttempted=True,clickCompleted=True),
                        dict(status='blocked',clickAttempted=1,clickCompleted=False),
                        dict(status='blocked',clickAttempted=False,clickCompleted=False,path='PRIVATE')):
            with self.assertRaises(ValueError):
                q.public_onboarding({**setup,'folderTrust':receipt},'chatgpt-desktop')
        for stage in ('authority','guard','deadline','dialog','form','title','path','controls','hit','identity','query'):
            receipt=dict(status='blocked',clickAttempted=False,clickCompleted=False,rejectionStage=stage)
            self.assertEqual(q.public_onboarding({**setup,'folderTrust':receipt},'chatgpt-desktop'),{**setup,'folderTrust':receipt})
        receipt = dict(status='blocked',clickAttempted=False,clickCompleted=False,rejectionStage='guard')
        for failure in ('deadline','native-ownership','page-set','main-identity','main-focus','main-scope',
                        'auxiliary-route','auxiliary-identity','auxiliary-focus','auxiliary-controls','query-failed','unmeasured'):
            measured = {**setup,'folderTrust':{**receipt,'guardFailure':failure}}
            self.assertEqual(q.public_onboarding(measured,'chatgpt-desktop'), measured)
        for changed in ({**receipt,'guardFailure':'PRIVATE'}, {**receipt,'guardFailure':True},
                        {**receipt,'guardFailure':'main-focus','rejectionStage':'path'},
                        {**receipt,'guardFailure':'main-focus','window':'PRIVATE'}):
            with self.assertRaises(ValueError):q.public_onboarding({**setup,'folderTrust':changed},'chatgpt-desktop')
        for stage in ('PRIVATE', [], {}, None):
            receipt=dict(status='blocked',clickAttempted=False,clickCompleted=False,rejectionStage=stage)
            with self.assertRaises(ValueError):q.public_onboarding({**setup,'folderTrust':receipt},'chatgpt-desktop')
        receipt=dict(status='completed',clickAttempted=True,clickCompleted=True,rejectionStage='path')
        with self.assertRaises(ValueError):q.public_onboarding({**setup,'folderTrust':receipt},'chatgpt-desktop')
        with self.assertRaises(ValueError):
            q.public_onboarding({**setup,'folderTrust':dict(status='absent',clickAttempted=False,clickCompleted=False)},'claude-desktop')

class ClaudeFailureAuthorityTests(unittest.TestCase):
    def test_failure_only_authority_is_closed_and_advisory(self):
        value=dict(schemaVersion=1,mechanism='claude-native-chat',diagnosticsOnly=True,
                   stage='copied',submittedTurns=2,inputVerifiedTurns=2,copiedResponses=2,
                   retryAttempted=False,clipboardCleared=True)
        authority=dict(status='context-unobserved',preparedTurns=2,learnedTurns=0,
                       rejectedStream=1,rejectedHistory=0,rejectedContext=0)
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);path=root/'authority.json'
            path.write_text(json.dumps({**value,'failureAuthority':authority}))
            self.assertEqual(q.semantic_observations(root,'claude-desktop'),[{**value,'failureAuthority':authority}])
            for extra in ({'status':'PRIVATE'}, {'learnedTurns':1}, {'rejectedStream':4097},
                          {'rejectedHistory':True}, {'context':'PRIVATE'}, {'preparedTurns':[]},
                          {'status':'prior-context-incomplete','learnedTurns':2}):
                path.write_text(json.dumps({**value,'failureAuthority':{**authority,**extra}}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')

class ClaudeArmedAuthorityTests(unittest.TestCase):
    def test_armed_negative_is_closed_and_never_claims_failure(self):
        value=dict(schemaVersion=1,mechanism='claude-native-chat',diagnosticsOnly=True,
                   stage='sent',submittedTurns=3,inputVerifiedTurns=3,copiedResponses=2,
                   retryAttempted=False,clipboardCleared=True,
                   providerObservation=dict(generationObserved=True,fixtureResponseVerified=True,failureObserved=False))
        authority=dict(status='armed-unobserved',preparedTurns=3,learnedTurns=2,
                       rejectedStream=1,rejectedHistory=1,rejectedContext=1)
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/'authority.json'
            path.write_text(json.dumps({**value,'failureAuthority':authority}))
            self.assertEqual(q.semantic_observations(tmp,'claude-desktop'),[{**value,'failureAuthority':authority}])
            for extra in ({'preparedTurns':2},{'learnedTurns':3},{'rejectedStream':4097},{'context':'PRIVATE'}):
                path.write_text(json.dumps({**value,'failureAuthority':{**authority,**extra}}))
                with self.assertRaises(ValueError):q.semantic_observations(tmp,'claude-desktop')
            for extra in ({'submittedTurns':2},{'retryAttempted':True},{'providerObservation':[]},
                          {'providerObservation':{**value['providerObservation'],'failureObserved':True}}):
                path.write_text(json.dumps({**value,**extra,'failureAuthority':authority}))
                with self.assertRaises(ValueError):q.semantic_observations(tmp,'claude-desktop')

class ClaudePersistOwnerTests(unittest.TestCase):
    def test_runner_forwarding_is_explicit_private_windows_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp).resolve();helper=root/'helper';helper.write_text('public fixture')
            source=dict(GITHUB_ACTIONS='true',RUNNER_ENVIRONMENT='github-hosted',RUNNER_OS='Windows',
                        NANH_DESKTOP_QUALIFICATION_MODE='startup-baseline',
                        NANH_CLAUDE_WINDOWS_PROFILE_POLICY='private-env',NANH_CLAUDE_PRELAUNCH_DIAGNOSTICS='1',
                        NANH_CLAUDE_PERSIST_OWNERS='1',FEASIBILITY_WINDOWS_PROOF_PYTHON=str(helper),
                        FEASIBILITY_WINDOWS_PROOF_SCRIPT=str(helper))
            with patch.object(runner,'validate_claude_windows_bundle'):
                environment=runner.qualification_environment('claude-desktop',root,helper,str(helper),source)
                self.assertEqual(environment['NANH_CLAUDE_PERSIST_OWNERS'],'1')
                self.assertNotIn('NANH_CLAUDE_PERSIST_CUTOFF_MS',environment)
                for changes in ({'NANH_CLAUDE_PERSIST_OWNERS':'other'},
                                {'NANH_CLAUDE_PRELAUNCH_DIAGNOSTICS':'0'},
                                {'NANH_CLAUDE_WINDOWS_PROFILE_POLICY':'other'},
                                {'NANH_DESKTOP_QUALIFICATION_MODE':'renderer'}):
                    with self.assertRaises(ValueError):runner.qualification_environment('claude-desktop',root,helper,str(helper),{**source,**changes})
            with self.assertRaises(ValueError):runner.qualification_environment('zed-desktop',root,helper,str(helper),source)

    def test_owner_counts_are_advisory_partition_and_payload_closed(self):
        value=dict(schemaVersion=1,mechanism='claude-config-persist-owners',diagnosticsOnly=True,
                   status='observed',stage='complete',destinationPresent=False,
                   ownerCount=2,currentProcessCount=1,otherProcessCount=1)
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);path=root/'owners.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root,'claude-desktop'),[value])
            for extra in ({'ownerCount':1},{'ownerCount':65},{'currentProcessCount':True},
                          {'stage':'PRIVATE'},{'path':'PRIVATE'},{'pid':123},{'destinationPresent':None}):
                path.write_text(json.dumps({**value,**extra}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')
            for category in ('available','sharing-denied','access-denied','missing','query-failed'):
                observed={**value,'sourceDeleteAccess':category}
                path.write_text(json.dumps(observed))
                self.assertEqual(q.semantic_observations(root,'claude-desktop'),[observed])
            for category in ('PRIVATE', [], None, True):
                path.write_text(json.dumps({**value,'sourceDeleteAccess':category}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')
            unavailable={**value,'status':'unavailable','stage':'query','destinationPresent':None,
                         'ownerCount':None,'currentProcessCount':None,'otherProcessCount':None}
            path.write_text(json.dumps(unavailable))
            self.assertEqual(q.semantic_observations(root,'claude-desktop'),[unavailable])
            path.write_text(json.dumps({**unavailable,'ownerCount':0}))
            with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')

            path.write_text(json.dumps({**unavailable,'sourceDeleteAccess':'available'}))
            with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')


class ClaudeLinuxNativeChatTests(unittest.TestCase):
    def test_closed_partial_receipt_does_not_claim_recovery(self):
        value=dict(schemaVersion=1,mechanism='claude-linux-native-chat',diagnosticsOnly=True,
                   stage='sent',submittedTurns=1,inputVerifiedTurns=1,copiedResponses=0,
                   retryAttempted=False,clipboardCleared=True)
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);path=root/'chat.json';path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root,'claude-desktop'),[value])
            for extra in ({'copiedResponses':2},{'inputVerifiedTurns':0},{'submittedTurns':True},
                          {'retryAttempted':True},{'path':'PRIVATE'},{'stage':'PRIVATE'}):
                path.write_text(json.dumps({**value,**extra}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):q.semantic_observations(root,'zed-desktop')

    def test_send_action_class_and_boundaries_are_closed(self):
        value=dict(schemaVersion=1,mechanism='claude-linux-native-chat',diagnosticsOnly=True,
                   stage='blocked',submittedTurns=0,inputVerifiedTurns=1,copiedResponses=0,
                   retryAttempted=False,clipboardCleared=True,failureBoundary='action-name')
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);path=root/'chat.json'
            for name in ('click','press','none','multiple','other'):
                path.write_text(json.dumps({**value,'sendActionClass':name}))
                self.assertEqual(q.semantic_observations(root,'claude-desktop')[0]['sendActionClass'],name)
            for extra in ({'sendActionClass':True},{'sendActionClass':'PRIVATE'},
                          {'sendActionClass':{'text':'PRIVATE'}},
                          {'sendActionClass':'press','inputVerifiedTurns':0},
                          {'failureBoundary':'PRIVATE'}):
                path.write_text(json.dumps({**value,**extra}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')

    def test_multi_action_observation_is_closed_and_cardinality_bound(self):
        action=dict(actionCount=3,activationMatchCount=1,selectedIndex=1,activationClass='press')
        value=dict(schemaVersion=1,mechanism='claude-linux-native-chat',diagnosticsOnly=True,
            stage='sent',submittedTurns=1,inputVerifiedTurns=1,copiedResponses=0,retryAttempted=False,
            clipboardCleared=True,sendActionClass='multiple',sendActionObservation=action)
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);path=root/'chat.json';path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root,'claude-desktop')[0]['sendActionObservation'],action)
            for change in ({'actionCount':True},{'actionCount':9},{'activationMatchCount':2},
                    {'selectedIndex':3},{'selectedIndex':True},{'activationClass':'PRIVATE'},
                    {'name':'PRIVATE'},{'actionCount':None}):
                path.write_text(json.dumps({**value,'sendActionObservation':{**action,**change}}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')
            for action in (dict(actionCount=2,activationMatchCount=0,selectedIndex=None,activationClass='none'),
                           dict(actionCount=2,activationMatchCount=2,selectedIndex=None,activationClass='ambiguous')):
                path.write_text(json.dumps({**value,'stage':'blocked','submittedTurns':0,'sendActionObservation':action}))
                self.assertEqual(q.semantic_observations(root,'claude-desktop')[0]['sendActionObservation'],action)

    def test_native_opt_in_only_accepts_owned_linux_trial(self):
        source=dict(GITHUB_ACTIONS='true',RUNNER_ENVIRONMENT='github-hosted',RUNNER_OS='Linux',
                    NANH_CLAUDE_LINUX_NATIVE_CHAT='first-turn',NANH_CLAUDE_LINUX_CHAT_ONLY='1')
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);helper=root/'helper';helper.write_text('fixture')
            env=runner.qualification_environment('claude-desktop',root,helper,str(helper),source)
            self.assertEqual(env['NANH_DESKTOP_QUALIFICATION_MODE'],'startup-baseline')
            self.assertEqual(env['NANH_CLAUDE_LINUX_NATIVE_CHAT'],'first-turn')
            for app,extra in [('zed-desktop',{}),('claude-desktop',{'RUNNER_OS':'macOS'}),
                              ('claude-desktop',{'NANH_CLAUDE_LINUX_NATIVE_CHAT':'other'})]:
                with self.assertRaises(ValueError):runner.qualification_environment(app,root,helper,str(helper),{**source,**extra})

class CodexEditableAncestryTests(unittest.TestCase):
    def test_partition_source_pins_and_privacy(self):
        observation=dict(status='observed',sourcePlatform='linux',sourceVersion='26.930.41038',
            initialSourceSha256='28c6096af241a37a9a33a2e5601f0aa05426910d5c84d08824d852342a2b4d5d',
            composerSourceSha256='7198ee078e78a748d03c3cc96f5c041d056584d762728fd0be23e695bc394da0',
            editableCount=1,codexHomeCount=0,codexThreadCount=0,codexOtherCount=0,
            classicChatGPTCount=0,genericInputCount=1,genericBodyCount=0,unboundCount=0,
            sidebarNewChatCount=1,sidebarNewChatHitActionable=True)
        setup=dict(schemaVersion=1,mechanism='codex-public-onboarding',diagnosticsOnly=True,
            stage='coding-readiness',errorCategory=None,conversationalScope=True,engineeringControl=True,
            roleClickAttempted=True,roleClickCompleted=True,engineeringChecked=True,continueControl=True,
            continueClickAttempted=True,continueClickCompleted=True,roleScopeAbsent=True,
            roleProofFailure='unmeasured',sessionProofFailure='unmeasured',taskScopeProved=True,
            taskClickAttempted=True,taskClickCompleted=True,codingEditableObservation=observation)
        self.assertEqual(q.public_onboarding(setup,'chatgpt-desktop'),setup)
        for change in ({'editableCount':True},{'editableCount':2},{'unboundCount':33},
                {'sidebarNewChatCount':2},{'sourcePlatform':'windows'},{'text':'PRIVATE'},
                {'genericInputCount':None},{'sidebarNewChatHitActionable':'PRIVATE'}, {'status':[]}):
            with self.assertRaises(ValueError):
                q.public_onboarding({**setup,'codingEditableObservation':{**observation,**change}},'chatgpt-desktop')
        overflow={key:(None if key.endswith('Count') or key=='sidebarNewChatHitActionable' else value)
                  for key,value in observation.items()}
        overflow['status']='overflow'
        self.assertEqual(q.public_onboarding({**setup,'codingEditableObservation':overflow},'chatgpt-desktop')['codingEditableObservation'],overflow)
        with self.assertRaises(ValueError):
            q.public_onboarding({**setup,'codingEditableObservation':{**overflow,'unboundCount':0}},'chatgpt-desktop')

class CodexHomeStateTests(unittest.TestCase):
    def test_pending_source_shape_is_closed_and_advisory(self):
        observation=dict(status='observed',sourcePlatform='linux',sourceVersion='26.930.41038',
            composerSourceSha256='7198ee078e78a748d03c3cc96f5c041d056584d762728fd0be23e695bc394da0',
            homeComposerCount=1,pendingTextareaCount=1,pendingGroupCount=1,proseMirrorEditableCount=0,
            enabledSendCount=0,disabledSendCount=1,workspaceControlCount=0)
        setup=dict(schemaVersion=1,mechanism='codex-public-onboarding',diagnosticsOnly=True,
            stage='coding-readiness',errorCategory=None,conversationalScope=True,engineeringControl=True,
            roleClickAttempted=True,roleClickCompleted=True,engineeringChecked=True,continueControl=True,
            continueClickAttempted=True,continueClickCompleted=True,roleScopeAbsent=True,
            roleProofFailure='unmeasured',sessionProofFailure='unmeasured',taskScopeProved=True,
            taskClickAttempted=True,taskClickCompleted=True,codingHomeStateObservation=observation)
        self.assertEqual(q.public_onboarding(setup,'chatgpt-desktop'),setup)
        for change in ({'homeComposerCount':0},{'enabledSendCount':True},{'disabledSendCount':33},
                {'sourceVersion':'PRIVATE'},{'text':'PRIVATE'},{'pendingTextareaCount':None},
                {'status':[]},{'status':{}}):
            with self.assertRaises(ValueError):q.public_onboarding({**setup,'codingHomeStateObservation':{**observation,**change}},'chatgpt-desktop')
        overflow={key:(None if key.endswith('Count') else value) for key,value in observation.items()}
        overflow['status']='overflow'
        self.assertEqual(q.public_onboarding({**setup,'codingHomeStateObservation':overflow},'chatgpt-desktop')['codingHomeStateObservation'],overflow)

class CodexPointObservationTests(unittest.TestCase):
    def test_measurement_is_closed_and_cannot_authorize_input(self):
        good=dict(reason='mapping-observed',mappingObserved=True,inputAuthorized=False,
            firstWebAreaCount=1,secondWebAreaCount=1,webAreaStable=True,nativeFocused=True,
            dimensionsMatched=True,nativePointClear=True,nativeHitWindowMatched=True,
            heldIdentityStable=True,webAreaUrlMatched=True)
        self.assertEqual(q.codex_point_observation(good),good)
        for cause in ['point-occluded','stack-unavailable','metadata-invalid','held-window-missing','held-window-changed']:
            value={**good,'reason':cause,'mappingObserved':False,'nativePointClear':False}
            self.assertEqual(q.codex_point_observation(value),value)
            for change in ({'nativePointClear':True},{'nativeFocused':False},
                           {'dimensionsMatched':False},{'heldIdentityStable':False},
                           {'ownerPid':1},{'ownerName':'PRIVATE'},{'bounds':[1,2,3,4]}):
                with self.assertRaises(ValueError):q.codex_point_observation({**value,**change})
            with self.assertRaises(ValueError):q.codex_point_observation(dict(reason=cause,mappingObserved=False,inputAuthorized=False))
        blocked=dict(reason='observation-unavailable',mappingObserved=False,inputAuthorized=False)
        self.assertEqual(q.codex_point_observation(blocked),blocked)
        for changes in ({'reason':[]},{'reason':'PRIVATE'},{'inputAuthorized':True},
                {'firstWebAreaCount':True},{'firstWebAreaCount':2},{'heldIdentityStable':False},
                {'webAreaUrlMatched':False},{'nativePointClear':False},{'bounds':'PRIVATE'},
                {'url':'PRIVATE'},{'mappingObserved':False}):
            with self.assertRaises(ValueError):q.codex_point_observation({**good,**changes})
        with self.assertRaises(ValueError):q.codex_point_observation({**blocked,'reason':'mapping-observed','mappingObserved':True})

class OwnedMoveFacts(unittest.TestCase):
    def test_closed_source_point_move_receipt_and_contradictions(self):
        native = dict(reason='moved-point-observed', candidateCount=1, inputAuthorized=False,
                      planMeasured=True, fullWorkareaBoundsBlocker=False, candidateFound=True,
                      moveAttempted=True, writeAcknowledged=True, sameIdentityTranslated=True,
                      nativePointClear=True, nativeHitWindowMatched=True, mappingStable=True,
                      nativeFocused=True)
        good = dict(reason='moved-source-point-observed', sourcePointRetained=True,
                    rendererReproved=True, postMappingObserved=True, inputAuthorized=False, native=native)
        self.assertEqual(q.codex_owned_move(good), good)
        for change in [{'inputAuthorized':True}, {'rawPath':'PRIVATE'}, {'sourcePointRetained':False},
                       {'rendererReproved':False}, {'native':None},
                       {'native':native|{'candidateCount':10}}, {'native':native|{'rawPID':100}},
                       {'native':native|{'nativeFocused':False}}, {'native':native|{'reason':'unknown'}}]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                q.codex_owned_move(good|change)
        denied = dict(reason='source-policy-rejected', sourcePointRetained=False,
                      rendererReproved=False, postMappingObserved=False, inputAuthorized=False)
        self.assertEqual(q.codex_owned_move(denied), denied)


class ClaudeLinuxOwnedInputTests(unittest.TestCase):
    def test_owned_text_diagnostic_is_closed_and_cannot_certify_submission(self):
        shape=dict(nodeCount=5,resolvedNodeCount=2,paragraphCount=1,rootChildCount=1,
            textLeafCount=3,otherRoleCount=0,objectLinkCount=1,completeTextCoverage=False,
            rootSingleParagraph=True,rootOnlyObjects=True,placeholderAttributeMatch=False,
            placeholderAttributeLfMatch=False,knownPromptMatchCount=0,latestPromptMatches=False)
        facts=dict(schemaVersion=1,mechanism='claude-linux-native-chat',diagnosticsOnly=True,
            stage='input-not-empty',submittedTurns=1,inputVerifiedTurns=1,copiedResponses=1,
            retryAttempted=False,clipboardCleared=True,
            inputShape=dict(charCount=17,onlyLineBreaks=False,onlyWhitespace=False,
                onlyZeroWidthMarkers=False,onlyObjectReplacement=False),
            embeddedTextObservation=dict(nodeCount=5,paragraphCount=1,literalLfLeafCount=1,
                brLfLeafCount=0,exactFillerLfLeafCount=0),ownedInputObservation=shape)
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);path=root/'closed.json'
            for relation in (shape,{**shape,'knownPromptMatchCount':1,'latestPromptMatches':True}):
                path.write_text(json.dumps({**facts,'ownedInputObservation':relation}))
                observed=q.semantic_observations(root,'claude-desktop')[0]
                self.assertEqual(observed['ownedInputObservation'],relation)
                self.assertEqual(observed['stage'],'input-not-empty')
            source=dict(paragraphTagPCount=1,paragraphEmptyClassPairCount=1,
                paragraphDataPlaceholderCount=1,unresolvedTextLeafCount=2,
                unresolvedOtherRoleCount=1,unresolvedEmptyTextCount=0,
                unresolvedLfTextCount=1,unresolvedExactResultCount=1,unresolvedOtherTextCount=0)
            path.write_text(json.dumps(facts|{'ownedInputObservation':shape|{'sourceShape':source}}))
            self.assertEqual(q.semantic_observations(root,'claude-desktop')[0]['ownedInputObservation']['sourceShape'],source)
            for changed in ({'rawValue':'PRIVATE'},{'unresolvedOtherRoleCount':0},
                {'unresolvedLfTextCount':2},{'paragraphTagPCount':2},{'unresolvedEmptyTextCount':True}):
                path.write_text(json.dumps(facts|{'ownedInputObservation':shape|{'sourceShape':source|changed}}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')
            for changed in ({**shape,'rawValue':'PRIVATE'},{**shape,'nodeCount':True},
                {**shape,'resolvedNodeCount':0},{**shape,'resolvedNodeCount':6},
                {**shape,'completeTextCoverage':True},{**shape,'objectLinkCount':5},
                {**shape,'latestPromptMatches':True},{**shape,'knownPromptMatchCount':2},
                {**shape,'paragraphCount':0}):
                path.write_text(json.dumps({**facts,'ownedInputObservation':changed}))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')
            for changed in ({**facts,'stage':'sent'},{**facts,'retryAttempted':True},
                {**facts,'submittedTurns':0,'inputVerifiedTurns':0,'copiedResponses':0},
                {key:value for key,value in facts.items() if key!='embeddedTextObservation'}):
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')

class ClaudeLinuxTransportCauseTests(unittest.TestCase):
    def test_closed_transport_causes_cannot_certify_submission(self):
        good=dict(schemaVersion=1,mechanism='claude-linux-native-chat',diagnosticsOnly=True,
            stage='blocked',submittedTurns=1,inputVerifiedTurns=1,copiedResponses=1,
            retryAttempted=False,clipboardCleared=True)
        with tempfile.TemporaryDirectory() as root:
            path=Path(root)/'transport.json'
            for cause in ['transport-spawn','transport-io','transport-wait','transport-status',
                'transport-size','transport-decode','transport-deadline','input-mapping-state',
                'input-mapping-changed','input-empty-state','input-empty-witness']:
                receipt=good|{'failureBoundary':cause};path.write_text(json.dumps(receipt))
                self.assertEqual(q.semantic_observations(root,'claude-desktop'),[receipt])
                for changed in [{'stage':'sent'},{'failureBoundary':[]},{'failureBoundary':'PRIVATE'},{'rawStderr':'PRIVATE'}]:
                    path.write_text(json.dumps(receipt|changed))
                    with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')

class CampaignDiagnosticTests(unittest.TestCase):
    def check_receipt(self, value, app, rejected):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'facts.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(root, app), [value])
            for change in rejected:
                path.write_text(json.dumps(value | change))
                with self.subTest(change=change), self.assertRaises(ValueError):
                    q.semantic_observations(root, app)

    def test_native_tree_failure_is_closed_and_never_claims_a_submission(self):
        tree = dict(operation='children', reason='query-unavailable', nodeScope='editor',
                    foreignBus=False, childCount=None, visitedCount=None)
        value = dict(schemaVersion=1, mechanism='claude-linux-native-chat', diagnosticsOnly=True,
                     stage='blocked', submittedTurns=1, inputVerifiedTurns=1, copiedResponses=1,
                     retryAttempted=False, clipboardCleared=True, nativeTreeObservation=tree)
        self.check_receipt(value, 'claude-desktop', [
            {'nativeTreeObservation': tree | {'rawObjectPath':'PRIVATE'}},
            {'nativeTreeObservation': tree | {'reason':'PRIVATE'}},
            {'nativeTreeObservation': tree | {'foreignBus':1}}, {'stage':'sent'}])

    def test_native_transport_reason_preserves_only_closed_error_names(self):
        tree = dict(operation='identity', reason='query-unavailable', nodeScope='other',
                    foreignBus=False, childCount=None, visitedCount=None, transportReason='object-unknown')
        value = dict(schemaVersion=1, mechanism='claude-linux-native-chat', diagnosticsOnly=True,
                     stage='blocked', submittedTurns=1, inputVerifiedTurns=1, copiedResponses=1,
                     retryAttempted=False, clipboardCleared=True, nativeTreeObservation=tree)
        self.check_receipt(value, 'claude-desktop', [
            {'nativeTreeObservation': tree | {'transportReason':'PRIVATE'}},
            {'nativeTreeObservation': tree | {'transportReason':None}},
            {'nativeTreeObservation': tree | {'reason':'wrong-owner'}}])

    def test_windows_prepare_receipt_cannot_export_paths_or_fake_completion(self):
        value = dict(schemaVersion=1, mechanism='codex-windows-profile-prepare', diagnosticsOnly=True,
                     stage='completed', cause=None, bindingIndex=None, ancestorCount=5, ownedCount=10,
                     privacy=['protected']*11, emptyRoots=[True,True], codeHomeAbsent=True, completed=True)
        self.check_receipt(value, 'chatgpt-desktop', [
            {'path':'PRIVATE'}, {'cause':'PRIVATE'}, {'ancestorCount':True},
            {'privacy':['inherited']*11}, {'emptyRoots':[False,True]}, {'ownedCount':9},
            {'bindingIndex':0}, {'stage':'command'}, {'codeHomeAbsent':False}])
        failed = value | dict(stage='command', cause='binding', bindingIndex=2, ancestorCount=0,
            ownedCount=0, privacy=[None]*11, emptyRoots=[None,None], codeHomeAbsent=None, completed=False)
        self.check_receipt(failed, 'chatgpt-desktop', [{'bindingIndex':8}, {'completed':True}])

    def test_linux_retry_receipt_requires_original_failed_turn_and_action(self):
        value = dict(schemaVersion=1, mechanism='claude-linux-native-chat', diagnosticsOnly=True,
                     stage='retry-forwarded', submittedTurns=3, inputVerifiedTurns=3,
                     copiedResponses=2, retryAttempted=True, clipboardCleared=True)
        self.check_receipt(value, 'claude-desktop', [
            {'submittedTurns':2}, {'copiedResponses':1}, {'retryAttempted':False},
            {'retryAttempted':1}, {'rawLabel':'PRIVATE'}])
        self.check_receipt(value | dict(stage='copied', copiedResponses=3), 'claude-desktop', [
            {'retryAttempted':False}, {'submittedTurns':2}])

    def test_retry_candidate_is_passive_and_bound_to_the_failed_third_turn(self):
        candidate = dict(pendingUserCount=1, historyMatched=True, conversationHeadingCount=5,
                         tryAgainCount=1, retryCount=0, candidateLabel='try-again',
                         candidateEnabled=True, candidateActionUnique=True, candidateHitMatched=True,
                         candidateUserAncestorMatched=True, candidateRowHeadingCount=2)
        value = dict(schemaVersion=1, mechanism='claude-linux-native-chat', diagnosticsOnly=True,
                     stage='retry-diagnostic', submittedTurns=3, inputVerifiedTurns=3, copiedResponses=2,
                     retryAttempted=False, clipboardCleared=True, retryCandidateObservation=candidate)
        self.check_receipt(value, 'claude-desktop', [
            {'retryCandidateObservation': candidate | {'rawLabel': 'PRIVATE'}},
            {'retryCandidateObservation': candidate | {'tryAgainCount': 2}},
            {'retryCandidateObservation': candidate | {'pendingUserCount': 0}},
            {'retryCandidateObservation': candidate | {'candidateHitMatched': 1}},
            {'retryAttempted': True}, {'submittedTurns': 2}, {'stage': 'copied'}])

    def test_empty_input_drift_is_closed_and_cannot_admit_a_turn(self):
        drift = dict(witnessPresent=True, rootSame=True, textSame=True, recordKeysSame=True,
                     stateSame=False, attributesSame=True, textRecordsSame=True,
                     otherValuesSame=True, focusOnlyStateChange=True, focusAttempted=True)
        value = dict(schemaVersion=1, mechanism='claude-linux-native-chat', diagnosticsOnly=True,
                     stage='blocked', submittedTurns=1, inputVerifiedTurns=1, copiedResponses=1,
                     retryAttempted=False, clipboardCleared=True,
                     failureBoundary='input-empty-witness', emptyInputDrift=drift)
        self.check_receipt(value, 'claude-desktop', [
            {'emptyInputDrift': drift | {'rawText': 'PRIVATE'}},
            {'emptyInputDrift': drift | {'stateSame': True}},
            {'emptyInputDrift': drift | {'focusAttempted': 1}},
            {'emptyInputDrift': drift | {'witnessPresent': False}},
            {'failureBoundary': 'transport-deadline'}, {'stage': 'sent'}])

    def test_accessible_retry_does_not_claim_a_cursor_match(self):
        selection = dict(status='accessible-hit', sampledPoints=1, exactPointerMatched=False,
                         accessibleHitVerified=True, guardBeforeVerified=1, guardAfterVerified=1,
                         accessibleChecks=1, accessibleExactMatches=1, cursorChecks=0,
                         cursorExactMatches=0, failureReason=None)
        value = dict(schemaVersion=1, mechanism='zed-pointer-observation', diagnosticsOnly=True,
                     maximizedHorizontal=True, maximizedVertical=True, enabled=True, sensitive=True,
                     showing=True, visible=True, defunct=False, retryContains=True,
                     pointerTarget='client', pointerChild='client', retryHitPolicy='accessibility',
                     cursorSelection=selection)
        self.check_receipt(value, 'zed-desktop', [
            {'retryHitPolicy': 'PRIVATE'},
            {'cursorSelection': selection | {'exactPointerMatched': True}},
            {'cursorSelection': selection | {'accessibleHitVerified': False}},
            {'cursorSelection': selection | {'failureReason': 'cursor-unmatched'}}])

    def test_bridge_receipt_preserves_the_actual_failure_boundary(self):
        bridge = dict(stage='endpoint-owner', failure='endpoint-rejected', privacy='protected',
                      endpointReason='listener-owner-mismatch')
        value = dict(schemaVersion=1, mechanism='claude-windows-profile-seal', diagnosticsOnly=True,
                     stage='bridge-authority', documentIndex=None, completed=False,
                     rootPrivacy='protected', libraryPrivacy=None, documentPrivacy=[None]*3,
                     configurationFailure=None, bridgeAuthority=bridge)
        self.check_receipt(value, 'claude-desktop', [
            {'bridgeAuthority': bridge | {'token': 'PRIVATE'}},
            {'bridgeAuthority': bridge | {'endpointReason': 'PRIVATE'}},
            {'bridgeAuthority': bridge | {'stage': 'completed'}},
            {'bridgeAuthority': bridge | {'privacy': True}}])

class ClaudeWindowsProfileSealTests(unittest.TestCase):
    def test_closed_stage_receipts_and_document_indices(self):
        documents={'document-metadata','document-open','document-privacy','document-lock','document-json'}
        stages=documents|{'initial-custody','native-policy','bridge-authority','library-metadata','library-lock',
            'configuration-values','final-custody','deadline','completed'}
        with tempfile.TemporaryDirectory() as root:
            path=Path(root)/'seal.json'
            for stage in stages:
                indices=range(3) if stage in documents else [None]
                for index in indices:
                    good=dict(schemaVersion=1,mechanism='claude-windows-profile-seal',diagnosticsOnly=True,
                        stage=stage,documentIndex=index,completed=stage=='completed')
                    path.write_text(json.dumps(good))
                    self.assertEqual(q.semantic_observations(root,'claude-desktop'),[good])
                    for changed in ({'stage':'private-path'},{'documentIndex':True},{'documentIndex':3},
                        {'completed':not good['completed']},{'diagnosticsOnly':False},{'rawValue':'PRIVATE'},
                        {'documentIndex':None if stage in documents else 0}):
                        path.write_text(json.dumps(good|changed))
                        with self.subTest(stage=stage,changed=changed), self.assertRaises(ValueError):
                            q.semantic_observations(root,'claude-desktop')
                    path.write_text(json.dumps(good))
                    with self.assertRaises(ValueError):q.semantic_observations(root,'zed-desktop')

class MacCodexHomeStateTests(unittest.TestCase):
    def test_passive_state_and_menu_never_admit_input(self):
        value=dict(status='observed',reason='menu-correlated',diagnosticsOnly=True,
            inputAuthorized=False,sendAuthorized=False,homeRetained=True,stateQueried=True,
            statePairStable=True,ordinaryLocalProjectObserved=True,selectedIdCorrelated=True,
            menuClickAttempted=True,menuClickCompleted=True)
        self.assertEqual(q.public_mac_codex_home_state(value),value)
        for change in [dict(inputAuthorized=True),dict(sendAuthorized=True),dict(rawText='PRIVATE'),
                dict(stateQueried=1),dict(homeRetained=False),dict(menuClickAttempted=False),
                dict(selectedIdCorrelated=False),dict(reason='unknown'),dict(statePairStable=False)]:
            with self.subTest(change=change),self.assertRaises(ValueError):
                q.public_mac_codex_home_state(value|change)
        passive=value|dict(reason='state-observed',selectedIdCorrelated=False,
            menuClickAttempted=False,menuClickCompleted=False)
        self.assertEqual(q.public_mac_codex_home_state(passive),passive)

class ClaudeWindowsImmutablePrivacyTests(unittest.TestCase):
    def test_configuration_mismatch_is_closed_and_never_success(self):
        good=dict(schemaVersion=1,mechanism='claude-windows-profile-seal',diagnosticsOnly=True,
            stage='configuration-values',documentIndex=None,completed=False,rootPrivacy='protected',
            libraryPrivacy='inherited',documentPrivacy=['inherited','protected','protected'])
        reasons=['document-count','deployment-mode','applied-profile','provider','base-url',
            'authentication-key','authentication-scheme','hybrid-pointer','profile-entries',
            'deployment-chooser','chat-only','alternate-configuration']
        with tempfile.TemporaryDirectory() as root:
            path=Path(root)/'seal.json'
            for reason in reasons:
                value=good|{'configurationFailure':reason};path.write_text(json.dumps(value))
                self.assertEqual(q.semantic_observations(root,'claude-desktop'),[value])
            for changed in [good|{'configurationFailure':'PRIVATE'},good|{'configurationFailure':{}},
                    good|{'configurationFailure':None},good|{'configurationFailure':'provider',
                        'stage':'completed','completed':True}]:
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')
    def test_exact_closed_descriptors_and_partial_failures(self):
        good=dict(schemaVersion=1,mechanism='claude-windows-profile-seal',diagnosticsOnly=True,
            stage='completed',documentIndex=None,completed=True,rootPrivacy='protected',
            libraryPrivacy='inherited',documentPrivacy=['inherited','protected','protected'])
        with tempfile.TemporaryDirectory() as root:
            path=Path(root)/'seal.json';path.write_text(json.dumps(good))
            self.assertEqual(q.semantic_observations(root,'claude-desktop'),[good])
            denied=good|dict(stage='document-privacy',documentIndex=0,completed=False,
                documentPrivacy=['unexpected',None,None])
            path.write_text(json.dumps(denied))
            self.assertEqual(q.semantic_observations(root,'claude-desktop'),[denied])
            for changed in [good|{'rootPrivacy':'inherited'},good|{'documentPrivacy':['protected']},
                good|{'documentPrivacy':['protected','protected',None]},good|{'libraryPrivacy':'unexpected'},
                good|{'libraryPrivacy':{}},good|{'documentPrivacy':['PRIVATE','protected','protected']},
                {k:v for k,v in good.items() if k!='rootPrivacy'},
                denied|{'documentPrivacy':['unexpected','protected',None]}]:
                path.write_text(json.dumps(changed))
                with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')

class WindowsCleanupProgressTests(unittest.TestCase):
    def test_closed_monotonic_stage_counts_never_export_identity(self):
        stages=['none','file-open','file-hash','file-identity','process-open','snapshot','targets','owner-recheck']
        with tempfile.TemporaryDirectory() as root:
            path=Path(root)/'progress.json'
            for count,stage in enumerate(stages):
                for outcome in ['deadline','transport','protocol','rejected','ready']:
                    good=dict(schemaVersion=1,mechanism='windows-owned-cleanup-preflight-progress',
                        diagnosticsOnly=True,completedStageCount=count,lastCompletedStage=stage,outcome=outcome)
                    path.write_text(json.dumps(good))
                    self.assertEqual(q.semantic_observations(root,'claude-desktop'),[good])
                    for change in [{'completedStageCount':True},{'completedStageCount':8},
                        {'lastCompletedStage':stages[(count+1)%8]},{'outcome':[]},
                        {'outcome':'PRIVATE'},{'rawPID':100},{'diagnosticsOnly':False}]:
                        path.write_text(json.dumps(good|change))
                        with self.assertRaises(ValueError):q.semantic_observations(root,'claude-desktop')
                    path.write_text(json.dumps(good))
                    with self.assertRaises(ValueError):q.semantic_observations(root,'zed-desktop')

if __name__ == '__main__':
    unittest.main()
