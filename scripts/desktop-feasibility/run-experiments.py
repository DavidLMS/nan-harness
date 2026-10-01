#!/usr/bin/env python3
"""Run owned semantic probes or same-runtime Hermes startup/CDP A/B cells."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'canary' / 'actions'))
from desktop_diagnostics import validate_bundle
from cell import write_json


def conditions(app, experiment='strict-semantic'):
    if experiment == 'native-copy-dom':
        return ['native-copy'] if app == 'zed-desktop' else ['startup-baseline', 'playwright-dom']
    return ['strict-ax'] if app == 'zed-desktop' else ['without-cdp', 'with-cdp']


def experiment_environment(app, condition, facts, real_nanh):
    environment = os.environ.copy()
    for name in ('NAN_API_KEY', 'OPENAI_API_KEY', 'ANTHROPIC_API_KEY', 'CODEX_API_KEY',
                 'GH_TOKEN', 'GITHUB_TOKEN', 'FEASIBILITY_ZED_AX_FACTS',
                 'FEASIBILITY_ZED_NATIVE_COPY_FACTS', 'FEASIBILITY_HERMES_DOM_FACTS',
                 'FEASIBILITY_HERMES_DOM_INPUT', 'FEASIBILITY_HERMES_CDP',
                 'FEASIBILITY_HERMES_DOM_DRIVER', 'FEASIBILITY_HERMES_STARTUP_CAPTURE',
                 'FEASIBILITY_HERMES_EXECUTABLE'):
        environment.pop(name, None)
    if app != 'zed-desktop':
        environment.pop('FEASIBILITY_ZED_INPUT_DRIVER', None)
    environment['FEASIBILITY_REAL_NANH'] = str(real_nanh)
    environment['FEASIBILITY_FACTS'] = str(facts)
    if condition == 'native-copy':
        environment['FEASIBILITY_ZED_NATIVE_COPY_FACTS'] = str(facts)
    elif app == 'zed-desktop':
        environment['FEASIBILITY_ZED_AX_FACTS'] = str(facts)
    else:
        environment['FEASIBILITY_HERMES_CDP'] = 'enabled' if condition in ('with-cdp', 'playwright-dom') else 'disabled'
        environment['FEASIBILITY_HERMES_STARTUP_CAPTURE'] = '1'
        if condition == 'playwright-dom':
            environment['FEASIBILITY_HERMES_DOM_INPUT'] = '1'
            environment['FEASIBILITY_HERMES_DOM_FACTS'] = str(facts)
            environment['FEASIBILITY_HERMES_DOM_DRIVER'] = str(Path(__file__).with_name('observe-hermes.cjs').resolve())
    return environment


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--app', choices=['zed-desktop', 'hermes-desktop'], required=True)
    parser.add_argument('--experiment', choices=['strict-semantic', 'native-copy-dom'], default='strict-semantic')
    for name in ('checker', 'real-nanh', 'prepared', 'frozen', 'directory'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--release-tag', required=True)
    parser.add_argument('--platform', choices=['linux', 'macos'], required=True)
    args = parser.parse_args()
    if os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted':
        raise SystemExit('Disposable hosted runner required.')
    arms = []
    frozen_digest = hashlib.sha256(args.frozen.read_bytes()).hexdigest()
    prepared = json.loads(args.prepared.read_text())
    prepared_app = next((app for app in prepared.get('apps', []) if app.get('app') == args.app), {})
    for condition in conditions(args.app, args.experiment):
        directory = args.directory / condition
        facts = directory / 'facts'
        facts.mkdir(mode=0o700, parents=True, exist_ok=False)
        report = directory / 'report.json'
        diagnostic = directory / 'diagnostics.json'
        environment = experiment_environment(args.app, condition, facts, args.real_nanh)
        if args.app == 'hermes-desktop':
            environment['FEASIBILITY_HERMES_EXECUTABLE'] = prepared_app['executable']['path']
        command = [sys.executable, str(ROOT / 'canary/actions/desktop_diagnostics.py'),
                   '--output', str(diagnostic), '--source-sha', args.source_sha,
                   '--platform', args.platform, '--timeout', '1200', '--', str(args.checker),
                   'run', '--app', args.app, '--model', 'qwen3.6', '--yes', '--non-interactive',
                   '--ephemeral', '--mode', 'deterministic', '--session', 'github-hosted',
                   '--prepared', str(args.prepared), '--output', str(report)]
        completed = subprocess.run(command, env=environment, timeout=1260, check=False)
        if report.exists():
            subprocess.run([str(args.checker), 'validate-report', str(report)],
                           env=environment, timeout=30, stdout=subprocess.DEVNULL, check=True)
        closed = validate_bundle(diagnostic, args.source_sha, args.platform)
        summary = directory / 'summary.json'
        reduction = [sys.executable, str(Path(__file__).with_name('summarize.py')),
                     '--report', str(report), '--frozen', str(args.frozen), '--prepared', str(args.prepared),
                     '--real-nanh', str(args.real_nanh), '--checker', str(args.checker),
                     '--shim', str(Path(__file__).with_name('nanh-shim.py')),
                     '--facts', str(facts), '--output', str(summary), '--source-sha', args.source_sha,
                     '--platform', args.platform + ('-aarch64' if args.platform == 'macos' else '-x86_64'),
                     '--app', args.app]
        subprocess.run(reduction, env=environment, timeout=30, check=True)
        arms.append(dict(condition=condition, checkerExit=completed.returncode,
                         frozenManifestSha256=frozen_digest,
                         diagnosticSha256=hashlib.sha256(diagnostic.read_bytes()).hexdigest(),
                         diagnostics=closed, summary=json.loads(summary.read_text())))
        if hashlib.sha256(args.frozen.read_bytes()).hexdigest() != frozen_digest:
            raise ValueError('frozen manifest changed between experiment arms')
        if arms[-1]['summary']['appCleanup'] != 'passed' or arms[-1]['summary']['reportCleanup'] != 'passed':
            break
    combined = dict(schemaVersion=1, experimentOnly=True, sourceCommit=args.source_sha,
                    app=args.app, experiment=args.experiment, officialReleaseTag=args.release_tag,
                    noOcrQualification=False, arms=arms)
    write_json(args.directory / 'summary.json', combined)
    # An experiment can complete with a blocked observation. It is never a
    # compatibility approval, and cleanup failure must not start another arm.
    if any(arm['summary']['appCleanup'] != 'passed' or arm['summary']['reportCleanup'] != 'passed'
           for arm in arms):
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
