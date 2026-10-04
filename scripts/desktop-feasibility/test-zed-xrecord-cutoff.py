"""Synthetic worker/callback lifecycle; never opens Xlib or a native application."""
import ctypes as C
import io
import json
from pathlib import Path
import runpy
from types import SimpleNamespace
from unittest import TestCase, main, mock
native=runpy.run_path(str(Path(__file__).with_name('zed-xrecord.py')))
supervisor=runpy.run_path(str(Path(__file__).with_name('zed-xrecord-supervisor.py')))
fixtures=runpy.run_path(str(Path(__file__).with_name('test-zed-xrecord.py')))
Recorder=native['NativeRecorder']

class CutoffTests(TestCase):
    def worker(self,scenario):
        clock=[0.0];calls=[]
        recorder=Recorder.__new__(Recorder)
        recorder.counts=native['Counts'](90,131,123)
        recorder.frozen=False;recorder.cutoff=1.0
        recorder.record=SimpleNamespace(XRecordFreeData=lambda _p:calls.append('free'))
        def callback():
            data=(C.c_ubyte*32).from_buffer_copy(fixtures['crossing']())
            event=native['Intercept']();event.category=0;event.length=8;event.base=123
            event.data=C.cast(data,C.POINTER(C.c_ubyte))
            recorder._event(None,C.pointer(event))
        def pump():
            calls.append('pump')
            if scenario=='pump-error':raise ValueError('PRIVATE')
            callback()  # Delivered/processed strictly before original cutoff.
            if scenario.startswith('cutoff'):
                clock[0]=1.0;callback()  # Exact cutoff must not admit another header.
        recorder.pump=pump
        def snapshot():
            calls.append('snapshot')
            if scenario=='snapshot-error':raise ValueError('PRIVATE')
            return recorder.freeze()
        recorder.snapshot=snapshot
        def close():
            calls.append('close');clock[0]=0.5;callback()  # Frozen receipt cannot mutate in cleanup.
        recorder.close=close
        def create(_pid,_window,cutoff):
            self.assertEqual(cutoff,1.0);return recorder
        input_data=json.dumps(dict(pid=10,window=90,cutoff=1.0)).encode()+b'\n'
        if scenario in {'finish','snapshot-error'}:input_data+=b'finish\n'
        if scenario in {'invalid-finish','cutoff-invalid-finish'}:input_data+=b'PRIVATE\n'
        stdin=SimpleNamespace(buffer=io.BytesIO(input_data));stdout=io.StringIO()
        def select(*_args):
            return ([stdin.buffer] if scenario in {'finish','snapshot-error','invalid-finish'} or scenario=='cutoff-invalid-finish' and clock[0]>=1.0 else [],[],[])
        with mock.patch.object(supervisor['sys'],'platform','linux'),\
          mock.patch.dict(supervisor['os'].environ,dict(GITHUB_ACTIONS='true',RUNNER_ENVIRONMENT='github-hosted',RUNNER_OS='Linux'),clear=True),\
          mock.patch.object(supervisor['sys'],'stdin',stdin),mock.patch.object(supervisor['sys'],'stdout',stdout),\
          mock.patch.object(supervisor['time'],'monotonic',lambda:clock[0]),\
          mock.patch.object(supervisor['select'],'select',select),\
          mock.patch.object(runpy,'run_path',return_value={'NativeRecorder':create,'Unavailable':native['Unavailable']}):
            supervisor['worker']()
        records=[json.loads(line) for line in stdout.getvalue().splitlines()]
        self.assertEqual(records[0],{'stage':'armed'})
        self.assertNotIn('PRIVATE',json.dumps(records))
        self.assertEqual(len(records),2)
        return recorder,records[1],calls
    def test_cutoff_freezes_only_admitted_prefix_without_snapshot_or_renewal(self):
        recorder,result,calls=self.worker('cutoff')
        self.assertEqual(result['status'],'complete');self.assertEqual(result['captureEnd'],'cutoff')
        self.assertEqual(result['crossingHeaders']['ownedNormalEnterCount'],1)
        self.assertEqual(recorder.counts.closed()['crossingHeaders']['ownedNormalEnterCount'],1)
        self.assertTrue(recorder.frozen);self.assertNotIn('snapshot',calls)
        self.assertEqual(calls.count('pump'),1);self.assertEqual(recorder.cutoff,1.0)
        self.assertEqual(calls[-2:],['close','free'])
    def test_finish_freezes_before_cleanup(self):
        _,result,_=self.worker('finish');self.assertEqual(result['captureEnd'],'finish')
        self.assertEqual(result['crossingHeaders']['ownedNormalEnterCount'],1)
    def test_invalid_finish_and_native_failures_remain_unobserved_and_closed(self):
        for scenario,reason in [('invalid-finish','finish-request'),('cutoff-invalid-finish','finish-request'),('pump-error','native-pump'),('snapshot-error','native-snapshot')]:
            _,result,_=self.worker(scenario)
            self.assertEqual(result['status'],'query-failed');self.assertEqual(result['failureReason'],reason)
            self.assertIsNone(result['pressCount']);self.assertNotIn('captureEnd',result)
    def test_optional_closed_fields_reject_raw_unknown_or_inconsistent_values(self):
        value=dict(status='complete',pressCount=0,releaseCount=0,orderedPair=False,captureEnd='cutoff')
        self.assertEqual(supervisor['validate'](value),value)
        for change in [dict(captureEnd='PRIVATE'),dict(captureEnd=[]),dict(failureReason='native-pump'),dict(status='timeout')]:
            with self.assertRaises(ValueError):supervisor['validate']({**value,**change})
        with self.assertRaises(ValueError):supervisor['validate']({**supervisor['unobserved']('query-failed'), 'failureReason':'PRIVATE'})

if __name__=='__main__':main()
