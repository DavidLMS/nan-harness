#!/usr/bin/env python3
"""The native key helper accepts fixed actions only and never echoes content."""
import io
import json
import os
import runpy
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import types
from unittest.mock import patch

module = runpy.run_path(str(Path(__file__).with_name('zed-input-x11.py')))


class Transport(unittest.TestCase):
    def setUp(self):
        replacement = patch.dict(module['main'].__globals__,
            normalized_retry_point=lambda request, active, geometry, facts: (request['x'], request['y']),
            maximized_observation=lambda *args: None, publish_observation=lambda facts: None,
            pointer_child=lambda *args: 'unavailable')
        replacement.start()
        self.addCleanup(replacement.stop)

    def call(self, mode, payload=b''):
        stdin = io.TextIOWrapper(io.BytesIO(payload))
        with patch.object(sys, 'argv', ['helper', mode]), patch.object(sys, 'stdin', stdin):
            return module['main']()

    def test_readonly_sampler_mode_never_dispatches_native_input(self):
        with patch('runpy.run_path', return_value={'run': lambda payload: 0}) as sampler, patch('subprocess.run') as run:
            self.assertEqual(self.call('atspi-observe', b'{}'), 0)
            sampler.assert_called_once()
            run.assert_not_called()
        with patch('runpy.run_path') as sampler:
            self.assertEqual(self.call('atspi-observe', b'x' * 32769), 2)
            sampler.assert_not_called()

    def test_foreign_keys_or_payload_cannot_send_input(self):
        with patch('subprocess.run') as run:
            self.assertEqual(self.call('arbitrary-command'), 2)
            self.assertEqual(self.call('submit', b'private synthetic prompt'), 2)
            run.assert_not_called()

    def test_fixed_actions_are_bounded_and_silent(self):
        with patch('subprocess.run', return_value=subprocess.CompletedProcess([], 0)) as run:
            for mode, key in module['KEYS'].items():
                self.assertEqual(self.call(mode), 0)
                args, kwargs = run.call_args
                self.assertEqual(args[0], ['/usr/bin/xdotool', 'key', '--clearmodifiers', key])
                self.assertEqual(kwargs['timeout'], 2)
                self.assertEqual(kwargs['stdout'], subprocess.DEVNULL)
                self.assertEqual(kwargs['stderr'], subprocess.DEVNULL)
        with patch('subprocess.run', side_effect=subprocess.TimeoutExpired('fixed-helper', 2)):
            self.assertEqual(self.call('submit'), 3)

    def test_recording_budget_exhaustion_is_closed_without_suppressing_one_click(self):
        request = json.dumps(dict(pid=20, window=40, x=100, y=200,
            bus=':1.2', path='/org/a11y/atspi/accessible/3')).encode()
        clock, observations, clicks = [0], [], []
        def execute(args, **kwargs):
            if args[1] == 'getmouselocation':
                clock[0] = 3.2
            if args[1] == 'click':
                clicks.append(args)
            output = (b'X=0\nY=0\nSCREEN=0\nWINDOW=40\nWIDTH=1280\nHEIGHT=800'
                if args[1] == 'getwindowgeometry' else
                b'X=100\nY=200\nSCREEN=0\nWINDOW=40'
                if args[1] == 'getmouselocation' else
                b'40\n' if args[1] == 'getactivewindow' else b'20\n')
            return subprocess.CompletedProcess(args, 0, stdout=output)
        with patch.dict(os.environ, {'NANH_ZED_XRECORD':'1', 'GITHUB_ACTIONS':'true',
                'RUNNER_ENVIRONMENT':'github-hosted', 'RUNNER_OS':'Linux'}), \
             patch.object(sys, 'platform', 'linux'), \
             patch('time.monotonic', side_effect=lambda: clock[0]), \
             patch('subprocess.run', side_effect=execute), \
             patch('runpy.run_path') as spawn, \
             patch.dict(module['main'].__globals__,
                normalized_retry_point=lambda request, active, geometry, facts=None: (100,200),
                pointer_child=lambda *args:'client',
                publish_observation=lambda facts:observations.append(facts)):
            self.assertEqual(self.call('retry-click', request),0)
            spawn.assert_not_called()
        self.assertEqual(len(clicks),1)
        self.assertEqual(observations[0]['inputDelivery'], dict(status='unavailable',
            stage='budget-insufficient',pressCount=None,releaseCount=None,orderedPair=None))

    def test_pointer_checks_foreground_before_one_activation(self):
        request = json.dumps(dict(pid=20, window=40, x=100, y=200, bus=':1.2', path='/org/a11y/atspi/accessible/3')).encode()
        calls = []
        def execute(args, **kwargs):
            calls.append(args)
            self.assertLessEqual(kwargs['timeout'], 4)
            self.assertEqual(kwargs['stderr'], subprocess.DEVNULL)
            output = b'X=0\nY=0\nSCREEN=0\nWINDOW=40\nWIDTH=1280\nHEIGHT=800' if args[1] == 'getwindowgeometry' else b'X=100\nY=200\nSCREEN=0\nWINDOW=40' if args[1] == 'getmouselocation' else b'40\n' if args[1] == 'getactivewindow' else b'20\n'
            return subprocess.CompletedProcess(args, 0, stdout=output)
        with patch('subprocess.run', side_effect=execute):
            self.assertEqual(self.call('retry-click', request), 0)
        self.assertEqual([args[1] for args in calls],
                         ['getactivewindow', 'getwindowpid', 'getwindowgeometry', 'mousemove', 'getmouselocation',
                          'getactivewindow', 'getwindowpid', 'click'])
        self.assertEqual(calls[-1], ['/usr/bin/xdotool', 'click', '--clearmodifiers', '1'])
        with patch.dict(module['main'].__globals__, owned_frame=lambda a, b: a == b), patch('subprocess.run', return_value=subprocess.CompletedProcess([], 0, stdout=b'99')) as run:
            self.assertEqual(self.call('retry-click', request), 11)
            self.assertEqual(run.call_count, 1)
        with patch.dict(module['main'].__globals__, owned_frame=lambda a, b: a == b), patch('subprocess.run', side_effect=[
                subprocess.CompletedProcess([], 0, stdout=b'40'),
                subprocess.CompletedProcess([], 0, stdout=b'20'),
                subprocess.CompletedProcess([], 0, stdout=b'X=0\nY=0\nSCREEN=0\nWINDOW=40\nWIDTH=1280\nHEIGHT=800'),
                subprocess.CompletedProcess([], 0),
                subprocess.CompletedProcess([], 0, stdout=b'X=100\nY=200\nSCREEN=0\nWINDOW=40'),
                subprocess.CompletedProcess([], 0, stdout=b'99')]) as run:
            self.assertEqual(self.call('retry-click', request), 11)
            self.assertEqual(run.call_count, 6)

    def test_active_client_must_have_the_exact_owned_frame_as_ancestor(self):
        matches = module['matches_owned_frame']
        self.assertTrue(matches(41, 40, lambda window: (1, 40)))
        self.assertFalse(matches(41, 40, lambda window: (1, 1)))
        self.assertFalse(matches(41, 40, lambda window: (1, 41)))
        calls = []
        def unbounded(window):
            calls.append(window)
            return 1, window + 1
        self.assertFalse(matches(41, 99, unbounded))
        self.assertEqual(len(calls), 16)

    def test_pointer_failure_stage_never_replays_input(self):
        request = json.dumps(dict(pid=20, window=40, x=100, y=200, bus=':1.2', path='/org/a11y/atspi/accessible/3')).encode()
        for failed, code in [('getactivewindow', 13), ('mousemove', 14), ('click', 16)]:
            calls = []
            def execute(args, **kwargs):
                calls.append(args[1])
                if args[1] == failed:
                    raise subprocess.TimeoutExpired('fixed-helper', 2)
                output = b'X=0\nY=0\nSCREEN=0\nWINDOW=40\nWIDTH=1280\nHEIGHT=800' if args[1] == 'getwindowgeometry' else b'X=100\nY=200\nSCREEN=0\nWINDOW=40' if args[1] == 'getmouselocation' else b'40' if args[1] == 'getactivewindow' else b'20'
                return subprocess.CompletedProcess(args, 0, stdout=output)
            with patch('subprocess.run', side_effect=execute):
                self.assertEqual(self.call('retry-click', request), code)
            self.assertLessEqual(calls.count('click'), 1)
            self.assertEqual(calls[-1], failed)

    def test_coordinate_conversion_accepts_only_owned_client_geometry(self):
        point = module['coordinate_point']
        window = (100, 200, 40, 20)
        geometry = (10, 30, 800, 600)
        self.assertEqual(point(window, window, geometry), (130, 240))
        self.assertEqual(point((110, 230, 40, 20), window, geometry), (130, 240))
        for screen, local in [((111, 230, 40, 20), window), (window, (-1, 200, 40, 20)),
                              (window, (790, 200, 40, 20))]:
            with self.assertRaises(ValueError):
                point(screen, local, geometry)

    def test_invalid_pointer_data_never_reaches_native_input(self):
        request = dict(pid=20, window=40, x=100, y=200, bus=':1.2', path='/org/a11y/atspi/accessible/3')
        with patch('subprocess.run') as run:
            for changed in ({**request, 'pid': True}, {**request, 'window': 0},
                            {**request, 'x': 32768}, {**request, 'command': 'PRIVATE'}):
                self.assertNotEqual(self.call('retry-click', json.dumps(changed).encode()), 0)
            self.assertNotEqual(self.call('retry-click', b'x' * 4097), 0)
            run.assert_not_called()


