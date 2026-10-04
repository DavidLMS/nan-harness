"""Supervise read-only XRecord observations in an owned bounded worker."""
import json
import os
from pathlib import Path
import select
import subprocess
import sys
import time
STATUSES = {'complete', 'unavailable', 'timeout', 'query-failed', 'identity-failed'}

STAGES = {'policy', 'budget-insufficient', 'request', 'library', 'display',
          'record-version', 'xres-version', 'xinput-extension', 'client-query',
          'client-identity', 'context', 'enable', 'identity-recheck', 'armed',
          'observation', 'cleanup'}

def unobserved(status, stage=None):
    result = {'status': status, 'pressCount': None, 'releaseCount': None, 'orderedPair': None}
    if stage is not None:
        result['stage'] = stage
    return result

def validate(value):
    if type(value) is not dict or set(value)-{'crossingHeaders'} not in ({'status', 'pressCount', 'releaseCount', 'orderedPair'}, {'status', 'pressCount', 'releaseCount', 'orderedPair', 'stage'}) or value['status'] not in STATUSES:
        raise ValueError('closed record rejected')
    if 'stage' in value and (type(value['stage']) is not str or value['stage'] not in STAGES):
        raise ValueError('closed record rejected')
    if value['status'] == 'complete':
        if any((type(value[k]) is not int or not 0 <= value[k] <= 2 for k in ['pressCount', 'releaseCount'])) or type(value['orderedPair']) is not bool:
            raise ValueError('closed record rejected')
        if value['orderedPair'] and (value['pressCount'], value['releaseCount']) != (1, 1):
            raise ValueError('closed record rejected')
    elif any((value[k] is not None for k in ['pressCount', 'releaseCount', 'orderedPair'])):
        raise ValueError('closed record rejected')
    if 'crossingHeaders' in value:
        headers=value['crossingHeaders']
        keys={'ownedNormalEnterCount','ownedNonNormalEnterCount','ownedNormalLeaveCount','ownedMotionCount'}
        if (value['status'] != 'complete' or type(headers) is not dict or set(headers)!=keys|{'status'}
            or type(headers['status']) is not str or headers['status'] not in {'observed','unavailable'}
            or headers['status']=='observed' and any(type(headers[k]) is not int or not 0<=headers[k]<=64 for k in keys)
            or headers['status']=='unavailable' and any(headers[k] is not None for k in keys)):
            raise ValueError('closed record rejected')
    return value

