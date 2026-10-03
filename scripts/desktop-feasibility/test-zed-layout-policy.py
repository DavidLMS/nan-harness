#!/usr/bin/env python3
"""The layout trial is opt-in, private and restricted to hosted Linux Zed."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('runner', Path(__file__).with_name('run-qualification.py'))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class Policy(unittest.TestCase):
    def test_layout_policy_is_forwarded_only_in_the_owned_linux_cell(self):
        source = {key: 'synthetic' for key in runner.ZED_HELPERS}
        source.update(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Linux',
                      FEASIBILITY_ZED_INPUT_DRIVER_MODE='paste', FEASIBILITY_ZED_RESPONSE_METHOD='thread-export')
        def environment(app='zed-desktop', values=None):
            return runner.qualification_environment(app, Path('/facts'), Path('/nanh'), '/zed', values or source)
        self.assertNotIn('NANH_ZED_LAYOUT_POLICY', environment())
        source['NANH_ZED_LAYOUT_POLICY'] = 'zoom-before-send'
        self.assertEqual(environment()['NANH_ZED_LAYOUT_POLICY'], 'zoom-before-send')
        for changes in ({'RUNNER_OS': 'Windows'}, {'RUNNER_OS': 'macOS'},
                        {'GITHUB_ACTIONS': 'false'}, {'RUNNER_ENVIRONMENT': 'self-hosted'},
                        {'NANH_ZED_LAYOUT_POLICY': 'force'}):
            with self.assertRaises(ValueError):
                environment(values={**source, **changes})
        with self.assertRaises(ValueError):
            environment(app='hermes-desktop')

    def test_cursor_trial_is_forwarded_only_to_hosted_linux_zed(self):
        source = {key: 'synthetic' for key in runner.ZED_HELPERS}
        source.update(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Linux',
                      FEASIBILITY_ZED_INPUT_DRIVER_MODE='paste', FEASIBILITY_ZED_RESPONSE_METHOD='thread-export',
                      NANH_ZED_CURSOR_HIT='1')
        args = (Path('/facts'), Path('/nanh'), '/zed')
        self.assertEqual(runner.qualification_environment('zed-desktop', *args, source)['NANH_ZED_CURSOR_HIT'], '1')
        for changes in ({'RUNNER_OS': 'macOS'}, {'RUNNER_OS': 'Windows'},
                        {'NANH_ZED_CURSOR_HIT': 'force'}, {'RUNNER_ENVIRONMENT': 'self-hosted'}):
            with self.assertRaises(ValueError):
                runner.qualification_environment('zed-desktop', *args, {**source, **changes})
        with self.assertRaises(ValueError):
            runner.qualification_environment('hermes-desktop', *args, source)

    def test_native_retry_trial_is_explicit_and_hosted_linux_only(self):
        source = {key: 'synthetic' for key in runner.ZED_HELPERS}
        source.update(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Linux',
                      FEASIBILITY_ZED_INPUT_DRIVER_MODE='paste', FEASIBILITY_ZED_RESPONSE_METHOD='thread-export')
        args = (Path('/facts'), Path('/nanh'), '/zed')
        self.assertNotIn('NANH_ZED_RETRY_METHOD', runner.qualification_environment('zed-desktop', *args, source))
        source['NANH_ZED_RETRY_METHOD'] = 'atspi-click'
        self.assertEqual(runner.qualification_environment('zed-desktop', *args, source)['NANH_ZED_RETRY_METHOD'], 'atspi-click')
        for changes in ({'RUNNER_OS': 'macOS'}, {'RUNNER_OS': 'Windows'},
                        {'NANH_ZED_RETRY_METHOD': 'force'}, {'RUNNER_ENVIRONMENT': 'self-hosted'}):
            with self.assertRaises(ValueError):
                runner.qualification_environment('zed-desktop', *args, {**source, **changes})
        with self.assertRaises(ValueError):
            runner.qualification_environment('hermes-desktop', *args, source)


if __name__ == '__main__':
    unittest.main()
