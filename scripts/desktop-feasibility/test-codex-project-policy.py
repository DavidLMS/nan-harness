#!/usr/bin/env python3
"""The native project trial admits only the exact inspected hosted Windows release."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('qualification', Path(__file__).with_name('run-qualification.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ProjectPolicy(unittest.TestCase):
    def test_exact_frozen_release_and_application_are_both_required(self):
        release = dict(version='26.930.31730', digest='sha256:f7b0266d6c00d4743da01d62bc82488f7ec5560c642501758119cb9885f67c87')
        executable = 'b35bf062c01d73da090c60e62186dc180c2a8545cb6fc9575b4403c8fa3db49e'
        module.validate_codex_project_release(release, executable)
        for changed, binary in (({**release, 'version': '26.930.2377.0'}, executable),
                                ({**release, 'digest': 'sha256:' + '0' * 64}, executable),
                                (release, '0' * 64), ({}, executable)):
            with self.assertRaises(ValueError):
                module.validate_codex_project_release(changed, binary)

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
            plain = {key: value for key, value in source.items() if key != 'NANH_CODEX_PROJECT_POLICY'}
            self.assertNotIn('NANH_CODEX_PROJECT_POLICY', module.qualification_environment('chatgpt-desktop', root, helper, helper, plain))
            for app, changed in [('hermes-desktop', source),
                                 ('chatgpt-desktop', {**source, 'RUNNER_OS': 'Linux'}),
                                 ('chatgpt-desktop', {**source, 'NANH_CODEX_PROJECT_POLICY': 'unknown'}),
                                 ('chatgpt-desktop', {**source, 'NANH_DESKTOP_QUALIFICATION_MODE': 'startup-baseline'}),
                                 ('chatgpt-desktop', {key: value for key, value in source.items() if key != 'NANH_CODEX_PUBLIC_ONBOARDING'})]:
                with self.assertRaises(ValueError):
                    module.qualification_environment(app, root, helper, helper, changed)


if __name__ == '__main__':
    unittest.main()
