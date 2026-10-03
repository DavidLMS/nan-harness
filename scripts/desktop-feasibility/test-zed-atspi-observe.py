#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch
import tempfile
import json
import os

spec = importlib.util.spec_from_file_location('sampler', Path(__file__).with_name('zed-atspi-observe.py'))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class Backend:
    def __init__(self, reads, geometry=(100, 200, 800, 600)):
        self.reads, self.geometry = iter(reads), geometry
    def guard(self):
        return self.geometry
    def read(self, held, deadline):
        return next(self.reads)


class Tests(unittest.TestCase):
    def test_phase_binding_preserves_legacy_and_rejects_unknown(self):
        request = dict(pid=7, window=8, buttons=[], icons=[])
        self.assertEqual(m.validate(request), request)
        for phase in ('pre-send', 'pre-retry'):
            current = dict(request, phase=phase)
            self.assertEqual(m.validate(current), current)
            measured = m.measure(current, Backend([]), 10, clock=lambda: 0)
            self.assertEqual(measured['phase'], phase)
        with self.assertRaises(ValueError):
            m.validate(dict(request, phase='private-output'))

    def test_published_ancestor_intersection_is_closed_and_not_action_authority(self):
        held, parent, app = (':1.2', '/retry'), (':1.2', '/group'), (':1.2', '/app')
        def collect(extent):
            chain = {held: (parent, extent), parent: (app, None)}
            return m.published_ancestors(held, app, (10, 20, 40, 20), chain.__getitem__, 10, lambda: 0)
        first = collect((0, 0, 80, 80))
        self.assertEqual(m.compare_ancestors(first, first), dict(
            centerWithinPublishedAncestors=True, ancestorBoundsStatus='complete', checkedAncestorCount=1,
            ancestorQueryStage='complete'))
        clipped = collect((0, 0, 20, 20))
        self.assertFalse(m.compare_ancestors(clipped, clipped)['centerWithinPublishedAncestors'])
        changed = collect((0, 0, 90, 90))
        self.assertIsNone(m.compare_ancestors(first, changed)['centerWithinPublishedAncestors'])
        self.assertEqual(m.compare_ancestors(first, changed)['ancestorQueryStage'], 'comparison')
        self.assertNotIn('/group', json.dumps(m.compare_ancestors(first, first)))

    def test_ancestor_cycles_foreign_owner_missing_bounds_and_limit_remain_unknown(self):
        held, app = (':1.2', '/retry'), (':1.2', '/app')
        def collect(read):
            return m.published_ancestors(held, app, (10, 20, 40, 20), read, 10, lambda: 0)
        cyclic = collect(lambda _: (held, (0, 0, 100, 100)))
        self.assertEqual(cyclic[0]['ancestorBoundsStatus'], 'cycle')
        foreign = collect(lambda _: ((':1.3', '/group'), (0, 0, 100, 100)))
        self.assertEqual(foreign[0], m.ancestor_result(stage='parent'))
        missing = collect(lambda _: ((':1.2', '/group'), None))
        self.assertEqual(missing[0], m.ancestor_result(stage='ancestor-bounds'))
        index = 0
        def endless(_):
            nonlocal index
            index += 1
            return (':1.2', '/group' + str(index)), (0, 0, 100, 100)
        limited = collect(endless)
        self.assertEqual(limited[0], m.ancestor_result('limit', 64))
        self.assertIsNone(limited[1])

    def test_late_ancestor_query_never_certifies_or_resets_deadline(self):
        clock = iter([0, 11])
        result = m.published_ancestors((':1.2', '/retry'), (':1.2', '/app'),
            (10, 20, 40, 20), lambda _: ((':1.2', '/app'), None), 10, lambda: next(clock))
        self.assertEqual(result, (m.ancestor_result(stage='deadline'), None))

    def test_ancestor_query_failure_and_malformed_payload_stages_are_private(self):
        held, app = (':1.2', '/retry'), (':1.2', '/app')
        def collect(read):
            return m.published_ancestors(held, app, (10, 20, 40, 20), read, 10, lambda: 0)[0]
        malformed_parent = collect(lambda _: (('PRIVATE',), None))
        self.assertEqual(malformed_parent['ancestorQueryStage'], 'parent')
        malformed_extent = collect(lambda _: ((':1.2', '/parent'), (0, 0, -1, 10)))
        self.assertEqual(malformed_extent['ancestorQueryStage'], 'ancestor-bounds')
        def failed_read(_):
            raise OSError('PRIVATE error payload')
        for stage in ('parent', 'ancestor-bounds'):
            failed_read.query_stage = stage
            result = collect(failed_read)
            self.assertEqual(result['ancestorQueryStage'], stage)
            self.assertNotIn('PRIVATE', json.dumps(result))
        def expired_read(_):
            raise TimeoutError('PRIVATE timeout')
        self.assertEqual(collect(expired_read)['ancestorQueryStage'], 'deadline')

    def request(self):
        return m.validate(dict(pid=71, window=91,
            buttons=[dict(bus=':1.2', path='/org/a11y/private')], icons=[[110, 220, 14, 14]]))
    def test_two_owned_proofs_missing_origin_and_raw_toggle(self):
        value = (43, (1 << 4) | (1 << 8) | (1 << 30), (10, 20, 40, 40), (10, 20, 40, 40))
        result = m.measure(self.request(), Backend([value, value]), 10, lambda: 0)
        self.assertEqual(result['offsetMissing'], 1)
        self.assertEqual(result['toggleOn'], 1)
        self.assertEqual(result['containmentRejected'], 0)
        self.assertNotIn('private', str(result))
    def test_toggle_button_pressed_state_is_used_instead_of_checked(self):
        value = (62, (1 << 20) | (1 << 8) | (1 << 30), (110, 220, 40, 40), (10, 20, 40, 40))
        result = m.measure(self.request(), Backend([value, value]), 10, lambda: 0)
        self.assertEqual(result['toggleOn'], 1)

    def test_expected_origin_and_indeterminate(self):
        value = (43, (1 << 32) | (1 << 8) | (1 << 25), (110, 220, 40, 40), (10, 20, 40, 40))
        result = m.measure(self.request(), Backend([value, value]), 10, lambda: 0)
        self.assertEqual(result['offsetExpected'], 1)
        self.assertEqual(result['toggleUnknown'], 1)
    def test_replaced_identity_and_unstable_state_not_certified(self):
        result = m.measure(self.request(), Backend([None, None]), 10, lambda: 0)
        self.assertEqual(result['identityRejected'], 1)
        self.assertEqual(result['sampledButtons'], 0)
        a = (43, 0, (1, 2, 3, 4), (1, 2, 3, 4))
        b = (43, 1 << 4, (1, 2, 3, 4), (1, 2, 3, 4))
        result = m.measure(self.request(), Backend([a, b]), 10, lambda: 0)
        self.assertEqual(result['stabilityRejected'], 1)
    def test_frame_geometry_never_becomes_client_origin(self):
        backend = m.Backend.__new__(m.Backend)
        backend.request = {'pid': 71, 'window': 91}
        backend.frame_matches = lambda active, frame: True
        calls = []
        def query(args):
            calls.append(args)
            return b'91'
        backend.query = query
        with self.assertRaises(ValueError):
            backend.guard()
        self.assertEqual(calls, [['getactivewindow']])

    def test_distinct_owned_client_requires_matching_pid(self):
        backend = m.Backend.__new__(m.Backend)
        backend.request = {'pid': 71, 'window': 91}
        backend.frame_matches = lambda active, frame: active == 92 and frame == 91
        backend.client_snapshot = lambda active: ((100, 200), (2, 24), (800, 600), 1, 3)
        values = iter([b'92', b'71', b'X=102\nY=224\nWIDTH=800\nHEIGHT=600\nSCREEN=0\nWINDOW=92', b'92', b'71'])
        backend.query = lambda args: next(values)
        self.assertEqual(backend.guard(), (100, 200, 800, 600))
        values = iter([b'92', b'72'])
        backend.query = lambda args: next(values)
        with self.assertRaises(ValueError):
            backend.guard()

    def test_client_geometry_change_fails_before_canonical_measurement(self):
        backend = m.Backend.__new__(m.Backend)
        backend.request = {'pid': 71, 'window': 91}
        backend.frame_matches = lambda active, frame: True
        values = iter([b'92', b'71', b'X=102\nY=224\nWIDTH=800\nHEIGHT=600\nSCREEN=0\nWINDOW=92'])
        backend.query = lambda args: next(values)
        snapshots = iter([((100, 200), (2, 24), (800, 600), 1, 3),
                          ((101, 200), (2, 24), (800, 600), 1, 3)])
        backend.client_snapshot = lambda active: next(snapshots)
        with self.assertRaises(ValueError):
            backend.guard()

    def test_canonical_bounds_add_origin_once_and_reject_inconsistent(self):
        geometry = (100, 200, 800, 600)
        local = (10, 20, 40, 40)
        root = (110, 220, 40, 40)
        self.assertEqual(m.canonical_rectangle(local, local, geometry), root)
        self.assertEqual(m.canonical_rectangle(root, local, geometry), root)
        self.assertIsNone(m.canonical_rectangle((111, 220, 40, 40), local, geometry))
        with self.assertRaises(ValueError):
            m.canonical_rectangle(local, local, (2**31 - 1, 0, 10, 10))
        value = (62, (1 << 20) | (1 << 8) | (1 << 30), local, local)
        private = []
        result = m.measure(self.request(), Backend([value, value]), 10, lambda: 0, private)
        self.assertEqual(private, [dict(index=0, role=62, bounds=list(root), toggle='on')])
        self.assertNotIn('bounds', result)
        self.assertEqual(result['containmentRejected'], 0)

    def test_off_client_and_inconsistent_extents_never_enter_private_handoff(self):
        for screen, window in (((790, 20, 40, 40), (790, 20, 40, 40)),
                               ((111, 220, 40, 40), (10, 20, 40, 40))):
            value = (62, (1 << 20) | (1 << 8) | (1 << 30), screen, window)
            private = []
            result = m.measure(self.request(), Backend([value, value]),
                               10, lambda: 0, private)
            self.assertEqual(private, [])
            self.assertEqual(result['containmentRejected'], 1)

    def test_foreign_identity_or_moving_client_does_not_export_normalized_bounds(self):
        value = (62, (1 << 20) | (1 << 8) | (1 << 30),
                 (10, 20, 40, 40), (10, 20, 40, 40))
        for reads, geometries in (([None, None], [(100, 200, 800, 600)] * 2),
                                  ([value, value], [(100, 200, 800, 600),
                                                   (101, 200, 800, 600)])):
            backend = Backend(reads)
            geometry = iter(geometries)
            backend.guard = lambda: next(geometry)
            private = []
            result = m.measure(self.request(), backend, 10, lambda: 0, private)
            self.assertEqual(private, [])
            self.assertIn(result['status'], ('observed', 'guard-rejected'))

    def test_deadline_no_queries(self):
        result = m.measure(self.request(), Backend([]), 0, lambda: 1)
        self.assertEqual(result['status'], 'budget-exceeded')
    def test_disabled_and_geometry_rejections_are_distinct(self):
        value = (43, 0, (500, 500, 30, 30), (10, 20, 30, 30))
        result = m.measure(self.request(), Backend([value, value]), 10, lambda: 0)
        self.assertEqual(result['offsetInconsistent'], 1)
        self.assertEqual(result['stateRejected'], 1)
    def test_constructor_failure_and_query_failure_write_only_closed_receipt(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            environment = dict(NANH_ZED_PANEL_ZOOM='observe', GITHUB_ACTIONS='true',
                RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Linux',
                NANH_DESKTOP_QUALIFICATION_FACTS=str(root))
            with patch.dict(os.environ, environment), patch.object(m, 'Backend', side_effect=ImportError('PRIVATE')):
                self.assertEqual(m.run(json.dumps(self.request())), 0)
            result = json.loads(next(root.glob('*.json')).read_text())
            self.assertEqual(result['status'], 'unavailable')
            self.assertNotIn('PRIVATE', str(result))
            self.assertEqual(next(root.glob('*.json')).stat().st_mode & 0o777, 0o600)
            class FailedBackend:
                closed = False
                def __init__(self, request, deadline): pass
                def guard(self): raise ValueError('PRIVATE')
                def close(self): FailedBackend.closed = True
            with patch.dict(os.environ, environment), patch.object(m, 'Backend', FailedBackend):
                self.assertEqual(m.run(json.dumps(self.request())), 0)
            self.assertTrue(FailedBackend.closed)

    def test_duplicates_and_raw_payload_rejected(self):
        request = self.request()
        request['buttons'] *= 2
        with self.assertRaises(ValueError):
            m.validate(request)
        request = self.request()
        request['raw'] = 'PRIVATE'
        with self.assertRaises(ValueError):
            m.validate(request)


if __name__ == '__main__':
    unittest.main()
