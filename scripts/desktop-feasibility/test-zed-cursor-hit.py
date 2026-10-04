#!/usr/bin/env python3
"""Synthetic cursor files and hover callbacks; no X server is queried."""
import runpy
import ctypes
from pathlib import Path
from unittest.mock import patch
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
    def test_size_zero_preserves_explicit_environment_selection(self):
        size = CURSOR['cursor_size']
        self.assertEqual(size({'XCURSOR_SIZE': '0'}, {'Xcursor.size': '32'}, (800, 600)),
                         (0, 'environment'))
        self.assertEqual(size({}, {'Xcursor.size': '32'}, (800, 600)), (32, 'resource'))
        self.assertEqual(size({}, {'Xft.dpi': '96'}, (800, 600)), (21, 'dpi'))
        self.assertEqual(size({}, {}, (800, 600)), (12, 'screen'))

    def test_classification_requires_exact_image_and_rejects_ambiguity(self):
        image = (2, 1, 1, 0, (1, 2))
        classify = CURSOR['classify_image']
        self.assertEqual(classify(image, {'hand': [image], 'arrow': []}), 'hand')
        self.assertEqual(classify(image, {'hand': [image], 'arrow': [image]}), 'unknown')
        self.assertEqual(classify((2, 1, 0, 0, (1, 2)), {'hand': [image]}), 'unknown')
        self.assertEqual(classify((1, 1, 0, 0, (0,)), {}), 'transparent')
        self.assertEqual(classify((2, 1, 0, 0, (0, 0)), {}), 'unknown')

    def test_classification_reuses_one_sample_and_does_not_change_hand_oracle(self):
        class Sample:
            size_provenance = 'resource'
            calls = 0
            def matches(self):
                self.calls += 1
                self.last_classification = 'arrow'
                return False
        sample, observation = Sample(), {'cursorChecks': 0, 'cursorExactMatches': 0}
        self.assertFalse(INPUT['sampled_cursor_match'](sample.matches, observation))
        self.assertEqual(sample.calls, 1)
        self.assertEqual(observation['cursorClasses']['arrow'], 1)
        self.assertEqual(observation['cursorExactMatches'], 0)
        self.assertEqual(observation['cursorSizeSource'], 'resource')

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
        self.assertEqual(point, (120, 210))
        self.assertEqual(len(moved), 1)
        self.assertEqual(proved[-2:], [point, point])

    def test_pointer_position_and_child_are_proved_before_cursor_sampling(self):
        for position, child, reason in [((0, 0, 1), 'client', 'pointer-position'),
                                        ((10, 5, 1), 'other', 'pointer-child')]:
            observed = dict(status='unavailable', sampledPoints=0)
            samples = []
            with self.assertRaises(ValueError):
                INPUT['select_live_retry_point']((0, 0, 20, 10), lambda _: None,
                    lambda _: None, lambda: samples.append(True) or True,
                    time.monotonic() + 1, lambda _: None, observed,
                    lambda point: INPUT['guarded_pointer_sample'](
                        point, lambda: position, lambda: child, observed))
            self.assertEqual(samples, [])
            self.assertEqual(observed['failureReason'], reason)
            self.assertEqual(observed['pointerChecks'], 1)

    def test_delayed_hand_stays_on_same_point_but_late_hand_is_never_selected(self):
        clock = [0.0]
        def pause(seconds): clock[0] += seconds
        samples = iter([False, True, True])
        observed = dict(status='unavailable', sampledPoints=0)
        with patch('time.monotonic', lambda: clock[0]):
            point = INPUT['select_live_retry_point']((0, 0, 20, 10), lambda _: None,
                lambda _: None, lambda: next(samples), 1, pause, observed,
                lambda point: INPUT['guarded_pointer_sample'](
                    point, lambda: (*point, 1), lambda: 'client', observed))
        self.assertEqual(point, (10, 5))
        self.assertEqual(observed['sampledPoints'], 1)
        self.assertEqual(observed['pointerChecks'], 3)
        self.assertEqual(observed['pointerChildMatches'], 3)
        clock[0] = 0
        def late(): clock[0] = 2; return True
        observed = dict(status='unavailable', sampledPoints=0)
        with patch('time.monotonic', lambda: clock[0]), self.assertRaises(ValueError):
            INPUT['select_live_retry_point']((0, 0, 20, 10), lambda _: None,
                lambda _: None, late, 1, pause, observed)
        self.assertEqual(observed['status'], 'deadline')
        self.assertFalse(observed.get('exactPointerMatched', False))

    def test_fresh_query_cost_leaves_time_for_two_hand_samples(self):
        clock = [0.0]
        events = []
        answers = iter([False, True, True])
        def prove(point):
            events.append('proof')
            clock[0] += 0.020
        def pointer(point):
            events.append('pointer')
            clock[0] += 0.015
        def cursor():
            events.append('cursor')
            clock[0] += 0.010
            return next(answers)
        moved = []
        with patch('time.monotonic', lambda: clock[0]):
            point = INPUT['select_live_retry_point']((0, 0, 20, 10), moved.append,
                prove, cursor, 0.4, lambda delay: clock.__setitem__(0, clock[0] + delay),
                pointer_proof=pointer)
        self.assertEqual(point, (10, 5))
        self.assertEqual(moved, [point])
        self.assertEqual(events, ['proof'] + ['proof', 'pointer', 'cursor'] * 3 + ['proof'])
        self.assertLess(clock[0], 0.27)

    def test_query_cost_cannot_authorize_late_hand_or_late_hover(self):
        clock = [0.0]
        moved = []
        samples = []
        def slow_proof(point): clock[0] += 0.12
        with patch('time.monotonic', lambda: clock[0]), self.assertRaises(ValueError):
            INPUT['select_live_retry_point']((0, 0, 20, 10), moved.append,
                slow_proof, lambda: samples.append(True) or True, 0.1, lambda _: None)
        self.assertEqual(moved, [])
        self.assertEqual(samples, [])
        clock[0] = 0
        observed = dict(status='unavailable', sampledPoints=0)
        def cursor(): clock[0] += 0.06; return True
        with patch('time.monotonic', lambda: clock[0]), self.assertRaises(ValueError):
            INPUT['select_live_retry_point']((0, 0, 20, 10), moved.append,
                lambda _: None, cursor, 0.1,
                lambda delay: clock.__setitem__(0, clock[0] + delay), observed)
        self.assertEqual(observed['status'], 'deadline')
        self.assertFalse(observed.get('exactPointerMatched', False))

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
            exactPointerMatched=True, accessibleHitVerified=True, guardBeforeVerified=4,
            guardAfterVerified=4, accessibleChecks=4, accessibleExactMatches=4,
            cursorChecks=2, cursorExactMatches=2, failureReason=None))

    def test_cursor_mismatch_and_instability_are_distinct_without_activation(self):
        for samples, reason, checks, matches in [([False] * 45, 'cursor-unmatched', 45, 0),
                                                ([True, False, True, False, False] * 9, 'cursor-unstable', 45, 18)]:
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
            shape.references = {"hand": shape.images}
            self.assertTrue(shape.matches())
            image.xhot = 0
            self.assertFalse(shape.matches())
            shape.guard = lambda: False
            self.assertFalse(shape.matches())
            self.assertEqual(len(freed), 2)


if __name__ == '__main__':
    unittest.main()
