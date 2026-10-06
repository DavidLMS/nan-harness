"""Validate only closed native-copy, startup and DOM observations for publication."""
import re
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "canary/actions"))
from desktop_diagnostics import CATEGORIES

DOM_ERRORS = set('unclassified invalid-request launcher-unowned endpoint-unowned target-ambiguous target-invalid composer-ambiguous send-unavailable stale-response input-mismatch response-timeout submit-action-timeout submit-action-intercepted submit-action-detached submit-action-failed response-observation-failed attachment-or-action-failed'.split())
from desktop_startup import startup


def shape(value, keys, mechanism):
    if type(value) is not dict or set(value) != set(keys.split()) or type(value['schemaVersion']) is not int or value['schemaVersion'] != 1 or value['mechanism'] != mechanism:
        raise ValueError('invalid closed experiment schema')


def flags(value, keys):
    if any(type(value[key]) is not bool for key in keys.split()):
        raise ValueError('invalid closed experiment flag')


def native_copy(value, reasons):
    value = dict(value)
    history = {}
    for key, bound in [('exportResumeCount', 128), ('exportAgentCount', 128), ('exportTotalAssistantTextCount', 16384)]:
        if key in value:
            count = value.pop(key)
            if type(count) is not int or not 0 <= count <= bound:
                raise ValueError('invalid closed export history count')
            history[key] = count
    shape(value, 'schemaVersion mechanism experimentOnly ocrUsed axTextUsed navigation keyboardTransport responseMethod lastExportError lastExportTransportError exportVersion exportUserCount exportAssistantTextCount stage substage guardKind guardCategory settleObservations clipboardReadback clipboardCharacterCount blocker trustControlCount panelControlCount responseControlCount clipboardCleanup input response', 'zed-native-copy')
    flags(value, 'experimentOnly ocrUsed axTextUsed')
    if not value['experimentOnly'] or value['ocrUsed'] or value['axTextUsed'] or value['navigation'] != 'private-keymap-new-thread':
        raise ValueError('invalid native-copy method')
    if value['stage'] not in {'trust', 'panel', 'input', 'submit', 'response-control', 'response-readback', 'completed'} or value['clipboardCleanup'] not in {'passed', 'failed', 'not-run'}:
        raise ValueError('invalid native-copy stage')
    if value['keyboardTransport'] not in {'xa11y', 'neutral-quartz', 'neutral-quartz-all', 'neutral-quartz-paste'}:
        raise ValueError('invalid keyboard transport')
    if value['responseMethod'] not in {'native-copy', 'thread-export'}:
        raise ValueError('invalid native response method')
    if value['lastExportError'] not in {None, 'request', 'schema', 'user-mismatch', 'assistant-mismatch', 'decompression'} or value['exportVersion'] not in {None, '1.0.0'}:
        raise ValueError('invalid export diagnostic')
    if value['lastExportTransportError'] not in {None, 'configuration', 'spawn', 'pipes', 'timeout', 'wait', 'write', 'read', 'exit', 'output-budget', 'json', 'verdict'}:
        raise ValueError('invalid export transport diagnostic')
    for key in ('exportUserCount', 'exportAssistantTextCount'):
        count = value[key]
        if count is not None and (type(count) is not int or not 0 <= count <= 128):
            raise ValueError('invalid export count')
    substages = set('trust-query trust-before trust-after panel-query new-thread-before new-thread-after panel-settle select-all-before select-all-after type-before type-after paste-before paste-after paste-settle input-sentinel-write copy-select-all-before copy-select-all-after input-copy-before input-copy-after collapse-selection-before submit-before response-control-query response-sentinel-write response-copy-before response-copy-after clipboard-read-before clipboard-read-after export-copy-before export-copy-after export-read-before export-read-after export-parse completed'.split())
    if value['substage'] not in substages or value['guardKind'] not in {None, 'native-window', 'direct-foreground'}:
        raise ValueError('invalid copy boundary')
    if value['guardCategory'] is not None and value['guardCategory'] not in CATEGORIES:
        raise ValueError('invalid copy guard category')
    if (value['guardKind'] is None) != (value['guardCategory'] is None) or type(value['settleObservations']) is not int or not 0 <= value['settleObservations'] <= 3:
        raise ValueError('invalid copy guard observation')
    if value['clipboardReadback'] not in {None, 'sentinel', 'empty', 'exact', 'nonmatching'}:
        raise ValueError('invalid clipboard readback category')
    count = value['clipboardCharacterCount']
    if count is not None and (type(count) is not int or not 0 <= count <= 65536):
        raise ValueError('invalid clipboard size')
    if value['blocker'] is not None and value['blocker'] not in reasons:
        raise ValueError('invalid native-copy blocker')
    for key in ('trustControlCount', 'panelControlCount', 'responseControlCount'):
        count = value[key]
        if count is not None and (type(count) is not int or not 0 <= count <= 4096):
            raise ValueError('invalid control count')
    for key, fields in [('input', 'entered clipboardVerified submitted'), ('response', 'copyAction clipboardVerified providerVerified providerGenerationCount')]:
        if type(value[key]) is not dict or set(value[key]) != set(fields.split()):
            raise ValueError('invalid copy observation')
        flags(value[key], fields.replace(' providerGenerationCount', ''))
    count = value['response']['providerGenerationCount']
    if count is not None and (type(count) is not int or not 0 <= count <= 4096):
        raise ValueError('invalid provider count')
    if value['response']['providerVerified'] and (count is None or count < 1):
        raise ValueError('provider proof without generation')
    if value['input']['submitted'] and not value['input']['clipboardVerified']:
        raise ValueError('submission without verified input')
    if value['responseMethod'] == 'thread-export' and value['response']['clipboardVerified'] and not (
            value['lastExportError'] is None and value['lastExportTransportError'] is None and value['exportVersion'] == '1.0.0'
            and value['exportUserCount'] == 1 and value['exportAssistantTextCount'] == 1):
        raise ValueError('invalid assistant export proof')
    if value['response']['clipboardVerified'] and not (value['input']['submitted'] and value['response']['copyAction']):
        raise ValueError('response without native copy')
    return {**value, **history}


