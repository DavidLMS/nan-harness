#!/usr/bin/env python3
"""Supervised read-only state of one caller-held source-labelled editor."""
import json
import os
import sys
import time

LABEL = 'Write your prompt to Claude'
FIELDS = ('visible', 'showing', 'boundsPositive', 'checkedAncestorCount', 'hiddenAncestorCount')


def result(status='unavailable', stage='source', values=None):
    return dict(schemaVersion=1, mechanism='claude-linux-classic-visibility', diagnosticsOnly=True,
                status=status, stage=stage, **(values or dict.fromkeys(FIELDS)))


def observe(held, adapter, deadline, clock=time.monotonic):
    stage = 'owner'
    try:
        def budget():
            if clock() >= deadline:
                raise TimeoutError()
        def owned(node):
            budget()
            if adapter.owner(node) != held['pid']:
                raise ValueError()
            budget()
        def editor():
            nonlocal stage
            owned((held['bus'], held['path']))
            stage = 'identity'
            node = (held['bus'], held['path'])
            role, name, description = adapter.identity(node)
            budget()
            if role not in (61, 78, 79) or LABEL not in (name, description):
                raise ValueError()
            stage = 'state'
            bits = adapter.state(node)
            budget()
            if type(bits) is not int or not 0 <= bits <= 0xffffffffffffffff:
                raise ValueError()
            if bits & (1 << 6) or not bits & (1 << 7):
                raise ValueError()
            stage = 'bounds'
            bounds = adapter.bounds(node)
            budget()
            if len(bounds) != 4 or any(type(v) is not int for v in bounds):
                raise ValueError()
            return role, bits, tuple(bounds)
        before = editor()
        current = (held['bus'], held['path'])
        seen = {current}
        checked = hidden = 0
        while True:
            stage = 'parent'
            budget()
            parent = adapter.parent(current)
            budget()
            if not isinstance(parent, tuple) or len(parent) != 2 or any(type(v) is not str for v in parent):
                raise ValueError()
            owned(parent)
            role = adapter.role(parent)
            budget()
            if role == 75:
                if 'root' in held and parent != held['root']:
                    raise ValueError()
                break
            if parent in seen:
                raise ValueError()
            if checked == 32:
                return result('limit', 'parent')
            seen.add(parent)
            stage = 'state'
            bits = adapter.state(parent)
            budget()
            if type(bits) is not int or not 0 <= bits <= 0xffffffffffffffff:
                raise ValueError()
            checked += 1
            hidden += int(not bits & ((1 << 25) | (1 << 30)))
            current = parent
        after = editor()
        if before != after:
            return result('changed', 'identity')
        bits, bounds = after[1:]
        return result('complete', 'complete', dict(visible=bool(bits & (1 << 30)),
            showing=bool(bits & (1 << 25)), boundsPositive=bounds[2] > 0 and bounds[3] > 0,
            checkedAncestorCount=checked, hiddenAncestorCount=hidden))
    except TimeoutError:
        return result(stage='deadline')
    except Exception:
        return result(stage=stage)


def resolve(root, adapter, deadline, clock=time.monotonic):
    """Unique source editor below the already cached owned Application endpoint."""
    def query(method, node):
        if clock() >= deadline:
            raise TimeoutError()
        value = getattr(adapter, method)(node)
        if clock() >= deadline:
            raise TimeoutError()
        return value
    endpoint = (root['bus'], root['path'])
    if query('owner', endpoint) != root['pid'] or query('role', endpoint) != 75:
        raise ValueError()
    stack, seen, matches = [(endpoint, 0)], set(), []
    while stack:
        node, depth = stack.pop()
        if node in seen or len(seen) >= 1024 or depth > 32:
            raise ValueError()
        seen.add(node)
        if query('owner', node) != root['pid']:
            raise ValueError()
        role = query('role', node)
        if role in (61, 78, 79):
            identity = query('identity', node)
            bits = query('state', node)
            if type(bits) is not int or not 0 <= bits <= 0xffffffffffffffff:
                raise ValueError()
            if LABEL in identity[1:] and bits & (1 << 7) and not bits & (1 << 6):
                matches.append(node)
                if len(matches) > 1:
                    raise ValueError()
        children = query('children', node)
        if type(children) is not list or len(children) > 1024 - len(seen):
            raise ValueError()
        for child in children:
            if type(child) is not tuple or len(child) != 2 or any(type(v) is not str or not v or len(v)>256 for v in child):
                raise ValueError()
            stack.append((child, depth + 1))
    if len(matches) != 1 or query('owner', endpoint) != root['pid'] or query('role', endpoint) != 75:
        raise ValueError()
    return dict(root, bus=matches[0][0], path=matches[0][1], root=endpoint)


