#!/usr/bin/env python3
"""Exact owned-file Restart Manager diagnostic; never shuts down an application."""
import ctypes
import json
import os
from pathlib import Path
import sys
import time

MAX_REQUEST = 4096
MAX_OWNERS = 64


def current_milliseconds():
    return time.time_ns() // 1_000_000


def parse_request(line):
    if len(line) > MAX_REQUEST:
        raise ValueError('request')
    def unique_object(pairs):
        result = {}
        for key, item in pairs:
            if key in result:
                raise ValueError('request')
            result[key] = item
        return result
    value = json.loads(line, object_pairs_hook=unique_object)
    if type(value) is not dict or set(value) != {'workspace', 'temporary', 'destination', 'cliPid', 'deadlineMs'}:
        raise ValueError('request')
    for name in ('workspace', 'temporary', 'destination'):
        if type(value[name]) is not str or not value[name] or any(ord(c) < 32 for c in value[name]):
            raise ValueError('request')
    if (type(value['cliPid']) is not int or not 0 < value['cliPid'] <= 0xffffffff
            or type(value['deadlineMs']) is not int or not current_milliseconds() < value['deadlineMs'] <= current_milliseconds() + 45_000):
        raise ValueError('request')
    return value


def owned_files(value):
    workspace = Path(value['workspace'])
    expected = workspace / 'profile' / 'home' / 'AppData' / 'Roaming' / 'Claude'
    temporary, destination = Path(value['temporary']), Path(value['destination'])
    if (not workspace.is_absolute() or not temporary.is_absolute() or not destination.is_absolute()
            or temporary.parent != expected or destination != expected / 'claude_desktop_config.json'
            or not temporary.name.startswith('.nan-') or temporary == destination):
        raise ValueError('scope')
    identities = []
    for path in [workspace, *expected.parents, expected]:
        info = path.lstat()
        if not path.is_dir() or path.is_symlink() or getattr(info, 'st_file_attributes', 0) & 0x400:
            raise ValueError('scope')
        identities.append((str(path), info.st_dev, info.st_ino, getattr(info, 'st_birthtime_ns', None)))
    for path in (temporary, destination):
        try:
            info = path.lstat()
        except FileNotFoundError:
            if path == destination:
                continue
            raise ValueError('scope') from None
        if not path.is_file() or path.is_symlink() or getattr(info, 'st_file_attributes', 0) & 0x400:
            raise ValueError('scope')
        identities.append((str(path), info.st_dev, info.st_ino, getattr(info, 'st_birthtime_ns', None)))
    return temporary, destination, destination.exists(), tuple(identities)


def classify_owners(owners, current, live_identity):
    if len(owners) > MAX_OWNERS or len(set(owners)) != len(owners):
        raise ValueError('identity')
    own = 0
    for identity in owners:
        if (len(identity) != 2 or type(identity[0]) is not int or type(identity[1]) is not int
                or not 0 < identity[0] <= 0xffffffff or identity[1] <= 0
                or live_identity(identity[0]) != identity[1]):
            raise ValueError('identity')
        own += identity == current
    return {'ownerCount': len(owners), 'currentProcessCount': own, 'otherProcessCount': len(owners) - own}


def classify_delete_access(open_file, identity, close):
    # Query-only handles never request delete-on-close or change file state.
    baseline, error = open_file(0)
    if baseline is None:
        return 'missing' if error in (2, 3) else 'query-failed'
    try:
        expected = identity(baseline)
        if expected is None:
            return 'query-failed'
        handle, error = open_file(0x10000)  # DELETE permits rename; does not perform it.
        if handle is None:
            return {32:'sharing-denied',5:'access-denied',2:'missing',3:'missing'}.get(error,'query-failed')
        try:
            return 'available' if identity(handle) == expected else 'query-failed'
        finally:
            close(handle)
    finally:
        close(baseline)


