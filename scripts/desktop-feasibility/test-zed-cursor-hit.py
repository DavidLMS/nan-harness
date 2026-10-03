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
        deadline = time.monotonic() + 1
        INPUT['select_live_retry_point']((123, 456, 40, 20), lambda _: None,
            lambda point: INPUT['guarded_retry_proof'](point, lambda: True, lambda _: True,
                                                     observation, deadline),
            lambda: True, deadline, lambda _: None, observation)
        self.assertEqual(observation, dict(status='matched', sampledPoints=1,
            exactPointerMatched=True, accessibleHitVerified=True, guardBeforeVerified=3,
            guardAfterVerified=3, accessibleChecks=3, accessibleExactMatches=3,
            cursorChecks=2, cursorExactMatches=2, failureReason=None))

    def test_cursor_mismatch_and_instability_are_distinct_without_activation(self):
        for samples, reason, checks, matches in [([False] * 9, 'cursor-unmatched', 9, 0),
                                                ([True, False] * 9, 'cursor-unstable', 18, 9)]:
            observed = dict(status='unavailable', sampledPoints=0,
                            exactPointerMatched=False, accessibleHitVerified=False)
            cursor = iter(samples)
            with self.assertRaises(ValueError):
                INPUT['select_live_retry_point']((0, 0, 40, 20), lambda _: None,
                    lambda _: None, lambda: next(cursor), time.monotonic() + 1,
                    lambda _: None, observed)
            self.assertEqual(observed['sampledPoints'], 9)
            self.assertEqual(observed['failureReason'], reason)
            self.assertEqual(observed['cursorChecks'], checks)
            self.assertEqual(observed['cursorExactMatches'], matches)
            self.assertFalse(observed['exactPointerMatched'])

    def test_existing_accessible_proof_and_each_guard_have_closed_counts(self):
        def observation():
            return dict(guardBeforeVerified=0, guardAfterVerified=0,
                        accessibleChecks=0, accessibleExactMatches=0)
        observed = observation()
        INPUT['guarded_retry_proof']((123, 456), lambda: True, lambda _: True,
                                    observed, time.monotonic() + 1)
        self.assertEqual(list(observed.values()), [1, 1, 1, 1])
        for category in ('accessible-hit-mismatch', 'accessible-query-unavailable'):
            observed = observation()
            def reject(_):
                raise INPUT['RetryHitFailure'](category)
            with self.assertRaises(ValueError):
                INPUT['guarded_retry_proof']((123, 456), lambda: True, reject,
                                            observed, time.monotonic() + 1)
            self.assertEqual(observed['failureReason'], category)
            self.assertEqual(observed['accessibleChecks'], 1)
            self.assertEqual(observed['accessibleExactMatches'], 0)
            self.assertEqual(observed['guardAfterVerified'], 0)
            self.assertNotIn('123', str(observed))
        observed = observation()
        guards = iter([True, False])
        with self.assertRaises(ValueError):
            INPUT['guarded_retry_proof']((123, 456), lambda: next(guards), lambda _: True,
                                        observed, time.monotonic() + 1)
        self.assertEqual(observed['failureReason'], 'identity-rejected')
        self.assertEqual(observed['accessibleExactMatches'], 1)
        self.assertEqual(observed['guardAfterVerified'], 0)

    def test_expired_guard_records_deadline_without_querying_or_moving(self):
        observed = dict(guardBeforeVerified=0, guardAfterVerified=0,
                        accessibleChecks=0, accessibleExactMatches=0)
        with self.assertRaises(ValueError):
            INPUT['guarded_retry_proof']((123, 456), lambda: self.fail('guard after deadline'),
                                        lambda _: self.fail('query after deadline'), observed,
                                        time.monotonic() - 1)
        self.assertEqual(observed['status'], 'deadline')
        self.assertEqual(observed['failureReason'], 'deadline')
        self.assertEqual(observed['accessibleChecks'], 0)

    def test_pre_click_rejection_invalidates_selected_point_without_replay(self):
        observed = dict(status='matched', exactPointerMatched=True, accessibleHitVerified=True,
                        guardBeforeVerified=3, guardAfterVerified=3,
                        accessibleChecks=3, accessibleExactMatches=3)
        with self.assertRaises(ValueError):
            INPUT['guarded_retry_proof']((123, 456), lambda: False, lambda _: self.fail('lost owner'),
                                        observed, time.monotonic() + 1)
        self.assertEqual(observed['status'], 'identity-rejected')
        self.assertFalse(observed['exactPointerMatched'])
        self.assertFalse(observed['accessibleHitVerified'])
        self.assertEqual(observed['accessibleChecks'], 3)

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
