#!/usr/bin/env python3
"""Release qualification rejects branch-built, mixed, stale and incomplete evidence."""
import copy
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'actions'))
import desktop_release_binding as binding
from desktop_maintenance import APPS, TARGETS, STEPS, baseline
from desktop_qualification import digest, envelope

SHA = 'a' * 40
COMMIT = 'b' * 40


class ReleaseBindingTests(unittest.TestCase):
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
