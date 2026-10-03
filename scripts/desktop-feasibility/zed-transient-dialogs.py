#!/usr/bin/env python3
"""Passive owned GPUI dialog inventory; never input or application content."""
import ctypes as C
import time


class Unavailable(Exception):
    def __init__(self, state):
        super().__init__('dialog observation unavailable')
        self.state = state


def result(state, total=None, mapped=None):
    return dict(state=state, ownedTransientDialogs=total, mappedOwnedTransientDialogs=mapped)


def observe(client, pid, backend, guard, deadline, clock=time.monotonic):
    def fresh():
        if clock() >= deadline:
            raise Unavailable('deadline')
        if not guard() or backend.pid(client) != pid:
            raise Unavailable('identity-rejected')
        if clock() >= deadline:
            raise Unavailable('deadline')
    try:
        fresh()
        queue, visited = [(backend.root(), 0)], set()
        total = mapped = 0
        while queue:
            if clock() >= deadline:
                raise Unavailable('deadline')
            window, depth = queue.pop()
            if window in visited:
                raise Unavailable('query-failed')
            visited.add(window)
            if len(visited) > 512 or depth > 8:
                raise Unavailable('limit')
            if window != client and backend.dialog_parent(window) == client:
                if backend.pid(window) == pid:
                    total += 1
                    if total > 32:
                        raise Unavailable('limit')
                    mapped += backend.mapped(window)
            children = backend.children(window)
            if len(children) > 512:
                raise Unavailable('limit')
            queue.extend((child, depth + 1) for child in children)
        fresh()
        return result('complete', total, mapped)
    except Unavailable as error:
        return result(error.state)
    except (ValueError, OSError, TypeError):
        return result('query-failed')


class Spec(C.Structure):
    _fields_ = [('client', C.c_ulong), ('mask', C.c_uint)]


class Identity(C.Structure):
    _fields_ = [('spec', Spec), ('length', C.c_long), ('value', C.c_void_p)]


class Attributes(C.Structure):
    _fields_ = [(name, C.c_int) for name in ('x', 'y', 'width', 'height', 'border_width', 'depth')]
    _fields_ += [('visual', C.c_void_p), ('root', C.c_ulong), ('window_class', C.c_int),
                ('bit_gravity', C.c_int), ('win_gravity', C.c_int), ('backing_store', C.c_int),
                ('backing_planes', C.c_ulong), ('backing_pixel', C.c_ulong),
                ('save_under', C.c_int), ('colormap', C.c_ulong), ('map_installed', C.c_int),
                ('map_state', C.c_int), ('all_event_masks', C.c_long),
                ('your_event_mask', C.c_long), ('do_not_propagate_mask', C.c_long),
                ('override_redirect', C.c_int), ('screen', C.c_void_p)]


