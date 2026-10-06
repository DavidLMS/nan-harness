#!/usr/bin/env bash
# Synthetic, credential-free contracts for the temporary wave-9 probe wrapper.
# Nothing here launches ChatGPT, reads a real receipt, or needs privileges: the
# checker and the graphical session script are stand-ins that record what they
# were asked to do.
set -euo pipefail

script_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
probe_contract="$script_root/chatgpt-wave9-probe.sh"
session_contract="$script_root/run-desktop-check-session.sh"
test_root=/tmp
[[ "$(uname -s)" != Darwin ]] || test_root=/private/tmp
workspace=$(mktemp -d "$test_root/chatgpt-wave9-tests.XXXXXX")
trap 'rm -rf -- "$workspace"' EXIT

calls="$workspace/calls.log"
output="$workspace/output.txt"
: > "$calls"

fail() {
    printf 'failed: %s\n' "$1" >&2
    exit 1
}

expect_contains() {
    local name=$1 needle=$2
    grep -qF -- "$needle" "$output" ||
        fail "$name: output missing '$needle': $(tr '\n' ' ' < "$output")"
}

expect_absent() {
    local name=$1 needle=$2
    grep -qF -- "$needle" "$output" && fail "$name: unexpected '$needle'" || true
}

# check <expected-exit> <name> <contract arguments...>
check() {
    local expected=$1 name=$2
    shift 2
    local status=0
    CHECKER="$fake_checker" SESSION_SCRIPT="$fake_session" SESSION_CALL_LOG="$calls" \
        FAKE_RUN_STATUS="$FAKE_RUN_STATUS" FAKE_VALIDATE_STATUS="$FAKE_VALIDATE_STATUS" \
        FAKE_WRITE_REPORT="$FAKE_WRITE_REPORT" FAKE_REPORT_TEMPLATE="$FAKE_REPORT_TEMPLATE" \
        bash "$probe_contract" "$@" > "$output" 2>&1 || status=$?
    [[ "$status" == "$expected" ]] ||
        fail "$name: expected exit $expected, got $status: $(tr '\n' ' ' < "$output")"
}

make_checker() {
    local path=$workspace/fake-checker
    cat > "$path" <<'EOF'
#!/usr/bin/env bash
set -eu
case "${1:-}" in
    run)
        output="" previous=""
        for argument in "$@"; do
            [[ "$previous" == "--output" ]] && output=$argument
            previous=$argument
        done
        [[ -n "$output" ]] || exit 78
        printf 'run %s\n' "$*" >> "$SESSION_CALL_LOG"
        if [[ "$FAKE_WRITE_REPORT" == yes ]]; then
            cp -- "$FAKE_REPORT_TEMPLATE" "$output"
        fi
        exit "$FAKE_RUN_STATUS"
        ;;
    validate-report)
        printf 'validate %s\n' "$*" >> "$SESSION_CALL_LOG"
        if [[ "$FAKE_VALIDATE_STATUS" == 0 ]]; then
            printf 'synthetic checker note\n%s\n' "$FAKE_DIGEST"
            exit 0
        fi
        printf 'the report is not valid\n' >&2
        exit "$FAKE_VALIDATE_STATUS"
        ;;
    *)
        printf 'unexpected checker call: %s\n' "$*" >&2
        exit 70
        ;;
esac
EOF
    chmod 755 "$path"
    printf '%s' "$path"
}

# Stands in for scripts/run-desktop-check-session.sh, which owns the disposable
# X server. The contract must forward the checker command unchanged.
make_session() {
    local path=$workspace/fake-session
    cat > "$path" <<'EOF'
#!/usr/bin/env bash
set -eu
printf 'forwarded %s\n' "$*" >> "$SESSION_CALL_LOG"
exec "$@"
EOF
    chmod 755 "$path"
    printf '%s' "$path"
}

