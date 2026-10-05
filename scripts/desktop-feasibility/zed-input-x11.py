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
        'right': 'Right', 'submit': 'Return', 'panel-zoom': 'ctrl+alt+z'}


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
                retryContains=None, pointerTarget='unavailable', pointerChild='unavailable',
                clientOriginVerified=None, retryOffsetRelation=None, coordinatePackage=None,
                coordinateRelation=None, coordinateAuthority=None,
                modifierState='unknown', buttonsHeld=None)


def independent_client_snapshot(active):
    """Translate the fresh owned client origin directly, without xdotool."""
    xlib = ctypes.CDLL('libX11.so.6')
    pointer = ctypes.POINTER(ctypes.c_ulong)
    xlib.XOpenDisplay.argtypes = [ctypes.c_char_p]
    xlib.XOpenDisplay.restype = ctypes.c_void_p
    xlib.XCloseDisplay.argtypes = [ctypes.c_void_p]
    xlib.XFree.argtypes = [ctypes.c_void_p]
    xlib.XQueryTree.argtypes = [ctypes.c_void_p, ctypes.c_ulong, pointer, pointer,
                               ctypes.POINTER(pointer), ctypes.POINTER(ctypes.c_uint)]
    xlib.XQueryTree.restype = ctypes.c_int
    xlib.XTranslateCoordinates.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong,
        ctypes.c_int, ctypes.c_int, ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_int), pointer]
    xlib.XTranslateCoordinates.restype = ctypes.c_int
    xlib.XGetGeometry.argtypes = [ctypes.c_void_p, ctypes.c_ulong, pointer,
        ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_uint),
        ctypes.POINTER(ctypes.c_uint), ctypes.POINTER(ctypes.c_uint), ctypes.POINTER(ctypes.c_uint)]
    xlib.XGetGeometry.restype = ctypes.c_int
    display = xlib.XOpenDisplay(None)
    if not display:
        raise ValueError('origin unavailable')
    children, count = pointer(), ctypes.c_uint()
    root, parent, child = ctypes.c_ulong(), ctypes.c_ulong(), ctypes.c_ulong()
    x, y, px, py = (ctypes.c_int() for _ in range(4))
    width, height, border, depth = (ctypes.c_uint() for _ in range(4))
    geometry_root = ctypes.c_ulong()
    try:
        if (not xlib.XQueryTree(display, active, ctypes.byref(root), ctypes.byref(parent),
                ctypes.byref(children), ctypes.byref(count)) or not root.value
                or not xlib.XTranslateCoordinates(display, active, root.value, 0, 0,
                    ctypes.byref(x), ctypes.byref(y), ctypes.byref(child))):
            raise ValueError('origin unavailable')
        if (not xlib.XGetGeometry(display, active, ctypes.byref(geometry_root), ctypes.byref(px),
                ctypes.byref(py), ctypes.byref(width), ctypes.byref(height), ctypes.byref(border), ctypes.byref(depth))
                or geometry_root.value != root.value or not parent.value
                or not 0 < width.value <= 2147483647 or not 0 < height.value <= 2147483647):
            raise ValueError('geometry unavailable')
        return ((x.value, y.value), (px.value, py.value), (width.value, height.value), root.value, parent.value)
    finally:
        if children:
            xlib.XFree(children)
        xlib.XCloseDisplay(display)


def independent_client_origin(active):
    return independent_client_snapshot(active)[0]


def geometry_authority(geometry, first, second, package):
    if first != second or first[2] != geometry[2:]:
        raise ValueError('client geometry changed')
    origin, parent_offset = first[:2]
    delta = (geometry[0] - origin[0], geometry[1] - origin[1])
    if delta == (0, 0):
        return geometry, 'equal', 'unchanged-xdotool'
    if delta != parent_offset or package != 'noble-5build1' or first[3] == first[4]:
        raise ValueError('unproved origin correction')
    return (*origin, *first[2]), 'parent-offset', 'verified-xtranslate'


def coordinate_package(deadline):
    if (os.environ.get('GITHUB_ACTIONS') != 'true'
            or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted'
            or os.environ.get('RUNNER_OS') != 'Linux'):
        return 'unverified'
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise subprocess.TimeoutExpired('fixed-helper', 0)
    result = subprocess.run(['/usr/bin/dpkg-query', '-W', '-f=${Package} ${Version}\n',
        'xdotool', 'libxdo3:amd64'], timeout=min(.5, remaining), stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL, check=True)
    if len(result.stdout) > 128:
        return 'unverified'
    return 'noble-5build1' if set(result.stdout.splitlines()) == {
        b'xdotool 1:3.20160805.1-5build1', b'libxdo3 1:3.20160805.1-5build1'
    } and len(result.stdout.splitlines()) == 2 else 'unverified'


def retry_offset_relation(screen, window, geometry):
    expected = (geometry[0] + window[0], geometry[1] + window[1], *window[2:])
    return 'expected-origin' if screen == expected else 'missing-origin' if screen == window else 'inconsistent'


def pointer_mask(mask):
    # Mod1..Mod5 have configurable meanings; never infer Alt or Super from them.
    if type(mask) is not int or mask < 0 or mask & ~0x1fff:
        return 'unknown', None
    modifiers = mask & 0xff
    state = ({0: 'none', 1: 'shift', 2: 'lock', 4: 'control'}.get(modifiers)
             or ('other-modifier' if modifiers.bit_count() == 1 else 'mixed'))
    return state, bool(mask & 0x1f00)


