#!/usr/bin/env python3
"""Run a pinned, credential-free Windows startup experiment; never release acceptance."""
import json
import os
from pathlib import Path
import shutil
import sys

from windows_diagnostic import isolated_environment, run_bounded
from cell import protect_private

ROOT = Path(__file__).resolve().parents[2]
TEST = 'conformance::runner::kimi_startup_diagnostic::native_kimi_startup_boundaries'


CASES = ['fresh-version', 'fresh-help', 'sentinel-cleared-environment',
         'sentinel-runtime-environment', 'sentinel-canonical-helper', 'sentinel-canonical-helper']

ERROR_SIGNALS = frozenset((
    'not-found', 'permission', 'operation-permission', 'bad-descriptor', 'invalid-argument',
    'broken-pipe', 'connection-refused', 'timeout', 'type-error', 'syntax-error', 'reference-error',
    'range-error', 'permission-denied', 'access-denied', 'missing-file', 'win32-missing', 'win32-denied',
    'database-locked', 'sqlite', 'configuration', 'rust-panic', 'assertion', 'unsupported',
    'unknown-option', 'certificate', 'stdin', 'stdout', 'stderr', 'console', 'terminal', 'mcp',
    'shell', 'utf8', 'json', 'toml', 'spawn', 'bun',
))


def bounded_integer(value, minimum=0, maximum=2**31):
    return type(value) is int and minimum <= value <= maximum


def safe_process(value):
    if not isinstance(value, dict):
        return False
    if value == {'status': 'process-error'}:
        return True
    return (set(value) == {'status', 'exitCode', 'stdoutBytes', 'stderrBytes', 'marker', 'errorSignals'}
            and value['status'] in ('passed', 'nonzero-exit')
            and (value['exitCode'] is None or bounded_integer(value['exitCode'], -2**31, 2**32 - 1))
            and all(bounded_integer(value[key]) for key in ('stdoutBytes', 'stderrBytes'))
            and type(value['marker']) is bool
            and isinstance(value['errorSignals'], list) and len(value['errorSignals']) <= len(ERROR_SIGNALS)
            and all(isinstance(code, str) and code in ERROR_SIGNALS for code in value['errorSignals']))


def closed_observation(value, completed):
    if (not isinstance(value, dict)
            or set(value) != {'schemaVersion', 'diagnosticsOnly', 'expectedVersion', 'records'}
            or type(value['schemaVersion']) is not int or value['schemaVersion'] != 1
            or value['diagnosticsOnly'] is not True or value['expectedVersion'] != '2.5.3'
            or not isinstance(value['records'], list) or len(value['records']) > len(CASES)
            or (completed and len(value['records']) != len(CASES))):
        return None
    for index, record in enumerate(value['records']):
        keys = {'case', 'process', 'versionVerified' if index < 2 else 'provider'}
        if (not isinstance(record, dict) or set(record) != keys
                or record['case'] != CASES[index] or not safe_process(record['process'])
                or (completed and record['process']['status'] == 'process-error')):
            return None
        if index < 2:
            if type(record['versionVerified']) is not bool:
                return None
        else:
            provider = record['provider']
            if (not isinstance(provider, dict)
                    or set(provider) != {'requestCount', 'completed', 'bounded', 'shutdown'}
                    or not bounded_integer(provider['requestCount'], maximum=10_000)
                    or any(type(provider[key]) is not bool for key in ('completed', 'bounded', 'shutdown'))
                    or (completed and provider['shutdown'] is not True)):
                return None
    return value


def main():
    output = Path(os.environ['RUNNER_TEMP']) / 'kimi-startup-report.json'
    cell = Path(os.environ['RUNNER_TEMP']) / 'kimi-startup-private'
    cell.mkdir(exist_ok=False)
    protect_private(cell)
    report = {'schemaVersion': 1, 'diagnosticsOnly': True,
              'sourceSha': os.environ['GITHUB_SHA'], 'expectedVersion': '2.5.3',
              'install': 'not-started', 'experiment': 'not-started', 'cleanup': 'not-started'}
    try:
        env = isolated_environment(cell)
        installer = ROOT / 'canary/guest/install-harness.ps1'
        code, _ = run_bounded(['pwsh', '-NoProfile', '-NonInteractive', '-File', str(installer),
                              '-Harness', 'kimi-code', '-Version', '2.5.3'], cell, env, 900)
        report['install'] = 'passed' if code == 0 else 'failed'
        if code != 0:
            raise RuntimeError('Pinned installation failed; closed evidence was retained.')
        build = Path(os.environ['RUNNER_TEMP']) / 'kimi-startup-test-build.jsonl'
        executables = []
        for line in build.read_text(encoding='utf-8').splitlines():
            record = json.loads(line)
            if (record.get('reason') == 'compiler-artifact'
                    and record.get('target', {}).get('name') == 'nan_harness_test_support'
                    and record.get('profile', {}).get('test') and record.get('executable')):
                executables.append(record['executable'])
        if len(executables) != 1:
            raise ValueError('one compiled diagnostic test executable required')
        closed = cell / 'closed-report.json'
        env.update({'NAN_KIMI_STARTUP_BINARY': str(ROOT / 'target/debug/nanh.exe'),
                    'NAN_KIMI_STARTUP_EXECUTABLE': str(cell / 'home/.kimi-code/bin/kimi.exe'),
                    'NAN_KIMI_STARTUP_OUTPUT': str(closed)})
        code, _ = run_bounded([executables[0], '--exact', TEST, '--ignored'], cell, env, 600)
        report['experiment'] = 'completed' if code == 0 else 'failed'
        if closed.is_file() and closed.stat().st_size <= 32_768:
            observation = closed_observation(json.loads(closed.read_text(encoding='utf-8')), code == 0)
            if observation is not None:
                report['observation'] = observation
        if 'observation' not in report:
            report['experiment'] = 'failed'
    finally:
        try:
            shutil.rmtree(cell)
            report['cleanup'] = 'passed'
        except OSError:
            report['cleanup'] = 'failed'
        output.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    if report['experiment'] != 'completed' or report['cleanup'] != 'passed':
        raise SystemExit('Startup experiment did not complete; closed evidence was retained.')


if __name__ == '__main__':
    main()
