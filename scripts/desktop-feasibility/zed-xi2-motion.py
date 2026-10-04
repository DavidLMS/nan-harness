#!/usr/bin/env python3
"""Advisory XI2 payload receipt for an already owned window; never input."""
import ctypes as C
import math
from pathlib import Path
import runpy
import time


def receipt(state, targets, motion=0, axes=0, matched=0, neutral=None, translated=None):
    return dict(state=state, targetPointCount=targets, ownedMotionCount=motion,
                motionWithXYCount=axes, retainedPointMatchedCount=matched,
                noPressedButtons=neutral, eventRootTranslationMatched=translated,
                observerOnly=True, inputAuthorized=False)


def payload_matches(payload, window, root, origin, points):
    if (payload['window'] != window or payload['root'] != root
            or len(payload['local']) != 2 or len(payload['screen']) != 2 or len(origin) != 2
            or type(payload['axes']) is not bytes or type(payload['buttons']) is not bytes
            or len(payload['axes']) > 32 or len(payload['buttons']) > 32
            or type(payload['modifiers']) is not int or payload['modifiers'] < 0
            or any(not math.isfinite(n) for n in (*payload['local'], *payload['screen']))):
        raise ValueError('identity-rejected')
    axes = bool(payload['axes'] and payload['axes'][0] & 3)
    neutral = not any(payload['buttons']) and payload['modifiers'] == 0
    tolerance = 1 / 65536
    translated = all(abs(screen - local - offset) <= tolerance
                     for screen, local, offset in zip(payload['screen'], payload['local'], origin))
    matched = {point for point in points
               if all(abs(observed - expected) <= tolerance
                      for observed, expected in zip(payload['screen'], point))}
    return axes, neutral, translated, matched if axes and neutral and translated else set()


class Observer:
    def __init__(self, window, pid, root, origin, points, guard, deadline):
        self.native = None
        self.window, self.pid, self.root, self.origin = window, pid, root, origin
        self.points, self.guard, self.deadline = set(points), guard, deadline
        self.motion = self.axes = 0
        self.matched = set()
        self.neutral = self.translated = True
        self.state = 'unavailable'
        try:
            self.fresh()
            if not 1 <= len(self.points) <= 9 or any(len(p) != 2 for p in self.points):
                raise ValueError('query-failed')
            base = Path(__file__).parent
            # Shared ABI declarations only: run_path never enters the fixture's
            # __main__ worker, which creates a neutral window and injects input.
            self.abi = runpy.run_path(str(base / 'x11-motion-fixture/motion-fixture.py'))
            tree = runpy.run_path(str(base / 'zed-transient-dialogs.py'))
            self.native = tree['NativeTree'].__new__(tree['NativeTree'])
            self.native.__init__(deadline)
            self.fresh()
            if self.native.pid(window) != pid or self.native.root() != root:
                raise ValueError('identity-rejected')
            self.x, self.xi = self.native.x, C.CDLL('libXi.so.6')
            self.x.XQueryExtension.argtypes = [C.c_void_p,C.c_char_p,C.POINTER(C.c_int),C.POINTER(C.c_int),C.POINTER(C.c_int)]
            self.x.XPending.argtypes = [C.c_void_p]
            self.x.XNextEvent.argtypes = [C.c_void_p,C.POINTER(self.abi['Event'])]
            self.x.XGetEventData.argtypes = [C.c_void_p,C.POINTER(self.abi['Cookie'])]
            self.x.XFreeEventData.argtypes = [C.c_void_p,C.POINTER(self.abi['Cookie'])]
            self.x.XSync.argtypes = [C.c_void_p,C.c_int]
            self.xi.XIQueryVersion.argtypes = [C.c_void_p,C.POINTER(C.c_int),C.POINTER(C.c_int)]
            self.xi.XISelectEvents.argtypes = [C.c_void_p,C.c_ulong,C.POINTER(self.abi['Mask']),C.c_int]
            self.opcode, event, error = C.c_int(), C.c_int(), C.c_int()
            if not self.x.XQueryExtension(self.native.display,b'XInputExtension',C.byref(self.opcode),C.byref(event),C.byref(error)):
                raise ValueError('unavailable')
            major, minor = C.c_int(2), C.c_int(0)
            if self.xi.XIQueryVersion(self.native.display,C.byref(major),C.byref(minor)) != 0:
                raise ValueError('unavailable')
            bits = (C.c_ubyte * 1)(1 << 6)
            mask = self.abi['Mask'](1,1,bits)
            # This connection's Motion mask only; no application mask change,
            # window creation, pointer motion, focus, grabs or button input.
            self.fresh()
            if self.xi.XISelectEvents(self.native.display,window,C.byref(mask),1) != 0:
                raise ValueError('query-failed')
            self.x.XSync(self.native.display,False)
            self.native.check()
            self.fresh()
            self.state = 'complete'
        except Exception as error:
            self.reject(error)
            self.close()

    def fresh(self):
        if time.monotonic() >= self.deadline:
            raise ValueError('deadline')
        if not self.guard():
            raise ValueError('identity-rejected')
        if time.monotonic() >= self.deadline:
            raise ValueError('deadline')

    def reject(self, error):
        reason = getattr(error,'state',str(error))
        self.state = reason if reason in {'deadline','identity-rejected','unavailable','query-failed','limit'} else 'query-failed'

    def poll(self):
        if self.state != 'complete' or self.native is None:
            return
        try:
            self.fresh()
            if self.native.pid(self.window) != self.pid or self.native.root() != self.root:
                raise ValueError('identity-rejected')
            handled = 0
            while self.x.XPending(self.native.display):
                self.fresh()
                if handled >= 128 or self.motion >= 128:
                    raise ValueError('limit')
                handled += 1
                event = self.abi['Event']()
                self.x.XNextEvent(self.native.display,C.byref(event))
                if event.type != 35 or event.cookie.extension != self.opcode.value:
                    raise ValueError('query-failed')
                if event.cookie.evtype != 6 or not self.x.XGetEventData(self.native.display,C.byref(event.cookie)):
                    raise ValueError('query-failed')
                try:
                    if not event.cookie.data:
                        raise ValueError('query-failed')
                    value = C.cast(event.cookie.data,C.POINTER(self.abi['DeviceEvent'])).contents
                    def mask_bytes(mask):
                        if not 0 <= mask.mask_len <= 32 or mask.mask_len and not mask.mask:
                            raise ValueError('query-failed')
                        return bytes(mask.mask[:mask.mask_len]) if mask.mask_len else b''
                    axes, neutral, translated, matched = payload_matches(dict(window=value.event,root=value.root,
                        local=(value.event_x,value.event_y),screen=(value.root_x,value.root_y),
                        axes=mask_bytes(value.valuators),buttons=mask_bytes(value.buttons),modifiers=value.mods.effective),
                        self.window,self.root,self.origin,self.points)
                    self.fresh()
                    self.native.check()
                    self.motion += 1
                    self.axes += axes
                    self.neutral &= neutral
                    self.translated &= translated
                    self.matched.update(matched)
                finally:
                    self.x.XFreeEventData(self.native.display,C.byref(event.cookie))
            self.fresh()
        except Exception as error:
            self.reject(error)

    def result(self):
        return receipt(self.state,len(self.points),self.motion,self.axes,len(self.matched),
                       self.neutral if self.motion else None,self.translated if self.motion else None)

    def close(self):
        if self.native is not None:
            self.native.close()
            self.native = None
