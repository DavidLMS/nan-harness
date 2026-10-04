"""XRecord delivered XI2 headers; no event selection or input injection.
Run any NativeRecorder use inside a separately supervised <=3s process.
The caller must supply fresh owned PID/client + unchanged geometry/state proof.
"""
import ctypes as C
import select
import struct
import sys
import time

class Unavailable(Exception):
    def __init__(self, stage):
        self.stage = stage
        super().__init__(stage)

class Counts:

    def __init__(self, window, opcode, base):
        self.window, self.opcode, self.base = (window, opcode, base)
        self.press = self.release = self.bytes = 0
        self.device = self.pressed_at = None
        self.ordered = False
        self.invalid = False
        self.event_order = []
        self.crossings = dict(ownedNormalEnterCount=0, ownedNonNormalEnterCount=0,
                              ownedNormalLeaveCount=0, ownedMotionCount=0)

    def accept(self, category, swapped, base, data):
        if category != 0:
            return
        self.bytes += len(data)
        if self.bytes > 4096 or swapped or base != self.base or (len(data) != 32):
            self.invalid = True
            return
        order = '<' if sys.byteorder == 'little' else '>'
        kind, opcode, _, length, event, device, stamp, detail, _, window, _ = struct.unpack(order + 'BBHIHHIIIII', data)
        if kind == 35 and opcode == self.opcode and event in (6, 7, 8):
            # XRecord supplies only the real first32 bytes; never infer coordinates,
            # axes, masks or whether GPUI consumed this delivered header.
            if window != self.window:
                return
            if event == 6:
                if length < 12 or length > 1024 or device <= 1:
                    self.invalid = True
                    return
                key = 'ownedMotionCount'
            else:
                _, _, _, _, _, _, _, source, mode, crossing_detail, _, _, _ = struct.unpack(
                    order + 'BBHIHHIHBBIII', data)
                if length < 10 or length > 1024 or device <= 1 or source <= 1 or mode > 5 or crossing_detail > 7:
                    self.invalid = True
                    return
                key = ('ownedNormalEnterCount' if mode == 0 else 'ownedNonNormalEnterCount') if event == 7 else (
                    'ownedNormalLeaveCount' if mode == 0 else None)
            if key:
                self.event_order.append({'ownedNormalEnterCount':'enter',
                    'ownedNonNormalEnterCount':'non-normal-enter',
                    'ownedNormalLeaveCount':'leave', 'ownedMotionCount':'motion'}[key])
                self.crossings[key] += 1
                if self.crossings[key] > 64:
                    self.invalid = True
            return
        if kind != 35 or opcode != self.opcode or event not in (4, 5):
            return
        if length < 12 or length > 1024 or device <= 1:
            self.invalid = True
            return
        if detail != 1 or window != self.window:
            return
        self.event_order.append('press' if event == 4 else 'release')
        if event == 4:
            self.press = min(2, self.press + 1)
            if self.device is not None or self.release:
                self.invalid = True
            self.device, self.pressed_at = (device, stamp)
        else:
            self.release = min(2, self.release + 1)
            self.ordered = self.press == self.release == 1 and device == self.device and (0 <= stamp - self.pressed_at & 4294967295 <= 2000) if self.pressed_at is not None else False
            if not self.ordered:
                self.invalid = True

    def closed(self):
        return {'pressCount': self.press, 'releaseCount': self.release, 'orderedPair': self.ordered and (not self.invalid),
                'crossingHeaders': {'status':'unavailable' if self.invalid else 'observed',
                    'eventOrder':None if self.invalid else list(self.event_order),
                    **{key:None if self.invalid else value for key,value in self.crossings.items()}}}

class R8(C.Structure):
    _fields_ = [('first', C.c_ubyte), ('last', C.c_ubyte)]

class R16(C.Structure):
    _fields_ = [('first', C.c_ushort), ('last', C.c_ushort)]

class Ext(C.Structure):
    _fields_ = [('major', R8), ('minor', R16)]

