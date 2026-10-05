#!/usr/bin/env python3
"""The native project trial admits only exact inspected hosted platform releases."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('qualification', Path(__file__).with_name('run-qualification.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ProjectPolicy(unittest.TestCase):
    def test_exact_frozen_release_and_application_are_both_required(self):
        targets = {
            'macos': ('f6cf4d2e9b69aeefa33adda4bcd1a2d306357f5253a1ac6049700870c28dd0c7',
                      '418a460276b195f5642e43b320ec2821d6c34c646cb316ed2c0285546298243f'),
            'windows': ('e03019134d729c6416173b0712aa5c51d079966253f77077f4bf105d29d8fce7',
                        '784300980f00a4ebd3bd978fb01c99b6da72bd4cab4d4bfdfc7085db4c60fe74'),
            'linux': ('ee7854145554718d7239d01ea37d44f6ba1e0ba4a93f47ac097d6e0f964da47c',
                      '207c4fbff7e2fcc1b0789448351ac6eed206206d94c5a0835e5f07c7cd73d6e3'),
        }
        for platform, (artifact, executable) in targets.items():
            release = dict(version='26.930.41038', digest='sha256:' + artifact)
            module.validate_codex_project_release(release, executable, platform)
            foreign = targets['linux' if platform == 'windows' else 'windows']
            for changed, binary in (({**release, 'version': '26.930.2377.0'}, executable),
                                    ({**release, 'version': '26.930.31730'}, executable),
                                    ({**release, 'digest': 'sha256:' + '0' * 64}, executable),
                                    ({**release, 'digest': 'sha256:' + foreign[0]}, executable),
                                    (release, foreign[1]), (release, '0' * 64), ({}, executable)):
                with self.assertRaises(ValueError):
                    module.validate_codex_project_release(changed, binary, platform)
            for unsupported in ('freebsd', 'unknown', ''):
                with self.assertRaises(ValueError):
                    module.validate_codex_project_release(release, executable, unsupported)

    def test_policy_is_opt_in_and_rejects_other_apps_platforms_or_modes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            helper = root / 'owned-helper'
            helper.write_bytes(b'synthetic')
            source = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Windows',
                          FEASIBILITY_WINDOWS_PROOF_PYTHON=str(helper), FEASIBILITY_WINDOWS_PROOF_SCRIPT=str(helper),
                          NANH_CODEX_PUBLIC_ONBOARDING='engineering', NANH_CODEX_PROJECT_POLICY='open-project')
            result = module.qualification_environment('chatgpt-desktop', root, helper, helper, source)
            self.assertEqual(result['NANH_CODEX_PROJECT_POLICY'], 'open-project')
            self.assertNotIn('NANH_CODEX_PROJECT_ARTIFACT_SHA256', result)
            linux = {**source, 'RUNNER_OS': 'Linux'}
            linux_result = module.qualification_environment('chatgpt-desktop', root, helper, helper, linux)
            self.assertEqual(linux_result['NANH_CODEX_PROJECT_POLICY'], 'open-project')
            self.assertNotIn('NANH_CODEX_PROJECT_ARTIFACT_SHA256', linux_result)
            plain = {key: value for key, value in source.items() if key != 'NANH_CODEX_PROJECT_POLICY'}
            self.assertNotIn('NANH_CODEX_PROJECT_POLICY', module.qualification_environment('chatgpt-desktop', root, helper, helper, plain))
            for app, changed in [('hermes-desktop', source),
                                 ('chatgpt-desktop', {**source, 'RUNNER_OS': 'FreeBSD'}),
                                 ('chatgpt-desktop', {**linux, 'GITHUB_ACTIONS': 'false'}),
                                 ('chatgpt-desktop', {**linux, 'RUNNER_ENVIRONMENT': 'self-hosted'}),
                                 ('chatgpt-desktop', {**source, 'NANH_CODEX_PROJECT_POLICY': 'unknown'}),
                                 ('chatgpt-desktop', {**source, 'NANH_DESKTOP_QUALIFICATION_MODE': 'startup-baseline'}),
                                 ('chatgpt-desktop', {key: value for key, value in source.items() if key != 'NANH_CODEX_PUBLIC_ONBOARDING'})]:
                with self.assertRaises(ValueError):
                    module.qualification_environment(app, root, helper, helper, changed)


if __name__ == '__main__':
    unittest.main()
