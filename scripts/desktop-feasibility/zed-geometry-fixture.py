#!/usr/bin/env python3
"""Exercise the actual Linux geometry probe against an owned synthetic process."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'canary/actions'))
from zed_retry_trace import attach_failure, read_ready
from zed_hit_geometry import observations, probe, save_target, split_maps


def diagnostics(data):
    result = {category for line in data.splitlines() if (category := attach_failure(line)) is not None}
    lowered = data.lower()
    # Fixed signatures only; compiler and verifier text never leaves this runner.
    for phrase, category in (
            (b'error loading program:', 'program-load'),
            (b'bpf program is too large', 'verifier-complexity'),
            (b'infinite loop detected', 'verifier-loop'),
            (b'invalid mem access', 'verifier-memory'),
            (b'invalid access to map', 'verifier-map'),
            (b'invalid indirect read from stack', 'verifier-stack'),
            (b'unknown identifier', 'compiler-identifier'),
            (b'unknown function', 'compiler-function'),
            (b'cannot cast', 'compiler-cast'),
            (b'type mismatch', 'compiler-type'),
            (b'failed to create map', 'map-creation')):
        if phrase in lowered:
            result.add(category)
    return sorted(result)


def run(directory, receipt):
    executable = directory / 'fixture'
    receipt['stage'] = 'build'
    subprocess.run(['cc', '-O0', '-g', str(Path(__file__).with_suffix('.c')), '-o', str(executable)],
                   check=True, timeout=30, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    save_target(directory, dict(point=[30, 30], bounds=[10, 20, 40, 20], viewport=[200, 200]))
    source = ('BEGIN { @active = 1; @slot = 1; @geometrySlot = 0; }\n'
              + probe(executable, 'synthetic_dispatch')
              + '\nEND { delete(@active); delete(@slot); delete(@geometrySlot); }')
    prefix = ['sudo', '-n', 'env', '__BPFTRACE_NOTIFY_PROBES_ATTACHED=1', 'BPFTRACE_STRLEN=128',
              '/usr/bin/bpftrace', '-kk', '-B', 'none', '-f', 'json']
    environment = {'PATH': '/usr/bin:/bin', 'LANG': 'C'}
    receipt['stage'] = 'compile'
    compiled = subprocess.run([*prefix, '-d', '-e', source], timeout=30,
                              stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, env=environment)
    if compiled.returncode != 0:
        receipt['failure'] = 'compilation'
        receipt['diagnosticCategories'] = diagnostics(compiled.stderr)
        return
    capture = directory / 'maps.jsonl'
    capture.touch(mode=0o600)
    receipt['stage'] = 'attach'
    categories = []
    def observe(line):
        categories.extend(diagnostics(line))
    process = subprocess.Popen(
        [*prefix, '-v', '-o', str(capture), '-e', source],
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        start_new_session=True, env=environment)
    try:
        ready = read_ready(process.stderr, observe=observe)
        if ready and process.poll() is None:
            receipt['stage'] = 'dispatch'
            subprocess.run([str(executable)], check=True, timeout=5,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        else:
            receipt['failure'] = 'readiness-incomplete' if process.poll() is None else 'tracer-exited'
    finally:
        if process.poll() is None:
            subprocess.run(['sudo', '-n', 'kill', '-INT', '--', str(-process.pid)], check=True,
                           timeout=3, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            output, errors = process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            subprocess.run(['sudo', '-n', 'kill', '-KILL', '--', str(-process.pid)], check=True,
                           timeout=3, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            output, errors = process.communicate(timeout=3)
            receipt['failure'] = 'stop-timeout'
        for line in errors.splitlines():
            observe(line)
        receipt['diagnosticCategories'] = sorted(set(categories) | set(diagnostics(output + errors)))
        receipt['processExitedSuccessfully'] = process.returncode == 0
    if receipt['failure'] is not None or process.returncode != 0:
        return
    receipt['stage'] = 'readback'
    remaining, maps = split_maps(capture.read_bytes())
    expected = dict(status='matched', renderedHitboxes=2, boundsMatches=1,
                    priorPointerMatches=True, targetMaskContainsPoint=True,
                    blockingHitboxesAhead=1, targetWouldBeHovered=False)
    if remaining.strip() or observations(maps, directory, 1) != [expected]:
        receipt['failure'] = 'geometry-mismatch'
        return
    receipt.update(status='passed', stage='complete')


def main():
    if (sys.platform != 'linux' or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted'
            or os.environ.get('GITHUB_ACTIONS') != 'true'):
        raise SystemExit('synthetic tracing requires hosted Linux')
    destination = Path(os.environ['RUNNER_TEMP']) / 'zed-geometry-fixture.json'
    receipt = dict(schemaVersion=1, mechanism='synthetic-zed-geometry', status='failed',
                   stage='prepare', failure=None, diagnosticCategories=[], processExitedSuccessfully=False)
    try:
        with tempfile.TemporaryDirectory(prefix='zed-geometry-', dir=os.environ['RUNNER_TEMP']) as temporary:
            run(Path(temporary).resolve(), receipt)
    except (OSError, ValueError, subprocess.SubprocessError):
        receipt['failure'] = 'fixture-error'
    finally:
        destination.write_text(json.dumps(receipt, sort_keys=True) + '\n')
        destination.chmod(0o600)
    return 0 if receipt['status'] == 'passed' else 1


if __name__ == '__main__':
    raise SystemExit(main())