make_receipt() {
    local name=$1 executable=$2 path=$workspace/receipt-$1.json
    cat > "$path" <<EOF
{
  "schemaVersion": 1,
  "runId": "0123456789abcdef0123456789abcdef",
  "platform": "linux",
  "architecture": "x86_64",
  "checker": { "path": "$workspace/checker", "sha256": "$(printf 'c%.0s' $(seq 1 64))" },
  "nanh": { "path": "$workspace/nanh", "sha256": "$(printf 'd%.0s' $(seq 1 64))" },
  "nanhIdentity": { "version": "0.1.4" },
  "apps": [ {
    "app": "chatgpt-desktop",
    "executable": { "path": "$executable", "sha256": "$(printf 'e%.0s' $(seq 1 64))" },
    "appVersion": "26.903.61454",
    "runtimeVersion": null
  } ]
}
EOF
    printf '%s' "$path"
}

# Mirrors the published sanitized report shape: enumerated status plus closed
# numbers, exactly one app and three deterministic probes.
make_template() {
    local name=$1 probe_status=$2 probe_reason=$3 probes=${4:-3} path=$workspace/template-$1.json
    jq -n --arg status "$probe_status" --arg reason "$probe_reason" --argjson probes "$probes" '{
        schemaVersion: 2, checkerVersion: "0.1.4",
        runId: "22222222222222222222222222222222",
        startedAt: "2026-09-10T12:00:00Z",
        platform: "linux", architecture: "x86_64",
        nanHarness: { version: "0.1.4" },
        results: [ {
            app: "chatgpt-desktop", appVersion: "26.903.61454", runtimeVersion: null,
            deterministic: [ range(0; $probes) | {
                status: $status,
                reason: (if $reason == "none" then null else $reason end),
                steps: ["launched"],
                inputMode: "accessibility",
                guiStage: (if $status == "passed" then "composer-send" else "composer-input" end),
                durationMilliseconds: (1200 + .)
            } ],
            live: { status: "skipped", reason: "not-run", steps: [], durationMilliseconds: 0 },
            cleanup: $status
        } ],
        cleanup: $status
    }' > "$path"
    printf '%s' "$path"
}

[[ -f "$probe_contract" ]] || fail "the probe contract is missing"
[[ -f "$session_contract" ]] || fail "the reusable session script must stay in place"

fake_checker=$(make_checker)
fake_session=$(make_session)
FAKE_DIGEST=$(printf 'a%.0s' $(seq 1 64))
export FAKE_DIGEST
FAKE_RUN_STATUS=0
FAKE_VALIDATE_STATUS=0
FAKE_WRITE_REPORT=yes
FAKE_REPORT_TEMPLATE=$(make_template passed passed none)

gui_path="$workspace/run/install-chatgpt/application/usr/lib/chatgpt/ChatGPT"
receipt=$(make_receipt gui "$gui_path")

# The prepared executable is the package's one GUI binary, identified by digest.
check 0 'prepared GUI executable' prepare-assert --receipt "$receipt"
expect_contains 'prepared GUI executable' 'version=26.903.61454'

# The launcher alias is a shell wrapper, so it must not be accepted as the app.
launcher=$(make_receipt launcher "$workspace/run/install-chatgpt/application/usr/lib/chatgpt/codex-launcher")
check 1 'launcher alias' prepare-assert --receipt "$launcher"
expect_contains 'launcher alias' 'not the package GUI executable'

check 1 'absent receipt' prepare-assert --receipt "$workspace/none.json"

# A passing probe forwards through the session wrapper and keeps its report.
good_report="$workspace/good-report.json"
check 0 'passing probe' run --receipt "$receipt" --report "$good_report"
expect_contains 'passing probe' 'status=probed exit=0 probes=3'
grep -qF -- 'forwarded' "$calls" || fail 'the probe skipped the session wrapper'
grep -qF -- 'run --yes --non-interactive --ephemeral --mode deterministic' "$calls" ||
    fail 'the probe changed the deterministic checker command'
[[ -s "$good_report" ]] || fail 'a passing probe must keep its report'

