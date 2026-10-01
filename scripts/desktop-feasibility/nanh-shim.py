#!/usr/bin/env python3
"""Experiment-only launcher: retain nanh ownership, observe Hermes renderer."""
import os
from pathlib import Path
import socket
import signal
import subprocess
import sys

real = os.environ['FEASIBILITY_REAL_NANH']
args = sys.argv[1:]
if not args or args[0] != 'hermes-desktop' or '--provider-base-url' not in args:
    os.execv(real, [real, *args])
if os.environ.get('FEASIBILITY_HERMES_CDP', 'enabled') == 'disabled':
    os.execv(real, [real, *args])
with socket.socket() as listener:
    listener.bind(('127.0.0.1', 0))
    port = listener.getsockname()[1]
# One input owner: the existing checker. The sidecar only observes DOM state.
delimiter = [] if '--' in args else ['--']
child = subprocess.Popen([real, *args, *delimiter, f'--remote-debugging-port={port}',
                          '--remote-debugging-address=127.0.0.1'])
observer = None
def interrupted(number, _frame):
    raise SystemExit(128 + number)

signal.signal(signal.SIGTERM, interrupted)
signal.signal(signal.SIGINT, interrupted)
try:
    observer = subprocess.Popen(['node', str(Path(__file__).with_name('observe-hermes.cjs')),
                                 str(port), str(child.pid),
                                 str(Path(os.environ['FEASIBILITY_FACTS']) / f'{child.pid}.json')],
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    code = child.wait()
finally:
    if child.poll() is None:
        child.terminate()
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=5)
    if observer is not None:
        try:
            observer.wait(timeout=5)
        except subprocess.TimeoutExpired:
            observer.terminate()
            observer.wait(timeout=5)
sys.exit(code)
