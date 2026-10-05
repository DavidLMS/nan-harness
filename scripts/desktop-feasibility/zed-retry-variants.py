#!/usr/bin/env python3
"""Compare two guarded Retry points after the control, reusing immutable binaries."""
import json
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'canary/actions'))
from cell import ensure_private_directory


def cleanup_passed(path):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 262144:
        return False
    value = json.loads(path.read_bytes())
    return value.get('appCleanup') == 'passed' and value.get('globalCleanup') == 'passed'


def main():
    if (sys.platform != 'linux' or os.environ.get('GITHUB_ACTIONS') != 'true'
            or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted'
            or os.environ.get('NANH_ZED_RETRY_ENTRY_TRACE') != '1'):
        raise ValueError('hosted instrumented Zed session required')
    root = Path(os.environ['RUNNER_TEMP']) / 'semantic-feasibility'
    checker = os.environ['FEASIBILITY_CHECKER']
    launcher = os.environ['FEASIBILITY_REAL_NANH']
    common = ['--app', 'zed-desktop', '--platform', 'linux', '--source-sha', os.environ['GITHUB_SHA'],
              '--checker', checker, '--real-nanh', launcher,
              '--prepared', str(root / 'prepared.json'), '--frozen', str(root / 'frozen.json')]
    previous = root / 'qualification.json'
    output = root / 'retry-variants'
    ensure_private_directory(output)
    for placement in ('left-quarter', 'right-quarter'):
        # A failed assertion may be compared; uncertain teardown must not leak
        # state into another experiment on this shared disposable runner.
        if not cleanup_passed(previous):
            return 1
        directory = output / placement
        ensure_private_directory(directory)
        environment = {**os.environ, 'NANH_ZED_RETRY_POINT': placement}
        subprocess.run([sys.executable, str(Path(__file__).with_name('run-qualification.py')),
                        *common, '--directory', str(directory)], env=environment,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=1300, check=False)
        report = directory / 'report.json'
        previous = directory / 'qualification.json'
        reducer = Path(__file__).resolve().parents[2] / 'canary/actions/desktop_qualification.py'
        subprocess.run([sys.executable, str(reducer), 'reduce', *common,
                        '--architecture', 'x86_64', '--model', 'qwen3.6', '--launcher', launcher,
                        '--report', str(report), '--facts', str(directory / 'qualification-facts'),
                        '--output', str(previous)], stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL, timeout=60, check=False)
    return 0 if cleanup_passed(previous) else 1


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, subprocess.SubprocessError):
        sys.exit(1)
