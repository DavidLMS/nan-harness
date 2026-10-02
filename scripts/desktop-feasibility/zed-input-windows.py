#!/usr/bin/env python3
"""Fixed Win32 key transport for foreground-guarded disposable probes."""
import ctypes
from ctypes import wintypes
import os
import sys

KEYS = {'trust': ([0x11, 0x12], 0x54), 'new-thread': ([0x11, 0x12], 0x4e), 'copy-thread': ([0x11, 0x12], 0x59),
        'select-all': ([0x11], 0x41), 'copy': ([0x11], 0x43),
        'paste': ([0x11], 0x56), 'right': ([], 0x27), 'submit': ([], 0x0d)}


def events(mode):
    modifiers, key = KEYS[mode]
    return [(code, False) for code in modifiers] + [(key, False), (key, True)] + [
        (code, True) for code in reversed(modifiers)]


def send(batch):
    # INPUT's union includes MOUSEINPUT even for keyboard-only batches, so its
    # native size/alignment stays correct on Windows x64.
    class Mouse(ctypes.Structure):
        _fields_ = [('dx', wintypes.LONG), ('dy', wintypes.LONG),
                    ('mouseData', wintypes.DWORD), ('dwFlags', wintypes.DWORD),
                    ('time', wintypes.DWORD), ('dwExtraInfo', ctypes.c_size_t)]

    class Keyboard(ctypes.Structure):
        _fields_ = [('wVk', wintypes.WORD), ('wScan', wintypes.WORD),
                    ('dwFlags', wintypes.DWORD), ('time', wintypes.DWORD),
                    ('dwExtraInfo', ctypes.c_size_t)]

    class Payload(ctypes.Union):
        _fields_ = [('mi', Mouse), ('ki', Keyboard)]

    class Input(ctypes.Structure):
        _fields_ = [('type', wintypes.DWORD), ('payload', Payload)]

    if ctypes.sizeof(Input) != 40 or ctypes.sizeof(ctypes.c_size_t) != 8:
        return 3
    user = ctypes.WinDLL('user32', use_last_error=True)
    user.GetAsyncKeyState.argtypes = [ctypes.c_int]
    user.GetAsyncKeyState.restype = ctypes.c_short
    if any(user.GetAsyncKeyState(key) & 0x8000 for key in [0x10, 0x11, 0x12, 0x5b, 0x5c]):
        return 3
    user.SendInput.argtypes = [wintypes.UINT, ctypes.POINTER(Input), ctypes.c_int]
    user.SendInput.restype = wintypes.UINT
    inputs = (Input * len(batch))()
    for index, (key, released) in enumerate(batch):
        inputs[index].type = 1
        inputs[index].payload.ki = Keyboard(key, 0, (2 if released else 0) | (1 if key == 0x27 else 0), 0, 0)
    # An incomplete receipt is terminal; never repeat a potentially sent action.
    return 0 if user.SendInput(len(inputs), inputs, ctypes.sizeof(Input)) == len(inputs) else 3


def main():
    if len(sys.argv) != 2 or sys.argv[1] not in KEYS or sys.stdin.buffer.read(1):
        return 2
    if os.name != 'nt':
        return 3
    try:
        return send(events(sys.argv[1]))
    except (OSError, ValueError):
        return 3


if __name__ == '__main__':
    raise SystemExit(main())
