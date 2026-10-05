#!/usr/bin/env python3
"""Failure boundaries of the synthetic hosted geometry preflight."""
import importlib.util
import io
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location('geometry_fixture', Path(__file__).with_name('zed-geometry-fixture.py'))
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


def maps():
    def bits(value):
        return struct.unpack('<I', struct.pack('<f', value))[0]
    header = [1, 1024, *map(bits, (30, 30, 200, 200))]
    target = [1, 0, *map(bits, (10, 20, 40, 20, 0, 0, 200, 200)), 0]
    blocker = [1, 1023, *map(bits, (0, 0, 200, 200, 0, 0, 200, 200)), 1]
    return b'\n'.join(json.dumps(dict(type='map', data={key: {
        ','.join(map(str, row)): 1 for row in rows}})).encode()
        for key, rows in (('@geometryHeaders', [header]), ('@geometryRects', [target, *([1, i, *([0] * 9)] for i in range(1, 1023)), blocker])))


class FixtureTests(unittest.TestCase):
    def execute(self, ready, returncode, output, errors):
        process = Mock(stderr=io.BytesIO(), pid=1234, returncode=returncode)
        process.poll.return_value = None if ready else returncode
        process.communicate.return_value = (b'', errors)
        def spawn(args, **kwargs):
            Path(args[args.index('-o') + 1]).write_bytes(output)
            return process
        receipt = dict(status='failed', failure=None)
        with tempfile.TemporaryDirectory() as temporary, \
                patch.object(fixture.subprocess, 'Popen', side_effect=spawn), \
                patch.object(fixture.subprocess, 'run', return_value=Mock(returncode=0, stderr=b'')) as run, \
                patch.object(fixture, 'read_ready', return_value=ready):
            fixture.run(Path(temporary).resolve(), receipt)
            calls = [call.args[0] for call in run.call_args_list]
        return receipt, calls

    def test_actual_reduction_must_match_known_occlusion(self):
        receipt, calls = self.execute(True, 0, maps(), b'')
        self.assertEqual(receipt['status'], 'passed')
        self.assertEqual(receipt['stage'], 'complete')
        self.assertEqual(len(calls), 4)  # Build, compile probe, dispatch, stop.
        self.assertEqual(calls[-1][-1], '-1234')

    def test_attach_failure_never_dispatches_or_exports_stderr(self):
        receipt, calls = self.execute(False, 1, b'', b'PRIVATE ERROR: failed to load program')
        self.assertEqual(receipt['status'], 'failed')
        self.assertEqual(receipt['failure'], 'tracer-exited')
        self.assertEqual(receipt['diagnosticCategories'], ['program-load'])
        self.assertEqual(len(calls), 2)
        self.assertNotIn('PRIVATE', str(receipt))

    def test_missing_geometry_cannot_pass_preflight(self):
        with self.assertRaises(ValueError):
            self.execute(True, 0, b'', b'')

    def test_compilation_rejection_never_attaches(self):
        receipt = dict(status='failed', failure=None)
        with tempfile.TemporaryDirectory() as temporary, \
                patch.object(fixture.subprocess, 'Popen') as spawn, \
                patch.object(fixture.subprocess, 'run', return_value=Mock(
                    returncode=1, stderr=b'PRIVATE ERROR: Unknown identifier: private')):
            fixture.run(Path(temporary).resolve(), receipt)
        spawn.assert_not_called()
        self.assertEqual(receipt['stage'], 'compile')
        self.assertEqual(receipt['failure'], 'compilation')
        self.assertEqual(receipt['diagnosticCategories'], ['compiler-identifier', 'tracer-error'])
        self.assertNotIn('PRIVATE', str(receipt))

    def test_verifier_diagnostics_are_closed_categories(self):
        observed = fixture.diagnostics(b'PRIVATE BPF program is too large. Processed 1000001 insn\n'
                                       b'ERROR: Error loading program: PRIVATE')
        self.assertEqual(observed, ['program-load', 'tracer-error', 'verifier-complexity'])


if __name__ == '__main__':
    unittest.main()
