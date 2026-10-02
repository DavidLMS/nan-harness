#!/usr/bin/env python3
"""Closed branch-only desktop qualification matrix and report reducer."""
import argparse
import hashlib
import json
from pathlib import Path
import re

from desktop_diagnostics import CATEGORIES, REASONS, validate_bundle
from desktop_startup import startup
from desktop_suite import read_frozen_manifest, validated_report

APPS = ('zed-desktop', 'chatgpt-desktop', 'claude-desktop', 'hermes-desktop', 'pen-desktop')
TARGETS = (('linux', 'x86_64', 'ubuntu-24.04'), ('macos', 'aarch64', 'macos-15'),
           ('windows', 'x86_64', 'windows-2025'))
BACKENDS = {('zed-desktop', 'macos', 'aarch64'): 'native-thread-export',
            ('zed-desktop', 'linux', 'x86_64'): 'native-thread-export',
            ('zed-desktop', 'windows', 'x86_64'): 'native-thread-export',
            ('hermes-desktop', 'linux', 'x86_64'): 'renderer-dom',
            ('hermes-desktop', 'macos', 'aarch64'): 'renderer-dom',
            ('hermes-desktop', 'windows', 'x86_64'): 'renderer-dom'}
BACKENDS.update({(app, platform, architecture): 'renderer-inventory'
                 for app in ('chatgpt-desktop', 'claude-desktop', 'pen-desktop')
                 for platform, architecture, _ in TARGETS})
STEPS = {'launched', 'input-submitted', 'response-verified', 'tool-verified', 'error-recovered'}
COMMIT = re.compile(r'[0-9a-f]{40}\Z')
HASH = re.compile(r'[0-9a-f]{64}\Z')
VERSION = re.compile(r'[0-9]+(?:\.[0-9]+){2}(?:[-+][A-Za-z0-9.-]+)?\Z')


def public_onboarding(setup, app):
    booleans = {'conversationalScope', 'engineeringControl', 'roleClickAttempted',
                'roleClickCompleted', 'engineeringChecked', 'continueControl',
                'continueClickAttempted', 'continueClickCompleted', 'roleScopeAbsent'}
    fields = booleans | {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'stage', 'errorCategory', 'roleProofFailure', 'sessionProofFailure'}
    failures = {'unmeasured', 'deadline-or-ownership', 'legend-count', 'group-absent', 'scope-count',
                'fieldset-count', 'login-present', 'engineering-count', 'engineering-disabled',
                'label-count', 'label-association', 'checked-mismatch', 'final-ownership'}
    sessions = {'unmeasured', 'guard-missing', 'deadline-invalid', 'deadline-expired', 'platform', 'host-policy', 'onboarding-policy'}
    stages = {'session', 'role-proof', 'role-action', 'role-readback', 'continue-action',
              'scope-transition', 'stopped-after-role'}
    errors = {None, 'invalid-session', 'scope-not-matched', 'role-already-selected',
              'action-blocked', 'role-readback-failed', 'continue-not-matched',
              'ownership-lost', 'scope-remained', 'action-uncertain', 'observation-failed'}
    if (app != 'chatgpt-desktop' or type(setup) is not dict or set(setup) != fields
            or type(setup['schemaVersion']) is not int or setup['schemaVersion'] != 1
            or setup['mechanism'] != 'codex-public-onboarding' or setup['diagnosticsOnly'] is not True
            or type(setup['stage']) is not str or setup['stage'] not in stages
            or (setup['errorCategory'] is not None and type(setup['errorCategory']) is not str)
            or setup['errorCategory'] not in errors
            or type(setup['roleProofFailure']) is not str or setup['roleProofFailure'] not in failures
            or type(setup['sessionProofFailure']) is not str or setup['sessionProofFailure'] not in sessions
            or any(type(setup[key]) is not bool for key in booleans)):
        raise ValueError('invalid public onboarding diagnostic')
    return setup


def matrix():
    return {'include': [dict(app=app, platform=platform, architecture=architecture,
                             runner=runner, backend=BACKENDS.get((app, platform, architecture), 'pending'))
                        for platform, architecture, runner in TARGETS for app in APPS]}


def cell(app, platform, architecture):
    matches = [item for item in matrix()['include']
               if (item['app'], item['platform'], item['architecture']) == (app, platform, architecture)]
    if len(matches) != 1:
        raise ValueError('target is outside the initial matrix')
    return matches[0]


