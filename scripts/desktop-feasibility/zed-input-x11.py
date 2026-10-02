#!/usr/bin/env python3
"""Fixed native key transport; the checker proves foreground before and after."""
import json
import ctypes
import os
from pathlib import Path
import re
import subprocess
import time
import sys
import uuid

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


def pointer_observation():
    return dict(schemaVersion=1, mechanism='zed-pointer-observation', diagnosticsOnly=True,
                maximizedHorizontal=None, maximizedVertical=None, enabled=None,
                sensitive=None, showing=None, visible=None, defunct=None,
                retryContains=None, pointerTarget='unavailable', pointerChild='unavailable')


def pointer_child(frame, active):
    # xdotool's root query reports the Openbox frame. Query that frame directly
    # to distinguish its client from decoration without publishing window IDs.
    xlib = ctypes.CDLL('libX11.so.6')
    xlib.XOpenDisplay.argtypes = [ctypes.c_char_p]
    xlib.XOpenDisplay.restype = ctypes.c_void_p
    xlib.XCloseDisplay.argtypes = [ctypes.c_void_p]
    long_pointer, int_pointer = ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_int)
    xlib.XQueryPointer.argtypes = [ctypes.c_void_p, ctypes.c_ulong, long_pointer,
        long_pointer, int_pointer, int_pointer, int_pointer, int_pointer,
        ctypes.POINTER(ctypes.c_uint)]
    xlib.XQueryPointer.restype = ctypes.c_int
    display = xlib.XOpenDisplay(None)
    if not display:
        return 'unavailable'
    try:
        root, child = ctypes.c_ulong(), ctypes.c_ulong()
        rx, ry, wx, wy, mask = ctypes.c_int(), ctypes.c_int(), ctypes.c_int(), ctypes.c_int(), ctypes.c_uint()
        if not xlib.XQueryPointer(display, frame, ctypes.byref(root), ctypes.byref(child),
            ctypes.byref(rx), ctypes.byref(ry), ctypes.byref(wx), ctypes.byref(wy), ctypes.byref(mask)):
            return 'unavailable'
        return ('client' if child.value == active or not child.value and frame == active
                else 'decoration-or-empty' if not child.value
                else 'client-descendant' if owned_frame(child.value, active) else 'other')
    finally:
        xlib.XCloseDisplay(display)


def publish_observation(facts):
    # Only closed facts reach the existing private qualification directory.
    if (os.environ.get('GITHUB_ACTIONS') != 'true'
            or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted'
            or os.environ.get('RUNNER_OS') != 'Linux'):
        return
    directory = Path(os.environ.get('NANH_DESKTOP_QUALIFICATION_FACTS', ''))
    if not directory.is_absolute() or directory.is_symlink() or not directory.is_dir():
        return
    try:
        path = directory / ('zed-pointer-observation-' + uuid.uuid4().hex + '.json')
        with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'w') as output:
            json.dump(facts, output)
    except OSError:
        pass


def maximized_observation(active, facts, deadline):
    try:
        remaining = min(0.5, deadline - time.monotonic())
        if remaining <= 0:
            return
        result = subprocess.run(['/usr/bin/xprop', '-id', str(active), '_NET_WM_STATE'],
                                timeout=remaining, stdout=subprocess.PIPE,
                                stderr=subprocess.DEVNULL, check=True)
        if len(result.stdout) > 2048:
            return
        text = result.stdout.decode('ascii')
        if not re.fullmatch(r'_NET_WM_STATE\(ATOM\) =\s*(?:_NET_WM_STATE_[A-Z_]+(?:, _NET_WM_STATE_[A-Z_]+)*)?\s*', text):
            return
        atoms = set(re.findall(r'_NET_WM_STATE_[A-Z_]+', text))
        facts['maximizedHorizontal'] = '_NET_WM_STATE_MAXIMIZED_HORZ' in atoms
        facts['maximizedVertical'] = '_NET_WM_STATE_MAXIMIZED_VERT' in atoms
    except (OSError, subprocess.SubprocessError, UnicodeError):
        pass