def inspect(root, adapter, deadline, clock=time.monotonic):
    try:
        held = resolve(root, adapter, deadline, clock)
        verdict = observe(held, adapter, deadline, clock)
        if verdict['status'] != 'complete':
            return verdict
        after = resolve(root, adapter, deadline, clock)
        if (held['bus'], held['path']) != (after['bus'], after['path']):
            return result('changed', 'identity')
        return verdict
    except TimeoutError:
        return result(stage='deadline')
    except Exception:
        return result(stage='source')


class Adapter:
    def __init__(self, deadline):
        import dbus
        self.dbus, self.deadline = dbus, deadline
        session = dbus.SessionBus()
        address = session.get_object('org.a11y.Bus', '/org/a11y/bus').GetAddress(
            dbus_interface='org.a11y.Bus', timeout=self.remaining())
        self.bus = dbus.bus.BusConnection(str(address))
    def remaining(self):
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError()
        return remaining
    def call(self, node, method, interface='org.a11y.atspi.Accessible', *args):
        return getattr(self.bus.get_object(*node), method)(*args, dbus_interface=interface,
                                                         timeout=self.remaining())
    def owner(self, node):
        return int(self.bus.get_object('org.freedesktop.DBus', '/org/freedesktop/DBus').GetConnectionUnixProcessID(
            node[0], dbus_interface='org.freedesktop.DBus', timeout=self.remaining()))
    def role(self, node):
        return int(self.call(node, 'GetRole'))
    def identity(self, node):
        return self.role(node), str(self.call(node, 'Get', 'org.freedesktop.DBus.Properties',
            'org.a11y.atspi.Accessible', 'Name')), str(self.call(node, 'Get',
            'org.freedesktop.DBus.Properties', 'org.a11y.atspi.Accessible', 'Description'))
    def state(self, node):
        words = self.call(node, 'GetState')
        if len(words) != 2 or any(not 0 <= int(v) <= 0xffffffff for v in words):
            raise ValueError()
        return int(words[0]) | (int(words[1]) << 32)
    def bounds(self, node):
        return tuple(int(v) for v in self.call(node, 'GetExtents', 'org.a11y.atspi.Component', self.dbus.UInt32(0)))
    def children(self, node):
        return [tuple(str(v) for v in child) for child in self.call(node, 'GetChildren')]
    def parent(self, node):
        return tuple(str(v) for v in self.call(node, 'Get', 'org.freedesktop.DBus.Properties',
                                              'org.a11y.atspi.Accessible', 'Parent'))


def main():
    scope = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Linux',
                 NANH_CLAUDE_LINUX_SOURCE_POLICY='official-2.9939.4', NANH_DESKTOP_QUALIFICATION_MODE='startup-baseline')
    verdict = result(stage='policy')
    if all(os.environ.get(k) == v for k, v in scope.items()):
        try:
            raw = sys.stdin.buffer.read(4097)
            request = json.loads(raw) if len(raw) <= 4096 else None
            if (type(request) is not dict or set(request) != {'pid', 'bus', 'path', 'remaining'}
                    or type(request['pid']) is not int or not 0 < request['pid'] <= 0xffffffff
                    or not all(type(request[k]) is str and 0 < len(request[k]) <= 256 for k in ('bus', 'path'))
                    or type(request['remaining']) not in (float, int) or not 0 < request['remaining'] <= 45):
                raise ValueError()
            deadline = time.monotonic() + request['remaining']
            verdict = inspect(request, Adapter(deadline), deadline)
        except Exception:
            verdict = result(stage='source')
    print(json.dumps(verdict, separators=(',', ':')))

if __name__ == '__main__':
    main()
