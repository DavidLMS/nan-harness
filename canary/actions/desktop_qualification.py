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
BACKENDS[('claude-desktop', 'macos', 'aarch64')] = 'native-assistant-clipboard'
BACKENDS[('chatgpt-desktop', 'windows', 'x86_64')] = 'renderer-dom'
BACKENDS[('chatgpt-desktop', 'linux', 'x86_64')] = 'renderer-dom'
BACKENDS[('chatgpt-desktop', 'macos', 'aarch64')] = 'renderer-dom'
RUNNER_FAILURES = set('windows-ownership-helper-missing windows-ownership-helper-invalid prepared-identity-mismatch prepared-app-unavailable prepared-executable-missing prepared-executable-changed host-platform-mismatch backend-unavailable frozen-app-unavailable report-absent claude-windows-executable-invalid claude-windows-bootstrap-invalid claude-windows-bootstrap-mismatch claude-windows-release-mismatch claude-windows-policy-invalid codex-project-release-mismatch invalid-preflight execution-failed'.split())
STEPS = {'launched', 'input-submitted', 'response-verified', 'tool-verified', 'error-recovered'}
COMMIT = re.compile(r'[0-9a-f]{40}\Z')
HASH = re.compile(r'[0-9a-f]{64}\Z')
VERSION = re.compile(r'[0-9]+(?:\.[0-9]+){2}(?:[-+][A-Za-z0-9.-]+)?\Z')


def main_aux_correlation(value, app):
    flags = set('heldMainUnchanged auxRouteMatched mainScopeUnique auxMainControlsAbsent auxComposerAbsent guarded'.split())
    focus = {'mainDocumentFocused', 'auxDocumentFocused'}
    fields = flags | focus | set('schemaVersion mechanism diagnosticsOnly status totalPages stableSamples main aux'.split())
    statuses = {'observed', 'initial-main-unavailable', 'page-count', 'source-scope',
                'identity-changed', 'ownership-lost', 'query-failed', 'deadline'}
    if (app != 'chatgpt-desktop' or type(value) is not dict or set(value) != fields
            or type(value['schemaVersion']) is not int or value['schemaVersion'] != 1
            or value['mechanism'] != 'codex-main-aux-correlation' or value['diagnosticsOnly'] is not True
            or type(value['status']) is not str or value['status'] not in statuses
            or any(type(value[key]) is not bool for key in flags)
            or any(value[key] is not None and type(value[key]) is not bool for key in focus)
            or value['totalPages'] is not None and (type(value['totalPages']) is not int or not 0 <= value['totalPages'] <= 32)
            or type(value['stableSamples']) is not int or not 0 <= value['stableSamples'] <= 2):
        raise ValueError('invalid Codex main auxiliary correlation')
    keys = set('roleLegend roleRadios engineering dialog quickChatComposer editable'.split())
    for key in ('main', 'aux'):
        counts = value[key]
        if counts is not None and (type(counts) is not dict or set(counts) != keys
                or any(type(count) is not int or not 0 <= count <= 4096 for count in counts.values())):
            raise ValueError('invalid Codex main auxiliary counts')
    main, aux = value['main'], value['aux']
    if (value['mainScopeUnique'] and (main is None or any(main[key] != count for key, count in
            {'roleLegend': 1, 'roleRadios': 11, 'engineering': 1}.items()))
            or value['auxMainControlsAbsent'] and (aux is None or any(aux[key] != 0 for key in
                ('roleLegend', 'roleRadios', 'engineering', 'dialog')))
            or value['auxComposerAbsent'] and (aux is None or aux['quickChatComposer'] != 0 or aux['editable'] != 0)):
        raise ValueError('inconsistent Codex main auxiliary counts')
    if value['status'] == 'observed' and not (value['totalPages'] == 2 and value['stableSamples'] == 2
            and all(value[key] for key in flags) and value['mainDocumentFocused'] is True
            and value['auxDocumentFocused'] is False and value['main'] is not None and value['aux'] is not None):
        raise ValueError('unproved Codex main auxiliary correlation')
    return value


def public_onboarding(setup, app):
    shape = set(setup) - {'rejectedPageInventory', 'taskScopeProved', 'taskClickAttempted', 'taskClickCompleted', 'codingComposerReady', 'mainGuardFailure', 'pageSetFailure', 'foreignOverlayImportSetup', 'foreignOverlaySourceCounts', 'foreignOverlayActionability'} if type(setup) is dict else set()
    booleans = {'conversationalScope', 'engineeringControl', 'roleClickAttempted',
                'roleClickCompleted', 'engineeringChecked', 'continueControl',
                'continueClickAttempted', 'continueClickCompleted', 'roleScopeAbsent'}
    fields = booleans | {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'stage', 'errorCategory', 'roleProofFailure', 'sessionProofFailure'}
    failures = {'unmeasured', 'deadline-or-ownership', 'deadline-expired', 'ownership-lost', 'page-count', 'page-changed', 'url-changed', 'query-failed', 'control-not-actionable', 'legend-count', 'group-absent', 'scope-count',
                'fieldset-count', 'login-present', 'engineering-count', 'engineering-disabled',
                'label-count', 'label-association', 'checked-mismatch', 'final-ownership'}
    sessions = {'unmeasured', 'guard-missing', 'deadline-invalid', 'deadline-expired', 'platform', 'host-policy', 'onboarding-policy'}
    stages = {'session', 'role-proof', 'role-action', 'role-readback', 'continue-action',
              'scope-transition', 'stopped-after-role', 'task-action', 'coding-readiness'}
    errors = {None, 'invalid-session', 'scope-not-matched', 'role-already-selected',
              'action-blocked', 'role-readback-failed', 'continue-not-matched',
              'ownership-lost', 'scope-remained', 'action-uncertain', 'observation-failed'}
    if (app != 'chatgpt-desktop' or type(setup) is not dict or shape not in (fields, fields | {'actionabilityFailure'}, fields | {'actionabilityFailure', 'foreignOverlay'}, fields | {'actionabilityFailure', 'foreignOverlay', 'foreignOverlayProof'}, fields | {'actionabilityFailure', 'foreignOverlay', 'foreignOverlayProof', 'foreignOverlaySurface', 'foreignOverlayFingerprint'}, fields | {'actionabilityFailure', 'foreignOverlay', 'foreignOverlayProof', 'foreignOverlaySurface', 'foreignOverlayFingerprint', 'foreignOverlayHeading'})
            or type(setup['schemaVersion']) is not int or setup['schemaVersion'] != 1
            or setup['mechanism'] != 'codex-public-onboarding' or setup['diagnosticsOnly'] is not True
            or type(setup['stage']) is not str or setup['stage'] not in stages
            or (setup['errorCategory'] is not None and type(setup['errorCategory']) is not str)
            or setup['errorCategory'] not in errors
            or type(setup['roleProofFailure']) is not str or setup['roleProofFailure'] not in failures
            or type(setup['sessionProofFailure']) is not str or setup['sessionProofFailure'] not in sessions
            or any(type(setup[key]) is not bool for key in booleans)):
        raise ValueError('invalid public onboarding diagnostic')
    if 'mainGuardFailure' in setup:
        failure = setup['mainGuardFailure']
        if (type(failure) is not str or failure not in {
                'deadline', 'native-ownership', 'page-set', 'main-identity', 'main-focus', 'main-scope',
                'auxiliary-route', 'auxiliary-identity', 'auxiliary-focus', 'auxiliary-controls',
                'query-failed', 'unmeasured'}):
            raise ValueError('invalid public onboarding main guard diagnostic')
    if 'pageSetFailure' in setup:
        details = setup['pageSetFailure']
        if (setup.get('mainGuardFailure') != 'page-set' or type(details) is not dict
                or set(details) != {'reason', 'initialCount', 'currentCount', 'heldPresent'}
                or type(details['reason']) is not str or details['reason'] not in {
                    'initial-count', 'held-main-missing', 'before-sample-changed', 'after-sample-changed'}
                or type(details['heldPresent']) is not bool
                or any(value is not None and (type(value) is not int or not 0 <= value <= 32)
                       for value in (details['initialCount'], details['currentCount']))):
            raise ValueError('invalid public onboarding page set diagnostic')
    if 'rejectedPageInventory' in setup:
        inventory = setup['rejectedPageInventory']
        counts = {'total', 'held', 'app', 'blank', 'devtools', 'other'}
        if (type(inventory) is not dict or type(inventory.get('status')) is not str
                or inventory['status'] not in {'complete', 'overflow'}
                or (inventory['status'] == 'overflow' and set(inventory) != {'status'})
                or (inventory['status'] == 'complete' and (
                    set(inventory) not in (counts | {'status'}, counts | {'status', 'source'})
                    or any(type(inventory[key]) is not int or not 0 <= inventory[key] <= 32 for key in counts)
                    or inventory['held'] > min(1, inventory['total']) or inventory['total'] == 1
                    or sum(inventory[key] for key in counts - {'total', 'held'}) != inventory['total']))):
            raise ValueError('invalid rejected renderer inventory')
        if 'source' in inventory:
            source = inventory['source']
            routes = {'avatarOverlay', 'hotkeyWindow', 'quickChat', 'quickChatPrewarm',
                      'detachedWindow', 'globalDictation', 'debug', 'unknown'}
            visibility = {'visible', 'hidden', 'unavailable'}
            if (inventory['status'] != 'complete' or type(source) is not dict
                    or type(source.get('status')) is not str or source['status'] not in {'complete', 'unavailable'}
                    or (source['status'] == 'unavailable' and set(source) != {'status'})
                    or (source['status'] == 'complete' and (
                        set(source) != {'status', 'routes', 'visibility'}
                        or any(type(source.get(key)) is not dict or set(source[key]) != fields
                               or any(type(count) is not int or not 0 <= count <= 32 for count in source[key].values())
                               or sum(source[key].values()) != inventory['total']
                               for key, fields in (('routes', routes), ('visibility', visibility)))))):
                raise ValueError('invalid rejected renderer source inventory')
    if 'taskScopeProved' in setup:
        if type(setup['taskScopeProved']) is not bool:
            raise ValueError('invalid Codex task scope proof')
        if setup['taskScopeProved'] and not setup['roleScopeAbsent']:
            raise ValueError('inconsistent Codex task scope proof')
    for key in ('taskClickAttempted', 'taskClickCompleted', 'codingComposerReady'):
        if key in setup and type(setup[key]) is not bool:
            raise ValueError('invalid Codex coding readiness flag')
    if (setup.get('taskClickCompleted') and not setup.get('taskClickAttempted')
            or setup.get('taskClickAttempted') and not all(setup.get(key) for key in
                ('roleClickCompleted', 'continueClickCompleted', 'taskScopeProved'))
            or setup.get('codingComposerReady') and setup['stage'] != 'coding-readiness'):
        raise ValueError('inconsistent Codex coding readiness')
    specific_overlay_failures = {
        'deadline-expired': {'deadline-expired'},
        'ownership-lost': {'ownership-lost', 'final-ownership', 'page-count', 'page-changed', 'url-changed', 'query-failed'},
        'role-proof-rejected': {'legend-count', 'group-absent', 'scope-count', 'fieldset-count', 'login-present',
                                'engineering-count', 'engineering-disabled', 'label-count', 'label-association', 'checked-mismatch'},
    }
    specific_overlay_rejection = (setup.get('actionabilityFailure') == 'foreign-overlay'
        and setup.get('foreignOverlay') == 'guard-rejected'
        and type(setup.get('foreignOverlayProof')) is str
        and setup.get('roleProofFailure') in specific_overlay_failures.get(setup.get('foreignOverlayProof'), set()))
    if 'actionabilityFailure' in setup:
        if (type(setup['actionabilityFailure']) is not str or setup['actionabilityFailure'] not in {
                'unsupported-control', 'detached-or-inert', 'ambiguous-overlays', 'foreign-overlay',
                'pointer-disabled', 'hidden', 'disabled', 'unstable', 'no-owned-point'}
                or (setup['roleProofFailure'] != 'control-not-actionable' and not specific_overlay_rejection)):
            raise ValueError('invalid public onboarding actionability')
    if 'foreignOverlay' in setup:
        if (setup.get('actionabilityFailure') != 'foreign-overlay'
                or type(setup['foreignOverlay']) is not str or setup['foreignOverlay'] not in {
                    'unmeasured', 'chatgpt-onboarding-complete', 'other', 'ambiguous', 'guard-rejected'}):
            raise ValueError('invalid public onboarding foreign overlay')
    if 'foreignOverlayProof' in setup:
        if (type(setup['foreignOverlayProof']) is not str or setup['foreignOverlayProof'] not in {
                'unmeasured', 'classified', 'deadline-expired', 'ownership-lost', 'role-proof-rejected',
                'control-replaced', 'frame-replaced', 'document-replaced', 'scope-missing',
                'role-group-changed', 'dialog-absent', 'dialog-replaced', 'unstable-classification', 'query-failed'}
                or (setup['foreignOverlayProof'] == 'classified') != (setup['foreignOverlay'] in {
                    'chatgpt-onboarding-complete', 'other', 'ambiguous'})):
            raise ValueError('invalid public onboarding overlay proof')
    if 'foreignOverlaySurface' in setup:
        if (setup['foreignOverlayProof'] != 'classified'
                or type(setup['foreignOverlaySurface']) is not str or setup['foreignOverlaySurface'] not in {
                    'unknown', 'enclosing-role-dialog', 'enclosing-role-alertdialog', 'enclosing-role-aria-modal',
                    'separate-dialog', 'separate-alertdialog', 'separate-menu'}
                or type(setup['foreignOverlayFingerprint']) is not str or setup['foreignOverlayFingerprint'] not in {
                    'not-applicable', 'heading-mismatch', 'form-mismatch', 'continue-mismatch', 'legal-links-mismatch', 'matched', 'computer-history-consent'}
                or (setup['foreignOverlayFingerprint'] != 'not-applicable' and setup['foreignOverlaySurface'] != 'separate-dialog')
                or (setup['foreignOverlayFingerprint'] == 'matched') != (setup['foreignOverlay'] == 'chatgpt-onboarding-complete')):
            raise ValueError('invalid public onboarding overlay surface')
    if 'foreignOverlayHeading' in setup:
        if (setup.get('foreignOverlayProof') != 'classified'
                or type(setup['foreignOverlayHeading']) is not str or setup['foreignOverlayHeading'] not in {
                    'all-set', 'external-import', 'skip-confirmation', 'imported-setup', 'computer-history-consent', 'project-import', 'unknown', 'ambiguous'}):
            raise ValueError('invalid public onboarding overlay heading')
    if 'foreignOverlayImportSetup' in setup:
        counts = setup['foreignOverlayImportSetup']
        if (setup.get('foreignOverlayProof') != 'classified' or 'foreignOverlayHeading' not in setup
                or type(counts) is not dict or set(counts) != {
                    'titleCount', 'continueCount', 'notNowCount', 'skipCount'}
                or any(type(count) is not int or not 0 <= count <= 32 for count in counts.values())
                or counts['titleCount'] == 0):
            raise ValueError('invalid imported setup dialog observation')
        paired = (counts['titleCount'] == 1 and counts['continueCount'] == 1
                  and (counts['notNowCount'], counts['skipCount']) in {(1, 0), (0, 1)})
        heading = setup['foreignOverlayHeading']
        if ((heading == 'imported-setup' and not paired)
                or paired and heading not in {'imported-setup', 'ambiguous'}):
            raise ValueError('inconsistent imported setup dialog observation')
    elif setup.get('foreignOverlayHeading') == 'imported-setup':
        raise ValueError('missing imported setup dialog observation')
    if 'foreignOverlayActionability' in setup:
        observation = setup['foreignOverlayActionability']
        bools = {'dialogOpacityZero', 'ancestorOpacityZero', 'dialogPointerEventsNone',
                 'ancestorPointerEventsNone', 'inert', 'stateClosed'}
        counts = {'targetOwnedPointCount', 'dialogOwnedPointCount', 'otherPointCount'}
        if (setup.get('foreignOverlayProof') != 'classified'
                or type(observation) is not dict or set(observation) - {'unavailableReason'} != bools | counts | {'status'}
                or type(observation['status']) is not str or observation['status'] not in {'observed', 'unavailable'}):
            raise ValueError('invalid passive overlay actionability observation')
        if 'unavailableReason' in observation:
            reason = observation['unavailableReason']
            reasons = {'ancestor-limit', 'ancestor-detached', 'opacity-invalid',
                       'pointer-property-invalid', 'geometry-invalid', 'geometry-outside'}
            if (observation['status'] == 'observed' and reason is not None
                    or observation['status'] == 'unavailable' and
                    (type(reason) is not str or reason not in reasons)):
                raise ValueError('invalid passive overlay unavailability reason')
        if observation['status'] == 'unavailable':
            if any(observation[key] is not None for key in bools | counts):
                raise ValueError('invalid unavailable overlay actionability observation')
        elif (any(type(observation[key]) is not bool for key in bools)
              or any(type(observation[key]) is not int or not 0 <= observation[key] <= 9 for key in counts)
              or sum(observation[key] for key in counts) != 9):
            raise ValueError('invalid overlay hit-point partition')
    if 'foreignOverlaySourceCounts' in setup:
        counts = setup['foreignOverlaySourceCounts']
        keys = {'computerHistoryTitleCount', 'computerHistoryFormCount', 'computerHistoryNotNowCount',
                'computerHistoryCustomizeCount', 'computerHistoryAllowCount', 'projectImportTitleCount',
                'projectImportContinueCount', 'projectImportNotNowCount'}
        if (setup.get('foreignOverlayProof') != 'classified' or setup.get('foreignOverlaySurface') != 'separate-dialog'
                or type(counts) is not dict or set(counts) != keys
                or any(type(count) is not int or not 0 <= count <= 32 for count in counts.values())):
            raise ValueError('invalid source dialog count diagnostic')
        if setup.get('foreignOverlayFingerprint') == 'computer-history-consent' and (
                setup.get('foreignOverlay') != 'other' or setup.get('foreignOverlayHeading') != 'computer-history-consent'
                or any(counts[key] != 1 for key in ('computerHistoryTitleCount', 'computerHistoryFormCount',
                                                  'computerHistoryNotNowCount', 'computerHistoryCustomizeCount'))
                or counts['computerHistoryAllowCount'] != 1):
            raise ValueError('inconsistent Computer History consent diagnostic')
    elif setup.get('foreignOverlayFingerprint') == 'computer-history-consent':
        raise ValueError('missing Computer History source counts')
    return setup