def digest(path):
    path = Path(path)
    if path.is_symlink() or not path.is_file():
        raise ValueError('identity requires a regular file')
    value = hashlib.sha256()
    with path.open('rb') as source:
        for block in iter(lambda: source.read(1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def envelope(app, platform, architecture, source_sha):
    selected = cell(app, platform, architecture)
    if not COMMIT.fullmatch(source_sha):
        raise ValueError('source identity is invalid')
    return dict(schemaVersion=1, suite='desktop-qualification', source='branch', sourceSha=source_sha,
                evidenceMode='deterministic', app=app, platform=platform, architecture=architecture,
                backend=selected['backend'], qualification='unqualified', outcome='blocked',
                reason='pending-backend' if selected['backend'] == 'pending' else 'not-run', checkerSha256=None, launcherSha256=None,
                realNanhSha256=None, frozenManifestSha256=None, preparedSha256=None,
                reportSha256=None, applicationSha256=None, upstreamRevision=None,
                upstreamArtifactSha256=None, appVersion=None, runtimeVersion=None,
                appCleanup=None, globalCleanup=None, probes=[], semanticObservations=[], nativeDiagnostics=[])


def bounded_json(path, limit=1024 * 1024):
    path = Path(path)
    if path.is_symlink() or not path.is_file():
        raise ValueError('evidence must be a regular file')
    with path.open('rb') as source:
        raw = source.read(limit + 1)
    if len(raw) > limit:
        raise ValueError('evidence exceeds its bound')
    return json.loads(raw)


DOM_ERRORS = set('unclassified invalid-request launcher-unowned endpoint-unowned target-ambiguous target-invalid composer-ambiguous send-unavailable stale-response input-mismatch response-timeout submit-action-timeout submit-action-intercepted submit-action-detached submit-action-failed response-observation-failed attachment-or-action-failed'.split())
NATIVE_STAGES = set('trust panel input submit response-control response-readback completed'.split())
NATIVE_SUBSTAGES = set('trust-query trust-before trust-after panel-query new-thread-before new-thread-after panel-settle select-all-before select-all-after type-before type-after paste-before paste-after paste-settle input-sentinel-write copy-select-all-before copy-select-all-after input-copy-before input-copy-after collapse-selection-before submit-before response-control-query response-sentinel-write response-copy-before response-copy-after clipboard-read-before clipboard-read-after export-copy-before export-copy-after export-read-before export-read-after export-parse completed retry-control-query retry-before retry-after retry-title-query retry-tooltip-reset retry-tooltip-hover retry-tooltip-query retry-tooltip-clear retry-revalidate retry-label-query retry-label-parent activation-before activation-after retry-inventory-before retry-inventory-after icon-baseline-before icon-baseline-after icon-observation-before icon-observation-settle icon-observation-after icon-observation-completed'.split())
DOM_TAGS = {'html', 'body', 'button', 'div', 'span', 'svg', 'other', 'none', 'unmeasured'}
DOM_REGIONS = {'thread-viewport', 'composer-root', 'composer-dock', 'composer-drag-region', 'composer-bounds', 'composer-portal', 'particle-field', 'chat-drop-overlay', 'titlebar-drag', 'pane-overlay', 'pane-host', 'narrow-overlay', 'floating-pane', 'tree-group', 'panel-header', 'panel-page-header', 'zone-tabstrip', 'window-drag-handle', 'gateway-connecting', 'onboarding', 'command-backdrop', 'dialog-overlay', 'dialog', 'popover', 'tooltip', 'other', 'none', 'unmeasured'}


def semantic_observations(directory, app):
    if directory is None:
        return []
    directory = Path(directory)
    if directory.is_symlink() or not directory.is_dir():
        raise ValueError('semantic facts directory is invalid')
    all_paths = sorted(directory.glob('*.json'))
    private = [path for path in all_paths if path.name.startswith(('connection-', 'startup-', 'closed-startup-'))]
    # Three fresh profiles produce connection/startup metadata. Bound those
    # separately so they cannot consume or bypass the closed-record budget.
    if len(private) > 12:
        raise ValueError('too many private semantic metadata files')
    paths = [path for path in all_paths if path.name != 'native-diagnostics.json'
             and not path.name.startswith(('connection-', 'startup-', 'closed-startup-'))]
    # Three complete renderer probes also retain policy, provider, frontend,
    # backend and fresh Windows ownership receipts. These can exceed 32 even
    # when every probe succeeds; each record still has its own closed schema
    # and 8 KiB bound, and private connection metadata remains separate.
    if len(paths) > 64:
        raise ValueError('too many semantic observations')
    startup_paths = [path for path in private if path.name.startswith('closed-startup-')]
    if len(startup_paths) > 3:
        raise ValueError('too many startup observations')
    observations = []
    for path in startup_paths:
        value = bounded_json(path, 8192)
        if isinstance(value, dict) and value.get('mechanism') == 'hermes-startup':
            if app != 'hermes-desktop':
                raise ValueError('startup application differs')
            observations.append(startup(value))
    def flag(record, source, key, output=None):
        if key in source:
            if type(source[key]) is not bool:
                raise ValueError('invalid semantic flag')
            record[output or key] = source[key]
    def enum(record, source, key, allowed):
        if key in source:
            value = source[key]
            if value is not None and (type(value) is not str or value not in allowed):
                raise ValueError('invalid semantic enum')
            record[key] = value
    for path in paths:
        value = bounded_json(path, 8192)
        if type(value) is not dict:
            raise ValueError('invalid semantic observation')
        mechanism = value.get('mechanism')
        if mechanism not in {'hermes-windows-catalog-readiness', 'hermes-renderer-qualification', 'zed-native-copy', 'semantic-provider-oracle', 'semantic-failure-policy', 'hermes-retry-policy', 'semantic-inventory', 'zed-native-icons', 'hermes-front-source', 'hermes-backend-failure', 'hermes-policy-preparation', 'zed-pointer-transport', 'zed-pointer-observation', 'zed-clipboard-transport', 'windows-endpoint-proof', 'renderer-inventory', 'native-window-stability', 'renderer-startup', 'renderer-startup-baseline', 'codex-owned-relaunch', 'claude-owned-configuration'}:
            continue
        expected = 'hermes-renderer-qualification' if app == 'hermes-desktop' else 'zed-native-copy'
        if (mechanism != expected and mechanism not in {'hermes-windows-catalog-readiness', 'semantic-provider-oracle', 'semantic-failure-policy', 'hermes-retry-policy', 'semantic-inventory', 'zed-native-icons', 'hermes-front-source', 'hermes-backend-failure', 'hermes-policy-preparation', 'zed-pointer-transport', 'zed-pointer-observation', 'zed-clipboard-transport', 'windows-endpoint-proof', 'renderer-inventory', 'native-window-stability', 'renderer-startup', 'renderer-startup-baseline', 'codex-owned-relaunch', 'claude-owned-configuration'}) or type(value.get('schemaVersion')) is not int or value['schemaVersion'] != 1:
            raise ValueError('semantic observation identity differs')
        record = {'schemaVersion': 1, 'mechanism': mechanism}
        if mechanism == 'hermes-windows-catalog-readiness':
            booleans = {'menuOpened', 'refreshAttempted', 'catalogVerified', 'modelRowVerified',
                        'menuDismissed', 'composerReverified'}
            fields = booleans | {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'stage', 'errorCategory'}
            stages = {'policy', 'menu', 'refresh', 'catalog', 'dismiss', 'composer', 'ready'}
            errors = {None, 'policy-rejected', 'menu-unavailable', 'refresh-uncertain',
                      'catalog-unavailable', 'dismiss-uncertain', 'composer-changed', 'composer-unavailable'}
            if (app != 'hermes-desktop' or set(value) != fields or value['diagnosticsOnly'] is not True
                    or type(value['stage']) is not str or value['stage'] not in stages
                    or (value['errorCategory'] is not None and type(value['errorCategory']) is not str)
                    or value['errorCategory'] not in errors
                    or any(type(value[key]) is not bool for key in booleans)
                    or (value['stage'] == 'ready' and (value['errorCategory'] is not None
                        or not all(value[key] for key in booleans)))
                    or (value['stage'] != 'ready' and value['errorCategory'] is None)):
                raise ValueError('invalid Hermes catalog readiness diagnostic')
            record.update({key: value[key] for key in fields - {'schemaVersion', 'mechanism'}})
        elif mechanism == 'semantic-failure-policy':
            if set(value) != set('schemaVersion mechanism failureStatus recoveryAction'.split()) or type(value['failureStatus']) is not int or value['failureStatus'] not in {400, 503} or value['recoveryAction'] != 'explicit-ui-retry':
                raise ValueError('invalid semantic failure policy')
            record.update(failureStatus=value['failureStatus'], recoveryAction=value['recoveryAction'])
        elif mechanism == 'codex-owned-relaunch':
            if (app != 'chatgpt-desktop' or set(value) != {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'stage'}
                    or value['diagnosticsOnly'] is not True
                    or type(value['stage']) is not str
                    or value['stage'] not in {'armed', 'restarted', 'no-request', 'invalid-request', 'executable-changed', 'child-exited', 'startup-timeout', 'bridge-stopped', 'cancelled'}):
                raise ValueError('invalid Codex relaunch observation')
            record.update(diagnosticsOnly=True, stage=value['stage'])
        elif mechanism == 'windows-endpoint-proof':
            if set(value) != set('schemaVersion mechanism diagnosticsOnly category'.split()) or value['diagnosticsOnly'] is not True:
                raise ValueError('invalid Windows endpoint proof identity')
            allowed = {'owned', 'process-budget', 'ancestry-cycle', 'process-unavailable', 'parent-unavailable', 'parent-reused', 'session-mismatch', 'ancestry-limit', 'listener-unavailable', 'query-failed', 'transport-timeout', 'transport-failed', 'unclassified'}
            if type(value['category']) is not str or value['category'] not in allowed:
                raise ValueError('invalid Windows endpoint proof category')
            record.update(diagnosticsOnly=True, category=value['category'])
        elif mechanism == 'claude-owned-configuration':
            flags = set('configurationPresent objectSchema deploymentModeMatches profileMatches providerGateway loopbackBaseUrlMatches authMatches modelDiscoveryEnabled chatEnabled chooserDisabled'.split())
            nullable = {'nativePathAlignment', 'configurationConsumed', 'modelDiscoverySeen'}
            if (set(value) != flags | nullable | {'schemaVersion', 'mechanism', 'diagnosticsOnly'}
                    or app != 'claude-desktop' or value['diagnosticsOnly'] is not True):
                raise ValueError('invalid Claude configuration observation identity')
            for key in flags | nullable:
                if type(value[key]) is not bool and not (key in nullable and value[key] is None):
                    raise ValueError('invalid Claude configuration observation flag')
                record[key] = value[key]
            record['diagnosticsOnly'] = True
        elif mechanism == 'zed-pointer-observation':
            flags = set('maximizedHorizontal maximizedVertical enabled sensitive showing visible defunct retryContains'.split())
            if (set(value) != flags | {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'pointerTarget', 'pointerChild'}
                    or app != 'zed-desktop' or value['diagnosticsOnly'] is not True):
                raise ValueError('invalid Zed pointer observation identity')
            for key in flags:
                if value[key] is not None and type(value[key]) is not bool:
                    raise ValueError('invalid Zed pointer observation flag')
                record[key] = value[key]
            if type(value['pointerTarget']) is not str or value['pointerTarget'] not in {'unavailable', 'client', 'owned-frame', 'client-descendant', 'foreign'}:
                raise ValueError('invalid Zed pointer target')
            if type(value['pointerChild']) is not str or value['pointerChild'] not in {'unavailable', 'client', 'client-descendant', 'decoration-or-empty', 'other'}:
                raise ValueError('invalid Zed pointer child')
            record.update(diagnosticsOnly=True, pointerTarget=value['pointerTarget'], pointerChild=value['pointerChild'])
        elif mechanism == 'zed-pointer-transport':
            if set(value) != set('schemaVersion mechanism diagnosticsOnly stage'.split()) or app != 'zed-desktop' or value['diagnosticsOnly'] is not True:
                raise ValueError('invalid Zed pointer transport identity')
            allowed = {'dispatched', 'invalid-request', 'foreground-mismatch', 'process-mismatch', 'initial-query-failed', 'movement-failed', 'final-query-failed', 'activation-failed', 'coordinate-unavailable', 'transport-failed'}
            if type(value['stage']) is not str or value['stage'] not in allowed:
                raise ValueError('invalid Zed pointer transport stage')
            record.update(diagnosticsOnly=True, stage=value['stage'])
        elif mechanism == 'zed-clipboard-transport':
            fields = {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'operation', 'stage', 'elapsed'}
            if set(value) != fields or app != 'zed-desktop' or value['diagnosticsOnly'] is not True:
                raise ValueError('invalid Zed clipboard transport identity')
            stages = {'executable', 'spawn', 'pipe', 'write', 'read', 'wait', 'wait-timeout', 'nonzero', 'decode', 'thread', 'invalid-input', 'output-invalid', 'clipboard-api'}
            for key, allowed in [('operation', {'read', 'write', 'clear'}), ('stage', stages),
                                 ('elapsed', {'under-1s', '1-to-3s', 'at-least-3s'})]:
                if type(value[key]) is not str or value[key] not in allowed:
                    raise ValueError('invalid Zed clipboard transport category')
                record[key] = value[key]
            record['diagnosticsOnly'] = True
        elif mechanism == 'hermes-policy-preparation':
            if set(value) != set('schemaVersion mechanism diagnosticsOnly stage'.split()) or app != 'hermes-desktop' or value['diagnosticsOnly'] is not True:
                raise ValueError('invalid Hermes policy preparation identity')
            stages = {'ownership', 'facts-directory', 'config-read', 'config-shape', 'ownership-recheck', 'config-replace', 'policy-receipt'}
            if type(value['stage']) is not str or value['stage'] not in stages:
                raise ValueError('invalid Hermes policy preparation stage')
            record.update(diagnosticsOnly=True, stage=value['stage'])
        elif mechanism == 'hermes-backend-failure':
            if set(value) != set('schemaVersion mechanism diagnosticsOnly category assistantTurnCount'.split()) or app != 'hermes-desktop' or value['diagnosticsOnly'] is not True:
                raise ValueError('invalid Hermes backend failure identity')
            allowed = {'python-import-failure', 'provider-unconfigured', 'backend-unavailable', 'invalid-model', 'permission-denied', 'connection-failed', 'multiple', 'unclassified'}
            if type(value['category']) is not str or value['category'] not in allowed or type(value['assistantTurnCount']) is not int or not 0 <= value['assistantTurnCount'] <= 4096:
                raise ValueError('invalid Hermes backend failure facts')
            record.update(diagnosticsOnly=True, category=value['category'], assistantTurnCount=value['assistantTurnCount'])
        elif mechanism == 'renderer-startup-baseline':
            if set(value) - {'accessibilityInventory'} != set('schemaVersion mechanism diagnosticsOnly windowAcquired rendererInstrumented'.split()) or value['diagnosticsOnly'] is not True or value['windowAcquired'] is not True or value['rendererInstrumented'] is not False:
                raise ValueError('invalid renderer startup baseline')
            record.update(diagnosticsOnly=True, windowAcquired=True, rendererInstrumented=False)
            if 'accessibilityInventory' in value:
                inventory = value['accessibilityInventory']
                fields = {'appPresent', 'editableCount', 'retryCount', 'loginCount'}
                if type(inventory) is not dict or set(inventory) != fields or type(inventory['appPresent']) is not bool:
                    raise ValueError('invalid native accessibility inventory')
                for key in fields - {'appPresent'}:
                    if inventory[key] is not None and (type(inventory[key]) is not int or not 0 <= inventory[key] <= 4096):
                        raise ValueError('invalid native accessibility count')
                record['accessibilityInventory'] = inventory

        elif mechanism == 'renderer-startup':
            if set(value) != set('schemaVersion mechanism diagnosticsOnly app exitCode stderrPresent captureTruncated startupCategory'.split()) or value['diagnosticsOnly'] is not True or value['app'] != app:
                raise ValueError('invalid renderer startup identity')
            code = value['exitCode']
            if code is not None and (type(code) is not int or not -(2**31) <= code < 2**31):
                raise ValueError('invalid renderer exit code')
            for key in ('stderrPresent', 'captureTruncated'):
                flag(record, value, key)
            enum(record, value, 'startupCategory', {'unclassified', 'no-usable-sandbox', 'missing-shared-library', 'display-unavailable', 'debugging-configuration', 'missing-runtime-module', 'runtime-exception', 'permission-denied'})
            record.update(app=app, diagnosticsOnly=True, exitCode=code)
        elif mechanism == 'native-window-stability':
            if set(value) != {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'counts'} or value['diagnosticsOnly'] is not True:
                raise ValueError('invalid window stability identity')
            counts = value['counts']
            keys = set('observations candidatesPresent candidatesAbsent identityChanges boundsChanges nameChanges stablePairs'.split())
            if type(counts) is not dict or set(counts) - {'lastWindowState'} != keys:
                raise ValueError('invalid window stability fields')
            if 'lastWindowState' in counts and counts['lastWindowState'] not in {'visible', 'gone', 'identity-changed', 'minimized', 'hidden', 'cloaked', 'child-window', 'process-name-unavailable', 'candidate-name-mismatch', 'candidate-too-small', 'query-unavailable'}:
                raise ValueError('invalid window visibility state')
            if any(type(counts[key]) is not int or not 0 <= counts[key] <= 512 for key in keys):
                raise ValueError('invalid window stability counts')
            if counts['candidatesPresent'] + counts['candidatesAbsent'] != counts['observations']:
                raise ValueError('inconsistent window stability counts')
            record.update(diagnosticsOnly=True, counts=counts)
        elif mechanism == 'renderer-inventory':
            fields = set('schemaVersion mechanism diagnosticsOnly app endpointOwned launcherOwned attached pageCount textareaCount editableCount sendCount retryCount newThreadCount loginCount dialogCount errorCategory'.split())
            if set(value) - {'documentState', 'startupScreen', 'landingCounts', 'onboardingCounts', 'publicOnboarding'} != fields or value['app'] != app or value['diagnosticsOnly'] is not True:
                raise ValueError('invalid renderer inventory identity')
            for key in ('endpointOwned', 'launcherOwned', 'attached'):
                flag(record, value, key)
            if 'landingCounts' in value:
                counts = value['landingCounts']
                keys = {'importHeading', 'importDismiss', 'createProject', 'sourceFolders', 'projectName'}
                if (type(counts) is not dict or set(counts) != keys
                        or any(type(count) is not int or not 0 <= count <= 4096 for count in counts.values())):
                    raise ValueError('invalid renderer landing counts')
                record['landingCounts'] = counts
            if 'onboardingCounts' in value:
                counts = value['onboardingCounts']
                if (type(counts) is not dict
                        or set(counts) != {'roleRadios', 'roleLegend', 'workHeading', 'suggestionsCheckbox'}
                        or any(type(item) is not int or not 0 <= item <= 4096 for item in counts.values())):
                    raise ValueError('invalid renderer onboarding counts')
                record['onboardingCounts'] = counts
            if 'publicOnboarding' in value:
                record['publicOnboarding'] = public_onboarding(value['publicOnboarding'], app)
            if 'startupScreen' in value:
                if (type(value['startupScreen']) is not str or value['startupScreen'] not in {'unmeasured', 'gpu-unavailable', 'startup-failed', 'other', 'cli-connection-failed'}
                        or (value['startupScreen'] == 'cli-connection-failed' and app != 'chatgpt-desktop')):
                    raise ValueError('invalid renderer startup screen')
                record['startupScreen'] = value['startupScreen']
            for key in ('pageCount', 'textareaCount', 'editableCount', 'sendCount', 'retryCount', 'newThreadCount', 'loginCount', 'dialogCount'):
                count = value[key]
                if type(count) is not int or not 0 <= count <= 4096:
                    raise ValueError('invalid renderer inventory count')
                record[key] = count
            if 'documentState' in value:
                state = value['documentState']
                keys = set('readyState targetKind bodyPresent elementCount visibleElementCount inputCount frameCount pageErrorCount'.split())
                if type(state) is not dict or set(state) != keys:
                    raise ValueError('invalid renderer document fields')
                if state['readyState'] is None or state['targetKind'] is None:
                    raise ValueError('missing renderer document state')
                closed = {}
                enum(closed, state, 'readyState', {'unobserved', 'loading', 'interactive', 'complete'})
                enum(closed, state, 'targetKind', {'unobserved', 'blank', 'file', 'http', 'https', 'browser-error', 'app', 'other'})
                flag(closed, state, 'bodyPresent')
                for key in ('elementCount', 'visibleElementCount', 'inputCount', 'frameCount', 'pageErrorCount'):
                    if type(state[key]) is not int or not 0 <= state[key] <= 4096:
                        raise ValueError('invalid renderer document count')
                    closed[key] = state[key]
                record['documentState'] = closed
            enum(record, value, 'errorCategory', DOM_ERRORS)
            record.update(app=app, diagnosticsOnly=True)
        elif mechanism == 'semantic-inventory':
            for key in ('requestCount', 'toolCount', 'knownReadToolCount'):
                count = value.get(key)
                if type(count) is not int or not 0 <= count <= 4096:
                    raise ValueError('invalid semantic inventory count')
                record[key] = count
            if type(value.get('readToolSelected')) is not bool:
                raise ValueError('invalid semantic inventory flag')
            record['readToolSelected'] = value['readToolSelected']
        elif mechanism == 'zed-native-icons':
            if app != 'zed-desktop' or value.get('diagnosticsOnly') is not True:
                raise ValueError('invalid native icon diagnostic identity')
            record['diagnosticsOnly'] = True
            enum(record, value, 'stage', {'baseline', 'templates', 'first-capture', 'second-capture', 'matching', 'completed'})
            enum(record, value, 'status', {'complete', 'unsupported', 'query-error'})
            enum(record, value, 'reason', REASONS)
            side = value.get('templateSide')
            if type(side) is not int or side not in {0, 14, 28}:
                raise ValueError('invalid native icon template side')
            record['templateSide'] = side
            for key in ('retryMatches', 'copyMatches', 'closeMatches', 'baselineClusters',
                        'firstClusters', 'secondClusters', 'newStableClusters'):
                count = value.get(key)
                if count is not None and (type(count) is not int or not 0 <= count <= 4096):
                    raise ValueError('invalid native icon diagnostic count')
                record[key] = count
            calibration = value.get('calibration')
            if calibration is not None:
                if type(calibration) is not dict:
                    raise ValueError('invalid native icon calibration')
                safe = {}
                for key, maximum in (('scaleMilli', 2000), ('grayRange', 255), ('grayStdMilli', 127500)):
                    number = calibration.get(key)
                    if type(number) is not int or not 0 <= number <= maximum or (key == 'scaleMilli' and number not in {1000, 2000}):
                        raise ValueError('invalid native icon calibration number')
                    safe[key] = number
                for icon in ('retry', 'copy', 'close'):
                    source = calibration.get(icon)
                    if type(source) is not dict:
                        raise ValueError('invalid native icon calibration metrics')
                    metrics = {}
                    for key, maximum in (('contrastPositions', 4194304), ('foregroundPositions', 4194304),
                                         ('maxCorrelationMilli', 1000), ('maxContrastMilli', 255000), ('maxSpreadMilli', 255000)):
                        number = source.get(key)
                        if type(number) is not int or not 0 <= number <= maximum:
                            raise ValueError('invalid native icon calibration metric')
                        metrics[key] = number
                    safe[icon] = metrics
                record['calibration'] = safe
        elif mechanism == 'hermes-front-source':
            if app != 'hermes-desktop' or value.get('diagnosticsOnly') is not True:
                raise ValueError('invalid Hermes source diagnostic identity')
            levels = value.get('levels')
            if type(levels) is not list or not 1 <= len(levels) <= 4:
                raise ValueError('invalid source diagnostic levels')
            safe, total = [], 0
            for index, level in enumerate(levels):
                if type(level) is not dict or type(level.get('level')) is not int or level['level'] != index:
                    raise ValueError('invalid source diagnostic depth')
                hashes, count = level.get('tokenHashes'), level.get('tokenCount')
                if type(count) is not int or not 0 <= count <= 24 or type(hashes) is not list or len(hashes) != count:
                    raise ValueError('invalid source diagnostic count')
                if any(type(item) is not str or not HASH.fullmatch(item) for item in hashes) or len(set(hashes)) != len(hashes):
                    raise ValueError('invalid source diagnostic hashes')
                total += count
                safe.append(dict(level=index, tokenCount=count, tokenHashes=hashes))
            if total > 48:
                raise ValueError('source diagnostic exceeds token budget')
            record.update(diagnosticsOnly=True, levels=safe)
        elif mechanism == 'hermes-retry-policy':
            if app != 'hermes-desktop' or value.get('policy') != 'explicit-ui-retry' or type(value.get('autoRecoveryCycles')) is not int or value['autoRecoveryCycles'] != 0 or type(value.get('apiMaxRetries')) is not int or value['apiMaxRetries'] != 3:
                raise ValueError('invalid Hermes qualification policy')
            record.update(policy='explicit-ui-retry', autoRecoveryCycles=0, apiMaxRetries=3)
            for key in ('configBeforeSha256', 'configAfterSha256'):
                if type(value.get(key)) is not str or not HASH.fullmatch(value[key]):
                    raise ValueError('invalid policy config identity')
                record[key] = value[key]
        elif mechanism == 'semantic-provider-oracle':
            enum(record, value, 'stage', {'tool', 'failure'})
            for key in ('toolCompleted', 'toolRecordingBounded', 'toolVerified',
                        'fixtureResponseVerified', 'failureObserved'):
                flag(record, value, key)
        elif mechanism == 'hermes-renderer-qualification':
            for key in ('endpointOwned', 'attached', 'targetVerified', 'uniqueComposer', 'inputReadback',
                        'inputSubmitted', 'responseVerified', 'providerResponseVerified', 'errorObserved',
                        'retryControl', 'retryHitOwned', 'retryPointStable',
                        'retryHitAncestor', 'retryHitSharesTurnPair', 'retryHitContainsComposer', 'retryRectInViewport', 'retryAncestorClipped',
                        'retryPointerEventsNone', 'inputCleared', 'userTurnObserved',
                        'retryFocusAfterAcquire', 'retryFocusBeforeAction', 'retryButtonConnected',
                        'retryAncestorHidden', 'retryAncestorInert', 'retryFieldsetDisabled', 'retryDocumentFocused'):
                flag(record, value, key)
            for key in ('assistantTurnCount', 'requestFailedCount', 'apiErrorResponseCount', 'providerGenerationCount'):
                if key in value:
                    count = value[key]
                    if count is not None and (type(count) is not int or not 0 <= count <= 4096):
                        raise ValueError('invalid renderer diagnostic count')
                    record[key] = count
            enum(record, value, 'requestFailureCategory', {'aborted', 'connection', 'tls', 'other'})
            if 'apiErrorStatus' in value:
                status = value['apiErrorStatus']
                if status is not None and (type(status) is not int or not 400 <= status <= 599):
                    raise ValueError('invalid renderer HTTP status')
                record['apiErrorStatus'] = status
            enum(record, value, 'retryReveal', {'none', 'command-dismissed', 'onboarding-skipped'})
            enum(record, value, 'retrySampleStatus', {'unmeasured', 'native-control-invalid', 'detached', 'hidden', 'disabled', 'inert', 'foreign-document', 'clipped', 'pointer-events-none', 'transformed', 'outside-viewport', 'no-owned-point', 'owned'})
            if 'retryHitOwnedPoints' in value:
                count = value['retryHitOwnedPoints']
                if count is not None and (type(count) is not int or not 0 <= count <= 9):
                    raise ValueError('invalid retry hit count')
                record['retryHitOwnedPoints'] = count
            for key in ('retryHitTag', 'retryActiveTag'):
                enum(record, value, key, DOM_TAGS)
            for key in ('retryHitRegion', 'retryActiveRegion'):
                enum(record, value, key, DOM_REGIONS)
            enum(record, value, 'retryHitTarget', {'self', 'composer', 'error-card', 'menu', 'modal', 'other', 'none', 'unmeasured'})
            enum(record, value, 'errorCategory', DOM_ERRORS)
            enum(record, value, 'sendBlocker', {'modal', 'menu', 'tooltip', 'composer-drag-region', 'other',
                                              'unmeasured', 'focus', 'disabled', 'inert'})
            if 'observedRuntimeVersion' in value:
                version = value['observedRuntimeVersion']
                if version is not None and (type(version) is not str or not re.fullmatch(r'[0-9]+(?:\.[0-9]+){1,3}', version) or len(version) > 64):
                    raise ValueError('invalid semantic runtime version')
                record['observedRuntimeVersion'] = version
        else:
            for key in ('activationAttempted', 'activationSucceeded'):
                flag(record, value, key)
            enum(record, value, 'stage', NATIVE_STAGES)
            enum(record, value, 'substage', NATIVE_SUBSTAGES)
            enum(record, value, 'blocker', REASONS)
            enum(record, value, 'guardKind', {'native-window', 'direct-foreground'})
            enum(record, value, 'guardCategory', CATEGORIES)
            enum(record, value, 'clipboardCleanup', {'passed', 'failed', 'not-run'})
            enum(record, value, 'retryActionReceipt', {'acknowledged', 'completion-unknown', 'native-pointer-dispatched'})
            enum(record, value, 'retrySelector', {'retry-name-or-description', 'retry-tooltip', 'retry-label'})
            enum(record, value, 'retryInventoryStatus', {'complete', 'budget-exceeded', 'query-error'})
            for key in ('exportResumeCount', 'exportAgentCount', 'exportTotalAssistantTextCount', 'exportUserCount', 'exportAssistantTextCount', 'trustControlCount', 'panelControlCount', 'retryControlCount', 'retryTitleCount', 'retryCandidateCount', 'retryTooltipCount', 'retryLabelCount',
                        'retryInventoryTotal', 'retryInventoryButtons', 'retryInventoryStaticText',
                        'retryInventoryTitleMatches', 'retryInventoryGenerationMatches', 'retryInventoryRetryMatches'):
                if key in value:
                    count = value[key]
                    bound = 16384 if key == 'exportTotalAssistantTextCount' else 4096
                    if count is not None and (type(count) is not int or not 0 <= count <= bound):
                        raise ValueError('invalid retry control count')
                    record[key] = count
            enum(record, value, 'lastExportError', {'request', 'schema', 'user-mismatch', 'assistant-mismatch', 'decompression'})
            enum(record, value, 'lastExportTransportError', {'configuration', 'spawn', 'pipes', 'timeout', 'wait', 'write', 'read', 'exit', 'output-budget', 'json', 'verdict'})
            for nested, fields in (('input', {'submitted': 'inputSubmitted', 'clipboardVerified': 'inputReadback'}),
                                   ('response', {'clipboardVerified': 'responseVerified', 'providerVerified': 'providerResponseVerified'})):
                source = value.get(nested, {})
                if type(source) is not dict:
                    raise ValueError('invalid semantic flags')
                for key, output in fields.items():
                    flag(record, source, key, output)
        observations.append(record)
    return observations


def reduce_report(*, app, platform, architecture, source_sha, model, frozen, prepared,
                  checker, launcher, real_nanh, report, facts=None):
    result = envelope(app, platform, architecture, source_sha)
    result['semanticObservations'] = semantic_observations(facts, app)
    if facts is not None and (Path(facts) / 'native-diagnostics.json').exists():
        bundle = validate_bundle(Path(facts) / 'native-diagnostics.json', source_sha, platform)
        if any(event['record'].get('app') != app for event in bundle['events']):
            raise ValueError('native diagnostic application differs')
        result['nativeDiagnostics'] = bundle['events']
    if result['backend'] == 'pending':
        raise ValueError('native backend is not qualified for this cell')
    manifest = read_frozen_manifest(frozen, [app], platform, architecture, model)
    entry = manifest['apps'][0]
    if entry['status'] != 'frozen':
        result['reason'] = entry['reason']
        result['frozenManifestSha256'] = digest(frozen)
        return result
    result.update(checkerSha256=digest(checker), launcherSha256=digest(launcher),
                  realNanhSha256=digest(real_nanh), frozenManifestSha256=digest(frozen),
                  preparedSha256=digest(prepared), upstreamRevision=entry.get('revision'),
                  upstreamArtifactSha256=entry.get('digest', '').removeprefix('sha256:') or None)
    receipt = bounded_json(prepared)
    if (receipt.get('schemaVersion') != 2 or receipt.get('platform') != platform
            or receipt.get('architecture') != architecture
            or (receipt.get('checker') or {}).get('sha256') != result['checkerSha256']
            or (receipt.get('nanh') or {}).get('sha256') != result['launcherSha256']
            or (receipt.get('frozen') or {}).get('sha256') != result['frozenManifestSha256']
            or (receipt.get('frozen') or {}).get('model') != model):
        raise ValueError('prepared provenance differs')
    installed = [item for item in receipt.get('apps', []) if item.get('app') == app]
    if len(installed) != 1:
        raise ValueError('prepared app identity differs')
    executable = installed[0].get('executable') or {}
    application_hash = executable.get('sha256')
    if not isinstance(application_hash, str) or not HASH.fullmatch(application_hash):
        raise ValueError('installed application identity is missing')
    checked, report_digest = validated_report(Path(report), Path(checker))
    if (checked.get('schemaVersion') != 3 or checked.get('platform') != platform
            or checked.get('architecture') != architecture
            or (checked.get('model') is not None and checked.get('model') != model)
            or (checked.get('nanHarness') or {}).get('sha256') != result['launcherSha256']
            or len(checked.get('results', [])) != 1 or checked['results'][0].get('app') != app):
        raise ValueError('report identity differs from the cell')
    observed = checked['results'][0]
    if observed.get('appVersion') != entry['version']:
        raise ValueError('observed application version differs from frozen version')
    runtime = observed.get('runtimeVersion')
    if runtime is not None and (not isinstance(runtime, str) or not VERSION.fullmatch(runtime)):
        raise ValueError('runtime version is not closed')
    probes = observed.get('deterministic', [])
    public = [{key: probe[key] for key in ('status', 'reason', 'steps', 'inputMode', 'responseVerification')
               if key in probe} for probe in probes]
    result.update(reportSha256=report_digest, applicationSha256=application_hash,
                  appVersion=entry['version'], runtimeVersion=runtime,
                  appCleanup=observed.get('cleanup'), globalCleanup=checked.get('cleanup'), probes=public)
    # Semantic mechanisms are explicit, never inferred from visual/OCR success.
    semantic_pairs = {('native-clipboard-and-keyboard', 'native-thread-export')} if result['backend'] == 'native-thread-export' else {('renderer-dom-and-keyboard', 'renderer-dom')}
    policy_disclosed = app != 'hermes-desktop' or sum(
        observation['mechanism'] == 'hermes-retry-policy'
        for observation in result['semanticObservations']) == 3
    accepted = (result['backend'] != 'renderer-inventory' and policy_disclosed and len(probes) == 3 and result['appCleanup'] == 'passed' and result['globalCleanup'] == 'passed'
                and all(probe.get('status') == 'passed' and len(probe.get('steps', [])) == 5
                        and set(probe.get('steps', [])) == STEPS
                        and (probe.get('inputMode'), probe.get('responseVerification')) in semantic_pairs for probe in probes))
    result.update(outcome='passed' if accepted else 'blocked',
                  qualification='deterministic-full' if accepted else 'unqualified',
                  reason=None if accepted else 'incomplete-acceptance')
    return result


def aggregate(directory, source_sha):
    records = [bounded_json(path) for path in Path(directory).glob('**/qualification.json')]
    expected = {(item['app'], item['platform'], item['architecture']) for item in matrix()['include']}
    observed = [(item.get('app'), item.get('platform'), item.get('architecture')) for item in records]
    if len(observed) != 15 or len(set(observed)) != 15 or set(observed) != expected:
        raise ValueError('qualification matrix is incomplete or duplicated')
    for item in records:
        template = envelope(item['app'], item['platform'], item['architecture'], source_sha)
        if item.get('qualification') not in {'unqualified', 'deterministic-full'} or item.get('outcome') not in {'blocked', 'passed'}:
            raise ValueError('qualification outcome is invalid')
        if item['qualification'] == 'deterministic-full' and (item['outcome'] != 'passed'
                or item['backend'] == 'pending' or item['appCleanup'] != 'passed'
                or item['globalCleanup'] != 'passed' or len(item['probes']) != 3
                or any(not isinstance(item[key], str) or not HASH.fullmatch(item[key]) for key in
                       ('checkerSha256', 'launcherSha256', 'realNanhSha256', 'frozenManifestSha256',
                        'preparedSha256', 'reportSha256', 'applicationSha256'))):
            raise ValueError('qualification success is missing evidence')
        if set(item) != set(template) or any(item[key] != template[key] for key in
                ('schemaVersion', 'suite', 'source', 'sourceSha', 'evidenceMode', 'backend')):
            raise ValueError('qualification provenance differs')
    return dict(schemaVersion=1, sourceSha=source_sha, qualification='deterministic-full'
                if all(item['qualification'] == 'deterministic-full' and item['outcome'] == 'passed'
                       for item in records) else 'incomplete', cells=records)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=('matrix', 'pending', 'reduce', 'aggregate'))
    for name in ('app', 'platform', 'architecture', 'source-sha', 'output', 'model', 'frozen', 'prepared',
                 'checker', 'launcher', 'real-nanh', 'report', 'directory', 'facts'):
        parser.add_argument('--' + name)
    args = parser.parse_args()
    try:
        if args.command == 'matrix':
            print(json.dumps(matrix(), separators=(',', ':')))
            return
        required = (['directory', 'source_sha', 'output'] if args.command == 'aggregate' else
                    ['app', 'platform', 'architecture', 'source_sha', 'output'])
        if args.command == 'reduce':
            required += ['model', 'frozen', 'prepared', 'checker', 'launcher', 'real_nanh', 'report']
        if any(getattr(args, key) is None for key in required):
            raise ValueError('required cell evidence is missing')
        result = (aggregate(args.directory, args.source_sha) if args.command == 'aggregate' else
                  envelope(args.app, args.platform, args.architecture, args.source_sha)
                  if args.command == 'pending' else reduce_report(**{key: getattr(args, key) for key in required if key != 'output'}, facts=args.facts))
        destination = Path(args.output)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(json.dumps(result, sort_keys=True) + '\n')
        destination.chmod(0o600)
        if args.command == 'aggregate' and result['qualification'] != 'deterministic-full':
            raise SystemExit('desktop qualification matrix remains incomplete')
    except (OSError, ValueError, TypeError, KeyError) as error:
        categories = {
            'too many semantic observations': 'observation-budget',
            'too many private semantic metadata files': 'metadata-budget',
            'semantic observation identity differs': 'observation-identity',
            'runtime version is not closed': 'runtime-version',
            'prepared provenance differs': 'prepared-provenance',
        }
        category = categories.get(str(error), 'invalid-evidence')
        raise SystemExit('desktop qualification evidence rejected: ' + category) from None


if __name__ == '__main__':
    main()
