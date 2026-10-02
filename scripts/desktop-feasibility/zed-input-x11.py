#!/usr/bin/env python3
"""Fixed native key transport; the checker proves foreground before and after."""
import json
import ctypes
import subprocess
import time
import sys

KEYS = {'trust': 'ctrl+alt+t', 'new-thread': 'ctrl+alt+n', 'copy-thread': 'ctrl+alt+y',
        'select-all': 'ctrl+a', 'copy': 'ctrl+c', 'paste': 'ctrl+v',
        'right': 'Right', 'submit': 'Return'}


def matches_owned_frame(active, expected, parent_query):
    # Openbox reports the client as active but the native inventory lists its
    # top-level frame. Accept only a freshly proved ancestor, never PID alone.
    visited = set()
    for _ in range(16):
        if active == expected:
            return True
        if not active or active in visited:
            return False
        visited.add(active)
        root, parent = parent_query(active)
        if not parent or parent == root:
            return False
        active = parent
    return False


def owned_frame(active, expected):
    if active == expected:
        return True
    xlib = ctypes.CDLL('libX11.so.6')
    xlib.XOpenDisplay.argtypes = [ctypes.c_char_p]
    xlib.XOpenDisplay.restype = ctypes.c_void_p
    xlib.XCloseDisplay.argtypes = [ctypes.c_void_p]
    xlib.XFree.argtypes = [ctypes.c_void_p]
    pointer = ctypes.POINTER(ctypes.c_ulong)
    xlib.XQueryTree.argtypes = [ctypes.c_void_p, ctypes.c_ulong, pointer, pointer,
                               ctypes.POINTER(pointer), ctypes.POINTER(ctypes.c_uint)]
    xlib.XQueryTree.restype = ctypes.c_int
    display = xlib.XOpenDisplay(None)
    if not display:
        return False
    def parent_query(window):
        root, parent = ctypes.c_ulong(), ctypes.c_ulong()
        children, count = pointer(), ctypes.c_uint()
        try:
            if not xlib.XQueryTree(display, window, ctypes.byref(root), ctypes.byref(parent),
                                  ctypes.byref(children), ctypes.byref(count)):
                raise ValueError('identity unavailable')
            return root.value, parent.value
        finally:
            if children:
                xlib.XFree(children)
    try:
        return matches_owned_frame(active, expected, parent_query)
    finally:
        xlib.XCloseDisplay(display)


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
            if query == 'position':
                if len(output) > 256:
                    raise ValueError('invalid pointer observation')
                parts = [line.split(b'=', 1) for line in output.splitlines()]
                if len(parts) != 4 or any(len(part) != 2 for part in parts):
                    raise ValueError('invalid pointer observation')
                values = dict(parts)
                if set(values) != {b'X', b'Y', b'SCREEN', b'WINDOW'}:
                    raise ValueError('invalid pointer observation')
                return int(values[b'X']), int(values[b'Y'])
            if len(output) > 32 or not output.isdigit():
                raise ValueError('invalid identity')
            return int(output)
        def owned_foreground():
            active = run(['getactivewindow'], True)
            if not owned_frame(active, request['window']):
                return 11
            if run(['getwindowpid', str(active)], True) != request['pid']:
                return 12
            return 0
        stage = 13
        guard = owned_foreground()
        if guard:
            return guard
        stage = 14
        # --sync waits for motion and can hang when the pointer is already here.
        # Dispatch once and prove the resulting position instead.
        run(['mousemove', '--', str(request['x']), str(request['y'])])
        if run(['getmouselocation', '--shell'], 'position') != (request['x'], request['y']):
            return 14
        stage = 15
        guard = owned_foreground()
        if guard:
            return guard
        stage = 16
        # One ordinary activation, never another press after an uncertain receipt.
        run(['click', '--clearmodifiers', '1'])
        return 0
    except (ValueError, TypeError, OSError, subprocess.SubprocessError):
        return locals().get("stage", 2)


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
