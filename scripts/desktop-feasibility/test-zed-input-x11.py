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
native_snapshot = module['independent_client_snapshot']
native_package = module['coordinate_package']


class Transport(unittest.TestCase):
    def setUp(self):
        replacement = patch.dict(module['main'].__globals__,
            normalized_retry_point=lambda request, active, geometry, facts: (request['x'], request['y']),
            independent_client_origin=lambda active: (10, 30),
            independent_client_snapshot=lambda active: ((0,0),(0,0),(1280,800),77,88),
            coordinate_package=lambda deadline: 'noble-5build1',
            maximized_observation=lambda *args: None, publish_observation=lambda facts: None,
            pointer_child=lambda *args: 'unavailable')
        replacement.start()
        self.addCleanup(replacement.stop)

    def call(self, mode, payload=b''):
        stdin = io.TextIOWrapper(io.BytesIO(payload))
        with patch.object(sys, 'argv', ['helper', mode]), patch.object(sys, 'stdin', stdin):
            return module['main']()

    def test_decoration_candidate_is_inside_only_measured_held_frame(self):
        frame=((0,0),(0,0),(1280,1024),1,1)
        client=((1,20),(1,20),(1278,1003),1,40)
        self.assertEqual(module['decoration_crossing_point'](frame,client,40),(640,10))
        for changed in (((1,0),(1,0),(1278,1024),1,40),
                        ((1,20),(1,20),(1278,1003),1,41),
                        ((1,20),(2,20),(1278,1003),1,40),
                        ((1,20),(1,20),(1280,1003),1,40)):
            with self.assertRaises(ValueError):
                module['decoration_crossing_point'](frame,changed,40)
        with self.assertRaises(ValueError):
            module['decoration_crossing_point'](((0,0),(0,0),(1280,1024),1,50),client,40)

    def test_crossing_point_rejects_foreign_stack_or_offscreen_before_motion(self):
        owner=[40];size=[1280];closed=[]
        def translate(display,source,target,x,y,dx,dy,child):
            child._obj.value=owner[0] if target==1 else 0
            return 1
        def geometry(display,window,root,x,y,width,height,border,depth):
            root._obj.value=1;width._obj.value=size[0];height._obj.value=1024
            return 1
        def pointer(display,window,root,child,rx,ry,wx,wy,mask):
            root._obj.value=1;child._obj.value=0;rx._obj.value=640;ry._obj.value=10
            mask._obj.value=0;return 1
        lib=types.SimpleNamespace(XOpenDisplay=lambda *args:1,
            XCloseDisplay=lambda *args:closed.append(True),XTranslateCoordinates=translate,
            XGetGeometry=geometry,XQueryPointer=pointer)
        with patch.object(module['crossing_point_hit'].__globals__['ctypes'],'CDLL',return_value=lib):
            self.assertEqual(module['crossing_point_hit'](1,40,(640,10)),(0,(640,10),0))
            owner[0]=50
            with self.assertRaises(ValueError):module['crossing_point_hit'](1,40,(640,10))
            owner[0]=40;size[0]=100
            with self.assertRaises(ValueError):module['crossing_point_hit'](1,40,(640,10))
        self.assertEqual(len(closed),3)

    def test_crossing_reproves_each_motion_and_never_replays_failed_action(self):
        events=[];clock=[0]
        def prove(point,after):events.append(('proof',point,after))
        def move(point):events.append(('move',point))
        motion=module['guarded_crossing_motion']
        for point in ((640,10),(400,200)):
            motion(point,move,prove,4,now=lambda:clock[0])
        self.assertEqual(events,[('proof',(640,10),False),('move',(640,10)),
            ('proof',(640,10),True),('proof',(400,200),False),('move',(400,200)),
            ('proof',(400,200),True)])
        events.clear()
        def rejected(point,after):
            events.append(('proof',after));raise ValueError('closed synthetic owner loss')
        with self.assertRaises(ValueError):motion((640,10),move,rejected,4,now=lambda:0)
        self.assertEqual(events,[('proof',False)])
        events.clear()
        def expiring(point,after):clock[0]=4
        with self.assertRaises(ValueError):motion((640,10),move,expiring,4,now=lambda:clock[0])
        self.assertEqual(events,[])
        clock[0]=0
        def uncertain(point):events.append(('move',point));clock[0]=4
        with self.assertRaises(ValueError):motion((640,10),uncertain,prove,4,now=lambda:clock[0])
        self.assertEqual([event for event in events if event[0]=='move'],[('move',(640,10))])

    def test_crossing_opt_in_rejects_unowned_policy_before_any_transport(self):
        request=json.dumps(dict(pid=20,window=40,x=100,y=200,bus=':1.2',
            path='/org/a11y/atspi/accessible/3')).encode()
        with patch.dict(os.environ,{'NANH_ZED_ENTER_POLICY':'owned-decoration-crossing',
                'GITHUB_ACTIONS':'false'}),patch('subprocess.run') as transport:
            self.assertEqual(self.call('retry-click',request),18)
            transport.assert_not_called()

    def test_source_on_proof_requires_enabled_visible_live_toggle(self):
        required = sum(1 << bit for bit in (8, 20, 25, 30))
        self.assertTrue(module['enabled_toggle_on']((required, 0)))
        for bit in (8, 20, 25, 30):
            self.assertFalse(module['enabled_toggle_on']((required & ~(1 << bit), 0)))
        for words in ((required | (1 << 6), 0), (required,), (required, 0, 0), (required, -1)):
            self.assertFalse(module['enabled_toggle_on'](words))

    def test_zoom_hover_retains_exact_owned_toggle_and_never_clicks(self):
        request = dict(pid=20, window=40, x=25, y=35, bus=':1.2',
                       path='/org/a11y/atspi/accessible/3', bounds=[10, 20, 30, 30])
        for changed, expected, motions in [(False, 0, 1), (True, 3, 1)]:
            queries, actions = [], []
            def proof(*args, **kwargs):
                queries.append(kwargs)
                return (11 if changed and len(queries) > 1 else 10, 20, 30, 30)
            def read(args, **kwargs):
                return b'40' if args[1] == 'getactivewindow' else b'20'
            with patch.dict(module['zoom_hover'].__globals__, normalized_retry_point=proof,
                            owned_frame=lambda *args: True), \
                    patch.object(subprocess, 'check_output', side_effect=read), \
                    patch.object(subprocess, 'run', side_effect=lambda args, **kwargs: actions.append(args)):
                self.assertEqual(self.call('zoom-hover', json.dumps(request).encode()), expected)
            self.assertEqual(len(actions), motions)
            self.assertTrue(all(action[1] == 'mousemove' for action in actions))
            self.assertTrue(all(query['toggle'] and query['hit_point'] == (25, 35) for query in queries))

    def test_zoom_hover_rejects_malformed_and_unowned_without_input(self):
        request = dict(pid=20, window=40, x=25, y=35, bus=':1.2',
                       path='/org/a11y/atspi/accessible/3', bounds=[10, 20, 30, 30])
        with patch.object(subprocess, 'run') as action:
            self.assertEqual(self.call('zoom-hover', b'{}'), 2)
            with patch.object(subprocess, 'check_output', return_value=b'40'), \
                    patch.dict(module['zoom_hover'].__globals__, owned_frame=lambda *args: False):
                self.assertEqual(self.call('zoom-hover', json.dumps(request).encode()), 3)
            action.assert_not_called()

    def test_corrected_transport_clicks_once_at_measured_client_point_or_not_at_all(self):
        request = json.dumps(dict(pid=20,window=40,x=137,y=269,bus=':1.2',path='/org/a11y/atspi/accessible/3')).encode()
        for case, package, expected_clicks in [('correct','noble-5build1',1),('unknown-package','unverified',0),('changed-before-click','noble-5build1',0)]:
            actions, pointer = [], [0,0]
            snapshots = [0]
            def snapshot(active):
                snapshots[0] += 1
                return ((101 if case=='changed-before-click' and snapshots[0]>2 else 100,200),(2,24),(800,600),77,88)
            def execute(args,**kwargs):
                if args[1]=='mousemove': pointer[:]=map(int,args[3:]);actions.append(args)
                if args[1]=='click': actions.append(args)
                output = (b'X=102\nY=224\nSCREEN=0\nWINDOW=41\nWIDTH=800\nHEIGHT=600' if args[1]=='getwindowgeometry'
                    else ('X=%s\nY=%s\nSCREEN=0\nWINDOW=41'%tuple(pointer)).encode() if args[1]=='getmouselocation'
                    else b'41' if args[1]=='getactivewindow' else b'20')
                return subprocess.CompletedProcess(args,0,stdout=output)
            with patch('subprocess.run',side_effect=execute), patch.dict(module['main'].__globals__,
                owned_frame=lambda a,b:a==41 and b==40,
                independent_client_snapshot=snapshot,
                coordinate_package=lambda deadline:package,
                normalized_retry_point=lambda request,active,geometry,facts=None:module['coordinate_point']((20,40,30,10),(20,40,30,10),geometry)):
                self.assertEqual(self.call('retry-click',request),0 if expected_clicks else 18)
            clicks=[a for a in actions if a[1]=='click']
            self.assertEqual(len(clicks),expected_clicks)
            if expected_clicks:self.assertEqual(pointer,[135,245])
            elif case=='unknown-package':self.assertEqual(actions,[])
            else:self.assertEqual([a[1] for a in actions],['mousemove'])

    def test_origin_authority_corrects_only_pinned_measured_parent_offset(self):
        authority = module['geometry_authority']
        snapshot = ((100,200),(2,24),(800,600),77,88)
        self.assertEqual(authority((102,224,800,600),snapshot,snapshot,'noble-5build1'),
                         ((100,200,800,600),'parent-offset','verified-xtranslate'))
        self.assertEqual(authority((100,200,800,600),snapshot,snapshot,'unverified'),
                         ((100,200,800,600),'equal','unchanged-xdotool'))
        for geometry, before, after, package in [
            ((102,224,800,600),snapshot,snapshot,'unverified'),
            ((103,224,800,600),snapshot,snapshot,'noble-5build1'),
            ((102,224,801,600),snapshot,snapshot,'noble-5build1'),
            ((102,224,800,600),snapshot,((101,200),*snapshot[1:]),'noble-5build1'),
            ((102,224,800,600),(*snapshot[:4],77),(*snapshot[:4],77),'noble-5build1'),
        ]:
            with self.assertRaises(ValueError): authority(geometry,before,after,package)

    def test_package_proof_requires_both_exact_versions_and_original_deadline(self):
        env = dict(GITHUB_ACTIONS='true',RUNNER_ENVIRONMENT='github-hosted',RUNNER_OS='Linux')
        with patch.dict(os.environ,env), patch('time.monotonic',return_value=1):
            for output, expected in [(b'libxdo3 1:3.20160805.1-5build1\nxdotool 1:3.20160805.1-5build1\n','noble-5build1'),
                    (b'libxdo3 1:3.20160805.1-5build1\nxdotool NEW\n','unverified'),
                    (b'xdotool 1:3.20160805.1-5build1\n','unverified'),(b'PRIVATE'*30,'unverified')]:
                with patch('subprocess.run',return_value=subprocess.CompletedProcess([],0,stdout=output)):
                    self.assertEqual(native_package(2),expected)
            with patch('subprocess.run') as query:
                with self.assertRaises(subprocess.TimeoutExpired): native_package(1)
                query.assert_not_called()

    def test_exact_retry_offset_relation_never_double_translates(self):
        relation = module['retry_offset_relation']
        screen = (20, 40, 30, 10)
        geometry = (100, 200, 800, 600)
        self.assertEqual(relation(screen, screen, geometry), 'missing-origin')
        self.assertEqual(relation((120, 240, 30, 10), screen, geometry), 'expected-origin')
        self.assertEqual(relation((121, 240, 30, 10), screen, geometry), 'inconsistent')
        self.assertEqual(module['coordinate_point'](screen, screen, geometry), (135, 245))

    def test_independent_translation_uses_client_zero_origin_and_releases_display(self):
        closed = []
        class Function:
            def __init__(self, call): self.call = call
            def __call__(self, *args): return self.call(*args)
        def tree(*args):
            args[2]._obj.value = 77
            args[3]._obj.value = 88
            return 1
        def translate(*args):
            self.assertEqual(args[1:5], (90, 77, 0, 0))
            args[5]._obj.value, args[6]._obj.value = 100, 200
            return 1
        def geometry(*args):
            args[2]._obj.value = 77
            args[5]._obj.value, args[6]._obj.value = 800, 600
            return 1
        library = types.SimpleNamespace(XGetGeometry=Function(geometry), XOpenDisplay=Function(lambda _: 1),
            XCloseDisplay=Function(lambda _: closed.append(True)), XFree=Function(lambda _: None),
            XQueryTree=Function(tree), XTranslateCoordinates=Function(translate))
        with patch('ctypes.CDLL', return_value=library):
            self.assertEqual(native_snapshot(90)[0], (100, 200))
        self.assertEqual(closed, [True])

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

    def test_panel_zoom_uses_the_private_public_action_binding(self):
        self.assertEqual(module['KEYS']['panel-zoom'], 'ctrl+alt+z')
        with patch('subprocess.run', return_value=subprocess.CompletedProcess([], 0)) as run:
            self.assertEqual(self.call('panel-zoom'), 0)
            self.assertEqual(run.call_count, 1)
            self.assertEqual(run.call_args.args[0],
                             ['/usr/bin/xdotool', 'key', '--clearmodifiers', 'ctrl+alt+z'])

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

    def test_hover_failure_records_before_motion_and_cleans_without_click(self):
        events=[]
        class Observer:
            def __init__(self,*args,**kwargs):
                events.append('armed');self.result={'status':'unavailable'}
            def finish(self):events.append('finish');return self.result
            def close(self):events.append('close')
        class Cursor:
            def __init__(self,*args):pass
            def matches(self,*args):return False
            def close(self):events.append('cursor-close')
        def sampler(path):
            name=Path(path).name
            if name=='zed-xrecord-supervisor.py':return {'Observer':Observer}
            if name=='zed-cursor-hit.py':return {'PointerShape':Cursor}
            if name=='zed-atspi-observe.py':return dict(ancestor_result=lambda:{},compare_ancestors=lambda *args:{})
            return dict(capture=lambda *args:{})
        def execute(args,**kwargs):
            if args[1]=='click':events.append('click')
            output=(b'X=0\nY=0\nSCREEN=0\nWINDOW=40\nWIDTH=1280\nHEIGHT=800'
                    if args[1]=='getwindowgeometry' else b'40' if args[1]=='getactivewindow' else b'20')
            return subprocess.CompletedProcess(args,0,stdout=output)
        def hover(select,*args):
            events.append('hover')
            raise ValueError('closed synthetic failure')
        request=json.dumps(dict(pid=20,window=40,x=100,y=200,bus=':1.2',
            path='/org/a11y/atspi/accessible/3')).encode()
        with patch.dict(os.environ,{'NANH_ZED_XRECORD':'1','NANH_ZED_CURSOR_HIT':'1',
                'GITHUB_ACTIONS':'true','RUNNER_ENVIRONMENT':'github-hosted','RUNNER_OS':'Linux'}), \
             patch.object(sys,'platform','linux'),patch('subprocess.run',side_effect=execute), \
             patch('runpy.run_path',side_effect=sampler), \
             patch.dict(module['main'].__globals__,owned_frame=lambda *args:True,
                maximized_observation=lambda *args:None,
                independent_client_snapshot=lambda *args:((0,0),(0,0),(1280,800)),
                normalized_retry_point=lambda *args,**kwargs:(100,200),
                select_with_ancestor_diagnostic=hover,publish_observation=lambda *args:None):
            self.assertNotEqual(self.call('retry-click',request),0)
        self.assertEqual(events,['armed','hover','finish','cursor-close','close'])

    def test_recording_budget_exhaustion_is_closed_without_suppressing_one_click(self):
        request = json.dumps(dict(pid=20, window=40, x=100, y=200,
            bus=':1.2', path='/org/a11y/atspi/accessible/3')).encode()
        clock, observations, clicks = [0], [], []
        def readonly_sampler(path):
            self.assertEqual(Path(path).name, 'zed-atspi-observe.py')
            unknown = dict(centerWithinPublishedAncestors=None,
                ancestorBoundsStatus='unavailable', checkedAncestorCount=0)
            return dict(retry_ancestors=lambda *args:(unknown, None),
                ancestor_result=lambda:unknown, compare_ancestors=lambda *args:unknown)
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
             patch('runpy.run_path', side_effect=readonly_sampler) as spawn, \
             patch.dict(module['main'].__globals__,
                normalized_retry_point=lambda request, active, geometry, facts=None: (100,200),
                pointer_child=lambda *args:'client',
                publish_observation=lambda facts:observations.append(facts)):
            self.assertEqual(self.call('retry-click', request),0)
            self.assertEqual(spawn.call_count, 1)
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
                         ['getactivewindow', 'getwindowpid', 'getwindowgeometry', 'getactivewindow', 'getwindowpid',
                          'mousemove', 'getmouselocation', 'getactivewindow', 'getwindowpid',
                          'getwindowgeometry', 'getactivewindow', 'getwindowpid', 'click'])
        self.assertEqual(calls[-1], ['/usr/bin/xdotool', 'click', '--clearmodifiers', '1'])
        with patch.dict(module['main'].__globals__, owned_frame=lambda a, b: a == b), patch('subprocess.run', return_value=subprocess.CompletedProcess([], 0, stdout=b'99')) as run:
            self.assertEqual(self.call('retry-click', request), 11)
            self.assertEqual(run.call_count, 1)
        with patch.dict(module['main'].__globals__, owned_frame=lambda a, b: a == b), patch('subprocess.run', side_effect=[
                subprocess.CompletedProcess([], 0, stdout=b'40'),
                subprocess.CompletedProcess([], 0, stdout=b'20'),
                subprocess.CompletedProcess([], 0, stdout=b'X=0\nY=0\nSCREEN=0\nWINDOW=40\nWIDTH=1280\nHEIGHT=800'),
                subprocess.CompletedProcess([], 0, stdout=b'40'),
                subprocess.CompletedProcess([], 0, stdout=b'20'),
                subprocess.CompletedProcess([], 0),
                subprocess.CompletedProcess([], 0, stdout=b'X=100\nY=200\nSCREEN=0\nWINDOW=40'),
                subprocess.CompletedProcess([], 0, stdout=b'99')]) as run:
            self.assertEqual(self.call('retry-click', request), 11)
            self.assertEqual(run.call_count, 8)

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
    def test_pointer_mask_preserves_configurable_modifiers_and_unknown_bits(self):
        classify = module['pointer_mask']
        for mask, expected in ((0, ('none', False)), (1, ('shift', False)),
                               (2, ('lock', False)), (4, ('control', False)),
                               (8, ('other-modifier', False)), (5, ('mixed', False)),
                               (256, ('none', True)), (4096 | 4, ('control', True)),
                               (8192, ('unknown', None)), (-1, ('unknown', None)),
                               (True, ('unknown', None))):
            self.assertEqual(classify(mask), expected)

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
                args[8]._obj.value = 4 | 256
                return 1
            xlib = types.SimpleNamespace(XOpenDisplay=Function(lambda _: 1),
                XCloseDisplay=Function(lambda _: closed.append(True)), XQueryPointer=Function(query))
            with patch('ctypes.CDLL', return_value=xlib), patch.dict(module['pointer_child'].__globals__,
                    owned_frame=lambda candidate, active: candidate == 41 and active == 40):
                facts = module['pointer_observation']()
                self.assertEqual(module['pointer_child'](50, 40, facts), expected)
                self.assertEqual((facts['modifierState'], facts['buttonsHeld']), ('control', True))
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


