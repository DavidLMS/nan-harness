#!/usr/bin/env python3
"""Synthetic GPUI geometry only; never read or launch a desktop application."""
import json
from pathlib import Path
import struct
import tempfile
import unittest

from zed_hit_geometry import classify, observations, probe, save_target, split_maps


TARGET = dict(point=[30, 30], bounds=[10, 20, 40, 20], viewport=[200, 200])
BOUNDS = (10., 20., 40., 20.)
MASK = (0., 0., 200., 200.)


def bits(value):
    return struct.unpack('<I', struct.pack('<f', value))[0]


def frame(boxes):
    return {'@geometryHeaders': [(1, len(boxes), bits(30), bits(30), bits(200), bits(200))],
            '@geometryRects': [(1, index, *(bits(n) for n in bounds), *(bits(n) for n in mask), behavior)
                               for index, (bounds, mask, behavior) in enumerate(boxes)]}


class GeometryTests(unittest.TestCase):
    def test_targets_are_private_exclusive_and_bounded_to_three_activations(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            for slot in range(1, 4):
                save_target(root, TARGET)
                path = root / f'target-{slot}.json'
                self.assertEqual(json.loads(path.read_text()), TARGET)
                self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            before = (root / 'target-1.json').read_bytes()
            with self.assertRaises(ValueError):
                save_target(root, TARGET)
            self.assertEqual((root / 'target-1.json').read_bytes(), before)
            root.chmod(0o755)
            with self.assertRaises(ValueError):
                save_target(root, TARGET)

    def test_classifies_clipping_occlusion_and_normal_overlap_separately(self):
        target = (BOUNDS, MASK, 0)
        for boxes, clipped, blockers, hovered in (
                ([target], False, 0, True),
                ([(BOUNDS, (0, 0, 20, 20), 0)], True, 0, False),
                ([target, (MASK, MASK, 1)], False, 1, False),
                ([target, (MASK, MASK, 2)], False, 1, False),
                ([target, (MASK, MASK, 0)], False, 0, True),
                ([(MASK, MASK, 1), target], False, 0, True),
                ([target, (MASK, (100, 100, 10, 10), 1)], False, 0, True)):
            result = classify(TARGET, (30, 30), boxes, (200, 200))
            self.assertEqual(result['status'], 'matched')
            self.assertEqual(result['targetMaskContainsPoint'], not clipped)
            self.assertEqual(result['blockingHitboxesAhead'], blockers)
            self.assertEqual(result['targetWouldBeHovered'], hovered)
            self.assertNotIn('bounds', result)
            self.assertNotIn('point', result)

    def test_geometric_identity_ambiguity_never_selects_by_paint_order(self):
        for boxes, expected in (([(MASK, MASK, 0)], 'absent'),
                                ([(BOUNDS, MASK, 0)] * 2, 'ambiguous')):
            result = classify(TARGET, (90, 90), boxes, (200, 200))
            self.assertEqual(result['status'], expected)
            self.assertIsNone(result['targetWouldBeHovered'])
            self.assertFalse(result['priorPointerMatches'])

    def test_physical_accessibility_pixels_are_scaled_to_the_rendered_viewport(self):
        target = {key: [n * 2 for n in values] for key, values in TARGET.items()}
        result = classify(target, (30, 30), [(BOUNDS, MASK, 0)], (200, 200))
        self.assertEqual(result['status'], 'matched')
        self.assertTrue(result['priorPointerMatches'])
        self.assertTrue(result['targetWouldBeHovered'])
        for viewport in ((0, 200), (200, 100), (float('nan'), 200)):
            with self.assertRaises(ValueError):
                classify(target, (30, 30), [(BOUNDS, MASK, 0)], viewport)

    def test_readback_requires_complete_finite_unique_rows_and_owned_target(self):
        original = frame([(BOUNDS, MASK, 0)])
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            target = root / 'target-1.json'
            target.write_text(json.dumps(TARGET))
            result = observations(original, root, 1)
            self.assertTrue(result[0]['targetWouldBeHovered'])
            for changed in (
                    {**original, '@geometryRects': []},
                    {**original, '@geometryRects': original['@geometryRects'] * 2},
                    {**original, '@geometryHeaders': [(1, 1025, 0, 0, bits(200), bits(200))]},
                    {**original, '@geometryHeaders': [(1, 1, 0x7fc00000, 0, bits(200), bits(200))]},
                    {**original, '@geometryRects': [(1, 0, *([0] * 8), 3)]}):
                with self.assertRaises(ValueError):
                    observations(changed, root, 1)
            target.unlink()
            other = root / 'other'
            other.write_text(json.dumps(TARGET))
            target.symlink_to(other)
            with self.assertRaises(ValueError):
                observations(original, root, 1)

    def test_parser_discards_numeric_maps_from_counter_stream(self):
        source = frame([(BOUNDS, MASK, 0)])
        encoded = [dict(type='map', data={name: {','.join(map(str, row)): 1 for row in rows}})
                   for name, rows in source.items()]
        counter = b'{"type":"map","data":{"@input":3}}'
        raw = b'\n'.join(json.dumps(item).encode() for item in encoded) + b'\n' + counter
        remaining, decoded = split_maps(raw)
        self.assertEqual(remaining, counter)
        self.assertEqual(decoded, source)
        for bad in (raw + b'\n' + json.dumps(encoded[0]).encode(),
                    b'{"type":"printf","data":"PRIVATE"}',
                    b'{"type":"map","data":{"@geometryHeaders":{"1,1,0,0":2}}}',
                    b'x' * (768 * 1024 + 1)):
            with self.assertRaises(ValueError):
                split_maps(bad)

    def test_probe_bounds_reads_and_never_exports_object_identity(self):
        source = probe('/owned/zed-editor', 'dispatch_event')
        self.assertIn('$i < 1024', source)
        self.assertIn('$n <= 1024', source)
        self.assertIn('@geometrySlot != @slot', source)
        self.assertNotIn('printf', source)
        self.assertNotIn('ustack', source)
        self.assertNotIn('arg1', source)


if __name__ == '__main__':
    unittest.main()