class Range(C.Structure):
    _fields_ = [('requests', R8), ('replies', R8), ('ext_requests', Ext), ('ext_replies', Ext), ('delivered', R8), ('device', R8), ('errors', R8), ('started', C.c_int), ('died', C.c_int)]

class Intercept(C.Structure):
    _fields_ = [('base', C.c_ulong), ('time', C.c_ulong), ('sequence', C.c_ulong), ('category', C.c_int), ('swapped', C.c_int), ('data', C.POINTER(C.c_ubyte)), ('length', C.c_ulong)]

class Spec(C.Structure):
    _fields_ = [('client', C.c_ulong), ('mask', C.c_uint)]

class Identity(C.Structure):
    _fields_ = [('spec', Spec), ('length', C.c_long), ('value', C.c_void_p)]
CALLBACK = C.CFUNCTYPE(None, C.c_void_p, C.POINTER(Intercept))

class NativeRecorder:
    """One owned client connection only; synthetic tests never instantiate this."""

    def __init__(self, pid, window, cutoff=None):
        self.control = self.data = None
        self.frozen = False
        self.cutoff = float('inf') if cutoff is None else cutoff
        self.context = 0
        self.pid, self.window = (pid, window)
        self.stage = 'library'
        try:
            self.x = C.CDLL('libX11.so.6')
            self.record = C.CDLL('libXtst.so.6')
            self.res = C.CDLL('libXRes.so.1')
            self._bind()
            self.stage = 'display'
            self.control = self.x.XOpenDisplay(None)
            self.data = self.x.XOpenDisplay(None)
            if not self.control or not self.data:
                raise Unavailable(self.stage)
            a, b = (C.c_int(), C.c_int())
            self.stage = 'record-version'
            if not self.record.XRecordQueryVersion(self.control, C.byref(a), C.byref(b)) or (a.value, b.value) < (1, 13):
                raise Unavailable(self.stage)
            self.stage = 'xres-version'
            if not self.res.XResQueryVersion(self.control, C.byref(a), C.byref(b)) or (a.value, b.value) < (1, 2):
                raise Unavailable(self.stage)
            opcode, ev, err = (C.c_int(), C.c_int(), C.c_int())
            self.stage = 'xinput-extension'
            if not self.x.XQueryExtension(self.control, b'XInputExtension', C.byref(opcode), C.byref(ev), C.byref(err)):
                raise Unavailable(self.stage)
            self.opcode = opcode.value
            if not 128 <= self.opcode <= 255:
                raise Unavailable(self.stage)
            base = self._identity()
            self.counts = Counts(window, self.opcode, base)
            ranges = Range()
            ranges.delivered = R8(35, 35)
            pointer = C.pointer(ranges)
            clients = (C.c_ulong * 1)(window)
            self.stage = 'context'
            self.context = self.record.XRecordCreateContext(self.control, 0, clients, 1, C.byref(pointer), 1)
            if not self.context:
                raise Unavailable(self.stage)
            # The data connection cannot enable a context until the control
            # connection's creation request has reached the server.
            self.x.XSync(self.control, 0)
            self.callback = CALLBACK(self._event)
            self.stage = 'enable'
            if not self.record.XRecordEnableContextAsync(self.data, self.context, self.callback, None):
                raise Unavailable(self.stage)
            self.stage = 'identity-recheck'
            if self._identity() != base:
                raise Unavailable(self.stage)
        except (OSError, Unavailable):
            failed_stage = self.stage
            self.close()
            raise Unavailable(failed_stage) from None

    def _bind(self):

        def bind(lib, name, args, result):
            fn = getattr(lib, name)
            fn.argtypes = args
            fn.restype = result
        p = C.c_void_p
        i = C.c_int
        u = C.c_ulong
        bind(self.x, 'XOpenDisplay', [C.c_char_p], p)
        bind(self.x, 'XCloseDisplay', [p], i)
        bind(self.x, 'XConnectionNumber', [p], i)
        bind(self.x, 'XSync', [p, i], i)
        bind(self.x, 'XQueryExtension', [p, C.c_char_p, C.POINTER(i), C.POINTER(i), C.POINTER(i)], i)
        bind(self.res, 'XResQueryVersion', [p, C.POINTER(i), C.POINTER(i)], i)
        bind(self.res, 'XResQueryClientIds', [p, C.c_long, C.POINTER(Spec), C.POINTER(C.c_long), C.POINTER(C.POINTER(Identity))], i)
        bind(self.res, 'XResGetClientPid', [C.POINTER(Identity)], i)
        bind(self.res, 'XResClientIdsDestroy', [C.c_long, C.POINTER(Identity)], None)
        bind(self.record, 'XRecordQueryVersion', [p, C.POINTER(i), C.POINTER(i)], i)
        bind(self.record, 'XRecordCreateContext', [p, i, C.POINTER(u), i, C.POINTER(C.POINTER(Range)), i], u)
        bind(self.record, 'XRecordEnableContextAsync', [p, u, CALLBACK, p], i)
        bind(self.record, 'XRecordProcessReplies', [p], None)
        bind(self.record, 'XRecordDisableContext', [p, u], i)
        bind(self.record, 'XRecordFreeContext', [p, u], i)
        bind(self.record, 'XRecordFreeData', [C.POINTER(Intercept)], None)

    def _identity(self):
        recheck = self.stage in {'identity-recheck', 'observation'}
        if not recheck:
            self.stage = 'client-query'
        spec = Spec(self.window, 2)
        n = C.c_long()
        values = C.POINTER(Identity)()
        if self.res.XResQueryClientIds(self.control, 1, C.byref(spec), C.byref(n), C.byref(values)) != 0:
            raise Unavailable(self.stage)
        try:
            if not recheck:
                self.stage = 'client-identity'
            if n.value != 1 or values[0].spec.mask != 2 or self.res.XResGetClientPid(values) != self.pid or (not values[0].spec.client):
                raise Unavailable(self.stage)
            return values[0].spec.client
        finally:
            self.res.XResClientIdsDestroy(n, values)

    def _event(self, _, pointer):
        try:
            if self.frozen or time.monotonic() >= self.cutoff:
                return
            event = pointer.contents
            if event.category != 0:
                return
            if event.length != 8:
                self.counts.invalid = True
                return
            data = C.string_at(event.data, 32)
            if not self.frozen and time.monotonic() < self.cutoff:
                self.counts.accept(event.category, bool(event.swapped), event.base, data)
        finally:
            self.record.XRecordFreeData(pointer)

    def pump(self):
        if self.frozen or time.monotonic() >= self.cutoff:
            return
        ready, _, _ = select.select([self.x.XConnectionNumber(self.data)], [], [], 0)
        if ready and not self.frozen and time.monotonic() < self.cutoff:
            self.record.XRecordProcessReplies(self.data)

    def freeze(self):
        # Stop admission BEFORE snapshot/cleanup. No Xlib read, pump or ownership
        # query occurs here, so cutoff cannot silently start a new capture.
        self.frozen = True
        return self.counts.closed()

    def snapshot(self):
        self.pump()
        result = self.freeze()
        self.stage = 'observation'
        if self._identity() != self.counts.base:
            raise Unavailable(self.stage)
        return result

    def observe(self, seconds=0.5):
        if not 0 < seconds <= 1:
            raise Unavailable(self.stage)
        self.stage = 'observation'
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            ready, _, _ = select.select([self.x.XConnectionNumber(self.data)], [], [], max(0, end - time.monotonic()))
            if ready:
                self.record.XRecordProcessReplies(self.data)
        if self._identity() != self.counts.base:
            raise Unavailable(self.stage)
        return self.counts.closed()

    def close(self):
        if self.control and self.context:
            self.record.XRecordDisableContext(self.control, self.context)
            self.record.XRecordFreeContext(self.control, self.context)
            self.x.XSync(self.control, 0)
        self.context = 0
        for display in [self.data, self.control]:
            if display:
                self.x.XCloseDisplay(display)
        self.data = self.control = None
