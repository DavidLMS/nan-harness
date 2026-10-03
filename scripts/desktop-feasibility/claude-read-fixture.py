#!/usr/bin/env python3
"""Private, bounded stdio MCP fixture; never an application qualification oracle."""
import json
import os
from pathlib import Path
import select
import stat
import sys
import time

MAX_LINE = 8192
MAX_FILE = 4096
MAX_OUTPUT = 32768
MAX_REQUESTS = 64
LIFETIME = 240
VERSIONS = {'2025-11-25', '2025-06-18', '2025-03-26', '2024-11-05'}


class InvalidFixture(Exception):
    pass


def identity(info):
    return (info.st_dev, info.st_ino, info.st_uid, info.st_mode,
            info.st_size, info.st_mtime_ns, info.st_ctime_ns)


class Fixture:
    def __init__(self, workspace, filename):
        self.root = Path(workspace)
        self.path = Path(filename)
        if os.name != 'posix' or not self.root.is_absolute():
            raise InvalidFixture()
        if self.root.resolve(strict=True) != self.root or self.path != self.root / 'read-target.txt':
            raise InvalidFixture()
        self.root_identity = self.directory()
        self.file_identity = self.file_stat()
        self.read()

    def directory(self):
        info = self.root.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
            raise InvalidFixture()
        return (info.st_dev, info.st_ino, info.st_uid, info.st_mode)

    def file_stat(self):
        info = self.path.lstat()
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077 or info.st_nlink != 1 or info.st_size > MAX_FILE:
            raise InvalidFixture()
        return identity(info)

    def read(self):
        # A replaced parent or file must not broaden the one-file capability.
        if self.root.resolve(strict=True) != self.root or self.directory() != self.root_identity or self.file_stat() != self.file_identity:
            raise InvalidFixture()
        fd = os.open(self.path, os.O_RDONLY | os.O_NOFOLLOW)
        try:
            if identity(os.fstat(fd)) != self.file_identity:
                raise InvalidFixture()
            data = os.read(fd, MAX_FILE + 1)
            if len(data) > MAX_FILE or identity(os.fstat(fd)) != self.file_identity:
                raise InvalidFixture()
            if self.root.resolve(strict=True) != self.root or self.directory() != self.root_identity or self.file_stat() != self.file_identity:
                raise InvalidFixture()
            return data.decode('utf-8', errors='strict')
        finally:
            os.close(fd)


class Server:
    def __init__(self, fixture):
        self.fixture = fixture
        self.initialized = False
        self.ready = False

    def handle(self, request):
        request_id = request.get('id') if isinstance(request, dict) else None
        valid_id = request_id is None or (type(request_id) is int and abs(request_id) <= 2**53 - 1) or (type(request_id) is str and request_id.isascii() and request_id.isprintable() and len(request_id) <= 64)
        if not valid_id:
            request_id = None
        def error(code, message):
            return {'jsonrpc': '2.0', 'id': request_id, 'error': {'code': code, 'message': message}}
        if not isinstance(request, dict) or not valid_id or request.get('jsonrpc') != '2.0' or not isinstance(request.get('method'), str) or set(request) - {'jsonrpc', 'id', 'method', 'params'}:
            return error(-32600, 'Invalid request')
        method = request['method']
        params = request.get('params', {})
        if 'id' not in request:
            if method == 'notifications/initialized' and self.initialized:
                self.ready = True
            return None
        if not isinstance(params, dict):
            return error(-32602, 'Invalid parameters')
        if method == 'initialize':
            if self.initialized or params.get('protocolVersion') not in VERSIONS:
                return error(-32602, 'Invalid initialization')
            self.initialized = True
            result = {'protocolVersion': params['protocolVersion'], 'capabilities': {'tools': {'listChanged': False}}, 'serverInfo': {'name': 'nanh-read-fixture', 'version': '1.0.0'}}
        elif not self.ready:
            return error(-32000, 'Not initialized')
        elif method == 'ping':
            result = {}
        elif method == 'tools/list':
            result = {'tools': [{'name': 'read_file', 'description': 'Read the single owned test fixture.', 'inputSchema': {'type': 'object', 'properties': {'path': {'type': 'string', 'const': str(self.fixture.path)}}, 'required': ['path'], 'additionalProperties': False}, 'annotations': {'readOnlyHint': True, 'destructiveHint': False, 'idempotentHint': True, 'openWorldHint': False}}]}
        elif method == 'tools/call':
            if set(params) != {'name', 'arguments'} or params.get('name') != 'read_file' or params.get('arguments') != {'path': str(self.fixture.path)}:
                return error(-32602, 'Invalid tool arguments')
            try:
                content = self.fixture.read()
            except (InvalidFixture, OSError, UnicodeError):
                result = {'content': [{'type': 'text', 'text': 'Fixture unavailable'}], 'isError': True}
            else:
                result = {'content': [{'type': 'text', 'text': content}], 'isError': False}
        else:
            return error(-32601, 'Method not found')
        return {'jsonrpc': '2.0', 'id': request_id, 'result': result}


def serve(server, input_fd, output_fd, deadline, max_requests=MAX_REQUESTS):
    os.set_blocking(input_fd, False)
    os.set_blocking(output_fd, False)
    pending = bytearray()
    count = 0
    total_output = 0
    while time.monotonic() < deadline and count < max_requests:
        if not select.select([input_fd], [], [], max(0, deadline - time.monotonic()))[0]:
            return 5
        chunk = os.read(input_fd, MAX_LINE + 1)
        if not chunk:
            return 0 if not pending else 2
        pending.extend(chunk)
        while b'\n' in pending:
            line, _, rest = pending.partition(b'\n')
            pending = bytearray(rest)
            if len(line) > MAX_LINE or count >= max_requests:
                return 2
            count += 1
            try:
                request = json.loads(line.decode('utf-8'))
            except (ValueError, UnicodeError):
                reply = {'jsonrpc': '2.0', 'id': None, 'error': {'code': -32700, 'message': 'Parse error'}}
            else:
                reply = server.handle(request)
            if reply is None:
                continue
            output = (json.dumps(reply, ensure_ascii=True, separators=(',', ':')) + '\n').encode()
            total_output += len(output)
            if len(output) > MAX_OUTPUT or total_output > MAX_OUTPUT * 8:
                return 2
            while output:
                if time.monotonic() >= deadline or not select.select([], [output_fd], [], max(0, deadline - time.monotonic()))[1]:
                    return 5
                output = output[os.write(output_fd, output):]
        if len(pending) > MAX_LINE:
            return 2
    return 5


def main():
    if len(sys.argv) != 5 or sys.argv[1] != '--workspace' or sys.argv[3] != '--file':
        return 2
    try:
        fixture = Fixture(sys.argv[2], sys.argv[4])
        return serve(Server(fixture), 0, 1, time.monotonic() + LIFETIME)
    except (InvalidFixture, OSError, UnicodeError, ValueError):
        return 2


if __name__ == '__main__':
    sys.exit(main())