def pointer_child(frame, active, facts=None):
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
        if facts is not None:
            facts['modifierState'], facts['buttonsHeld'] = pointer_mask(mask.value)
        return ('client' if child.value == active or not child.value and frame == active
                else 'decoration-or-empty' if not child.value
                else 'client-descendant' if owned_frame(child.value, active) else 'other')
    finally:
        xlib.XCloseDisplay(display)


def decoration_crossing_point(frame, client, held_frame):
    if (len(frame)!=5 or len(client)!=5 or client[4]!=held_frame
            or frame[3]!=client[3] or frame[4]!=frame[3] or held_frame==frame[3]):
        raise EntryCrossingFailure('frame-relationship')
    (fx,fy),_,(fw,fh),_,_=frame
    (cx,cy),_,(cw,ch),_,_=client
    # XTranslateCoordinates gives the inner origin; XGetGeometry x/y denotes
    # the outer border corner. Relative x/y equality is therefore not authority.
    if min(fw,fh,cw,ch)<=0 or cx<fx or cx+cw>fx+fw or cy+ch>fy+fh:
        raise EntryCrossingFailure('frame-geometry')
    if cy<=fy:
        raise EntryCrossingFailure('decoration-unavailable')
    point=(cx+cw//2,fy+(cy-fy)//2)
    if not -32768<=point[0]<=32767 or not -32768<=point[1]<=32767:
        raise EntryCrossingFailure('off-display')
    return point


def decoration_child_binding(child, active, frame, root, point, snapshot):
    # Window managers may render decoration in mapped children of the held
    # outer frame. A hover may use that exact measured direct child, never the
    # client or a child belonging to another frame. IDs/geometry stay private.
    if child == 0:
        if snapshot is not None:
            raise EntryCrossingFailure('decoration-child-hit')
        return child, None
    if child in (active, frame, root) or snapshot is None or len(snapshot) != 5:
        raise EntryCrossingFailure('decoration-child-hit')
    (x,y),_,(width,height),measured_root,parent=snapshot
    if (measured_root != root or parent != frame or width <= 0 or height <= 0
            or not x <= point[0] < x+width or not y <= point[1] < y+height):
        raise EntryCrossingFailure('decoration-child-hit')
    return child,snapshot


def crossing_point_hit(root, frame, point):
    # Query the planned point before moving: no foreign/root hover is admitted.
    xlib=ctypes.CDLL('libX11.so.6')
    longp=ctypes.POINTER(ctypes.c_ulong);intp=ctypes.POINTER(ctypes.c_int)
    xlib.XOpenDisplay.argtypes=[ctypes.c_char_p];xlib.XOpenDisplay.restype=ctypes.c_void_p
    xlib.XCloseDisplay.argtypes=[ctypes.c_void_p]
    xlib.XTranslateCoordinates.argtypes=[ctypes.c_void_p,ctypes.c_ulong,ctypes.c_ulong,
        ctypes.c_int,ctypes.c_int,intp,intp,longp]
    xlib.XTranslateCoordinates.restype=ctypes.c_int
    xlib.XQueryPointer.argtypes=[ctypes.c_void_p,ctypes.c_ulong,longp,longp,intp,intp,intp,intp,
        ctypes.POINTER(ctypes.c_uint)]
    xlib.XQueryPointer.restype=ctypes.c_int
    xlib.XGetGeometry.argtypes=[ctypes.c_void_p,ctypes.c_ulong,longp,intp,intp,
        ctypes.POINTER(ctypes.c_uint),ctypes.POINTER(ctypes.c_uint),
        ctypes.POINTER(ctypes.c_uint),ctypes.POINTER(ctypes.c_uint)]
    xlib.XGetGeometry.restype=ctypes.c_int
    display=xlib.XOpenDisplay(None)
    if not display:raise EntryCrossingFailure('query-unavailable')
    try:
        geometry_root=ctypes.c_ulong();gx,gy=ctypes.c_int(),ctypes.c_int()
        width,height,border,depth=(ctypes.c_uint() for _ in range(4))
        if (not xlib.XGetGeometry(display,root,ctypes.byref(geometry_root),ctypes.byref(gx),ctypes.byref(gy),
                ctypes.byref(width),ctypes.byref(height),ctypes.byref(border),ctypes.byref(depth))
                or geometry_root.value!=root):
            raise EntryCrossingFailure('query-unavailable')
        if not 0<=point[0]<width.value or not 0<=point[1]<height.value:
            raise EntryCrossingFailure('off-display')
        x,y=ctypes.c_int(),ctypes.c_int();top,child=ctypes.c_ulong(),ctypes.c_ulong()
        if not xlib.XTranslateCoordinates(display,root,root,*point,ctypes.byref(x),ctypes.byref(y),ctypes.byref(top)):
            raise EntryCrossingFailure('query-unavailable')
        if top.value!=frame:
            raise EntryCrossingFailure('top-frame-hit')
        if not xlib.XTranslateCoordinates(display,root,frame,*point,ctypes.byref(x),ctypes.byref(y),ctypes.byref(child)):
            raise EntryCrossingFailure('query-unavailable')
        actual_root,actual_child=ctypes.c_ulong(),ctypes.c_ulong()
        rx,ry,wx,wy=(ctypes.c_int() for _ in range(4));mask=ctypes.c_uint()
        if (not xlib.XQueryPointer(display,frame,ctypes.byref(actual_root),ctypes.byref(actual_child),
                ctypes.byref(rx),ctypes.byref(ry),ctypes.byref(wx),ctypes.byref(wy),ctypes.byref(mask))
                or actual_root.value!=root or mask.value!=0):
            raise EntryCrossingFailure('pointer-state')
        return child.value,(rx.value,ry.value),actual_child.value
    finally:
        xlib.XCloseDisplay(display)


def guarded_crossing_motion(point,move,prove,deadline,now=None,observation=None,phase='client'):
    if now is None:now=time.monotonic
    def stage(suffix):
        if observation is not None:observation['stage']=phase+'-'+suffix
        if now()>=deadline:raise EntryCrossingFailure('deadline')
    stage('before')
    prove(point,False)
    stage('dispatch')
    move(point)
    stage('after')
    prove(point,True)
    if now()>=deadline:raise EntryCrossingFailure('deadline')
    if observation is not None:observation['stage']=phase+'-complete'


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


class RetryHitFailure(ValueError):
    """Closed failure category; native payloads never enter the receipt."""
    def __init__(self, category):
        super().__init__('retry proof rejected')
        self.category = category


def enabled_toggle_on(states):
    words = tuple(states)
    if len(words) != 2 or any(not 0 <= int(word) <= 4294967295 for word in words):
        return False
    bits = int(words[0]) | (int(words[1]) << 32)
    return all(bits & (1 << bit) for bit in (8, 20, 25, 30)) and not bits & (1 << 6)


def normalized_retry_point(request, active, geometry, facts=None, return_bounds=False, hit_point=None, toggle=False):
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
        if int(role) != (62 if toggle else 43) or (not toggle and str(name) != 'Retry'):
            raise ValueError('retry control changed')
        if toggle:
            states = component.GetState(dbus_interface='org.a11y.atspi.Accessible', timeout=0.5)
            if not enabled_toggle_on(states):
                raise ValueError('toggle state changed')
        screen = tuple(int(value) for value in component.GetExtents(dbus.UInt32(0),
            dbus_interface='org.a11y.atspi.Component', timeout=0.5))
        window = tuple(int(value) for value in component.GetExtents(dbus.UInt32(1),
            dbus_interface='org.a11y.atspi.Component', timeout=0.5))
        if facts is not None:
            accessibility_observation(component, dbus, window, facts)
            facts['retryOffsetRelation'] = retry_offset_relation(screen, window, geometry)
        point = coordinate_point(screen, window, geometry)
        if hit_point is not None:
            ancestor = component
            seen = set()
            for _ in range(16):
                if int(ancestor.GetRole(dbus_interface='org.a11y.atspi.Accessible', timeout=0.2)) == 23:
                    hit = ancestor.GetAccessibleAtPoint(dbus.Int32(hit_point[0] - geometry[0]),
                        dbus.Int32(hit_point[1] - geometry[1]), dbus.UInt32(1),
                        dbus_interface='org.a11y.atspi.Component', timeout=0.2)
                    if tuple(str(value) for value in hit) != (request['bus'], request['path']):
                        raise RetryHitFailure('accessible-hit-mismatch')
                    break
                parent = ancestor.Get('org.a11y.atspi.Accessible', 'Parent',
                    dbus_interface='org.freedesktop.DBus.Properties', timeout=0.2)
                reference = tuple(str(value) for value in parent)
                if (reference in seen or reference[0] != request['bus']
                        or not re.fullmatch(r'/org/a11y/atspi/accessible/[A-Za-z0-9_/]+', reference[1])):
                    raise ValueError('retry root unavailable')
                seen.add(reference)
                ancestor = bus.get_object(*reference)
            else:
                raise ValueError('retry root unavailable')
        return (geometry[0] + window[0], geometry[1] + window[1], window[2], window[3]) if return_bounds else point
    except dbus.DBusException:
        if hit_point is not None:
            raise RetryHitFailure('accessible-query-unavailable') from None
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


def interior_points(bounds):
    x, y, width, height = bounds
    if width < 3 or height < 3:
        raise ValueError('retry interior unavailable')
    points = [(x + width // 2, y + height // 2)]
    points.extend((x + width * column // 4, y + height * row // 4)
                  for row in (1, 2, 3) for column in (1, 2, 3))
    return list(dict.fromkeys(points))


def guarded_retry_proof(point, scope, hit, observation, deadline):
    try:
        if time.monotonic() >= deadline:
            raise RetryHitFailure('deadline')
        if not scope():
            raise RetryHitFailure('identity-rejected')
        observation['guardBeforeVerified'] += 1
        observation['accessibleChecks'] += 1
        if not hit(point):
            raise RetryHitFailure('identity-rejected')
        observation['accessibleExactMatches'] += 1
        if time.monotonic() >= deadline:
            raise RetryHitFailure('deadline')
        if not scope():
            raise RetryHitFailure('identity-rejected')
        observation['guardAfterVerified'] += 1
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        reason = error.category if isinstance(error, RetryHitFailure) else 'identity-rejected'
        observation.update(status=('deadline' if reason == 'deadline' else 'identity-rejected'),
                           failureReason=reason, exactPointerMatched=False, accessibleHitVerified=False)
        raise


class EntryCrossingFailure(RetryHitFailure):
    def __init__(self,reason):
        super().__init__('deadline' if reason=='deadline' else 'identity-rejected')
        self.reason=reason


def sampled_cursor_match(matches, observation):
    if observation is not None:
        observation['cursorChecks'] += 1
    matched = matches()
    owner = getattr(matches, '__self__', None)
    if observation is not None and owner is not None and hasattr(owner, 'last_classification'):
        kind = owner.last_classification
        if kind in ('hand', 'arrow', 'notallowed', 'transparent', 'unknown'):
            counts = observation.setdefault('cursorClasses', dict.fromkeys(
                ('hand', 'arrow', 'notallowed', 'transparent', 'unknown'), 0))
            counts[kind] += 1
        observation['cursorSizeSource'] = owner.size_provenance
    if matched and observation is not None:
        observation['cursorExactMatches'] += 1
    return matched


def guarded_pointer_sample(point, position, child, observation):
    observation['pointerChecks'] = observation.get('pointerChecks', 0) + 1
    if position()[:2] != point:
        raise RetryHitFailure('pointer-position')
    observation['pointerPositionMatches'] = observation.get('pointerPositionMatches', 0) + 1
    if child() not in ('client', 'client-descendant'):
        raise RetryHitFailure('pointer-child')
    observation['pointerChildMatches'] = observation.get('pointerChildMatches', 0) + 1


def select_accessible_retry_point(bounds, move, prove, pointer_proof, deadline, now=time.monotonic):
    """One retained accessible target; cursor appearance is not actionability."""
    point=interior_points(bounds)[0]
    def checked():
        if now()>=deadline:
            raise RetryHitFailure('deadline')
        prove(point)
        if now()>=deadline:
            raise RetryHitFailure('deadline')
    checked()
    move(point)
    checked()
    pointer_proof(point)
    checked()
    return point


def select_live_retry_point(bounds, move, prove, matches, deadline, pause=time.sleep, observation=None, pointer_proof=None):
    # Hover only until one stable public hand cursor and held accessible hit
    # agree. Never replay an activation or select a point after the click.
    if observation is not None:
        for field in ('guardBeforeVerified', 'guardAfterVerified', 'accessibleChecks',
                      'accessibleExactMatches', 'cursorChecks', 'cursorExactMatches'):
            observation.setdefault(field, 0)
        observation.setdefault('failureReason', None)
        if pointer_proof is not None:
            for field in ('pointerChecks', 'pointerPositionMatches', 'pointerChildMatches'):
                observation.setdefault(field, 0)
    unstable_cursor = False

    def proof(point):
        try:
            prove(point)
        except (ValueError, OSError, subprocess.SubprocessError) as error:
            if observation is not None:
                reason = error.category if isinstance(error, RetryHitFailure) else 'identity-rejected'
                observation.update(status=('deadline' if reason == 'deadline' else 'identity-rejected'),
                                   failureReason=reason)
            raise

    for point in interior_points(bounds):
        if time.monotonic() >= deadline:
            if observation is not None:
                observation.update(status='deadline', failureReason='deadline')
            raise ValueError('retry hit deadline')
        proof(point)
        if time.monotonic() >= deadline:
            if observation is not None:
                observation.update(status='deadline', failureReason='deadline')
            raise RetryHitFailure('deadline')
        move(point)
        if observation is not None:
            observation['sampledPoints'] += 1
        # Fresh AX, client and pointer proofs consume this allowance too.
        # Keep the original overall cutoff and five samples; never cache proofs.
        point_deadline = min(deadline, time.monotonic() + 0.25)
        consecutive = 0
        for _ in range(5):
            pause(min(0.02, max(0, point_deadline - time.monotonic())))
            if time.monotonic() >= deadline:
                if observation is not None:
                    observation.update(status='deadline', failureReason='deadline')
                raise RetryHitFailure('deadline')
            if time.monotonic() >= point_deadline:
                break
            proof(point)
            if pointer_proof is not None:
                try:
                    pointer_proof(point)
                except (ValueError, OSError, subprocess.SubprocessError) as error:
                    if observation is not None:
                        reason = error.category if isinstance(error, RetryHitFailure) else 'identity-rejected'
                        observation.update(status='identity-rejected', failureReason=reason)
                    raise
            if time.monotonic() >= point_deadline:
                break
            matched = sampled_cursor_match(matches, observation)
            if time.monotonic() >= deadline:
                if observation is not None:
                    observation.update(status='deadline', failureReason='deadline')
                raise RetryHitFailure('deadline')
            if time.monotonic() >= point_deadline:
                break
            consecutive = consecutive + 1 if matched else 0
            unstable_cursor |= matched
            if consecutive == 2:
                proof(point)
                if time.monotonic() >= deadline:
                    if observation is not None:
                        observation.update(status='deadline', failureReason='deadline')
                    raise RetryHitFailure('deadline')
                if time.monotonic() >= point_deadline:
                    break
                if observation is not None:
                    observation.update(status='matched', exactPointerMatched=True,
                                       accessibleHitVerified=True, failureReason=None)
                return point
    if observation is not None:
        observation.update(status='no-hit', failureReason=('cursor-unstable' if unstable_cursor
                                                         else 'cursor-unmatched'))
    raise ValueError('retry live hit unavailable')


def select_with_ancestor_diagnostic(select, observe, compare, scope, facts, deadline):
    """Observe published ancestry even when rendered hit proof rejects every hover.

    These rectangles are advisory AX layout, not GPUI paint masks. A rejected
    cursor scan keeps its original exception and cannot authorize activation.
    """
    def sample():
        if time.monotonic() >= deadline:
            return None
        try:
            if not scope():
                return None
            result = observe()
            return result if scope() and time.monotonic() < deadline else None
        except (ValueError, OSError, subprocess.SubprocessError):
            return None

    first = sample()
    try:
        point = select()
    except (ValueError, OSError, subprocess.SubprocessError):
        if facts.get('cursorSelection', {}).get('status') == 'no-hit':
            second = sample()
            if first is not None and second is not None:
                facts.update(compare(first, second))
        raise
    second = sample()
    if first is not None and second is not None:
        facts.update(compare(first, second))
    return point


def trace_marker(name, target=None):
    directory = os.environ.get('NANH_ZED_RETRY_TRACE_MARKERS')
    if directory is None:
        return False
    if (name not in ('start', 'end') or sys.platform != 'linux'
            or os.environ.get('GITHUB_ACTIONS') != 'true'
            or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted'
            or os.environ.get('RUNNER_OS') != 'Linux'):
        raise ValueError('invalid hosted trace marker')
    path = Path(directory) / name
    if not path.is_absolute() or path.resolve() != path:
        raise ValueError('invalid trace marker path')
    if name == 'start' and target is not None:
        from zed_hit_geometry import save_target
        save_target(path.parent, target)
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        import stat
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_size != 0:
            raise ValueError('invalid trace marker identity')
    finally:
        os.close(descriptor)
    return True


def retry_click(payload):
    facts = pointer_observation()
    observer = None
    xi_motion = None
    trace_started = False
    try:
        request = json.loads(payload)
        if (type(request) is not dict or set(request) != {'pid', 'window', 'x', 'y', 'bus', 'path'}
                or any(type(request[key]) is not int for key in ('pid', 'window', 'x', 'y'))
                or not isinstance(request['bus'], str) or not re.fullmatch(r':[0-9]+\.[0-9]+', request['bus'])
                or not isinstance(request['path'], str) or not re.fullmatch(r'/org/a11y/atspi/accessible/[A-Za-z0-9_/]+', request['path'])
                or not 1 < request['pid'] <= 2147483647 or not 0 < request['window'] <= 4294967295
                or any(not -32768 <= request[key] <= 32767 for key in ('x', 'y'))):
            return 2
        hit_policy=os.environ.get('NANH_ZED_RETRY_HIT_POLICY')
        accessible_policy=hit_policy=='accessibility'
        if hit_policy is not None:
            if (not accessible_policy or os.environ.get('NANH_ZED_CURSOR_HIT')!='1'
                    or sys.platform!='linux' or os.environ.get('GITHUB_ACTIONS')!='true'
                    or os.environ.get('RUNNER_ENVIRONMENT')!='github-hosted'):
                return 18
            facts['retryHitPolicy']=hit_policy
        crossing=os.environ.get('NANH_ZED_ENTER_POLICY')
        if crossing is not None and (crossing!='owned-decoration-crossing'
                or sys.platform!='linux' or os.environ.get('GITHUB_ACTIONS')!='true'
                or os.environ.get('RUNNER_ENVIRONMENT')!='github-hosted'
                or os.environ.get('RUNNER_OS')!='Linux'
                or os.environ.get('NANH_ZED_CURSOR_HIT')!='1'
                or os.environ.get('NANH_ZED_XRECORD')!='1'):
            return 18
        if crossing is not None:
            facts['entryCrossing']=dict(stage='preflight',failureReason=None)
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
        raw_geometry = run(['getwindowgeometry', '--shell', str(active)], 'geometry')
        first_geometry = independent_client_snapshot(active)
        facts['coordinatePackage'] = coordinate_package(deadline) if first_geometry[0] != raw_geometry[:2] else 'unverified'
        second_geometry = independent_client_snapshot(active)
        if owned_foreground() != (0, active) or time.monotonic() >= deadline:
            return 11
        facts['clientOriginVerified'] = first_geometry[0] == raw_geometry[:2]
        if active == request['window'] and first_geometry[0] != raw_geometry[:2]:
            return 18
        geometry, relation, authority = geometry_authority(raw_geometry, first_geometry,
            second_geometry, facts['coordinatePackage'])
        facts['coordinateRelation'], facts['coordinateAuthority'] = relation, authority
        point = normalized_retry_point(request, active, geometry, facts)
        live_cursor = None
        if os.environ.get('NANH_ZED_CURSOR_HIT') == '1':
            facts['cursorSelection'] = dict(status='unavailable', sampledPoints=0,
                exactPointerMatched=False, accessibleHitVerified=False,
                guardBeforeVerified=0, guardAfterVerified=0, accessibleChecks=0,
                accessibleExactMatches=0, cursorChecks=0, cursorExactMatches=0, failureReason=None)
            if (sys.platform != 'linux' or os.environ.get('GITHUB_ACTIONS') != 'true'
                    or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted'
                    or os.environ.get('RUNNER_OS') != 'Linux'):
                return 18
            import runpy
            module = runpy.run_path(str(Path(__file__).with_name('zed-cursor-hit.py')))
            held_bounds = normalized_retry_point(request, active, geometry, return_bounds=True)
            def cursor_scope():
                return (owned_foreground() == (0, active)
                        and independent_client_snapshot(active) == second_geometry)
            def exact_accessible_hit(candidate):
                current={}
                bounds=normalized_retry_point(request,active,geometry,current,
                    return_bounds=True,hit_point=candidate)
                return (bounds==held_bounds and current.get('enabled') is True
                    and current.get('sensitive') is True and current.get('defunct') is False)
            def prove_hit(candidate):
                return guarded_retry_proof(candidate, cursor_scope,
                    exact_accessible_hit if accessible_policy else lambda point: normalized_retry_point(request, active, geometry,
                        return_bounds=True, hit_point=point) == held_bounds,
                    facts['cursorSelection'], deadline)
            def prove_pointer(candidate):
                if not cursor_scope():
                    raise RetryHitFailure('identity-rejected')
                if xi_motion is not None:
                    xi_motion.poll()
                guarded_pointer_sample(candidate,
                    lambda: run(['getmouselocation', '--shell'], 'position'),
                    lambda: pointer_child(request['window'], active), facts['cursorSelection'])
                if not cursor_scope():
                    raise RetryHitFailure('identity-rejected')
            if os.environ.get('NANH_ZED_XRECORD') == '1':
                # GPUI can block parent input for an unmapped owned dialog.
                # Capture before hover, while the original budget still permits
                # the complete tree census; this never grants click authority.
                dialog_module = runpy.run_path(str(Path(__file__).with_name('zed-transient-dialogs.py')))
                facts['transientDialogsBeforeHover'] = dialog_module['capture'](
                    active, request['pid'], cursor_scope, deadline)
            live_cursor = module['PointerShape'](request['pid'], cursor_scope, deadline)
            if (os.environ.get('NANH_ZED_XI2_PAYLOAD') == '1'
                    and os.environ.get('NANH_ZED_XRECORD') == '1'):
                motion_module = runpy.run_path(str(Path(__file__).with_name('zed-xi2-motion.py')))
                xi_motion = motion_module['Observer'](active, request['pid'], second_geometry[3],
                    second_geometry[0], interior_points(held_bounds), cursor_scope, deadline)
            if os.environ.get('NANH_ZED_XRECORD') == '1':
                record_module = runpy.run_path(str(Path(__file__).with_name('zed-xrecord-supervisor.py')))
                remaining = deadline-time.monotonic()
                if remaining > 1:
                    observer = record_module['Observer'](request['pid'], active, cursor_scope,
                        budget=min(3,remaining))

            move=lambda candidate:run(['mousemove','--',str(candidate[0]),str(candidate[1])])
            if crossing is not None:
                entry=facts['entryCrossing'];entry['stage']='observer'
                if observer is None or observer.stage!='armed' or observer.result is not None:
                    raise EntryCrossingFailure('observer-unavailable')
                entry['stage']='frame-measurement'
                frame_geometry=independent_client_snapshot(request['window'])
                entry['stage']='candidate'
                decoration=decoration_crossing_point(frame_geometry,second_geometry,request['window'])
                held_decoration=None
                def crossing_proof(candidate,after,decoration=False):
                    nonlocal held_decoration
                    if (not cursor_scope()
                            or independent_client_snapshot(request['window'])!=frame_geometry):
                        raise EntryCrossingFailure('identity-changed')
                    child,position,actual_child=crossing_point_hit(
                        second_geometry[3],request['window'],candidate)
                    expected=active
                    if decoration:
                        measured=independent_client_snapshot(child) if child else None
                        binding=decoration_child_binding(child,active,request['window'],
                            second_geometry[3],candidate,measured)
                        if held_decoration is None:
                            held_decoration=binding
                        elif binding!=held_decoration:
                            raise EntryCrossingFailure('identity-changed')
                        expected=held_decoration[0]
                    if child!=expected:
                        raise EntryCrossingFailure('decoration-child-hit' if decoration else 'client-child-hit')
                    if after and position!=candidate:
                        raise EntryCrossingFailure('pointer-position')
                    if after and actual_child!=expected:
                        raise EntryCrossingFailure('pointer-child-current')
                    if (not cursor_scope()
                            or independent_client_snapshot(request['window'])!=frame_geometry):
                        raise EntryCrossingFailure('identity-changed')
                    if decoration and child and independent_client_snapshot(child)!=held_decoration[1]:
                        raise EntryCrossingFailure('identity-changed')
                guarded_crossing_motion(decoration,move,
                    lambda candidate,after:crossing_proof(candidate,after,True),deadline,
                    observation=entry,phase='decoration')
                ordinary_move=move
                move=lambda candidate:guarded_crossing_motion(candidate,ordinary_move,crossing_proof,deadline,
                    observation=entry)

            def select():
                if accessible_policy:
                    for key in ('pointerChecks','pointerPositionMatches','pointerChildMatches'):
                        facts['cursorSelection'].setdefault(key,0)
                    selected=select_accessible_retry_point(held_bounds,move,prove_hit,prove_pointer,deadline)
                    facts['cursorSelection'].update(status='accessible-hit',sampledPoints=1,
                        accessibleHitVerified=True,exactPointerMatched=False,failureReason=None)
                    return selected
                return select_live_retry_point(held_bounds,
                    move,
                    prove_hit, live_cursor.matches, deadline, observation=facts['cursorSelection'],
                    pointer_proof=prove_pointer)
            if os.environ.get('NANH_ZED_XRECORD') == '1':
                ancestor_module = runpy.run_path(str(Path(__file__).with_name('zed-atspi-observe.py')))
                facts.update(ancestor_module['ancestor_result']())
                point = select_with_ancestor_diagnostic(select,
                    lambda: ancestor_module['retry_ancestors'](request, None, deadline),
                    ancestor_module['compare_ancestors'], cursor_scope, facts, deadline)
            else:
                point = select()
        ancestor_module, ancestor_before = None, None
        if (live_cursor is None and os.environ.get('NANH_ZED_XRECORD') == '1'
                and sys.platform == 'linux'
                and os.environ.get('GITHUB_ACTIONS') == 'true'
                and os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted'
                and os.environ.get('RUNNER_OS') == 'Linux'):
            import runpy
            ancestor_module = runpy.run_path(str(Path(__file__).with_name('zed-atspi-observe.py')))
            ancestor_before = ancestor_module['retry_ancestors'](request, None, deadline)
            facts.update(ancestor_module['ancestor_result']())
        stage = 14
        # --sync waits for motion and can hang when the pointer is already here.
        # Dispatch once and prove the resulting position instead.
        if live_cursor is None:
            run(['mousemove', '--', str(point[0]), str(point[1])])
        px, py, pointer_window = run(['getmouselocation', '--shell'], 'position')
        if (px, py) != point:
            return 14
        facts['pointerTarget'] = ('client' if pointer_window == active else
            'owned-frame' if pointer_window == request['window'] else
            'client-descendant' if owned_frame(pointer_window, active) else 'foreign')
        facts['pointerChild'] = pointer_child(request['window'], active, facts)
        stage = 15
        if owned_foreground() != (0, active):
            return 11
        if ancestor_module is not None:
            ancestor_after = ancestor_module['retry_ancestors'](request, None, deadline)
            if owned_foreground() != (0, active):
                return 11
            if run(['getmouselocation', '--shell'], 'position') != (px, py, pointer_window):
                return 14
            facts.update(ancestor_module['compare_ancestors'](ancestor_before, ancestor_after))
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
                            and run(['getwindowgeometry', '--shell', str(active)], 'geometry') == raw_geometry
                            and independent_client_snapshot(active) == second_geometry
                            and pointer_child(request['window'], active) == 'client')
                except (OSError, ValueError, subprocess.SubprocessError):
                    return False

            remaining = deadline - time.monotonic()
            facts['inputDelivery'] = dict(status='unavailable', stage='budget-insufficient', pressCount=None,
                                          releaseCount=None, orderedPair=None)
            if remaining > 1 and observer is None:
                module = runpy.run_path(str(Path(__file__).with_name('zed-xrecord-supervisor.py')))
                observer = module['Observer'](request['pid'], active, record_scope,
                                              budget=min(3, remaining - .5))
            if (not record_scope()
                    or (live_cursor is None and normalized_retry_point(request, active, geometry) != point)):
                if observer is not None:
                    facts['inputDelivery'] = observer.finish()
                return 18
        if (independent_client_snapshot(active) != second_geometry
                or run(['getwindowgeometry', '--shell', str(active)], 'geometry') != raw_geometry
                or owned_foreground() != (0, active) or time.monotonic() >= deadline):
            return 18
        stage = 16
        if live_cursor is not None:
            prove_hit(point)
            if run(['getmouselocation', '--shell'], 'position') != (px, py, pointer_window):
                facts['cursorSelection'].update(status='identity-rejected', failureReason='identity-rejected',
                                               exactPointerMatched=False, accessibleHitVerified=False)
                return 18
            if not accessible_policy and not sampled_cursor_match(live_cursor.matches, facts['cursorSelection']):
                facts['cursorSelection'].update(status='no-hit', failureReason='cursor-unstable',
                                               exactPointerMatched=False, accessibleHitVerified=False)
                return 18
        if (os.environ.get('NANH_ZED_XRECORD') == '1'
                and sys.platform == 'linux'
                and os.environ.get('GITHUB_ACTIONS') == 'true'
                and os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted'
                and os.environ.get('RUNNER_OS') == 'Linux'):
            # Reuse the complete mapped/unmapped census at the actual dispatch
            # boundary. X properties do not expose GPUI's private child set,
            # so this receipt remains advisory and never grants actionability.
            import runpy
            dialog_module = runpy.run_path(str(Path(__file__).with_name('zed-transient-dialogs.py')))
            facts['transientDialogsBeforeDispatch'] = dialog_module['capture'](
                active, request['pid'], record_scope, deadline)
            # The passive walk can consume time or observe an owner transition;
            # retain the original click guard and cutoff after it completes.
            if (time.monotonic() >= deadline or not record_scope()
                    or independent_client_snapshot(active) != second_geometry):
                return 18
        # One ordinary activation, never another press after an uncertain receipt.
        trace_started = trace_marker('start', dict(
            point=[point[0] - geometry[0], point[1] - geometry[1]],
            viewport=list(geometry[2:]),
            bounds=[held_bounds[0] - geometry[0], held_bounds[1] - geometry[1], *held_bounds[2:]])
            if live_cursor is not None else None)
        run(['click', '--clearmodifiers', '1'])
        if os.environ.get('NANH_ZED_XRECORD') == '1':
            facts['targetAfterClick'] = 'unavailable'
            if live_cursor is not None and time.monotonic() < deadline:
                try:
                    module = runpy.run_path(str(Path(__file__).with_name('zed-atspi-retry.py')))
                    facts['targetAfterClick'] = module['observe_pointer_target'](
                        dict(request,bounds=held_bounds,clientOrigin=geometry[:2]), cursor_scope, deadline)
                except Exception:
                    pass
        if observer is not None:
            facts['inputDelivery'] = observer.finish()
        return 0
    except (ValueError, TypeError, OSError, subprocess.SubprocessError, ImportError) as error:
        entry=facts.get('entryCrossing')
        if entry is not None and not entry['stage'].endswith('-complete'):
            entry['failureReason']=(error.reason if isinstance(error,EntryCrossingFailure)
                else 'motion-uncertain' if entry['stage'].endswith('-dispatch')
                else 'deadline' if isinstance(error,subprocess.TimeoutExpired)
                else 'query-unavailable')
        # A blocked GPUI parent forces Arrow and ignores all input, even if its
        # owned transient Dialog is unmapped. Observe only; never dismiss it.
        if (facts.get('cursorSelection', {}).get('status') == 'no-hit'
                and os.environ.get('NANH_ZED_XRECORD') == '1'
                and sys.platform == 'linux'
                and os.environ.get('GITHUB_ACTIONS') == 'true'
                and os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted'
                and os.environ.get('RUNNER_OS') == 'Linux'):
            facts['transientDialogs'] = dict(state='unavailable',
                ownedTransientDialogs=None, mappedOwnedTransientDialogs=None)
            try:
                scope = locals().get('cursor_scope')
                client = locals().get('active')
                cutoff = locals().get('deadline')
                held_request = locals().get('request')
                if (not callable(scope) or type(client) is not int or client <= 0
                        or type(cutoff) not in (int, float) or type(held_request) is not dict
                        or type(held_request.get('pid')) is not int):
                    return locals().get('stage', 2)
                import runpy
                module = runpy.run_path(str(Path(__file__).with_name('zed-transient-dialogs.py')))
                facts['transientDialogs'] = module['capture'](
                    client, held_request['pid'], scope, cutoff)
            except (ValueError, TypeError, OSError, ImportError):
                pass
        return locals().get("stage", 2)
    finally:
        if xi_motion is not None:
            facts['xi2Motion'] = xi_motion.result()
            xi_motion.close()
        if observer is not None:
            facts['inputDelivery'] = observer.finish()
        live_cursor = locals().get('live_cursor')
        if live_cursor is not None:
            live_cursor.close()
        observer = locals().get('observer')
        if observer is not None:
            observer.close()
            facts['inputDelivery'] = observer.result
        if trace_started:
            try:
                trace_marker('end')
            except (ValueError, OSError):
                pass  # An unmatched marker invalidates the separate trace receipt.
        publish_observation(facts)


def zoom_hover(payload):
    """Read-only retained ToggleButton hit proof surrounding one ordinary hover."""
    try:
        request = json.loads(payload)
        if (type(request) is not dict or set(request) != {'pid', 'window', 'x', 'y', 'bus', 'path', 'bounds'}
                or any(type(request[key]) is not int for key in ('pid', 'window', 'x', 'y'))
                or not 1 < request['pid'] <= 2147483647 or not 0 < request['window'] <= 4294967295
                or not isinstance(request['bus'], str) or not re.fullmatch(r':[0-9]+\.[0-9]+', request['bus'])
                or not isinstance(request['path'], str) or not re.fullmatch(r'/org/a11y/atspi/accessible/[A-Za-z0-9_/]+', request['path'])
                or type(request['bounds']) is not list or len(request['bounds']) != 4
                or any(type(value) is not int or not -2147483648 <= value <= 2147483647 for value in request['bounds'])
                or any(value <= 0 for value in request['bounds'][2:])
                or any(not -32768 <= request[key] <= 32767 for key in ('x', 'y'))):
            return 2
        deadline = time.monotonic() + 3
        point = (request['x'], request['y'])
        def owned_snapshot():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise ValueError('hover deadline')
            active_bytes = subprocess.check_output(['/usr/bin/xdotool', 'getactivewindow'],
                timeout=remaining, stderr=subprocess.DEVNULL)
            if len(active_bytes) > 32 or not active_bytes.strip().isdigit():
                raise ValueError('invalid active window')
            active = int(active_bytes)
            if not owned_frame(active, request['window']):
                raise ValueError('window ownership changed')
            pid = subprocess.check_output(['/usr/bin/xdotool', 'getwindowpid', str(active)],
                timeout=max(0.001, deadline - time.monotonic()), stderr=subprocess.DEVNULL)
            if len(pid) > 32 or not pid.strip().isdigit() or int(pid) != request['pid']:
                raise ValueError('process ownership changed')
            snapshot = independent_client_snapshot(active)
            geometry = (*snapshot[0], *snapshot[2])
            if (normalized_retry_point(request, active, geometry, return_bounds=True, hit_point=point, toggle=True)
                    != tuple(request['bounds']) or point != (request['bounds'][0] + request['bounds'][2] // 2,
                                                            request['bounds'][1] + request['bounds'][3] // 2)):
                raise ValueError('toggle geometry changed')
            if time.monotonic() >= deadline:
                raise ValueError('hover deadline')
            return active, geometry
        before = owned_snapshot()
        subprocess.run(['/usr/bin/xdotool', 'mousemove', '--sync', str(point[0]), str(point[1])],
            check=True, timeout=max(0.001, deadline - time.monotonic()),
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if owned_snapshot() != before:
            raise ValueError('hover identity changed')
        return 0
    except Exception:
        return 3


def main():
    if len(sys.argv) != 2:
        return 2
    limit = 32768 if sys.argv[1] == 'atspi-observe' else 4096
    payload = sys.stdin.buffer.read(limit + 1)
    if len(payload) > limit:
        return 2
    if sys.argv[1] == 'atspi-observe':
        import runpy
        return runpy.run_path(str(Path(__file__).with_name('zed-atspi-observe.py')))['run'](payload)
    if sys.argv[1] == 'zoom-hover':
        return zoom_hover(payload)
    if sys.argv[1] == 'retry-atspi':
        import runpy
        return runpy.run_path(str(Path(__file__).with_name('zed-atspi-retry.py')))['run'](payload)
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
