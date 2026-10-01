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
    def test_closed_environment_excludes_credentials_and_experiment_opt_ins(self):
        source = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Linux',
                      PATH='/synthetic/bin', NAN_API_KEY='PRIVATE', AWS_SECRET_ACCESS_KEY='PRIVATE',
                      GITHUB_TOKEN='PRIVATE', FEASIBILITY_HERMES_DOM_FACTS='/old-experiment',
                      FEASIBILITY_HERMES_STARTUP_ONLY='1', FEASIBILITY_ZED_NATIVE_COPY_FACTS='/old',
                      HERMES_DESKTOP_HERMES='/frozen/venv/bin/hermes')
        env = runner.qualification_environment('hermes-desktop', Path('/facts'), Path('/nanh'), '/app', source)
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
    def test_exact_initial_matrix_and_two_explicit_backends(self):
        cells = q.matrix()['include']
        self.assertEqual(len(cells), 15)
        self.assertEqual(len({(c['app'], c['platform'], c['architecture']) for c in cells}), 15)
        self.assertEqual(sum(c['backend'] != 'pending' for c in cells), 2)
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
                          retryControlCount=0, retrySelector='retry-name-or-description',
                          input={'submitted': True, 'clipboardVerified': True, 'private': 'PRIVATE'},
                          response={'clipboardVerified': False, 'providerVerified': True})
            path.write_text(json.dumps(native))
            public = q.semantic_observations(root, 'zed-desktop')
            self.assertEqual(public[0]['substage'], 'retry-control-query')
            self.assertTrue(public[0]['inputSubmitted'])
            self.assertEqual(public[0]['retryControlCount'], 0)
            self.assertEqual(public[0]['retrySelector'], 'retry-name-or-description')
            for field, invalid in [('retryControlCount', True), ('retryControlCount', 4097),
                                   ('retrySelector', 'PRIVATE_SYNTHETIC')]:
                path.write_text(json.dumps({**native, field: invalid}))
                with self.assertRaises(ValueError):
                    q.semantic_observations(root, 'zed-desktop')
            path.write_text(json.dumps(native))
            self.assertNotIn('PRIVATE', str(public))
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

    def trial(self, mutate=lambda report: None):
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
                 patch.object(q, 'validated_report', return_value=(report, 'd' * 64)):
                return q.reduce_report(app='hermes-desktop', platform='linux', architecture='x86_64',
                                       source_sha='a' * 40, model='qwen3.6', **paths)

    def test_full_three_probes_and_cleanup_are_required(self):
        self.assertEqual(self.trial()['qualification'], 'deterministic-full')
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


if __name__ == '__main__':
    unittest.main()
