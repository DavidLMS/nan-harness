"""Exercise native TUI exit choreography without weakening process-exit proof."""

import errno
import importlib.util
import itertools
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'canary/research/zcode'))
spec = importlib.util.spec_from_file_location('zcode_runtime', ROOT / 'canary/research/zcode/runtime_probe.py')
runtime = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runtime)


class TuiExitContracts(unittest.TestCase):
    def drive(self, acknowledge=True, exits=True):
        with tempfile.TemporaryDirectory() as directory:
            owner = runtime.Runtime.__new__(runtime.Runtime)
            owner.node, owner.cli = 'synthetic-node', Path('synthetic-cli')
            owner.workspace, owner.env = Path(directory), {}
            owner.personal = owner.workspace / 'provider.json'
            owner.personal.write_text(json.dumps({'config': {'defaultModelSelection': {'modelId': 'synthetic-model'}}}))
            owner.scenario = SimpleNamespace(requests=[])

            class Terminal:
                def __init__(self, *_args):
                    self.pending = [b'synthetic-model Type a prompt']
                    self.running, self.closed, self.waited = True, False, False
                    self.draft, self.exit_keys, self.clear_keys = '', 0, 0

                def read(self):
                    return self.pending.pop(0) if self.pending else b''

                def write(self, data):
                    if data.startswith(b'/model '):
                        owner.personal.write_text(json.dumps({'config': {'defaultModelSelection': {'modelId': 'synthetic-second'}}}))
                        self.pending.append(b'Model switched')
                    elif data == b'TUI_NATIVE_PROBE\r':
                        self.draft = 'TUI_NATIVE_PROBE'
                        owner.scenario.requests.append({'model': 'synthetic-second'})
                        self.pending.append(b'NATIVE_PROBE_OK')
                    elif data == b'\x15':
                        self.clear_keys += 1
                        self.draft = ''
                        if acknowledge:
                            self.pending.append(b'Type a prompt')
                    elif data == b'\x03':
                        if self.draft:
                            raise AssertionError('Exit key consumed by retained draft')
                        self.exit_keys += 1
                        if self.exit_keys == 2 and exits:
                            self.running = False

                def alive(self):
                    return self.running

                def wait_exit(self):
                    self.waited = True
                    if self.running:
                        raise AssertionError('TUI did not exit after Ctrl-C')

                def close(self):
                    self.closed = True

            terminal = Terminal()
            self.terminal = terminal
            clock = itertools.count()
            with patch.object(runtime, 'Terminal', return_value=terminal), \
                    patch.object(runtime.time, 'monotonic', side_effect=lambda: next(clock)), \
                    patch.object(runtime.time, 'sleep'), patch('builtins.print'):
                owner.tui()

    def test_retained_draft_is_cleared_before_confirmed_native_exit(self):
        self.drive()
        self.assertEqual(self.terminal.clear_keys, 1)
        self.assertEqual(self.terminal.exit_keys, 2)
        self.assertTrue(self.terminal.waited)
        self.assertFalse(self.terminal.running)
        self.assertTrue(self.terminal.closed)

    def test_stale_startup_placeholder_cannot_authorize_exit(self):
        with self.assertRaisesRegex(AssertionError, 'observation timed out'):
            self.drive(acknowledge=False)
        self.assertEqual(self.terminal.exit_keys, 0)
        self.assertFalse(self.terminal.waited)
        self.assertTrue(self.terminal.closed)

    def test_acknowledged_input_does_not_replace_actual_process_exit(self):
        with self.assertRaisesRegex(AssertionError, 'did not exit'):
            self.drive(exits=False)
        self.assertEqual(self.terminal.exit_keys, 2)
        self.assertTrue(self.terminal.waited)
        self.assertTrue(self.terminal.closed)


class TerminalEofContracts(unittest.TestCase):
    def terminal(self, windows=False):
        terminal = runtime.Terminal.__new__(runtime.Terminal)
        terminal.windows, terminal.handle = windows, 123
        terminal.process = SimpleNamespace(read=lambda _size: '', poll=lambda: None)
        return terminal

    def test_linux_eio_is_terminal_eof(self):
        terminal = self.terminal()
        module = sys.modules[type(terminal).__module__]
        with patch.object(module.select, 'select', return_value=([123], [], [])), \
                patch.object(module.os, 'read', side_effect=OSError(errno.EIO, 'synthetic closed slave')):
            self.assertEqual(terminal.read(), b'')

    def test_other_terminal_errors_remain_errors(self):
        terminal = self.terminal()
        module = sys.modules[type(terminal).__module__]
        with patch.object(module.select, 'select', return_value=([123], [], [])), \
                patch.object(module.os, 'read', side_effect=PermissionError(errno.EACCES, 'synthetic denial')):
            with self.assertRaises(PermissionError):
                terminal.read()

    def test_windows_terminal_eof_is_not_a_transport_failure(self):
        terminal = self.terminal(windows=True)
        terminal.process.read = lambda _size: (_ for _ in ()).throw(EOFError())
        module = sys.modules[type(terminal).__module__]
        with patch.object(module.select, 'select', return_value=([123], [], [])):
            self.assertEqual(terminal.read(), b'')

    def test_terminal_eof_does_not_certify_a_live_process_as_exited(self):
        terminal = self.terminal()
        module = sys.modules[type(terminal).__module__]
        clock = itertools.count()
        with patch.object(terminal, 'read', return_value=b''), \
                patch.object(module.time, 'monotonic', side_effect=lambda: next(clock)), \
                patch.object(module.time, 'sleep'):
            with self.assertRaisesRegex(AssertionError, 'did not exit'):
                terminal.wait_exit(timeout=2)


if __name__ == '__main__':
    unittest.main()
