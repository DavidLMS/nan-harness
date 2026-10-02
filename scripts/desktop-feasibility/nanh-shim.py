#!/usr/bin/env python3
"""Experiment-only launcher: retain nanh ownership, observe Hermes renderer."""
import os
from pathlib import Path
import socket
import signal
import subprocess
import sys
import json
import runpy

Capture = runpy.run_path(str(Path(__file__).with_name("hermes-startup.py")))["Capture"]

real = os.environ['FEASIBILITY_REAL_NANH']
args = sys.argv[1:]
if not args or args[0] not in {'hermes-desktop', 'chatgpt-desktop', 'claude-desktop', 'pen-desktop'} or '--provider-base-url' not in args:
    os.execv(real, [real, *args])
hermes = args[0] == 'hermes-desktop'
cdp = os.environ.get('FEASIBILITY_HERMES_CDP', 'enabled') != 'disabled' and not (not hermes and os.environ.get('NANH_DESKTOP_QUALIFICATION_MODE') == 'startup-baseline')
command = [real, *args]
scoped_namespace = os.environ.get('FEASIBILITY_HERMES_NAMESPACE_POLICY') == 'scoped-apparmor-userns'
if scoped_namespace and hermes:
    command.extend([*([] if '--' in args else ['--']), '--disable-setuid-sandbox'])
if cdp:
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    delimiter = [] if '--' in command else ['--']
    if hermes:
        command.extend([*delimiter, f'--remote-debugging-port={port}',
                        '--remote-debugging-address=127.0.0.1'])
    else:
        os.environ['NANH_DESKTOP_QUALIFICATION_CDP_PORT'] = str(port)
child = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
capture = Capture(child.stderr)
capture.start()
code = None
observer = None
startup_output = Path(os.environ['FEASIBILITY_FACTS']) / f'closed-startup-{child.pid}.json'
def interrupted(number, _frame):
    if hermes:
        capture.save(startup_output, code, join_timeout=0)
    raise SystemExit(128 + number)

signal.signal(signal.SIGTERM, interrupted)
signal.signal(signal.SIGINT, interrupted)
try:
    if cdp and (not hermes or os.environ.get('FEASIBILITY_HERMES_DOM_INPUT') == '1'):
        connection = Path(os.environ['FEASIBILITY_FACTS']) / f'connection-{os.getpid()}.json'
        with connection.open('x') as output:
            os.chmod(connection, 0o600)
            json.dump({'schemaVersion': 1, 'port': port, 'launcherPid': child.pid}, output)
    elif cdp:
        observer = subprocess.Popen(['node', str(Path(__file__).with_name('observe-hermes.cjs')),
                                 str(port), str(child.pid),
                                 str(Path(os.environ['FEASIBILITY_FACTS']) / f'{child.pid}.json')],
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if hermes:
        capture.save(startup_output, code, join_timeout=0)
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
    if hermes:
        capture.save(startup_output, code)
sys.exit(code)
