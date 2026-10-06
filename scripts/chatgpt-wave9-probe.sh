#!/usr/bin/env bash
# Temporary wave-9 contract for the ChatGPT Linux official-package probe.
#
# Ownership boundaries kept explicit on purpose:
#   run       preserves the canonical report of a failed probe and re-raises the
#             checker status, because a blocked launch is environment evidence.
#   evidence  derives closed numeric and status fields only from a report that the
#             source-built checker validated, so no unvalidated artifact is uploadable.
#   qualify   asserts the pass condition separately from validation and upload.
# The official DEB layout is not modified here: no helper creation, no privilege,
# no sandbox flags, no startup workaround, no system policy change.
set -euo pipefail

CHECKER="${CHECKER:-target/debug/nanh-desktop-check}"
SESSION="${SESSION_SCRIPT:-scripts/run-desktop-check-session.sh}"
APP="chatgpt-desktop"

fail() {
    printf 'wave9: %s\n' "$1" >&2
    exit 1
}

need() {
    local label=$1 value=$2
    [[ -n "$value" ]] || fail "$label is required"
}

# Reads one dotted field from the receipt without printing private paths.
receipt_field() {
    local receipt=$1 filter=$2
    jq -er "$filter" "$receipt" >/dev/null || fail "the receipt field $filter is missing"
}

cmd_prepare_assert() {
    local receipt=""
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --receipt)
                receipt=${2:-}
                shift 2
                ;;
            *) fail "prepare-assert: unknown argument $1" ;;
        esac
    done
    need --receipt "$receipt"
    [[ -s "$receipt" ]] || fail "the preparation receipt is empty"
    [[ "$(jq -er '.platform' "$receipt")" == linux ]] || fail 'prepared platform'
    [[ "$(jq -er '.architecture' "$receipt")" == x86_64 ]] || fail 'prepared architecture'
    [[ "$(jq -er '.apps | length' "$receipt")" == 1 ]] || fail 'prepared app count'
    [[ "$(jq -er '.apps[0].app' "$receipt")" == "$APP" ]] || fail 'prepared app identity'
    receipt_field "$receipt" '.apps[0].executable.path'
    receipt_field "$receipt" '.apps[0].executable.sha256'
    receipt_field "$receipt" '.checker.sha256'
    receipt_field "$receipt" '.nanh.sha256'
    # The official package ships one GUI executable at this in-run relative path.
    [[ "$(jq -er '.apps[0].executable.path' "$receipt")" == */usr/lib/chatgpt/ChatGPT ]] ||
        fail 'the prepared executable is not the package GUI executable'
    printf 'status=prepared app=%s version=%s\n' "$APP" "$(jq -r '.apps[0].appVersion // "unknown"' "$receipt")"
}

cmd_run() {
    local receipt="" report=""
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --receipt)
                receipt=${2:-}
                shift 2
                ;;
            --report)
                report=${2:-}
                shift 2
                ;;
            *) fail "run: unknown argument $1" ;;
        esac
    done
    need --receipt "$receipt"
    need --report "$report"
    [[ -s "$receipt" ]] || fail "the preparation receipt is missing"
    [[ -x "$CHECKER" ]] || fail "the source-built checker is not executable"
    [[ -f "$SESSION" ]] || fail "the graphical session script is missing"
    local status=0
    # One run owns the three deterministic probes; the report schema bounds them.
    bash "$SESSION" "$CHECKER" run --yes --non-interactive --ephemeral \
        --mode deterministic --app "$APP" --prepared "$receipt" --output "$report" || status=$?
    [[ -s "$report" ]] || fail "probe exit=$status recorded no canonical report to preserve"
    printf 'status=probed exit=%s probes=%s\n' "$status" \
        "$(jq -r '.results[0].deterministic | length' "$report")"
    return "$status"
}

cmd_evidence() {
    local report="" out=""
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --report)
                report=${2:-}
                shift 2
                ;;
            --out)
                out=${2:-}
                shift 2
                ;;
            *) fail "evidence: unknown argument $1" ;;
        esac
    done
    need --report "$report"
    need --out "$out"
    local digest=""
    # The validator's own status gates publication; a pipeline would mask it.
    if ! digest="$("$CHECKER" validate-report "$report" 2> /dev/null)"; then
        fail "the checker rejected the report; nothing is published from it"
    fi
    digest=${digest##*$'\n'}
    [[ "$digest" =~ ^[0-9a-f]{64}$ ]] || fail "validation returned no digest"
    local temporary="$out.tmp"
    # Closed shape first: one ChatGPT result carrying exactly three deterministic probes.
    if ! jq -e '
        (.results | length == 1) and
        (.results[0].app == "chatgpt-desktop") and
        (.results[0].deterministic | length == 3) and
        (all(.results[0].deterministic[]; (.status | type == "string") and
            (.durationMilliseconds | type == "number")))
    ' "$report" > /dev/null; then
        rm -f -- "$temporary"
        fail "the validated report does not match the closed evidence shape"
    fi
    if ! jq --arg digest "$digest" '{
        schemaVersion: 1,
        app: .results[0].app,
        appVersion: (.results[0].appVersion // null),
        runtimeVersion: (.results[0].runtimeVersion // null),
        checkerVersion: .checkerVersion,
        runId: .runId,
        startedAt: .startedAt,
        platform: .platform,
        architecture: .architecture,
        probes: [ .results[0].deterministic | to_entries[] | {
            index: (.key + 1),
            status: .value.status,
            reason: (.value.reason // null),
            guiStage: (.value.guiStage // null),
            inputMode: (.value.inputMode // null),
            steps: .value.steps,
            durationMilliseconds: .value.durationMilliseconds
        } ],
        cleanup: { app: .results[0].cleanup, report: .cleanup },
        reportSha256: $digest
    }' "$report" > "$temporary"; then
        rm -f -- "$temporary"
        fail "the validated report does not match the closed evidence shape"
    fi
    mv -f -- "$temporary" "$out"
    printf 'status=validated digest=%s\n' "$digest"
}

cmd_qualify() {
    local evidence=""
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --evidence)
                evidence=${2:-}
                shift 2
                ;;
            *) fail "qualify: unknown argument $1" ;;
        esac
    done
    need --evidence "$evidence"
    [[ -s "$evidence" ]] || fail "no validated evidence is available to qualify"
    if jq -e '
        .app == "chatgpt-desktop" and
        (.appVersion != null) and
        (.probes | length == 3) and
        (all(.probes[]; .status == "passed" and (.durationMilliseconds | type == "number"))) and
        (.cleanup.app == "passed") and (.cleanup.report == "passed")
    ' "$evidence" > /dev/null; then
        printf 'status=qualified probes=3\n'
    else
        printf 'status=not-qualified probes=%s first=%s reason=%s\n' \
            "$(jq -r '.probes | length // 0' "$evidence")" \
            "$(jq -r '.probes[0].status // "none"' "$evidence")" \
            "$(jq -r '.probes[0].reason // "none"' "$evidence")" >&2
        exit 1
    fi
}

usage() {
    printf '%s\n' \
        "usage: $0 prepare-assert --receipt PATH" \
        "   or: $0 run --receipt PATH --report PATH" \
        "   or: $0 evidence --report PATH --out PATH" \
        "   or: $0 qualify --evidence PATH" >&2
    exit 2
}

command_name=${1:-}
[[ -n "$command_name" ]] || usage
shift
case "$command_name" in
    prepare-assert) cmd_prepare_assert "$@" ;;
    run) cmd_run "$@" ;;
    evidence) cmd_evidence "$@" ;;
    qualify) cmd_qualify "$@" ;;
    *) usage ;;
esac
