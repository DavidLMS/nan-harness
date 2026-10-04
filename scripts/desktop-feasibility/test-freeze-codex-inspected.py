#!/usr/bin/env python3
"""The inspected baseline is fixed and corrupt downloads cannot be installed."""
import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('freeze_codex', Path(__file__).with_name('freeze-codex-inspected.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class InspectedCodex(unittest.TestCase):
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

    def test_staging_requires_exact_bytes_and_never_reuses_existing_file(self):
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
            with self.assertRaises(FileExistsError):
                module.stage(entry, root, fetch)
            self.assertEqual(destination.read_bytes(), content)
            self.assertEqual(list(root.iterdir()), [destination])

    def test_mismatch_cleans_partial_and_publishes_nothing(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / 'artifacts'
            with self.assertRaisesRegex(ValueError, 'artifact-mismatch'):
                module.stage(module.manifest('macos')['apps'][0], root,
                             lambda _url, path: path.write_bytes(b'new upstream bytes'))
            self.assertEqual(list(root.iterdir()), [])


if __name__ == '__main__':
    unittest.main()