def unique(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError('closed record rejected')
        value[key] = item
    return value

class Observer:

    def __init__(self, pid, window, preflight, worker=None, budget=3):
        self.child = None
        self.preflight = preflight
        self.pending = bytearray()
        self.absolute_end = time.monotonic() + budget
        self.end = self.absolute_end - min(0.2, budget / 4)
        self.stage = 'request'
        self.result = unobserved('unavailable', self.stage)
        if not 0 < budget <= 3:
            self.result = unobserved('unavailable', 'budget-insufficient')
            return
        if not self._owned():
            self.result = unobserved('identity-failed', self.stage)
            return
        if type(pid) is not int or not 1 < pid <= 2147483647 or type(window) is not int or (not 0 < window <= 4294967295):
            return
        env = {k: v for k, v in os.environ.items() if k in {'DISPLAY', 'XAUTHORITY', 'GITHUB_ACTIONS', 'RUNNER_ENVIRONMENT', 'RUNNER_OS'}}
        command = worker or [sys.executable, '-s', str(Path(__file__).resolve()), 'worker']
        try:
            self.child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env, close_fds=True)
            self.child.stdin.write(json.dumps({'pid': pid, 'window': window, 'cutoff':self.end}).encode() + b'\n')
            self.child.stdin.flush()
            ready = self._line()
            if ready == {'stage': 'armed'}:
                self.stage = 'armed'
                self.result = None if self._owned() else unobserved('identity-failed', self.stage)
            elif isinstance(ready, dict) and 'status' in ready:
                self.result = validate(ready)
            else:
                self.result = unobserved('identity-failed' if ready == {'stage': 'armed'} else 'query-failed', self.stage)
        except TimeoutError:
            self.result = unobserved('timeout', self.stage)
        except (OSError, ValueError, TypeError):
            self.result = unobserved('query-failed', self.stage)
        if self.result is not None:
            self.close()

    def _owned(self):
        # A failed bounded caller query cannot strand an already-armed child.
        try:
            return self.preflight() is True
        except (OSError, ValueError, TypeError, subprocess.SubprocessError):
            return False

    def _line(self):
        while b'\n' not in self.pending:
            remaining = self.end - time.monotonic()
            # The worker may have emitted its bounded final receipt at its
            # original cutoff. Consume already-ready bytes without waiting or
            # extending recording; absence still times out immediately.
            ready, _, _ = select.select([self.child.stdout], [], [], max(0, remaining))
            if not ready:
                raise TimeoutError()
            chunk = os.read(self.child.stdout.fileno(), 257)
            if not chunk or len(self.pending) + len(chunk) > 512:
                raise ValueError('closed record rejected')
            self.pending.extend(chunk)
        line, _, rest = self.pending.partition(b'\n')
        self.pending = bytearray(rest)
        return json.loads(line, object_pairs_hook=unique)

    def finish(self):
        try:
            if self.result is None:
                self.stage = 'observation'
                if not self._owned():
                    self.result = unobserved('identity-failed', self.stage)
                else:
                    if time.monotonic() < self.end and self.child.poll() is None:
                        try:
                            self.child.stdin.write(b'finish\n')
                            self.child.stdin.flush()
                        except BrokenPipeError:
                            pass  # A spontaneous cutoff receipt may already be queued.
                    self.result = validate(self._line())
                    if not self._owned():
                        self.result = unobserved('identity-failed', self.stage)
        except TimeoutError:
            self.result = unobserved('timeout', self.stage)
        except (OSError, ValueError, TypeError):
            self.result = unobserved('query-failed', self.stage)
        finally:
            self.close()
        return self.result

    def close(self):
        if self.child is None:
            return
        child = self.child
        reaped = False
        try:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=max(0, self.absolute_end - time.monotonic()))
            reaped = True
            # Output is a two-line protocol, never an arbitrary diagnostic log.
            # After reaping, EOF is bounded: reject even private trailing bytes.
            trailing = bytes(self.pending)
            if not child.stdout.closed:
                while len(trailing) <= 512:
                    ready, _, _ = select.select([child.stdout], [], [], 0)
                    if not ready:
                        break
                    chunk = os.read(child.stdout.fileno(), 513)
                    if not chunk:
                        break
                    trailing += chunk
            if trailing:
                self.result = unobserved('query-failed', 'cleanup')
        except subprocess.TimeoutExpired:
            self.result = unobserved('timeout', 'cleanup')
        except OSError:
            self.result = unobserved('query-failed', 'cleanup')
        finally:
            for stream in (child.stdin, child.stdout):
                try:
                    stream.close()
                except OSError:
                    pass
            # A failed reap retains explicit ownership for another bounded
            # cleanup attempt; never claim that the child disappeared.
            if reaped:
                self.child = None


def worker():
    if sys.platform != 'linux' or os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted' or (os.environ.get('RUNNER_OS') != 'Linux'):
        print(json.dumps(unobserved('unavailable', 'policy')), flush=True)
        return
    recorder = None
    stage = 'request'
    try:
        request = json.loads(sys.stdin.buffer.readline(257), object_pairs_hook=unique)
        if type(request) is not dict or set(request) != {'pid', 'window', 'cutoff'} or type(request['pid']) is not int or (not 1 < request['pid'] <= 2147483647) or (type(request['window']) is not int) or (not 0 < request['window'] <= 4294967295):
            raise ValueError()
        cutoff=request['cutoff']
        if type(cutoff) not in (int,float) or not time.monotonic()<cutoff<=time.monotonic()+3:
            raise ValueError()
        import runpy
        native = runpy.run_path(str(Path(__file__).with_name('zed-xrecord.py')))
        NativeRecorder = native['NativeRecorder']
        Unavailable = native['Unavailable']
        try:
            recorder = NativeRecorder(request['pid'], request['window'])
        except Unavailable as error:
            print(json.dumps(validate(unobserved('unavailable', error.stage))), flush=True)
            return
        stage = 'armed'
        print(json.dumps({'stage': 'armed'}), flush=True)
        # Consume the existing server stream during hover, not a fresh delay
        # after it. Parent cutoff and kill/reap remain authoritative.
        while True:
            remaining=cutoff-time.monotonic()
            if remaining<=0:
                raise TimeoutError()
            ready, _, _=select.select([sys.stdin.buffer], [], [], min(.02,remaining))
            recorder.pump()
            if ready:
                if sys.stdin.buffer.readline(16)!=b'finish\n':
                    raise ValueError()
                break
        stage = 'observation'
        result = {'status': 'complete', 'stage': stage, **recorder.snapshot()}
        print(json.dumps(validate(result)), flush=True)
    except Exception:
        print(json.dumps(unobserved('query-failed', stage)), flush=True)
    finally:
        if recorder is not None:
            recorder.close()
if __name__ == '__main__':
    if sys.argv[1:] == ['worker']:
        worker()
