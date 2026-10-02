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

class Tests(unittest.TestCase):

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
        self.assertEqual(c.closed(), {'pressCount': 1, 'releaseCount': 1, 'orderedPair': True})

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
