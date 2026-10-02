#!/usr/bin/env python3
"""Matrix completeness and fail-closed full-acceptance reducer contracts."""
import copy
import importlib.util
import argparse
from pathlib import Path
import sys
import json
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'canary/actions'))
import desktop_qualification as q


spec = importlib.util.spec_from_file_location('qualification_runner', ROOT / 'scripts/desktop-feasibility/run-qualification.py')
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class RunnerTests(unittest.TestCase):
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

    def test_public_onboarding_opt_in_is_hosted_windows_codex_renderer_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            helper = root / 'helper'
            helper.write_text('synthetic helper')
            source = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Windows',
                          FEASIBILITY_WINDOWS_PROOF_PYTHON=str(helper), FEASIBILITY_WINDOWS_PROOF_SCRIPT=str(helper),
                          NANH_CODEX_PUBLIC_ONBOARDING='engineering')
            env = runner.qualification_environment('chatgpt-desktop', root, helper, str(helper), source)
            self.assertEqual(env['NANH_CODEX_PUBLIC_ONBOARDING'], 'engineering')
            for changed, app in (({'NANH_CODEX_PUBLIC_ONBOARDING': 'PRIVATE'}, 'chatgpt-desktop'),
                                 ({'RUNNER_OS': 'Linux'}, 'chatgpt-desktop'),
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

    def test_exact_initial_matrix_separates_scenario_backends_and_inventories(self):
        cells = q.matrix()['include']
        self.assertEqual(len(cells), 15)
        self.assertEqual(len({(c['app'], c['platform'], c['architecture']) for c in cells}), 15)
        self.assertEqual(sum(c['backend'] != 'renderer-inventory' for c in cells), 6)
        self.assertEqual({(c['platform'], c['architecture']) for c in cells},
                         {('linux', 'x86_64'), ('macos', 'aarch64'), ('windows', 'x86_64')})
        with self.assertRaises(ValueError):
            q.envelope('zed-desktop', 'macos', 'x86_64', 'a' * 40)

    def test_pending_is_explicit_unqualified_and_never_publishes_payloads(self):
        value = q.envelope('pen-desktop', 'windows', 'x86_64', 'a' * 40)
        self.assertEqual(value['outcome'], 'blocked')
        self.assertEqual(value['qualification'], 'unqualified')
        self.assertIsNone(value['applicationSha256'])
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

    def test_public_onboarding_receipts_reject_private_and_untyped_data(self):
        setup = dict(schemaVersion=1, mechanism='codex-public-onboarding', diagnosticsOnly=True,
                     stage='stopped-after-role', errorCategory=None, conversationalScope=True,
                     engineeringControl=True, roleClickAttempted=True, roleClickCompleted=True,
                     engineeringChecked=True, continueControl=True, continueClickAttempted=True,
                     continueClickCompleted=True, roleScopeAbsent=True)
        self.assertEqual(q.public_onboarding(setup, 'chatgpt-desktop'), setup)
        for changed in ({**setup, 'label': 'PRIVATE'}, {**setup, 'stage': 'PRIVATE'},
                        {**setup, 'errorCategory': []}, {**setup, 'engineeringChecked': 1},
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

if __name__ == '__main__':
    unittest.main()
