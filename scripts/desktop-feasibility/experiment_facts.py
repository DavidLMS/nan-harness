"""Validate only closed native-copy, startup and DOM observations for publication."""
import re
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "canary/actions"))
from desktop_diagnostics import CATEGORIES

DOM_ERRORS = set('unclassified invalid-request launcher-unowned endpoint-unowned target-ambiguous target-invalid composer-ambiguous stale-response input-mismatch response-timeout attachment-or-action-failed'.split())
STARTUP_CATEGORIES = set('sandbox-helper namespace-denied root-without-sandbox display-unavailable missing-library gpu-fatal native-module unclassified'.split())


def shape(value, keys, mechanism):
    if type(value) is not dict or set(value) != set(keys.split()) or type(value['schemaVersion']) is not int or value['schemaVersion'] != 1 or value['mechanism'] != mechanism:
        raise ValueError('invalid closed experiment schema')


def flags(value, keys):
    if any(type(value[key]) is not bool for key in keys.split()):
        raise ValueError('invalid closed experiment flag')


def native_copy(value, reasons):
    shape(value, 'schemaVersion mechanism experimentOnly ocrUsed axTextUsed navigation stage substage guardKind guardCategory settleObservations blocker trustControlCount panelControlCount responseControlCount clipboardCleanup input response', 'zed-native-copy')
    flags(value, 'experimentOnly ocrUsed axTextUsed')
    if not value['experimentOnly'] or value['ocrUsed'] or value['axTextUsed'] or value['navigation'] != 'private-keymap-new-thread':
        raise ValueError('invalid native-copy method')
    if value['stage'] not in {'trust', 'panel', 'input', 'submit', 'response-control', 'response-readback', 'completed'} or value['clipboardCleanup'] not in {'passed', 'failed', 'not-run'}:
        raise ValueError('invalid native-copy stage')
    substages = set('trust-query trust-before trust-after panel-query new-thread-before new-thread-after panel-settle select-all-before select-all-after type-before type-after input-sentinel-write copy-select-all-before copy-select-all-after input-copy-before input-copy-after collapse-selection-before submit-before response-control-query response-sentinel-write response-copy-before response-copy-after clipboard-read-before clipboard-read-after completed'.split())
    if value['substage'] not in substages or value['guardKind'] not in {None, 'native-window', 'direct-foreground'}:
        raise ValueError('invalid copy boundary')
    if value['guardCategory'] is not None and value['guardCategory'] not in CATEGORIES:
        raise ValueError('invalid copy guard category')
    if (value['guardKind'] is None) != (value['guardCategory'] is None) or type(value['settleObservations']) is not int or not 0 <= value['settleObservations'] <= 3:
        raise ValueError('invalid copy guard observation')
    if value['blocker'] is not None and value['blocker'] not in reasons:
        raise ValueError('invalid native-copy blocker')
    for key in ('trustControlCount', 'panelControlCount', 'responseControlCount'):
        count = value[key]
        if count is not None and (type(count) is not int or not 0 <= count <= 4096):
            raise ValueError('invalid control count')
    for key, fields in [('input', 'entered clipboardVerified submitted'), ('response', 'copyAction clipboardVerified providerVerified')]:
        if type(value[key]) is not dict or set(value[key]) != set(fields.split()):
            raise ValueError('invalid copy observation')
        flags(value[key], fields)
    if value['input']['submitted'] and not value['input']['clipboardVerified']:
        raise ValueError('submission without verified input')
    if value['response']['clipboardVerified'] and not (value['input']['submitted'] and value['response']['copyAction']):
        raise ValueError('response without native copy')
    return value


def startup(value):
    shape(value, 'schemaVersion mechanism startupCategory namespacePolicy disableSetuidSandbox stderrPresent captureTruncated drainComplete launcherExitCode effectiveUserIsRoot apparmor_restrict_unprivileged_userns unprivileged_userns_clone sandboxHelperPresent sandboxHelperOwnerIsRoot sandboxHelperModeIs4755', 'hermes-startup')
    flags(value, 'stderrPresent captureTruncated drainComplete disableSetuidSandbox')
    if value['namespacePolicy'] not in {'default', 'scoped-apparmor-userns'} or value['disableSetuidSandbox'] and value['namespacePolicy'] != 'scoped-apparmor-userns':
        raise ValueError('invalid namespace experiment policy')
    if value['startupCategory'] not in STARTUP_CATEGORIES:
        raise ValueError('invalid startup category')
    if (value['captureTruncated'] or not value['drainComplete']) and value['startupCategory'] != 'unclassified':
        raise ValueError('classification from incomplete stderr')
    for key in ('effectiveUserIsRoot', 'sandboxHelperPresent', 'sandboxHelperOwnerIsRoot', 'sandboxHelperModeIs4755'):
        if value[key] is not None and type(value[key]) is not bool:
            raise ValueError('invalid startup fact')
    for key in ('apparmor_restrict_unprivileged_userns', 'unprivileged_userns_clone'):
        if value[key] is not None and (type(value[key]) is not int or value[key] not in (0, 1)):
            raise ValueError('invalid namespace policy')
    code = value['launcherExitCode']
    if code is not None and (type(code) is not int or not -127 <= code <= 255):
        raise ValueError('invalid launcher exit')
    return value


def dom(value):
    # A guard may stop the checker before it adds the independent provider fact.
    value = dict(value)
    value.setdefault('providerResponseVerified', False)
    shape(value, 'schemaVersion mechanism endpointOwned attached uniqueComposer inputReadback syntheticTextPresent targetVerified responseVerified inputSubmitted errorCategory playwrightVersion observedRuntimeVersion providerResponseVerified', 'hermes-playwright-dom')
    flags(value, 'endpointOwned attached uniqueComposer inputReadback syntheticTextPresent targetVerified responseVerified inputSubmitted providerResponseVerified')
    if value['errorCategory'] is not None and value['errorCategory'] not in DOM_ERRORS:
        raise ValueError('invalid DOM error')
    for key in ('playwrightVersion', 'observedRuntimeVersion'):
        if value[key] is not None and (type(value[key]) is not str or not re.fullmatch(r'[A-Za-z0-9. -]{1,64}', value[key])):
            raise ValueError('invalid observed version')
    if value['inputSubmitted'] and not all(value[key] for key in ('endpointOwned', 'targetVerified', 'attached', 'uniqueComposer', 'inputReadback')):
        raise ValueError('DOM submission before preconditions')
    if value['responseVerified'] and (not value['inputSubmitted'] or not value['syntheticTextPresent'] or value['errorCategory'] is not None):
        raise ValueError('invalid DOM response')
    return value
