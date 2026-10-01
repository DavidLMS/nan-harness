#!/usr/bin/env python3
"""Install and remove one hosted-only executable-scoped namespace policy."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import uuid


def profile_path(value):
    path = Path(value)
    if not re.fullmatch(r'/etc/apparmor.d/nanh-hermes-feasibility-[0-9a-f]{32}', str(path)):
        raise ValueError('invalid owned profile path')
    return path


def profile_text(executable, name):
    if not re.fullmatch(r'/[A-Za-z0-9/_.-]+', str(executable)):
        raise ValueError('unsupported executable path')
    if not re.fullmatch(r'nanh-hermes-feasibility-[0-9a-f]{32}', name):
        raise ValueError('invalid profile identity')
    return f'abi <abi/4.0>,\ninclude <tunables/global>\nprofile {name} "{executable}" flags=(unconfined) {{\n  userns,\n}}\n'


def run(command):
    subprocess.run(command, check=True, timeout=30, stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL)


def cleanup(state):
    if not state.exists():
        return
    value = json.loads(state.read_text())
    if set(value) != {'profilePath', 'loaded'} or type(value['loaded']) is not bool:
        raise ValueError('invalid policy journal')
    target = profile_path(value['profilePath'])
    if value['loaded']:
        run(['sudo', 'apparmor_parser', '-R', str(target)])
    run(['sudo', 'rm', '-f', '--', str(target)])
    state.unlink()


def prepare(prepared, state, runner_root):
    if state.exists():
        raise ValueError('policy journal already exists')
    receipt = json.loads(prepared.read_text())
    apps = [app for app in receipt['apps'] if app['app'] == 'hermes-desktop']
    if len(apps) != 1:
        raise ValueError('ambiguous Hermes receipt')
    executable = Path(apps[0]['executable']['path'])
    if executable.is_symlink() or not executable.is_file():
        raise ValueError('invalid installed executable')
    executable = executable.resolve(strict=True)
    executable.relative_to(runner_root)
    if Path('/proc/sys/kernel/unprivileged_userns_clone').read_text().strip() != '1':
        raise ValueError('user namespaces unavailable')
    name = 'nanh-hermes-feasibility-' + uuid.uuid4().hex
    target = profile_path('/etc/apparmor.d/' + name)
    temporary = state.with_suffix('.profile')
    with temporary.open('x') as output:
        os.chmod(temporary, 0o600)
        output.write(profile_text(executable, name))
    with state.open('x') as output:
        os.chmod(state, 0o600)
        json.dump({'profilePath': str(target), 'loaded': False}, output)
    try:
        run(['sudo', 'install', '-m', '0644', '--', str(temporary), str(target)])
        run(['sudo', 'apparmor_parser', '-r', str(target)])
        replacement = state.with_suffix('.next')
        with replacement.open('x') as output:
            os.chmod(replacement, 0o600)
            json.dump({'profilePath': str(target), 'loaded': True}, output)
        replacement.replace(state)
    except Exception:
        cleanup(state)
        raise
    finally:
        temporary.unlink()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('operation', choices=['prepare', 'cleanup'])
    parser.add_argument('--prepared', type=Path)
    parser.add_argument('--state', type=Path, required=True)
    args = parser.parse_args()
    if sys.platform != 'linux' or os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted':
        raise ValueError('disposable Linux hosted runner required')
    runner_root = Path(os.environ['RUNNER_TEMP']).resolve(strict=True)
    args.state.parent.resolve(strict=True).relative_to(runner_root)
    if args.operation == 'cleanup':
        cleanup(args.state)
    else:
        if args.prepared is None:
            raise ValueError('prepared receipt required')
        prepare(args.prepared, args.state, runner_root)


if __name__ == '__main__':
    main()
