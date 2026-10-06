"""Closed, read-only census of the owned AccessKit cache; no text is retained."""
import time

LABELS = {'Cancel': 'cancel', 'OK': 'ok', 'Yes': 'yes', 'No': 'no', 'Save': 'save',
          "Don't Save": 'discard', 'Reload': 'reload', 'Close': 'close',
          'Trust and Continue': 'trust', 'Retry': 'retry', 'Continue': 'continue',
          'Open': 'open', 'Dismiss': 'dismiss'}
COUNT_KEYS = ('nodes', 'dialogs', 'alerts', 'modalNodes', 'focusedNodes', 'buttons')


def unavailable():
    return dict(status='unavailable', counts=None, actions=None)


def classify(items, owner, held):
    # CacheItem layout is pinned by atspi-common 0.13.0 and AccessKit Unix
    # 0.22.1: object/app/parent/index/children/interfaces/name/role/description/states.
    if not isinstance(items, (list, tuple)) or not 1 <= len(items) <= 1024:
        raise ValueError('cache census limit')
    counts = dict.fromkeys(COUNT_KEYS, 0)
    actions = dict.fromkeys((*LABELS.values(), 'other'), 0)
    seen = set()
    for item in items:
        if not isinstance(item, (list, tuple)) or len(item) != 10:
            raise ValueError('cache item shape')
        obj, app = item[:2]
        if (len(obj) != 2 or len(app) != 2 or str(obj[0]) != owner or str(app[0]) != owner
                or str(obj[1]) in seen):
            raise ValueError('cache ownership differs')
        seen.add(str(obj[1]))
        role, words = int(item[7]), item[9]
        if (not 0 <= role <= 255 or len(words) != 2
                or any(not 0 <= int(word) <= 0xffffffff for word in words)):
            raise ValueError('cache state shape')
        states = int(words[0]) | (int(words[1]) << 32)
        counts['nodes'] += 1
        counts['dialogs'] += role == 16
        counts['alerts'] += role == 2
        counts['modalNodes'] += bool(states & (1 << 16))
        counts['focusedNodes'] += bool(states & (1 << 12))
        if role == 43:
            counts['buttons'] += 1
            actions[LABELS.get(str(item[6]), 'other')] += 1
    if held not in seen:
        raise ValueError('retained target absent')
    return dict(status='complete', counts=counts, actions=actions)


def capture(request, scope, deadline):
    bus = None
    try:
        import dbus
        def remaining():
            value = min(0.1, deadline - time.monotonic())
            if value <= 0:
                raise TimeoutError()
            return value
        if not scope():
            return unavailable()
        session = dbus.SessionBus()
        address = session.get_object('org.a11y.Bus', '/org/a11y/bus').GetAddress(
            dbus_interface='org.a11y.Bus', timeout=remaining())
        bus = dbus.bus.BusConnection(str(address))
        daemon = bus.get_object('org.freedesktop.DBus', '/org/freedesktop/DBus')
        def owned():
            return int(daemon.GetConnectionUnixProcessID(request['bus'],
                dbus_interface='org.freedesktop.DBus', timeout=remaining())) == request['pid']
        if not owned():
            return unavailable()
        items = bus.get_object(request['bus'], '/org/a11y/atspi/cache').GetItems(
            dbus_interface='org.a11y.atspi.Cache', timeout=remaining())
        result = classify(items, request['bus'], request['path'])
        return result if owned() and scope() and time.monotonic() < deadline else unavailable()
    except Exception:
        # Diagnostic failure cannot authorize input or publish native error text.
        return unavailable()
    finally:
        if bus is not None:
            try:
                bus.close()
            except Exception:
                pass
