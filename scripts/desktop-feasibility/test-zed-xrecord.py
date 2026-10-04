import struct, sys, unittest
from pathlib import Path
import runpy
from unittest import mock
from types import SimpleNamespace
import json
module = runpy.run_path(str(Path(__file__).with_name('zed-xrecord.py')))
Counts, NativeRecorder, Unavailable = (module[name] for name in ('Counts', 'NativeRecorder', 'Unavailable'))

def event(kind=4, window=90, stamp=100, device=2):
    return struct.pack(('<' if sys.byteorder == 'little' else '>') + 'BBHIHHIIIII', 35, 131, 0, 12, kind, device, stamp, 1, 1, window, 0)

def crossing(kind=7, window=90, mode=0, detail=2, source=3, length=10):
    return struct.pack(('<' if sys.byteorder == 'little' else '>') + 'BBHIHHIHBBIII',
        35,131,0,length,kind,2,100,source,mode,detail,1,window,0)

class Tests(unittest.TestCase):

    def test_header_only_owned_crossings_and_motion_preserve_normal_inferior(self):
        c=Counts(90,131,123)
        for packet in (crossing(detail=2),crossing(mode=4),crossing(kind=8),event(kind=6),crossing(window=91)):
            c.accept(0,False,123,packet)
        self.assertEqual(c.closed()['crossingHeaders'],dict(status='observed',ownedNormalEnterCount=1,
            ownedNonNormalEnterCount=1,ownedNormalLeaveCount=1,ownedMotionCount=1))
        self.assertFalse(c.closed()['orderedPair'])
        self.assertNotIn('90',json.dumps(c.closed()))
        absent=Counts(90,131,123).closed()['crossingHeaders']
        self.assertEqual(absent['ownedNormalEnterCount'],0)  # absence is no consumption proof
        self.assertNotIn('focused',absent)

    def test_invalid_or_truncated_crossing_headers_never_emit_invented_fields(self):
        for packet in (crossing()[:-1],crossing()+b'PRIVATE',crossing(mode=6),crossing(detail=8),
                       crossing(source=0),crossing(length=9)):
            c=Counts(90,131,123);c.accept(0,False,123,packet)
            result=c.closed()['crossingHeaders']
            self.assertEqual(result['status'],'unavailable')
            self.assertTrue(all(v is None for k,v in result.items() if k!='status'))
        c=Counts(90,131,123)
        for _ in range(65):c.accept(0,False,123,crossing())
        self.assertEqual(c.closed()['crossingHeaders']['status'],'unavailable')

    def test_constructor_failures_retain_closed_boundary_and_close_displays(self):
        for failure in ['library', 'display', 'record-version', 'xres-version',
                        'xinput-extension', 'client-query', 'client-identity',
                        'context', 'enable', 'identity-recheck']:
            def version(stage, major, minor):
                def call(_display, a, b):
                    a._obj.value, b._obj.value = major, minor
                    return int(failure != stage)
                return call
            x = SimpleNamespace(XOpenDisplay=mock.Mock(return_value=0 if failure=='display' else 1),
                XCloseDisplay=mock.Mock(), XSync=mock.Mock(),
                XQueryExtension=mock.Mock(side_effect=lambda _d,_name,a,_b,_c:
                    (setattr(a._obj,'value',131) or int(failure!='xinput-extension'))))
            record = SimpleNamespace(XRecordQueryVersion=version('record-version',1,13),
                XRecordCreateContext=mock.Mock(return_value=0 if failure=='context' else 123),
                XRecordEnableContextAsync=mock.Mock(return_value=int(failure!='enable')),
                XRecordDisableContext=mock.Mock(), XRecordFreeContext=mock.Mock())
            res = SimpleNamespace(XResQueryVersion=version('xres-version',1,2))
            def identity(recorder):
                if failure in {'client-query','client-identity'}:
                    recorder.stage=failure
                    raise Unavailable(failure)
                if recorder.stage=='identity-recheck' and failure=='identity-recheck':
                    raise Unavailable(failure)
                return 456
            libraries = [x,record,res]
            with mock.patch.object(module['C'],'CDLL',side_effect=OSError('PRIVATE') if failure=='library' else libraries), \
                 mock.patch.object(NativeRecorder,'_bind'), \
                 mock.patch.object(NativeRecorder,'_identity',identity):
                with self.assertRaises(Unavailable) as caught:
                    NativeRecorder(10,20)
            self.assertEqual(caught.exception.stage,failure)
            self.assertNotIn('PRIVATE',json.dumps({'stage':caught.exception.stage}))
            if failure not in {'library','display'}:
                self.assertEqual(x.XCloseDisplay.call_count,2)
            if failure in {'enable','identity-recheck'}:
                record.XRecordFreeContext.assert_called_once()

    def test_owned_ordered_delivery_and_private_reduction(self):
        c = Counts(90, 131, 123)
        c.accept(0, False, 123, event())
        c.accept(0, False, 123, event(5, stamp=110))
        self.assertEqual({k:c.closed()[k] for k in ('pressCount','releaseCount','orderedPair')}, {'pressCount': 1, 'releaseCount': 1, 'orderedPair': True})

    def test_foreign_window_cannot_certify_pair(self):
        c = Counts(90, 131, 123)
        c.accept(0, False, 123, event(window=91))
        c.accept(0, False, 123, event(5, stamp=110))
        self.assertFalse(c.closed()['orderedPair'])

    def test_reversed_duplicate_or_identity_uncertainty_fails(self):
        for sequence in [(event(5), event()), (event(), event(), event(5)), (event(), event(5, device=3))]:
            c = Counts(90, 131, 123)
            for data in sequence:
                c.accept(0, False, 123, data)
            self.assertFalse(c.closed()['orderedPair'])
        for swap, base, data in [(True, 123, event()), (False, 124, event()), (False, 123, b'private')]:
            c = Counts(90, 131, 123)
            c.accept(0, swap, base, data)
            c.accept(0, False, 123, event())
            c.accept(0, False, 123, event(5, stamp=110))
            self.assertFalse(c.closed()['orderedPair'])

    def test_byte_budget_never_approves(self):
        c = Counts(90, 131, 123)
        for _ in range(129):
            c.accept(0, False, 123, event(window=91))
        c.accept(0, False, 123, event())
        c.accept(0, False, 123, event(5, stamp=110))
        self.assertFalse(c.closed()['orderedPair'])
if __name__ == '__main__':
    unittest.main()