# A blocked probe still preserves the canonical report and propagates nonzero.
FAKE_RUN_STATUS=1
failed_report="$workspace/failed-report.json"
check 1 'blocked probe' run --receipt "$receipt" --report "$failed_report"
expect_contains 'blocked probe' 'status=probed exit=1 probes=3'
[[ -s "$failed_report" ]] || fail 'a blocked probe must still preserve its report'
FAKE_RUN_STATUS=0

# A nonzero probe that produced no report is a closed failure, not an empty bundle.
FAKE_WRITE_REPORT=no
missing_report="$workspace/missing-report.json"
check 1 'probe without report' run --receipt "$receipt" --report "$missing_report"
expect_absent 'probe without report' 'status=probed'
[[ ! -e "$missing_report" ]] || fail 'the wrapper must not invent a report'
FAKE_WRITE_REPORT=yes

# Validation gates evidence: a rejected report yields no uploadable artifact.
evidence="$workspace/evidence.json"
FAKE_VALIDATE_STATUS=3
check 1 'unvalidated report' evidence --report "$good_report" --out "$evidence"
[[ ! -e "$evidence" && ! -e "$evidence.tmp" ]] || fail 'an unvalidated report produced evidence'
FAKE_VALIDATE_STATUS=0

check 0 'validated evidence' evidence --report "$good_report" --out "$evidence"
expect_contains 'validated evidence' 'status=validated digest='
[[ ! -e "$evidence.tmp" ]] || fail 'the temporary evidence file must be renamed away'

local_keys=$(jq -r 'keys_unsorted | join(",")' "$evidence")
expected_keys='schemaVersion,app,appVersion,runtimeVersion,checkerVersion,runId,startedAt,platform,architecture,probes,cleanup,reportSha256'
[[ "$local_keys" == "$expected_keys" ]] || fail "unexpected evidence keys: $local_keys"
probe_keys=$(jq -r '.probes[0] | keys_unsorted | join(",")' "$evidence")
[[ "$probe_keys" == "index,status,reason,guiStage,inputMode,steps,durationMilliseconds" ]] ||
    fail "unexpected probe keys: $probe_keys"
[[ "$(jq -r '.probes | length' "$evidence")" == 3 ]] || fail 'three probes must be listed'
[[ "$(jq -r '[.probes[].index] | join(",")' "$evidence")" == "1,2,3" ]] || fail 'probe numbering'
[[ "$(jq -r '[.probes[].durationMilliseconds | type] | unique | join(",")' "$evidence")" == "number" ]] ||
    fail 'durations must stay numeric'
[[ "$(jq -r '.reportSha256' "$evidence")" == "$FAKE_DIGEST" ]] ||
    fail 'the evidence must carry the validated digest'

# An out-of-shape probe list is refused even when the validator is satisfied.
short_report="$workspace/short-report.json"
FAKE_REPORT_TEMPLATE=$(make_template short passed none 2)
check 0 'short probe run' run --receipt "$receipt" --report "$short_report"
check 1 'short probe list' evidence --report "$short_report" --out "$workspace/short-evidence.json"
[[ ! -e "$workspace/short-evidence.json" ]] || fail 'an out-of-shape report produced evidence'
FAKE_REPORT_TEMPLATE=$(make_template passed passed none)

# Qualification is asserted from closed evidence, separately from validation.
check 0 'qualified run' qualify --evidence "$evidence"
expect_contains 'qualified run' 'status=qualified probes=3'

blocked_evidence="$workspace/blocked-evidence.json"
FAKE_REPORT_TEMPLATE=$(make_template blocked blocked application-exited)
blocked_report="$workspace/blocked-report.json"
check 0 'blocked template run' run --receipt "$receipt" --report "$blocked_report"
check 0 'blocked evidence' evidence --report "$blocked_report" --out "$blocked_evidence"
check 1 'blocked qualification' qualify --evidence "$blocked_evidence"
expect_contains 'blocked qualification' 'status=not-qualified'
expect_contains 'blocked qualification' 'reason=application-exited'

check 1 'absent evidence' qualify --evidence "$workspace/none.json"

printf 'chatgpt wave9 probe contracts passed\n'
