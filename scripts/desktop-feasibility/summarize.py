#!/usr/bin/env python3
"""Reduce private feasibility evidence into closed, experiment-only facts."""
import argparse
import hashlib
import json
import sys
from pathlib import Path



sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'canary/actions'))
from desktop_diagnostics import REASONS

ROLES = set("unknown window application button check_box radio_button text_field text_area static_text combo_box list list_item menu menu_item menu_bar tab tab_group table table_row table_cell toolbar scroll_bar slider image link group dialog alert progress_bar tree_item web_area heading separator split_group switch spin_button tooltip status navigation scroll_thumb".split())
AX_STAGES = set("initial-inventory trust-control agent-panel after-panel before-keyboard after-keyboard response completed".split())
READBACK = set("no-matching-control readable-empty-value readable-nonmatching-value value-read-unavailable query-failed value-matches".split())


def optional_count(value):
    return value is None or type(value) is int and 0 <= value <= 4096


def optional_reason(value):
    return value is None or type(value) is str and value in REASONS


def validate_ax(value):
    keys = set("schemaVersion mechanism experimentOnly noOcrQualification appByPid appError stage trustControlCount trustControlError blocker keyboardEntered semanticInputVerified semanticResponseVerified providerResponseVerified inventories".split())
    if type(value) is not dict or set(value) != keys or type(value['schemaVersion']) is not int or value['schemaVersion'] != 1 or value['mechanism'] != 'zed-native-accessibility':
        raise ValueError('invalid accessibility facts')
    flags = {'experimentOnly', 'noOcrQualification', 'appByPid', 'keyboardEntered', 'semanticInputVerified', 'semanticResponseVerified', 'providerResponseVerified'}
    if any(type(value[key]) is not bool for key in flags) or not value['experimentOnly'] or value['noOcrQualification']:
        raise ValueError('invalid accessibility qualification')
    if value['stage'] not in AX_STAGES or not optional_count(value['trustControlCount']):
        raise ValueError('invalid accessibility stage')
    if any(not optional_reason(value[key]) for key in ('appError', 'trustControlError', 'blocker')):
        raise ValueError('invalid accessibility reason')
    inventories = value['inventories']
    if type(inventories) is not list or len(inventories) > 5:
        raise ValueError('invalid accessibility inventory bound')
    fields = set("stage roleCounts rolesError namedComposerCount namedComposerError editableCount editableError valueReadback responseMatches responseError".split())
    for observation in inventories:
        if type(observation) is not dict or set(observation) != fields or observation['stage'] not in AX_STAGES:
            raise ValueError('invalid accessibility inventory')
        if observation['valueReadback'] not in READBACK:
            raise ValueError('invalid accessibility readback')
        if any(not optional_count(observation[key]) for key in ('namedComposerCount', 'editableCount', 'responseMatches')):
            raise ValueError('invalid accessibility count')
        if any(not optional_reason(observation[key]) for key in ('rolesError', 'namedComposerError', 'editableError', 'responseError')):
            raise ValueError('invalid accessibility query error')
        roles = observation['roleCounts']
        if roles is not None and (type(roles) is not dict or not set(roles) <= ROLES
                                  or any(type(count) is not int or not 1 <= count <= 4096 for count in roles.values())
                                  or sum(roles.values()) > 4096):
            raise ValueError('invalid accessibility role aggregate')
    return value


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
    parser.add_argument('--prepared')
    args = parser.parse_args()
    report = json.loads(Path(args.report).read_text()) if Path(args.report).exists() else {}
    manifest = json.loads(Path(args.frozen).read_text()) if Path(args.frozen).exists() else {}
    entry = next((a for a in manifest.get('apps', []) if a.get('app') == args.app), {})
    result = dict(schemaVersion=1, experimentOnly=True, sourceCommit=args.source_sha,
                  platform=args.platform, app=args.app,
                  appVersion=entry.get('version'), runtimeVersion=entry.get('runtimeVersion'),
                  upstreamRevision=entry.get('revision'),
                  realNanhSha256=hashlib.sha256(Path(args.real_nanh).read_bytes()).hexdigest() if Path(args.real_nanh).exists() else None)
    prepared = json.loads(Path(args.prepared).read_text()) if args.prepared and Path(args.prepared).exists() else {}
    prepared_app = next((app for app in prepared.get('apps', []) if app.get('app') == args.app), {})
    result['applicationSha256'] = (prepared_app.get('executable') or {}).get('sha256')
    result['observedRuntimeVersion'] = prepared_app.get('runtimeVersion')
    result['observedAppVersion'] = prepared_app.get('appVersion')
    result['frozenArtifactDigest'] = entry.get('digest')
    result['checkerSha256'] = hashlib.sha256(Path(args.checker).read_bytes()).hexdigest() if Path(args.checker).exists() else None
    result['reportSha256'] = hashlib.sha256(Path(args.report).read_bytes()).hexdigest() if Path(args.report).exists() else None
    app_result = next((app for app in report.get('results', []) if app.get('app') == args.app), {})
    fields = {'status', 'reason', 'steps', 'inputMode', 'responseVerification', 'guiStage'}
    result['probes'] = [{key: value for key, value in probe.items() if key in fields}
                        for probe in app_result.get('deterministic', [])]
    result['appCleanup'] = app_result.get('cleanup')
    result['reportCleanup'] = report.get('cleanup')
    result['noOcrQualification'] = False
    result['stage'] = ('measured' if any(probe.get('steps') for probe in result['probes'])
                       else 'blocked-before-ui' if report else 'preparation-or-report-missing')
    if args.app == 'zed-desktop':
        result['semanticConversationReadback'] = bool(semantic_zed(report) and entry.get('version') and result['checkerSha256'] and len(args.source_sha) == 40)
        result['verdict'] = 'conversation-readback-viable' if result['semanticConversationReadback'] else 'inconclusive'
        result['accessibilityInventories'] = [validate_ax(json.loads(path.read_text()))
                                            for path in sorted(Path(args.facts).glob('*.json'))] if args.facts else []
        if result['accessibilityInventories']:
            result['accessibilityObservationCount'] = len(result['accessibilityInventories'])
            result['expectedAccessibilityObservationCount'] = 3
            result['semanticConversationReadback'] = (len(result['accessibilityInventories']) == 3
                                                      and all(value['semanticInputVerified'] and value['semanticResponseVerified']
                                                              for value in result['accessibilityInventories'])
                                                      and result['appCleanup'] == 'passed' and result['reportCleanup'] == 'passed')
            result['verdict'] = 'semantic-readback-observed' if result['semanticConversationReadback'] else 'inconclusive'

    else:
        observations = []
        for path in Path(args.facts).glob('*.json'):
            value = json.loads(path.read_text())
            keys = {'schemaVersion', 'mechanism', 'endpointOwned', 'attached', 'uniqueComposer', 'inputReadback', 'syntheticTextPresent'}
            if set(value) != keys or type(value['schemaVersion']) is not int or value['schemaVersion'] != 1 or value['mechanism'] != 'hermes-cdp':
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
