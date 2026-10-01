#!/usr/bin/env python3
"""Policy generation is exact and cleanup cannot remove an unrelated profile."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
spec = importlib.util.spec_from_file_location('namespace', Path(__file__).with_name('hermes-namespace.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class NamespacePolicy(unittest.TestCase):
    def test_exact_executable_without_global_or_suid_changes(self):
        text = module.profile_text(Path('/tmp/owned/hermes'), 'nanh-hermes-feasibility-' + 'a' * 32)
        self.assertIn('"/tmp/owned/hermes" flags=(unconfined)', text)
        self.assertIn('userns,', text)
        self.assertNotIn('*', text)
        for path in ['/tmp/a"/hermes', '/tmp/a\n/hermes', '/tmp/../hermes*']:
            with self.assertRaises(ValueError):
                module.profile_text(Path(path), 'nanh-hermes-feasibility-' + 'a' * 32)

    def test_cleanup_rejects_foreign_profile_and_unloads_before_removal(self):
        with tempfile.TemporaryDirectory() as root:
            state = Path(root) / 'state.json'
            state.write_text(json.dumps({'profilePath': '/etc/apparmor.d/unrelated', 'loaded': True}))
            with patch.object(module, 'run') as run:
                with self.assertRaises(ValueError): module.cleanup(state)
                run.assert_not_called()
            target = '/etc/apparmor.d/nanh-hermes-feasibility-' + 'a' * 32
            state.write_text(json.dumps({'profilePath': target, 'loaded': True}))
            with patch.object(module, 'run') as run:
                module.cleanup(state)
                self.assertEqual(run.call_args_list[0].args[0], ['sudo', 'apparmor_parser', '-R', target])
                self.assertEqual(run.call_args_list[1].args[0], ['sudo', 'rm', '-f', '--', target])
            self.assertFalse(state.exists())

if __name__ == '__main__': unittest.main()
