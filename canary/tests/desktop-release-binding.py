#!/usr/bin/env python3
"""Release qualification rejects branch-built, mixed, stale and incomplete evidence."""
import copy
import os
import re
import subprocess
import textwrap
from unittest.mock import patch
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'actions'))
import desktop_release_binding as binding
import desktop_release
import state
from desktop_maintenance import APPS, TARGETS, STEPS, baseline
from desktop_qualification import digest, envelope

SHA = 'a' * 40
COMMIT = 'b' * 40


class ReleaseBindingTests(unittest.TestCase):
    def test_regression_preflight_validates_without_a_manifest_anchor(self):
        root = Path(__file__).resolve().parents[2]
        workflow = (root / '.github/workflows/desktop-release-regression.yml').read_text()
        step = workflow.split('      - name: Verify trusted source and release selection\n', 1)[1].split('      - id: assets', 1)[0]
        script = textwrap.dedent(step.split('        run: |\n', 1)[1])
        head = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=root, capture_output=True,
                              text=True, check=True).stdout.strip()
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            called = directory / 'gh-called'
            gh = directory / 'gh'
            gh.write_text("""#!/usr/bin/env python3
import json, os
from pathlib import Path
Path(os.environ['GH_FIXTURE_CALLED']).write_text('called')
print(json.dumps([[dict(tag_name='v0.1.30')]]))
""")
            gh.chmod(0o700)
            env = {**os.environ, 'PATH': temporary + os.pathsep + os.environ['PATH'],
                   'RELEASE_TAG': 'v0.1.30', 'RELEASE_COMMIT': COMMIT,
                   'GITHUB_SHA': head, 'GITHUB_REPOSITORY': 'owner/repo',
                   'GH_FIXTURE_CALLED': str(called)}
            for key in ('RELEASE_MANIFEST_SHA', 'GH_TOKEN', 'GITHUB_TOKEN'):
                env.pop(key, None)
            for change, accepted in (({}, True), ({'RELEASE_TAG': 'main'}, False),
                                     ({'RELEASE_COMMIT': 'main'}, False),
                                     ({'GITHUB_SHA': '0' * 40}, False),
                                     ({'RELEASE_TAG': '', 'RELEASE_COMMIT': ''}, False)):
                with self.subTest(change=change):
                    called.unlink(missing_ok=True)
                    result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', script],
                        cwd=root, env={**env, **change}, capture_output=True, text=True)
                    self.assertEqual(result.returncode == 0, accepted, result.stderr)
                    self.assertEqual(called.exists(), accepted)

    def test_isolated_regression_stages_attested_same_run_windows_assets(self):
        workflow = (Path(__file__).resolve().parents[2] /
                    '.github/workflows/desktop-release-regression.yml').read_text()
        self.assertEqual(re.findall(r'^  ([a-z-]+):$', workflow.split('jobs:', 1)[1], re.M),
                         ['prepare', 'qualify'])
        self.assertIn('if: github.ref_name == github.event.repository.default_branch', workflow)
        self.assertIn('ref: ${{ github.sha }}', workflow)
        self.assertIn('persist-credentials: false', workflow)
        self.assertIn('release_manifest_sha256: ${{ needs.prepare.outputs.manifest_sha256 }}', workflow)
        self.assertIn('app: hermes-desktop', workflow)
        self.assertIn('platform: windows', workflow)
        self.assertIn('experiment: deterministic-full', workflow)
        self.assertNotIn('release-publication', workflow)
        self.assertNotIn('run-id:', workflow)
        source = textwrap.dedent(workflow.split("python3 - <<'PYTHON'\n", 1)[1].split('          PYTHON', 1)[0])
        with tempfile.TemporaryDirectory() as temporary:
            root, calls = Path(temporary), []
            output = root / 'output'
            release = dict(tag_name='v0.1.30', draft=True)
            class Store:
                def __init__(self, repository):
                    self.repository = repository
                def call(self, endpoint):
                    self_endpoint = 'releases/tags/v0.1.30'
                    if endpoint != self_endpoint:
                        raise AssertionError('unexpected release lookup')
                    return release
            def stage(store, tag, commit, system, architecture, directory):
                calls.append((store.repository, tag, commit, system, architecture))
                self.assertFalse(directory.exists())
                directory.mkdir()
                (directory / 'SHA256SUMS').write_text('synthetic attested manifest')
            env = dict(RELEASE_TAG='v0.1.30', RELEASE_COMMIT=COMMIT,
                       GITHUB_REPOSITORY='owner/repo', RUNNER_TEMP=temporary, GITHUB_OUTPUT=str(output))
            with patch.dict(os.environ, env), patch.object(state, 'Store', Store), patch.object(desktop_release, 'stage', stage):
                exec(compile(source, '<workflow fixture>', 'exec'), {})
                self.assertEqual(calls, [('owner/repo', 'v0.1.30', COMMIT, 'windows', 'x86_64')])
                self.assertEqual(output.read_text(), 'manifest_sha256=' + digest(root / 'desktop-release-assets/SHA256SUMS') + '\n')
                for change in (dict(draft=False), dict(tag_name='v0.1.29')):
                    previous = dict(release)
                    release.update(change)
                    with self.assertRaises(SystemExit):
                        exec(compile(source, '<workflow fixture>', 'exec'), {})
                    release.clear(); release.update(previous)
                self.assertEqual(len(calls), 1)

    def test_release_selection_cannot_be_partial_or_malformed(self):
        self.assertFalse(binding.validate_selection('', ''))
        self.assertTrue(binding.validate_selection('v0.1.15', COMMIT))
        for tag, commit in [('', COMMIT), ('v0.1.15', ''), ('main', COMMIT),
                            ('v0.1.15', 'main'), ('v0.1.15\n', COMMIT)]:
            with self.assertRaises(ValueError):
                binding.validate_selection(tag, commit)

    def test_all_cells_must_match_exact_attested_bytes_and_complete_acceptance(self):
        with tempfile.TemporaryDirectory() as temp:
            binaries, cells = {}, []
            for platform, (arch, _) in TARGETS.items():
                binary = Path(temp) / platform
                binary.write_bytes(platform.encode())
                binaries[platform] = binary
                for app in APPS:
                    expected = baseline(platform)[app]
                    cell = envelope(app, platform, arch, SHA)
                    cell.update(qualification='deterministic-full', outcome='passed',
                                appCleanup='passed', globalCleanup='passed',
                                appVersion=expected['version'], upstreamRevision=expected.get('revision'),
                                upstreamArtifactSha256=expected.get('digest', '').removeprefix('sha256:') or None,
                                probes=[dict(status='passed', steps=sorted(STEPS)) for _ in range(3)])
                    for key in ('checkerSha256', 'launcherSha256', 'realNanhSha256',
                                'frozenManifestSha256', 'preparedSha256', 'reportSha256', 'applicationSha256'):
                        cell[key] = digest(binary)
                    cells.append(cell)
            matrix = dict(schemaVersion=1, sourceSha=SHA, qualification='deterministic-full',
                          excludedApps=['pen-desktop'], cells=cells)
            result = binding.bind(matrix, SHA, 'v0.1.15', COMMIT, binaries)
            self.assertEqual(result['release']['commit'], COMMIT)
            self.assertEqual(result['release']['assets']['linux']['sha256'], digest(binaries['linux']))
            for change in [dict(realNanhSha256='c' * 64), dict(sourceSha=COMMIT),
                           dict(appCleanup='failed'), dict(probes=[]), dict(appVersion='0.0.0')]:
                invalid = copy.deepcopy(matrix)
                invalid['cells'][0].update(change)
                with self.assertRaises(ValueError):
                    binding.bind(invalid, SHA, 'v0.1.15', COMMIT, binaries)
            for altered in [cells[:-1], cells + [cells[0]], cells[:-1] + [cells[0]]]:
                with self.assertRaises(ValueError):
                    binding.bind({**matrix, 'cells': altered}, SHA, 'v0.1.15', COMMIT, binaries)


if __name__ == '__main__':
    unittest.main()
