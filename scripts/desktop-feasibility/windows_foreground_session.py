"""Temporary foreground timeout policy for a disposable hosted Windows desktop."""
from contextlib import contextmanager
import ctypes
from ctypes import wintypes
import os
from pathlib import Path
import sys

from cell import write_json


class ForegroundTimeout:
    def __init__(self):
        self.spi = ctypes.WinDLL('user32', use_last_error=True).SystemParametersInfoW
        self.spi.argtypes = [wintypes.UINT, wintypes.UINT, ctypes.c_void_p, wintypes.UINT]
        self.spi.restype = wintypes.BOOL

    def read(self):
        value = wintypes.DWORD()
        if not self.spi(0x2000, 0, ctypes.byref(value), 0):
            raise RuntimeError('foreground timeout query failed')
        return value.value

    def set(self, value):
        # pvParam is the timeout itself for SET, not a pointer to a DWORD.
        # Flags zero neither persists the value nor broadcasts a setting change.
        if not self.spi(0x2001, 0, ctypes.c_void_p(value), 0):
            raise RuntimeError('foreground timeout update failed')


@contextmanager
def prepare(facts):
    if (sys.platform != 'win32' or os.environ.get('GITHUB_ACTIONS') != 'true'
            or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted'
            or os.environ.get('RUNNER_OS') != 'Windows'):
        raise ValueError('foreground preparation requires hosted Windows')
    receipt = dict(schemaVersion=1, mechanism='windows-foreground-session',
                   diagnosticsOnly=True, stage='read', originalTimeoutMs=None,
                   prepared=False, restored=False, failureStage=None)
    api = None
    original = None
    changed = False
    try:
        api = ForegroundTimeout()
        original = api.read()
        receipt['originalTimeoutMs'] = original
        receipt['stage'] = 'prepare'
        if original != 0:
            # Even a failed SET is followed by restoration: do not assume it
            # could not have changed state before reporting failure.
            changed = True
            api.set(0)
        receipt['stage'] = 'verify'
        if api.read() != 0:
            raise RuntimeError('foreground timeout preparation unverified')
        receipt.update(prepared=True, stage='running')
        yield
    except (OSError, RuntimeError, ValueError):
        receipt['failureStage'] = receipt['stage']
        raise
    finally:
        try:
            if original is not None:
                receipt['stage'] = 'restore'
                if changed:
                    api.set(original)
                if api.read() != original:
                    raise RuntimeError('foreground timeout restoration unverified')
                receipt['restored'] = True
                if receipt['prepared']:
                    receipt['stage'] = 'completed'
        except (OSError, RuntimeError, ValueError):
            if receipt['failureStage'] is None:
                receipt['failureStage'] = 'restore'
            raise
        finally:
            write_json(Path(facts) / 'windows-foreground-session.json', receipt)