def accessibility_observation(component, dbus, window, facts):
    # These are advisory measurements, not substitutes for provider recovery.
    try:
        states = tuple(int(value) for value in component.GetState(
            dbus_interface='org.a11y.atspi.Accessible', timeout=0.2))
        if len(states) == 2 and all(0 <= value <= 0xffffffff for value in states):
            bits = states[0] | states[1] << 32
            # AtspiStateType: editable is 7; enabled is 8.
            for name, bit in [('defunct', 6), ('enabled', 8), ('sensitive', 24),
                              ('showing', 25), ('visible', 30)]:
                facts[name] = bool(bits & 1 << bit)
        x, y, width, height = window
        facts['retryContains'] = bool(component.Contains(dbus.Int32(x + width // 2),
            dbus.Int32(y + height // 2), dbus.UInt32(1),
            dbus_interface='org.a11y.atspi.Component', timeout=0.2))
    except (dbus.DBusException, ValueError, TypeError):
        pass


def normalized_retry_point(request, active, geometry, facts=None):
    import dbus
    bus = None
    try:
        session = dbus.SessionBus()
        address = session.get_object('org.a11y.Bus', '/org/a11y/bus').GetAddress(
            dbus_interface='org.a11y.Bus', timeout=0.5)
        bus = dbus.bus.BusConnection(str(address))
        owner = bus.get_object('org.freedesktop.DBus', '/org/freedesktop/DBus').GetConnectionUnixProcessID(
            request['bus'], dbus_interface='org.freedesktop.DBus', timeout=0.5)
        if int(owner) != request['pid']:
            raise ValueError('accessibility owner mismatch')
        component = bus.get_object(request['bus'], request['path'])
        role = component.GetRole(dbus_interface='org.a11y.atspi.Accessible', timeout=0.5)
        name = component.Get('org.a11y.atspi.Accessible', 'Name',
            dbus_interface='org.freedesktop.DBus.Properties', timeout=0.5)
        if int(role) != 43 or str(name) != 'Retry':
            raise ValueError('retry control changed')
        screen = tuple(int(value) for value in component.GetExtents(dbus.UInt32(0),
            dbus_interface='org.a11y.atspi.Component', timeout=0.5))
        window = tuple(int(value) for value in component.GetExtents(dbus.UInt32(1),
            dbus_interface='org.a11y.atspi.Component', timeout=0.5))
        if facts is not None:
            accessibility_observation(component, dbus, window, facts)
        return coordinate_point(screen, window, geometry)
    except dbus.DBusException:
        raise ValueError('accessibility query unavailable') from None
    finally:
        if bus is not None:
            bus.close()



def coordinate_point(screen, window, geometry):
    # A missing AccessKit screen origin is accepted only when both coordinate
    # queries agree and the window-relative control is inside this owned client.
    x, y, width, height = window
    gx, gy, gw, gh = geometry
    if width <= 0 or height <= 0 or x < 0 or y < 0 or x + width > gw or y + height > gh:
        raise ValueError('control outside client')
    expected = (gx + x, gy + y, width, height)
    if screen != window and screen != expected:
        raise ValueError('coordinate conversion unavailable')
    return gx + x + width // 2, gy + y + height // 2


def retry_click(payload):
    facts = pointer_observation()
    try:
        request = json.loads(payload)
        if (type(request) is not dict or set(request) != {'pid', 'window', 'x', 'y', 'bus', 'path'}
                or any(type(request[key]) is not int for key in ('pid', 'window', 'x', 'y'))
                or not isinstance(request['bus'], str) or not re.fullmatch(r':[0-9]+\.[0-9]+', request['bus'])
                or not isinstance(request['path'], str) or not re.fullmatch(r'/org/a11y/atspi/accessible/[A-Za-z0-9_/]+', request['path'])
                or not 1 < request['pid'] <= 2147483647 or not 0 < request['window'] <= 4294967295
                or any(not -32768 <= request[key] <= 32767 for key in ('x', 'y'))):
            return 2
        deadline = time.monotonic() + 4
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
            if query in ('position', 'geometry'):
                if len(output) > 256:
                    raise ValueError('invalid pointer observation')
                parts = [line.split(b'=', 1) for line in output.splitlines()]
                if any(len(part) != 2 for part in parts):
                    raise ValueError('invalid pointer observation')
                values = dict(parts)
                expected = {b'X', b'Y', b'SCREEN', b'WINDOW'} if query == 'position' else {b'X', b'Y', b'SCREEN', b'WINDOW', b'WIDTH', b'HEIGHT'}
                if len(parts) != len(expected) or set(values) != expected:
                    raise ValueError('invalid pointer observation')
                point = (int(values[b'X']), int(values[b'Y']))
                return (*point, int(values[b'WINDOW'])) if query == 'position' else (*point, int(values[b'WIDTH']), int(values[b'HEIGHT']))
            if len(output) > 32 or not output.isdigit():
                raise ValueError('invalid identity')
            return int(output)
        def owned_foreground():
            active = run(['getactivewindow'], True)
            if not owned_frame(active, request['window']):
                return 11, None
            if run(['getwindowpid', str(active)], True) != request['pid']:
                return 12, None
            return 0, active
        stage = 13
        failure, active = owned_foreground()
        if failure:
            return failure
        maximized_observation(active, facts, deadline)
        stage = 18
        geometry = run(['getwindowgeometry', '--shell', str(active)], 'geometry')
        point = normalized_retry_point(request, active, geometry, facts)
        stage = 14
        # --sync waits for motion and can hang when the pointer is already here.
        # Dispatch once and prove the resulting position instead.
        run(['mousemove', '--', str(point[0]), str(point[1])])
        px, py, pointer_window = run(['getmouselocation', '--shell'], 'position')
        if (px, py) != point:
            return 14
        facts['pointerTarget'] = ('client' if pointer_window == active else
            'owned-frame' if pointer_window == request['window'] else
            'client-descendant' if owned_frame(pointer_window, active) else 'foreign')
        facts['pointerChild'] = pointer_child(request['window'], active)
        stage = 15
        if owned_foreground() != (0, active):
            return 11
        observer = None
        if (os.environ.get('NANH_ZED_XRECORD') == '1'
                and sys.platform == 'linux'
                and os.environ.get('GITHUB_ACTIONS') == 'true'
                and os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted'
                and os.environ.get('RUNNER_OS') == 'Linux'):
            # Observation never selects events or authorizes activation.
            import runpy

            def record_scope():
                try:
                    return (owned_foreground() == (0, active)
                            and run(['getwindowgeometry', '--shell', str(active)], 'geometry') == geometry
                            and pointer_child(request['window'], active) == 'client')
                except (OSError, ValueError, subprocess.SubprocessError):
                    return False

            remaining = deadline - time.monotonic()
            facts['inputDelivery'] = dict(status='unavailable', pressCount=None,
                                          releaseCount=None, orderedPair=None)
            if remaining > 1:
                module = runpy.run_path(str(Path(__file__).with_name('zed-xrecord-supervisor.py')))
                observer = module['Observer'](request['pid'], active, record_scope,
                                              budget=min(3, remaining - .5))
            if (not record_scope()
                    or normalized_retry_point(request, active, geometry) != point):
                if observer is not None:
                    facts['inputDelivery'] = observer.finish()
                return 18
        stage = 16
        # One ordinary activation, never another press after an uncertain receipt.
        run(['click', '--clearmodifiers', '1'])
        if observer is not None:
            facts['inputDelivery'] = observer.finish()
        return 0
    except (ValueError, TypeError, OSError, subprocess.SubprocessError, ImportError):
        return locals().get("stage", 2)
    finally:
        observer = locals().get('observer')
        if observer is not None:
            observer.close()
            facts['inputDelivery'] = observer.result
        publish_observation(facts)


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
