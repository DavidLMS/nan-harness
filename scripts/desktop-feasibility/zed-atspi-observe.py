#!/usr/bin/env python3
"""Read-only, closed measurements of held AT-SPI button geometry and state."""
import json
import os
from pathlib import Path
import re
import subprocess
import time
import uuid

COUNTS = ('sampledButtons', 'identityRejected', 'stateRejected', 'stabilityRejected',
          'containmentRejected', 'offsetExpected', 'offsetMissing', 'offsetInconsistent',
          'toggleOn', 'toggleOff', 'toggleUnknown')


def observation(status='unavailable'):
    return dict(schemaVersion=1, mechanism='zed-atspi-geometry', diagnosticsOnly=True,
                status=status, phase='pre-retry', **dict.fromkeys(COUNTS, 0))


def rectangle(value):
    if (not isinstance(value, (list, tuple)) or len(value) != 4
            or any(type(n) is not int or not -(2**31) <= n < 2**31 for n in value)
            or value[2] <= 0 or value[3] <= 0):
        raise ValueError('invalid rectangle')
    return tuple(value)


def contains(outer, inner):
    x, y, w, h = outer
    a, b, c, d = inner
    return a >= x and b >= y and a + c <= x + w and b + d <= y + h


def canonical_rectangle(screen, window, geometry):
    expected = rectangle([geometry[0] + window[0], geometry[1] + window[1], *window[2:]])
    if screen == window:
        return expected
    if screen == expected:
        return screen
    return None


def measure(request, backend, deadline, clock=time.monotonic, canonical=None):
    result = observation('observed')
    geometry = backend.guard()
    for index, held in enumerate(request['buttons']):
        if clock() >= deadline:
            result['status'] = 'budget-exceeded'
            break
        try:
            first = backend.read(held, deadline)
            second = backend.read(held, deadline)
            if backend.guard() != geometry:
                result['status'] = 'guard-rejected'
                break
            if first is None or second is None:
                result['identityRejected'] += 1
                continue
            if first != second:
                result['stabilityRejected'] += 1
                continue
            role, bits, screen, window = first
            result['sampledButtons'] += 1
            expected = (geometry[0] + window[0], geometry[1] + window[1], *window[2:])
            relation = ('offsetExpected' if screen == expected else
                        'offsetMissing' if screen == window else 'offsetInconsistent')
            result[relation] += 1
            result['toggleUnknown' if bits & (1 << 32) or role == 43 and not bits & (1 << 4) else
                   'toggleOn' if bits & (1 << (20 if role == 62 else 4)) else 'toggleOff'] += 1
            if (bits & (1 << 6) or not bits & ((1 << 8) | (1 << 24))
                    or not bits & ((1 << 25) | (1 << 30))):
                result['stateRejected'] += 1
            # Diagnostic containment normalizes the two source-supported coordinate
            # relations; it never changes an activation target or coordinate.
            root = canonical_rectangle(screen, window, geometry)
            if (canonical is not None and root is not None
                    and role in (43, 62) and not bits & (1 << 6)
                    and bits & ((1 << 8) | (1 << 24))
                    and bits & ((1 << 25) | (1 << 30))):
                canonical.append(dict(index=index, role=role, bounds=list(root),
                    toggle='unknown' if bits & (1 << 32) or role == 43 else
                    'on' if bits & (1 << 20) else 'off'))
            if root is None or not any(contains(root, icon) for icon in request['icons']):
                result['containmentRejected'] += 1
        except (OSError, ValueError, TimeoutError):
            result['status'] = 'partial'
    return result


def validate(request):
    if (set(request) not in ({'pid', 'window', 'buttons', 'icons'},
                            {'pid', 'window', 'buttons', 'icons', 'privateName'})
            or any(type(request[k]) is not int or request[k] <= 0 for k in ('pid', 'window'))
            or type(request['buttons']) is not list or len(request['buttons']) > 64
            or type(request['icons']) is not list or len(request['icons']) > 64):
        raise ValueError('invalid request')
    if ('privateName' in request and (type(request['privateName']) is not str
            or not re.fullmatch(r'zed-canonical-[0-9a-f]{16}\.private', request['privateName']))):
        raise ValueError('invalid private handoff')
    identities = set()
    for button in request['buttons']:
        if (set(button) != {'bus', 'path'} or type(button['bus']) is not str
                or not re.fullmatch(r':[0-9]+\.[0-9]+', button['bus'])
                or type(button['path']) is not str
                or not re.fullmatch(r'/[A-Za-z0-9_/]{1,255}', button['path'])
                or (button['bus'], button['path']) in identities):
            raise ValueError('invalid held identity')
        identities.add((button['bus'], button['path']))
    request['icons'] = [rectangle(icon) for icon in request['icons']]
    return request