def query(value):
    from ctypes import wintypes as w
    class FT(ctypes.Structure):
        _fields_ = [('low', w.DWORD), ('high', w.DWORD)]
    class Unique(ctypes.Structure):
        _fields_ = [('pid', w.DWORD), ('started', FT)]
    class ProcessInfo(ctypes.Structure):
        _fields_ = [('process', Unique), ('app', w.WCHAR * 256), ('service', w.WCHAR * 64),
                    ('kind', ctypes.c_int), ('status', w.ULONG), ('session', w.DWORD), ('restartable', w.BOOL)]
    k = ctypes.WinDLL('kernel32', use_last_error=True)
    class FileInfo(ctypes.Structure):
        _fields_ = [('attributes',w.DWORD),('created',FT),('accessed',FT),('written',FT),
                    ('volume',w.DWORD),('sizeHigh',w.DWORD),('sizeLow',w.DWORD),
                    ('links',w.DWORD),('indexHigh',w.DWORD),('indexLow',w.DWORD)]
    k.CreateFileW.argtypes = [w.LPCWSTR,w.DWORD,w.DWORD,ctypes.c_void_p,w.DWORD,w.DWORD,w.HANDLE]
    k.CreateFileW.restype = w.HANDLE
    k.GetFileInformationByHandle.argtypes = [w.HANDLE,ctypes.POINTER(FileInfo)]
    k.GetFileInformationByHandle.restype = w.BOOL
    rm = ctypes.WinDLL('rstrtmgr', use_last_error=True)
    k.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]; k.OpenProcess.restype = w.HANDLE
    k.GetProcessTimes.argtypes = [w.HANDLE, ctypes.POINTER(FT), ctypes.POINTER(FT), ctypes.POINTER(FT), ctypes.POINTER(FT)]; k.GetProcessTimes.restype = w.BOOL
    k.WaitForSingleObject.argtypes = [w.HANDLE, w.DWORD]; k.WaitForSingleObject.restype = w.DWORD
    k.GetExitCodeProcess.argtypes = [w.HANDLE, ctypes.POINTER(w.DWORD)]; k.GetExitCodeProcess.restype = w.BOOL
    k.CloseHandle.argtypes = [w.HANDLE]; k.CloseHandle.restype = w.BOOL
    rm.RmStartSession.argtypes = [ctypes.POINTER(w.DWORD), w.DWORD, w.LPWSTR]; rm.RmStartSession.restype = w.DWORD
    rm.RmRegisterResources.argtypes = [w.DWORD, w.UINT, ctypes.POINTER(w.LPCWSTR), w.UINT, ctypes.POINTER(Unique), w.UINT, ctypes.POINTER(w.LPCWSTR)]; rm.RmRegisterResources.restype = w.DWORD
    rm.RmGetList.argtypes = [w.DWORD, ctypes.POINTER(w.UINT), ctypes.POINTER(w.UINT), ctypes.POINTER(ProcessInfo), ctypes.POINTER(w.DWORD)]; rm.RmGetList.restype = w.DWORD
    rm.RmEndSession.argtypes = [w.DWORD]; rm.RmEndSession.restype = w.DWORD
    def live(pid):
        handle = k.OpenProcess(0x101000, False, pid)
        if not handle:
            raise ValueError('identity')
        try:
            created, exited, kernel, user, code = FT(), FT(), FT(), FT(), w.DWORD()
            if (not k.GetProcessTimes(handle, ctypes.byref(created), ctypes.byref(exited), ctypes.byref(kernel), ctypes.byref(user))
                    or k.WaitForSingleObject(handle, 0) != 258):
                raise ValueError('identity')
            return created.low | created.high << 32
        finally:
            k.CloseHandle(handle)
    if os.getppid() != value['cliPid']:
        raise ValueError('identity')
    current = (value['cliPid'], live(value['cliPid']))
    def guarded():
        if current_milliseconds() >= value['deadlineMs']:
            raise ValueError('deadline')
        if live(current[0]) != current[1]:
            raise ValueError('identity')
    temporary, destination, present, identities = owned_files(value)
    def source_open(access):
        handle = k.CreateFileW(str(temporary),access,7,None,3,0x00200000,None)
        return (None,ctypes.get_last_error()) if handle == ctypes.c_void_p(-1).value else (handle,0)
    def source_identity(handle):
        info = FileInfo()
        if not k.GetFileInformationByHandle(handle,ctypes.byref(info)) or info.attributes & (0x400|0x10):
            return None
        return (info.volume,info.indexHigh,info.indexLow,info.created.low,info.created.high)
    session, key = w.DWORD(), ctypes.create_unicode_buffer(33)
    guarded()
    if rm.RmStartSession(ctypes.byref(session), 0, key):
        raise ValueError('session')
    try:
        guarded()
        paths = (w.LPCWSTR * 2)(str(temporary), str(destination))
        if rm.RmRegisterResources(session, 2, paths, 0, None, 0, None):
            raise ValueError('register')
        guarded()
        needed, count, reasons = w.UINT(), w.UINT(), w.DWORD()
        code = rm.RmGetList(session, ctypes.byref(needed), ctypes.byref(count), None, ctypes.byref(reasons))
        guarded()
        if code not in (0, 234) or needed.value > MAX_OWNERS:
            raise ValueError('list')
        entries = (ProcessInfo * MAX_OWNERS)(); count.value = MAX_OWNERS
        if rm.RmGetList(session, ctypes.byref(needed), ctypes.byref(count), entries, ctypes.byref(reasons)) or count.value > MAX_OWNERS:
            raise ValueError('list')
        guarded()
        owners = [(e.process.pid, e.process.started.low | e.process.started.high << 32) for e in entries[:count.value]]
        result = classify_owners(owners, current, live)
        guarded()
        if owned_files(value)[2:] != (present, identities):
            raise ValueError('scope')
        if classify_owners(owners, current, live) != result:
            raise ValueError('identity')
        guarded()
        source_delete = classify_delete_access(source_open,source_identity,k.CloseHandle)
        guarded()
        if owned_files(value)[2:] != (present,identities):
            raise ValueError('scope')
        return dict(status='observed', stage='complete', destinationPresent=present,
                    sourceDeleteAccess=source_delete, **result)
    finally:
        rm.RmEndSession(session)


def main():
    result = dict(status='unavailable', stage='request', destinationPresent=None,
                  ownerCount=None, currentProcessCount=None, otherProcessCount=None)
    try:
        value = parse_request(sys.stdin.buffer.readline(MAX_REQUEST + 1))
        if sys.platform != 'win32':
            raise ValueError('platform')
        result = query(value)
    except (ValueError, OSError, TypeError, AttributeError) as error:
        stage = str(error) if type(error) is ValueError else 'query'
        result['stage'] = stage if stage in {'request','platform','scope','session','register','list','identity','deadline'} else 'query'
        if result['stage'] == 'deadline':
            result['status'] = 'deadline'
    sys.stdout.write(json.dumps(result, separators=(',', ':')) + '\n')


if __name__ == '__main__':
    main()
