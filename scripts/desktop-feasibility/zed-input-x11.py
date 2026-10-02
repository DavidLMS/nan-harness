#!/usr/bin/env python3
"""Fixed native key transport; the checker proves foreground before and after."""
import json
import subprocess
import time
import sys

KEYS = {'trust': 'ctrl+alt+t', 'new-thread': 'ctrl+alt+n', 'copy-thread': 'ctrl+alt+y',
        'select-all': 'ctrl+a', 'copy': 'ctrl+c', 'paste': 'ctrl+v',
        'right': 'Right', 'submit': 'Return'}


def retry_click(payload):
    try:
        request = json.loads(payload)
        if (type(request) is not dict or set(request) != {'pid', 'window', 'x', 'y'}
                or any(type(value) is not int for value in request.values())
                or not 1 < request['pid'] <= 2147483647 or not 0 < request['window'] <= 4294967295
                or any(not -32768 <= request[key] <= 32767 for key in ('x', 'y'))):
            return 2
        deadline = time.monotonic() + 2
        def run(args, query=False):
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise subprocess.TimeoutExpired('fixed-helper', 2)
            result = subprocess.run(['/usr/bin/xdotool', *args], timeout=remaining,
                                    stdout=subprocess.PIPE if query else subprocess.DEVNULL,
                                    stderr=subprocess.DEVNULL, check=True)
            if not query:
                return None
            output = result.stdout.strip()
            if len(output) > 32 or not output.isdigit():
                raise ValueError('invalid identity')
            return int(output)
        def owned_foreground():
            return (run(['getactivewindow'], True) == request['window']
                    and run(['getwindowpid', str(request['window'])], True) == request['pid'])
        if not owned_foreground():
            return 3
        run(['mousemove', '--sync', '--', str(request['x']), str(request['y'])])
        if not owned_foreground():
            return 3
        # One ordinary activation, never another press after an uncertain receipt.
        run(['click', '--clearmodifiers', '1'])
        return 0
    except (ValueError, TypeError, OSError, subprocess.SubprocessError):
        return 3


def main():
    if len(sys.argv) != 2:
        return 2
    payload = sys.stdin.buffer.read(4097)
    if len(payload) > 4096:
        return 2
    if sys.argv[1] == 'retry-click':
        return retry_click(payload)
    if sys.argv[1] not in KEYS or payload:
        return 2
    try:
        return subprocess.run(['/usr/bin/xdotool', 'key', '--clearmodifiers', KEYS[sys.argv[1]]],
                              timeout=2, stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL, check=False).returncode
    except (OSError, subprocess.TimeoutExpired):
        return 3


if __name__ == '__main__':
    raise SystemExit(main())
