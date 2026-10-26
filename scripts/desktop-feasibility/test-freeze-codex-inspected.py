#!/usr/bin/env python3
"""The inspected baseline is fixed and corrupt downloads cannot be installed."""
import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import subprocess

spec = importlib.util.spec_from_file_location('freeze_codex', Path(__file__).with_name('freeze-inspected.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class InspectedCodex(unittest.TestCase):
    def test_windows_versioned_source_rejects_changed_identity_or_source(self):
        value = module.manifest('windows')
        entry = value['apps'][0]
        changes = [dict(version='26.1007.2314'), dict(digest='sha256:' + 'a' * 64),
                   dict(url=entry['url'].replace('26.1007.2314.0', '26.1021.3705.0')),
                   dict(url=entry['url'].replace('oaistatic.com', 'oaistatic.com.attacker.example')),
                   dict(url=entry['url'] + '?download=1'), dict(url=entry['url'] + '#fragment')]
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'manifest.json'
            for change in changes:
                module.write_json(path, {**value, 'apps': [{**entry, **change}]})
                with self.assertRaises(ValueError):
                    module.read_frozen_manifest(path, ['chatgpt-desktop'], 'windows', 'x86_64', 'qwen3.6')

    def test_windows_stages_inspected_release_when_mutable_latest_has_changed(self):
        original = b'original inspected package'
        entry = {**module.manifest('windows')['apps'][0],
                 'digest': 'sha256:' + hashlib.sha256(original).hexdigest()}
        fetched = []
        def fetch(url, path):
            fetched.append(url)
            path.write_bytes(original if '/releases/26.1007.2314.0/' in url else b'new upstream package')
        with tempfile.TemporaryDirectory() as temporary:
            staged = module.stage(entry, Path(temporary) / 'artifacts', fetch)
            self.assertEqual(staged.read_bytes(), original)
            self.assertEqual(fetched, ['https://persistent.oaistatic.com/codex-app-prod/releases/26.1007.2314.0/ChatGPT-x64.msix'])

    def test_download_failures_are_closed_without_urls_or_process_output(self):
        for status, code in {22: 'http', 23: 'local-io', 28: 'timeout',
                             63: 'size-limit', 99: 'unavailable'}.items():
            error = subprocess.CalledProcessError(status, ['curl', 'PRIVATE'], stderr='PRIVATE')
            with patch.object(module.subprocess, 'run', side_effect=error):
                with self.assertRaises(ValueError) as caught:
                    module.download('https://private.invalid/value', Path('private-output'))
            self.assertEqual(module.failure_code(caught.exception), 'inspected-desktop-download-' + code)
        with patch.object(module.subprocess, 'run', side_effect=subprocess.TimeoutExpired('PRIVATE', 190)):
            with self.assertRaises(ValueError) as caught:
                module.download('https://private.invalid/value', Path('private-output'))
        self.assertEqual(module.failure_code(caught.exception), 'inspected-desktop-download-timeout')
        self.assertEqual(module.failure_code(ValueError('PRIVATE')), 'inspected-desktop-freeze-manifest')
        self.assertEqual(module.failure_code(OSError('PRIVATE')), 'inspected-desktop-freeze-local-io')

    def test_all_platform_manifests_match_existing_install_contract(self):
        for platform in ('linux', 'macos', 'windows'):
            value = module.manifest(platform)
            with tempfile.TemporaryDirectory() as temporary:
                path = Path(temporary) / 'manifest.json'
                module.write_json(path, value)
                self.assertEqual(module.read_frozen_manifest(path, ['chatgpt-desktop'], platform,
                                                            value['architecture'], 'qwen3.6'), value)
            entry = value['apps'][0]
            self.assertEqual(entry['version'], module.CODEX_PROJECT_VERSIONS[platform])
            self.assertEqual(entry['digest'], 'sha256:' + module.CODEX_PROJECT_RELEASES[platform][0])

    def test_claude_manifests_use_the_admitted_immutable_releases(self):
        runner = Path(__file__).with_name('run-qualification.py').read_text()
        for platform in ('linux', 'macos', 'windows'):
            value = module.claude_manifest(platform)
            with tempfile.TemporaryDirectory() as temporary:
                path = Path(temporary) / 'manifest.json'
                module.write_json(path, value)
                self.assertEqual(module.read_frozen_manifest(path, ['claude-desktop'], platform,
                                                            value['architecture'], 'qwen3.6'), value)
            self.assertIn(value['apps'][0]['version'], runner)
            self.assertIn(value['apps'][0]['digest'], runner)
            self.assertIn(value['apps'][0]['version'], value['apps'][0]['url'])

    def test_staging_reuses_only_verified_cached_bytes_without_fetching(self):
        content = b'synthetic inspected package'
        digest = hashlib.sha256(content).hexdigest()
        entry = {**module.manifest('macos')['apps'][0], 'digest': 'sha256:' + digest}
        def fetch(_url, path):
            path.write_bytes(content)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / 'artifacts'
            destination = module.stage(entry, root, fetch)
            self.assertEqual(destination.name, 'chatgpt-desktop-' + digest)
            self.assertEqual(destination.read_bytes(), content)
            self.assertEqual(module.stage(entry, root, lambda *_: self.fail('unexpected download')),
                             destination)
            self.assertEqual(destination.read_bytes(), content)
            self.assertEqual(list(root.iterdir()), [destination])
            destination.write_bytes(b'corrupt cache')
            with self.assertRaisesRegex(ValueError, 'artifact-mismatch'):
                module.stage(entry, root, lambda *_: self.fail('unexpected download'))

    def test_staging_rejects_symlink_cache(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            entry = module.manifest('macos')['apps'][0]
            destination = root / (entry['app'] + '-' + entry['digest'].removeprefix('sha256:'))
            destination.symlink_to(root / 'missing')
            with self.assertRaisesRegex(ValueError, 'cache-invalid'):
                module.stage(entry, root, lambda *_: self.fail('unexpected download'))

    def test_mismatch_cleans_partial_and_publishes_nothing(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / 'artifacts'
            with self.assertRaisesRegex(ValueError, 'artifact-mismatch'):
                module.stage(module.manifest('macos')['apps'][0], root,
                             lambda _url, path: path.write_bytes(b'new upstream bytes'))
            self.assertEqual(list(root.iterdir()), [])


if __name__ == '__main__':
    unittest.main()
