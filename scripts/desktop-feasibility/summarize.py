#!/usr/bin/env python3
"""Reduce private feasibility evidence into closed, experiment-only facts."""
import argparse
import hashlib
import json
from pathlib import Path


def semantic_zed(report):
    apps = [app for app in report.get('results', []) if app.get('app') == 'zed-desktop']
    probes = apps[0].get('deterministic', []) if len(apps) == 1 else []
    return bool(len(probes) == 3 and report.get('cleanup') == 'passed'
                and apps[0].get('cleanup') == 'passed'
                and all(p.get('status') == 'passed'
                        and p.get('inputMode') in {'accessibility', 'accessibility-and-keyboard'}
                        and p.get('responseVerification') == 'accessibility' for p in probes))


def main():
    parser = argparse.ArgumentParser()
    for name in ('report', 'frozen', 'real-nanh', 'checker', 'output', 'source-sha', 'platform', 'app'):
        parser.add_argument('--' + name, required=True)
    parser.add_argument('--facts')
    parser.add_argument('--shim')
    args = parser.parse_args()
    report = json.loads(Path(args.report).read_text()) if Path(args.report).exists() else {}
    manifest = json.loads(Path(args.frozen).read_text()) if Path(args.frozen).exists() else {}
    entry = next((a for a in manifest.get('apps', []) if a.get('app') == args.app), {})
    result = dict(schemaVersion=1, experimentOnly=True, sourceCommit=args.source_sha,
                  platform=args.platform, app=args.app,
                  appVersion=entry.get('version'), runtimeVersion=entry.get('runtimeVersion'),
                  upstreamRevision=entry.get('revision'),
                  realNanhSha256=hashlib.sha256(Path(args.real_nanh).read_bytes()).hexdigest() if Path(args.real_nanh).exists() else None)
    result['checkerSha256'] = hashlib.sha256(Path(args.checker).read_bytes()).hexdigest() if Path(args.checker).exists() else None
    result['reportSha256'] = hashlib.sha256(Path(args.report).read_bytes()).hexdigest() if Path(args.report).exists() else None
    result['noOcrQualification'] = False
    result['stage'] = 'measured' if report and entry.get('version') else 'preparation-or-report-missing'
    if args.app == 'zed-desktop':
        result['semanticConversationReadback'] = bool(semantic_zed(report) and entry.get('version') and result['checkerSha256'] and len(args.source_sha) == 40)
        result['verdict'] = 'conversation-readback-viable' if result['semanticConversationReadback'] else 'inconclusive'
    else:
        observations = []
        for path in Path(args.facts).glob('*.json'):
            value = json.loads(path.read_text())
            keys = {'schemaVersion', 'mechanism', 'endpointOwned', 'attached', 'uniqueComposer', 'inputReadback', 'syntheticTextPresent'}
            if set(value) != keys or value['schemaVersion'] != 1 or value['mechanism'] != 'hermes-cdp':
                raise ValueError('invalid observer facts')
            if any(type(value[k]) is not bool for k in keys - {'schemaVersion', 'mechanism'}):
                raise ValueError('invalid observer flag')
            observations.append(value)
        result['shimSha256'] = hashlib.sha256(Path(args.shim).read_bytes()).hexdigest()
        result['observations'] = observations
        result['verdict'] = 'renderer-readable' if any(v['endpointOwned'] and v['attached'] and v['uniqueComposer'] for v in observations) else 'inconclusive'
        result['domDrivenQualification'] = False
    Path(args.output).parent.mkdir(parents=True, exist_ok=True)
    Path(args.output).write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