class AncestorScanDiagnostic(unittest.TestCase):
    def test_no_hit_retains_error_and_records_stable_published_ancestry(self):
        facts = {'cursorSelection': {'status': 'no-hit'}}
        error = ValueError('fixed rejection')
        observations = []
        def observe():
            observations.append(True)
            return ('closed', 'private in-memory signature')
        def reject():
            raise error
        with self.assertRaises(ValueError) as caught:
            module['select_with_ancestor_diagnostic'](reject, observe,
                lambda a, b: {'ancestorBoundsStatus': 'complete',
                    'centerWithinPublishedAncestors': False}, lambda: True,
                facts, module['time'].monotonic() + 1)
        self.assertIs(caught.exception, error)
        self.assertEqual(len(observations), 2)
        self.assertFalse(facts['centerWithinPublishedAncestors'])
        self.assertNotIn('private', json.dumps(facts))

    def test_identity_failure_skips_post_scan_and_never_becomes_a_hit(self):
        facts = {'cursorSelection': {'status': 'identity-rejected'}}
        reads = []
        def reject():
            raise module['RetryHitFailure']('pointer-child')
        with self.assertRaises(ValueError):
            module['select_with_ancestor_diagnostic'](reject,
                lambda: reads.append(True), lambda a,b: {}, lambda: True,
                facts, module['time'].monotonic() + 1)
        self.assertEqual(reads, [True])
        self.assertNotIn('ancestorBoundsStatus', facts)

    def test_expired_or_lost_scope_does_not_query_and_diagnostic_error_keeps_rejection(self):
        for scope, deadline in [(lambda: False, module['time'].monotonic()+1),
                                (lambda: True, 0)]:
            reads = []
            with self.assertRaisesRegex(ValueError, 'original'):
                module['select_with_ancestor_diagnostic'](
                    lambda: (_ for _ in ()).throw(ValueError('original')),
                    lambda: reads.append(True), lambda a,b: {}, scope,
                    {'cursorSelection': {'status':'no-hit'}}, deadline)
            self.assertEqual(reads, [])
        with self.assertRaisesRegex(ValueError, 'original'):
            module['select_with_ancestor_diagnostic'](
                lambda: (_ for _ in ()).throw(ValueError('original')),
                lambda: (_ for _ in ()).throw(OSError('PRIVATE')),
                lambda a,b: {}, lambda: True,
                {'cursorSelection': {'status':'no-hit'}}, module['time'].monotonic()+1)

    def test_success_keeps_selected_point_and_compares_both_samples(self):
        facts, reads = {}, []
        point = module['select_with_ancestor_diagnostic'](lambda: (10,20),
            lambda: reads.append(True) or len(reads),
            lambda a,b: {'ancestorBoundsStatus': 'unavailable' if a != b else 'complete'},
            lambda: True, facts, module['time'].monotonic()+1)
        self.assertEqual(point, (10,20))
        self.assertEqual(reads, [True,True])
        self.assertEqual(facts['ancestorBoundsStatus'], 'unavailable')


if __name__ == '__main__':
    unittest.main()