def matrix(excluded=()):
    if len(set(excluded)) != len(excluded) or not set(excluded) < set(APPS):
        raise ValueError('invalid qualification exclusions')
    return {'include': [dict(app=app, platform=platform, architecture=architecture,
                             runner=runner, backend=BACKENDS.get((app, platform, architecture), 'pending'))
                        for platform, architecture, runner in TARGETS for app in APPS if app not in excluded]}


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
                appCleanup=None, globalCleanup=None, probes=[], semanticObservations=[], nativeDiagnostics=[],
                nativeDiagnosticInvalidEvents=None)


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
NATIVE_SUBSTAGES = set('trust-query trust-before trust-after panel-query layout-zoom-before layout-zoom-after new-thread-before new-thread-after panel-settle select-all-before select-all-after type-before type-after paste-before paste-after paste-settle input-sentinel-write copy-select-all-before copy-select-all-after input-copy-before input-copy-after collapse-selection-before submit-before response-control-query response-sentinel-write response-copy-before response-copy-after clipboard-read-before clipboard-read-after export-copy-before export-copy-after export-read-before export-read-after export-parse completed retry-control-query retry-before retry-after retry-title-query retry-tooltip-reset retry-tooltip-hover retry-tooltip-query retry-tooltip-clear retry-revalidate retry-label-query retry-label-parent activation-before activation-after retry-inventory-before retry-inventory-after icon-baseline-before icon-baseline-after icon-observation-before icon-observation-settle icon-observation-after icon-observation-completed'.split())
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
        if mechanism not in {'codex-renderer-qualification', 'qualification-runner-failure', 'hermes-windows-catalog-readiness', 'hermes-renderer-qualification', 'zed-native-copy', 'semantic-provider-oracle', 'semantic-failure-policy', 'hermes-retry-policy', 'semantic-inventory', 'zed-native-icons', 'zed-retry-visual', 'hermes-front-source', 'hermes-backend-failure', 'hermes-policy-preparation', 'zed-atspi-retry', 'zed-pointer-transport', 'zed-pointer-observation', 'zed-clipboard-transport', 'windows-endpoint-proof', 'renderer-inventory', 'native-window-stability', 'renderer-startup', 'renderer-startup-baseline', 'codex-owned-relaunch', 'codex-restore', 'codex-project-preflight', 'windows-process-absence', 'windows-post-stop-process', 'windows-process-baseline', 'windows-process-settlement', 'windows-owned-stop', 'windows-process-correlation', 'windows-owned-descendant-cleanup', 'windows-owned-cleanup-preflight', 'claude-owned-configuration', 'claude-restore', 'claude-model-discovery', 'claude-window-stack', 'claude-window-focus', 'claude-chat-navigation', 'claude-native-chat', 'claude-window-fit', 'claude-windows-fit', 'claude-windows-uia', 'claude-windows-fit-rejection', 'claude-storage-use', 'claude-native-storage', 'claude-private-storage-stage', 'claude-native-composer', 'claude-linux-mode-roles', 'claude-native-root-preflight', 'zed-panel-zoom', 'zed-atspi-geometry'}:
            continue
        expected = 'hermes-renderer-qualification' if app == 'hermes-desktop' else 'zed-native-copy'
        if (mechanism != expected and mechanism not in {'codex-renderer-qualification', 'qualification-runner-failure', 'hermes-windows-catalog-readiness', 'semantic-provider-oracle', 'semantic-failure-policy', 'hermes-retry-policy', 'semantic-inventory', 'zed-native-icons', 'zed-retry-visual', 'hermes-front-source', 'hermes-backend-failure', 'hermes-policy-preparation', 'zed-atspi-retry', 'zed-pointer-transport', 'zed-pointer-observation', 'zed-clipboard-transport', 'windows-endpoint-proof', 'renderer-inventory', 'native-window-stability', 'renderer-startup', 'renderer-startup-baseline', 'codex-owned-relaunch', 'codex-restore', 'codex-project-preflight', 'windows-process-absence', 'windows-post-stop-process', 'windows-process-baseline', 'windows-process-settlement', 'windows-owned-stop', 'windows-process-correlation', 'windows-owned-descendant-cleanup', 'windows-owned-cleanup-preflight', 'claude-owned-configuration', 'claude-restore', 'claude-model-discovery', 'claude-window-stack', 'claude-window-focus', 'claude-chat-navigation', 'claude-native-chat', 'claude-window-fit', 'claude-windows-fit', 'claude-windows-uia', 'claude-windows-fit-rejection', 'claude-storage-use', 'claude-native-storage', 'claude-private-storage-stage', 'claude-native-composer', 'claude-linux-mode-roles', 'claude-native-root-preflight', 'zed-panel-zoom', 'zed-atspi-geometry'}) or type(value.get('schemaVersion')) is not int or value['schemaVersion'] != 1:
            raise ValueError('semantic observation identity differs')
        record = {'schemaVersion': 1, 'mechanism': mechanism}
        if mechanism == 'codex-project-preflight':
            fields = {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'stage'}
            if (app != 'chatgpt-desktop' or set(value) != fields or value['diagnosticsOnly'] is not True
                    or type(value['stage']) is not str or value['stage'] not in {
                        'policy', 'release', 'facts', 'electron', 'workspace', 'root-component',
                        'profile-binding', 'fixture', 'executable-hash'}):
                raise ValueError('invalid Codex project preflight')
            record.update(diagnosticsOnly=True, stage=value['stage'])
        elif mechanism == 'hermes-windows-catalog-readiness':
            booleans = {'menuOpened', 'refreshAttempted', 'catalogVerified', 'modelRowVerified',
                        'menuDismissed', 'composerReverified'}
            fields = booleans | {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'stage', 'errorCategory'}
            diagnostics = {'composerObservation', 'guardFailure'}
            optional = {'actionObservation', 'onboardingSkipped'}
            stages = {'policy', 'onboarding', 'menu', 'refresh', 'catalog', 'dismiss', 'composer', 'ready'}
            errors = {None, 'policy-rejected', 'onboarding-unavailable', 'menu-unavailable', 'refresh-uncertain',
                      'catalog-unavailable', 'dismiss-uncertain', 'composer-changed', 'composer-unavailable'}
            if (app != 'hermes-desktop' or (set(value) != fields and not (diagnostics <= set(value) and fields <= set(value) and set(value) <= fields | diagnostics | optional)) or value['diagnosticsOnly'] is not True
                    or type(value['stage']) is not str or value['stage'] not in stages
                    or (value['errorCategory'] is not None and type(value['errorCategory']) is not str)
                    or value['errorCategory'] not in errors
                    or any(type(value[key]) is not bool for key in booleans)
                    or (value['stage'] == 'ready' and (value['errorCategory'] is not None
                        or not all(value[key] for key in booleans)))
                    or (value['stage'] != 'ready' and value['errorCategory'] is None)):
                raise ValueError('invalid Hermes catalog readiness diagnostic')
            record.update({key: value[key] for key in fields - {'schemaVersion', 'mechanism'}})
            if diagnostics <= set(value):
                failure = value['guardFailure']
                failures = {None, 'unmeasured', 'deadline-expired', 'ownership-lost',
                            'page-count', 'url-changed', 'query-failed'}
                observation = value['composerObservation']
                if ((failure is not None and type(failure) is not str) or failure not in failures):
                    raise ValueError('invalid Hermes readiness guard observation')
                if observation is not None:
                    counts = {'roots', 'editors', 'expectedModelPills', 'modelPills', 'pickerButtons', 'switchButtons'}
                    if (type(observation) is not dict or set(observation) != counts | {'readyState'}
                            or any(type(observation[key]) is not int or not 0 <= observation[key] <= 64 for key in counts)
                            or type(observation['readyState']) is not str
                            or observation['readyState'] not in {'loading', 'interactive', 'complete'}):
                        raise ValueError('invalid Hermes composer observation')
                if value['stage'] == 'ready' and (failure is not None or observation is None):
                    raise ValueError('unmeasured Hermes readiness success')
                record.update(composerObservation=observation, guardFailure=failure)
            if 'actionObservation' in value:
                action = value['actionObservation']
                if (type(action) is not dict or set(action) != {'action', 'sampleStatus', 'blocker'}
                        or type(action['action']) is not str or action['action'] not in {'onboarding', 'menu', 'refresh'}
                        or type(action['sampleStatus']) is not str or action['sampleStatus'] not in {
                            'unmeasured', 'guard-rejected', 'hidden', 'outside-viewport',
                            'no-owned-point', 'owned', 'unstable', 'control-replaced', 'click-failed'}
                        or type(action['blocker']) is not str or action['blocker'] not in {
                            'unmeasured', 'none', 'onboarding', 'modal', 'other'}):
                    raise ValueError('invalid Hermes action observation')
                record['actionObservation'] = action
            if 'onboardingSkipped' in value:
                if type(value['onboardingSkipped']) is not bool:
                    raise ValueError('invalid Hermes onboarding observation')
                record['onboardingSkipped'] = value['onboardingSkipped']
        elif mechanism == 'semantic-failure-policy':
            if set(value) != set('schemaVersion mechanism failureStatus recoveryAction'.split()) or type(value['failureStatus']) is not int or value['failureStatus'] not in {400, 503} or value['recoveryAction'] != 'explicit-ui-retry':
                raise ValueError('invalid semantic failure policy')
            record.update(failureStatus=value['failureStatus'], recoveryAction=value['recoveryAction'])
        elif mechanism == 'codex-restore':
            fields = {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'stage', 'cause'}
            stages = {'lock', 'process', 'ownership', 'restore'}
            causes = {'session-busy', 'app-running', 'process-inspection', 'unsafe-path',
                      'profile-invalid', 'receipt-invalid', 'backup-missing', 'backup-mismatch',
                      'config-invalid', 'orphaned-session', 'io', 'persistence', 'unclassified'}
            if (app != 'chatgpt-desktop' or set(value) != fields
                    or value['diagnosticsOnly'] is not True
                    or type(value['stage']) is not str or value['stage'] not in stages
                    or type(value['cause']) is not str or value['cause'] not in causes):
                raise ValueError('invalid Codex restore observation')
            record.update(diagnosticsOnly=True, stage=value['stage'], cause=value['cause'])
        elif mechanism == 'codex-owned-relaunch':
            if (app != 'chatgpt-desktop' or set(value) != {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'stage'}
                    or value['diagnosticsOnly'] is not True
                    or type(value['stage']) is not str
                    or value['stage'] not in {'armed', 'restarted', 'no-request', 'invalid-request', 'executable-changed', 'child-exited', 'startup-timeout', 'bridge-stopped', 'cancelled'}):
                raise ValueError('invalid Codex relaunch observation')
            record.update(diagnosticsOnly=True, stage=value['stage'])
        elif mechanism == 'windows-endpoint-proof':
            if set(value) - {'categoryCounts'} != set('schemaVersion mechanism diagnosticsOnly category'.split()) or value['diagnosticsOnly'] is not True:
                raise ValueError('invalid Windows endpoint proof identity')
            allowed = {'owned', 'process-budget', 'ancestry-cycle', 'process-unavailable', 'parent-unavailable', 'parent-reused', 'session-mismatch', 'ancestry-limit', 'listener-unavailable', 'query-failed', 'transport-timeout', 'transport-failed', 'unclassified'}
            if type(value['category']) is not str or value['category'] not in allowed:
                raise ValueError('invalid Windows endpoint proof category')
            if 'categoryCounts' in value:
                counts = value['categoryCounts']
                if (type(counts) is not dict or set(counts) != allowed
                        or any(type(count) is not int or not 0 <= count <= 4096 for count in counts.values())
                        or counts[value['category']] == 0):
                    raise ValueError('invalid Windows endpoint proof counts')
                record['categoryCounts'] = dict(counts)
            record.update(diagnosticsOnly=True, category=value['category'])
        elif mechanism == 'claude-native-root-preflight':
            fields = {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'stage', 'failure'}
            failures = {'process-absence': {'query-failed', 'process-present'},
                        'foundation-query': {'query-failed'},
                        'native-alignment': {'alignment-mismatch'},
                        'roots-absent': {'existing-root', 'query-failed'},
                        'roots-created': {None, 'creation-failed'}}
            if (app != 'claude-desktop' or set(value) != fields or value['diagnosticsOnly'] is not True
                    or type(value['stage']) is not str or value['stage'] not in failures
                    or (value['failure'] is not None and type(value['failure']) is not str)
                    or value['failure'] not in failures[value['stage']]):
                raise ValueError('invalid Claude native root preflight')
            record.update(diagnosticsOnly=True, stage=value['stage'], failure=value['failure'])
        elif mechanism == 'zed-atspi-geometry':
            counts = set('sampledButtons identityRejected stateRejected stabilityRejected containmentRejected offsetExpected offsetMissing offsetInconsistent toggleOn toggleOff toggleUnknown'.split())
            if (app != 'zed-desktop' or set(value) != counts | {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'status', 'phase'}
                    or value['diagnosticsOnly'] is not True or value.get('phase') not in {'pre-send', 'pre-retry'}
                    or type(value['status']) is not str
                    or value['status'] not in {'observed', 'partial', 'unavailable', 'budget-exceeded', 'guard-rejected'}
                    or any(type(value[key]) is not int or not 0 <= value[key] <= 64 for key in counts)
                    or value['sampledButtons'] + value['identityRejected'] + value['stabilityRejected'] > 64
                    or max(value['stateRejected'], value['containmentRejected']) > value['sampledButtons']
                    or value['sampledButtons'] != sum(value[key] for key in ('toggleOn', 'toggleOff', 'toggleUnknown'))
                    or value['sampledButtons'] != sum(value[key] for key in ('offsetExpected', 'offsetMissing', 'offsetInconsistent'))):
                raise ValueError('invalid Zed AT-SPI geometry observation')
            record.update({key: value[key] for key in counts | {'diagnosticsOnly', 'status', 'phase'}})
        elif mechanism == 'zed-panel-zoom':
            counts = {'maximizeMatches', 'minimizeMatches', 'stableMaximizeMatches',
                      'stableMinimizeMatches', 'correlatedButtons'}
            fields = counts | {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'status',
                               'checkedState', 'uniqueCorrelation', 'activationAttempted'}
            if 'iconCalibration' in value:
                calibration = value['iconCalibration']
                metrics = {'contrastPositions': 4194304, 'foregroundPositions': 4194304,
                           'maxCorrelationMilli': 1000, 'maxContrastMilli': 255000, 'maxSpreadMilli': 255000}
                if (type(calibration) is not dict
                        or set(calibration) != {'templateSide', 'scaleMilli', 'maximize', 'minimize'}
                        or type(calibration['templateSide']) is not int or calibration['templateSide'] not in {14, 28}
                        or type(calibration['scaleMilli']) is not int
                        or calibration['scaleMilli'] != calibration['templateSide'] // 14 * 1000):
                    raise ValueError('invalid Zed zoom icon calibration')
                for name in ('maximize', 'minimize'):
                    measured = calibration[name]
                    if (type(measured) is not dict or set(measured) != set(metrics)
                            or any(type(measured[key]) is not int or not 0 <= measured[key] <= bound
                                   for key, bound in metrics.items())
                            or measured['foregroundPositions'] > measured['contrastPositions']):
                        raise ValueError('invalid Zed zoom icon metrics')
                fields.add('iconCalibration')
            tooltip_fields = {'tooltipStatus', 'tooltipCandidates', 'tooltipMatches'}
            if tooltip_fields & set(value):
                if (tooltip_fields - set(value)
                        or type(value['tooltipStatus']) is not str
                        or value['tooltipStatus'] not in {'unmeasured', 'proved', 'missing', 'ambiguous', 'unavailable'}
                        or any(type(value[key]) is not int or not 0 <= value[key] <= 3 for key in tooltip_fields - {'tooltipStatus'})
                        or value['tooltipMatches'] > value['tooltipCandidates']
                        or value['tooltipStatus'] in {'unmeasured', 'unavailable'} and (value['tooltipCandidates'] != 0 or value['tooltipMatches'] != 0)
                        or value['tooltipStatus'] == 'missing' and value['tooltipMatches'] != 0
                        or value['tooltipStatus'] == 'proved' and value['tooltipMatches'] != 1
                        or value['tooltipStatus'] == 'ambiguous' and value['tooltipMatches'] < 2):
                    raise ValueError('invalid Zed source tooltip proof')
                fields |= tooltip_fields
            progress = {'tooltipStartRemainingMs', 'tooltipEndRemainingMs', 'tooltipPhase'}
            if progress & set(value):
                if (progress - set(value)
                        or any(type(value[key]) is not int or not 0 <= value[key] <= 30000 for key in progress - {'tooltipPhase'})
                        or value['tooltipEndRemainingMs'] > value['tooltipStartRemainingMs']
                        or type(value['tooltipPhase']) is not str or value['tooltipPhase'] not in {
                            'initial-clear', 'absence', 'hover', 'present', 'confirmation', 'final-clear', 'completed'}):
                    raise ValueError('invalid Zed tooltip progress')
                fields |= progress
            role_fields = {'matchedPushButtons', 'matchedToggleButtons', 'nestedContainingControls', 'matchedRole'}
            present_roles = role_fields & set(value)
            if present_roles:
                if (present_roles != role_fields
                        or type(value.get('correlatedButtons')) is not int
                        or any(type(value[key]) is not int or not 0 <= value[key] <= 64 for key in role_fields - {'matchedRole'})
                        or value['matchedPushButtons'] + value['matchedToggleButtons'] != value.get('correlatedButtons')
                        or value['nestedContainingControls'] > value.get('correlatedButtons', -1)
                        or (value['correlatedButtons'] < 2 and value['nestedContainingControls'] != 0)
                        or type(value.get('checkedState')) is not str
                        or type(value['matchedRole']) is not str
                        or value['matchedRole'] != ('none' if value['matchedPushButtons'] + value['matchedToggleButtons'] == 0 else
                            'push-button' if value['matchedToggleButtons'] == 0 else
                            'toggle-button' if value['matchedPushButtons'] == 0 else 'mixed')
                        or (value.get('checkedState') in {'on', 'off', 'mixed'} and
                            (value['matchedToggleButtons'] != 1 or value['matchedPushButtons'] != 0))):
                    raise ValueError('invalid Zed matched control roles')
                fields |= role_fields
            if (app != 'zed-desktop' or set(value) != fields or value['diagnosticsOnly'] is not True
                    or type(value['status']) is not str or value['status'] not in {
                        'observed', 'templates-unavailable', 'unsupported-scale', 'inventory-unavailable',
                        'budget-exceeded', 'guard-rejected'}
                    or type(value['checkedState']) is not str or value['checkedState'] not in {
                        'off', 'on', 'mixed', 'unavailable', 'ambiguous'}
                    or any(type(value[key]) is not int or not 0 <= value[key] <= 64 for key in counts)
                    or type(value['uniqueCorrelation']) is not bool or value['activationAttempted'] is not False
                    or value['uniqueCorrelation'] != (value['correlatedButtons'] == 1
                        and value['stableMaximizeMatches'] + value['stableMinimizeMatches'] == 1)):
                raise ValueError('invalid Zed panel zoom observation')
            record.update({key: value[key] for key in fields - {'schemaVersion', 'mechanism'}})
        elif mechanism == 'windows-post-stop-process':
            fields = {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'phase', 'state'}
            if (app != 'claude-desktop' or set(value) != fields or value['diagnosticsOnly'] is not True
                    or value['phase'] != 'first-accessibility-rejection'
                    or type(value['state']) is not str or value['state'] not in {'present', 'absent', 'query-failed'}):
                raise ValueError('invalid Windows post-stop process observation')
            record.update(diagnosticsOnly=True, phase=value['phase'], state=value['state'])
        elif mechanism == 'windows-process-absence':
            fields = {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'app', 'stage'}
            stages = {'deadline', 'system-root', 'private-output', 'spawn', 'exit', 'read', 'schema', 'oversize', 'snapshot', 'first', 'next'}
            if (set(value) != fields or app not in {'claude-desktop', 'chatgpt-desktop'}
                    or value['app'] != app or value['diagnosticsOnly'] is not True
                    or type(value['stage']) is not str or value['stage'] not in stages):
                raise ValueError('invalid Windows process absence diagnostic')
            record.update(diagnosticsOnly=True, app=app, stage=value['stage'])
        elif mechanism == 'claude-restore':
            fields = {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'stage', 'outcome', 'errorCategory'}
            categories = {'session-busy', 'app-running', 'process-query', 'unsafe-state', 'backup-mismatch',
                          'receipt-schema', 'no-receipt', 'permissions', 'lock-io', 'receipt-read',
                          'backup-read', 'document-restore', 'backup-remove', 'receipt-remove', 'state-read',
                          'state-write', 'directory-create', 'orphan-backup', 'other'}
            stage_categories = {'session-lock': {'session-busy', 'permissions', 'lock-io', 'unsafe-state', 'state-read', 'directory-create', 'other'},
                                'process-check': {'app-running', 'process-query', 'other'},
                                'receipt': categories - {'session-busy', 'app-running', 'process-query', 'lock-io'}}
            stage, outcome, category = value.get('stage'), value.get('outcome'), value.get('errorCategory')
            expected = 'restored' if category is None else ('nothing-to-restore' if category == 'no-receipt' else 'rejected')
            if (app != 'claude-desktop' or set(value) != fields or value['diagnosticsOnly'] is not True
                    or type(stage) is not str or stage not in stage_categories
                    or (category is not None and (type(category) is not str or category not in stage_categories[stage]))
                    or outcome != expected or (category is None and stage != 'receipt')):
                raise ValueError('invalid Claude restore observation')
            record.update({key: value[key] for key in fields - {'schemaVersion', 'mechanism'}})
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
        elif mechanism == 'claude-chat-navigation':
            flags = set('preconditionsVerified pressAttempted chatPostconditionVerified nativeGuardVerified'.split())
            fields = flags | set('schemaVersion mechanism diagnosticsOnly phase actionStatus'.split())
            phase, action = value.get('phase'), value.get('actionStatus')
            if (set(value) - {'nativePressStage', 'postconditionCounts'} != fields or app != 'claude-desktop' or value['diagnosticsOnly'] is not True
                    or type(phase) is not str or phase not in {'preflight', 'press', 'postcondition', 'completed'}
                    or type(action) is not str or action not in {'not-attempted', 'completed', 'uncertain'}
                    or any(type(value[key]) is not bool for key in flags)):
                raise ValueError('invalid Claude Chat navigation identity')
            if 'nativePressStage' in value:
                stage = value['nativePressStage']
                stages = {'request', 'initial-proof', 'window-bounds', 'tree', 'mode', 'chat', 'control-recheck', 'hit-test', 'deadline', 'press-uncertain', 'completed', 'current-chat'}
                if (type(stage) is not str or stage not in stages
                        or (stage in {'completed', 'press-uncertain'}) != value['pressAttempted']):
                    raise ValueError('invalid Claude native Chat press stage')
                record['nativePressStage'] = stage
            if 'postconditionCounts' in value:
                counts = value['postconditionCounts']
                keys = set('classicEditable classicVisible modernMessageEditable sendMessageVisible sendMessageEnabled startTaskVisible modeGroupVisible modeChatVisible modeChatEnabled modeCoworkVisible'.split())
                if (phase not in {'postcondition', 'completed'} or type(counts) is not dict or set(counts) != keys
                        or any(count is not None and (type(count) is not int or not 0 <= count <= 4096) for count in counts.values())):
                    raise ValueError('invalid Claude Chat postcondition counts')
                record['postconditionCounts'] = counts
            attempted, ready, post, guarded = (value[key] for key in
                ('pressAttempted', 'preconditionsVerified', 'chatPostconditionVerified', 'nativeGuardVerified'))
            if (attempted != (action != 'not-attempted') or attempted and not ready
                    or phase == 'preflight' and (attempted or post)
                    or phase != 'preflight' and not attempted
                    or post and not (attempted and ready and guarded)
                    or (phase == 'completed') != post
                    or value.get('nativePressStage') == 'current-chat' and not (phase == 'preflight' and ready and guarded)):
                raise ValueError('inconsistent Claude Chat navigation observation')
            record.update(diagnosticsOnly=True, phase=phase, actionStatus=action,
                          **{key: value[key] for key in flags})
        elif mechanism == 'claude-windows-uia':
            counts = {'nodeCount', 'classicEditorCount', 'modernEditorCount', 'sendControlCount',
                      'startTaskControlCount', 'assistantHeadingCount', 'copyControlCount'}
            fields = counts | {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'phase', 'status',
                               'nativeGuardVerified', 'treeComplete'}
            statuses = set('observed query deadline identity foreground bounds visibility display limit occlusion duplicate element-identity root-process-query root-process-mismatch root-process-zero root-process-invalid descendant-process-query descendant-process-mismatch descendant-process-zero descendant-process-invalid owned-descendant-process foreign-descendant-process descendant-correlation-unavailable heading-property com root-replaced transport protocol policy directory'.split())
            observed = value.get('status') == 'observed'
            if (app != 'claude-desktop' or set(value) != fields or value['diagnosticsOnly'] is not True
                    or value['phase'] != 'post-ready' or type(value['status']) is not str or value['status'] not in statuses
                    or value['nativeGuardVerified'] is not observed or value['treeComplete'] is not observed
                    or any((type(value[key]) is not int or not 0 <= value[key] <= 1024) if observed else value[key] is not None for key in counts)
                    or observed and (value['nodeCount'] == 0 or any(value[key] > value['nodeCount'] for key in counts))):
                raise ValueError('invalid passive Windows Claude UIA diagnostic')
            record.update({key: value[key] for key in fields - {'schemaVersion', 'mechanism'}})
        elif mechanism == 'claude-windows-fit':
            fields = {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'phase', 'fitAttempted', 'helperSucceeded'}
            if (app != 'claude-desktop' or set(value) != fields or value['diagnosticsOnly'] is not True
                    or value['phase'] != 'final-ready' or value['fitAttempted'] is not True
                    or type(value['helperSucceeded']) is not bool):
                raise ValueError('invalid Claude final Windows fit diagnostic')
            record.update(diagnosticsOnly=True, phase='final-ready', fitAttempted=True,
                          helperSucceeded=value['helperSucceeded'])
        elif mechanism == 'claude-windows-fit-rejection':
            fields = set('schemaVersion mechanism diagnosticsOnly phase policyEnabled fitAttempted sourceComposerReady candidateReason guardFailure eligibleCount sameProcessAheadCount overlapAheadCount'.split())
            if (app != 'claude-desktop' or set(value) != fields or value['diagnosticsOnly'] is not True
                    or type(value['phase']) is not str or value['phase'] not in {'initial-pending', 'pending-attachment', 'final-ready'}
                    or any(type(value[key]) is not bool for key in ('policyEnabled', 'fitAttempted'))
                    or (value['sourceComposerReady'] is not True if value['phase'] == 'final-ready' else value['sourceComposerReady'] is not None)
                    or type(value['candidateReason']) is not str or value['candidateReason'] not in {
                        'already-fitted', 'candidate-count', 'identity-mismatch', 'native-guard',
                        'display-relation-unavailable', 'snapshot-identity-missing', 'same-process-ahead', 'overlap-ahead'}
                    or value['guardFailure'] is not None and (type(value['guardFailure']) is not str
                        or value['guardFailure'] not in {'identity-missing', 'bounds-changed', 'foreground-changed',
                                                       'same-process-window', 'off-display', 'occluded'})
                    or type(value['eligibleCount']) is not int or not 0 <= value['eligibleCount'] <= 64
                    or any(value[key] is not None and (type(value[key]) is not int or not 0 <= value[key] <= 64)
                           for key in ('sameProcessAheadCount', 'overlapAheadCount'))):
                raise ValueError('invalid Claude Windows fit rejection diagnostic')
            record.update({key: value[key] for key in fields - {'schemaVersion', 'mechanism'}})
        elif mechanism == 'claude-native-chat':
            counts = {'submittedTurns', 'inputVerifiedTurns', 'copiedResponses'}
            flags = {'retryAttempted', 'clipboardCleared'}
            fields = counts | flags | {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'stage'}
            stages = set('request window tree tree-query tree-duplicate tree-type tree-limit tree-pid tree-focus tree-window mode composer focus input-mismatch input-initial-unavailable input-initial-nonempty input-clipboard-mismatch input-value-mismatch control scope scope-anchor-absent scope-heading-absent scope-assistant-heading-absent scope-marker-heading-absent scope-anchor-ambiguous scope-control-absent scope-control-ambiguous scope-heading-ambiguous scope-prompt-mismatch deadline action-uncertain response-mismatch sent copied retry-ready retried completed'.split())
            phase_fields = {'actionPhase', 'transportFailure'}
            if (app != 'claude-desktop' or set(value) - {'providerObservation', 'guardRejection'} not in (fields, fields | phase_fields) or value['diagnosticsOnly'] is not True
                    or type(value['stage']) is not str or value['stage'] not in stages
                    or any(type(value[key]) is not bool for key in flags)
                    or any(type(value[key]) is not int or not 0 <= value[key] <= 3 for key in counts)
                    or value['submittedTurns'] > value['inputVerifiedTurns']
                    or value['copiedResponses'] > value['submittedTurns'] + int(value['retryAttempted'])):
                raise ValueError('invalid Claude native Chat observation')
            if phase_fields <= set(value):
                phase, failure = value['actionPhase'], value['transportFailure']
                if ((phase is not None and (type(phase) is not str or phase not in {
                        'before-guard', 'after-guard', 'transport', 'post-guard', 'completed'}))
                        or (failure is not None and (type(failure) is not str or failure not in {
                            'invalid-input', 'spawn', 'pipe', 'output', 'timeout', 'nonzero-exit',
                            'window-changed', 'window-query-rejected', 'session-unavailable'}))
                        or failure is not None and phase not in {'before-guard', 'transport', 'post-guard'}):
                    raise ValueError('invalid Claude action transport diagnostic')
                record.update(actionPhase=phase, transportFailure=failure)
            if 'guardRejection' in value:
                rejection = value['guardRejection']
                if (type(rejection) is not str or rejection not in {
                        'identity-missing', 'bounds-changed', 'foreground-changed',
                        'same-process-window', 'off-display', 'occluded'}
                        or not phase_fields <= set(value)
                        or value['actionPhase'] not in {'before-guard', 'post-guard'}
                        or value['transportFailure'] is not None or value['stage'] == 'completed'):
                    raise ValueError('invalid Claude native guard rejection')
                record['guardRejection'] = rejection
            if 'providerObservation' in value:
                observation = value['providerObservation']
                if observation is not None and (type(observation) is not dict or set(observation) != {
                        'generationObserved', 'fixtureResponseVerified', 'failureObserved'}
                        or any(type(flag) is not bool for flag in observation.values())):
                    raise ValueError('invalid Claude provider observation')
                record['providerObservation'] = observation
            record.update({key: value[key] for key in fields - {'schemaVersion', 'mechanism'}})
        elif mechanism == 'windows-process-correlation':
            flags = {'sameLauncherSurvives', 'verifiedDescendantsPresent', 'unlinkedMatchesPresent'}
            counts = {'matchedCount', 'verifiedDescendantCount', 'unlinkedCount'}
            fields = flags | counts | {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'status'}
            if (app != 'claude-desktop' or set(value) != fields or value['diagnosticsOnly'] is not True
                    or type(value['status']) is not str or value['status'] not in {'observed', 'unavailable', 'deadline'}):
                raise ValueError('invalid Windows process correlation')
            if value['status'] == 'observed':
                if (any(type(value[key]) is not bool for key in flags)
                        or any(type(value[key]) is not int or not 0 <= value[key] <= 64 for key in counts)
                        or value['matchedCount'] != value['verifiedDescendantCount'] + value['unlinkedCount']
                        or value['verifiedDescendantsPresent'] != (value['verifiedDescendantCount'] > 0)
                        or value['unlinkedMatchesPresent'] != (value['unlinkedCount'] > 0)):
                    raise ValueError('invalid Windows process correlation counts')
            elif any(value[key] is not None for key in flags | counts):
                raise ValueError('incomplete Windows process correlation')
            record.update({key: value[key] for key in fields - {'schemaVersion', 'mechanism'}})
        elif mechanism == 'windows-owned-descendant-cleanup':
            flags = set('triggerAttempted expectedExecutableVerified historicalOwnershipVerified'.split())
            counts = set('retainedCount alreadyExitedCount targetedCount exitedCount rejectedCount'.split())
            fields = flags | counts | {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'status'}
            status = value.get('status')
            if (app != 'claude-desktop' or set(value) != fields or value['diagnosticsOnly'] is not True
                    or type(status) is not str or status not in {'completed', 'partial', 'unavailable', 'deadline', 'uncertain'}
                    or any(type(value[key]) is not bool for key in flags)
                    or any(value[key] is not None and (type(value[key]) is not int or not 0 <= value[key] <= 64) for key in counts)
                    or value['historicalOwnershipVerified'] and not value['expectedExecutableVerified']):
                raise ValueError('invalid Windows owned descendant cleanup')
            if status in {'completed', 'partial'}:
                if (not all(value[key] for key in flags) or any(value[key] is None for key in counts)
                        or value['retainedCount'] != value['alreadyExitedCount'] + value['targetedCount'] + value['rejectedCount']
                        or value['exitedCount'] > value['targetedCount']
                        or (status == 'completed') != (value['rejectedCount'] == 0 and value['exitedCount'] == value['targetedCount'])):
                    raise ValueError('invalid Windows owned descendant cleanup counts')
            elif status == 'unavailable':
                if any(value[key] for key in flags) or any(value[key] is not None for key in counts):
                    raise ValueError('unavailable Windows owned descendant cleanup has evidence')
            elif (any(value[key] is not None for key in counts - {'retainedCount'})
                    or value['triggerAttempted'] and not (value['expectedExecutableVerified']
                        and value['historicalOwnershipVerified'] and value['retainedCount'] is not None)):
                raise ValueError('invalid Windows owned descendant cleanup deadline')
            record.update({key: value[key] for key in fields - {'schemaVersion', 'mechanism'}})
        elif mechanism == 'windows-owned-stop':
            flags = {'wrapperPresent', 'launcherHandleAvailable', 'jobClosed'}
            fields = flags | {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'terminateResult'}
            outcome = value.get('terminateResult')
            if (set(value) != fields or app != 'claude-desktop' or value['diagnosticsOnly'] is not True
                    or type(outcome) is not str or outcome not in {'issued', 'failed', 'not-attempted'}
                    or any(type(value[key]) is not bool for key in flags)
                    or not value['wrapperPresent'] and (value['launcherHandleAvailable'] or outcome != 'not-attempted')):
                raise ValueError('invalid Windows owned stop observation')
            record.update({key: value[key] for key in fields - {'schemaVersion', 'mechanism'}})
        elif mechanism == 'windows-process-settlement':
            fields = set('schemaVersion mechanism diagnosticsOnly firstState lastState queryCount'.split())
            states = {'present', 'absent', 'query-failed', 'not-queried'}
            first, last, count = value.get('firstState'), value.get('lastState'), value.get('queryCount')
            if (set(value) != fields or app != 'claude-desktop' or value['diagnosticsOnly'] is not True
                    or type(first) is not str or first not in states or type(last) is not str or last not in states
                    or count is not None and (type(count) is not int or not 0 <= count <= 128)
                    or count == 0 and (first != 'not-queried' or last != 'not-queried')
                    or count != 0 and ('not-queried' in (first, last))
                    or count == 1 and first != last):
                raise ValueError('invalid Windows process settlement observation')
            record.update(diagnosticsOnly=True, firstState=first, lastState=last, queryCount=count)
        elif mechanism == 'claude-window-fit':
            fields = set('schemaVersion mechanism diagnosticsOnly stage'.split())
            stages = {'completed', 'request', 'initial-proof', 'screen', 'rectangle', 'settable',
                      'identity-recheck', 'allocation', 'size', 'position', 'postcondition',
                      'transport', 'invalid-output'}
            if (set(value) != fields or app != 'claude-desktop' or value['diagnosticsOnly'] is not True
                    or type(value['stage']) is not str or value['stage'] not in stages):
                raise ValueError('invalid Claude window fit observation')
            record.update(diagnosticsOnly=True, stage=value['stage'])
        elif mechanism == 'claude-window-focus':
            fields = {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'status', 'nativeForegroundWindowMatchedHeld'}
            statuses = {'proved', 'untrusted', 'query-error', 'focus-mismatch', 'not-standard',
                        'identity-changed', 'no-match', 'ambiguous'}
            if (app != 'claude-desktop' or set(value) - {'phase', 'candidateState', 'guardCategory', 'windowOnlyQuery', 'agreement', 'windowOnlyAgreement'} not in (fields, fields | {'query'}, fields | {'windowOnlyStatus', 'windowOnlyMatchedHeld'}, fields | {'query', 'windowOnlyStatus', 'windowOnlyMatchedHeld'}) or value['diagnosticsOnly'] is not True
                    or type(value['status']) is not str or value['status'] not in statuses
                    or (type(value['nativeForegroundWindowMatchedHeld']) is not bool
                        if value['status'] == 'proved' else value['nativeForegroundWindowMatchedHeld'] is not None)):
                raise ValueError('invalid Claude focus observation')
            if 'windowOnlyStatus' in value:
                window_status = value['windowOnlyStatus']
                window_matched = value['windowOnlyMatchedHeld']
                if (window_status is not None and (type(window_status) is not str or window_status not in statuses)
                        or (type(window_matched) is not bool if window_status == 'proved' else window_matched is not None)):
                    raise ValueError('invalid Claude window-only focus observation')
                record.update(windowOnlyStatus=window_status, windowOnlyMatchedHeld=window_matched)
            if 'phase' in value:
                if type(value['phase']) is not str or value['phase'] not in {'initial', 'final-stability', 'initial-decision'}:
                    raise ValueError('invalid Claude focus phase')
                record['phase'] = value['phase']
            if 'guardCategory' in value:
                category = value['guardCategory']
                if (value.get('phase') != 'initial-decision'
                        or category is not None and (type(category) is not str or category not in {
                            'identity-missing', 'bounds-changed', 'foreground-changed',
                            'same-process-window', 'off-display', 'occluded'})):
                    raise ValueError('invalid Claude initial decision category')
                record['guardCategory'] = category
            elif value.get('phase') == 'initial-decision':
                raise ValueError('missing Claude initial decision category')
            if 'candidateState' in value:
                if (value.get('phase') != 'final-stability' or type(value['candidateState']) is not str
                        or value['candidateState'] not in {'absent', 'ambiguous', 'identity-changed', 'bounds-changed',
                                                          'focus-unproved', 'same-process-window', 'off-display', 'occluded', 'proved'}):
                    raise ValueError('invalid Claude acquisition candidate state')
                record['candidateState'] = value['candidateState']
            if 'windowOnlyQuery' in value and 'windowOnlyStatus' not in value:
                raise ValueError('missing Claude window-only focus status')
            for key, status_key in (('agreement', 'status'), ('windowOnlyAgreement', 'windowOnlyStatus')):
                if key in value:
                    agreement = value[key]
                    if (value.get(status_key) != 'identity-changed' or type(agreement) is not str
                            or agreement not in {'foreground-changed', 'after-proof-unready',
                                                 'window-element-changed', 'geometry-changed'}):
                        raise ValueError('invalid Claude focus agreement observation')
                    record[key] = agreement
            for key, status_key in (('query', 'status'), ('windowOnlyQuery', 'windowOnlyStatus')):
                query = value.get(key)
                if query is not None:
                    stages = set('app-create app-timeout focused-window main-window focused-element input-timeout input-window element-type pid window-timeout role subrole position size geometry'.split())
                    errors = set('failure illegal-argument invalid-element cannot-complete attribute-unsupported not-implemented api-disabled no-value other empty-value type-mismatch owner-mismatch geometry-invalid'.split())
                    if (type(query) is not dict or set(query) != {'phase', 'stage', 'error'}
                            or any(type(query[field]) is not str for field in ('phase', 'stage', 'error'))
                            or query['phase'] not in {'before', 'after'} or query['stage'] not in stages
                            or query['error'] not in errors
                            or value.get(status_key) != {'before': 'query-error', 'after': 'identity-changed'}[query['phase']]):
                        raise ValueError('invalid Claude focus query observation')
                if key in value:
                    record[key] = query
            record.update({key: value[key] for key in fields - {'schemaVersion', 'mechanism'}})
        elif mechanism == 'claude-window-stack':
            counts = set('samePidAheadCount samePidAheadEligibleCount samePidAheadIntersectsHeldCount samePidAheadNormalLayerCount samePidAheadOtherLayerCount'.split())
            flags = {'foregroundPidMatchesHeld', 'frontmostWindowSamePid'}
            fields = counts | flags | {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'status'}
            if (app != 'claude-desktop' or set(value) != fields or value['diagnosticsOnly'] is not True
                    or value['status'] not in {'complete', 'overflow', 'unavailable'}
                    or any(type(value[key]) is not bool for key in flags)
                    or any((type(value[key]) is not int or not 0 <= value[key] <= 32)
                           if value['status'] == 'complete' else value[key] is not None for key in counts)):
                raise ValueError('invalid Claude window stack observation')
            if value['status'] == 'complete':
                total = value['samePidAheadCount']
                if (any(value[key] > total for key in counts)
                        or value['samePidAheadNormalLayerCount'] + value['samePidAheadOtherLayerCount'] != total):
                    raise ValueError('invalid Claude window stack counts')
            record.update({key: value[key] for key in fields - {'schemaVersion', 'mechanism'}})
        elif mechanism == 'claude-model-discovery':
            fields = {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'authenticatedModelsCount', 'complete', 'modelDiscoverySeen'}
            count = value.get('authenticatedModelsCount')
            seen = value.get('modelDiscoverySeen')
            if (set(value) != fields or app != 'claude-desktop' or value['diagnosticsOnly'] is not True
                    or type(count) is not int or not 0 <= count <= 32 or type(value['complete']) is not bool
                    or (count == 0 and seen is not None) or (count > 0 and seen is not True)):
                raise ValueError('invalid Claude model discovery observation')
            record.update(diagnosticsOnly=True, authenticatedModelsCount=count,
                          complete=value['complete'], modelDiscoverySeen=seen)
        elif mechanism == 'claude-native-storage':
            fields = set('schemaVersion mechanism diagnosticsOnly freshBefore observationValid before after'.split())
            storage = set('claudeLocalState claudePreferences thirdPartyLocalState thirdPartyPreferences'.split())
            if (set(value) != fields or app != 'claude-desktop' or value['diagnosticsOnly'] is not True
                    or type(value['observationValid']) is not bool):
                raise ValueError('invalid Claude native storage observation identity')
            for phase in ('before', 'after'):
                flags = value[phase]
                if flags is not None and (type(flags) is not dict or set(flags) != storage
                                          or any(type(flag) is not bool for flag in flags.values())):
                    raise ValueError('invalid Claude native storage observation flags')
            expected_fresh = None if value['before'] is None else not any(value['before'].values())
            if (value['freshBefore'] is not expected_fresh
                    or value['observationValid'] is not (value['before'] is not None and value['after'] is not None)):
                raise ValueError('inconsistent Claude native storage observation')
            record.update(diagnosticsOnly=True, freshBefore=value['freshBefore'],
                          observationValid=value['observationValid'], before=value['before'], after=value['after'])
        elif mechanism == 'claude-private-storage-stage':
            fields = set('schemaVersion mechanism diagnosticsOnly phase stage'.split())
            stages = {
                'before-launch': {'captured', 'scope-rejected', 'workspace-unavailable', 'home-unavailable',
                                  'environment-unbound', 'root-rejected', 'root-create-failed',
                                  'snapshot-unavailable', 'checkpoint-write-failed'},
                'after-stop': {'scope-rejected', 'checkpoint-read-failed', 'checkpoint-decode-failed',
                               'recorded', 'snapshot-unavailable'},
            }
            phase, stage = value.get('phase'), value.get('stage')
            if (set(value) != fields or app != 'claude-desktop' or value['diagnosticsOnly'] is not True
                    or type(phase) is not str or phase not in stages
                    or type(stage) is not str or stage not in stages[phase]):
                raise ValueError('invalid Claude private storage stage')
            record.update(diagnosticsOnly=True, phase=phase, stage=stage)
        elif mechanism == 'windows-owned-cleanup-preflight':
            fields = set('schemaVersion mechanism diagnosticsOnly stage'.split())
            stages = set('request path file-open file-hash file-identity process-open snapshot inspector-parent ancestry target-open target-identity owner-recheck deadline transport'.split())
            if (set(value) != fields or app != 'claude-desktop' or value['diagnosticsOnly'] is not True
                    or type(value['stage']) is not str or value['stage'] not in stages):
                raise ValueError('invalid Windows owned cleanup preflight')
            record.update(diagnosticsOnly=True, stage=value['stage'])
        elif mechanism == 'claude-linux-mode-roles':
            hashes = dict(modeSourceSha256='62ffbc1b8a3e4440ae77a33be142afd1914796f945bcd75d58cfe73679925f61',
                          segmentedSourceSha256='1fe986422649ab736613079340a52157efd7791b96e0b9c00c46681731b7a4ea',
                          radioSourceSha256='9c6ff87b4eaf0e9ad25e6329536f4337586b015e0f868389e72480c1769920a9')
            fields = set(hashes) | set('schemaVersion mechanism diagnosticsOnly sourceVersion status sourceCount'.split())
            keys = set('modeGroupVisible chatButtonVisible chatButtonEnabled chatRadioVisible chatRadioEnabled coworkButtonVisible coworkButtonEnabled coworkRadioVisible coworkRadioEnabled'.split())
            counts = value.get('sourceCount')
            if (app != 'claude-desktop' or set(value) != fields or value['diagnosticsOnly'] is not True
                    or value['sourceVersion'] != '2.9939.4' or any(value[key] != digest for key, digest in hashes.items())
                    or type(value['status']) is not str or value['status'] not in {'observed', 'group-unavailable', 'group-ambiguous', 'query-failed'}
                    or type(counts) is not dict or set(counts) != keys
                    or any(count is not None and (type(count) is not int or not 0 <= count <= 4096) for count in counts.values())):
                raise ValueError('invalid passive Linux Claude mode roles')
            group = counts['modeGroupVisible']
            controls = [counts[key] for key in keys - {'modeGroupVisible'}]
            status = value['status']
            if ((status == 'observed' and (group != 1 or any(count is None for count in controls)))
                    or (status != 'observed' and any(count is not None for count in controls))
                    or (status == 'group-unavailable' and group != 0)
                    or (status == 'group-ambiguous' and (group is None or group < 2))
                    or (status == 'query-failed' and group not in {None, 1})):
                raise ValueError('inconsistent passive Linux Claude mode roles')
            record.update(diagnosticsOnly=True, sourceVersion=value['sourceVersion'], status=status,
                          sourceCount=dict(counts), **hashes)
        elif mechanism == 'claude-native-composer':
            hashes = {
                'classicSourceSha256': '6e6be632eb7adc0e66c1bb795448269d6c1f3ffe8821bea59d9e9374671cf0ea',
                'sendSourceSha256': '69d43f83ac78605402b590559cfb9bd355215336a193cedf80cc30b246c1db60',
                'modernSourceSha256': 'a9f54a8a154e19f86a9d9d696b808bd693904b5e47ec63517abb635003a4244d',
            }
            expected_version = '2.19675.0'
            expected_mode = '0d16680f19e10d03bc11e7797d842d01159da37b5ab410cad9b7307f7eeef3aa'
            if value.get('sourceVersion') == '2.9939.4':
                expected_version = '2.9939.4'
                expected_mode = '62ffbc1b8a3e4440ae77a33be142afd1914796f945bcd75d58cfe73679925f61'
                hashes = dict(classicSourceSha256='26f823bafc90cff4a749bfad6916ee69e4c3189f18b54a4e958ca387939c1181',
                              sendSourceSha256='d076b2f208fc5e572d0f3cd39aba35c6bacbe100a82db569851a0ce2317fa05c',
                              modernSourceSha256='5d1afc949ac69080ef6fe15491137ca0c3d2056991a9581537cba2bcc3724287')
            fields = set(hashes) | set('schemaVersion mechanism diagnosticsOnly sourceVersion sourceCount'.split())
            mode_hash = value.get('modeSourceSha256')
            if 'modeSourceSha256' in value:
                if mode_hash != expected_mode:
                    raise ValueError('invalid Claude mode source identity')
                fields.add('modeSourceSha256')
            counts = value.get('sourceCount')
            keys = set('classicEditable classicVisible modernMessageEditable sendMessageVisible sendMessageEnabled startTaskVisible'.split())
            if 'modeSourceSha256' in value:
                keys.update('modeGroupVisible modeChatVisible modeChatEnabled modeCoworkVisible'.split())
            if (set(value) != fields or app != 'claude-desktop' or value['diagnosticsOnly'] is not True
                    or value['sourceVersion'] != expected_version or any(value[key] != digest for key, digest in hashes.items())
                    or type(counts) is not dict or set(counts) != keys
                    or any(count is not None and (type(count) is not int or not 0 <= count <= 4096) for count in counts.values())):
                raise ValueError('invalid Claude native composer observation')
            record.update(diagnosticsOnly=True, sourceVersion=value['sourceVersion'], sourceCount=dict(counts), **hashes)
            if 'modeSourceSha256' in value:
                record['modeSourceSha256'] = mode_hash
        elif mechanism == 'claude-storage-use':
            fields = set('schemaVersion mechanism diagnosticsOnly freshBefore observationValid before after'.split())
            storage = set('claudeLocalState claudePreferences thirdPartyLocalState thirdPartyPreferences'.split())
            if (set(value) != fields or app != 'claude-desktop' or value['diagnosticsOnly'] is not True
                    or type(value['freshBefore']) is not bool or type(value['observationValid']) is not bool):
                raise ValueError('invalid Claude storage observation identity')
            for phase in ('before', 'after'):
                flags = value[phase]
                if type(flags) is not dict or set(flags) != storage or any(type(flag) is not bool for flag in flags.values()):
                    raise ValueError('invalid Claude storage observation flags')
            initial_present = any(value['before'].values())
            if (value['freshBefore'] and initial_present
                    or value['observationValid'] and value['freshBefore'] == initial_present):
                raise ValueError('inconsistent Claude storage freshness')
            record.update(diagnosticsOnly=True, freshBefore=value['freshBefore'],
                          observationValid=value['observationValid'], before=dict(value['before']), after=dict(value['after']))
        elif mechanism == 'zed-pointer-observation':
            flags = set('maximizedHorizontal maximizedVertical enabled sensitive showing visible defunct retryContains'.split())
            base = flags | {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'pointerTarget', 'pointerChild'}
            coordinate_fields = {'clientOriginVerified', 'retryOffsetRelation'}
            authority_fields = {'coordinatePackage', 'coordinateRelation', 'coordinateAuthority'}
            modifier_fields = {'modifierState', 'buttonsHeld'}
            present_modifiers = modifier_fields & set(value)
            ancestor_fields = {'centerWithinPublishedAncestors', 'ancestorBoundsStatus', 'checkedAncestorCount'}
            present_ancestors = ancestor_fields & set(value)
            ancestor_stage_fields = {'ancestorQueryStage', 'ancestorBoundsFailure'}
            if 'ancestorQueryStage' in value:
                stage = value['ancestorQueryStage']
                stages = {'unavailable', 'dbus-import', 'bus', 'owner', 'identity', 'retry-bounds',
                          'application', 'parent', 'ancestor-bounds', 'comparison', 'deadline', 'chain', 'complete'}
                state = value.get('ancestorBoundsStatus')
                if (present_ancestors != ancestor_fields or type(stage) is not str or stage not in stages
                        or (state == 'complete') != (stage == 'complete')
                        or state in {'cycle', 'limit'} and stage != 'chain'):
                    raise ValueError('invalid Zed ancestor query stage')
                record['ancestorQueryStage'] = stage
            if 'ancestorBoundsFailure' in value:
                failure = value['ancestorBoundsFailure']
                if (present_ancestors != ancestor_fields or value.get('ancestorBoundsStatus') != 'unavailable'
                        or value.get('ancestorQueryStage') != 'ancestor-bounds'
                        or type(failure) is not str or failure not in {
                            'component-unavailable', 'query-failed', 'invalid-geometry', 'unmeasured'}):
                    raise ValueError('invalid Zed ancestor bounds failure')
                record['ancestorBoundsFailure'] = failure
            if present_ancestors:
                state, count = value.get('ancestorBoundsStatus'), value.get('checkedAncestorCount')
                within = value.get('centerWithinPublishedAncestors')
                if (present_ancestors != ancestor_fields or type(state) is not str
                        or state not in {'complete', 'unavailable', 'cycle', 'limit'}
                        or type(count) is not int or not 0 <= count <= 64
                        or (type(within) is not bool if state == 'complete' else within is not None)
                        or state == 'limit' and count != 64):
                    raise ValueError('invalid Zed published ancestor observation')
                record.update(centerWithinPublishedAncestors=within, ancestorBoundsStatus=state,
                              checkedAncestorCount=count)
            if 'transientDialogs' in value:
                dialogs = value['transientDialogs']
                if (type(dialogs) is not dict or set(dialogs) != {
                        'state', 'ownedTransientDialogs', 'mappedOwnedTransientDialogs'}
                        or type(dialogs['state']) is not str or dialogs['state'] not in {
                            'complete', 'unavailable', 'query-failed', 'identity-rejected', 'deadline', 'limit'}):
                    raise ValueError('invalid Zed transient dialog observation')
                total, mapped = dialogs['ownedTransientDialogs'], dialogs['mappedOwnedTransientDialogs']
                if dialogs['state'] == 'complete':
                    if (type(total) is not int or type(mapped) is not int
                            or not 0 <= mapped <= total <= 32):
                        raise ValueError('invalid Zed transient dialog counts')
                elif total is not None or mapped is not None:
                    raise ValueError('incomplete Zed transient dialog counts')
                record['transientDialogs'] = dict(dialogs)
            if (set(value) - modifier_fields - ancestor_fields - ancestor_stage_fields - {'cursorSelection', 'transientDialogs'} not in (base, base | {'inputDelivery'}, base | coordinate_fields,
                                  base | coordinate_fields | {'inputDelivery'}, base | coordinate_fields | authority_fields,
                                  base | coordinate_fields | authority_fields | {'inputDelivery'})
                    or present_modifiers and present_modifiers != modifier_fields
                    or app != 'zed-desktop' or value['diagnosticsOnly'] is not True):
                raise ValueError('invalid Zed pointer observation identity')
            if 'cursorSelection' in value:
                selection = value['cursorSelection']
                fields = {'status', 'sampledPoints', 'exactPointerMatched', 'accessibleHitVerified'}
                counts = {'guardBeforeVerified': 20, 'guardAfterVerified': 20, 'accessibleChecks': 20,
                          'accessibleExactMatches': 20, 'cursorChecks': 19, 'cursorExactMatches': 19}
                extended = fields | set(counts) | {'failureReason'}
                classified = extended | {'cursorClasses', 'cursorSizeSource'}
                pointer_fields = {'pointerChecks', 'pointerPositionMatches', 'pointerChildMatches'}
                pointer_proved = type(selection) is dict and pointer_fields <= set(selection)
                if pointer_proved:
                    # The click boundary repeats the cursor and AX proof once.
                    counts = {key: 46 if key.startswith('cursor') else 56 for key in counts}
                if (type(selection) is not dict or set(selection) not in (fields, extended, classified, extended | pointer_fields, classified | pointer_fields)
                        or type(selection['status']) is not str or selection['status'] not in {'matched', 'unavailable', 'no-hit', 'deadline', 'identity-rejected'}
                        or type(selection['sampledPoints']) is not int or not 0 <= selection['sampledPoints'] <= 9
                        or any(type(selection[key]) is not bool for key in ('exactPointerMatched', 'accessibleHitVerified'))
                        or (selection['status'] == 'matched') != (selection['exactPointerMatched'] and selection['accessibleHitVerified'])
                        or selection['status'] == 'matched' and selection['sampledPoints'] == 0
                        or selection['status'] != 'matched' and (selection['exactPointerMatched'] or selection['accessibleHitVerified'])):
                    raise ValueError('invalid Zed cursor selection')
                if set(selection) in (extended, classified, extended | pointer_fields, classified | pointer_fields):
                    reason = selection['failureReason']
                    if (any(type(selection[key]) is not int or not 0 <= selection[key] <= limit
                            for key, limit in counts.items())
                            or reason is not None and (type(reason) is not str or reason not in {
                                'cursor-unmatched', 'cursor-unstable', 'accessible-hit-mismatch',
                                'accessible-query-unavailable', 'identity-rejected', 'deadline'} |
                                ({'pointer-position', 'pointer-child'} if pointer_proved else set()))
                            or selection['accessibleExactMatches'] > selection['accessibleChecks']
                            or selection['guardAfterVerified'] > selection['guardBeforeVerified']
                            or selection['cursorExactMatches'] > selection['cursorChecks']
                            or selection['status'] == 'matched' and reason is not None):
                        raise ValueError('invalid Zed cursor proof counters')
                if pointer_proved:
                    final_cursor_sample = int(selection['pointerChecks'] == selection['pointerChildMatches']
                                              and type(selection['pointerChecks']) is int and selection['pointerChecks'] > 0)
                    if (any(type(selection[key]) is not int or not 0 <= selection[key] <= 45 for key in pointer_fields)
                            or selection['pointerPositionMatches'] > selection['pointerChecks']
                            or selection['pointerChildMatches'] > selection['pointerPositionMatches']
                            or selection['cursorChecks'] > selection['pointerChildMatches'] + final_cursor_sample
                            or selection['cursorChecks'] > 0 and selection['pointerChildMatches'] == 0):
                        raise ValueError('invalid Zed pointer sample counters')
                if set(selection) in (classified, classified | pointer_fields):
                    classes = selection['cursorClasses']
                    if (type(classes) is not dict or set(classes) != {'hand', 'arrow', 'notallowed', 'transparent', 'unknown'}
                            or any(type(count) is not int or not 0 <= count <= (46 if pointer_proved else 19) for count in classes.values())
                            or sum(classes.values()) != selection['cursorChecks']
                            or type(selection['cursorSizeSource']) is not str
                            or selection['cursorSizeSource'] not in {'environment', 'resource', 'dpi', 'screen'}):
                        raise ValueError('invalid Zed cursor classification')
                record['cursorSelection'] = dict(selection)
            if present_modifiers:
                state = value['modifierState']
                if (type(state) is not str or state not in {'none', 'shift', 'control', 'lock', 'other-modifier', 'mixed', 'unknown'}
                        or (value['buttonsHeld'] is not None if state == 'unknown' else type(value['buttonsHeld']) is not bool)):
                    raise ValueError('invalid Zed pointer modifier observation')
                record.update(modifierState=state, buttonsHeld=value['buttonsHeld'])
            for key in flags:
                if value[key] is not None and type(value[key]) is not bool:
                    raise ValueError('invalid Zed pointer observation flag')
                record[key] = value[key]
            if type(value['pointerTarget']) is not str or value['pointerTarget'] not in {'unavailable', 'client', 'owned-frame', 'client-descendant', 'foreign'}:
                raise ValueError('invalid Zed pointer target')
            if type(value['pointerChild']) is not str or value['pointerChild'] not in {'unavailable', 'client', 'client-descendant', 'decoration-or-empty', 'other'}:
                raise ValueError('invalid Zed pointer child')
            record.update(diagnosticsOnly=True, pointerTarget=value['pointerTarget'], pointerChild=value['pointerChild'])
            if 'clientOriginVerified' in value:
                if ((value['clientOriginVerified'] is not None and type(value['clientOriginVerified']) is not bool)
                        or (value['retryOffsetRelation'] is not None and
                            (type(value['retryOffsetRelation']) is not str or value['retryOffsetRelation'] not in {
                                'expected-origin', 'missing-origin', 'inconsistent'}))):
                    raise ValueError('invalid Zed Retry coordinate observation')
                record.update({key: value[key] for key in coordinate_fields})
            if 'coordinateAuthority' in value:
                enums = {'coordinatePackage': {'noble-5build1', 'unverified'},
                         'coordinateRelation': {'equal', 'parent-offset', 'other'},
                         'coordinateAuthority': {'unchanged-xdotool', 'verified-xtranslate'}}
                if (any(value[key] is not None and (type(value[key]) is not str or value[key] not in allowed)
                        for key, allowed in enums.items())
                        or value['coordinateAuthority'] == 'verified-xtranslate' and (
                            value['coordinatePackage'] != 'noble-5build1' or value['coordinateRelation'] != 'parent-offset'
                            or value['clientOriginVerified'] is not False)
                        or value['coordinateAuthority'] == 'unchanged-xdotool' and (
                            value['coordinateRelation'] != 'equal' or value['clientOriginVerified'] is not True)):
                    raise ValueError('invalid Zed coordinate authority')
                record.update({key: value[key] for key in authority_fields})
            if 'inputDelivery' in value:
                delivery = value['inputDelivery']
                fields = {'status', 'pressCount', 'releaseCount', 'orderedPair'}
                statuses = {'complete', 'unavailable', 'timeout', 'query-failed', 'identity-failed'}
                stages = {'policy', 'budget-insufficient', 'request', 'library', 'display',
                          'record-version', 'xres-version', 'xinput-extension', 'client-query',
                          'client-identity', 'context', 'enable', 'identity-recheck',
                          'armed', 'observation', 'cleanup'}
                if (type(delivery) is not dict or set(delivery) not in (fields, fields | {'stage'})
                        or type(delivery['status']) is not str or delivery['status'] not in statuses):
                    raise ValueError('invalid Zed input delivery observation')
                if 'stage' in delivery and (type(delivery['stage']) is not str or delivery['stage'] not in stages):
                    raise ValueError('invalid Zed input delivery stage')
                if delivery['status'] == 'complete':
                    if (any(type(delivery[key]) is not int or not 0 <= delivery[key] <= 2
                            for key in ('pressCount', 'releaseCount'))
                            or type(delivery['orderedPair']) is not bool
                            or delivery['orderedPair'] and (delivery['pressCount'], delivery['releaseCount']) != (1, 1)):
                        raise ValueError('invalid Zed input delivery counts')
                elif any(delivery[key] is not None for key in fields - {'status'}):
                    raise ValueError('unmeasured Zed input delivery contains counts')
                record['inputDelivery'] = dict(delivery)
        elif mechanism == 'zed-atspi-retry':
            fields = set('schemaVersion mechanism diagnosticsOnly method stage actionAttempted forwarded'.split())
            if (set(value) != fields or app != 'zed-desktop' or value['diagnosticsOnly'] is not True
                    or value['method'] != 'atspi-click'
                    or type(value['stage']) is not str
                    or value['stage'] not in {'policy', 'request', 'preflight', 'action', 'forwarded', 'postflight'}
                    or type(value['actionAttempted']) is not bool or type(value['forwarded']) is not bool
                    or value['forwarded'] and not value['actionAttempted']
                    or value['stage'] in {'policy', 'request', 'preflight'} and value['actionAttempted']
                    or value['stage'] in {'forwarded', 'postflight'} and not value['forwarded']):
                raise ValueError('invalid Zed native action receipt')
            record.update(diagnosticsOnly=True, method=value['method'], stage=value['stage'],
                          actionAttempted=value['actionAttempted'], forwarded=value['forwarded'])
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
        elif mechanism == 'qualification-runner-failure':
            fields = {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'errorCategory'}
            if (set(value) != fields or value['diagnosticsOnly'] is not True
                    or type(value['errorCategory']) is not str or value['errorCategory'] not in RUNNER_FAILURES):
                raise ValueError('invalid qualification runner failure')
            record.update(diagnosticsOnly=True, errorCategory=value['errorCategory'])
        elif mechanism == 'codex-renderer-qualification':
            flags = set('endpointOwned targetVerified attached bindingVerified auxiliaryInert codingComposerReady uniqueComposer inputReadback inputSubmitted userTurnObserved responseVerified errorObserved retryControl retryAttempted retryCompleted providerResponseVerified'.split())
            fields = flags | set('schemaVersion mechanism diagnosticsOnly assistantTurnCount providerGenerationCount errorCategory'.split())
            errors = {None, 'ownership-lost', 'composer-unavailable', 'stale-turn', 'input-mismatch', 'action-uncertain', 'retry-unavailable', 'response-timeout', 'query-failed', 'invalid-request'}
            if (app != 'chatgpt-desktop' or set(value) != fields or value['diagnosticsOnly'] is not True
                    or any(type(value[key]) is not bool for key in flags)
                    or type(value['assistantTurnCount']) is not int or not 0 <= value['assistantTurnCount'] <= 4096
                    or value['providerGenerationCount'] is not None and (type(value['providerGenerationCount']) is not int or not 0 <= value['providerGenerationCount'] <= 4096)
                    or value['errorCategory'] is not None and type(value['errorCategory']) is not str
                    or value['errorCategory'] not in errors
                    or value['retryCompleted'] and not value['retryAttempted']):
                raise ValueError('invalid Codex renderer qualification')
            record.update({key: value[key] for key in fields - {'schemaVersion', 'mechanism'}})
        elif mechanism == 'renderer-inventory':
            fields = set('schemaVersion mechanism diagnosticsOnly app endpointOwned launcherOwned attached pageCount textareaCount editableCount sendCount retryCount newThreadCount loginCount dialogCount errorCategory'.split())
            if set(value) - {'documentState', 'startupScreen', 'landingCounts', 'onboardingCounts', 'publicOnboarding', 'mainAuxCorrelation', 'codexSession', 'initialMainBinding', 'initialMainConfirmation', 'sourceScreen', 'managedSignIn', 'sourceDialog', 'nativeOwnershipFailure', 'nativeListenerShape'} != fields or value['app'] != app or value['diagnosticsOnly'] is not True:
                raise ValueError('invalid renderer inventory identity')
            for key in ('endpointOwned', 'launcherOwned', 'attached'):
                flag(record, value, key)
            if 'nativeOwnershipFailure' in value:
                failure = value['nativeOwnershipFailure']
                if (type(failure) is not str or failure not in {'ancestor-unowned', 'ancestor-query',
                        'listener-unavailable', 'listener-shape', 'listener-unowned', 'listener-query', 'unmeasured'}):
                    raise ValueError('invalid native renderer ownership diagnostic')
                record['nativeOwnershipFailure'] = failure
            if 'nativeListenerShape' in value:
                shape = value['nativeListenerShape']
                if (value.get('nativeOwnershipFailure') != 'listener-shape' or type(shape) is not dict
                        or set(shape) != {'reason', 'listenerCount', 'uniquePidCount'}
                        or type(shape['reason']) is not str or shape['reason'] not in {
                            'unexpected-field', 'malformed-descriptor', 'missing-descriptor',
                            'endpoint-mismatch', 'multiple-listeners'}
                        or any(shape[key] is not None and (type(shape[key]) is not int or not 0 <= shape[key] <= 4096)
                               for key in ('listenerCount', 'uniquePidCount'))
                        or shape['listenerCount'] is not None and shape['uniquePidCount'] is not None
                            and shape['uniquePidCount'] > shape['listenerCount']):
                    raise ValueError('invalid native listener shape diagnostic')
                record['nativeListenerShape'] = dict(shape)
            if 'sourceDialog' in value:
                dialog = value['sourceDialog']
                keys = {'dialogs', 'workspaceFailureTitle', 'retryButton'}
                if (app != 'chatgpt-desktop' or type(dialog) is not dict
                        or set(dialog) != {'status', 'counts'} or type(dialog['counts']) is not dict
                        or set(dialog['counts']) != keys
                        or any(type(count) is not int or not 0 <= count <= 32 for count in dialog['counts'].values())):
                    raise ValueError('invalid Codex public dialog counts')
                counts = dialog['counts']
                expected = ('ambiguous' if any(count > 1 for count in counts.values()) else
                            'workspace-discovery-failed' if all(count == 1 for count in counts.values()) else 'unknown')
                if type(dialog['status']) is not str or dialog['status'] != expected:
                    raise ValueError('inconsistent Codex public dialog category')
                record['sourceDialog'] = dialog
            if 'managedSignIn' in value:
                requirements = value['managedSignIn']
                status_keys = {'loading', 'unsupported', 'disabled', 'error'}
                keys = status_keys | {'chatgptChoice', 'apiKeyChoice'}
                if (app != 'chatgpt-desktop' or type(requirements) is not dict
                        or set(requirements) != {'status', 'counts'}
                        or type(requirements['counts']) is not dict or set(requirements['counts']) != keys
                        or any(type(count) is not int or not 0 <= count <= 32
                               for count in requirements['counts'].values())):
                    raise ValueError('invalid Codex managed sign-in observation')
                counts = requirements['counts']
                statuses = [key for key in status_keys if counts[key] > 0]
                choices = counts['chatgptChoice'] + counts['apiKeyChoice']
                expected = ('ambiguous' if any(count > 1 for count in counts.values())
                            else ('sign-in-options' if choices > 0 else 'unknown') if not statuses
                            else statuses[0] if len(statuses) == 1 and choices == 0 else 'ambiguous')
                if type(requirements['status']) is not str or requirements['status'] != expected:
                    raise ValueError('inconsistent Codex managed sign-in observation')
                record['managedSignIn'] = requirements
            if 'sourceScreen' in value:
                screen = value['sourceScreen']
                headings = dict(gatewayHeading='gateway-connect', recoveryHeading='app-recovery',
                                importHeading='external-import', allSetHeading='all-set',
                                permissionHeading='permission-setup')
                keys = set(headings) | {'continueSignIn'}
                if (app != 'chatgpt-desktop' or type(screen) is not dict
                        or set(screen) != {'status', 'counts'}
                        or type(screen['counts']) is not dict or set(screen['counts']) != keys
                        or any(type(count) is not int or not 0 <= count <= 32
                               for count in screen['counts'].values())):
                    raise ValueError('invalid Codex source screen')
                present = [key for key in headings if screen['counts'][key] > 0]
                expected = ('unknown' if not present else headings[present[0]]
                            if len(present) == 1 and screen['counts'][present[0]] == 1 else 'ambiguous')
                if type(screen['status']) is not str or screen['status'] != expected:
                    raise ValueError('inconsistent Codex source screen')
                record['sourceScreen'] = screen
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
            if 'initialMainBinding' in value:
                binding = value['initialMainBinding']
                flags = {'targetPresent', 'framePresent', 'loaderPresent'}
                if (app != 'chatgpt-desktop' or type(binding) is not dict
                        or set(binding) != flags | {'status', 'route'}
                        or any(type(binding[key]) is not bool for key in flags)
                        or type(binding['status']) is not str or binding['status'] not in {
                            'unmeasured', 'captured', 'deadline', 'ownership-lost', 'page-count',
                            'route-rejected', 'identity-query-failed', 'identity-rejected',
                            'identity-changed', 'query-failed'}
                        or type(binding['route']) is not str or binding['route'] not in {
                            'unmeasured', 'blank', 'primary', 'primary-query',
                            'primary-fragment', 'other-app', 'other'}
                        or binding['status'] == 'captured' and (
                            binding['route'] != 'primary' or not all(binding[key] for key in flags))):
                    raise ValueError('invalid Codex initial main binding')
                record['initialMainBinding'] = binding
            if 'initialMainConfirmation' in value:
                confirmation = value['initialMainConfirmation']
                flags = {'identityUnchanged', 'mainScopeUnique', 'documentFocused'}
                count_keys = set('roleLegend roleRadios engineering dialog quickChatComposer editable'.split())
                if (app != 'chatgpt-desktop' or type(confirmation) is not dict
                        or set(confirmation) != flags | {'status', 'counts'}
                        or any(confirmation[key] is not None and type(confirmation[key]) is not bool for key in flags)
                        or type(confirmation['status']) is not str or confirmation['status'] not in {
                            'unmeasured', 'initial-missing', 'deadline', 'ownership-lost', 'identity-changed',
                            'source-scope', 'document-unfocused', 'guard-rejected', 'confirmed', 'query-failed'}
                        or confirmation['counts'] is not None and (type(confirmation['counts']) is not dict
                            or set(confirmation['counts']) != count_keys
                            or any(type(count) is not int or not 0 <= count <= 4096 for count in confirmation['counts'].values()))
                        or confirmation['status'] == 'confirmed' and (not all(confirmation[key] is True for key in flags)
                            or confirmation['counts'] is None
                            or any(confirmation['counts'][key] != count for key, count in
                                {'roleLegend': 1, 'roleRadios': 11, 'engineering': 1}.items()))):
                    raise ValueError('invalid Codex initial main confirmation')
                record['initialMainConfirmation'] = confirmation
            if 'mainAuxCorrelation' in value:
                record['mainAuxCorrelation'] = main_aux_correlation(value['mainAuxCorrelation'], app)
            if 'codexSession' in value:
                session = value['codexSession']
                flags = {'bindingVerified', 'codingComposerReady', 'auxiliaryInert'}
                if (app != 'chatgpt-desktop' or type(session) is not dict or set(session) != flags | {'pageCount'}
                        or any(type(session[key]) is not bool for key in flags)
                        or type(session['pageCount']) is not int or not 0 <= session['pageCount'] <= 32
                        or session['codingComposerReady'] and not (session['bindingVerified'] and session['auxiliaryInert'])):
                    raise ValueError('invalid Codex session observation')
                record['codexSession'] = dict(session)
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
        elif mechanism == 'windows-process-baseline':
            fields = set('schemaVersion mechanism diagnosticsOnly app phase state'.split())
            if (set(value) != fields or app != 'claude-desktop' or value['app'] != app
                    or value['diagnosticsOnly'] is not True or value['phase'] != 'before-launch'
                    or type(value['state']) is not str or value['state'] not in {'present', 'absent', 'query-failed'}):
                raise ValueError('invalid Windows process baseline')
            record.update({k: value[k] for k in fields - {'schemaVersion', 'mechanism'}})
        elif mechanism == 'zed-retry-visual':
            counts = set('copyMatches closeMatches baselinePairs firstPairs secondPairs newStablePairs retryCorrelations'.split())
            fields = counts | set('schemaVersion mechanism diagnosticsOnly status reason templateSide relation'.split())
            if (app != 'zed-desktop' or set(value) != fields or value['diagnosticsOnly'] is not True
                    or any(type(value[k]) is not int or not 0 <= value[k] <= 32 for k in counts)):
                raise ValueError('invalid Zed Retry visual observation')
            enum(record, value, 'status', {'complete', 'unsupported', 'query-error'})
            enum(record, value, 'reason', REASONS)
            relation = value['relation']
            if type(relation) is not str or relation not in {'unavailable', 'no-pair', 'ambiguous', 'left-same-row', 'mismatch'}:
                raise ValueError('invalid Zed Retry visual relation')
            stable, correlated = value['newStablePairs'], value['retryCorrelations']
            if (stable > min(value['firstPairs'], value['secondPairs']) or correlated > stable
                    or type(value['templateSide']) is not int):
                raise ValueError('inconsistent Zed Retry visual counts')
            if value['status'] == 'complete':
                expected_relation = 'no-pair' if stable == 0 else ('left-same-row' if correlated == 1 else 'mismatch') if stable == 1 else 'ambiguous'
                if value['reason'] is not None or value['templateSide'] not in {14, 28} or relation != expected_relation:
                    raise ValueError('inconsistent complete Zed Retry visual observation')
            elif (value['reason'] is None or value['templateSide'] != 0 or relation != 'unavailable'
                  or any(value[k] != 0 for k in counts)):
                raise ValueError('inconsistent unavailable Zed Retry visual observation')
            record.update({k: value[k] for k in fields - {'schemaVersion', 'mechanism'}})
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
            if 'toolResult' in value:
                result = value['toolResult']
                keys = {'selectedTool', 'resultPresent', 'resultCount', 'status',
                        'shape', 'toolErrorDetected', 'errorCategory'}
                if value['stage'] != 'tool' or type(result) is not dict or set(result) != keys:
                    raise ValueError('invalid tool result observation')
                closed = {}
                enum(closed, result, 'selectedTool', {'read', 'read-file', 'read-files', 'exec-command'})
                enum(closed, result, 'status', {'complete', 'limit'})
                enum(closed, result, 'shape', {'absent', 'string', 'text-array', 'mixed', 'unsupported'})
                enum(closed, result, 'errorCategory', {'none', 'file-not-found', 'file-too-large',
                                                     'read-budget', 'directory', 'unknown'})
                flag(closed, result, 'resultPresent')
                flag(closed, result, 'toolErrorDetected')
                count = result['resultCount']
                if type(count) is not int or not 0 <= count <= 32:
                    raise ValueError('invalid tool result count')
                if result['resultPresent'] != (count > 0):
                    raise ValueError('inconsistent tool result presence')
                if (result['shape'] == 'absent') != (count == 0):
                    raise ValueError('inconsistent tool result shape')
                if result['toolErrorDetected'] != (result['errorCategory'] != 'none'):
                    raise ValueError('inconsistent tool error observation')
                if result['toolErrorDetected'] and not result['resultPresent']:
                    raise ValueError('tool error without result')
                closed['resultCount'] = count
                record['toolResult'] = closed
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
            enum(record, value, 'retryActionReceipt', {'acknowledged', 'completion-unknown', 'native-pointer-dispatched', 'native-atspi-forwarded'})
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
        result['nativeDiagnosticInvalidEvents'] = bundle['invalidEvents']
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
    semantic_pairs = {
        'native-thread-export': {('native-clipboard-and-keyboard', 'native-thread-export')},
        'native-assistant-clipboard': {('native-clipboard-and-keyboard', 'native-assistant-clipboard')},
        'renderer-dom': {('renderer-dom-and-keyboard', 'renderer-dom')},
    }.get(result['backend'], set())
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


def aggregate(directory, source_sha, excluded=()):
    records = [bounded_json(path) for path in Path(directory).glob('**/qualification.json')]
    expected = {(item['app'], item['platform'], item['architecture']) for item in matrix(excluded)['include']}
    observed = [(item.get('app'), item.get('platform'), item.get('architecture')) for item in records]
    if len(observed) != len(expected) or len(set(observed)) != len(expected) or set(observed) != expected:
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
        invalid = item['nativeDiagnosticInvalidEvents']
        if invalid is not None and (type(invalid) is not int or not 0 <= invalid <= 4 * 1024 * 1024 + 1):
            raise ValueError('invalid native diagnostic rejection count')
    result = dict(schemaVersion=1, sourceSha=source_sha, qualification='deterministic-full'
                if all(item['qualification'] == 'deterministic-full' and item['outcome'] == 'passed'
                       for item in records) else 'incomplete', cells=records)
    if excluded:
        result['excludedApps'] = sorted(excluded)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=('matrix', 'pending', 'reduce', 'aggregate'))
    parser.add_argument('--exclude-app', action='append', choices=APPS, default=[])
    for name in ('app', 'platform', 'architecture', 'source-sha', 'output', 'model', 'frozen', 'prepared',
                 'checker', 'launcher', 'real-nanh', 'report', 'directory', 'facts'):
        parser.add_argument('--' + name)
    args = parser.parse_args()
    try:
        if args.command == 'matrix':
            print(json.dumps(matrix(args.exclude_app), separators=(',', ':')))
            return
        required = (['directory', 'source_sha', 'output'] if args.command == 'aggregate' else
                    ['app', 'platform', 'architecture', 'source_sha', 'output'])
        if args.command == 'reduce':
            required += ['model', 'frozen', 'prepared', 'checker', 'launcher', 'real_nanh', 'report']
        if any(getattr(args, key) is None for key in required):
            raise ValueError('required cell evidence is missing')
        result = (aggregate(args.directory, args.source_sha, args.exclude_app) if args.command == 'aggregate' else
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