class AccessibilityIdentity(unittest.TestCase):
    def test_pointer_child_distinguishes_client_from_frame_decoration(self):
        class Function:
            def __init__(self, callback):
                self.callback = callback
            def __call__(self, *args):
                return self.callback(*args)
        for child, expected in [(40, 'client'), (0, 'decoration-or-empty'), (41, 'client-descendant'), (99, 'other')]:
            closed = []
            def query(*args):
                args[3]._obj.value = child
                return 1
            xlib = types.SimpleNamespace(XOpenDisplay=Function(lambda _: 1),
                XCloseDisplay=Function(lambda _: closed.append(True)), XQueryPointer=Function(query))
            with patch('ctypes.CDLL', return_value=xlib), patch.dict(module['pointer_child'].__globals__,
                    owned_frame=lambda candidate, active: candidate == 41 and active == 40):
                self.assertEqual(module['pointer_child'](50, 40), expected)
            self.assertEqual(closed, [True])

    def test_closed_receipt_requires_hosted_metadata_and_private_new_file(self):
        publish = module['publish_observation']
        with tempfile.TemporaryDirectory() as tmp:
            facts = module['pointer_observation']()
            environment = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted',
                               RUNNER_OS='Linux', NANH_DESKTOP_QUALIFICATION_FACTS=tmp)
            with patch.dict(os.environ, environment, clear=True):
                publish(facts)
            receipts = list(Path(tmp).glob('*.json'))
            self.assertEqual(len(receipts), 1)
            self.assertEqual(json.loads(receipts[0].read_text()), facts)
            self.assertEqual(receipts[0].stat().st_mode & 0o777, 0o600)
            for changed in ({**environment, 'RUNNER_ENVIRONMENT': 'self-hosted'},
                            {**environment, 'GITHUB_ACTIONS': 'false'},
                            {**environment, 'RUNNER_OS': 'Windows'}):
                with patch.dict(os.environ, changed, clear=True):
                    publish(facts)
            self.assertEqual(len(list(Path(tmp).glob('*.json'))), 1)

    def test_observation_reduces_native_state_and_never_exposes_coordinates(self):
        facts = module['pointer_observation']()
        calls = []
        component = types.SimpleNamespace(GetState=lambda **kwargs: [(1 << 8) | (1 << 24) | (1 << 25) | (1 << 30), 0],
            Contains=lambda *args, **kwargs: calls.append(args) or True)
        module['accessibility_observation'](component,
            types.SimpleNamespace(Int32=int, UInt32=int, DBusException=RuntimeError),
            (100, 200, 40, 20), facts)
        self.assertTrue(all(facts[key] for key in ('enabled', 'sensitive', 'showing', 'visible', 'retryContains')))
        self.assertFalse(facts['defunct'])
        self.assertEqual(calls, [(120, 210, 1)])
        self.assertNotIn('120', json.dumps(facts))
        component.GetState = lambda **kwargs: [1 << 7, 0]
        editable = module['pointer_observation']()
        module['accessibility_observation'](component,
            types.SimpleNamespace(Int32=int, UInt32=int, DBusException=RuntimeError),
            (100, 200, 40, 20), editable)
        self.assertFalse(editable['enabled'])
        component.GetState = lambda **kwargs: (_ for _ in ()).throw(RuntimeError('PRIVATE'))
        unavailable = module['pointer_observation']()
        module['accessibility_observation'](component,
            types.SimpleNamespace(DBusException=RuntimeError), (0, 0, 1, 1), unavailable)
        self.assertIsNone(unavailable['enabled'])
        self.assertNotIn('PRIVATE', json.dumps(unavailable))

    def test_maximization_uses_only_the_fixed_property(self):
        for output, expected in [(b'_NET_WM_STATE(ATOM) = _NET_WM_STATE_MAXIMIZED_VERT, _NET_WM_STATE_MAXIMIZED_HORZ\n', True),
                                 (b'_NET_WM_STATE(ATOM) = \n', False),
                                 (b'PRIVATE\n', None)]:
            facts = module['pointer_observation']()
            with patch('subprocess.run', return_value=subprocess.CompletedProcess([], 0, stdout=output)) as run:
                module['maximized_observation'](40, facts, module['time'].monotonic() + 1)
            self.assertIs(facts['maximizedHorizontal'], expected)
            self.assertIs(facts['maximizedVertical'], expected)
            self.assertEqual(run.call_args.args[0], ['/usr/bin/xprop', '-id', '40', '_NET_WM_STATE'])
            self.assertNotIn('PRIVATE', json.dumps(facts))

    def test_changed_retry_identity_or_failed_queries_never_supply_coordinates(self):
        class QueryFailure(Exception):
            pass
        request = dict(pid=20, bus=':1.2', path='/org/a11y/atspi/accessible/3')
        for owner, role, name, failed, accepted in [
                (20, 43, 'Retry', False, True),
                (21, 43, 'Retry', False, False),
                (20, 42, 'Retry', False, False),
                (20, 43, 'PRIVATE_OTHER_CONTROL', False, False),
                (20, 43, 'Retry', True, False)]:
            closed = []
            component = types.SimpleNamespace(
                GetRole=lambda **kwargs: role,
                Get=lambda *args, **kwargs: name,
                GetExtents=lambda *args, **kwargs: (100, 200, 40, 20))
            def query(*args):
                if failed:
                    raise QueryFailure('PRIVATE_QUERY_DETAIL')
                return component
            bus = types.SimpleNamespace(
                get_object=lambda destination, path: types.SimpleNamespace(
                    GetConnectionUnixProcessID=lambda *args, **kwargs: owner)
                    if destination == 'org.freedesktop.DBus' else query(),
                close=lambda: closed.append(True))
            session = types.SimpleNamespace(get_object=lambda *args: types.SimpleNamespace(
                GetAddress=lambda **kwargs: 'PRIVATE_BUS_ADDRESS'))
            fake = types.SimpleNamespace(SessionBus=lambda: session,
                bus=types.SimpleNamespace(BusConnection=lambda address: bus),
                UInt32=int, DBusException=QueryFailure)
            with patch.dict(sys.modules, dbus=fake):
                if accepted:
                    self.assertEqual(module['normalized_retry_point'](request, 40,
                        (10, 30, 800, 600)), (130, 240))
                else:
                    with self.assertRaises(ValueError) as failure:
                        module['normalized_retry_point'](request, 40, (10, 30, 800, 600))
                    self.assertNotIn('PRIVATE', str(failure.exception))
            self.assertEqual(closed, [True])


if __name__ == '__main__':
    unittest.main()
