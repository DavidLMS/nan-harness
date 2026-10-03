#!/usr/bin/env python3
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

SCRIPT = Path(__file__).with_name('claude-read-fixture.py')
spec = importlib.util.spec_from_file_location('fixture', SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class FixtureTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name).resolve()
        self.root.chmod(0o700)
        self.path = self.root / 'read-target.txt'
        self.path.write_text('owned fixture evidence\n', encoding='utf-8')
        self.path.chmod(0o600)
        self.fixture = module.Fixture(str(self.root), str(self.path))
        self.server = module.Server(self.fixture)
        self.request('initialize', {'protocolVersion': '2025-11-25'})
        self.server.handle({'jsonrpc': '2.0', 'method': 'notifications/initialized'})

    def tearDown(self):
        self.temp.cleanup()

    def request(self, method, params=None):
        return self.server.handle({'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params or {}})

    def read(self, path=None):
        return self.request('tools/call', {'name': 'read_file', 'arguments': {'path': str(path or self.path)}})

    def test_list_and_actual_content(self):
        tools = self.request('tools/list')['result']['tools']
        self.assertEqual([tool['name'] for tool in tools], ['read_file'])
        self.assertEqual(tools[0]['inputSchema']['properties']['path']['const'], str(self.path))
        self.assertEqual(self.read()['result'], {'content': [{'type': 'text', 'text': 'owned fixture evidence\n'}], 'isError': False})

    def test_wrong_path_and_unknown_method(self):
        self.assertEqual(self.read(self.root / 'other')['error']['code'], -32602)
        self.assertEqual(self.request('secret')['error']['code'], -32601)

    def test_modified_file_not_read(self):
        self.path.write_text('PRIVATE SENTINEL')
        reply = self.read()
        self.assertTrue(reply['result']['isError'])
        self.assertNotIn('PRIVATE SENTINEL', json.dumps(reply))

    def test_replaced_inode_and_symlink_not_read(self):
        self.path.unlink()
        other = self.root / 'other'
        other.write_text('PRIVATE SENTINEL')
        self.path.symlink_to(other)
        self.assertTrue(self.read()['result']['isError'])
        with self.assertRaises(module.InvalidFixture):
            module.Fixture(str(self.root), str(self.path))

    def test_private_parent_and_file_required(self):
        self.root.chmod(0o755)
        self.assertTrue(self.read()['result']['isError'])
        self.root.chmod(0o700)
        self.path.chmod(0o644)
        with self.assertRaises(module.InvalidFixture):
            module.Fixture(str(self.root), str(self.path))

    def test_replaced_private_root_not_read(self):
        displaced = self.root.with_name(self.root.name + '-old')
        self.root.rename(displaced)
        self.root.mkdir(mode=0o700)
        try:
            self.path.write_text('FOREIGN SENTINEL')
            self.path.chmod(0o600)
            self.assertTrue(self.read()['result']['isError'])
        finally:
            self.path.unlink()
            self.root.rmdir()
            displaced.rename(self.root)

    def test_hardlink_not_admitted(self):
        os.link(self.path, self.root / 'alias')
        with self.assertRaises(module.InvalidFixture):
            module.Fixture(str(self.root), str(self.path))

    def test_output_budget_is_enforced(self):
        class LargeReply:
            def handle(self, request):
                return {'payload': 'x' * (module.MAX_OUTPUT + 1)}
        read_fd, write_fd = os.pipe()
        out_read, out_write = os.pipe()
        try:
            os.write(write_fd, b'{}\n')
            self.assertEqual(module.serve(LargeReply(), read_fd, out_write, time.monotonic() + 1), 2)
            os.set_blocking(out_read, False)
            with self.assertRaises(BlockingIOError):
                os.read(out_read, 1)
        finally:
            for fd in (read_fd, write_fd, out_read, out_write):
                os.close(fd)

    def test_initialized_and_ids_strict(self):
        server = module.Server(self.fixture)
        self.assertEqual(server.handle({'jsonrpc': '2.0', 'id': 1, 'method': 'tools/list'})['error']['code'], -32000)
        reply = self.server.handle({'jsonrpc': '2.0', 'id': 'PRIVATE\nID', 'method': 'ping'})
        self.assertIsNone(reply['id'])
        self.assertNotIn('PRIVATE', json.dumps(reply))

    def launch(self, payload):
        return subprocess.run([sys.executable, str(SCRIPT), '--workspace', str(self.root), '--file', str(self.path)], input=payload, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=3)

    def test_real_stdio_protocol(self):
        messages = [{'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {'protocolVersion': '2025-11-25'}}, {'jsonrpc': '2.0', 'method': 'notifications/initialized'}, {'jsonrpc': '2.0', 'id': 2, 'method': 'tools/list'}, {'jsonrpc': '2.0', 'id': 3, 'method': 'tools/call', 'params': {'name': 'read_file', 'arguments': {'path': str(self.path)}}}]
        result = self.launch(('\n'.join(json.dumps(item) for item in messages) + '\n').encode())
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stderr, b'')
        replies = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(len(replies), 3)
        self.assertEqual(replies[-1]['result']['content'][0]['text'], 'owned fixture evidence\n')

    def test_oversize_input_and_request_budget(self):
        self.assertEqual(self.launch(b'x' * (module.MAX_LINE + 1)).returncode, 2)
        payload = (json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': 'ping'}) + '\n').encode() * (module.MAX_REQUESTS + 1)
        self.assertNotEqual(self.launch(payload).returncode, 0)

    def test_absolute_deadline_no_wait(self):
        read_fd, write_fd = os.pipe()
        out_read, out_write = os.pipe()
        try:
            self.assertEqual(module.serve(self.server, read_fd, out_write, time.monotonic() - 1), 5)
        finally:
            for fd in (read_fd, write_fd, out_read, out_write):
                os.close(fd)

    def test_invalid_utf8_and_size(self):
        self.path.write_bytes(b'\xff')
        with self.assertRaises(UnicodeError):
            module.Fixture(str(self.root), str(self.path))
        self.path.write_bytes(b'x' * (module.MAX_FILE + 1))
        with self.assertRaises(module.InvalidFixture):
            module.Fixture(str(self.root), str(self.path))


if __name__ == '__main__':
    unittest.main()
