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
BACKENDS[('claude-desktop', 'windows', 'x86_64')] = 'native-assistant-clipboard'
BACKENDS[('claude-desktop', 'linux', 'x86_64')] = 'native-assistant-clipboard'
BACKENDS[('chatgpt-desktop', 'windows', 'x86_64')] = 'renderer-dom'
BACKENDS[('chatgpt-desktop', 'linux', 'x86_64')] = 'renderer-dom'
BACKENDS[('chatgpt-desktop', 'macos', 'aarch64')] = 'renderer-dom'
RUNNER_FAILURES = set('windows-ownership-helper-missing windows-ownership-helper-invalid prepared-identity-mismatch prepared-app-unavailable prepared-executable-missing prepared-executable-changed host-platform-mismatch backend-unavailable frozen-app-unavailable report-absent claude-windows-executable-invalid claude-windows-bootstrap-invalid claude-windows-bootstrap-mismatch claude-windows-release-mismatch claude-windows-policy-invalid codex-project-release-mismatch claude-persist-policy-invalid claude-persist-source-mismatch invalid-preflight execution-failed'.split())
PREPARATION_FAILURES = set('receipt-shape unclassified installation-unavailable installation-unreadable installation-ambiguous installation-failed unsupported-version version-unknown'.split())
STEPS = {'launched', 'input-submitted', 'response-verified', 'tool-verified', 'error-recovered'}
COMMIT = re.compile(r'[0-9a-f]{40}\Z')
HASH = re.compile(r'[0-9a-f]{64}\Z')
VERSION = re.compile(r'[0-9]+(?:\.[0-9]+){2}(?:[-+][A-Za-z0-9.-]+)?\Z')


def claude_native_tree(value):
    fields = {'operation','reason','nodeScope','foreignBus','childCount','visitedCount'}
    choices = {'operation': {'owner','children','identity'}, 'reason': {'wrong-owner','query-unavailable','deadline','null-reference','non-list','limit','duplicate'},
               'nodeScope': {'frame','editor','other'}}
    if (type(value) is not dict or set(value) - {'transportReason'} != fields
            or any(type(value[k]) is not str or value[k] not in choices[k] for k in choices)
            or type(value['foreignBus']) is not bool
            or any(value[k] is not None and (type(value[k]) is not int or not 0 <= value[k] <= n)
                   for k,n in [('childCount',1025),('visitedCount',1024)])):
        raise ValueError('invalid Claude native tree observation')
    if 'transportReason' in value and (value['reason'] not in {'query-unavailable','deadline','null-reference'}
            or type(value['transportReason']) is not str or value['transportReason'] not in {
                'name-unowned','service-unknown','object-unknown','interface-unknown','method-unknown',
                'no-reply','timeout','disconnected','failed','other'}):
        raise ValueError('invalid Claude native transport observation')
    return value


def codex_windows_prepare(value):
    fields = set('schemaVersion mechanism diagnosticsOnly stage cause bindingIndex ancestorCount ownedCount privacy emptyRoots codeHomeAbsent completed'.split())
    stages = set('policy command ancestor-acquisition owned-acquisition privacy empty-roots code-home final-custody completed retained-custody'.split())
    causes = set('policy arguments cwd binding original-cutoff ancestor-budget directory-missing directory-access directory-sharing directory-open directory-metadata directory-reparse directory-type privacy directory-enumeration root-populated code-home-metadata code-home-present custody'.split())
    if (type(value) is not dict or set(value) != fields or type(value['schemaVersion']) is not int
            or value['schemaVersion'] != 1 or value['mechanism'] != 'codex-windows-profile-prepare'
            or value['diagnosticsOnly'] is not True or type(value['stage']) is not str or value['stage'] not in stages
            or value['cause'] is not None and (type(value['cause']) is not str or value['cause'] not in causes)
            or value['bindingIndex'] is not None and (type(value['bindingIndex']) is not int or not 0 <= value['bindingIndex'] <= 7)
            or any(type(value[k]) is not int or not 0 <= value[k] <= n for k,n in [('ancestorCount',64),('ownedCount',10)])
            or type(value['privacy']) is not list or len(value['privacy']) != 11
            or any(v is not None and (type(v) is not str or v not in {'protected','inherited','unexpected','unavailable'}) for v in value['privacy'])
            or type(value['emptyRoots']) is not list or len(value['emptyRoots']) != 2
            or any(v is not None and type(v) is not bool for v in value['emptyRoots'])
            or value['codeHomeAbsent'] is not None and type(value['codeHomeAbsent']) is not bool
            or type(value['completed']) is not bool or value['completed'] != (value['stage'] == 'completed')
            or (value['cause'] is None) != value['completed']):
        raise ValueError('invalid Codex Windows profile preparation')
    if value['completed'] and not (value['ancestorCount'] > 0 and value['ownedCount'] == 10
            and value['privacy'] == ['protected'] * 11 and value['emptyRoots'] == [True, True]
            and value['codeHomeAbsent'] is True and value['bindingIndex'] is None):
        raise ValueError('unproved Codex Windows profile preparation')
    return value


def claude_retry_candidate(value):
    counts = set('pendingUserCount conversationHeadingCount tryAgainCount retryCount candidateRowHeadingCount'.split())
    flags = set('historyMatched candidateEnabled candidateActionUnique candidateHitMatched candidateUserAncestorMatched'.split())
    if (type(value) is not dict or set(value) != counts | flags | {'candidateLabel'}
            or any(type(value[k]) is not int or not 0 <= value[k] <= 32 for k in counts)
            or any(type(value[k]) is not bool for k in flags)):
        raise ValueError('invalid Claude retry candidate')
    pair = (value['tryAgainCount'], value['retryCount'])
    total = sum(pair)
    label = {(0, 0): 'none', (1, 0): 'try-again', (0, 1): 'retry'}.get(pair, 'ambiguous')
    if (total > 32 or value['candidateLabel'] != label
            or total != 1 and (any(value[k] for k in flags - {'historyMatched'})
                              or value['candidateRowHeadingCount'] != 0)
            or value['candidateUserAncestorMatched'] and value['pendingUserCount'] != 1):
        raise ValueError('inconsistent Claude retry candidate')
    return value


def main_aux_correlation(value, app):
    flags = set('heldMainUnchanged auxRouteMatched mainScopeUnique auxMainControlsAbsent auxComposerAbsent guarded'.split())
    focus = {'mainDocumentFocused', 'auxDocumentFocused'}
    fields = flags | focus | set('schemaVersion mechanism diagnosticsOnly status totalPages stableSamples main aux'.split())
    statuses = {'observed', 'initial-main-unavailable', 'page-count', 'source-scope',
                'identity-changed', 'ownership-lost', 'query-failed', 'deadline'}
    if (app != 'chatgpt-desktop' or type(value) is not dict or set(value) - {'inputChannel'} != fields
            or type(value.get('inputChannel', 'native-focused')) is not str
            or value.get('inputChannel', 'native-focused') not in {'native-focused', 'cdp-dom'}
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
            and all(value[key] for key in flags)
            and (value['mainDocumentFocused'] is True or value.get('inputChannel') == 'cdp-dom'
                 and value['mainDocumentFocused'] is False)
            and value['auxDocumentFocused'] is False and value['main'] is not None and value['aux'] is not None):
        raise ValueError('unproved Codex main auxiliary correlation')
    return value


def task_scope_observation(value):
    flags = {'heldScopeConnected', 'heldScopeVisible'}
    counts = {'roleRadioCount', 'exactAckLeafCount', 'exactGetStartedCount'}
    if (type(value) is not dict or set(value) != flags | counts
            or any(type(value[key]) is not bool for key in flags)
            or value['heldScopeVisible'] and not value['heldScopeConnected']
            or any(value[key] is not None and (type(value[key]) is not int
                   or not 0 <= value[key] <= 4096) for key in counts)):
        raise ValueError('invalid held task scope diagnostic')
    available = sum(value[key] is not None for key in counts)
    if available not in {0, 3} or not value['heldScopeVisible'] and available:
        raise ValueError('inconsistent held task scope diagnostic')
    return value


def codex_public_dom(value):
    categories=set('codexHomeCount codexThreadCount codexOtherCount classicChatGPTCount genericInputCount genericBodyCount unboundCount'.split())
    shapes={
        'navigation':(set('codexButtonCount codexLinkCount codexMenuItemCount chatModeTriggerCount codexModeTriggerCount projectSelectorCount newChatCount projectsLinkCount'.split()),{'uniqueCodexRole','uniqueCodexHitActionable'}),
        'editable':(categories|{'editableCount','sidebarNewChatCount'},{'sidebarNewChatHitActionable'}),
        'home':(set('homeComposerCount pendingTextareaCount pendingGroupCount proseMirrorEditableCount enabledSendCount disabledSendCount workspaceControlCount'.split()),set())}
    if type(value) is not dict or set(value)!=set(shapes):
        raise ValueError('invalid Codex public DOM shape')
    for key,(counts,other) in shapes.items():
        row=value[key]
        if type(row) is not dict or set(row)!=counts|other|{'status'} or row['status'] not in ('observed','overflow'):
            raise ValueError('invalid Codex public DOM row')
        if row['status']=='overflow':
            if any(row[k] is not None for k in counts|other):
                raise ValueError('invalid Codex public DOM overflow')
            continue
        if any(type(row[k]) is not int or not 0<=row[k]<=32 for k in counts):
            raise ValueError('invalid Codex public DOM counts')
        if key=='navigation':
            roles={'button':'codexButtonCount','link':'codexLinkCount','menuitem':'codexMenuItemCount'}
            total=sum(row[k] for k in roles.values())
            role=row['uniqueCodexRole'];hit=row['uniqueCodexHitActionable']
            if (type(role) is not str or role not in {*roles,'none'} or type(hit) is not bool
                    or (total==1)!=(role!='none') or total==1 and row[roles[role]]!=1 or hit and total!=1):
                raise ValueError('invalid Codex public navigation identity')
        if key=='editable' and (sum(row[k] for k in categories)!=row['editableCount']
                or type(row['sidebarNewChatHitActionable']) is not bool
                or row['sidebarNewChatHitActionable'] and row['sidebarNewChatCount']!=1):
            raise ValueError('invalid Codex public editable partition')
    return value


def zed_xi2_motion(value):
    counts={'targetPointCount','ownedMotionCount','motionWithXYCount','retainedPointMatchedCount'}
    flags={'noPressedButtons','eventRootTranslationMatched'}
    if (type(value) is not dict or set(value)!=counts|flags|{'state','observerOnly','inputAuthorized'}
            or value.get('observerOnly') is not True or value.get('inputAuthorized') is not False
            or type(value.get('state')) is not str
            or value['state'] not in {'complete','unavailable','query-failed','identity-rejected','deadline','limit'}
            or any(type(value[k]) is not int for k in counts)
            or not 1<=value['targetPointCount']<=9
            or not 0<=value['motionWithXYCount']<=value['ownedMotionCount']<=128
            or not 0<=value['retainedPointMatchedCount']<=min(value['targetPointCount'],value['motionWithXYCount'])
            or any(type(value[k]) is not bool if value['ownedMotionCount'] else value[k] is not None for k in flags)):
        raise ValueError('invalid advisory Zed XI2 payload observation')
    return value


def codex_prewarm_context(value):
    base={'verified','reason','inputAuthorized'}
    fixed={'sourcePinned','newHomeController','localContext','retainedProject','retainedRoot'}
    selection={'prewarmResolvedSelection','noPriorReservation'}
    failures={'deadline-or-owner','runtime-unavailable','descriptor-unavailable','source-mismatch',
        'scope-unavailable','root-mismatch','project-mismatch','mode-mismatch','existing-workspace',
        'remote-override','controller-mismatch','host-or-cwd-mismatch','context-override','follow-up',
        'prepare-override','reservation-mismatch','reservation-unavailable','reservation-pending',
        'render-changed','identity-changed','editor-unavailable','editor-changed'}
    if (type(value) is not dict or type(value.get('verified')) is not bool
            or value.get('inputAuthorized') is not False or type(value.get('reason')) is not str):
        raise ValueError('invalid Codex passive context observation')
    if value['verified']:
        if (set(value)!=base|fixed|selection or value['reason']!='verified'
                or any(value[k] is not True for k in fixed)
                or any(type(value[k]) is not bool for k in selection)
                or value['prewarmResolvedSelection']==value['noPriorReservation']):
            raise ValueError('inconsistent Codex passive context proof')
    elif set(value)!=base or value['reason'] not in failures:
        raise ValueError('invalid Codex passive context boundary')
    return value


def codex_owned_move(value):
    bits = {'sourcePointRetained', 'rendererReproved', 'postMappingObserved'}
    reasons = {'source-policy-rejected', 'source-point-unavailable', 'deadline-or-owner',
               'renderer-changed', 'native-move-rejected', 'move-unavailable-or-uncertain',
               'post-mapping-unproved', 'moved-source-point-observed'}
    base = bits | {'reason', 'inputAuthorized'}
    if (type(value) is not dict or set(value) not in (base, base | {'native'})
            or type(value.get('reason')) is not str or value['reason'] not in reasons
            or value.get('inputAuthorized') is not False
            or any(type(value[k]) is not bool for k in bits)):
        raise ValueError('invalid Codex owned move observation')
    native = value.get('native')
    if 'native' in value:
        native_bits = {'planMeasured', 'fullWorkareaBoundsBlocker', 'candidateFound',
                       'moveAttempted', 'writeAcknowledged', 'sameIdentityTranslated',
                       'nativePointClear', 'nativeHitWindowMatched', 'mappingStable', 'nativeFocused'}
        native_reasons = {'plan-unavailable', 'full-workarea-bounds-blocker', 'no-clear-candidate',
                         'position-not-settable', 'pre-move-identity', 'write-uncertain',
                         'transition-unproved', 'post-point-occluded', 'post-hit-unproved',
                         'post-mapping-changed', 'post-focus-unproved', 'source-point-unavailable',
                         'deadline', 'moved-point-observed'}
        if (type(native) is not dict
                or set(native) != native_bits | {'reason', 'candidateCount', 'inputAuthorized'}
                or type(native.get('reason')) is not str or native['reason'] not in native_reasons
                or native.get('inputAuthorized') is not False
                or any(type(native[k]) is not bool for k in native_bits)
                or type(native['candidateCount']) is not int or not 0 <= native['candidateCount'] <= 9
                or not value['sourcePointRetained']):
            raise ValueError('invalid Codex native move facts')
        if (not native['planMeasured'] and (native['candidateCount'] or any(
                    native[k] for k in native_bits - {'planMeasured'}))
                or native['fullWorkareaBoundsBlocker'] and (native['candidateFound']
                    or native['candidateCount'] or native['moveAttempted'])
                or native['candidateFound'] and (not native['planMeasured'] or native['candidateCount'] < 1)
                or native['moveAttempted'] and not native['candidateFound']
                or native['writeAcknowledged'] and not native['moveAttempted']
                or native['sameIdentityTranslated'] and not native['writeAcknowledged']
                or native['mappingStable'] and not native['sameIdentityTranslated']
                or native['nativePointClear'] and not (native['mappingStable'] and native['nativeFocused'])
                or native['nativeHitWindowMatched'] and not native['nativePointClear']
                or native['reason'] == 'moved-point-observed' and (native['fullWorkareaBoundsBlocker']
                    or not all(native[k] for k in native_bits - {'fullWorkareaBoundsBlocker'}))
                or native['reason'] == 'full-workarea-bounds-blocker' and not native['fullWorkareaBoundsBlocker']
                or native['reason'] == 'no-clear-candidate' and (not native['planMeasured']
                    or native['candidateFound'] or native['fullWorkareaBoundsBlocker'])
                or native['reason'] == 'write-uncertain' and (not native['moveAttempted'] or native['writeAcknowledged'])):
            raise ValueError('inconsistent Codex native move facts')
    moved = native is not None and native['reason'] == 'moved-point-observed'
    if (value['rendererReproved'] and not value['sourcePointRetained']
            or value['postMappingObserved'] != (value['reason'] == 'moved-source-point-observed')
            or value['postMappingObserved'] and not (value['rendererReproved'] and moved)
            or value['reason'] == 'source-policy-rejected' and (
                any(value[k] for k in bits) or 'native' in value)
            or value['reason'] == 'post-mapping-unproved' and not (
                value['sourcePointRetained'] and value['rendererReproved'] and moved)
            or value['reason'] == 'native-move-rejected' and (
                native is None or moved or not value['rendererReproved'])):
        raise ValueError('inconsistent Codex owned move observation')
    return value


def codex_point_observation(value):
    reasons={'measured','ax-limit-or-deadline','ax-query','ax-visibility-unavailable',
        'ax-webarea-geometry','ax-webarea-ambiguous','ax-webarea-missing','ax-webarea-changed',
        'native-focus-unavailable','viewport-dimensions-mismatch','native-point-not-clear',
        'point-occluded','stack-unavailable','metadata-invalid','held-window-missing','held-window-changed',
        'native-hit-window-unproved','renderer-webarea-correlation-unproved','mapping-observed',
        'held-identity-or-deadline','cdp-identity-changed','cdp-child-frame-present',
        'css-viewport-unavailable','css-viewport-invalid','css-viewport-transform-unproved',
        'css-viewport-changed','cdp-held-identity-invalid','deadline-or-owner','deadline','observation-unavailable'}
    stack_causes={'point-occluded','stack-unavailable','metadata-invalid','held-window-missing','held-window-changed'}
    base={'reason','mappingObserved','inputAuthorized'}
    counts={'firstWebAreaCount','secondWebAreaCount'}
    flags={'webAreaStable','nativeFocused','dimensionsMatched','nativePointClear',
        'nativeHitWindowMatched','heldIdentityStable','webAreaUrlMatched'}
    if (type(value) is not dict or set(value) not in (base,base|counts|flags)
            or type(value.get('reason')) is not str or value['reason'] not in reasons
            or type(value.get('mappingObserved')) is not bool or value.get('inputAuthorized') is not False
            or value['mappingObserved']!=(value['reason']=='mapping-observed')):
        raise ValueError('invalid Codex point observation')
    if set(value)==base:
        if value['mappingObserved']:
            raise ValueError('incomplete Codex point observation')
    elif (any(type(value[key]) is not bool for key in flags)
            or any(type(value[key]) is not int or not 0<=value[key]<=2 for key in counts)
            or value['webAreaStable'] and any(value[key]!=1 for key in counts)
            or not value['webAreaStable'] and any(value[key] for key in flags-{'heldIdentityStable'})
            or value['mappingObserved'] and not all(value[key] for key in flags)):
        raise ValueError('inconsistent Codex point observation')
    if value['reason'] in stack_causes and (set(value)!=base|counts|flags
            or not value['webAreaStable'] or not value['nativeFocused']
            or not value['dimensionsMatched'] or value['nativePointClear']
            or not value['heldIdentityStable']):
        raise ValueError('inconsistent Codex point stack cause')
    return value


def public_mac_codex_home_state(value):
    flags={'homeRetained','stateQueried','statePairStable','ordinaryLocalProjectObserved',
           'selectedIdCorrelated','menuClickAttempted','menuClickCompleted'}
    fixed={'diagnosticsOnly':True,'inputAuthorized':False,'sendAuthorized':False}
    reasons={'custody','home','document','state','project','control','menu','selected-id',
             'state-changed','deadline','query','menu-correlated','state-observed'}
    if (type(value) is not dict or set(value)!=flags|set(fixed)|{'status','reason'}
            or any(type(value[key]) is not bool for key in flags)
            or any(type(value[key]) is not bool or value[key]!=expected for key,expected in fixed.items())
            or type(value['status']) is not str or type(value['reason']) is not str
            or value['status'] not in {'blocked','observed'} or value['reason'] not in reasons):
        raise ValueError('invalid Mac Codex home state')
    if (value['menuClickCompleted'] and not value['menuClickAttempted']
            or value['stateQueried'] and not value['homeRetained']
            or value['statePairStable'] and not value['stateQueried']
            or value['selectedIdCorrelated'] and not value['menuClickCompleted']
            or value['status']=='observed' and (not value['homeRetained'] or not value['statePairStable']
                or not value['ordinaryLocalProjectObserved']
                or value['reason']!='menu-correlated' and value['reason']!='state-observed'
                or (value['reason']=='menu-correlated')!=value['selectedIdCorrelated'])
            or value['reason']=='state-observed' and value['menuClickAttempted']
            or value['status']=='blocked' and value['reason'] in {'menu-correlated','state-observed'}):
        raise ValueError('inconsistent Mac Codex home state')
    return value


def folder_trust_observation(trust):
    if (type(trust) is not dict or set(trust) - {'rejectionStage','guardFailure'} != {'status', 'clickAttempted', 'clickCompleted'}
            or type(trust['status']) is not str
            or trust['status'] not in {'absent', 'blocked', 'completed', 'action-uncertain'}
            or type(trust['clickAttempted']) is not bool or type(trust['clickCompleted']) is not bool
            or trust['clickCompleted'] and not trust['clickAttempted']
            or trust['status'] == 'absent' and (trust['clickAttempted'] or trust['clickCompleted'])
            or trust['status'] == 'completed' and not (trust['clickAttempted'] and trust['clickCompleted'])
            or trust['status'] == 'action-uncertain' and not (trust['clickAttempted'] and not trust['clickCompleted'])):
        raise ValueError('invalid public onboarding folder trust diagnostic')
    if 'rejectionStage' in trust:
        stage = trust['rejectionStage']
        if (type(stage) is not str or stage not in {
                'authority', 'guard', 'deadline', 'dialog', 'form', 'title', 'path',
                'controls', 'hit', 'identity', 'query'}
                or trust['status'] not in {'blocked', 'action-uncertain'}):
            raise ValueError('invalid folder trust rejection diagnostic')
    if 'guardFailure' in trust:
        failure = trust['guardFailure']
        if (trust.get('rejectionStage') != 'guard' or type(failure) is not str or failure not in {
                'deadline','native-ownership','page-set','main-identity','main-focus','main-scope',
                'auxiliary-route','auxiliary-identity','auxiliary-focus','auxiliary-controls','query-failed','unmeasured'}):
            raise ValueError('invalid folder trust guard failure')