class NativeTree:
    """Existing X display only, XRes PID authority; no selection/grab/input."""
    def __init__(self, deadline):
        self.display = None
        self.deadline = deadline
        self.error = False
        self.old_error_handler = None
        self.x = C.CDLL('libX11.so.6')
        self.res = C.CDLL('libXRes.so.1')
        p, u, i = C.c_void_p, C.c_ulong, C.c_int
        up = C.POINTER(u)
        def bind(lib, name, args, returned):
            fun = getattr(lib, name)
            fun.argtypes, fun.restype = args, returned
        bind(self.x, 'XOpenDisplay', [C.c_char_p], p)
        bind(self.x, 'XCloseDisplay', [p], i)
        bind(self.x, 'XDefaultRootWindow', [p], u)
        bind(self.x, 'XFree', [p], i)
        bind(self.x, 'XSetErrorHandler', [p], p)
        bind(self.x, 'XInternAtom', [p, C.c_char_p, i], u)
        bind(self.x, 'XQueryTree', [p,u,up,up,C.POINTER(up),C.POINTER(C.c_uint)], i)
        bind(self.x, 'XGetWindowProperty', [p,u,u,C.c_long,C.c_long,i,u,up,C.POINTER(i),
             up,up,C.POINTER(C.POINTER(C.c_ubyte))], i)
        bind(self.x, 'XGetWindowAttributes', [p,u,C.POINTER(Attributes)], i)
        bind(self.res, 'XResQueryVersion', [p,C.POINTER(i),C.POINTER(i)], i)
        bind(self.res, 'XResQueryClientIds', [p,C.c_long,C.POINTER(Spec),
             C.POINTER(C.c_long),C.POINTER(C.POINTER(Identity))], i)
        bind(self.res, 'XResGetClientPid', [C.POINTER(Identity)], i)
        bind(self.res, 'XResClientIdsDestroy', [C.c_long,C.POINTER(Identity)], None)
        self.check()
        self.display = self.x.XOpenDisplay(None)
        if not self.display:
            raise Unavailable('unavailable')
        handler_type = C.CFUNCTYPE(i,p,p)
        def failed_query(_display, _event):
            self.error = True
            return 0
        self.error_handler = handler_type(failed_query)
        self.old_error_handler = self.x.XSetErrorHandler(C.cast(self.error_handler,p))
        major, minor = i(), i()
        self.check()
        if not self.res.XResQueryVersion(self.display,C.byref(major),C.byref(minor)) or (major.value,minor.value)<(1,2):
            self.close()
            raise Unavailable('unavailable')
        self.atoms = {}
        for name in ('WM_TRANSIENT_FOR','_NET_WM_WINDOW_TYPE','_NET_WM_WINDOW_TYPE_DIALOG'):
            self.check()
            self.atoms[name] = self.x.XInternAtom(self.display,name.encode(),True)

    def check(self):
        if time.monotonic() >= self.deadline:
            raise Unavailable('deadline')
        if self.error:
            raise Unavailable('query-failed')

    def root(self):
        self.check()
        return self.x.XDefaultRootWindow(self.display)

    def children(self, window):
        self.check()
        root, parent, count = C.c_ulong(), C.c_ulong(), C.c_uint()
        children = C.POINTER(C.c_ulong)()
        try:
            if not self.x.XQueryTree(self.display,window,C.byref(root),C.byref(parent),C.byref(children),C.byref(count)):
                raise Unavailable('query-failed')
            if count.value > 512:
                raise Unavailable('limit')
            return [children[index] for index in range(count.value)]
        finally:
            if children:
                self.x.XFree(children)

    def property(self, window, name, expected_type):
        self.check()
        atom = self.atoms[name]
        if not atom:
            return []
        kind, count, remaining, fmt = C.c_ulong(), C.c_ulong(), C.c_ulong(), C.c_int()
        value = C.POINTER(C.c_ubyte)()
        try:
            if self.x.XGetWindowProperty(self.display,window,atom,0,32,False,expected_type,
                    C.byref(kind),C.byref(fmt),C.byref(count),C.byref(remaining),C.byref(value)) != 0:
                raise Unavailable('query-failed')
            if kind.value == 0:
                return []
            if kind.value != expected_type or fmt.value != 32 or remaining.value or count.value > 32:
                raise Unavailable('query-failed')
            data = C.cast(value,C.POINTER(C.c_ulong))
            return [data[index] for index in range(count.value)]
        finally:
            if value:
                self.x.XFree(value)

    def dialog_parent(self, window):
        self.check()
        if self.atoms['_NET_WM_WINDOW_TYPE_DIALOG'] not in self.property(window,'_NET_WM_WINDOW_TYPE',4):
            return None
        parent = self.property(window,'WM_TRANSIENT_FOR',33)
        if len(parent) != 1:
            raise Unavailable('query-failed')
        return parent[0]

    def pid(self, window):
        self.check()
        spec, count = Spec(window,2), C.c_long()
        ids = C.POINTER(Identity)()
        if self.res.XResQueryClientIds(self.display,1,C.byref(spec),C.byref(count),C.byref(ids)) != 0:
            raise Unavailable('query-failed')
        try:
            if count.value != 1 or ids[0].spec.mask != 2 or not ids[0].spec.client:
                raise Unavailable('query-failed')
            pid = self.res.XResGetClientPid(ids)
            if pid <= 1:
                raise Unavailable('query-failed')
            return pid
        finally:
            self.res.XResClientIdsDestroy(count,ids)

    def mapped(self, window):
        self.check()
        attributes = Attributes()
        if not self.x.XGetWindowAttributes(self.display,window,C.byref(attributes)) or attributes.map_state not in (0,1,2):
            raise Unavailable('query-failed')
        return int(attributes.map_state != 0)

    def close(self):
        if self.display:
            try:
                # Closing may flush race errors: keep the payload-free handler
                # installed until the connection has finished closing.
                self.x.XCloseDisplay(self.display)
            finally:
                if hasattr(self, 'error_handler'):
                    self.x.XSetErrorHandler(self.old_error_handler)
                self.display = None


def capture(client, pid, guard, deadline):
    backend = None
    try:
        if time.monotonic() >= deadline:
            return result('deadline')
        backend = NativeTree.__new__(NativeTree)
        backend.__init__(deadline)
        return observe(client,pid,backend,guard,deadline)
    except (OSError, AttributeError):
        return result('unavailable')
    except Unavailable as error:
        return result(error.state)
    finally:
        if backend is not None and hasattr(backend, 'display'):
            backend.close()