def dom(value):
    # A guard may stop the checker before it adds the independent provider fact.
    value = dict(value)
    value.setdefault('providerResponseVerified', False)
    value.setdefault('providerGenerationCount', None)
    shape(value, 'schemaVersion mechanism endpointOwned attached uniqueComposer inputReadback syntheticTextPresent targetVerified responseVerified inputSubmitted errorCategory playwrightVersion observedRuntimeVersion providerResponseVerified providerGenerationCount inputCleared userTurnObserved assistantTurnCount uniqueSendControl canSend sendBlocker sendMechanism requestFailedCount requestFailureCategory apiErrorStatus apiErrorResponseCount', 'hermes-playwright-dom')
    flags(value, 'endpointOwned attached uniqueComposer inputReadback syntheticTextPresent targetVerified responseVerified inputSubmitted providerResponseVerified inputCleared userTurnObserved uniqueSendControl canSend')
    for key in ('assistantTurnCount', 'requestFailedCount', 'apiErrorResponseCount'):
        if type(value[key]) is not int or not 0 <= value[key] <= 4096:
            raise ValueError('invalid DOM observation count')
    if value['sendBlocker'] not in {None, 'modal', 'menu', 'tooltip', 'composer-drag-region', 'other', 'unmeasured', 'focus', 'disabled', 'inert'}:
        raise ValueError('invalid Send blocker')
    if value['sendMechanism'] not in {'pointer', 'semantic-keyboard'}:
        raise ValueError('invalid Send mechanism')
    if value['inputSubmitted'] and value['sendBlocker'] is not None:
        raise ValueError('DOM submission through blocked control')
    if value['requestFailureCategory'] not in {None, 'aborted', 'connection', 'tls', 'other'}:
        raise ValueError('invalid request failure category')
    status = value['apiErrorStatus']
    if status is not None and (type(status) is not int or not 400 <= status <= 599):
        raise ValueError('invalid API error status')
    count = value['providerGenerationCount']
    if count is not None and (type(count) is not int or not 0 <= count <= 4096):
        raise ValueError('invalid provider count')
    if value['providerResponseVerified'] and (count is None or count < 1):
        raise ValueError('provider proof without generation')
    if value['errorCategory'] is not None and value['errorCategory'] not in DOM_ERRORS:
        raise ValueError('invalid DOM error')
    for key in ('playwrightVersion', 'observedRuntimeVersion'):
        if value[key] is not None and (type(value[key]) is not str or not re.fullmatch(r'[A-Za-z0-9. -]{1,64}', value[key])):
            raise ValueError('invalid observed version')
    if value['inputSubmitted'] and not all(value[key] for key in ('endpointOwned', 'targetVerified', 'attached', 'uniqueComposer', 'inputReadback', 'uniqueSendControl', 'canSend')):
        raise ValueError('DOM submission before preconditions')
    if value['responseVerified'] and (not value['inputSubmitted'] or not value['syntheticTextPresent'] or value['errorCategory'] is not None):
        raise ValueError('invalid DOM response')
    return value