def public_onboarding(setup, app):
    shape = set(setup) - {'transitionPublicDOMObservation', 'transitionReadinessObservation', 'homeAfterContinueReady', 'folderTrust', 'rejectedPageInventory', 'taskScopeProved', 'taskClickAttempted', 'taskClickCompleted', 'codingComposerReady', 'taskScopeObservation', 'taskControlKind', 'codingReadinessObservation', 'codingNavigationObservation', 'codingPublicDOMObservation', 'codingHomeObservation', 'codingEditableObservation', 'codingHomeStateObservation', 'workspaceMenuObservation', 'macHomeStateObservation', 'taskSkipConfirmationAttempted', 'taskSkipConfirmationCompleted', 'taskSkipConfirmationProof', 'mainGuardFailure', 'pageSetFailure', 'foreignOverlayImportSetup', 'foreignOverlaySourceCounts', 'foreignOverlayActionability'} if type(setup) is dict else set()
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
    transition_fields = {'transitionPublicDOMObservation','transitionReadinessObservation'}
    if transition_fields & set(setup):
        if (not transition_fields <= set(setup) or setup['continueClickCompleted'] is not True
                or setup['roleScopeAbsent'] is not True
                or setup['stage'] not in {'scope-transition','task-action','coding-readiness'}):
            raise ValueError('invalid Codex transition observation phase')
        codex_public_dom(setup['transitionPublicDOMObservation'])
        coding = setup['transitionReadinessObservation']
        keys = set('composerCount conversationCount modalCount roleRadioCount exactAckLeafCount exactGetStartedCount exactSkipCount'.split())
        if (type(coding) is not dict or set(coding) != keys | {'status'}
                or type(coding['status']) is not str or coding['status'] not in {'observed','overflow'}
                or coding['status'] == 'observed' and any(type(coding[k]) is not int or not 0 <= coding[k] <= 32 for k in keys)
                or coding['status'] == 'overflow' and any(coding[k] is not None for k in keys)):
            raise ValueError('invalid Codex transition readiness')
    if 'homeAfterContinueReady' in setup:
        if (setup['homeAfterContinueReady'] is not True or not transition_fields <= set(setup)
                or setup['stage'] != 'coding-readiness' or setup.get('codingComposerReady') is not True
                or any(setup.get(k) is not False for k in ('taskScopeProved','taskClickAttempted','taskClickCompleted'))
                or setup['errorCategory'] is not None):
            raise ValueError('invalid direct Codex home readiness')
    if 'taskControlKind' in setup:
        if (type(setup['taskControlKind']) is not str
                or setup['taskControlKind'] not in {'get-started', 'skip-optional-capabilities'}
                or setup.get('taskScopeProved') is not True
                or setup['roleScopeAbsent'] is not True):
            raise ValueError('invalid onboarding task control kind')
    if 'taskSkipConfirmationProof' in setup:
        proof = setup['taskSkipConfirmationProof']
        if (type(proof) is not str or proof not in {'overlay-count','form','retained-identity',
                'source-controls','pointer-ancestry','heading','subtitle','matched'}
                or setup.get('taskControlKind') != 'skip-optional-capabilities'
                or setup.get('taskClickCompleted') is not True or setup['stage'] != 'coding-readiness'):
            raise ValueError('invalid Codex skip confirmation proof')
    confirmation = {'taskSkipConfirmationAttempted','taskSkipConfirmationCompleted'}
    if confirmation & set(setup):
        if (not confirmation <= set(setup) or any(type(setup[key]) is not bool for key in confirmation)
                or setup['taskSkipConfirmationAttempted'] is not True
                or setup.get('taskClickCompleted') is not True or setup.get('taskScopeProved') is not True
                or setup.get('taskControlKind') != 'skip-optional-capabilities'
                or setup['stage'] != 'coding-readiness'):
            raise ValueError('invalid Codex skip confirmation receipt')
    if 'codingReadinessObservation' in setup:
        coding = setup['codingReadinessObservation']
        keys = {'composerCount','conversationCount','modalCount','roleRadioCount',
                'exactAckLeafCount','exactGetStartedCount','exactSkipCount'}
        if (type(coding) is not dict or set(coding) != keys | {'status'}
                or type(coding['status']) is not str or coding['status'] not in {'observed','overflow'}
                or setup.get('taskScopeProved') is not True or setup.get('taskClickCompleted') is not True
                or setup['stage'] != 'coding-readiness'
                or coding['status'] == 'observed' and any(type(coding[key]) is not int or not 0 <= coding[key] <= 32 for key in keys)
                or coding['status'] == 'overflow' and any(coding[key] is not None for key in keys)):
            raise ValueError('invalid Codex coding readiness observation')
    if 'codingPublicDOMObservation' in setup:
        if setup.get('taskClickCompleted') is not True or setup['stage']!='coding-readiness':
            raise ValueError('invalid Codex public DOM phase')
        codex_public_dom(setup['codingPublicDOMObservation'])
    if 'codingHomeObservation' in setup:
        home = setup['codingHomeObservation']
        counts = {'homeRootCount','localHomeComposerCount','homeEditableCount',
                  'homeProseMirrorCount','workspaceControlCount'}
        fixed = {'sourcePlatform':'linux','sourceVersion':'26.930.41038',
                 'composerSourceSha256':'7198ee078e78a748d03c3cc96f5c041d056584d762728fd0be23e695bc394da0',
                 'pageSourceSha256':'9c9d0d9247226d43edeb4606a539b518e3be06fb65aa37bd014984fbe3998ba9'}
        if (type(home) is not dict or set(home) != counts | set(fixed) | {'status'}
                or any(home.get(key) != value for key,value in fixed.items())
                or type(home.get('status')) is not str or home['status'] not in {'observed','overflow'}
                or setup.get('taskScopeProved') is not True or setup.get('taskClickCompleted') is not True
                or setup['stage'] != 'coding-readiness'
                or home['status'] == 'observed' and any(type(home[key]) is not int or not 0 <= home[key] <= 32 for key in counts)
                or home['status'] == 'overflow' and any(home[key] is not None for key in counts)):
            raise ValueError('invalid Codex coding home observation')
        if home['status'] == 'observed' and (home['homeProseMirrorCount'] > home['homeEditableCount']
                or home['homeRootCount'] == 0 and any(home[key] for key in counts - {'homeRootCount'})
                or home['localHomeComposerCount'] == 0 and home['homeEditableCount'] != 0):
            raise ValueError('inconsistent Codex coding home observation')
    if 'codingHomeStateObservation' in setup:
        observation=setup['codingHomeStateObservation']
        counts={'homeComposerCount','pendingTextareaCount','pendingGroupCount','proseMirrorEditableCount',
                'enabledSendCount','disabledSendCount','workspaceControlCount'}
        fixed={'sourcePlatform':'linux','sourceVersion':'26.930.41038',
               'composerSourceSha256':'7198ee078e78a748d03c3cc96f5c041d056584d762728fd0be23e695bc394da0'}
        if (type(observation) is not dict or set(observation) != counts | set(fixed) | {'status'}
                or any(observation.get(key) != value for key,value in fixed.items())
                or type(observation.get('status')) is not str or observation['status'] not in {'observed','overflow'}
                or setup.get('taskScopeProved') is not True or setup.get('taskClickCompleted') is not True
                or setup['stage'] != 'coding-readiness'
                or observation['status']=='observed' and any(type(observation[key]) is not int or not 0<=observation[key]<=32 for key in counts)
                or observation['status']=='overflow' and any(observation[key] is not None for key in counts)):
            raise ValueError('invalid Codex home state observation')
        if observation['status']=='observed' and observation['homeComposerCount']==0 and any(observation[key] for key in counts-{'homeComposerCount'}):
            raise ValueError('inconsistent Codex home state observation')
    if 'macHomeStateObservation' in setup:
        observation=setup['macHomeStateObservation']
        public_mac_codex_home_state(observation)
        if (app!='chatgpt-desktop' or setup.get('taskScopeProved') is not True
                or setup.get('taskClickCompleted') is not True or setup['stage']!='coding-readiness'):
            raise ValueError('invalid Mac Codex home state context')
    if 'workspaceMenuObservation' in setup:
        menu=setup['workspaceMenuObservation']
        fixed={'diagnosticsOnly':True,'sendAuthorized':False}
        flags={'clickAttempted','clickCompleted'}
        selectionflags={'selectionClickAttempted','selectionClickCompleted'}
        present=set(menu)&selectionflags if type(menu) is dict else set()
        if present and (present not in ({'selectionClickAttempted'},selectionflags)
                or menu['selectionClickAttempted'] is not True
                or 'selectionClickCompleted' in menu and menu['selectionClickCompleted'] is not True
                or not menu.get('clickCompleted')):
            raise ValueError('invalid Codex ordinary project selection')
        stage_fields={'selectionStage'} if type(menu) is dict and 'selectionStage' in menu else set()
        if stage_fields and (not present or type(menu['selectionStage']) is not str or menu['selectionStage'] not in
                {'item-click','original-popup-close','closed-source','reopen-click','reopened-popup','completed'}
                or (menu['selectionStage']=='completed')!=menu.get('selectionClickCompleted',False)):
            raise ValueError('invalid Codex project selection stage')
        failure_fields={'selectionFailure'} if type(menu) is dict and 'selectionFailure' in menu else set()
        if failure_fields and (not present or menu.get('status')!='blocked'
                or menu.get('selectionStage')!='original-popup-close'
                or type(menu['selectionFailure']) is not str or menu['selectionFailure'] not in
                    {'deadline-or-owner','identity-changed','editor-changed','editor-unavailable','runtime-unavailable','source-close-unproved'}):
            raise ValueError('invalid Codex selection document boundary')
        base=set(fixed)|flags|{'status'}|present|stage_fields|failure_fields
        if (type(menu) is not dict or type(menu.get('status')) is not str
                or menu['status'] not in {'observed','blocked'}
                or any(menu.get(k) is not v for k,v in fixed.items())
                or any(type(menu.get(k)) is not bool for k in flags)
                or menu['clickCompleted'] and not menu['clickAttempted']
                or setup.get('stage')!='coding-readiness' or setup.get('taskClickCompleted') is not True):
            raise ValueError('invalid Codex workspace menu observation')
        if menu['status']=='blocked':
            if (set(menu)!=base|{'reason'} or type(menu['reason']) is not str
                    or menu['reason'] not in {'guard','control','menu','deadline','action-uncertain','query'}):
                raise ValueError('invalid Codex workspace menu boundary')
        else:
            state=menu.get('profileStateObservation')
            stateflags={'statePairStable','ordinaryLocalProjectObserved','selectedIdCorrelated'}
            statebase=stateflags|{'status','diagnosticsOnly','sendAuthorized'}
            if (set(menu)!=base|{'profileStateObservation'} or not all(menu[k] for k in flags)
                    or type(state) is not dict or type(state.get('status')) is not str
                    or state['status'] not in {'observed','blocked'}
                    or state.get('diagnosticsOnly') is not True or state.get('sendAuthorized') is not False
                    or any(type(state.get(k)) is not bool for k in stateflags)):
                raise ValueError('invalid Codex profile state observation')
            if state['status']=='observed':
                if (set(state)!=statebase|({'prewarmContext'} if 'prewarmContext' in state else set())|({'ordinarySelectionCompleted'} if 'ordinarySelectionCompleted' in state else set())
                        or not all(state[k] for k in stateflags)
                        or 'ordinarySelectionCompleted' in state and (state['ordinarySelectionCompleted'] is not True
                            or menu.get('selectionClickCompleted') is not True)
                        or menu.get('selectionClickCompleted') is True and state.get('ordinarySelectionCompleted') is not True):
                    raise ValueError('inconsistent Codex selected project observation')
            elif (set(state)!=statebase|{'reason'}|({'projectFailure'} if 'projectFailure' in state else set())|({'selectedProjectObservation'} if 'selectedProjectObservation' in state else set()) or type(state['reason']) is not str
                    or state['reason'] not in {'profile-custody','workspace','state','project','guard','selected-id','state-changed','selection-transition','selection-state','deadline','query'}
                    or state['statePairStable'] or state['selectedIdCorrelated']):
                raise ValueError('invalid Codex profile state boundary')
            if 'projectFailure' in state and (state['status']!='blocked' or state.get('reason')!='project'
                    or type(state['projectFailure']) is not str
                    or state['projectFailure'] not in {'container','projects-shape','projects-count','project-namespace',
                        'record-shape','record-identity','record-time','record-root','stored-selection'}):
                raise ValueError('invalid Codex project projection boundary')
            if 'prewarmContext' in state:
                codex_prewarm_context(state['prewarmContext'])
            if 'selectedProjectObservation' in state:
                selected=state['selectedProjectObservation']
                if (state['status']!='blocked' or state.get('reason')!='selected-id'
                        or state['ordinaryLocalProjectObserved'] is not True or type(selected) is not dict
                        or type(selected.get('reason')) is not str or selected['reason'] not in {'menu','list','limit','selected-id'}):
                    raise ValueError('invalid Codex selected project boundary')
                if selected['reason']=='selected-id':
                    if (set(selected)!={'reason','selectedItemCount','matchingItemCount'}
                            or any(type(selected[k]) is not int or not 0<=selected[k]<=32
                                for k in ('selectedItemCount','matchingItemCount'))):
                        raise ValueError('invalid Codex selected project counts')
                elif set(selected)!={'reason'}:
                    raise ValueError('invalid Codex selected project shape')
    if 'codingEditableObservation' in setup:
        observation = setup['codingEditableObservation']
        categories = {'codexHomeCount','codexThreadCount','codexOtherCount','classicChatGPTCount',
                      'genericInputCount','genericBodyCount','unboundCount'}
        counts = categories | {'editableCount','sidebarNewChatCount'}
        fixed = {'sourcePlatform':'linux','sourceVersion':'26.930.41038',
                 'initialSourceSha256':'28c6096af241a37a9a33a2e5601f0aa05426910d5c84d08824d852342a2b4d5d',
                 'composerSourceSha256':'7198ee078e78a748d03c3cc96f5c041d056584d762728fd0be23e695bc394da0'}
        if (type(observation) is not dict
                or set(observation) != counts | set(fixed) | {'status','sidebarNewChatHitActionable'}
                or any(observation.get(key) != value for key,value in fixed.items())
                or type(observation.get('status')) is not str
                or observation['status'] not in {'observed','overflow'}
                or setup.get('taskScopeProved') is not True or setup.get('taskClickCompleted') is not True
                or setup['stage'] != 'coding-readiness'):
            raise ValueError('invalid Codex editable ancestry observation')
        if observation['status'] == 'observed':
            if (any(type(observation[key]) is not int or not 0 <= observation[key] <= 32 for key in counts)
                    or sum(observation[key] for key in categories) != observation['editableCount']
                    or type(observation['sidebarNewChatHitActionable']) is not bool
                    or observation['sidebarNewChatHitActionable'] and observation['sidebarNewChatCount'] != 1):
                raise ValueError('inconsistent Codex editable ancestry observation')
        elif any(observation[key] is not None for key in counts | {'sidebarNewChatHitActionable'}):
            raise ValueError('invalid Codex editable ancestry overflow')
    if 'codingNavigationObservation' in setup:
        nav = setup['codingNavigationObservation']
        count_keys = {'codexButtonCount','codexLinkCount','codexMenuItemCount','chatModeTriggerCount',
                      'codexModeTriggerCount','projectSelectorCount','newChatCount','projectsLinkCount'}
        if (type(nav) is not dict or set(nav) != count_keys | {'status','sourceVersion','sourceSha256',
                'uniqueCodexRole','uniqueCodexHitActionable'}
                or nav['sourceVersion'] != '26.930.41038'
                or nav['sourceSha256'] != '28c6096af241a37a9a33a2e5601f0aa05426910d5c84d08824d852342a2b4d5d'
                or setup.get('taskClickCompleted') is not True or setup['stage'] != 'coding-readiness'
                or type(nav['status']) is not str or nav['status'] not in {'observed','overflow'}):
            raise ValueError('invalid Codex navigation observation')
        if nav['status'] == 'overflow':
            if any(nav[key] is not None for key in count_keys | {'uniqueCodexRole','uniqueCodexHitActionable'}):
                raise ValueError('invalid Codex navigation overflow')
        else:
            if (any(type(nav[key]) is not int or not 0 <= nav[key] <= 32 for key in count_keys)
                    or type(nav['uniqueCodexRole']) is not str
                    or nav['uniqueCodexRole'] not in {'none','button','link','menuitem'}
                    or type(nav['uniqueCodexHitActionable']) is not bool):
                raise ValueError('invalid Codex navigation counts')
            total = sum(nav[key] for key in ('codexButtonCount','codexLinkCount','codexMenuItemCount'))
            expected = {'button':'codexButtonCount','link':'codexLinkCount','menuitem':'codexMenuItemCount'}
            if ((total == 1) != (nav['uniqueCodexRole'] != 'none')
                    or total == 1 and nav[expected[nav['uniqueCodexRole']]] != 1
                    or nav['uniqueCodexHitActionable'] and total != 1):
                raise ValueError('invalid Codex navigation identity')
    if 'taskScopeObservation' in setup:
        task_scope_observation(setup['taskScopeObservation'])
    if 'folderTrust' in setup:
        folder_trust_observation(setup['folderTrust'])
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
NATIVE_SUBSTAGES = set('trust-query trust-before trust-after panel-query layout-zoom-before layout-zoom-after new-thread-before new-thread-after panel-settle select-all-before select-all-after type-before type-after paste-before paste-after paste-settle input-sentinel-write copy-select-all-before copy-select-all-after input-copy-before input-copy-after collapse-selection-before submit-before response-control-query response-sentinel-write response-copy-before response-copy-after clipboard-read-before clipboard-read-after export-copy-before export-copy-after export-read-before export-read-after export-parse completed retry-control-query retry-visible-wait retry-element-capture retry-action-dispatch retry-before retry-after retry-title-query retry-tooltip-reset retry-tooltip-hover retry-tooltip-query retry-tooltip-clear retry-revalidate retry-label-query retry-label-parent activation-before activation-after retry-inventory-before retry-inventory-after icon-baseline-before icon-baseline-after icon-observation-before icon-observation-settle icon-observation-after icon-observation-completed'.split())
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
    # Claude's three native sessions retain storage, focus, process lifecycle,
    # provider and recovery receipts, plus the shared foreground receipt. A
    # hosted campaign produced 70 records, exceeding the old renderer budget.
    # Allow 96 for Claude (at most 768 KiB); retain every record's closed schema
    # and 8 KiB bound, and keep private connection metadata separate.
    limit = 96 if app == 'claude-desktop' else 64
    if len(paths) > limit:
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
        if mechanism not in {'codex-renderer-qualification', 'qualification-runner-failure', 'qualification-reduction-failure', 'hermes-windows-catalog-readiness', 'hermes-renderer-qualification', 'zed-native-copy', 'semantic-provider-oracle', 'semantic-failure-policy', 'hermes-retry-policy', 'semantic-inventory', 'zed-native-icons', 'zed-retry-visual', 'hermes-front-source', 'hermes-backend-failure', 'hermes-policy-preparation', 'zed-atspi-retry', 'windows-foreground-session', 'zed-retry-entry-counts', 'zed-pointer-transport', 'zed-pointer-observation', 'zed-clipboard-transport', 'windows-endpoint-proof', 'renderer-inventory', 'codex-static-dialog-title', 'codex-linux-startup-dialog', 'native-window-stability', 'renderer-startup', 'renderer-startup-baseline', 'codex-owned-relaunch', 'codex-restore', 'codex-project-preflight', 'windows-process-absence', 'windows-post-stop-process', 'windows-process-baseline', 'windows-process-settlement', 'windows-owned-stop', 'windows-process-correlation', 'windows-owned-descendant-cleanup', 'windows-owned-cleanup-preflight', 'windows-owned-cleanup-preflight-progress', 'claude-owned-configuration', 'claude-restore', 'claude-model-discovery', 'claude-cli-prelaunch', 'claude-config-persist-owners', 'claude-window-stack', 'claude-window-focus', 'claude-chat-navigation', 'claude-native-chat', 'claude-windows-native-chat', 'claude-linux-native-chat', 'claude-window-fit', 'claude-windows-fit', 'claude-windows-uia', 'claude-windows-fit-rejection', 'claude-storage-use', 'claude-native-storage', 'claude-private-storage-stage', 'claude-windows-profile-seal', 'codex-windows-profile-prepare', 'claude-native-composer', 'claude-linux-mode-roles', 'claude-linux-classic-visibility', 'claude-native-root-preflight', 'zed-panel-zoom', 'zed-atspi-geometry'}:
            continue
        expected = 'hermes-renderer-qualification' if app == 'hermes-desktop' else 'zed-native-copy'
        if (mechanism != expected and mechanism not in {'codex-renderer-qualification', 'qualification-runner-failure', 'qualification-reduction-failure', 'hermes-windows-catalog-readiness', 'semantic-provider-oracle', 'semantic-failure-policy', 'hermes-retry-policy', 'semantic-inventory', 'zed-native-icons', 'zed-retry-visual', 'hermes-front-source', 'hermes-backend-failure', 'hermes-policy-preparation', 'zed-atspi-retry', 'windows-foreground-session', 'zed-retry-entry-counts', 'zed-pointer-transport', 'zed-pointer-observation', 'zed-clipboard-transport', 'windows-endpoint-proof', 'renderer-inventory', 'codex-static-dialog-title', 'codex-linux-startup-dialog', 'native-window-stability', 'renderer-startup', 'renderer-startup-baseline', 'codex-owned-relaunch', 'codex-restore', 'codex-project-preflight', 'windows-process-absence', 'windows-post-stop-process', 'windows-process-baseline', 'windows-process-settlement', 'windows-owned-stop', 'windows-process-correlation', 'windows-owned-descendant-cleanup', 'windows-owned-cleanup-preflight', 'windows-owned-cleanup-preflight-progress', 'claude-owned-configuration', 'claude-restore', 'claude-model-discovery', 'claude-cli-prelaunch', 'claude-config-persist-owners', 'claude-window-stack', 'claude-window-focus', 'claude-chat-navigation', 'claude-native-chat', 'claude-windows-native-chat', 'claude-linux-native-chat', 'claude-window-fit', 'claude-windows-fit', 'claude-windows-uia', 'claude-windows-fit-rejection', 'claude-storage-use', 'claude-native-storage', 'claude-private-storage-stage', 'claude-windows-profile-seal', 'codex-windows-profile-prepare', 'claude-native-composer', 'claude-linux-mode-roles', 'claude-linux-classic-visibility', 'claude-native-root-preflight', 'zed-panel-zoom', 'zed-atspi-geometry'}) or type(value.get('schemaVersion')) is not int or value['schemaVersion'] != 1:
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
            optional = {'actionObservation', 'onboardingSkipped', 'onboardingObservation'}
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
            if 'onboardingObservation' in value:
                observation = value['onboardingObservation']
                counts = {'coverCount', 'choiceCount'}
                flags = {'coverVisible', 'choiceVisible', 'choiceEnabled'}
                if (type(observation) is not dict or set(observation) != counts | flags
                        or any(type(observation[key]) is not int or not 0 <= observation[key] <= 64 for key in counts)
                        or any(observation[key] is not None and type(observation[key]) is not bool for key in flags)):
                    raise ValueError('invalid Hermes onboarding controls')
                record['onboardingObservation'] = observation
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
            statuses.update('limit-depth limit-nodes limit-name limit-text limit-windows limit-processes'.split())
            observed = value.get('status') == 'observed'
            if (app != 'claude-desktop' or set(value) - {'currentMode', 'chatCapability'} != fields or value['diagnosticsOnly'] is not True
                    or value['phase'] != 'post-ready' or type(value['status']) is not str or value['status'] not in statuses
                    or value['nativeGuardVerified'] is not observed or value['treeComplete'] is not observed
                    or any((type(value[key]) is not int or not 0 <= value[key] <= 1024) if observed else value[key] is not None for key in counts)
                    or observed and (value['nodeCount'] == 0 or any(value[key] > value['nodeCount'] for key in counts))):
                raise ValueError('invalid passive Windows Claude UIA diagnostic')
            if 'currentMode' in value:
                mode = value['currentMode']
                keys = {'modeGroupCount', 'chatCount', 'coworkCount', 'currentChatCount', 'currentCoworkCount'}
                if (not observed or type(mode) is not dict or set(mode) != keys | {'status'}
                        or type(mode['status']) is not str or mode['status'] not in {'chat', 'cowork', 'missing', 'ambiguous', 'unavailable', 'changed'}):
                    raise ValueError('invalid Windows Claude current mode')
                if mode['status'] in {'unavailable', 'changed'}:
                    if any(mode[key] is not None for key in keys):
                        raise ValueError('partial Windows Claude mode')
                else:
                    if (any(type(mode[key]) is not int or not 0 <= mode[key] <= value['nodeCount'] for key in keys)
                            or mode['currentChatCount'] > mode['chatCount'] or mode['currentCoworkCount'] > mode['coworkCount']):
                        raise ValueError('invalid Windows Claude mode counts')
                    current = mode['currentChatCount'] + mode['currentCoworkCount']
                    expected = ('ambiguous' if any(mode[key] > 1 for key in ('modeGroupCount', 'chatCount', 'coworkCount')) or current > 1
                                else 'missing' if mode['modeGroupCount'] != 1 or mode['chatCount'] + mode['coworkCount'] == 0 or current != 1
                                else 'chat' if mode['currentChatCount'] == 1 else 'cowork')
                    legacy_missing = (mode['status'] == 'missing' and expected in {'chat', 'cowork'}
                                      and (mode['chatCount'] == 0 or mode['coworkCount'] == 0))
                    if mode['status'] != expected and not legacy_missing:
                        raise ValueError('contradictory Windows Claude mode')
                record['currentMode'] = dict(mode)
            if 'chatCapability' in value:
                capability = value['chatCapability']
                keys = {'valuePattern', 'valueReadOnly', 'valueEmpty', 'password',
                        'keyboardFocusable', 'startTaskInvokePattern'}
                if (not observed or type(capability) is not dict or set(capability) != keys | {'status'}
                        or type(capability['status']) is not str or capability['status'] not in {'observed', 'missing', 'ambiguous', 'unavailable', 'changed'}):
                    raise ValueError('invalid Windows Chat capability')
                if capability['status'] != 'observed':
                    if any(capability[key] is not None for key in keys):
                        raise ValueError('partial Windows Chat capability')
                else:
                    if (value.get('currentMode', {}).get('status') != 'chat'
                            or value['classicEditorCount'] != 1 or value['startTaskControlCount'] != 1
                            or any(type(capability[key]) is not bool for key in keys - {'valueReadOnly', 'valueEmpty'})):
                        raise ValueError('unbound Windows Chat capability')
                    for key in ('valueReadOnly', 'valueEmpty'):
                        if ((capability['valuePattern'] and type(capability[key]) is not bool)
                                or (not capability['valuePattern'] and capability[key] is not None)):
                            raise ValueError('invalid Windows Chat value capability')
                record['chatCapability'] = dict(capability)
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
        elif mechanism in {'claude-native-chat', 'claude-windows-native-chat'}:
            counts = {'submittedTurns', 'inputVerifiedTurns', 'copiedResponses'}
            flags = {'retryAttempted', 'clipboardCleared'}
            fields = counts | flags | {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'stage'}
            stages = set('request window tree tree-query tree-duplicate tree-type tree-limit tree-pid tree-focus tree-window mode composer composer-send-pending focus input-focus-guard input-focus-setting input-focused-identity input-replace-select-key input-prompt-before-guard input-prompt-clipboard input-prompt-after-guard input-paste-key input-readback-before-guard input-sentinel-clipboard input-sentinel-after-guard input-readback-select-key input-readback-select-guard input-readback-copy-key input-collapse-guard input-collapse-key input-mismatch input-initial-unavailable input-initial-nonempty input-clipboard-mismatch input-value-mismatch control scope scope-anchor-absent scope-heading-absent scope-assistant-heading-absent scope-marker-heading-absent scope-anchor-ambiguous scope-control-absent scope-control-ambiguous scope-heading-ambiguous scope-prompt-absent scope-prompt-mismatch deadline action-uncertain response-mismatch sent copied retry-ready failure-details-ready failure-details-opened retried completed deadline-window deadline-tree deadline-focus deadline-input deadline-input-paste deadline-input-readback deadline-press deadline-copy deadline-retry-ready deadline-retry'.split())
            stages.update('tree-depth tree-nodes tree-name-limit tree-text-limit tree-window-limit tree-process-limit'.split())
            stages.update('clipboard-owner clipboard-allocation clipboard-lock clipboard-empty clipboard-set clipboard-close clipboard-guard-before clipboard-guard-after clipboard-deadline-before clipboard-deadline-after clipboard-open-deadline'.split())
            phase_fields = {'actionPhase', 'transportFailure'}
            if (app != 'claude-desktop' or set(value) - {'providerObservation', 'guardRejection', 'rowShape', 'scopeShape', 'failureAuthority', 'failureScopeCounts', 'operationTiming'} not in (fields, fields | phase_fields) or value['diagnosticsOnly'] is not True
                    or type(value['stage']) is not str or value['stage'] not in stages
                    or any(type(value[key]) is not bool for key in flags)
                    or any(type(value[key]) is not int or not 0 <= value[key] <= 3 for key in counts)
                    or value['submittedTurns'] > value['inputVerifiedTurns']
                    or value['copiedResponses'] > value['submittedTurns'] + int(value['retryAttempted'])):
                raise ValueError('invalid Claude native Chat observation')
            if 'operationTiming' in value:
                timing = value['operationTiming']
                required = {'budgetMs', 'elapsedMs'}
                remaining = {'transportRemainingMs', 'postGuardRemainingMs'}
                if (mechanism != 'claude-windows-native-chat' or type(timing) is not dict
                        or set(timing) != required | remaining
                        or any(type(timing[key]) is not int or not 0 <= timing[key] <= 600000 for key in required)
                        or any(timing[key] is not None and (type(timing[key]) is not int
                            or not 0 <= timing[key] <= timing['budgetMs']) for key in remaining)):
                    raise ValueError('invalid Claude Windows operation timing')
                record['operationTiming'] = timing
            if 'failureAuthority' in value:
                authority = value['failureAuthority']
                counters = {'rejectedStream', 'rejectedHistory', 'rejectedContext'}
                if (mechanism != 'claude-native-chat' or value['submittedTurns'] not in (2, 3)
                        or value['copiedResponses'] != 2 or value['retryAttempted']
                        or type(authority) is not dict
                        or set(authority) != counters | {'status', 'preparedTurns', 'learnedTurns'}
                        or type(authority['status']) is not str or authority['status'] not in {
                            'armed-unobserved', 'context-unobserved', 'context-changed', 'prior-context-incomplete', 'policy'}
                        or any(type(authority[key]) is not int or not 0 <= authority[key] <= 4096 for key in counters)
                        or type(authority['preparedTurns']) is not int or not 0 <= authority['preparedTurns'] <= 3
                        or type(authority['learnedTurns']) is not int or not 0 <= authority['learnedTurns'] <= authority['preparedTurns']
                        or (authority['status'] == 'armed-unobserved' and (value['submittedTurns'] != 3 or authority['preparedTurns'] != 3 or authority['learnedTurns'] != 2 or type(value.get('providerObservation')) is not dict or value['providerObservation'].get('failureObserved') is not False))
                        or (authority['status'] != 'armed-unobserved' and (value['submittedTurns'] != 2 or authority['preparedTurns'] > 2))
                        or authority['status'] == 'context-unobserved' and authority['learnedTurns'] != 0
                        or authority['status'] == 'prior-context-incomplete' and authority['learnedTurns'] >= 2):
                    raise ValueError('invalid Claude failure authority diagnostic')
                record['failureAuthority'] = authority
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
            if 'failureScopeCounts' in value:
                counts = value['failureScopeCounts']
                names = set('serverErrorCount failedUserHeadingCount failedPromptTextCount retryButtonCount detailsButtonCount exactPromptGroupCount groupRetryButtonCount groupDetailsButtonCount'.split())
                label_names = {'retryLabelCount', 'detailsLabelCount'}
                unfiltered_names = {'unfilteredRetryLabelCount', 'unfilteredDetailsLabelCount'}
                if (mechanism != 'claude-windows-native-chat' or type(counts) is not dict or set(counts) not in (names, names | label_names, names | label_names | unfiltered_names, names | label_names | unfiltered_names | {'buttonShape'})
                        or any(type(item) is not int or not 0 <= item <= 1024 for key,item in counts.items() if key not in unfiltered_names | {'buttonShape'})
                        or any(item is not None and (type(item) is not int or not 0 <= item <= 1024)
                               for key,item in counts.items() if key in unfiltered_names)
                        or counts['groupRetryButtonCount'] > counts['retryButtonCount']
                        or counts['groupDetailsButtonCount'] > counts['detailsButtonCount']):
                    raise ValueError('invalid Windows failure scope counts')
                if label_names <= set(counts) and (counts['retryButtonCount'] > counts['retryLabelCount']
                        or counts['detailsButtonCount'] > counts['detailsLabelCount']):
                    raise ValueError('invalid Windows failure label counts')
                if 'buttonShape' in counts:
                    shape = counts['buttonShape']
                    if (type(shape) is not list or len(shape) != 6
                            or any(type(item) is not int or not 0 <= item <= 1024 for item in shape)
                            or any(shape[a] > shape[b] for a,b in ((1,0),(2,0),(3,2),(3,1),(4,2),(5,4),(5,3)))
                            or any(counts[key] > shape[index] for key,index in (('retryButtonCount',0),('detailsButtonCount',0),('groupRetryButtonCount',2),('groupDetailsButtonCount',2)))):
                        raise ValueError('invalid Windows failure button shape')
                record['failureScopeCounts'] = counts.copy()
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
            if 'scopeShape' in value:
                shape = value['scopeShape']
                counts = {'groupAncestorCount', 'sourceRowLabelsAnyRole', 'streamingLabelsAnyRole',
                          'tryAgainLabelsAnyRole', 'tryAgainButtons', 'viewDetailsLabelsAnyRole', 'viewDetailsButtons'}
                if (mechanism != 'claude-native-chat' or 'rowShape' not in value or type(shape) is not dict
                        or set(shape) != counts | {'parentKind', 'walkEnd'}
                        or type(shape['parentKind']) is not str or shape['parentKind'] not in {'none', 'group', 'web-area', 'scroll-area', 'window', 'other'}
                        or type(shape['walkEnd']) is not str or shape['walkEnd'] not in {'boundary-web-area', 'boundary-scroll-area', 'boundary-window', 'root', 'depth-limit'}
                        or any(type(shape[key]) is not int or not 0 <= shape[key] <= 1024 for key in counts)
                        or shape['groupAncestorCount'] > 6
                        or shape['tryAgainButtons'] > shape['tryAgainLabelsAnyRole']
                        or shape['viewDetailsButtons'] > shape['viewDetailsLabelsAnyRole']
                        or (shape['parentKind'] == 'none' and (shape['walkEnd'] != 'root' or shape['groupAncestorCount'] != 0))):
                    raise ValueError('invalid passive Claude failure scope shape')
                record['scopeShape'] = shape
            if 'rowShape' in value:
                shape = value['rowShape']
                count_keys = {'sourceRows', 'streamingRows', 'exactUserHeadings', 'exactPromptNodes',
                              'serverErrorLabels', 'retryControls', 'detailsControls', 'userRows', 'errorRows',
                              'sharedParentPairs', 'adjacentPairs', 'assistantHeadingsInErrorRows', 'duplicatePositions'}
                if (mechanism != 'claude-native-chat' or type(shape) is not dict
                        or set(shape) != {'sourceVersion', 'sourceSha256', 'navigationSourceSha256', 'phase', 'counts'}
                        or shape['sourceVersion'] != '2.19675.0' or shape['phase'] != 'pre-disclosure'
                        or shape['sourceSha256'] != '87e6b710a540352fcd4f9a1f0f6a8f9f9b6377ca676fd99c3e4d8bc87653dceb'
                        or shape['navigationSourceSha256'] != '948270963cdf93cc411d95393157f5c7e2c06f18916d8c4ea1971828fb0c677c'
                        or type(shape['counts']) is not dict or set(shape['counts']) != count_keys
                        or any(type(count) is not int or not 0 <= count <= 1024 for count in shape['counts'].values())):
                    raise ValueError('invalid passive Claude failure row shape')
                row = shape['counts']
                if (any(row[key] > row['sourceRows'] for key in ('streamingRows', 'userRows', 'errorRows', 'duplicatePositions'))
                        or row['adjacentPairs'] > row['sharedParentPairs']
                        or row['sharedParentPairs'] > row['userRows'] * row['errorRows']):
                    raise ValueError('inconsistent passive Claude failure row shape')
                record['rowShape'] = shape
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
                      'identity-recheck', 'pre-resize-identity', 'resize-acknowledgement',
                      'pre-position-identity', 'allocation', 'size', 'position', 'postcondition',
                      'transport', 'invalid-output'}
            if (set(value) - {'positionError'} != fields or app != 'claude-desktop' or value['diagnosticsOnly'] is not True
                    or type(value['stage']) is not str or value['stage'] not in stages):
                raise ValueError('invalid Claude window fit observation')
            if 'positionError' in value:
                error = value['positionError']
                if (value['stage'] != 'position' or type(error) is not str or error not in {
                        'cannot-complete','attribute-unsupported','illegal-argument',
                        'invalid-element','api-disabled','failure','other'}):
                    raise ValueError('invalid Claude position error observation')
                record['positionError'] = error
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
                if type(value['phase']) is not str or value['phase'] not in {'initial', 'final-stability', 'initial-decision', 'runtime-rejection'}:
                    raise ValueError('invalid Claude focus phase')
                record['phase'] = value['phase']
            if 'guardCategory' in value:
                category = value['guardCategory']
                if (value.get('phase') not in {'initial-decision', 'runtime-rejection'}
                        or category is not None and (type(category) is not str or category not in {
                            'identity-missing', 'bounds-changed', 'foreground-changed',
                            'same-process-window', 'off-display', 'occluded'})):
                    raise ValueError('invalid Claude decision category')
                if value.get('phase') == 'runtime-rejection' and category is None:
                    raise ValueError('missing Claude runtime rejection category')
                record['guardCategory'] = category
            elif value.get('phase') in {'initial-decision', 'runtime-rejection'}:
                raise ValueError('missing Claude decision category')
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
        elif mechanism == 'codex-windows-profile-prepare':
            if app != 'chatgpt-desktop':
                raise ValueError('invalid Codex profile application')
            record.update(codex_windows_prepare(value))
        elif mechanism == 'claude-linux-native-chat':
            fields = {'schemaVersion','mechanism','diagnosticsOnly','stage','submittedTurns',
                      'inputVerifiedTurns','copiedResponses','retryAttempted','clipboardCleared'}
            if (app != 'claude-desktop' or set(value) - {'failureBoundary','inputShape','embeddedTextObservation','ownedInputObservation','sendActionClass','sendActionObservation','emptyInputDrift','retryCandidateObservation','nativeTreeObservation','queryObservation'} != fields or value['diagnosticsOnly'] is not True
                    or type(value['stage']) is not str or value['stage'] not in {
                        'source','focus','paste','readback','send','input-not-empty','blocked',
                        'action-uncertain','deadline','clipboard-cleanup','sent','response-pending',
                        'response-mismatch','copied','recovery-scope-unimplemented','retry-diagnostic','retry-forwarded'}
                    or any(type(value[key]) is not int or not 0 <= value[key] <= 3
                           for key in ('submittedTurns','inputVerifiedTurns','copiedResponses'))
                    or not value['copiedResponses'] <= value['submittedTurns'] <= value['inputVerifiedTurns']
                    or type(value['retryAttempted']) is not bool or type(value['clipboardCleared']) is not bool
                    or value['retryAttempted'] and (value['submittedTurns'] != 3 or value['copiedResponses'] < 2)
                    or value['copiedResponses'] == 3 and not value['retryAttempted']
                    or value['stage'] == 'retry-forwarded' and not value['retryAttempted']
                    or value['stage'] == 'retry-diagnostic' and value['retryAttempted']):
                raise ValueError('invalid Claude Linux native Chat diagnostic')
            if 'queryObservation' in value:
                timing = value['queryObservation']
                timing_keys = {'calls', 'elapsedMs', 'nativeWindowMs', 'lastMs'}
                if (type(timing) is not dict or set(timing) != timing_keys
                        or any(type(timing[key]) is not int or not 0 <= timing[key] <= (
                            100000 if key == 'calls' else 600000) for key in timing_keys)
                        or timing['nativeWindowMs'] > timing['elapsedMs']
                        or timing['lastMs'] > timing['elapsedMs']):
                    raise ValueError('invalid Claude native query timing')
                record['queryObservation'] = timing
            if 'nativeTreeObservation' in value:
                if value['stage'] not in {'blocked','deadline','clipboard-cleanup','action-uncertain'}:
                    raise ValueError('unexpected Claude tree observation')
                record['nativeTreeObservation'] = claude_native_tree(value['nativeTreeObservation'])
            if 'retryCandidateObservation' in value:
                if value['stage'] != 'retry-diagnostic' or value['submittedTurns'] != 3 or value['copiedResponses'] != 2:
                    raise ValueError('unexpected Claude retry diagnostic')
                record['retryCandidateObservation'] = claude_retry_candidate(value['retryCandidateObservation'])
            if 'emptyInputDrift' in value:
                drift=value['emptyInputDrift']
                keys=set('witnessPresent rootSame textSame recordKeysSame stateSame attributesSame textRecordsSame otherValuesSame focusOnlyStateChange focusAttempted'.split())
                if (type(drift) is not dict or set(drift)!=keys or any(type(v) is not bool for v in drift.values())
                        or value.get('failureBoundary') not in {'input-empty-state','input-empty-witness'}
                        or drift['focusOnlyStateChange'] and (drift['stateSame'] or not drift['recordKeysSame'])
                        or not drift['witnessPresent'] and any(drift[k] for k in keys-{'focusAttempted','witnessPresent'})):
                    raise ValueError('invalid Claude empty input drift')
                record['emptyInputDrift']=drift
            if 'sendActionObservation' in value:
                action=value['sendActionObservation']
                if (type(action) is not dict or set(action) != {'actionCount','activationMatchCount','selectedIndex','activationClass'}
                        or type(action['actionCount']) is not int or not 0 <= action['actionCount'] <= 8
                        or type(action['activationMatchCount']) is not int or not 0 <= action['activationMatchCount'] <= action['actionCount']
                        or type(action['activationClass']) is not str or value['inputVerifiedTurns'] < 1
                        or 'sendActionClass' not in value):
                    raise ValueError('invalid Claude Linux Send action observation')
                matches=action['activationMatchCount'];index=action['selectedIndex'];kind=action['activationClass']
                if not (matches == 0 and index is None and kind == 'none'
                        or matches == 1 and type(index) is int and 0 <= index < action['actionCount'] and kind in {'click','press'}
                        or matches > 1 and index is None and kind == 'ambiguous'):
                    raise ValueError('inconsistent Claude Linux Send action observation')
                record['sendActionObservation']=action
            if 'sendActionClass' in value:
                if (type(value['sendActionClass']) is not str
                        or value['sendActionClass'] not in {'click','press','none','multiple','other'}
                        or value['inputVerifiedTurns'] < 1):
                    raise ValueError('invalid Claude Linux Send action class')
                record['sendActionClass'] = value['sendActionClass']
            if 'failureBoundary' in value:
                boundary = value['failureBoundary']
                if (type(boundary) is not str or boundary not in {
                        'request','policy','native-window','source-owner','tree','tree-cycle','tree-depth','tree-limit','tree-identity','tree-children','response-heading','response-row','response-row-role-limit','response-row-copy-absent','response-row-copy-ambiguous','response-row-headings','response-row-attachment','state','frame','frame-active','frame-state','frame-identity','frame-bounds','frame-ancestry-cycle','frame-ancestry-depth','frame-nested-dialog','frame-nested-frame','frame-nested-window','frame-editor-outside','frame-count','frame-client','client',
                        'mode','focus','input','input-mapping-state','input-mapping-changed','input-empty-state','input-empty-witness','clipboard','action','action-count','action-name','action-hit','response','transport','transport-spawn','transport-io','transport-wait',
                        'transport-status','transport-size','transport-decode','transport-deadline'}
                        or value['stage'] not in {'blocked','action-uncertain','deadline','clipboard-cleanup',
                                                 'input-not-empty','response-mismatch'}):
                    raise ValueError('invalid Claude Linux Chat failure boundary')
                record['failureBoundary'] = boundary
            if 'inputShape' in value:
                shape = value['inputShape']
                flags = {'onlyLineBreaks','onlyWhitespace','onlyZeroWidthMarkers'}
                if (type(shape) is not dict or set(shape) - {'onlyObjectReplacement'} != flags | {'charCount'}
                        or type(shape['charCount']) is not int or not 1 <= shape['charCount'] <= 4096
                        or any(type(shape[key]) is not bool for key in flags)
                        or 'onlyObjectReplacement' in shape and type(shape['onlyObjectReplacement']) is not bool
                        or shape.get('onlyObjectReplacement') is True and any(shape[key] for key in flags)
                        or shape['onlyLineBreaks'] and not shape['onlyWhitespace']
                        or shape['onlyZeroWidthMarkers'] and (shape['onlyWhitespace'] or shape['onlyLineBreaks'])
                        or value['stage'] not in {'input-not-empty','blocked','deadline','clipboard-cleanup'}):
                    raise ValueError('invalid Claude Linux input shape')
                record['inputShape'] = shape
            if 'embeddedTextObservation' in value:
                shape = value['embeddedTextObservation']
                keys = {'nodeCount','paragraphCount','literalLfLeafCount','brLfLeafCount','exactFillerLfLeafCount'}
                if (type(shape) is not dict or set(shape) != keys
                        or any(type(shape[key]) is not int or not 0 <= shape[key] <= 64 for key in keys)
                        or shape['nodeCount'] == 0 or shape['paragraphCount'] > shape['nodeCount']
                        or not shape['exactFillerLfLeafCount'] <= shape['brLfLeafCount'] <= shape['literalLfLeafCount'] <= shape['nodeCount']
                        or value['stage'] != 'input-not-empty' or 'inputShape' not in value
                        or len({value[key] for key in ('submittedTurns','inputVerifiedTurns','copiedResponses')}) != 1
                        or value['submittedTurns'] > 2
                        or value['retryAttempted'] is not False):
                    raise ValueError('invalid Claude Linux embedded text observation')
                record['embeddedTextObservation'] = shape
            if 'ownedInputObservation' in value:
                shape = value['ownedInputObservation']
                counts = set('nodeCount resolvedNodeCount paragraphCount rootChildCount textLeafCount otherRoleCount objectLinkCount knownPromptMatchCount'.split())
                flags = set('completeTextCoverage rootSingleParagraph rootOnlyObjects placeholderAttributeMatch placeholderAttributeLfMatch latestPromptMatches'.split())
                if (type(shape) is not dict or set(shape) - {'sourceShape'} != counts | flags
                        or any(type(shape[k]) is not int or not 0 <= shape[k] <= 64 for k in counts)
                        or any(type(shape[k]) is not bool for k in flags)
                        or shape['nodeCount'] == 0 or shape['resolvedNodeCount'] == 0
                        or any(shape[k] > shape['nodeCount'] for k in counts - {'knownPromptMatchCount'})
                        or shape['objectLinkCount'] >= shape['nodeCount']
                        or shape['completeTextCoverage'] is not (shape['resolvedNodeCount'] == shape['nodeCount'])
                        or (shape['rootSingleParagraph'] and (shape['rootChildCount'] != 1 or shape['paragraphCount'] == 0))
                        or (shape['rootOnlyObjects'] and shape['objectLinkCount'] == 0)
                        or shape['knownPromptMatchCount'] > 1
                        or (shape['latestPromptMatches'] and shape['knownPromptMatchCount'] != 1)
                        or value['stage'] != 'input-not-empty' or 'embeddedTextObservation' not in value
                        or shape['nodeCount'] != value['embeddedTextObservation']['nodeCount']
                        or shape['paragraphCount'] != value['embeddedTextObservation']['paragraphCount']
                        or value['submittedTurns'] != value['inputVerifiedTurns'] or value['submittedTurns'] != value['copiedResponses']
                        or not 1 <= value['submittedTurns'] <= 2 or value['retryAttempted'] is not False):
                    raise ValueError('invalid Claude Linux owned input observation')
                if 'sourceShape' in shape:
                    source=shape['sourceShape']
                    paragraph=set('paragraphTagPCount paragraphEmptyClassPairCount paragraphDataPlaceholderCount'.split())
                    totals=set('unresolvedTextLeafCount unresolvedOtherRoleCount'.split())
                    kinds=set('unresolvedEmptyTextCount unresolvedLfTextCount unresolvedExactResultCount unresolvedOtherTextCount'.split())
                    if (type(source) is not dict or set(source)!=paragraph|totals|kinds
                            or any(type(n) is not int or not 0<=n<=64 for n in source.values())
                            or any(source[k]>shape['paragraphCount'] for k in paragraph)
                            or sum(source[k] for k in totals)!=shape['nodeCount']-shape['resolvedNodeCount']
                            or sum(source[k] for k in kinds)!=source['unresolvedTextLeafCount']):
                        raise ValueError('invalid Claude Linux owned input source shape')
                record['ownedInputObservation'] = shape
            record.update({key:value[key] for key in fields - {'schemaVersion','mechanism'}})
        elif mechanism == 'claude-config-persist-owners':
            fields = {'schemaVersion','mechanism','diagnosticsOnly','status','stage','destinationPresent',
                      'ownerCount','currentProcessCount','otherProcessCount'}
            counters = {'ownerCount','currentProcessCount','otherProcessCount'}
            if (app != 'claude-desktop' or set(value) - {'sourceDeleteAccess'} != fields or value['diagnosticsOnly'] is not True
                    or type(value['status']) is not str or value['status'] not in {'observed','unavailable','deadline'}
                    or type(value['stage']) is not str or value['stage'] not in {
                        'request','platform','scope','session','register','list','identity','deadline','query','complete'}):
                raise ValueError('invalid Claude persist owner diagnostic')
            if 'sourceDeleteAccess' in value:
                if (value['status'] != 'observed' or type(value['sourceDeleteAccess']) is not str
                        or value['sourceDeleteAccess'] not in {'available','sharing-denied','access-denied','missing','query-failed'}):
                    raise ValueError('invalid Claude source delete diagnostic')
                record['sourceDeleteAccess'] = value['sourceDeleteAccess']
            if value['status'] == 'observed':
                if (value['stage'] != 'complete' or type(value['destinationPresent']) is not bool
                        or any(type(value[key]) is not int or not 0 <= value[key] <= 64 for key in counters)
                        or value['currentProcessCount'] > 1
                        or value['ownerCount'] != value['currentProcessCount'] + value['otherProcessCount']):
                    raise ValueError('invalid Claude persist owner counts')
            elif (value['stage'] == 'complete' or (value['status'] == 'deadline') != (value['stage'] == 'deadline')
                    or value['destinationPresent'] is not None or any(value[key] is not None for key in counters)):
                raise ValueError('incomplete Claude persist owner diagnostic')
            record.update({key:value[key] for key in fields - {'schemaVersion','mechanism'}})
        elif mechanism == 'claude-cli-prelaunch':
            fields = {'schemaVersion', 'mechanism', 'phase', 'stage', 'status', 'diagnosticsOnly'}
            optional = {'configurationSubstage', 'configurationDocument', 'configurationIoFailure', 'configurationFileAttributes', 'stdRenameSelected', 'stdRenameBoundary'}
            if (not fields <= set(value) or set(value) - fields - optional or app != 'claude-desktop' or path.name != 'claude-cli-prelaunch.json'
                    or value['diagnosticsOnly'] is not True or value['phase'] != 'prelaunch'
                    or value['status'] != 'failed' or type(value['stage']) is not str or value['stage'] not in {
                        'persistence', 'remembered-model', 'paths', 'session-lock', 'pending-recovery',
                        'process-query', 'process-present', 'credentials', 'bridge', 'snapshot',
                        'receipt-write', 'configuration', 'vendor-launch'}):
                raise ValueError('invalid Claude prelaunch observation')
            if 'configurationSubstage' in value:
                substage = value['configurationSubstage']
                documents = {'normal-config', 'third-party-config', 'metadata', 'profile'}
                policy = {'mac-policy', 'windows-policy', 'linux-policy'}
                writes = {'serialize', 'existing-permissions', 'parent-create', 'path-check',
                          'temporary-create', 'temporary-write', 'temporary-permissions', 'persist'}
                if (value['stage'] != 'configuration' or type(substage) is not str
                        or substage not in policy | writes | {'document-read', 'managed-mcp'}
                        or (substage in policy and 'configurationDocument' in value)
                        or (substage not in policy and (type(value.get('configurationDocument')) is not str
                            or value['configurationDocument'] not in documents))
                        or (substage == 'managed-mcp' and value.get('configurationDocument') != 'profile')):
                    raise ValueError('invalid Claude configuration boundary')
                record['configurationSubstage'] = substage
                if 'configurationDocument' in value:
                    record['configurationDocument'] = value['configurationDocument']
            elif 'configurationDocument' in value:
                raise ValueError('invalid Claude configuration boundary')
            if 'configurationIoFailure' in value:
                failure = value['configurationIoFailure']
                if (value['stage'] != 'configuration' or value.get('configurationSubstage') != 'persist'
                        or type(failure) is not str or failure not in {'sharing-violation', 'access-denied',
                            'invalid-name', 'path-not-found', 'already-exists', 'invalid-input', 'other'}):
                    raise ValueError('invalid Claude configuration I/O failure')
                record['configurationIoFailure'] = failure
            if 'stdRenameSelected' in value or 'stdRenameBoundary' in value:
                selected=value.get('stdRenameSelected')
                boundary=value.get('stdRenameBoundary')
                boundaries={'original-source','bridge-reader','retained-reader','destination-preflight',
                            'rename-dispatch','destination-identity','private-postcheck','deadline'}
                if (type(selected) is not bool or value.get('configurationDocument')!='normal-config'
                        or value.get('configurationSubstage')!='persist' or 'configurationIoFailure' not in value
                        or selected and (type(boundary) is not str or boundary not in boundaries)
                        or not selected and 'stdRenameBoundary' in value):
                    raise ValueError('invalid Claude std rename boundary')
                record['stdRenameSelected']=selected
                if selected: record['stdRenameBoundary']=boundary
            if 'configurationFileAttributes' in value:
                attributes = value['configurationFileAttributes']
                keys = {'temporaryBefore', 'temporaryAfter', 'readonlyBefore', 'readonlyAfter'}
                if (value.get('configurationDocument') != 'normal-config'
                        or value.get('configurationSubstage') != 'persist'
                        or value.get('configurationIoFailure') != 'sharing-violation'
                        or type(attributes) is not dict or set(attributes) != keys
                        or any(attributes[key] is not None and type(attributes[key]) is not bool for key in keys)
                        or (attributes['temporaryBefore'] is None) != (attributes['readonlyBefore'] is None)
                        or (attributes['temporaryAfter'] is None) != (attributes['readonlyAfter'] is None)):
                    raise ValueError('invalid Claude persist file attributes')
                record['configurationFileAttributes'] = attributes
            record.update(diagnosticsOnly=True, phase=value['phase'], stage=value['stage'], status=value['status'])
        elif mechanism == 'claude-linux-classic-visibility':
            observation = {'visible', 'showing', 'boundsPositive', 'checkedAncestorCount', 'hiddenAncestorCount'}
            fields = {'schemaVersion', 'mechanism', 'diagnosticsOnly', 'status', 'stage'} | observation
            stages = {'policy', 'deadline', 'guard', 'source', 'owner', 'state', 'bounds', 'parent', 'identity'}
            if (app != 'claude-desktop' or set(value) != fields or value.get('diagnosticsOnly') is not True
                    or type(value.get('status')) is not str or value.get('status') not in {'complete', 'unavailable', 'changed', 'limit'}):
                raise ValueError('invalid Claude Linux visibility observation')
            if value['status'] == 'complete':
                checked, hidden = value.get('checkedAncestorCount'), value.get('hiddenAncestorCount')
                if (value.get('stage') != 'complete'
                        or any(type(value.get(key)) is not bool for key in ('visible', 'showing', 'boundsPositive'))
                        or type(checked) is not int or type(hidden) is not int or not 0 <= hidden <= checked <= 32):
                    raise ValueError('invalid Claude Linux visibility states')
            elif (type(value.get('stage')) is not str or value.get('stage') not in stages or any(value.get(key) is not None for key in observation)
                    or (value['status'] == 'limit' and value['stage'] != 'parent')
                    or (value['status'] == 'changed' and value['stage'] != 'identity')):
                raise ValueError('inconsistent Claude Linux visibility observation')
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
        elif mechanism == 'claude-windows-profile-seal':
            document_stages={'document-metadata','document-open','document-privacy','document-lock','document-json'}
            stages=document_stages|{'initial-custody','native-policy','bridge-authority','library-metadata','library-lock',
                'configuration-values','final-custody','deadline','completed'}
            fields=set('schemaVersion mechanism diagnosticsOnly stage documentIndex completed'.split())
            privacy_fields={'rootPrivacy','libraryPrivacy','documentPrivacy'}
            if (app!='claude-desktop' or set(value) not in (fields,fields|privacy_fields,fields|privacy_fields|{'configurationFailure'},fields|privacy_fields|{'configurationFailure','bridgeAuthority'})
                    or value['diagnosticsOnly'] is not True or type(value['stage']) is not str or value['stage'] not in stages
                    or type(value['completed']) is not bool or value['completed']!=(value['stage']=='completed')
                    or value['stage'] in document_stages and (type(value['documentIndex']) is not int or not 0<=value['documentIndex']<=2)
                    or value['stage'] not in document_stages and value['documentIndex'] is not None):
                raise ValueError('invalid Claude Windows profile seal observation')
            if privacy_fields<=set(value):
                allowed={'protected','inherited','unexpected','unavailable'}
                nullable=lambda v:v is None or type(v) is str and v in allowed
                accepted=lambda v:type(v) is str and v in {'protected','inherited'}
                documents=value['documentPrivacy']
                if (not nullable(value['rootPrivacy']) or not nullable(value['libraryPrivacy'])
                        or type(documents) is not list or len(documents)!=3 or not all(map(nullable,documents))
                        or value['completed'] and (value['rootPrivacy']!='protected'
                            or not accepted(value['libraryPrivacy']) or not all(map(accepted,documents)))
                        or value['documentIndex'] is not None and any(documents[i] is not None
                            for i in range(value['documentIndex']+1,3))):
                    raise ValueError('invalid Claude Windows immutable privacy observation')
                record.update({k:value[k] for k in privacy_fields})
            if 'bridgeAuthority' in value:
                bridge=value['bridgeAuthority']
                enums={
                    'stage':set('root-custody receipt-metadata receipt-open receipt-lock receipt-privacy receipt-json receipt-schema receipt-values endpoint-owner final-custody completed'.split()),
                    'failure':set('original-cutoff root-custody receipt-missing receipt-metadata receipt-open receipt-sharing receipt-lock receipt-privacy receipt-json receipt-schema schema-version process-identity token-format url-parse url-policy url-port proof-environment proof-input proof-spawn proof-wait proof-exit proof-output endpoint-rejected'.split()),
                    'privacy':{'protected','inherited','unexpected','unavailable'},
                    'endpointReason':set('owned listener-missing listener-ambiguous listener-nonloopback listener-owner-mismatch listener-changed process-budget process-unavailable parent-unavailable parent-reused session-mismatch ancestry-cycle ancestry-limit query-failed'.split())}
                if (type(bridge) is not dict or set(bridge)!=set(enums)
                        or any(v is not None and (type(v) is not str or v not in enums[k]) for k,v in bridge.items())
                        or bridge['stage'] is None and any(v is not None for v in bridge.values())
                        or bridge['stage']=='completed' and (bridge['failure'] is not None or bridge['privacy']!='protected' or bridge['endpointReason']!='owned')
                        or value['completed'] and bridge['stage']!='completed'):
                    raise ValueError('invalid Claude bridge authority diagnostic')
                record['bridgeAuthority']=bridge
            if 'configurationFailure' in value:
                failure=value['configurationFailure']
                failures={'document-count','deployment-mode','applied-profile','provider','base-url',
                    'authentication-key','authentication-scheme','hybrid-pointer','profile-entries',
                    'deployment-chooser','chat-only','alternate-configuration'}
                if failure is not None and (type(failure) is not str or failure not in failures
                        or value['stage']!='configuration-values' or value['completed']):
                    raise ValueError('invalid Claude Windows configuration failure')
                if value['stage']=='configuration-values' and failure is None:
                    raise ValueError('missing Claude Windows configuration failure')
                record['configurationFailure']=failure
            record.update({key:value[key] for key in ['diagnosticsOnly','stage','documentIndex','completed']})
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
        elif mechanism == 'windows-owned-cleanup-preflight-progress':
            stages=['none','file-open','file-hash','file-identity','process-open','snapshot','targets','owner-recheck']
            if (app!='claude-desktop' or set(value)!=set('schemaVersion mechanism diagnosticsOnly completedStageCount lastCompletedStage outcome'.split())
                    or value['diagnosticsOnly'] is not True or type(value['completedStageCount']) is not int
                    or not 0<=value['completedStageCount']<=7
                    or value['lastCompletedStage']!=stages[value['completedStageCount']]
                    or type(value['outcome']) is not str or value['outcome'] not in {'deadline','transport','protocol','rejected','ready'}):
                raise ValueError('invalid Windows cleanup preflight progress')
            record.update({k:value[k] for k in ['diagnosticsOnly','completedStageCount','lastCompletedStage','outcome']})
        elif mechanism == 'windows-owned-cleanup-preflight':
            fields = set('schemaVersion mechanism diagnosticsOnly stage'.split())
            stages = set('request path file-open file-hash file-identity process-open snapshot inspector-parent ancestry target-open target-identity target-creation target-state target-image owner-recheck deadline transport'.split())
            stages.update('target-image-' + part for part in 'query sharing access open canonical metadata path volume file-id file-id-query size write-time'.split())
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
            if 'roleShape' in value:
                fields.add('roleShape')
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
            if 'roleShape' in value:
                shape = value['roleShape']
                shape_keys = set('buttonAll buttonVisible radioAll radioVisible switchAll switchVisible staticTextAll staticTextVisible chatAll chatAwaitingAll chatUnreadAll chatWorkingAll coworkAll coworkAwaitingAll coworkUnreadAll coworkWorkingAll'.split())
                if (type(shape) is not dict or set(shape) != {'status', 'counts'}
                        or type(shape['status']) is not str or shape['status'] not in {'observed', 'unavailable'}
                        or type(shape['counts']) is not dict or set(shape['counts']) != shape_keys):
                    raise ValueError('invalid Linux Mode shape')
                shape_counts = shape['counts']
                if ((shape['status'] == 'observed' and any(type(count) is not int or not 0 <= count <= 4096 for count in shape_counts.values()))
                        or (shape['status'] == 'unavailable' and any(count is not None for count in shape_counts.values()))
                        or (shape['status'] == 'observed' and any(shape_counts[key + 'Visible'] > shape_counts[key + 'All'] for key in ('button', 'radio', 'switch', 'staticText')))):
                    raise ValueError('inconsistent Linux Mode shape')
                record['roleShape'] = dict(status=shape['status'], counts=dict(shape_counts))
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
            if 'classicRoleShape' in value:
                fields.add('classicRoleShape')
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
            if 'classicRoleShape' in value:
                shape = value['classicRoleShape']
                role_keys = {'textArea', 'textField', 'editableTextArea', 'editableTextField'}
                if (expected_version != '2.9939.4' or type(shape) is not dict
                        or set(shape) != {'status', 'counts'} or type(shape['status']) is not str or shape['status'] not in {'observed', 'unavailable'}
                        or type(shape['counts']) is not dict or set(shape['counts']) != role_keys):
                    raise ValueError('invalid Claude classic role shape')
                roles = shape['counts']
                if shape['status'] == 'unavailable':
                    if any(item is not None for item in roles.values()):
                        raise ValueError('partial Claude classic role shape')
                elif (any(type(item) is not int or not 0 <= item <= 4096 for item in roles.values())
                      or roles['editableTextArea'] > roles['textArea']
                      or roles['editableTextField'] > roles['textField']):
                    raise ValueError('inconsistent Claude classic role shape')
                record['classicRoleShape'] = {'status': shape['status'], 'counts': dict(roles)}
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
            for field in ('transientDialogs', 'transientDialogsBeforeHover', 'transientDialogsBeforeDispatch'):
                if field not in value:
                    continue
                dialogs = value[field]
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
                record[field] = dict(dialogs)
            if 'targetAfterClick' in value:
                target = value['targetAfterClick']
                if type(target) is not str or target not in {'unavailable','defunct','same-source','changed-source'}:
                    raise ValueError('invalid Zed retained target observation')
                record['targetAfterClick'] = target
            if 'xi2Motion' in value:
                record['xi2Motion']=zed_xi2_motion(value['xi2Motion'])
            if (set(value) - modifier_fields - ancestor_fields - ancestor_stage_fields - {'cursorSelection', 'transientDialogs','transientDialogsBeforeHover','transientDialogsBeforeDispatch','entryCrossing','xi2Motion','retryHitPolicy','targetAfterClick'} not in (base, base | {'inputDelivery'}, base | coordinate_fields,
                                  base | coordinate_fields | {'inputDelivery'}, base | coordinate_fields | authority_fields,
                                  base | coordinate_fields | authority_fields | {'inputDelivery'})
                    or present_modifiers and present_modifiers != modifier_fields
                    or app != 'zed-desktop' or value['diagnosticsOnly'] is not True):
                raise ValueError('invalid Zed pointer observation identity')
            if 'entryCrossing' in value:
                entry=value['entryCrossing']
                stages={'preflight','observer','frame-measurement','candidate',
                    'decoration-before','decoration-dispatch','decoration-after','decoration-complete',
                    'client-before','client-dispatch','client-after','client-complete'}
                reasons={'observer-unavailable','frame-relationship','frame-geometry','decoration-unavailable',
                    'off-display','point-ownership','top-frame-hit','decoration-child-hit','client-child-hit',
                    'pointer-position','pointer-child-current','pointer-state','identity-changed','deadline',
                    'query-unavailable','motion-uncertain'}
                if (type(entry) is not dict or set(entry)!={'stage','failureReason'}
                        or type(entry['stage']) is not str or entry['stage'] not in stages
                        or entry['failureReason'] is not None and (type(entry['failureReason']) is not str
                            or entry['failureReason'] not in reasons)
                        or entry['stage'].endswith('-complete') and entry['failureReason'] is not None):
                    raise ValueError('invalid Zed entry-crossing diagnostic')
                record['entryCrossing']=dict(entry)
            if 'retryHitPolicy' in value:
                if value['retryHitPolicy']!='accessibility':
                    raise ValueError('invalid Zed accessible target policy')
                record['retryHitPolicy']=value['retryHitPolicy']
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
                        or type(selection['status']) is not str or selection['status'] not in {'matched', 'accessible-hit', 'unavailable', 'no-hit', 'deadline', 'identity-rejected'}
                        or type(selection['sampledPoints']) is not int or not 0 <= selection['sampledPoints'] <= 9
                        or any(type(selection[key]) is not bool for key in ('exactPointerMatched', 'accessibleHitVerified'))
                        or (selection['status'] == 'matched') != (selection['exactPointerMatched'] and selection['accessibleHitVerified'])
                        or selection['status'] == 'matched' and selection['sampledPoints'] == 0
                        or selection['status'] not in {'matched','accessible-hit'} and (selection['exactPointerMatched'] or selection['accessibleHitVerified'])):
                    raise ValueError('invalid Zed cursor selection')
                if selection['status']=='accessible-hit' and (value.get('retryHitPolicy')!='accessibility'
                        or selection['accessibleHitVerified'] is not True or selection['exactPointerMatched'] is not False
                        or selection.get('failureReason') is not None):
                    raise ValueError('invalid Zed accessible target proof')
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
                if (type(delivery) is not dict or set(delivery)-{'crossingHeaders','captureEnd','failureReason'} not in (fields, fields | {'stage'})
                        or type(delivery['status']) is not str or delivery['status'] not in statuses):
                    raise ValueError('invalid Zed input delivery observation')
                if 'captureEnd' in delivery and (delivery['status'] != 'complete'
                        or type(delivery['captureEnd']) is not str or delivery['captureEnd'] not in {'finish','cutoff'}):
                    raise ValueError('invalid Zed capture end')
                if 'failureReason' in delivery and (delivery['status'] == 'complete'
                        or type(delivery['failureReason']) is not str or delivery['failureReason'] not in {
                            'worker-cutoff','worker-request','armed-select','native-pump',
                            'finish-request','native-snapshot','receipt-validation'}):
                    raise ValueError('invalid Zed record failure')
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

                if 'crossingHeaders' in delivery:
                    headers=delivery['crossingHeaders']
                    keys={'ownedNormalEnterCount','ownedNonNormalEnterCount','ownedNormalLeaveCount','ownedMotionCount'}
                    if (delivery['status']!='complete' or type(headers) is not dict or set(headers)-{'eventOrder'}!=keys|{'status'}
                            or type(headers['status']) is not str or headers['status'] not in {'observed','unavailable'}
                            or headers['status']=='observed' and any(type(headers[key]) is not int or not 0<=headers[key]<=64 for key in keys)
                            or headers['status']=='unavailable' and any(headers[key] is not None for key in keys)):
                        raise ValueError('invalid Zed crossing-header observation')
                    if 'eventOrder' in headers:
                        order=headers['eventOrder']
                        if (headers['status']=='unavailable' and order is not None
                                or headers['status']=='observed' and (type(order) is not list or len(order)>128
                                    or any(type(event) is not str or event not in {
                                        'enter','non-normal-enter','leave','motion','press','release'} for event in order))):
                            raise ValueError('invalid Zed crossing event order')
                record['inputDelivery'] = dict(delivery)
        elif mechanism == 'windows-foreground-session':
            fields = set('schemaVersion mechanism diagnosticsOnly stage originalTimeoutMs prepared restored'.split())
            timeout = value.get('originalTimeoutMs')
            if (app != 'claude-desktop' or set(value) - {'failureStage'} != fields or value['diagnosticsOnly'] is not True
                    or type(value['stage']) is not str or value['stage'] not in {'read', 'prepare', 'verify', 'running', 'restore', 'completed'}
                    or timeout is not None and (type(timeout) is not int or not 0 <= timeout <= 4294967295)
                    or type(value['prepared']) is not bool or type(value['restored']) is not bool
                    or (value['prepared'] or value['restored']) and timeout is None
                    or value['stage'] == 'completed' and not (value['prepared'] and value['restored'])):
                raise ValueError('invalid Windows foreground session receipt')
            record.update({key: value[key] for key in fields - {'schemaVersion', 'mechanism'}})
            enum(record, value, 'failureStage', {'read', 'prepare', 'verify', 'running', 'restore'})
        elif mechanism == 'zed-retry-entry-counts':
            fields = set('schemaVersion mechanism diagnosticsOnly status stage cleanup retryEntries nativeRetryEntries'.split())
            if (set(value) - {'inputDispatchEntries', 'activationWindows', 'attachFailure', 'hitTestGeometry', 'readbackFailure'} != fields or app != 'zed-desktop' or value['diagnosticsOnly'] is not True
                    or type(value['status']) is not str or value['status'] not in {'complete', 'unavailable'}
                    or type(value['stage']) is not str or value['stage'] not in {'attach', 'stop', 'readback', 'complete'}
                    or (value['status'] == 'complete') != (value['stage'] == 'complete')
                    or type(value['cleanup']) is not str or value['cleanup'] not in {'passed', 'failed'}):
                raise ValueError('invalid Zed entry count identity')
            counts = [value['retryEntries'], value['nativeRetryEntries']]
            if (value['status'] == 'complete' and (value['cleanup'] != 'passed'
                    or any(type(count) is not int or not 0 <= count <= 1024 for count in counts))
                    or value['status'] == 'unavailable' and counts != [None, None]):
                raise ValueError('invalid Zed entry counts')
            if 'inputDispatchEntries' in value:
                inputs = value['inputDispatchEntries']
                if (value['status'] == 'complete' and (type(inputs) is not int or not 0 <= inputs <= 65536)
                        or value['status'] == 'unavailable' and inputs is not None):
                    raise ValueError('invalid Zed input entry count')
            enum(record, value, 'attachFailure', {'unclassified', 'version-mismatch', 'compiler-stack', 'compiler-syntax', 'tracepoint-unavailable', 'permission', 'program-load', 'symbol-unavailable', 'tracer-error', 'readiness-incomplete', 'tracer-exited'})
            enum(record, value, 'readbackFailure', set('geometry-output-budget geometry-output-shape geometry-map-duplicate geometry-map-budget geometry-tuple geometry-fields user-memory-read marker-read helper-error lost-events counter-output-budget counter-output-shape activation-counter activation-incomplete dispatch-returns counter-value counter-incomplete rejected'.split()))
            if value.get('readbackFailure') is not None and (value['stage'] != 'readback' or value['status'] != 'unavailable'):
                raise ValueError('invalid Zed readback failure state')
            if 'activationWindows' in value:
                clicks = value['activationWindows']
                if (value['status'] != 'complete' or type(clicks) is not dict
                        or set(clicks) != {'started', 'ended', 'windows'}
                        or type(clicks['started']) is not int or type(clicks['ended']) is not int
                        or not 0 <= clicks['started'] == clicks['ended'] <= 3
                        or type(clicks['windows']) is not list or len(clicks['windows']) != 3):
                    raise ValueError('invalid activation windows')
                for window in clicks['windows']:
                    base_fields = {'retryEntries', 'nativeRetryEntries', 'inputDispatchEntries', 'errorClearEntries'}
                    return_fields = {'inputDispatchReturns', 'inputPropagationStops', 'inputDefaultPreventions',
                                     'inputInvalidReturns', 'hoverTrueReturns', 'hoverFalseReturns', 'hoverInvalidReturns'}
                    if (type(window) is not dict or set(window) not in (base_fields, base_fields | return_fields)
                            or any(type(count) is not int or not 0 <= count <= (1024 if key in {'retryEntries', 'nativeRetryEntries', 'errorClearEntries'} else 65536)
                                   for key, count in window.items())):
                        raise ValueError('invalid activation window counts')
                    if return_fields <= set(window) and any(window[key] > window['inputDispatchReturns']
                            for key in ('inputPropagationStops', 'inputDefaultPreventions', 'inputInvalidReturns')):
                        raise ValueError('inconsistent dispatch returns')
            if 'hitTestGeometry' in value:
                geometry = value['hitTestGeometry']
                if (value['status'] != 'complete' or type(geometry) is not dict
                        or set(geometry) != {'status', 'windows'}
                        or geometry['status'] not in ('complete', 'unavailable')
                        or type(geometry['windows']) is not list
                        or 'activationWindows' not in value
                        or len(geometry['windows']) != (value['activationWindows']['started'] if geometry['status'] == 'complete' else 0)):
                    raise ValueError('invalid hit-test geometry identity')
                for item in geometry['windows']:
                    if (type(item) is not dict or set(item) != {'status', 'renderedHitboxes', 'boundsMatches',
                            'priorPointerMatches', 'targetMaskContainsPoint', 'blockingHitboxesAhead', 'targetWouldBeHovered'}
                            or type(item['renderedHitboxes']) is not int or not 1 <= item['renderedHitboxes'] <= 1024
                            or type(item['boundsMatches']) is not int or not 0 <= item['boundsMatches'] <= item['renderedHitboxes']
                            or type(item['priorPointerMatches']) is not bool
                            or item['status'] != ('matched' if item['boundsMatches'] == 1 else 'absent' if item['boundsMatches'] == 0 else 'ambiguous')):
                        raise ValueError('invalid hit-test geometry counts')
                    if item['status'] == 'matched':
                        if (type(item['targetMaskContainsPoint']) is not bool or type(item['targetWouldBeHovered']) is not bool
                                or type(item['blockingHitboxesAhead']) is not int
                                or not 0 <= item['blockingHitboxesAhead'] < item['renderedHitboxes']
                                or item['targetWouldBeHovered'] != (item['targetMaskContainsPoint'] and item['blockingHitboxesAhead'] == 0)):
                            raise ValueError('inconsistent hit-test geometry')
                    elif any(item[key] is not None for key in ('targetMaskContainsPoint', 'blockingHitboxesAhead', 'targetWouldBeHovered')):
                        raise ValueError('ambiguous hit-test geometry')
            record.update(value)
        elif mechanism == 'zed-atspi-retry':
            fields = set('schemaVersion mechanism diagnosticsOnly method stage actionAttempted forwarded'.split())
            if (set(value) - {'postTargetState'} != fields or app != 'zed-desktop' or value['diagnosticsOnly'] is not True
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
            if 'postTargetState' in value:
                if (value['stage'] != 'postflight' or not value['forwarded']
                        or type(value['postTargetState']) is not str
                        or value['postTargetState'] not in {'unchanged', 'defunct', 'changed', 'unavailable'}):
                    raise ValueError('invalid retained Zed target observation')
                record['postTargetState'] = value['postTargetState']
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
            if set(value) - {'responseShape'} != set('schemaVersion mechanism diagnosticsOnly category assistantTurnCount'.split()) or app != 'hermes-desktop' or value['diagnosticsOnly'] is not True:
                raise ValueError('invalid Hermes backend failure identity')
            allowed = {'python-import-failure', 'provider-unconfigured', 'backend-unavailable', 'invalid-model', 'permission-denied', 'connection-failed', 'multiple', 'unclassified'}
            if type(value['category']) is not str or value['category'] not in allowed or type(value['assistantTurnCount']) is not int or not 0 <= value['assistantTurnCount'] <= 4096:
                raise ValueError('invalid Hermes backend failure facts')
            record.update(diagnosticsOnly=True, category=value['category'], assistantTurnCount=value['assistantTurnCount'])
            if 'responseShape' in value:
                shape = value['responseShape']
                keys = {'exactUserCount', 'markerAssistantCount', 'boundMarkerAssistantCount'}
                if (type(shape) is not dict or set(shape) != keys
                        or any(type(shape[key]) is not int or not 0 <= shape[key] <= 4096 for key in keys)
                        or shape['boundMarkerAssistantCount'] > shape['markerAssistantCount']
                        or shape['markerAssistantCount'] > value['assistantTurnCount']):
                    raise ValueError('invalid Hermes response shape')
                record['responseShape'] = shape
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
            if (set(value) - {'preparationReason'} != fields or value['diagnosticsOnly'] is not True
                    or type(value['errorCategory']) is not str or value['errorCategory'] not in RUNNER_FAILURES):
                raise ValueError('invalid qualification runner failure')
            if 'preparationReason' in value:
                reason = value['preparationReason']
                if (value['errorCategory'] != 'prepared-app-unavailable'
                        or type(reason) is not str or reason not in PREPARATION_FAILURES):
                    raise ValueError('invalid preparation reason')
                record['preparationReason'] = reason
            record.update(diagnosticsOnly=True, errorCategory=value['errorCategory'])
        elif mechanism == 'qualification-reduction-failure':
            fields = set('schemaVersion mechanism diagnosticsOnly category reportPresent observationCount metadataCount countsCapped'.split())
            if (set(value) != fields or value['diagnosticsOnly'] is not True
                    or type(value['category']) is not str or value['category'] not in REDUCTION_CATEGORIES
                    or type(value['reportPresent']) is not bool or type(value['countsCapped']) is not bool
                    or any(value[key] is not None and (type(value[key]) is not int or not 0 <= value[key] <= 1024)
                           for key in ('observationCount', 'metadataCount'))):
                raise ValueError('invalid reduction failure receipt')
            record.update(value)
        elif mechanism == 'codex-renderer-qualification':
            flags = set('endpointOwned targetVerified attached bindingVerified auxiliaryInert codingComposerReady uniqueComposer inputReadback inputSubmitted userTurnObserved responseVerified errorObserved retryControl retryAttempted retryCompleted providerResponseVerified'.split())
            fields = flags | set('schemaVersion mechanism diagnosticsOnly assistantTurnCount providerGenerationCount errorCategory'.split())
            errors = {None, 'ownership-lost', 'composer-unavailable', 'stale-turn', 'input-mismatch', 'action-uncertain', 'retry-unavailable', 'response-timeout', 'query-failed', 'invalid-request'}
            if (app != 'chatgpt-desktop' or set(value) - {'preAttachFailure', 'composerAdmissionFailure', 'composerReadinessObservation', 'retryClickPhase'} != fields or value['diagnosticsOnly'] is not True
                    or any(type(value[key]) is not bool for key in flags)
                    or type(value['assistantTurnCount']) is not int or not 0 <= value['assistantTurnCount'] <= 4096
                    or value['providerGenerationCount'] is not None and (type(value['providerGenerationCount']) is not int or not 0 <= value['providerGenerationCount'] <= 4096)
                    or value['errorCategory'] is not None and type(value['errorCategory']) is not str
                    or value['errorCategory'] not in errors
                    or value['retryCompleted'] and not value['retryAttempted']):
                raise ValueError('invalid Codex renderer qualification')
            record.update({key: value[key] for key in fields - {'schemaVersion', 'mechanism'}})
            if 'retryClickPhase' in value:
                enum(record, value, 'retryClickPhase', {'admission','capture','sample-first',
                     'revalidate','sample-second','sample-final','dispatch','post-guard','complete'})
            if 'preAttachFailure' in value:
                if value['errorCategory'] != 'invalid-request' or value['attached']:
                    raise ValueError('unexpected Codex preattach failure')
                enum(record, value, 'preAttachFailure', {'request-json', 'request-policy',
                     'connection-read', 'binding-read', 'connection-schema', 'binding-schema'})
            if 'composerAdmissionFailure' in value:
                if value['errorCategory'] != 'composer-unavailable':
                    raise ValueError('unexpected Codex composer admission failure')
                enum(record, value, 'composerAdmissionFailure', {
                    'scope-not-ready', 'nonunique-editor', 'missing-editor', 'unsupported-control',
                    'detached-or-inert', 'disabled', 'hidden', 'foreign-overlay', 'pointer-disabled',
                    'ancestor-limit', 'hit-unavailable', 'sample-changed'})
            if 'composerReadinessObservation' in value:
                if value['errorCategory'] != 'composer-unavailable' or 'composerAdmissionFailure' not in value:
                    raise ValueError('unexpected Codex composer readiness observation')
                observed = value['composerReadinessObservation']
                counts = {'homeComposerCount', 'pendingTextareaCount', 'proseMirrorEditableCount',
                          'workspaceControlCount', 'editableCount', 'codexThreadCount', 'classicChatGPTCount'}
                if (type(observed) is not dict or set(observed) != counts | {'overflow'}
                        or type(observed['overflow']) is not bool
                        or any(observed[key] is not None and (type(observed[key]) is not int
                               or not 0 <= observed[key] <= 32) for key in counts)
                        or observed['overflow'] != any(observed[key] is None for key in counts)):
                    raise ValueError('invalid Codex composer readiness counts')
                record['composerReadinessObservation'] = observed
        elif mechanism == 'codex-linux-startup-dialog':
            hashes = dict(completeSourceSha256='16b6c59e36aa19da0c4ec1560b6cedec43fabffeca2601710cb6f25f22c593cc',
                          onboardingSourceSha256='b8dff84333a6cfb62341d43642087ba8d72dd31225ed2b3b8e29ad7da31372c6',
                          projectSourceSha256='802041599f534cdc852760bcc3eb18bc4bdc2fda523b8983098c5946476504a9')
            fields = set(hashes) | set('schemaVersion mechanism diagnosticsOnly sourceVersion status candidate sourceCount'.split())
            names = ('allSet', 'importedSetup', 'computerHistory', 'projectImport')
            keys = {'dialogCount'} | {name + suffix for name in names for suffix in ('TitleCount', 'MatchCount')}
            counts = value.get('sourceCount')
            candidates = ('all-set', 'imported-setup', 'computer-history', 'project-import')
            if (app != 'chatgpt-desktop' or set(value) != fields or value['diagnosticsOnly'] is not True
                    or value['sourceVersion'] != '26.930.31730' or any(value[key] != digest for key, digest in hashes.items())
                    or type(value['status']) is not str or value['status'] not in {'matched', 'other', 'ambiguous', 'guard-rejected'}
                    or type(value['candidate']) is not str or value['candidate'] not in set(candidates) | {'unknown', 'ambiguous'}
                    or type(counts) is not dict or set(counts) != keys):
                raise ValueError('invalid passive Linux Codex initial dialog')
            if value['status'] == 'guard-rejected':
                if value['candidate'] != 'unknown' or any(count is not None for count in counts.values()):
                    raise ValueError('invalid unavailable Linux Codex dialog')
            else:
                if (counts['dialogCount'] != 1 or any(type(count) is not int or not 0 <= count <= 32 for count in counts.values())
                        or any(counts[name + 'MatchCount'] not in {0, 1}
                               or counts[name + 'MatchCount'] == 1 and counts[name + 'TitleCount'] != 1 for name in names)):
                    raise ValueError('invalid Linux Codex source dialog counts')
                matched = [candidate for name, candidate in zip(names, candidates) if counts[name + 'MatchCount'] == 1]
                expected = ('matched', matched[0]) if len(matched) == 1 else ('ambiguous', 'ambiguous') if matched else ('other', 'unknown')
                if (value['status'], value['candidate']) != expected:
                    raise ValueError('inconsistent Linux Codex source dialog')
            record.update(diagnosticsOnly=True, sourceVersion=value['sourceVersion'], status=value['status'],
                          candidate=value['candidate'], sourceCount=dict(counts), **hashes)
        elif mechanism == 'codex-static-dialog-title':
            pins = {
                'windows': ('f7b0266d6c00d4743da01d62bc82488f7ec5560c642501758119cb9885f67c87',
                            '5e3a36d643393af861d2009584f64289f2247928e793f1985fe12cfec803a40b'),
                'macos': ('f6cf4d2e9b69aeefa33adda4bcd1a2d306357f5253a1ac6049700870c28dd0c7',
                          '0703d0aa97450d6d21346e1c79c887a5bf9062cd0069e8251ec03748a33b6dd0'),
                'linux': ('ee7854145554718d7239d01ea37d44f6ba1e0ba4a93f47ac097d6e0f964da47c',
                          'c3c9a86a6d9c3a2a8cecaf0a6a22527c69f89949cb0d8958896bc86131e9c6c9'),
            }
            allowed = {'macos': ['apps.connectMfa.title', 'chatGpt.quietHours.active.title', 'chatgpt.new-onboarding.all-done-2', 'chatgpt.pepper.personalization.voice.title', 'chatgptConversations.gpts.about.loadError', 'chatgptConversations.gpts.about.loading', 'codexMobile.setupPage.waiting.pairing.fullscreenQrCodeTitle', 'composer.worktreePrompt.education.title', 'electron.onboarding.conversationalOnboarding.skipDialog.title', 'keyboardShortcutsDialog.title', 'personalVault.importTitle', 'personalWorkspaceAccessBlocked.title', 'plugins.create.title', 'projectSetup.remoteSource.choose', 'projectSetupDialog.createMissingRemoteFolderTitle', 'restricted.aeon.slack.setup.title', 'restricted.environmentSetup.create.title', 'settings.chatGpt.security.orderYubikey.title', 'sitesPreview.handoff.title', 'speechSettings.content.customDictionary.page.title', 'workspaceAgents.sharing.removeTitle'], 'linux': ['appgenSettings.general.url.changeDialog.title', 'appgenSettings.transfer.dialogTitle', 'apps.connectMfa.title', 'artifactFeedback.title', 'automations.sharing.preview.dialog.title', 'businessProfiles.feedProducts.title', 'chatGpt.quietHours.active.title', 'chatgpt.new-onboarding.all-done-2', 'chatgpt.pepper.personalization.voice.title', 'chatgpt.tasks.run_history.title', 'chatgpt.versions.title', 'chatgptConversations.gpts.about.loadError', 'chatgptConversations.gpts.about.loading', 'chatgptConversations.gpts.mine.deleteTitle', 'chatgptConversations.imageUploadReminder.title', 'chatgptConversations.preciseLocation.permissionDenied.title', 'chatgptConversations.projectHome.textSource.title', 'chatgptConversations.projectSettings.title', 'chatgptConversations.summaryPanel.sources.memory.delete.title', 'chatgptProjectHome.website.title', 'codex.space.creation.discardTitle', 'codex.space.sitePage.attach.title', 'codex.writingBlock.confirmCloseTitle', 'codexMobile.setupPage.waiting.pairing.fullscreenQrCodeTitle', 'composer.hooks.review.title', 'composer.worktreePrompt.education.title', 'electron.onboarding.conversationalOnboarding.skipDialog.title', 'feedbackFormDialog.title', 'inbox.automations.habitatMigration.title', 'keyboardShortcutsDialog.title', 'libraryNext.flashcards.dialogTitle', 'libraryNext.move.title', 'localConversationPage.createPrModal.gitLabTitle', 'message.trustedContactNoThanksModal.title', 'orbit.primary.reboot.title', 'personalVault.importTitle', 'personalWorkspaceAccessBlocked.title', 'plugins.create.title', 'pricing.comparison.title', 'profile.sharing.title', 'projectSetup.remoteSource.choose', 'projectSetupDialog.createMissingRemoteFolderTitle', 'restricted.aeon.slack.setup.title', 'restricted.environmentSetup.create.title', 'settings.chatGpt.security.orderYubikey.title', 'settings.chatGpt.sharedLinks.deleteAllConfirmTitle', 'settings.chatGpt.sharedLinks.title', 'settings.cloudEnvironments.packages.title', 'settings.consumerBilling.downgradeTitle', 'settings.consumerBilling.renewalTitle', 'settings.general.realtimeVoice.dialog.title', 'settings.unsavedChanges.discardTitle', 'settings.usage.pricingPlanPage.subscriptionUpdate.title', 'sidebarElectron.deleteThreadDialog.title', 'sitesPreview.handoff.title', 'sitesPreview.share.title', 'speechSettings.content.customDictionary.page.title', 'teams.spaces.setup.loading', 'workspaceAgents.schedules.editor.title', 'workspaceAgents.sharing.removeTitle']}
            allowed['windows'] = ['appgenSettings.general.url.changeDialog.title', 'appgenSettings.transfer.dialogTitle', 'apps.connectMfa.title', 'artifactFeedback.title', 'automations.sharing.preview.dialog.title', 'businessProfiles.feedProducts.title', 'chatGpt.quietHours.active.title', 'chatgpt.new-onboarding.all-done-2', 'chatgpt.pepper.personalization.voice.title', 'chatgpt.tasks.run_history.title', 'chatgpt.versions.title', 'chatgptConversations.gpts.about.loadError', 'chatgptConversations.gpts.about.loading', 'chatgptConversations.gpts.mine.deleteTitle', 'chatgptConversations.imageUploadReminder.title', 'chatgptConversations.preciseLocation.permissionDenied.title', 'chatgptConversations.projectHome.textSource.title', 'chatgptConversations.projectSettings.title', 'chatgptConversations.summaryPanel.sources.memory.delete.title', 'chatgptProjectHome.website.title', 'codex.space.creation.discardTitle', 'codex.space.sitePage.attach.title', 'codex.writingBlock.confirmCloseTitle', 'codexMobile.setupPage.waiting.pairing.fullscreenQrCodeTitle', 'composer.hooks.review.title', 'composer.worktreePrompt.education.title', 'electron.onboarding.conversationalOnboarding.skipDialog.title', 'feedbackFormDialog.title', 'inbox.automations.habitatMigration.title', 'keyboardShortcutsDialog.title', 'libraryNext.flashcards.dialogTitle', 'libraryNext.move.title', 'localConversationPage.createPrModal.gitLabTitle', 'message.trustedContactNoThanksModal.title', 'orbit.primary.reboot.title', 'personalVault.importTitle', 'personalWorkspaceAccessBlocked.title', 'plugins.create.title', 'pricing.comparison.title', 'profile.sharing.title', 'projectSetup.remoteSource.choose', 'projectSetupDialog.createMissingRemoteFolderTitle', 'restricted.aeon.slack.setup.title', 'restricted.environmentSetup.create.title', 'settings.chatGpt.security.orderYubikey.title', 'settings.chatGpt.sharedLinks.deleteAllConfirmTitle', 'settings.chatGpt.sharedLinks.title', 'settings.cloudEnvironments.packages.title', 'settings.consumerBilling.downgradeTitle', 'settings.consumerBilling.renewalTitle', 'settings.general.realtimeVoice.dialog.title', 'settings.unsavedChanges.discardTitle', 'settings.usage.pricingPlanPage.subscriptionUpdate.title', 'sidebarElectron.deleteThreadDialog.title', 'sitesPreview.handoff.title', 'sitesPreview.share.title', 'speechSettings.content.customDictionary.page.title', 'teams.spaces.setup.loading', 'workspaceAgents.schedules.editor.title', 'workspaceAgents.sharing.removeTitle']
            for identities in allowed.values():
                identities.extend(['chatgpt.global_search.modal.title', 'settings.browserUse.profileImport.title', 'settings.browserUse.profileImport.extensionsConfirmationTitle'])
            allowed['linux'] = ['MoonshineNuxV2Modal.title', 'appgenSettings.accessRequest.loadError', 'appgenSettings.customDomains.addDialog.title', 'appgenSettings.general.url.changeDialog.title', 'appgenSettings.transfer.dialogTitle', 'apps.connectMfa.title', 'artifactFeedback.title', 'assistantMessage.autoReviewStats.title', 'assistantMessage.hookStats.dialogTitle', 'automations.sharing.preview.dialog.title', 'browser.adminConsoleOAuthRedirect.status', 'browserProfileImport.nux.modal.title', 'browserSkills.duplicateTitle', 'browserSkills.uploadTitle', 'businessProfiles.feedProducts.title', 'businessProfiles.field.conflict.title', 'businessProfiles.leave.title', 'businessProfiles.manual.productTitle', 'chatGpt.quietHours.active.title', 'chatgpt.ads.onboarding.dialogTitle', 'chatgpt.contentReferences.learningBlock.dialog.title', 'chatgpt.global_search.modal.title', 'chatgpt.new-onboarding.all-done-2', 'chatgpt.onboarding.new_workspace.codex_screen.chatgpt_desktop_app.title', 'chatgpt.pepper.personalization.accent_color.title', 'chatgpt.pepper.personalization.voice.title', 'chatgpt.pythonExecution.analysisTitle', 'chatgpt.tasks.run_history.title', 'chatgpt.tpp.onboarding.plugins_nux.browse.title', 'chatgpt.tpp.onboarding.website_permission.title', 'chatgpt.versions.title', 'chatgptConversations.businessAgent.disclosureTitle', 'chatgptConversations.debugPanel.messageJsonTitle', 'chatgptConversations.dil.geolocationPermission.title', 'chatgptConversations.gpts.about.loadError', 'chatgptConversations.gpts.about.loading', 'chatgptConversations.gpts.mine.deleteTitle', 'chatgptConversations.imageUploadReminder.title', 'chatgptConversations.paragen.dialogTitle', 'chatgptConversations.preciseLocation.permissionDenied.title', 'chatgptConversations.projectHome.textSource.title', 'chatgptConversations.projectSettings.title', 'chatgptConversations.summaryPanel.sources.memory.delete.title', 'chatgptConversations.toolApproval.detailsTitle', 'chatgptProjectHome.website.title', 'choose_your_voice', 'cloudEnvironments.createLink.unavailableTitle', 'code.diffComment.dismissConfirmation.title', 'codex.applyResultsDialog.title', 'codex.lunaReserve.dialog.title', 'codex.mcpTool.confirmFollowUp.widgetStateTitle', 'codex.page.taskMention.deleteTitle', 'codex.page.versions.restore.title', 'codex.remoteConnectionEditor.title', 'codex.review.revertDialog.title', 'codex.space.creation.discardTitle', 'codex.space.page.linkDialog.title', 'codex.space.page.recovery.discardTitle', 'codex.space.page.recovery.title', 'codex.space.sitePage.attach.title', 'codex.subscriptionPaymentRecovery.loadError.title', 'codex.visualization.externalLinkConfirmation.title', 'codex.writingBlock.confirmCloseTitle', 'codex.writingBlock.slides.templates.title', 'codexMobile.setupPage.waiting.pairing.fullscreenQrCodeTitle', 'composer.annotationEditor.title', 'composer.hooks.review.title', 'composer.memoriesSlashCommand.dialogTitle', 'composer.mode.agentMode.ultraFullAccessConfirm.title', 'composer.threadGoal.editDialog.title', 'composer.threadGoal.replaceConfirmation.title', 'composer.worktreePrompt.education.title', 'defenseFactory.editors.title', 'electron.onboarding.conversationalOnboarding.skipDialog.title', 'environmentSetup.vpn.dialogTitle', 'feedbackFormDialog.title', 'imagePanorama.title', 'imagePreviewDialog.label', 'inbox.automations.habitatMigration.title', 'inbox.automations.scheduleRule.title', 'keyboardShortcutsDialog.title', 'latex.images.title', 'library.emptyTrash.loadError.title', 'library.emptyTrash.title', 'libraryNext.flashcards.dialogTitle', 'libraryNext.move.title', 'lighthouseHome.keyboard.title', 'lighthouseHome.preview.title', 'localConversation.forkFromOlderTurnDialog.title', 'localConversationPage.createPrModal.gitLabTitle', 'localConversationPage.createPrModal.title', 'message.trustedContactNoThanksModal.title', 'multiplayer.debug.title', 'orbit.landing.loadError', 'orbit.primary.reboot.title', 'orbit.safety.confirmation.title', 'personalFinance.dashboard.settings.accessibleTitle', 'personalFinance.memories.accessibleTitle', 'personalVault.importTitle', 'personalWorkspaceAccessBlocked.title', 'plugins.create.title', 'postSharingModal.contentUnavailable', 'pricing.comparison.title', 'profile.shareCard.preview.title', 'profile.sharing.title', 'projectSetup.remoteSource.choose', 'projectSetupDialog.createMissingRemoteFolderTitle', 'pullRequestDetail.merge.title', 'pullRequestSubmitReview.title', 'restricted.aeon.email.dialog.loadingTitle', 'restricted.aeon.slack.setup.title', 'restricted.aeonMessaging.contactSetupTitle', 'restricted.aeonMessaging.email.details', 'restricted.environmentSetup.create.title', 'security.findingClose.title', 'security.scanDetail.context.dialogTitle', 'settings.ads.deleteDataTitle', 'settings.browserUse.profileImport.extensionsConfirmationTitle', 'settings.browserUse.profileImport.title', 'settings.chatGpt.cloudBrowser.addDialog.title', 'settings.chatGpt.cloudBrowser.credentials.choose', 'settings.chatGpt.personalization.memories.experience.confirmRevert.title', 'settings.chatGpt.personalization.memories.title', 'settings.chatGpt.security.linkedApps.connectedApps.manageTitle', 'settings.chatGpt.security.orderYubikey.title', 'settings.chatGpt.security.passkeys.renameTitle', 'settings.chatGpt.sharedLinks.deleteAllConfirmTitle', 'settings.chatGpt.sharedLinks.title', 'settings.cloudEnvironments.editor.repository.dialogTitle', 'settings.cloudEnvironments.editor.repository.loadFailed', 'settings.cloudEnvironments.editor.repository.loading', 'settings.cloudEnvironments.packages.title', 'settings.cloudEnvironments.resetCache.title', 'settings.codexMicro.analog.title', 'settings.codexMicro.encoder.title', 'settings.consumerBilling.downgradeTitle', 'settings.consumerBilling.renewalTitle', 'settings.general.realtimeVoice.dialog.title', 'settings.localEnvironments.createDialog.discardTitle', 'settings.memory.resetDialogTitle.host', 'settings.pets.preview.customize.title', 'settings.remoteConnections.details.allowSignedInDevicesDialog.title', 'settings.remoteConnections.manualPairingDialog.title', 'settings.teams.connectionRequest.title', 'settings.unsavedChanges.discardTitle', 'settings.usage.pricingPlanPage.subscriptionUpdate.title', 'settings.userRules.deleteTitle', 'settings.worktrees.autoCleanup.confirm.title', 'sidebarElectron.deleteThreadDialog.title', 'sites.preview.discardAnnotations.title', 'sitesPreview.handoff.title', 'sitesPreview.share.title', 'skills.appsPage.addMarketplace.title', 'skills.appsPage.pluginRequest.title', 'speechSettings.content.customDictionary.page.title', 'teams.spaces.setup.loading', 'thread.browser.tweaks.cancelConfirmTitle', 'voice-navigation-blocker.title', 'voiceFloatingOrbSettingsModal.title', 'work.onboarding.role.new.question', 'workspaceAgents.builder.apps.dialogTitle', 'workspaceAgents.builder.history.title', 'workspaceAgents.builder.skillPickerTitle', 'workspaceAgents.management.deleteTitle', 'workspaceAgents.schedules.editor.title', 'workspaceAgents.sharing.removeTitle', 'workspaceDiscovery.loadFailed', 'workspaceOnboarding.dialogTitle']
            allowed['macos'] = ['MoonshineNuxV2Modal.title', 'NoAuthPromoRedemptionModal.title', 'appUpdate.installProgress.title', 'appgenPublicationTerms.modal.title.v20260612', 'appgenSettings.accessRequest.loadError', 'appgenSettings.customDomains.addDialog.title', 'appgenSettings.general.url.changeDialog.title', 'appgenSettings.transfer.dialogTitle', 'apps.connectMfa.title', 'artifactFeedback.title', 'artifactViewer.present.slideshow', 'assistantMessage.autoReviewStats.title', 'assistantMessage.hookStats.dialogTitle', 'automations.sharing.preview.dialog.title', 'beacons.connectApp.loadError.title', 'beacons.connectApp.loading.title', 'browser.adminConsoleOAuthRedirect.status', 'browserProfileImport.nux.modal.title', 'browserSkills.duplicateTitle', 'browserSkills.uploadTitle', 'businessProfiles.feedProducts.title', 'businessProfiles.field.conflict.title', 'businessProfiles.leave.title', 'businessProfiles.manual.productTitle', 'chatGpt.quietHours.active.title', 'chatgpt.ads.onboarding.dialogTitle', 'chatgpt.contentReferences.learningBlock.dialog.title', 'chatgpt.contentReferences.map.title', 'chatgpt.global_search.modal.title', 'chatgpt.new-onboarding.all-done-2', 'chatgpt.onboarding.new_workspace.codex_screen.chatgpt_desktop_app.title', 'chatgpt.pepper.personalization.accent_color.title', 'chatgpt.pepper.personalization.voice.title', 'chatgpt.pythonExecution.analysisTitle', 'chatgpt.tasks.run_history.title', 'chatgpt.tpp.onboarding.plugins_nux.browse.title', 'chatgpt.tpp.onboarding.website_permission.title', 'chatgpt.versions.title', 'chatgptConversations.businessAgent.disclosureTitle', 'chatgptConversations.debugPanel.messageJsonTitle', 'chatgptConversations.dil.geolocationPermission.title', 'chatgptConversations.gpts.about.loadError', 'chatgptConversations.gpts.about.loading', 'chatgptConversations.gpts.mine.deleteTitle', 'chatgptConversations.imageUploadReminder.title', 'chatgptConversations.paragen.dialogTitle', 'chatgptConversations.preciseLocation.permissionDenied.title', 'chatgptConversations.projectHome.textSource.title', 'chatgptConversations.projectSettings.title', 'chatgptConversations.sidebar.project.delete.dialog.title.crossMode', 'chatgptConversations.summaryPanel.sources.memory.delete.title', 'chatgptConversations.toolApproval.detailsTitle', 'chatgptProjectHome.website.title', 'choose_your_voice', 'cloudEnvironments.createLink.unavailableTitle', 'code.diffComment.dismissConfirmation.title', 'codex.applyResultsDialog.title', 'codex.lunaReserve.dialog.title', 'codex.mcpTool.confirmFollowUp.widgetStateTitle', 'codex.page.taskMention.deleteTitle', 'codex.page.versions.restore.title', 'codex.remoteConnectionEditor.loadError.title', 'codex.remoteConnectionEditor.title', 'codex.review.revertDialog.title', 'codex.space.creation.discardTitle', 'codex.space.page.linkDialog.title', 'codex.space.page.move.title', 'codex.space.page.recovery.discardTitle', 'codex.space.page.recovery.title', 'codex.space.sitePage.attach.title', 'codex.subscriptionPaymentRecovery.loadError.title', 'codex.visualization.externalLinkConfirmation.title', 'codex.writingBlock.confirmCloseTitle', 'codex.writingBlock.slides.templates.title', 'codexMobile.setupPage.waiting.pairing.fullscreenQrCodeTitle', 'composer.annotationEditor.title', 'composer.appshotCapture.firstUse.title', 'composer.hooks.review.title', 'composer.memoriesSlashCommand.dialogTitle', 'composer.mode.agentMode.ultraFullAccessConfirm.title', 'composer.threadGoal.editDialog.title', 'composer.threadGoal.replaceConfirmation.title', 'composer.worktreePrompt.education.title', 'defenseFactory.editors.title', 'electron.onboarding.conversationalOnboarding.skipDialog.title', 'environmentSetup.vpn.dialogTitle', 'feedback.dialog.loadError.title', 'feedbackFormDialog.title', 'imagePanorama.title', 'imagePreviewDialog.label', 'inbox.automations.habitatMigration.title', 'inbox.automations.scheduleRule.title', 'keyboardShortcutsDialog.title', 'latex.images.title', 'library.emptyTrash.loadError.title', 'library.emptyTrash.title', 'libraryNext.connectPlugin.title', 'libraryNext.createFolder.title', 'libraryNext.flashcards.dialogTitle', 'libraryNext.move.title', 'lighthouseHome.keyboard.title', 'lighthouseHome.preview.title', 'localConversation.forkFromOlderTurnDialog.title', 'localConversation.sideChat.closeConfirmation.title', 'localConversationPage.createPrModal.gitLabTitle', 'localConversationPage.createPrModal.title', 'message.trustedContactNoThanksModal.title', 'multiplayer.debug.title', 'notifications.clearAllUnreads.title', 'orbit.landing.loadError', 'orbit.primary.reboot.title', 'orbit.safety.confirmation.title', 'personalFinance.dashboard.settings.accessibleTitle', 'personalFinance.memories.accessibleTitle', 'personalVault.importTitle', 'personalWorkspaceAccessBlocked.title', 'plugins.create.title', 'postSharingModal.contentUnavailable', 'pricing.comparison.title', 'profile.shareCard.preview.title', 'profile.sharing.title', 'projectSetup.editRemoteProject.title', 'projectSetup.remoteSource.choose', 'projectSetupDialog.createMissingRemoteFolderTitle', 'pullRequestDetail.merge.title', 'realtimeVoice.onboarding.title', 'restricted.aeon.email.dialog.loadingTitle', 'restricted.aeon.slack.setup.title', 'restricted.aeonMessaging.contactSetupTitle', 'restricted.aeonMessaging.email.details', 'restricted.environmentSetup.create.title', 'security.findingClose.title', 'security.scanDetail.context.dialogTitle', 'settings.ads.deleteDataTitle', 'settings.browserUse.profileImport.extensionsConfirmationTitle', 'settings.browserUse.profileImport.title', 'settings.chatGpt.cloudBrowser.addDialog.title', 'settings.chatGpt.cloudBrowser.credentials.choose', 'settings.chatGpt.personalization.memories.experience.confirmRevert.title', 'settings.chatGpt.personalization.memories.title', 'settings.chatGpt.security.linkedApps.connectedApps.manageTitle', 'settings.chatGpt.security.orderYubikey.title', 'settings.chatGpt.security.passkeys.renameTitle', 'settings.chatGpt.sharedLinks.deleteAllConfirmTitle', 'settings.chatGpt.sharedLinks.title', 'settings.cloudEnvironments.editor.repository.dialogTitle', 'settings.cloudEnvironments.editor.repository.loadFailed', 'settings.cloudEnvironments.editor.repository.loading', 'settings.cloudEnvironments.packages.title', 'settings.cloudEnvironments.resetCache.title', 'settings.codexMicro.analog.title', 'settings.codexMicro.encoder.title', 'settings.consumerBilling.downgradeTitle', 'settings.consumerBilling.renewalTitle', 'settings.general.realtimeVoice.dialog.title', 'settings.localEnvironments.createDialog.discardTitle', 'settings.memory.resetDialogTitle.host', 'settings.pets.preview.customize.title', 'settings.remoteConnections.details.allowSignedInDevicesDialog.title', 'settings.remoteConnections.manualPairingDialog.title', 'settings.teams.connectionRequest.title', 'settings.unsavedChanges.discardTitle', 'settings.usage.pricingPlanPage.subscriptionUpdate.title', 'settings.worktrees.autoCleanup.confirm.title', 'sidebarCustomization.customizeSidebar', 'sidebarElectron.deleteThreadDialog.title', 'sites.preview.discardAnnotations.title', 'sitesPreview.handoff.title', 'sitesPreview.share.title', 'skills.appsPage.addMarketplace.title', 'skills.appsPage.pluginRequest.title', 'speechSettings.content.customDictionary.page.title', 'teams.spaces.setup.loading', 'thread.browser.tweaks.cancelConfirmTitle', 'traceRecording.details.title', 'traceRecording.startDetails.title', 'voice-navigation-blocker.title', 'voiceFloatingOrbSettingsModal.title', 'work.onboarding.role.new.question', 'workspaceAgents.builder.apps.dialogTitle', 'workspaceAgents.builder.history.title', 'workspaceAgents.builder.skillPickerTitle', 'workspaceAgents.management.deleteTitle', 'workspaceAgents.schedules.editor.title', 'workspaceAgents.sharing.removeTitle', 'workspaceDiscovery.loadFailed', 'workspaceOnboarding.dialogTitle']
            allowed['linux'] = ['GizmoInformation.audioSummary', 'MoonshineNuxV2Modal.title', 'appgenSettings.accessRequest.loadError', 'appgenSettings.customDomains.addDialog.title', 'appgenSettings.general.url.changeDialog.title', 'appgenSettings.transfer.dialogTitle', 'apps.connectMfa.title', 'artifactFeedback.title', 'assistantMessage.autoReviewStats.title', 'assistantMessage.hookStats.dialogTitle', 'automations.sharing.preview.dialog.title', 'browser.adminConsoleOAuthRedirect.status', 'browserProfileImport.nux.modal.title', 'browserSkills.duplicateTitle', 'browserSkills.uploadTitle', 'businessProfiles.editor.preview', 'businessProfiles.feedProducts.title', 'businessProfiles.field.conflict.title', 'businessProfiles.leave.title', 'businessProfiles.manual.productTitle', 'chatGpt.quietHours.active.title', 'chatgpt.ads.onboarding.dialogTitle', 'chatgpt.contentReferences.learningBlock.dialog.title', 'chatgpt.delinquent_upgrade.recovery.modal.title.payment', 'chatgpt.global_search.modal.title', 'chatgpt.new-onboarding.all-done-2', 'chatgpt.onboarding.new_workspace.codex_screen.chatgpt_desktop_app.title', 'chatgpt.pepper.personalization.accent_color.title', 'chatgpt.pepper.personalization.voice.title', 'chatgpt.pythonExecution.analysisTitle', 'chatgpt.shopping.virtual_try_on.onboarding.camera.button.take_selfie', 'chatgpt.tasks.run_history.title', 'chatgpt.tpp.onboarding.plugins_nux.browse.title', 'chatgpt.tpp.onboarding.website_permission.title', 'chatgpt.versions.title', 'chatgptConversations.businessAgent.disclosureTitle', 'chatgptConversations.debugPanel.messageJsonTitle', 'chatgptConversations.dil.geolocationPermission.title', 'chatgptConversations.gpts.about.loadError', 'chatgptConversations.gpts.about.loading', 'chatgptConversations.gpts.mine.deleteTitle', 'chatgptConversations.imageUploadReminder.title', 'chatgptConversations.paragen.dialogTitle', 'chatgptConversations.preciseLocation.permissionDenied.title', 'chatgptConversations.projectHome.textSource.title', 'chatgptConversations.projectSettings.title', 'chatgptConversations.sidebar.project.createTitle', 'chatgptConversations.summaryPanel.sources.memory.delete.title', 'chatgptConversations.temporaryChat.onboarding.title', 'chatgptConversations.toolApproval.detailsTitle', 'chatgptProjectHome.website.title', 'choose_your_voice', 'cloudEnvironments.createLink.unavailableTitle', 'code.diffComment.dismissConfirmation.title', 'codex.applyResultsDialog.title', 'codex.lunaReserve.dialog.title', 'codex.mcpTool.confirmFollowUp.widgetStateTitle', 'codex.page.taskMention.deleteTitle', 'codex.page.versions.restore.title', 'codex.remoteConnectionEditor.title', 'codex.review.revertDialog.title', 'codex.space.creation.discardTitle', 'codex.space.page.linkDialog.title', 'codex.space.page.recovery.discardTitle', 'codex.space.page.recovery.title', 'codex.space.sitePage.attach.title', 'codex.subscriptionPaymentRecovery.loadError.title', 'codex.visualization.externalLinkConfirmation.title', 'codex.writingBlock.confirmCloseTitle', 'codex.writingBlock.slides.templates.title', 'codexMobile.setupPage.waiting.pairing.fullscreenQrCodeTitle', 'composer.annotationEditor.title', 'composer.hooks.review.title', 'composer.memoriesSlashCommand.dialogTitle', 'composer.mode.agentMode.ultraFullAccessConfirm.title', 'composer.threadGoal.editDialog.title', 'composer.threadGoal.replaceConfirmation.title', 'composer.worktreePrompt.education.title', 'defenseFactory.editors.title', 'electron.onboarding.conversationalOnboarding.skipDialog.title', 'environmentSetup.vpn.dialogTitle', 'feedbackFormDialog.title', 'health.onboarding.dialogLabel', 'imagePanorama.title', 'imagePreviewDialog.label', 'inbox.automations.habitatMigration.title', 'inbox.automations.scheduleRule.title', 'keyboardShortcutsDialog.title', 'latex.images.title', 'library.emptyTrash.loadError.title', 'library.emptyTrash.title', 'libraryNext.flashcards.dialogTitle', 'libraryNext.move.title', 'lighthouseHome.keyboard.title', 'lighthouseHome.preview.title', 'localConversation.forkFromOlderTurnDialog.title', 'localConversationPage.createPrModal.gitLabTitle', 'localConversationPage.createPrModal.title', 'message.trustedContactNoThanksModal.title', 'multiplayer.debug.title', 'navigation.survey.valueSurvey.title', 'orbit.landing.loadError', 'orbit.primary.reboot.title', 'orbit.safety.confirmation.title', 'personalFinance.dashboard.settings.accessibleTitle', 'personalFinance.memories.accessibleTitle', 'personalVault.importTitle', 'personalWorkspaceAccessBlocked.title', 'plugins.create.title', 'plugins.detail.accounts.reconnectFirst', 'plugins.management.delete', 'plugins.management.upload', 'postSharingModal.contentUnavailable', 'pricing.comparison.title', 'profile.photoCrop.title', 'profile.shareCard.preview.title', 'profile.sharing.title', 'projectSetup.remoteSource.choose', 'projectSetupDialog.createMissingRemoteFolderTitle', 'pullRequestDetail.merge.title', 'pullRequestSubmitReview.title', 'restricted.aeon.email.dialog.loadingTitle', 'restricted.aeon.slack.setup.title', 'restricted.aeonMessaging.contactSetupTitle', 'restricted.aeonMessaging.email.details', 'restricted.environmentSetup.configuration.network.domains', 'restricted.environmentSetup.create.title', 'security.findingClose.title', 'security.scanDetail.context.dialogTitle', 'settings.ads.deleteDataTitle', 'settings.browserUse.profileImport.extensionsConfirmationTitle', 'settings.browserUse.profileImport.title', 'settings.chatGpt.cloudBrowser.addDialog.title', 'settings.chatGpt.cloudBrowser.credentials.choose', 'settings.chatGpt.personalization.memories.experience.confirmRevert.title', 'settings.chatGpt.personalization.memories.title', 'settings.chatGpt.security.linkedApps.connectedApps.manageTitle', 'settings.chatGpt.security.orderYubikey.title', 'settings.chatGpt.security.passkeys.renameTitle', 'settings.chatGpt.sharedLinks.deleteAllConfirmTitle', 'settings.chatGpt.sharedLinks.title', 'settings.cloudEnvironments.editor.repository.dialogTitle', 'settings.cloudEnvironments.editor.repository.loadFailed', 'settings.cloudEnvironments.editor.repository.loading', 'settings.cloudEnvironments.packages.title', 'settings.cloudEnvironments.resetCache.title', 'settings.codexMicro.analog.title', 'settings.codexMicro.encoder.title', 'settings.consumerBilling.downgradeTitle', 'settings.consumerBilling.renewalTitle', 'settings.general.realtimeVoice.dialog.title', 'settings.import.autosync.content', 'settings.localEnvironments.createDialog.discardTitle', 'settings.memory.resetDialogTitle.host', 'settings.pets.preview.customize.title', 'settings.remoteConnections.details.allowSignedInDevicesDialog.title', 'settings.remoteConnections.manualPairingDialog.title', 'settings.teams.connectionRequest.title', 'settings.unsavedChanges.discardTitle', 'settings.usage.pricingPlanPage.subscriptionUpdate.title', 'settings.userRules.deleteTitle', 'settings.worktrees.autoCleanup.confirm.title', 'settingsModal.trustedContacts.infoModal.title.v2', 'settingsModal.trustedContacts.inviteModalTitle', 'settingsModal.trustedContacts.removeConfirmation.title', 'sidebarElectron.deleteThreadDialog.title', 'sites.preview.discardAnnotations.title', 'sitesPreview.handoff.title', 'sitesPreview.share.title', 'skills.appsPage.addMarketplace.title', 'skills.appsPage.pluginRequest.title', 'source.dialogTitle.processDetails', 'speechSettings.content.customDictionary.page.title', 'teams.spaces.setup.loading', 'thread.browser.tweaks.cancelConfirmTitle', 'voice-navigation-blocker.title', 'voiceFloatingOrbSettingsModal.title', 'work.onboarding.role.new.question', 'workspaceAgents.builder.apps.dialogTitle', 'workspaceAgents.builder.history.title', 'workspaceAgents.builder.skillPickerTitle', 'workspaceAgents.management.deleteTitle', 'workspaceAgents.schedules.editor.title', 'workspaceAgents.sharing.removeTitle', 'workspaceDiscovery.loadFailed', 'workspaceOnboarding.dialogTitle']
            allowed['windows'] = ['MoonshineNuxV2Modal.title', 'NoAuthPromoRedemptionModal.title', 'appUpdate.installProgress.title', 'appgenPublicationTerms.modal.title.v20260612', 'appgenSettings.accessRequest.loadError', 'appgenSettings.customDomains.addDialog.title', 'appgenSettings.general.url.changeDialog.title', 'appgenSettings.transfer.dialogTitle', 'apps.connectMfa.title', 'artifactFeedback.title', 'artifactViewer.present.slideshow', 'assistantMessage.autoReviewStats.title', 'assistantMessage.hookStats.dialogTitle', 'automations.sharing.preview.dialog.title', 'beacons.connectApp.loadError.title', 'beacons.connectApp.loading.title', 'browser.adminConsoleOAuthRedirect.status', 'browserProfileImport.nux.modal.title', 'browserSkills.duplicateTitle', 'browserSkills.uploadTitle', 'businessProfiles.feedProducts.title', 'businessProfiles.field.conflict.title', 'businessProfiles.leave.title', 'businessProfiles.manual.productTitle', 'chatGpt.quietHours.active.title', 'chatgpt.ads.onboarding.dialogTitle', 'chatgpt.contentReferences.learningBlock.dialog.title', 'chatgpt.contentReferences.map.title', 'chatgpt.global_search.modal.title', 'chatgpt.new-onboarding.all-done-2', 'chatgpt.onboarding.new_workspace.codex_screen.chatgpt_desktop_app.title', 'chatgpt.pepper.personalization.accent_color.title', 'chatgpt.pepper.personalization.voice.title', 'chatgpt.pythonExecution.analysisTitle', 'chatgpt.tasks.run_history.title', 'chatgpt.tpp.onboarding.plugins_nux.browse.title', 'chatgpt.tpp.onboarding.website_permission.title', 'chatgpt.versions.title', 'chatgptConversations.businessAgent.disclosureTitle', 'chatgptConversations.debugPanel.messageJsonTitle', 'chatgptConversations.dil.geolocationPermission.title', 'chatgptConversations.gpts.about.loadError', 'chatgptConversations.gpts.about.loading', 'chatgptConversations.gpts.mine.deleteTitle', 'chatgptConversations.imageUploadReminder.title', 'chatgptConversations.paragen.dialogTitle', 'chatgptConversations.preciseLocation.permissionDenied.title', 'chatgptConversations.projectHome.textSource.title', 'chatgptConversations.projectSettings.title', 'chatgptConversations.sidebar.project.delete.dialog.title.crossMode', 'chatgptConversations.summaryPanel.sources.memory.delete.title', 'chatgptConversations.toolApproval.detailsTitle', 'chatgptProjectHome.website.title', 'choose_your_voice', 'cloudEnvironments.createLink.unavailableTitle', 'code.diffComment.dismissConfirmation.title', 'codex.applyResultsDialog.title', 'codex.cme.dialog.cme', 'codex.cme.dialog.removeConversationFromClaim', 'codex.lunaReserve.dialog.title', 'codex.mcpTool.confirmFollowUp.widgetStateTitle', 'codex.page.taskMention.deleteTitle', 'codex.page.versions.restore.title', 'codex.remoteConnectionEditor.loadError.title', 'codex.remoteConnectionEditor.title', 'codex.review.revertDialog.title', 'codex.space.creation.discardTitle', 'codex.space.page.linkDialog.title', 'codex.space.page.move.title', 'codex.space.page.recovery.discardTitle', 'codex.space.page.recovery.title', 'codex.space.sitePage.attach.title', 'codex.subscriptionPaymentRecovery.loadError.title', 'codex.visualization.externalLinkConfirmation.title', 'codex.writingBlock.confirmCloseTitle', 'codex.writingBlock.slides.templates.title', 'codexMobile.setupPage.waiting.pairing.fullscreenQrCodeTitle', 'composer.annotationEditor.title', 'composer.appshotCapture.firstUse.title', 'composer.hooks.review.title', 'composer.memoriesSlashCommand.dialogTitle', 'composer.mode.agentMode.ultraFullAccessConfirm.title', 'composer.threadGoal.editDialog.title', 'composer.threadGoal.replaceConfirmation.title', 'composer.worktreePrompt.education.title', 'defenseFactory.editors.title', 'electron.onboarding.conversationalOnboarding.skipDialog.title', 'environmentSetup.vpn.dialogTitle', 'feedback.dialog.loadError.title', 'feedbackFormDialog.title', 'imagePanorama.title', 'imagePreviewDialog.label', 'inbox.automations.habitatMigration.title', 'inbox.automations.scheduleRule.title', 'keyboardShortcutsDialog.title', 'latex.images.title', 'library.emptyTrash.loadError.title', 'library.emptyTrash.title', 'libraryNext.connectPlugin.title', 'libraryNext.createFolder.title', 'libraryNext.flashcards.dialogTitle', 'libraryNext.move.title', 'lighthouseHome.keyboard.title', 'lighthouseHome.preview.title', 'localConversation.forkFromOlderTurnDialog.title', 'localConversation.sideChat.closeConfirmation.title', 'localConversationPage.createPrModal.gitLabTitle', 'localConversationPage.createPrModal.title', 'message.trustedContactNoThanksModal.title', 'multiplayer.debug.title', 'notifications.clearAllUnreads.title', 'orbit.landing.loadError', 'orbit.primary.reboot.title', 'orbit.safety.confirmation.title', 'personalFinance.dashboard.settings.accessibleTitle', 'personalFinance.memories.accessibleTitle', 'personalVault.importTitle', 'personalWorkspaceAccessBlocked.title', 'plugins.create.title', 'postSharingModal.contentUnavailable', 'pricing.comparison.title', 'profile.shareCard.preview.title', 'profile.sharing.title', 'projectSetup.editRemoteProject.title', 'projectSetup.remoteSource.choose', 'projectSetupDialog.createMissingRemoteFolderTitle', 'pullRequestDetail.comment.deleteTitle', 'pullRequestDetail.merge.title', 'pullRequestSubmitReview.title', 'realtimeVoice.onboarding.title', 'restricted.aeon.email.dialog.loadingTitle', 'restricted.aeon.slack.setup.title', 'restricted.aeonMessaging.contactSetupTitle', 'restricted.aeonMessaging.email.details', 'restricted.environmentSetup.create.title', 'security.findingClose.title', 'security.scanDetail.context.dialogTitle', 'settings.ads.deleteDataTitle', 'settings.browserUse.profileImport.extensionsConfirmationTitle', 'settings.browserUse.profileImport.title', 'settings.chatGpt.cloudBrowser.addDialog.title', 'settings.chatGpt.cloudBrowser.credentials.choose', 'settings.chatGpt.personalization.memories.experience.confirmRevert.title', 'settings.chatGpt.personalization.memories.title', 'settings.chatGpt.security.linkedApps.connectedApps.manageTitle', 'settings.chatGpt.security.orderYubikey.title', 'settings.chatGpt.security.passkeys.renameTitle', 'settings.chatGpt.sharedLinks.deleteAllConfirmTitle', 'settings.chatGpt.sharedLinks.title', 'settings.cloudEnvironments.editor.repository.dialogTitle', 'settings.cloudEnvironments.editor.repository.loadFailed', 'settings.cloudEnvironments.editor.repository.loading', 'settings.cloudEnvironments.packages.title', 'settings.cloudEnvironments.resetCache.title', 'settings.codexMicro.analog.title', 'settings.codexMicro.encoder.title', 'settings.consumerBilling.downgradeTitle', 'settings.consumerBilling.renewalTitle', 'settings.general.realtimeVoice.dialog.title', 'settings.localEnvironments.createDialog.discardTitle', 'settings.memory.resetDialogTitle.host', 'settings.pets.preview.customize.title', 'settings.remoteConnections.details.allowSignedInDevicesDialog.title', 'settings.remoteConnections.manualPairingDialog.title', 'settings.teams.connectionRequest.title', 'settings.unsavedChanges.discardTitle', 'settings.usage.pricingPlanPage.subscriptionUpdate.title', 'settings.userRules.deleteTitle', 'settings.worktrees.autoCleanup.confirm.title', 'sidebarCustomization.customizeSidebar', 'sidebarElectron.deleteThreadDialog.title', 'sites.preview.discardAnnotations.title', 'sitesPreview.handoff.title', 'sitesPreview.share.title', 'skills.appsPage.addMarketplace.title', 'skills.appsPage.pluginRequest.title', 'speechSettings.content.customDictionary.page.title', 'teams.spaces.setup.loading', 'thread.browser.tweaks.cancelConfirmTitle', 'traceRecording.details.title', 'traceRecording.startDetails.title', 'voice-navigation-blocker.title', 'voiceFloatingOrbSettingsModal.title', 'work.onboarding.role.new.question', 'workspaceAgents.builder.apps.dialogTitle', 'workspaceAgents.builder.history.title', 'workspaceAgents.builder.skillPickerTitle', 'workspaceAgents.management.deleteTitle', 'workspaceAgents.schedules.editor.title', 'workspaceAgents.sharing.removeTitle', 'workspaceDiscovery.loadFailed', 'workspaceOnboarding.dialogTitle']
            allowed['linux'] = ['GizmoInformation.audioSummary', 'MoonshineNuxV2Modal.title', 'appgenSettings.accessRequest.loadError', 'appgenSettings.customDomains.addDialog.title', 'appgenSettings.general.url.changeDialog.title', 'appgenSettings.transfer.dialogTitle', 'apps.connectMfa.title', 'artifactFeedback.title', 'assistantMessage.autoReviewStats.title', 'assistantMessage.hookStats.dialogTitle', 'automations.sharing.preview.dialog.title', 'browser.adminConsoleOAuthRedirect.status', 'browserProfileImport.nux.modal.title', 'browserSkills.duplicateTitle', 'browserSkills.uploadTitle', 'businessProfiles.editor.preview', 'businessProfiles.feedProducts.title', 'businessProfiles.field.conflict.title', 'businessProfiles.leave.title', 'businessProfiles.manual.productTitle', 'chatGpt.quietHours.active.title', 'chatgpt.ads.onboarding.dialogTitle', 'chatgpt.contentReferences.learningBlock.dialog.title', 'chatgpt.delinquent_upgrade.recovery.modal.title.payment', 'chatgpt.global_search.modal.title', 'chatgpt.new-onboarding.all-done-2', 'chatgpt.onboarding.new_workspace.codex_screen.chatgpt_desktop_app.title', 'chatgpt.pepper.personalization.accent_color.title', 'chatgpt.pepper.personalization.voice.title', 'chatgpt.promotion.credit_grant_redemption.modal.title.v2', 'chatgpt.pythonExecution.analysisTitle', 'chatgpt.shopping.virtual_try_on.onboarding.camera.button.take_selfie', 'chatgpt.tasks.run_history.title', 'chatgpt.tpp.onboarding.plugins_nux.browse.title', 'chatgpt.tpp.onboarding.website_permission.title', 'chatgpt.versions.title', 'chatgptConversations.businessAgent.disclosureTitle', 'chatgptConversations.debugPanel.messageJsonTitle', 'chatgptConversations.dil.geolocationPermission.title', 'chatgptConversations.gpts.about.loadError', 'chatgptConversations.gpts.about.loading', 'chatgptConversations.gpts.mine.deleteTitle', 'chatgptConversations.imageUploadReminder.title', 'chatgptConversations.paragen.dialogTitle', 'chatgptConversations.preciseLocation.permissionDenied.title', 'chatgptConversations.projectHome.textSource.title', 'chatgptConversations.projectSettings.title', 'chatgptConversations.sidebar.project.createTitle', 'chatgptConversations.summaryPanel.sources.memory.delete.title', 'chatgptConversations.temporaryChat.onboarding.title', 'chatgptConversations.toolApproval.detailsTitle', 'chatgptProjectHome.website.title', 'choose_your_voice', 'cloudEnvironments.createLink.unavailableTitle', 'code.diffComment.dismissConfirmation.title', 'codex.applyResultsDialog.title', 'codex.lunaReserve.dialog.title', 'codex.mcpTool.confirmFollowUp.widgetStateTitle', 'codex.page.taskMention.deleteTitle', 'codex.page.versions.restore.title', 'codex.remoteConnectionEditor.title', 'codex.review.revertDialog.title', 'codex.space.creation.discardTitle', 'codex.space.page.linkDialog.title', 'codex.space.page.recovery.discardTitle', 'codex.space.page.recovery.title', 'codex.space.sitePage.attach.title', 'codex.subscriptionPaymentRecovery.loadError.title', 'codex.visualization.externalLinkConfirmation.title', 'codex.writingBlock.confirmCloseTitle', 'codex.writingBlock.slides.templates.title', 'codexMobile.setupPage.waiting.pairing.fullscreenQrCodeTitle', 'composer.annotationEditor.title', 'composer.hooks.review.title', 'composer.memoriesSlashCommand.dialogTitle', 'composer.mode.agentMode.ultraFullAccessConfirm.title', 'composer.threadGoal.editDialog.title', 'composer.threadGoal.replaceConfirmation.title', 'composer.worktreePrompt.education.title', 'defenseFactory.editors.title', 'electron.onboarding.conversationalOnboarding.skipDialog.title', 'environmentSetup.vpn.dialogTitle', 'feedbackFormDialog.title', 'health.onboarding.dialogLabel', 'imagePanorama.title', 'imagePreviewDialog.label', 'inbox.automations.habitatMigration.title', 'inbox.automations.scheduleRule.title', 'keyboardShortcutsDialog.title', 'latex.images.title', 'library.emptyTrash.loadError.title', 'library.emptyTrash.title', 'libraryNext.flashcards.dialogTitle', 'libraryNext.move.title', 'lighthouseHome.keyboard.title', 'lighthouseHome.preview.title', 'localConversation.forkFromOlderTurnDialog.title', 'localConversationPage.createPrModal.gitLabTitle', 'localConversationPage.createPrModal.title', 'message.trustedContactNoThanksModal.title', 'multiplayer.debug.title', 'navigation.survey.valueSurvey.title', 'orbit.landing.loadError', 'orbit.primary.reboot.title', 'orbit.safety.confirmation.title', 'personalFinance.dashboard.settings.accessibleTitle', 'personalFinance.memories.accessibleTitle', 'personalVault.importTitle', 'personalWorkspaceAccessBlocked.title', 'plugins.create.title', 'plugins.detail.accounts.reconnectFirst', 'plugins.management.delete', 'plugins.management.upload', 'postSharingModal.contentUnavailable', 'pricing.comparison.title', 'profile.photoCrop.title', 'profile.shareCard.preview.title', 'profile.sharing.title', 'projectSetup.remoteSource.choose', 'projectSetupDialog.createMissingRemoteFolderTitle', 'pullRequestDetail.merge.title', 'pullRequestSubmitReview.title', 'restricted.aeon.email.dialog.loadingTitle', 'restricted.aeon.slack.setup.title', 'restricted.aeonMessaging.contactSetupTitle', 'restricted.aeonMessaging.email.details', 'restricted.environmentSetup.configuration.network.domains', 'restricted.environmentSetup.create.title', 'security.findingClose.title', 'security.scanDetail.context.dialogTitle', 'settings.ads.deleteDataTitle', 'settings.browserUse.profileImport.extensionsConfirmationTitle', 'settings.browserUse.profileImport.title', 'settings.chatGpt.cloudBrowser.addDialog.title', 'settings.chatGpt.cloudBrowser.credentials.choose', 'settings.chatGpt.personalization.memories.experience.confirmRevert.title', 'settings.chatGpt.personalization.memories.title', 'settings.chatGpt.security.linkedApps.connectedApps.manageTitle', 'settings.chatGpt.security.orderYubikey.title', 'settings.chatGpt.security.passkeys.renameTitle', 'settings.chatGpt.sharedLinks.deleteAllConfirmTitle', 'settings.chatGpt.sharedLinks.title', 'settings.cloudEnvironments.editor.repository.dialogTitle', 'settings.cloudEnvironments.editor.repository.loadFailed', 'settings.cloudEnvironments.editor.repository.loading', 'settings.cloudEnvironments.packages.title', 'settings.cloudEnvironments.resetCache.title', 'settings.codexMicro.analog.title', 'settings.codexMicro.encoder.title', 'settings.consumerBilling.downgradeTitle', 'settings.consumerBilling.renewalTitle', 'settings.general.realtimeVoice.dialog.title', 'settings.import.autosync.content', 'settings.localEnvironments.createDialog.discardTitle', 'settings.memory.resetDialogTitle.host', 'settings.pets.preview.customize.title', 'settings.remoteConnections.details.allowSignedInDevicesDialog.title', 'settings.remoteConnections.manualPairingDialog.title', 'settings.teams.connectionRequest.title', 'settings.unsavedChanges.discardTitle', 'settings.usage.pricingPlanPage.subscriptionUpdate.title', 'settings.userRules.deleteTitle', 'settings.worktrees.autoCleanup.confirm.title', 'settingsModal.trustedContacts.infoModal.title.v2', 'settingsModal.trustedContacts.inviteModalTitle', 'settingsModal.trustedContacts.removeConfirmation.title', 'sidebarElectron.deleteThreadDialog.title', 'sites.preview.discardAnnotations.title', 'sitesPreview.handoff.title', 'sitesPreview.share.title', 'skills.appsPage.addMarketplace.title', 'skills.appsPage.pluginRequest.title', 'source.dialogTitle.processDetails', 'speechSettings.content.customDictionary.page.title', 'teams.spaces.setup.loading', 'thread.browser.tweaks.cancelConfirmTitle', 'voice-navigation-blocker.title', 'voiceFloatingOrbSettingsModal.title', 'work.onboarding.role.new.question', 'workspaceAgents.builder.apps.dialogTitle', 'workspaceAgents.builder.history.title', 'workspaceAgents.builder.skillPickerTitle', 'workspaceAgents.management.deleteTitle', 'workspaceAgents.schedules.editor.title', 'workspaceAgents.sharing.removeTitle', 'workspaceDiscovery.loadFailed', 'workspaceOnboarding.dialogTitle']
            for source_platform in allowed:
                allowed[source_platform].extend(('codex.commandMenu.title', 'desktop.windowCloseConfirmation.title', 'chatgptConversations.lockdown.dialog.title', 'appHeader.installUpdate.confirmTitle'))
            allowed['linux'] += ['projectSetup.consent.title.one', 'projectSetup.consent.title.other', 'projectSetup.consent.untrustedTitle.one', 'projectSetup.consent.untrustedTitle.other']
            allowed['macos'] += ['projectSetup.consent.title.one', 'projectSetup.consent.title.other', 'projectSetup.consent.untrustedTitle.one', 'projectSetup.consent.untrustedTitle.other']
            allowed['windows'] += ['projectSetup.consent.title.one', 'projectSetup.consent.title.other', 'projectSetup.consent.untrustedTitle.one', 'projectSetup.consent.untrustedTitle.other']
            fields = set('schemaVersion mechanism diagnosticsOnly sourceVersion platform artifactSha256 wrapperSourceSha256 catalogSha256 status titleReferenceCount matchCount sourceTitleIds'.split())
            if 'sourceTitleEmpty' in value:
                fields.add('sourceTitleEmpty')
            if 'guardFailure' in value:
                fields.add('guardFailure')
            if 'commandMenuShape' in value:
                fields.add('commandMenuShape')
            if 'sourceShape' in value:
                fields.add('sourceShape')
            if 'rejectionStage' in value:
                fields.add('rejectionStage')
            platform = value.get('platform')
            if (app != 'chatgpt-desktop' or set(value) != fields or value['diagnosticsOnly'] is not True
                    or value['sourceVersion'] != ('26.930.41038' if platform != 'windows' else '26.930.31730') or type(platform) is not str or platform not in pins
                    or (value['artifactSha256'], value['wrapperSourceSha256']) != pins[platform]
                    or value['catalogSha256'] != ('1b5e63b9905a66ae8355ae581beb190f0f8175d742ecc5a37076f5791bab758e' if platform == 'windows' else 'b6566a8d50edd58ed59e29eb2c9ef9de10d72f6e650f3ee0ec0a50a927106ee0' if platform == 'linux' else '82df6ff119bf98beba8ffe1a593aca671119decdbb8f4f3d39e5378e39b02c48')
                    or type(value['status']) is not str or value['status'] not in {'matched', 'unknown', 'ambiguous', 'guard-rejected'}
                    or type(value['sourceTitleIds']) is not list or len(value['sourceTitleIds']) > (195 if platform == 'linux' else 195 if platform == 'macos' else 200)
                    or any(type(identity) is not str or identity not in allowed[platform] for identity in value['sourceTitleIds'])):
                raise ValueError('invalid passive Codex static dialog title')
            identities = value['sourceTitleIds']
            if identities != sorted(set(identities)):
                raise ValueError('ambiguous or duplicate Codex title identities')
            if value['status'] == 'guard-rejected':
                if identities or value['titleReferenceCount'] is not None or value['matchCount'] is not None:
                    raise ValueError('invalid unavailable Codex title')
            else:
                expected = 'matched' if len(identities) == 1 else 'ambiguous' if identities else 'unknown'
                if (type(value['titleReferenceCount']) is not int or value['titleReferenceCount'] != 1
                        or type(value['matchCount']) is not int or value['matchCount'] != len(identities)
                        or value['status'] != expected):
                    raise ValueError('inconsistent passive Codex title')
            if 'sourceTitleEmpty' in value:
                empty = value['sourceTitleEmpty']
                if ((value['status'] == 'guard-rejected' and empty is not None)
                        or (value['status'] != 'guard-rejected' and type(empty) is not bool)
                        or (empty is True and (value['status'] != 'unknown' or identities))):
                    raise ValueError('invalid passive Codex empty title')
            if 'rejectionStage' in value:
                stage = value['rejectionStage']
                if ((value['status'] == 'guard-rejected' and (type(stage) is not str or stage not in {
                        'unmeasured', 'scope', 'deadline', 'dialog-count', 'reference', 'title-count',
                        'title-tag', 'title-text', 'actionability', 'query', 'changed'}))
                        or (value['status'] != 'guard-rejected' and stage is not None)):
                    raise ValueError('invalid passive Codex catalog rejection')
            if 'guardFailure' in value:
                failure = value['guardFailure']
                if failure is not None and (value.get('rejectionStage') != 'scope'
                        or value['status'] != 'guard-rejected' or type(failure) is not str
                        or failure not in {'held-document', 'page-set', 'native-ownership',
                                           'retained-document', 'document-focus', 'catalog-limit'}):
                    raise ValueError('invalid passive Codex scope guard')
            if 'sourceShape' in value:
                shape = value['sourceShape']
                shape_keys = {'pageRoleLegend', 'dialogRoleLegend', 'pageRoleRadios', 'dialogRoleRadios',
                              'pageEngineering', 'dialogEngineering', 'dialogContinue', 'dialogGetStarted'}
                if shape is not None and (value['status'] == 'guard-rejected' or type(shape) is not dict
                        or set(shape) != shape_keys or any(type(count) is not int or not 0 <= count <= 4096 for count in shape.values())
                        or shape['dialogRoleLegend'] > shape['pageRoleLegend'] or shape['dialogRoleRadios'] > shape['pageRoleRadios']
                        or shape['dialogEngineering'] > shape['pageEngineering'] or shape['pageEngineering'] > shape['pageRoleRadios']
                        or shape['dialogEngineering'] > shape['dialogRoleRadios']):
                    raise ValueError('invalid passive Codex dialog source shape')
            if 'commandMenuShape' in value:
                shape = value['commandMenuShape']
                shape_keys = {'dialogMarkerCount', 'globalScopeCount', 'rootCount', 'inputCount', 'listCount'}
                if shape is not None and (value['status'] == 'guard-rejected' or type(shape) is not dict
                        or set(shape) != shape_keys or any(type(count) is not int or not 0 <= count <= 4096 for count in shape.values())
                        or shape['dialogMarkerCount'] > 1 or shape['globalScopeCount'] > 1):
                    raise ValueError('invalid passive Codex command menu shape')
            record.update({key: value[key] for key in fields - {'schemaVersion', 'mechanism'}})
        elif mechanism == 'renderer-inventory':
            fields = set('schemaVersion mechanism diagnosticsOnly app endpointOwned launcherOwned attached pageCount textareaCount editableCount sendCount retryCount newThreadCount loginCount dialogCount errorCategory'.split())
            if set(value) - {'observerStage', 'documentState', 'startupScreen', 'landingCounts', 'onboardingCounts', 'publicOnboarding', 'mainAuxCorrelation', 'codexSession', 'initialMainBinding', 'initialMainConfirmation', 'initialMainActivation', 'sourceScreen', 'managedSignIn', 'sourceDialog', 'nativeOwnershipFailure', 'nativeListenerShape', 'sourceDialogPhase', 'folderTrustObservation', 'mainGuardObservation'} != fields or value['app'] != app or value['diagnosticsOnly'] is not True:
                raise ValueError('invalid renderer inventory identity')
            if 'sourceDialogPhase' in value:
                if app != 'chatgpt-desktop' or type(value['sourceDialogPhase']) is not str:
                    raise ValueError('invalid source dialog progress')
                enum(record, value, 'sourceDialogPhase', {'guard-before', 'identity-before',
                     'hold-dialog', 'sample-first', 'sample-second', 'identity-after', 'guard-after', 'finished'})
            if 'mainGuardObservation' in value:
                guard = value['mainGuardObservation']
                numbers = {'elapsedMs', 'nativeProofCount', 'nativeProofMs', 'identityCount', 'identityMs'}
                if (app != 'chatgpt-desktop' or type(guard) is not dict
                        or set(guard) != numbers | {'phase'}
                        or type(guard['phase']) is not str or guard['phase'] not in {
                            'native-ownership', 'main-identity', 'auxiliary-identity', 'complete', 'rejected'}
                        or any(type(guard[key]) is not int or not 0 <= guard[key] <= (
                            4096 if key.endswith('Count') else 600000) for key in numbers)):
                    raise ValueError('invalid main guard progress')
                record['mainGuardObservation'] = guard
            if 'folderTrustObservation' in value:
                trust = value['folderTrustObservation']
                phases = {'initial', 'authority-before', 'guard', 'authority-after', 'sample-initial',
                          'sample-result', 'sample-held', 'dispatch', 'post-dispatch', 'finished'}
                if (app != 'chatgpt-desktop' or type(trust) is not dict
                        or type(trust.get('phase')) is not str or trust['phase'] not in phases):
                    raise ValueError('invalid folder trust progress')
                folder_trust_observation({key: item for key, item in trust.items() if key != 'phase'})
                record['folderTrustObservation'] = trust
            if 'observerStage' in value:
                if type(value['observerStage']) is not str:
                    raise ValueError('invalid renderer checkpoint phase')
                enum(record, value, 'observerStage', {'request', 'endpoint', 'attach', 'main-binding',
                     'source-dialog', 'folder-trust', 'role-binding', 'onboarding', 'final-inventory', 'complete'})
                if value['observerStage'] != 'complete' and value['errorCategory'] is None:
                    raise ValueError('incomplete renderer checkpoint cannot claim completion')
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
            if 'initialMainActivation' in value:
                activation = value['initialMainActivation']
                if (app != 'chatgpt-desktop' or type(activation) is not dict
                        or set(activation) - {'nativeBoundary','nativeInventoryFailure','nativeActivationFailure','nativePendingStack','nativePointObservation','nativeOwnedMove','focusSamples'} != {'phase', 'status', 'activationAttempted', 'guardFailure'}
                        or type(activation['phase']) is not str or activation['phase'] not in {
                            'pre-proof', 'pre-identity', 'activation', 'polling', 'final-proof'}
                        or type(activation['status']) is not str or activation['status'] not in {
                            'focused', 'rejected', 'deadline', 'query-failed'}
                        or type(activation['activationAttempted']) is not bool
                        or activation['guardFailure'] is not None and (
                            type(activation['guardFailure']) is not str or activation['guardFailure'] not in {
                                'deadline', 'native-ownership', 'page-set', 'main-identity', 'main-focus', 'main-scope',
                                'auxiliary-route', 'auxiliary-identity', 'auxiliary-focus', 'auxiliary-controls',
                                'query-failed', 'unmeasured'})
                        or activation['phase'] in {'pre-proof', 'pre-identity'} and activation['activationAttempted']
                        or activation['status'] == 'focused' and (
                            activation['phase'] != 'final-proof' or activation['guardFailure'] is not None)):
                    raise ValueError('invalid Codex initial main activation')
                if 'focusSamples' in activation:
                    samples=activation['focusSamples']
                    if (type(samples) is not dict or set(samples)!={'beforePrepare','afterPrepare','afterActivation'}
                            or any(type(v) is not str or v not in {'unmeasured','focused','unfocused'} for v in samples.values())
                            or samples['afterPrepare']!='unmeasured' and samples['beforePrepare']=='unmeasured'
                            or samples['afterActivation']!='unmeasured' and (not activation['activationAttempted'] or samples['afterPrepare']=='unmeasured')):
                        raise ValueError('invalid Codex focus phase samples')
                if 'nativePointObservation' in activation:
                    if (not activation['activationAttempted']
                            or activation['phase'] not in {'polling','final-proof'}):
                        raise ValueError('invalid Codex passive point phase')
                    codex_point_observation(activation['nativePointObservation'])
                if 'nativeOwnedMove' in activation:
                    if (activation.get('nativePointObservation', {}).get('reason') != 'point-occluded'
                            or not activation['activationAttempted']
                            or activation['phase'] not in {'polling', 'final-proof'}):
                        raise ValueError('invalid Codex owned move phase')
                    codex_owned_move(activation['nativeOwnedMove'])
                if 'nativeBoundary' in activation:
                    boundary = activation['nativeBoundary']
                    if (type(boundary) is not str or boundary not in {
                            'request','cg-inventory-before','ax-main-before','cg-inventory-after',
                            'ax-main-after','identity','trust'}
                            or activation['activationAttempted'] or activation['status'] == 'focused'
                            or activation['phase'] != 'pre-identity'):
                        raise ValueError('invalid Codex native preparation boundary')
                if 'nativeInventoryFailure' in activation:
                    inventory = activation['nativeInventoryFailure']
                    counts = {'candidateCount','executableRejectedCount','ancestryRejectedCount'}
                    if (activation.get('nativeBoundary') not in {'cg-inventory-before','cg-inventory-after'}
                            or type(inventory) is not dict or set(inventory) != counts | {'reason'}
                            or type(inventory['reason']) is not str or inventory['reason'] not in {
                                'inventory-unavailable','limit','metadata','geometry','process-identity','identity',
                                'candidates-missing','candidates-ambiguous','other-owned-normal','overlapping-ahead',
                                'off-display','deadline'}
                            or any(type(inventory[k]) is not int or not 0 <= inventory[k] <= 1024 for k in counts)
                            or sum(inventory[k] for k in counts) > 1024):
                        raise ValueError('invalid Codex native inventory failure')
                if 'nativePendingStack' in activation:
                    stack=activation['nativePendingStack']
                    counts={'normalOverlapCount','elevatedOverlapCount','lowerOverlapCount'}
                    if (type(stack) is not dict or set(stack)-{'elevatedLevels','workArea','occluderKinds','otherPublicExecutables'}!=counts|{'sample','displayContained'}
                            or type(stack['sample']) is not str or stack['sample'] not in {'before','after'}
                            or stack['displayContained'] is not True
                            or any(type(stack[key]) is not int or not 0<=stack[key]<=1024 for key in counts)
                            or not 1<=sum(stack[key] for key in counts)<=1024
                            or activation['activationAttempted'] is not True):
                        raise ValueError('invalid Codex pending native stack')
                    if 'elevatedLevels' in stack:
                        levels=stack['elevatedLevels']
                        keys={'menuLevelCount','statusLevelCount','dockLevelCount','otherLevelCount'}
                        if (type(levels) is not dict or set(levels)!=keys
                                or any(type(levels[key]) is not int or not 0<=levels[key]<=1024 for key in keys)
                                or sum(levels.values())!=stack['elevatedOverlapCount']):
                            raise ValueError('invalid Codex elevated window levels')
                    if 'occluderKinds' in stack:
                        kinds=stack['occluderKinds']
                        keys={'controlCenter','notificationCenter','systemUIServer','dock','windowServer',
                              'launcherOwned','checkerOwned','other','unobserved'}
                        if (type(kinds) is not dict or set(kinds)!=keys
                                or any(type(kinds[key]) is not int or not 0<=kinds[key]<=1024 for key in keys)
                                or sum(kinds.values())!=sum(stack[key] for key in counts)):
                            raise ValueError('invalid Codex occluder process categories')
                    if 'otherPublicExecutables' in stack:
                        public_other=stack['otherPublicExecutables']
                        keys={'coreServicesUIAgent','textInputMenuAgent','securityAgent'}
                        if ('occluderKinds' not in stack or type(public_other) is not dict
                                or set(public_other)!=keys
                                or any(type(public_other[key]) is not int or not 0<=public_other[key]<=1024 for key in keys)
                                or sum(public_other.values())>stack['occluderKinds']['other']):
                            raise ValueError('invalid Codex public executable subpartition')
                    if 'workArea' in stack:
                        area=stack['workArea']
                        if (type(area) is not dict or set(area)!={'measured','windowContained','overlapIntersectionCount'}
                                or type(area['measured']) is not bool
                                or area['measured'] and (type(area['windowContained']) is not bool
                                    or type(area['overlapIntersectionCount']) is not int
                                    or not 0<=area['overlapIntersectionCount']<=sum(stack[key] for key in counts)
                                    or area['windowContained'] and area['overlapIntersectionCount']!=sum(stack[key] for key in counts))
                                or not area['measured'] and (area['windowContained'] is not None
                                    or area['overlapIntersectionCount'] is not None)):
                            raise ValueError('invalid Codex work-area observation')
                if 'nativeActivationFailure' in activation:
                    failure = activation['nativeActivationFailure']
                    boundaries = {'request','cg-inventory-before','ax-main-before','cg-inventory-after',
                                  'ax-main-after','identity','trust','app-unavailable','app-unfocused',
                                  'foreground-unfocused','focused-window-query','focused-window-type',
                                  'focused-window-identity','app-activate','raise','deadline'}
                    if (type(failure) is not dict or set(failure) - {'inventory'} != {'phase','boundary'}
                            or type(failure['phase']) is not str or failure['phase'] not in {'activation','verification'}
                            or type(failure['boundary']) is not str or failure['boundary'] not in boundaries
                            or not activation['activationAttempted'] or activation['status'] not in {'query-failed','deadline'}
                            or activation['phase'] != ('activation' if failure['phase'] == 'activation' else 'polling')
                            or 'nativeBoundary' in activation or 'nativeInventoryFailure' in activation):
                        raise ValueError('invalid Codex native activation failure')
                    if 'inventory' in failure:
                        inventory = failure['inventory']
                        counts = {'candidateCount','executableRejectedCount','ancestryRejectedCount'}
                        if (failure['boundary'] not in {'cg-inventory-before','cg-inventory-after'}
                                or type(inventory) is not dict or set(inventory) != counts | {'reason'}
                                or type(inventory['reason']) is not str or inventory['reason'] not in {
                                    'inventory-unavailable','limit','metadata','geometry','process-identity','identity',
                                    'candidates-missing','candidates-ambiguous','other-owned-normal','overlapping-ahead',
                                    'off-display','deadline'}
                                or any(type(inventory[k]) is not int or not 0 <= inventory[k] <= 1024 for k in counts)
                                or sum(inventory[k] for k in counts) > 1024):
                            raise ValueError('invalid Codex activation inventory failure')
                record['initialMainActivation'] = activation
            if 'initialMainConfirmation' in value:
                confirmation = value['initialMainConfirmation']
                flags = {'identityUnchanged', 'mainScopeUnique', 'documentFocused'}
                count_keys = set('roleLegend roleRadios engineering dialog quickChatComposer editable'.split())
                if (app != 'chatgpt-desktop' or type(confirmation) is not dict
                        or set(confirmation) - {'inputChannel'} != flags | {'status', 'counts'}
                        or type(confirmation.get('inputChannel', 'native-focused')) is not str
                        or confirmation.get('inputChannel', 'native-focused') not in {'native-focused', 'cdp-dom'}
                        or any(confirmation[key] is not None and type(confirmation[key]) is not bool for key in flags)
                        or type(confirmation['status']) is not str or confirmation['status'] not in {
                            'unmeasured', 'initial-missing', 'deadline', 'ownership-lost', 'identity-changed',
                            'source-scope', 'document-unfocused', 'guard-rejected', 'confirmed', 'query-failed'}
                        or confirmation['counts'] is not None and (type(confirmation['counts']) is not dict
                            or set(confirmation['counts']) != count_keys
                            or any(type(count) is not int or not 0 <= count <= 4096 for count in confirmation['counts'].values()))
                        or confirmation['status'] == 'confirmed' and (not all(confirmation[key] is True for key in {'identityUnchanged', 'mainScopeUnique'})
                            or not (confirmation['documentFocused'] is True
                                            or confirmation.get('inputChannel') == 'cdp-dom' and confirmation['documentFocused'] is False)
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
                if (app != 'chatgpt-desktop' or type(session) is not dict or set(session) - {'inputChannel'} != flags | {'pageCount'}
                        or type(session.get('inputChannel', 'native-focused')) is not str
                        or session.get('inputChannel', 'native-focused') not in {'native-focused', 'cdp-dom'}
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
            if 'ownedReadFixtureToolCount' in value:
                count = value['ownedReadFixtureToolCount']
                if (app != 'claude-desktop' or (count is not None and
                        (type(count) is not int or not 0 <= count <= 4096
                         or count > value['toolCount']))):
                    raise ValueError('invalid owned read fixture offer count')
                record['ownedReadFixtureToolCount'] = count
            if 'ownedReadFixtureSelection' in value:
                status = value['ownedReadFixtureSelection']
                if (app != 'claude-desktop' or type(status) is not str or status not in
                        {'selected', 'missing', 'ambiguous', 'schema-mismatch', 'limit'}
                        or 'ownedReadFixtureToolCount' not in value
                        or value['readToolSelected'] != (status == 'selected')):
                    raise ValueError('invalid owned read fixture selection')
                record['ownedReadFixtureSelection'] = status
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
            enum(record, value, 'stage', {'tool', 'failure', 'recovery'})
            if 'providerGenerationCount' in value:
                count = value['providerGenerationCount']
                if type(count) is not int or not 0 <= count <= 100000:
                    raise ValueError('invalid provider generation count')
                record['providerGenerationCount'] = count
            for key in ('toolCompleted', 'toolRecordingBounded', 'toolVerified',
                        'fixtureResponseVerified', 'failureObserved'):
                flag(record, value, key)
            if 'toolResult' in value:
                result = value['toolResult']
                keys = {'selectedTool', 'resultPresent', 'resultCount', 'status',
                        'shape', 'toolErrorDetected', 'errorCategory'}
                if value['stage'] != 'tool' or type(result) is not dict or not keys <= set(result) <= keys | {'errorEnvelope', 'execResult', 'failureHint'}:
                    raise ValueError('invalid tool result observation')
                closed = {}
                enum(closed, result, 'selectedTool', {'read', 'read-file', 'read-files', 'exec-command', 'fixture-read'})
                enum(closed, result, 'status', {'complete', 'limit'})
                enum(closed, result, 'shape', {'absent', 'string', 'text-array', 'mixed', 'unsupported'})
                enum(closed, result, 'errorCategory', {'none', 'file-not-found', 'file-too-large',
                                                     'read-budget', 'directory', 'unknown'})
                if 'execResult' in result:
                    if result['selectedTool'] != 'exec-command' or not result['resultPresent'] or result['execResult'] is None:
                        raise ValueError('exec result requires owned exec tool result')
                    enum(closed, result, 'execResult', {'launch-failed', 'exited-zero',
                         'exited-nonzero', 'running', 'unknown', 'ambiguous'})
                if 'failureHint' in result:
                    if result['failureHint'] is None or not result['resultPresent'] or not (result['toolErrorDetected'] or result.get('execResult') in {'launch-failed', 'exited-nonzero', 'ambiguous'}):
                        raise ValueError('failure hint requires owned failed tool result')
                    enum(closed, result, 'failureHint', {'permission-denied', 'missing-file',
                         'invalid-path', 'sandbox', 'missing-command', 'unsupported', 'unknown', 'ambiguous'})
                if result['selectedTool'] == 'fixture-read' and app != 'claude-desktop':
                    raise ValueError('foreign owned fixture selection')
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
                if 'errorEnvelope' in result:
                    envelope = result['errorEnvelope']
                    if ((not result['toolErrorDetected'] and envelope is not None)
                            or (result['toolErrorDetected'] and (type(envelope) is not str
                                or envelope not in {'single-xml-read-wrapper', 'single-xml-other',
                                    'plain-read-wrapper', 'plain-other', 'multiple-or-incomplete-xml', 'mixed-fragments'}))
                            or (envelope in {'single-xml-read-wrapper', 'plain-read-wrapper'}
                                and result['selectedTool'] != 'read')):
                        raise ValueError('invalid tool error envelope')
                    closed['errorEnvelope'] = envelope
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
            if 'retryLogObservation' in value:
                log = value['retryLogObservation']
                counters = {'sessionFound', 'sessionMissing', 'turnStarted', 'turnCompleted', 'turnFailed', 'turnCancelled'}
                if type(log) is dict and 'priorTurnObserved' in log:
                    counters |= {'messageTotals'}
                    fields = counters | {'status', 'priorTurnObserved'}
                    if type(log['priorTurnObserved']) is not bool:
                        raise ValueError('invalid passive Zed baseline observation')
                else:
                    # Historical receipts used line numbers absent from native logs.
                    counters |= {'resumeMessages', 'ordinarySend'}
                    fields = counters | {'status'}
                if (app != 'zed-desktop' or type(log) is not dict or set(log) != fields
                        or type(log['status']) is not str or log['status'] not in {'complete', 'missing', 'rotated', 'truncated', 'unavailable', 'limit'}
                        or any(type(log[key]) is not int or not 0 <= log[key] <= 255 for key in counters)
                        or (log['status'] != 'complete' and any(log[key] for key in counters))):
                    raise ValueError('invalid passive Zed retry log observation')
                record['retryLogObservation'] = log.copy()

            for key in ('exportResumeCount', 'exportAgentCount', 'exportTotalAssistantTextCount', 'exportUserCount', 'exportAssistantTextCount', 'trustControlCount', 'panelControlCount', 'retryControlCount', 'retryControlCountAfterActivation', 'retryControlCountAfterReadback',
                        'retryErrorTitleCountBeforeActivation', 'retryErrorTitleCountAfterActivation', 'retryErrorTitleCountAfterReadback', 'retryTitleCount', 'retryCandidateCount', 'retryTooltipCount', 'retryLabelCount',
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
    instrumented = any(item['mechanism'] == 'zed-retry-entry-counts'
                       for item in result['semanticObservations'])
    foreground_restored = all(item['stage'] == 'completed' and item['prepared'] and item['restored']
                              for item in result['semanticObservations']
                              if item['mechanism'] == 'windows-foreground-session')
    accepted = (foreground_restored and not instrumented and result['backend'] != 'renderer-inventory' and policy_disclosed and len(probes) == 3 and result['appCleanup'] == 'passed' and result['globalCleanup'] == 'passed'
                and all(probe.get('status') == 'passed' and len(probe.get('steps', [])) == 5
                        and set(probe.get('steps', [])) == STEPS
                        and (probe.get('inputMode'), probe.get('responseVerification')) in semantic_pairs for probe in probes))
    result.update(outcome='passed' if accepted else 'blocked',
                  qualification='deterministic-full' if accepted else 'unqualified',
                  reason=None if accepted else 'instrumented-diagnostic' if instrumented else 'incomplete-acceptance')
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


def diagnose_pending(app, platform, architecture, source_sha, output, facts):
    """Retain closed preparation facts even when no checker report was created."""
    pending = envelope(app, platform, architecture, source_sha)
    if bounded_json(output, 65536) != pending:
        raise ValueError('pending evidence differs')
    observations = semantic_observations(facts, app) if facts and Path(facts).exists() else []
    pending['semanticObservations'] = [dict(schemaVersion=1,
        mechanism='qualification-runner-failure', diagnosticsOnly=True,
        errorCategory='report-absent'), *observations]
    return pending


REDUCTION_ERRORS = {
    'too many semantic observations': 'observation-budget',
    'too many private semantic metadata files': 'metadata-budget',
    'semantic observation identity differs': 'observation-identity',
    'runtime version is not closed': 'runtime-version',
    'prepared provenance differs': 'prepared-provenance',
    'checker report exceeds its bound': 'report-budget',
    'checker report failed trusted validation': 'report-validation',
    'report identity differs from the cell': 'report-identity',
    'observed application version differs from frozen version': 'application-version',
    'invalid Windows foreground session receipt': 'foreground-receipt',
    'pending evidence differs': 'pending-identity',
    'evidence exceeds its bound': 'evidence-budget',
}
REDUCTION_CATEGORIES = set(REDUCTION_ERRORS.values()) | {'invalid-evidence', 'io-failure', 'runtime-failure'}


def publish_reduction_failure(args, category):
    # Never replace a completed result or evidence from another commit. This
    # fallback reads filenames only for counts, never rejected record contents.
    try:
        if category not in REDUCTION_CATEGORIES:
            return
        pending = envelope(args.app, args.platform, args.architecture, args.source_sha)
        output = Path(args.output)
        if bounded_json(output, 65536) != pending:
            return
        ordinary = metadata = None
        capped = False
        if args.facts:
            facts = Path(args.facts)
            if not facts.is_symlink() and facts.is_dir():
                ordinary = metadata = 0
                for index, path in enumerate(facts.glob('*.json')):
                    if index == 1024:
                        capped = True
                        break
                    if path.name == 'native-diagnostics.json':
                        continue
                    if path.name.startswith(('connection-', 'startup-', 'closed-startup-')):
                        metadata += 1
                    else:
                        ordinary += 1
        report = Path(args.report) if args.report else output.parent / 'report.json'
        fact = dict(schemaVersion=1, mechanism='qualification-reduction-failure',
                    diagnosticsOnly=True, category=category, reportPresent=report.is_file() and not report.is_symlink(),
                    observationCount=ordinary, metadataCount=metadata, countsCapped=capped)
        pending['semanticObservations'] = [fact]
        output.write_text(json.dumps(pending, sort_keys=True) + '\n')
    except (OSError, ValueError, TypeError, KeyError):
        return  # Preserve the original rejection even if publication is unavailable.


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=('matrix', 'pending', 'diagnose-pending', 'reduce', 'aggregate'))
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
        result = (diagnose_pending(args.app, args.platform, args.architecture, args.source_sha,
                                  args.output, args.facts) if args.command == 'diagnose-pending' else
                  aggregate(args.directory, args.source_sha, args.exclude_app) if args.command == 'aggregate' else
                  envelope(args.app, args.platform, args.architecture, args.source_sha)
                  if args.command == 'pending' else reduce_report(**{key: getattr(args, key) for key in required if key != 'output'}, facts=args.facts))
        destination = Path(args.output)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(json.dumps(result, sort_keys=True) + '\n')
        destination.chmod(0o600)
        if args.command == 'aggregate' and result['qualification'] != 'deterministic-full':
            raise SystemExit('desktop qualification matrix remains incomplete')
    except (OSError, ValueError, TypeError, KeyError, RuntimeError) as error:
        category = REDUCTION_ERRORS.get(str(error), 'io-failure' if isinstance(error, OSError)
                                        else 'runtime-failure' if isinstance(error, RuntimeError) else 'invalid-evidence')
        if args.command in {'reduce', 'diagnose-pending'}:
            publish_reduction_failure(args, category)
        raise SystemExit('desktop qualification evidence rejected: ' + category) from None


if __name__ == '__main__':
    main()
