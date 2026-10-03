#!/usr/bin/env python3
"""One ordinary native accessibility Click; no pointer fallback or replay."""
import json
import os
from pathlib import Path
import re
import runpy
import subprocess
import sys
import time


def valid_request(request):
    return (type(request) is dict
            and set(request) == {'pid', 'window', 'x', 'y', 'bus', 'path', 'bounds'}
            and type(request['pid']) is int and 1 < request['pid'] <= 2147483647
            and type(request['window']) is int and 0 < request['window'] <= 4294967295
            and all(type(request[key]) is int and -32768 <= request[key] <= 32767 for key in ('x', 'y'))
            and type(request['bus']) is str and re.fullmatch(r':[0-9]+\.[0-9]+', request['bus'])
            and type(request['path']) is str
            and re.fullmatch(r'/org/a11y/atspi/accessible/[A-Za-z0-9_/]+', request['path'])
            and type(request['bounds']) is list and len(request['bounds']) == 4
            and all(type(value) is int and -32768 <= value <= 32767 for value in request['bounds'])
            and request['bounds'][2] > 0 and request['bounds'][3] > 0)


def perform_once(snapshot, invoke, deadline, facts, clock=time.monotonic, post_guard=lambda: True):
    """Reproof is mandatory before sealing; uncertainty never invokes again."""
    facts['stage'] = 'preflight'
    first = snapshot()
    if clock() >= deadline or snapshot() != first or clock() >= deadline:
        return False
    facts['stage'] = 'action'
    facts['actionAttempted'] = True
    response = invoke()
    # dbus.Boolean is integer-like; arbitrary truthy objects are not receipts.
    if type(response) is not bool:
        return False
    facts['forwarded'] = response
    if not response or clock() >= deadline:
        return False
    facts['stage'] = 'forwarded'
    facts['stage'] = 'postflight'
    return post_guard() and clock() < deadline


def close_transport(bus, publish, facts):
    closed = True
    try:
        if bus is not None:
            bus.close()
    except Exception:
        closed = False
    finally:
        publish(facts)
    return closed


def run(payload):
    facts = dict(schemaVersion=1, mechanism='zed-atspi-retry', diagnosticsOnly=True,
                 method='atspi-click', stage='policy', actionAttempted=False, forwarded=False)
    driver = runpy.run_path(str(Path(__file__).with_name('zed-input-x11.py')))
    bus = None
    try:
        if (sys.platform != 'linux' or os.environ.get('GITHUB_ACTIONS') != 'true'
                or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted'
                or os.environ.get('RUNNER_OS') != 'Linux'
                or os.environ.get('NANH_ZED_RETRY_METHOD') != 'atspi-click'):
            return 2
        facts['stage'] = 'request'
        request = json.loads(payload)
        if not valid_request(request):
            return 2
        deadline = time.monotonic() + 4
        import dbus
        def timeout():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError('deadline')
            return min(0.25, remaining)
        session = dbus.SessionBus()
        address = session.get_object('org.a11y.Bus', '/org/a11y/bus').GetAddress(
            dbus_interface='org.a11y.Bus', timeout=timeout())
        bus = dbus.bus.BusConnection(str(address))
        node = bus.get_object(request['bus'], request['path'])
        held = []
        def snapshot():
            owner = bus.get_object('org.freedesktop.DBus', '/org/freedesktop/DBus').GetConnectionUnixProcessID(
                request['bus'], dbus_interface='org.freedesktop.DBus', timeout=timeout())
            def query(args):
                output = subprocess.run(['/usr/bin/xdotool', *args], check=True,
                    stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=timeout()).stdout.strip()
                if len(output) > 32 or not output.isdigit():
                    raise ValueError('identity')
                return int(output)
            active = query(['getactivewindow'])
            if (int(owner) != request['pid'] or not driver['owned_frame'](active, request['window'])
                    or query(['getwindowpid', str(active)]) != request['pid']):
                raise ValueError('ownership')
            geometry = driver['independent_client_snapshot'](active)
            role = node.GetRole(dbus_interface='org.a11y.atspi.Accessible', timeout=timeout())
            name = node.Get('org.a11y.atspi.Accessible', 'Name',
                dbus_interface='org.freedesktop.DBus.Properties', timeout=timeout())
            states = tuple(int(value) for value in node.GetState(
                dbus_interface='org.a11y.atspi.Accessible', timeout=timeout()))
            required = (8, 24, 25, 30)  # enabled, sensitive, showing, visible
            if (int(role) != 43 or str(name) != 'Retry' or len(states) != 2
                    or any(not states[bit // 32] & (1 << (bit % 32)) for bit in required)
                    or states[0] & (1 << 6)):
                raise ValueError('control')
            extents = tuple(tuple(int(value) for value in node.GetExtents(dbus.UInt32(kind),
                dbus_interface='org.a11y.atspi.Component', timeout=timeout())) for kind in (0, 1))
            driver['coordinate_point'](extents[0], extents[1], (*geometry[0], *geometry[2]))
            if list(extents[0]) != request['bounds']:
                raise ValueError('bounds')
            properties = 'org.freedesktop.DBus.Properties'
            count = node.Get('org.a11y.atspi.Action', 'NActions', dbus_interface=properties, timeout=timeout())
            action = node.GetName(dbus.Int32(0), dbus_interface='org.a11y.atspi.Action', timeout=timeout())
            if int(count) != 1 or str(action) != 'click':
                raise ValueError('action')
            timeout()
            result = active, geometry, extents, states
            if not held:
                held.append(result)
            return result
        def invoke():
            response = node.DoAction(dbus.Int32(0), dbus_interface='org.a11y.atspi.Action', timeout=timeout())
            return bool(response) if isinstance(response, dbus.Boolean) else None
        def post_guard():
            timeout()
            result = subprocess.run(['/usr/bin/xdotool', 'getactivewindow'], check=True,
                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=timeout()).stdout.strip()
            owner = bus.get_object('org.freedesktop.DBus', '/org/freedesktop/DBus').GetConnectionUnixProcessID(
                request['bus'], dbus_interface='org.freedesktop.DBus', timeout=timeout())
            timeout()
            return (int(owner) == request['pid'] and result == str(held[0][0]).encode()
                    and driver['owned_frame'](held[0][0], request['window'])
                    and driver['independent_client_snapshot'](held[0][0]) == held[0][1])
        return 0 if perform_once(snapshot, invoke, deadline, facts, post_guard=post_guard) else 3
    except (OSError, ValueError, TimeoutError, subprocess.SubprocessError):
        return 3
    except Exception:
        # Transport errors are intentionally payload-free and never retried.
        return 3
    finally:
        if not close_transport(bus, driver['publish_observation'], facts):
            return 3
