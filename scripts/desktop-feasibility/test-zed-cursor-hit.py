#!/usr/bin/env python3
"""Synthetic cursor files and hover callbacks; no X server is queried."""
import runpy
import ctypes
from pathlib import Path
import struct
import tempfile
import time
import unittest

HERE = Path(__file__).parent
CURSOR = runpy.run_path(str(HERE / 'zed-cursor-hit.py'))
INPUT = runpy.run_path(str(HERE / 'zed-input-x11.py'))


def cursor_file(frames):
    offset = 16 + 12 * len(frames)
    table, images = [], []
    for size, pixels in frames:
        image = struct.pack('<9I', 36, 0xfffd0002, size, 1, 2, 1, 1, 0, 10)
        image += struct.pack('<2I', *pixels)
        table.append(struct.pack('<3I', 0xfffd0002, size, offset))
        images.append(image)
        offset += len(image)
    return b'Xcur' + struct.pack('<3I', 16, 1, len(frames)) + b''.join(table + images)


class CursorTests(unittest.TestCase):
    def test_exact_pixels_hotspot_and_nearest_size_first_tie(self):
        data = cursor_file([(16, [0xff000001, 0xff000002]), (32, [3, 4])])
        self.assertEqual(CURSOR['parse_images'](data, 24), [(2, 1, 1, 0, (0xff000001, 0xff000002))])

    def test_animated_same_size_frames_retained(self):
        images = CURSOR['parse_images'](cursor_file([(24, [1, 2]), (24, [3, 4])]), 24)
        self.assertEqual(len(images), 2)
        self.assertNotIn((2, 1, 0, 0, (1, 2)), images)
        self.assertNotIn((2, 1, 1, 0, (1, 3)), images)

    def test_truncated_table_and_image_bounds_rejected(self):
        valid = cursor_file([(24, [1, 2])])
        for data in (b'', valid[:20], valid[:-1], valid[:16] + struct.pack('<3I', 0xfffd0002, 24, 999999)):
            with self.assertRaises(ValueError):
                CURSOR['parse_images'](data, 24)

    def test_alias_theme_fallback_and_source_size_selection(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory).resolve()
            public = home / 'themes' / 'default' / 'cursors'
            public.mkdir(parents=True)
            (public / 'hand').write_bytes(cursor_file([(16, [1, 2]), (32, [3, 4])]))
            env = {'HOME': str(home), 'XCURSOR_PATH': str(home / 'themes'), 'XCURSOR_THEME': 'missing'}
            images = CURSOR['expected_images'](env, {'Xft.dpi': '144'}, (1920, 1080))
            self.assertEqual(images[0][-1], (3, 4))

    def test_system_symlink_cannot_escape_public_root(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory).resolve()
            link = home / 'cursor'
            link.symlink_to('/etc/passwd')
            with self.assertRaises(ValueError):
                CURSOR['public_file'](link, str(home))

    def test_core_theme_and_relative_search_path_rejected(self):
        with self.assertRaises(ValueError):
            CURSOR['find_icon']([], 'core', 'pointer', None)
        with self.assertRaises(ValueError):
            CURSOR['search_paths']({'XCURSOR_PATH': 'relative'})

    def test_scan_selects_first_two_sample_live_hit_without_activation(self):
        moved, proved, matches = [], [], iter([False, True, True])
        point = INPUT['select_live_retry_point']((100, 200, 40, 20), moved.append,
            proved.append, lambda: next(matches), time.monotonic() + 1, lambda _: None)
        self.assertEqual(point, (110, 205))
        self.assertEqual(len(moved), 2)
        self.assertEqual(proved[-2:], [point, point])

    def test_scan_all_mismatches_never_selects_or_activates(self):
        moved = []
        with self.assertRaises(ValueError):
            INPUT['select_live_retry_point']((0, 0, 40, 20), moved.append,
                lambda _: None, lambda: False, time.monotonic() + 1, lambda _: None)
        self.assertEqual(len(moved), 9)

    def test_ambiguous_accessible_hit_and_owner_loss_stop_before_move(self):
        moved = []
        def reject(_):
            raise ValueError('synthetic identity failure')
        with self.assertRaises(ValueError):
            INPUT['select_live_retry_point']((0, 0, 40, 20), moved.append,
                reject, lambda: True, time.monotonic() + 1, lambda _: None)
        self.assertEqual(moved, [])

    def test_deadline_zero_hover_and_no_reset(self):
        moved = []
        with self.assertRaises(ValueError):
            INPUT['select_live_retry_point']((0, 0, 40, 20), moved.append,
                lambda _: None, lambda: True, time.monotonic() - 1, lambda _: None)
        self.assertEqual(moved, [])

    def test_closed_selection_receipt_never_contains_pixels_or_coordinates(self):
        observation = dict(status='unavailable', sampledPoints=0,
            exactPointerMatched=False, accessibleHitVerified=False)
        INPUT['select_live_retry_point']((123, 456, 40, 20), lambda _: None,
            lambda _: None, lambda: True, time.monotonic() + 1, lambda _: None, observation)
        self.assertEqual(observation, dict(status='matched', sampledPoints=1,
            exactPointerMatched=True, accessibleHitVerified=True))

    def test_live_image_exact_comparison_and_ownership_loss(self):
        shape = CURSOR['PointerShape'].__new__(CURSOR['PointerShape'])
        pixels = (ctypes.c_ulong * 2)(0xff000001, 0xff000002)
        image = CURSOR['CursorImage'](0, 0, 2, 1, 1, 0, 7, pixels, 0, None)
        freed = []
        class Fix:
            @staticmethod
            def XFixesGetCursorImage(_):
                return ctypes.pointer(image)
        class Library:
            XFree = staticmethod(freed.append)
        with tempfile.TemporaryDirectory() as directory:
            stat = Path(directory) / 'stat'
            stat.write_bytes(b'1 (synthetic) ' + b'0 ' * 19 + b'99')
            shape.process_stat, shape.process_start = stat, b'99'
            shape.display, shape.fix, shape.xlib = 1, Fix(), Library()
            shape.deadline, shape.guard = time.monotonic() + 1, lambda: True
            shape.images = [(2, 1, 1, 0, (0xff000001, 0xff000002))]
            self.assertTrue(shape.matches())
            image.xhot = 0
            self.assertFalse(shape.matches())
            shape.guard = lambda: False
            self.assertFalse(shape.matches())
            self.assertEqual(len(freed), 2)


if __name__ == '__main__':
    unittest.main()
