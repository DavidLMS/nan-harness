#!/usr/bin/env python3
"""Fresh bounded Win32 ownership metadata; only a closed verdict leaves memory."""
import ctypes as c
import socket
import sys

D = c.c_uint32
H = c.c_void_p


class Entry(c.Structure):
    _fields_ = [('size', D), ('usage', D), ('pid', D), ('heap', c.c_size_t),
                ('module', D), ('threads', D), ('parent', D), ('priority', c.c_int32),
                ('flags', D), ('name', c.c_wchar * 260)]


class Tcp4(c.Structure):
    _fields_ = [(key, D) for key in ('state', 'address', 'port', 'remote', 'remote_port', 'pid')]


class Tcp6(c.Structure):
    _fields_ = [('address', c.c_ubyte * 16), ('scope', D), ('port', D),
                ('remote', c.c_ubyte * 16), ('remote_scope', D), ('remote_port', D),
                ('state', D), ('pid', D)]


def ancestry(candidate, owner, parents, identity):
    seen = set()
    for _ in range(32):
        if candidate <= 1 or candidate in seen:
            return 'ancestry-cycle' if candidate in seen else 'ancestry-limit'
        seen.add(candidate)
        current = identity(candidate)
        if candidate not in parents or current is None:
            return 'process-unavailable'
        if candidate == owner:
            return 'true'
        parent = parents[candidate]
        previous = identity(parent)
        if parent not in parents or previous is None:
            return 'parent-unavailable'
        if previous[0] > current[0]:
            return 'parent-reused'
        if previous[1] != current[1]:
            return 'session-mismatch'
        candidate = parent
    return 'ancestry-limit'


class Native:
    def __init__(self):
        self.k = c.WinDLL('kernel32', use_last_error=True)
        self.ip = c.WinDLL('iphlpapi', use_last_error=True)
        signatures = {
            'CreateToolhelp32Snapshot': ([D, D], H),
            'Process32FirstW': ([H, c.POINTER(Entry)], c.c_int),
            'Process32NextW': ([H, c.POINTER(Entry)], c.c_int),
            'CloseHandle': ([H], c.c_int),
            'OpenProcess': ([D, c.c_int, D], H),
            'GetProcessTimes': ([H, H, H, H, H], c.c_int),
            'GetExitCodeProcess': ([H, c.POINTER(D)], c.c_int),
            'ProcessIdToSessionId': ([D, c.POINTER(D)], c.c_int),
            'GetSystemTimeAsFileTime': ([H], None),
        }
        for name, (arguments, result) in signatures.items():
            function = getattr(self.k, name)
            function.argtypes, function.restype = arguments, result
        self.ip.GetExtendedTcpTable.argtypes = [H, c.POINTER(D), c.c_int, D, D, D]
        self.ip.GetExtendedTcpTable.restype = D
        # A PID reused after this proof starts cannot inherit its snapshot identity.
        self.started = c.c_uint64()
        self.k.GetSystemTimeAsFileTime(c.byref(self.started))

    def parents(self):
        handle = self.k.CreateToolhelp32Snapshot(2, 0)
        if handle == H(-1).value:
            raise OSError('snapshot unavailable')
        try:
            entry = Entry()
            entry.size = c.sizeof(entry)
            records = {}
            present = self.k.Process32FirstW(handle, c.byref(entry))
            while present:
                if len(records) >= 4096:
                    return None
                if entry.pid in records:
                    raise OSError('duplicate process identity')
                records[entry.pid] = entry.parent
                present = self.k.Process32NextW(handle, c.byref(entry))
            if c.get_last_error() != 18:
                raise OSError('incomplete snapshot')
            return records
        finally:
            self.k.CloseHandle(handle)

    def identity(self, pid):
        handle = self.k.OpenProcess(0x1000, False, pid)
        if not handle:
            return None
        try:
            created, exited, kernel, user = [c.c_uint64() for _ in range(4)]
            session, code = D(), D()
            if (not self.k.GetProcessTimes(handle, c.byref(created), c.byref(exited),
                                          c.byref(kernel), c.byref(user))
                    or not self.k.GetExitCodeProcess(handle, c.byref(code)) or code.value != 259
                    or created.value > self.started.value
                    or not self.k.ProcessIdToSessionId(pid, c.byref(session))):
                return None
            return created.value, session.value
        finally:
            self.k.CloseHandle(handle)

    def listeners(self, port):
        listeners = []
        for family, row in [(2, Tcp4), (23, Tcp6)]:
            size = D()
            if self.ip.GetExtendedTcpTable(None, c.byref(size), False, family, 3, 0) != 122:
                raise OSError('listener sizing unavailable')
            if not 4 <= size.value <= 4 + 4096 * c.sizeof(row):
                raise OSError('listener budget')
            buffer = c.create_string_buffer(size.value)
            if self.ip.GetExtendedTcpTable(buffer, c.byref(size), False, family, 3, 0):
                raise OSError('listener snapshot unavailable')
            count = D.from_buffer_copy(buffer.raw[:4]).value
            if count > 4096 or 4 + count * c.sizeof(row) > size.value:
                raise OSError('listener bounds')
            for index in range(count):
                record = row.from_buffer_copy(buffer, 4 + index * c.sizeof(row))
                if socket.ntohs(record.port & 65535) == port and record.state == 2:
                    loopback = family == 2 and record.address == int.from_bytes(b'\x7f\x00\x00\x01', 'little')
                    listeners.append((record.pid, loopback))
        return listeners


def prove(mode, value, owner, native):
    listeners = native.listeners(value) if mode == 'endpoint' else None
    if listeners is not None and (len(listeners) != 1 or listeners[0][1] is not True):
        return 'listener-unavailable'
    candidate = listeners[0][0] if listeners is not None else value
    parents = native.parents()
    if parents is None:
        return 'process-budget'
    verdict = ancestry(candidate, owner, parents, native.identity)
    # A listener change during ancestry lookup never authorizes the old owner.
    if verdict == 'true' and listeners is not None and native.listeners(value) != listeners:
        return 'listener-unavailable'
    return verdict


def main(arguments):
    if (sys.platform != 'win32' or len(arguments) != 3
            or arguments[0] not in ('endpoint', 'descendant')
            or not all(value.isascii() and value.isdecimal() for value in arguments[1:])):
        return 'query-failed'
    mode, value, owner = arguments[0], int(arguments[1]), int(arguments[2])
    if not 1 < owner <= 2147483647 or not 1 < value <= (65535 if mode == 'endpoint' else 2147483647):
        return 'query-failed'
    try:
        return prove(mode, value, owner, Native())
    except (OSError, ValueError, OverflowError):
        return 'query-failed'


if __name__ == '__main__':
    sys.stdout.write(main(sys.argv[1:]))
