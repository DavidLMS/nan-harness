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

class LinuxMcpPolicyTests(unittest.TestCase):
    def test_linux_fixture_is_distinct_and_exactly_scoped(self):
        source = {'GITHUB_ACTIONS':'true','RUNNER_ENVIRONMENT':'github-hosted','RUNNER_OS':'Linux',
                  'NANH_DESKTOP_QUALIFICATION_MODE':'startup-baseline',
                  'NANH_CLAUDE_LINUX_CHAT_ONLY':'1','NANH_CLAUDE_LINUX_MCP_FIXTURE':'read-only'}
        expected = 'ecb56f97d549f3040908f1bb8f0bb32235f9b48d9572ea348098135fe7999fc0'
        with patch.object(module, 'digest', return_value=expected):
            result = module.qualification_environment('claude-desktop',Path('/facts'),Path('/nanh'),Path('/Claude'),source)
            self.assertEqual(result['NANH_CLAUDE_LINUX_MCP_FIXTURE'],'read-only')
            self.assertNotIn('NANH_CLAUDE_MCP_FIXTURE',result)
            self.assertEqual(result['NANH_CLAUDE_MCP_SOURCE_SHA256'],expected)
            for change in [{'RUNNER_OS':'macOS'},{'NANH_CLAUDE_LINUX_CHAT_ONLY':'0'},
                           {'NANH_DESKTOP_QUALIFICATION_MODE':'renderer'},
                           {'NANH_CLAUDE_LINUX_MCP_FIXTURE':'other'},
                           {'NANH_CLAUDE_MCP_FIXTURE':'read-only'}, {'GITHUB_ACTIONS':'false'}]:
                with self.subTest(change=change), self.assertRaises(ValueError):
                    module.qualification_environment('claude-desktop',Path('/facts'),Path('/nanh'),Path('/Claude'),{**source,**change})
            with self.assertRaises(ValueError):
                module.qualification_environment('zed-desktop',Path('/facts'),Path('/nanh'),Path('/Claude'),source)
        with patch.object(module,'digest',return_value='different source'), self.assertRaises(ValueError):
            module.qualification_environment('claude-desktop',Path('/facts'),Path('/nanh'),Path('/Claude'),source)

if __name__ == '__main__':
    unittest.main()
