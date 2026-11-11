#!/usr/bin/env python3
"""Project reduced Desktop evidence into a closed, advisory Actions summary."""
import argparse
import importlib.util
from pathlib import Path

from desktop_diagnostics import APPS, PLATFORMS, REASONS
from desktop_qualification import bounded_json

ROOT = Path(__file__).resolve().parents[2]
_spec = importlib.util.spec_from_file_location(
    'qualification_retry_policy', ROOT / 'scripts/desktop-feasibility/run-qualification.py')
_runner = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_runner)

STATUSES = {'passed', 'failed', 'blocked', 'skipped'}
LAUNCH_STAGES = {'not-started', 'started', 'exited-before-window', 'window-unavailable',
                 'window-acquired', 'renderer-acquired'}
READINESS_STAGES = {'policy', 'onboarding', 'menu', 'refresh', 'catalog', 'dismiss', 'composer', 'ready'}
READINESS_ERRORS = {'policy-rejected', 'onboarding-unavailable', 'menu-unavailable',
                    'menu-timeout', 'refresh-uncertain', 'catalog-unavailable', 'dismiss-uncertain',
                    'composer-changed', 'composer-unavailable'}


def closed(value, allowed, missing='unavailable'):
    return value if type(value) is str and value in allowed else missing


def records(value, maximum):
    if not isinstance(value, list) or len(value) > maximum or any(type(item) is not dict for item in value):
        raise ValueError('invalid summary records')
    return value


def render(result):
    if (type(result) is not dict or type(result.get('schemaVersion')) is not int
            or result['schemaVersion'] != 1 or result.get('app') not in APPS
            or result.get('platform') not in PLATFORMS):
        raise ValueError('invalid summary envelope')
    probes = records(result.get('probes', []), 3)
    observations = records(result.get('semanticObservations', []), 96)
    native = records(result.get('nativeDiagnostics') or [], 64)
    outcome = closed(result.get('outcome'), {'passed', 'blocked'})
    lines = [f"### Desktop qualification: {result['app']} / {result['platform']}", '',
             f'Outcome: **{outcome}**.',
             'Cleanup: application **' + closed(result.get('appCleanup'), STATUSES)
             + '**; global **' + closed(result.get('globalCleanup'), STATUSES) + '**.']
    try:
        retry = ('not needed' if outcome == 'passed' else
                 'eligible' if _runner.retryable_qualification(result) else 'ineligible')
    except (KeyError, TypeError, ValueError, AttributeError):
        retry = 'unavailable'
    lines += [f'Retry policy: **{retry}** (eligibility only; the attempt limit still applies).', '',
              '| Probe (1-based) | Status | Failure category | Native launch stage |',
              '| --- | --- | --- | --- |']
    for index, probe in enumerate(probes):
        stages = {closed(event.get('record', {}).get('launchStage'), LAUNCH_STAGES)
                  for event in native if type(event.get('record')) is dict
                  and type(event['record'].get('probeIndex')) is int
                  and event['record']['probeIndex'] == index}
        stage = next(iter(stages)) if len(stages) == 1 else 'unavailable'
        reason = closed(probe.get('reason'), REASONS, 'none' if probe.get('status') == 'passed' else 'unavailable')
        lines.append(f"| {index + 1} | {closed(probe.get('status'), STATUSES)} | {reason} | {stage} |")
    if not probes:
        lines += ['', 'No probe evidence was produced.']
    readiness = set()
    for item in observations:
        if item.get('mechanism') == 'hermes-windows-catalog-readiness' and item.get('stage') != 'ready':
            readiness.add((closed(item.get('stage'), READINESS_STAGES),
                           closed(item.get('errorCategory'), READINESS_ERRORS)))
    if readiness:
        lines += ['', 'Readiness observations (not individually correlated to probes):']
        lines += [f'- Stage **{stage}**; category **{category}**.' for stage, category in sorted(readiness)]
    oracle = [item for item in observations if item.get('mechanism') == 'semantic-provider-oracle']
    if oracle:
        expected = sum(item.get('stage') == 'failure' and item.get('failureObserved') is True
                       for item in oracle)
        recovered = sum(item.get('stage') == 'recovery' and item.get('fixtureResponseVerified') is True
                        for item in oracle)
        unverified = sum(item.get('stage') == 'recovery' and item.get('fixtureResponseVerified') is not True
                         for item in oracle)
        lines += ['', f'Provider observations: {expected} expected injected failures observed; '
                  f'{recovered} verified recovery responses; {unverified} unverified recovery responses.',
                  '`fixtureResponseVerified=false` during the injected failure phase is not a separate '
                  'response failure. Recovery verification remains required; these counts do not replace '
                  'the probe verdicts.']
    return '\n'.join(lines) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        summary = render(bounded_json(args.report, 65536))
    except (OSError, ValueError, TypeError, RecursionError):
        summary = '### Desktop qualification\n\nReduced evidence is absent or unavailable for a safe summary.\n'
    with args.output.open('a', encoding='utf-8') as output:
        output.write(summary)


if __name__ == '__main__':
    main()
