#!/usr/bin/env python3
"""Official metadata identity and channel checks remain fail closed."""
import base64
import copy
import importlib.util
import json
from pathlib import Path
import unittest
import sys
import tempfile
import subprocess
import textwrap

spec = importlib.util.spec_from_file_location('freeze', Path(__file__).with_name('freeze-official.py'))
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'canary' / 'actions'))
from desktop_suite import read_frozen_manifest

module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class OfficialIdentity(unittest.TestCase):
    def test_workflow_freeze_arguments_work_with_runner_bash_and_nounset(self):
        workflow = (Path(__file__).resolve().parents[2] / '.github/workflows/desktop-automation-feasibility.yml').read_text()
        start = workflow.index('          freeze_args=')
        fragment = textwrap.dedent(workflow[start:workflow.index('\n      - name: Install exact', start)])
        for app, tag in [('zed-desktop', 'v1.22.0'), ('hermes-desktop', 'v2026.9.24')]:
            command = 'python3() { shift; printf "%s\\n" "$@"; }; RUNNER_TEMP=/tmp/fixture; '\
                + fragment.replace('${{ matrix.app }}', app)
            result = subprocess.run(['/bin/bash', '-uc', command], capture_output=True,
                                    text=True, timeout=20, check=True)
            arguments = result.stdout.splitlines()
            self.assertEqual(arguments[:2], ['--tag', tag])
            self.assertEqual('--expected-revision' in arguments, app == 'hermes-desktop')
            self.assertIn(app, arguments)

    def setUp(self):
        self.release = dict(draft=False, prerelease=False, tag_name='v1.2.3', assets=[
            dict(name='Zed-aarch64.dmg', digest='sha256:' + 'a' * 64,
                 browser_download_url='https://github.com/zed-industries/zed/releases/download/v1.2.3/Zed-aarch64.dmg')])

    def test_zed_digest_and_fixed_platform_asset(self):
        entry = module.freeze_zed(lambda _: self.release)
        self.assertEqual(entry['version'], '1.2.3')
        self.assertEqual(entry['digest'], 'sha256:' + 'a' * 64)
        self.assertFalse(entry['staged'])

    def test_zed_native_assets_match_platform_manifest_policy(self):
        for platform, name in [('linux', 'zed-linux-x86_64.tar.gz'), ('windows', 'Zed-x86_64.exe')]:
            release = copy.deepcopy(self.release)
            release['assets'][0]['name'] = name
            release['assets'][0]['browser_download_url'] = f'https://github.com/zed-industries/zed/releases/download/v1.2.3/{name}'
            entry = module.freeze_zed(lambda _: release, platform=platform)
            manifest = dict(schemaVersion=1, suite='desktop', platform=platform,
                            architecture='x86_64', model='qwen3.6', apps=[entry])
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / 'frozen.json'
                path.write_text(json.dumps(manifest))
                self.assertEqual(read_frozen_manifest(path, ['zed-desktop'], platform,
                                                      'x86_64', 'qwen3.6'), manifest)

    def test_frozen_zed_is_accepted_by_existing_manifest_contract(self):
        entry = module.freeze_zed(lambda _: self.release)
        manifest = dict(schemaVersion=1, suite='desktop', platform='macos',
                        architecture='aarch64', model='qwen3.6', apps=[entry])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'frozen.json'
            path.write_text(json.dumps(manifest))
            self.assertEqual(read_frozen_manifest(path, ['zed-desktop'], 'macos',
                                                  'aarch64', 'qwen3.6'), manifest)

    def test_exact_tag_endpoint_and_moved_identity_fail_closed(self):
        endpoints = []
        def fetch(endpoint):
            endpoints.append(endpoint)
            return self.release
        module.freeze_zed(fetch, tag='v1.2.3')
        self.assertEqual(endpoints, ['repos/zed-industries/zed/releases/tags/v1.2.3'])
        with self.assertRaises(ValueError):
            module.freeze_zed(fetch, tag='v1.2.4')
        with self.assertRaises(ValueError):
            module.freeze_zed(fetch, tag='v1.2.3/../../other')

    def test_zed_rejects_channel_duplicate_asset_url_and_digest(self):
        changes = [lambda r: r.update(prerelease=True),
                   lambda r: r['assets'].append(copy.deepcopy(r['assets'][0])),
                   lambda r: r['assets'][0].update(browser_download_url='https://example.test/asset'),
                   lambda r: r['assets'][0].update(digest='sha256:invalid')]
        for change in changes:
            release = copy.deepcopy(self.release)
            change(release)
            with self.assertRaises(ValueError):
                module.freeze_zed(lambda _: release)

    def test_hermes_pins_annotated_tag_commit_and_package_version(self):
        revision = 'b' * 40
        endpoints = []
        def fetch(endpoint):
            endpoints.append(endpoint)
            if endpoint.endswith('releases/latest'):
                return dict(draft=False, prerelease=False, tag_name='v0.20.0')
            if '/git/ref/' in endpoint:
                return dict(object=dict(type='tag', sha='a' * 40))
            if '/git/tags/' in endpoint:
                return dict(object=dict(type='commit', sha=revision))
            return dict(type='file', path='apps/desktop/package.json', encoding='base64',
                        content=base64.b64encode(json.dumps(dict(version='0.21.5')).encode()).decode())
        entry = module.freeze_hermes(fetch)
        self.assertEqual(entry['revision'], revision)
        self.assertEqual(entry['version'], '0.21.5')
        self.assertTrue(endpoints[-1].endswith('?ref=' + revision))

    def test_hermes_rejects_nested_tag(self):
        def fetch(endpoint):
            if endpoint.endswith('releases/latest'):
                return dict(draft=False, prerelease=False, tag_name='v0.20.0')
            return dict(object=dict(type='tag', sha='a' * 40))
        with self.assertRaises(ValueError):
            module.freeze_hermes(fetch)


if __name__ == '__main__':
    unittest.main()
