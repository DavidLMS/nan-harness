#!/usr/bin/env python3
"""Publish only a closed read-only hosted Windows session preflight."""
import json
import os
from pathlib import Path
import subprocess
import sys


def validate(value):
    fields = set('schemaVersion mechanism diagnosticsOnly userInteractive sameConsoleSession foregroundPresent displayCount uiaRootPresent uiaChildCount errorCategory errorCode'.split())
    if type(value) is not dict or set(value) != fields or type(value['schemaVersion']) is not int or value['schemaVersion'] != 1 or value['mechanism'] != 'windows-session' or value['diagnosticsOnly'] is not True:
        raise ValueError('invalid session schema')
    for key in ('userInteractive', 'sameConsoleSession', 'foregroundPresent', 'uiaRootPresent'):
        if type(value[key]) is not bool:
            raise ValueError('invalid session flag')
    for key, maximum in [('displayCount', 32), ('uiaChildCount', 4096)]:
        if value[key] is not None and (type(value[key]) is not int or not 0 <= value[key] <= maximum):
            raise ValueError('invalid session count')
    if value['errorCategory'] not in {None, 'native-query', 'uia-query', 'timeout', 'transport'}:
        raise ValueError('invalid session category')
    if value['errorCode'] is not None and (type(value['errorCode']) is not int or not -(2**31) <= value['errorCode'] < 2**31):
        raise ValueError('invalid session error code')
    return value


def run(output):
    if sys.platform != 'win32' or os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted':
        raise ValueError('hosted Windows session required')
    with subprocess.Popen(['powershell.exe', '-NoProfile', '-NonInteractive', '-Sta', '-File',
                           str(Path(__file__).with_suffix('.ps1'))], stdin=subprocess.DEVNULL,
                          stdout=subprocess.PIPE, stderr=subprocess.DEVNULL) as child:
        try:
            raw, _ = child.communicate(timeout=30)
        except subprocess.TimeoutExpired:
            child.kill()
            child.communicate()
            raise ValueError('session observation timed out') from None
    if child.returncode != 0 or len(raw) > 4096:
        raise ValueError('session observation rejected')
    value = validate(json.loads(raw))
    with output.open('x') as destination:
        json.dump(value, destination)
        destination.write('\n')


if __name__ == '__main__':
    try:
        run(Path(sys.argv[1]))
    except (IndexError, ValueError, OSError):
        raise SystemExit('closed Windows preflight could not complete') from None
