#!/usr/bin/env python3
"""Hosted neutral-window XI2 Warp/XTEST discriminator. Never operates Zed."""
import ctypes as c
import json
import os
import subprocess
import sys
import time


def motion_axes(mask):
    if type(mask) is not bytes or not 0 <= len(mask) <= 32:
        raise ValueError('mask')
    return bool(mask and mask[0] & 3)


class Cookie(c.Structure):
    _fields_=[('type',c.c_int),('serial',c.c_ulong),('send_event',c.c_int),('display',c.c_void_p),
              ('extension',c.c_int),('evtype',c.c_int),('cookie',c.c_uint),('data',c.c_void_p)]
class Event(c.Union):
    _fields_=[('type',c.c_int),('cookie',Cookie),('pad',c.c_long*24)]
class Buttons(c.Structure):
    _fields_=[('mask_len',c.c_int),('mask',c.POINTER(c.c_ubyte))]
class Valuators(c.Structure):
    _fields_=[('mask_len',c.c_int),('mask',c.POINTER(c.c_ubyte)),('values',c.POINTER(c.c_double))]
class Modifiers(c.Structure):
    _fields_=[('base',c.c_int),('latched',c.c_int),('locked',c.c_int),('effective',c.c_int)]
class DeviceEvent(c.Structure):
    _fields_=[('type',c.c_int),('serial',c.c_ulong),('send_event',c.c_int),('display',c.c_void_p),
              ('extension',c.c_int),('evtype',c.c_int),('time',c.c_ulong),('deviceid',c.c_int),
              ('sourceid',c.c_int),('detail',c.c_int),('root',c.c_ulong),('event',c.c_ulong),('child',c.c_ulong),
              ('root_x',c.c_double),('root_y',c.c_double),('event_x',c.c_double),('event_y',c.c_double),
              ('flags',c.c_int),('buttons',Buttons),('valuators',Valuators),('mods',Modifiers),('group',Modifiers)]
class EnterEvent(c.Structure):
    _fields_=[('type',c.c_int),('serial',c.c_ulong),('send_event',c.c_int),('display',c.c_void_p),
              ('extension',c.c_int),('evtype',c.c_int),('time',c.c_ulong),('deviceid',c.c_int),('sourceid',c.c_int),
              ('detail',c.c_int),('root',c.c_ulong),('event',c.c_ulong),('child',c.c_ulong),
              ('root_x',c.c_double),('root_y',c.c_double),('event_x',c.c_double),('event_y',c.c_double),
              ('mode',c.c_int),('focus',c.c_int),('same_screen',c.c_int),('buttons',Buttons),('mods',Modifiers),('group',Modifiers)]
class Mask(c.Structure):
    _fields_=[('deviceid',c.c_int),('mask_len',c.c_int),('mask',c.POINTER(c.c_ubyte))]


def empty_counts():
    return {'motionCount':0,'motionAxesCount':0,'motionWithoutAxesCount':0,'normalEnterCount':0,'pointerSampleCount':0,'pointerOwnedCount':0}


