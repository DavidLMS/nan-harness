#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('qualification', Path(__file__).with_name('run-qualification.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class NativePolicyTests(unittest.TestCase):
    def test_native_policy_is_scoped_to_hosted_mac_startup(self):
        source = {'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted',
                  'RUNNER_OS': 'macOS', 'NANH_DESKTOP_QUALIFICATION_MODE': 'startup-baseline',
                  'NANH_CLAUDE_MAC_PROFILE_POLICY': 'native-known-folders',
                  'NANH_CLAUDE_MAC_CHAT_NAVIGATION': '1'}
        with patch.object(module, 'validate_claude_bundle') as validate:
            result = module.qualification_environment('claude-desktop', Path('/facts'), Path('/nanh'), Path('/Claude'), source)
            self.assertEqual(result['NANH_CLAUDE_MAC_PROFILE_POLICY'], 'native-known-folders')
            self.assertEqual(result['NANH_CLAUDE_MAC_CHAT_NAVIGATION'], '1')
            validate.assert_called_once()
            for changes in [{'NANH_CLAUDE_MAC_CHAT_NAVIGATION': 'yes'}, {'NANH_CLAUDE_MAC_PROFILE_POLICY': 'electron-user-data-dir'}]:
                with self.assertRaises(ValueError):
                    module.qualification_environment('claude-desktop', Path('/facts'), Path('/nanh'), Path('/Claude'), {**source, **changes})
            for key, value in [('RUNNER_OS', 'Windows'), ('GITHUB_ACTIONS', 'false'),
                               ('RUNNER_ENVIRONMENT', 'self-hosted'), ('NANH_DESKTOP_QUALIFICATION_MODE', 'renderer')]:
                invalid = {**source, key: value}
                with self.assertRaises(ValueError):
                    module.qualification_environment('claude-desktop', Path('/facts'), Path('/nanh'), Path('/Claude'), invalid)
            with self.assertRaises(ValueError):
                module.qualification_environment('chatgpt-desktop', Path('/facts'), Path('/nanh'), Path('/Claude'), source)

if __name__ == '__main__':
    unittest.main()