class Backend:
    def __init__(self, request, deadline):
        import dbus
        import runpy
        self.dbus, self.request, self.deadline = dbus, request, deadline
        transport = runpy.run_path(str(Path(__file__).with_name('zed-input-x11.py')))
        self.frame_matches = transport['owned_frame']
        self.client_snapshot = transport['independent_client_snapshot']
        try:
            session = dbus.SessionBus()
            address = session.get_object('org.a11y.Bus', '/org/a11y/bus').GetAddress(
                dbus_interface='org.a11y.Bus', timeout=self.remaining())
            self.bus = dbus.bus.BusConnection(str(address))
        except dbus.DBusException:
            raise ValueError('accessibility unavailable') from None

    def remaining(self):
        remaining = min(0.15, self.deadline - time.monotonic())
        if remaining <= 0:
            raise TimeoutError('deadline')
        return remaining

    def query(self, args):
        output = subprocess.run(['/usr/bin/xdotool', *args], check=True,
                                timeout=self.remaining(), stdout=subprocess.PIPE,
                                stderr=subprocess.DEVNULL).stdout
        if len(output) > 256:
            raise ValueError('bounded output')
        return output.strip()

    def guard(self):
        active = self.query(['getactivewindow'])
        # The native inventory may hold the WM decoration frame. A frame's
        # geometry is not a client origin; only a distinct freshly owned
        # descendant with its own matching process property can supply it.
        if (not active.isdigit() or int(active) == self.request['window']
                or not self.frame_matches(int(active), self.request['window'])):
            raise ValueError('client origin unavailable')
        if self.query(['getwindowpid', active.decode('ascii')]) != str(self.request['pid']).encode():
            raise ValueError('foreign process')
        lines = self.query(['getwindowgeometry', '--shell', active.decode('ascii')]).splitlines()
        parts = [line.split(b'=', 1) for line in lines]
        values = dict(parts)
        if len(parts) != 6 or set(values) != {b'X', b'Y', b'WIDTH', b'HEIGHT', b'SCREEN', b'WINDOW'}:
            raise ValueError('invalid geometry')
        raw = rectangle([int(values[k]) for k in (b'X', b'Y', b'WIDTH', b'HEIGHT')])
        first = self.client_snapshot(int(active))
        second = self.client_snapshot(int(active))
        if first is None or first != second or first[2] != raw[2:]:
            raise ValueError('client geometry unavailable')
        if (self.query(['getactivewindow']) != active
                or self.query(['getwindowpid', active.decode('ascii')]) != str(self.request['pid']).encode()
                or not self.frame_matches(int(active), self.request['window'])):
            raise ValueError('client identity changed')
        return rectangle([*first[0], *first[2]])

    def read(self, held, deadline):
        try:
            daemon = self.bus.get_object('org.freedesktop.DBus', '/org/freedesktop/DBus')
            def owner():
                return int(daemon.GetConnectionUnixProcessID(held['bus'],
                    dbus_interface='org.freedesktop.DBus', timeout=self.remaining()))
            if owner() != self.request['pid']:
                return None
            obj = self.bus.get_object(held['bus'], held['path'])
            role = int(obj.GetRole(dbus_interface='org.a11y.atspi.Accessible', timeout=self.remaining()))
            if role not in (43, 62):
                return None
            states = tuple(int(n) for n in obj.GetState(
                dbus_interface='org.a11y.atspi.Accessible', timeout=self.remaining()))
            if len(states) != 2 or any(not 0 <= n <= 0xffffffff for n in states):
                raise ValueError('invalid state')
            extents = [rectangle([int(n) for n in obj.GetExtents(self.dbus.UInt32(kind),
                dbus_interface='org.a11y.atspi.Component', timeout=self.remaining())]) for kind in (0, 1)]
            return (role, states[0] | states[1] << 32, *extents) if owner() == self.request['pid'] else None
        except self.dbus.DBusException:
            raise ValueError('query unavailable') from None

    def close(self):
        self.bus.close()


def run(payload):
    if (os.environ.get('NANH_ZED_PANEL_ZOOM') != 'observe'
            or os.environ.get('GITHUB_ACTIONS') != 'true'
            or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted'
            or os.environ.get('RUNNER_OS') != 'Linux'):
        return 2
    result, backend, canonical, request = observation(), None, [], None
    try:
        request = validate(json.loads(payload))
        deadline = time.monotonic() + 2
        backend = Backend(request, deadline)
        result = measure(request, backend, deadline, canonical=canonical)
    except (ValueError, TypeError, KeyError, OSError, ImportError, subprocess.SubprocessError, TimeoutError):
        pass
    finally:
        if backend is not None:
            backend.close()
    directory = Path(os.environ.get('NANH_DESKTOP_QUALIFICATION_FACTS', ''))
    if (not directory.is_absolute() or directory.is_symlink() or not directory.is_dir()
            or directory.resolve() != directory or directory.stat().st_mode & 0o077
            or directory.stat().st_uid != os.getuid()):
        return 2
    try:
        if request is not None and 'privateName' in request:
            private = directory / request['privateName']
            with os.fdopen(os.open(private, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'w') as output:
                json.dump(canonical if result['status'] == 'observed' else [], output)
        path = directory / ('zed-atspi-' + uuid.uuid4().hex + '.json')
        with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'w') as output:
            json.dump(result, output)
    except OSError:
        return 2
    return 0