def worker():
    x=c.CDLL('libX11.so.6');xi=c.CDLL('libXi.so.6');xt=c.CDLL('libXtst.so.6')
    x.XOpenDisplay.argtypes=[c.c_char_p];x.XOpenDisplay.restype=c.c_void_p
    x.XDefaultRootWindow.argtypes=[c.c_void_p];x.XDefaultRootWindow.restype=c.c_ulong
    x.XCreateSimpleWindow.argtypes=[c.c_void_p,c.c_ulong,c.c_int,c.c_int,c.c_uint,c.c_uint,c.c_uint,c.c_ulong,c.c_ulong];x.XCreateSimpleWindow.restype=c.c_ulong
    x.XMapWindow.argtypes=[c.c_void_p,c.c_ulong];x.XDestroyWindow.argtypes=[c.c_void_p,c.c_ulong]
    x.XCloseDisplay.argtypes=[c.c_void_p];x.XSync.argtypes=[c.c_void_p,c.c_int];x.XFlush.argtypes=[c.c_void_p]
    x.XPending.argtypes=[c.c_void_p];x.XNextEvent.argtypes=[c.c_void_p,c.POINTER(Event)]
    x.XGetEventData.argtypes=[c.c_void_p,c.POINTER(Cookie)];x.XFreeEventData.argtypes=[c.c_void_p,c.POINTER(Cookie)]
    x.XQueryExtension.argtypes=[c.c_void_p,c.c_char_p,c.POINTER(c.c_int),c.POINTER(c.c_int),c.POINTER(c.c_int)]
    x.XQueryPointer.argtypes=[c.c_void_p,c.c_ulong,c.POINTER(c.c_ulong),c.POINTER(c.c_ulong),c.POINTER(c.c_int),c.POINTER(c.c_int),c.POINTER(c.c_int),c.POINTER(c.c_int),c.POINTER(c.c_uint)]
    x.XWarpPointer.argtypes=[c.c_void_p,c.c_ulong,c.c_ulong,c.c_int,c.c_int,c.c_uint,c.c_uint,c.c_int,c.c_int]
    xi.XIQueryVersion.argtypes=[c.c_void_p,c.POINTER(c.c_int),c.POINTER(c.c_int)]
    xi.XISelectEvents.argtypes=[c.c_void_p,c.c_ulong,c.POINTER(Mask),c.c_int]
    xt.XTestFakeMotionEvent.argtypes=[c.c_void_p,c.c_int,c.c_int,c.c_int,c.c_ulong]
    callback_type=c.CFUNCTYPE(c.c_int,c.c_void_p,c.c_void_p);errors=[False]
    callback=callback_type(lambda _display,_error:(errors.__setitem__(0,True),0)[1])
    x.XSetErrorHandler.argtypes=[c.c_void_p];x.XSetErrorHandler.restype=c.c_void_p
    old=x.XSetErrorHandler(c.cast(callback,c.c_void_p));display=None;window=0
    cutoff=time.monotonic()+4
    try:
        display=x.XOpenDisplay(None)
        if not display:raise ValueError('display')
        opcode=c.c_int();event=c.c_int();error=c.c_int()
        if not x.XQueryExtension(display,b'XInputExtension',c.byref(opcode),c.byref(event),c.byref(error)):raise ValueError('xi2')
        major=c.c_int(2);minor=c.c_int(0)
        if xi.XIQueryVersion(display,c.byref(major),c.byref(minor))!=0:raise ValueError('xi2')
        root=x.XDefaultRootWindow(display)
        window=x.XCreateSimpleWindow(display,root,50,50,320,240,0,0,0)
        bits=(c.c_ubyte*2)(0,0)
        for code in (6,7):bits[code//8]|=1<<(code%8)
        mask=Mask(1,2,bits) # XIAllMasterDevices, Motion and Enter only.
        xi.XISelectEvents(display,window,c.byref(mask),1);x.XMapWindow(display,window);x.XSync(display,False)
        def read(counts):
            handled=0
            while x.XPending(display):
                if time.monotonic()>=cutoff or handled>=64:raise ValueError('deadline')
                handled+=1;ev=Event();x.XNextEvent(display,c.byref(ev))
                if ev.type!=35 or ev.cookie.extension!=opcode.value:continue
                if not x.XGetEventData(display,c.byref(ev.cookie)):raise ValueError('event')
                try:
                    if ev.cookie.evtype==6:
                        data=c.cast(ev.cookie.data,c.POINTER(DeviceEvent)).contents
                        if data.event!=window:raise ValueError('owner')
                        if not 0<=data.valuators.mask_len<=32:raise ValueError('mask')
                        if data.valuators.mask_len and not data.valuators.mask:raise ValueError('mask')
                        axes=motion_axes(bytes(data.valuators.mask[:data.valuators.mask_len]) if data.valuators.mask_len else b'')
                        counts['motionCount']+=1;counts['motionAxesCount' if axes else 'motionWithoutAxesCount']+=1
                    elif ev.cookie.evtype==7:
                        data=c.cast(ev.cookie.data,c.POINTER(EnterEvent)).contents
                        if data.event!=window:raise ValueError('owner')
                        if data.mode==0:counts['normalEnterCount']+=1
                    if any(n>32 for n in counts.values()):raise ValueError('limit')
                finally:x.XFreeEventData(display,c.byref(ev.cookie))
        # Keep all test points strictly inside this neutral owned window. No clicks.
        methods={}
        for kind in ('warp','xtest'):
            counts=empty_counts()
            for point in ((100,100),(160,140),(220,180)):
                if time.monotonic()>=cutoff:raise ValueError('deadline')
                if kind=='warp':x.XWarpPointer(display,0,root,0,0,0,0,*point)
                elif not xt.XTestFakeMotionEvent(display,-1,*point,0):raise ValueError('xtest')
                x.XSync(display,False);time.sleep(min(.05,max(0,cutoff-time.monotonic())));read(counts)
                if time.monotonic()>=cutoff:raise ValueError('deadline')
                found_root=c.c_ulong();child=c.c_ulong();rx=c.c_int();ry=c.c_int();wx=c.c_int();wy=c.c_int();buttons=c.c_uint()
                counts['pointerSampleCount']+=1
                if (not x.XQueryPointer(display,window,c.byref(found_root),c.byref(child),c.byref(rx),c.byref(ry),c.byref(wx),c.byref(wy),c.byref(buttons))
                        or found_root.value!=root or child.value!=0 or (rx.value,ry.value)!=point
                        or not 0<=wx.value<320 or not 0<=wy.value<240):raise ValueError('pointer')
                counts['pointerOwnedCount']+=1
                if errors[0]:raise ValueError('xerror')
            methods[kind]=counts
        return {'schemaVersion':1,'status':'complete',**methods}
    finally:
        # Retain payload-free error handling while close flushes pending requests.
        try:
            if display:
                if window:x.XDestroyWindow(display,window)
                x.XCloseDisplay(display)
        finally:x.XSetErrorHandler(old)


def main():
    unavailable={'schemaVersion':1,'status':'unavailable','warp':None,'xtest':None}
    if (os.environ.get('GITHUB_ACTIONS')!='true' or os.environ.get('RUNNER_OS')!='Linux'
            or os.environ.get('RUNNER_ENVIRONMENT')!='github-hosted'):
        print(json.dumps(unavailable));return
    if '--worker' in sys.argv:
        try:result=worker()
        except (ValueError,OSError):result=unavailable
    else:
        try:
            p=subprocess.run([sys.executable,__file__,'--worker'],capture_output=True,text=True,timeout=6)
            result=json.loads(p.stdout) if p.returncode==0 and not p.stderr and len(p.stdout)<=1024 else unavailable
        except (ValueError,subprocess.SubprocessError):result=unavailable
    print(json.dumps(result,separators=(',',':')))

if __name__=='__main__':main()
