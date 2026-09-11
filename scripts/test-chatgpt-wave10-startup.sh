#!/usr/bin/env bash
# Synthetic, credential-free contracts for the temporary wave-10 startup
# diagnostic. Nothing here launches ChatGPT, reads a real receipt, needs `ps`,
# `ldd` or a display: the checker, the graphical session, the native inventory
# helper and the process table are all fixtures that record what they were
# asked to do, so the closed classifications are proved before any hosted run.
set -euo pipefail

script_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
contract="$script_root/chatgpt-wave10-startup.sh"
session_contract="$script_root/run-desktop-check-session.sh"
test_root=/tmp
[[ "$(uname -s)" != Darwin ]] || test_root=/private/tmp
workspace=$(mktemp -d "$test_root/chatgpt-wave10-tests.XXXXXX")
trap 'rm -rf -- "$workspace" "${audit_dir:-}"' EXIT

calls="$workspace/calls.log"
output="$workspace/output.txt"
: > "$calls"

pass_count=0
fail() {
    printf 'failed: %s\n' "$1" >&2
    exit 1
}

ok() {
    pass_count=$((pass_count + 1))
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
    "$@" > "$output" 2>&1 || status=$?
    [[ "$status" == "$expected" ]] ||
        fail "$name: expected exit $expected, got $status: $(tr '\n' ' ' < "$output")"
    ok
}

run_contract() {
    CHECKER="$fake_checker" SESSION_SCRIPT="$fake_session" NATIVE_HELPER="$fake_helper" \
        PS_COMMAND="$fake_ps" TIMEOUT_BIN="$fake_timeout" \
        LDD_COMMAND="$fake_ldd" \
        FAKE_LDD_GUI="${FAKE_LDD_GUI:-ok}" FAKE_LDD_RUNTIME="${FAKE_LDD_RUNTIME:-ok}" \
        DIAG_DIR="${diagnose_dir:-}" \
        SESSION_CALL_LOG="$calls" \
        SAMPLE_INTERVAL_MS=1 SAMPLE_MAX_TICKS=40 INVENTORY_DEADLINE=2s \
        FAKE_RUN_STATUS="$FAKE_RUN_STATUS" FAKE_VALIDATE_STATUS="$FAKE_VALIDATE_STATUS" \
        FAKE_WRITE_REPORT="$FAKE_WRITE_REPORT" FAKE_REPORT_TEMPLATE="$FAKE_REPORT_TEMPLATE" \
        FAKE_INVENTORY="$FAKE_INVENTORY" FAKE_PS_SCENARIO="$FAKE_PS_SCENARIO" \
        FAKE_HELPER_VERSION="$FAKE_HELPER_VERSION" \
        FAKE_PS_TICK_FILE="$FAKE_PS_TICK_FILE" \
        FAKE_PS_CHECKER_TICKS="${FAKE_PS_CHECKER_TICKS:-4}" \
        FAKE_CHATGPT_HEX="$CHATGPT_HEX" FAKE_OTHER_HEX="$OTHER_HEX" \
        FAKE_CHATGPT_LOWER_HEX="$CHATGPT_LOWER_HEX" \
        FAKE_CHATGPT_PACKAGE_HEX="$CHATGPT_PACKAGE_HEX" \
        FAKE_RAW_TITLE="$RAW_TITLE" FAKE_RAW_TITLE_HEX="$RAW_TITLE_HEX" \
        FAKE_FIXTURE_DIR="$workspace/fixtures" \
        FAKE_CHECKER_PID_FILE="$fake_checker_pid_file" \
        FAKE_WORKER_PGID=777 FAKE_OUTSIDE_PGID=999 \
        bash "$contract" "$@"
}

# Hex encoding of a process owner name, exactly as the native helper emits it.
# The helper never prints a name, a title or a path: an owner field is hex, so
# every fixture below speaks hex too.
name_hex() { printf '%s' "$1" | od -An -tx1 | tr -d ' \n'; }
CHATGPT_HEX=$(name_hex ChatGPT)
OTHER_HEX=$(name_hex other-process)
# The shipped matcher is case-insensitive, so a lower-cased owner is the same
# application. The helper only ever reports this encoded form, never the name.
CHATGPT_LOWER_HEX=$(name_hex chatgpt)
# A packaged helper starts with the application name without being the process
# the checker attributes: it proves the package ran, and nothing more.
CHATGPT_PACKAGE_HEX=$(name_hex 'ChatGPT Helper')
# A window title is not an owner name. Shaped the way the helper shapes a name,
# it still has to be neither claimed as the application nor echoed anywhere.
RAW_TITLE='ChatGPT - Sign in'
RAW_TITLE_HEX=$(name_hex "$RAW_TITLE")

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
        printf '%s\n' "$$" > "$FAKE_CHECKER_PID_FILE"
        printf 'run %s\n' "$*" >> "$SESSION_CALL_LOG"
        if [[ "$FAKE_WRITE_REPORT" == yes ]]; then
            cp -- "$FAKE_REPORT_TEMPLATE" "$output"
        fi
        # The shipped checker writes this summary to stdout; it names a path.
        printf 'Report: %s\n' "/home/runner/work/nan-harness/$output"
        printf 'Desktop launch diagnostic: Code(1)\n' >&2
        printf '%s\n' "private note: /home/runner/secret/path and $FAKE_RAW_TITLE" >&2
        sleep 0.2
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

# Stands in for scripts/run-desktop-check-session.sh, which owns the X server.
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

# Stands in for the native helper. The record shapes are the ones the shipped X11
# inventory writes: a leading `FG <pid> <window id>`, exactly one `DISPLAY` line,
# and `WIN <id> <pid> <x> <y> <width> <height> <hex-owner>` with geometry in the
# fixed three-decimal form the helper prints. A fixture that invented a friendlier
# shape would only prove that the contract agrees with itself.
make_helper() {
    local fixtures=$workspace/fixtures
    mkdir -p "$fixtures"
    # One record per argument. A fixture must not need line continuation to be
    # readable: inside single quotes a trailing backslash is a literal
    # character, and the native parser is required to reject exactly that kind
    # of stray record.
    write_fixture() {
        local name=$1 record
        shift
        : > "$fixtures/$name"
        for record in "$@"; do
            printf '%s\n' "$record" >> "$fixtures/$name"
        done
    }
    write_fixture empty 'FG 0 0' 'DISPLAY 0 0 1280 1024'
    write_fixture app-window 'FG 4242 9999' 'DISPLAY 0 0 1280 1024' \
        'WIN 1234 4242 0.000 0.000 1024.000 768.000 __CHATGPT__'
    # The same owner spelled in lower case is the same application: the shipped
    # matcher folds case, and `/proc/<pid>/comm` does not capitalise for us.
    write_fixture lowered-window 'FG 4242 9999' 'DISPLAY 0 0 1280 1024' \
        'WIN 1234 4242 0.000 0.000 1024.000 768.000 __CHATGPT_LOWER__'
    write_fixture small-window 'FG 0 0' 'DISPLAY 0 0 1280 1024' \
        'WIN 1234 4242 0.000 0.000 200.000 120.000 __CHATGPT__'
    # A foreign owner is a window; a name that merely starts with the application
    # name is a package helper, not the process the checker attributes. Neither
    # may be claimed, and a title-shaped value is not an owner field at all.
    write_fixture decoy 'FG 0 0' 'DISPLAY 0 0 1280 1024' \
        'WIN 1234 4242 0.000 0.000 1024.000 768.000 __OTHER__' \
        'WIN 1235 4243 0.000 0.000 1024.000 768.000 __CHATGPT_PACKAGE__' \
        'WIN 1236 4244 0.000 0.000 1024.000 768.000 __RAW_TITLE_HEX__'
    write_fixture fail 'a native failure message that must never be copied'
    # Grammar violations: each of these is a snapshot the shipped parser would
    # refuse, so none of them may reduce to a count of anything.
    write_fixture truncated 'FG 0 0' 'DISPLAY 0 0 1280 1024' \
        'WIN 1234 4242 0.000'
    write_fixture blank-line 'FG 0 0' '' 'DISPLAY 0 0 1280 1024'
    write_fixture no-foreground 'DISPLAY 0 0 1280 1024' \
        'WIN 1234 4242 0.000 0.000 100.000 100.000 -'
    write_fixture no-display 'FG 0 0' \
        'WIN 1234 4242 0.000 0.000 100.000 100.000 -'
    write_fixture unknown-record 'FG 0 0' 'DISPLAY 0 0 1280 1024' 'EXPOSE 1'
    write_fixture oversized-x 'FG 0 0' 'DISPLAY 0 0 1280 1024' \
        'WIN 1234 4242 999999.000 0.000 100.000 100.000 -'
    write_fixture zero-extent 'FG 0 0' 'DISPLAY 0 0 1280 1024' \
        'WIN 1234 4242 0.000 0.000 0.000 100.000 -'
    write_fixture fractional-x 'FG 0 0' 'DISPLAY 0 0 1280 1024' \
        'WIN 1234 4242 0.0001 0.000 100.000 100.000 -'
    write_fixture huge-pid 'FG 0 0' 'DISPLAY 0 0 1280 1024' \
        'WIN 1234 99999999999999 0.000 0.000 100.000 100.000 -'
    write_fixture malformed-owner 'FG 0 0' 'DISPLAY 0 0 1280 1024' \
        'WIN 1234 4242 0.000 0.000 100.000 100.000 zz'
    write_fixture stray-record 'FG 0 0' 'DISPLAY 0 0 1280 1024' '\'
    write_fixture empty-reply
    # Bounds, not just shapes: the shipped parser caps the owner token, the
    # display records and the window records, and a reply past any cap is a
    # reply it would refuse. Each file is generated because the bound is a size.
    {
        printf 'FG 0 0\nDISPLAY 0 0 1280 1024\n'
        printf 'WIN 1234 4242 0.000 0.000 100.000 100.000 %s\n' "$(printf 'a%.0s' $(seq 1 514))"
    } > "$fixtures/oversized-owner"
    {
        printf 'FG 0 0\n'
        for _ in $(seq 1 33); do printf 'DISPLAY 0 0 1280 1024\n'; done
    } > "$fixtures/many-displays"
    {
        printf 'FG 0 0\nDISPLAY 0 0 1280 1024\n'
        for id in $(seq 1 1025); do
            printf 'WIN %s %s 0.000 0.000 100.000 100.000 -\n' "$((1000 + id))" "$((2000 + id))"
        done
    } > "$fixtures/many-windows"
    # A loop over names only proves something when the file behind each name
    # exists: a typo would make the fake fail for the wrong reason and still
    # look like a refusal, so the shapes are checked before they are trusted.
    for expected in empty app-window lowered-window small-window decoy fail truncated \
        blank-line no-foreground no-display unknown-record oversized-x zero-extent \
        fractional-x huge-pid malformed-owner stray-record oversized-owner \
        many-displays many-windows; do
        [[ -s "$fixtures/$expected" ]] || fail "the $expected fixture is missing or empty"
    done
    local path=$workspace/fake-helper
    cat > "$path" <<'EOF'
#!/usr/bin/env bash
set -eu
case "${1:-}" in
    --version)
        [[ "$FAKE_HELPER_VERSION" == yes ]] || exit 3
        printf 'nanh-desktop-native tesseract-synthetic\n'
        ;;
    --windows)
        case "$FAKE_INVENTORY" in
            # The names below are the shapes the real helper can produce. Each
            # one stands for a different answer the contract has to classify:
            # a refusal with a message, an exit-zero silence, and a hang.
            fail)
                printf 'a native failure message that must never be copied\n' >&2
                exit 5
                ;;
            exit-zero-empty) exit 0 ;;
            timeout-hang) exec sleep 30 ;;
            *)
                sed -e "s/__CHATGPT__/$FAKE_CHATGPT_HEX/g" \
                    -e "s/__CHATGPT_LOWER__/$FAKE_CHATGPT_LOWER_HEX/g" \
                    -e "s/__CHATGPT_PACKAGE__/$FAKE_CHATGPT_PACKAGE_HEX/g" \
                    -e "s/__OTHER__/$FAKE_OTHER_HEX/g" \
                    -e "s/__RAW_TITLE_HEX__/$FAKE_RAW_TITLE_HEX/g" \
                    "$FAKE_FIXTURE_DIR/$FAKE_INVENTORY"
                ;;
        esac
        ;;
    *) exit 2 ;;
esac
EOF
    chmod 755 "$path"
    printf '%s' "$path"
}

# Stands in for the coreutils deadline the contract asks for: run a command, end
# it when the deadline passes, and report what happened. A host without `timeout`
# still has to prove that a hung inventory is bounded rather than believed.
make_timeout() {
    local path=$workspace/fake-timeout
    cat > "$path" <<'EOF'
#!/usr/bin/env bash
set -u
deadline=$1
shift
seconds=${deadline%s}
[[ "$seconds" =~ ^[0-9]+$ ]] || seconds=1
"$@" &
child=$!
# The watcher holds no descriptor of the caller's: a sleeper that keeps the
# capture pipe open would make every bounded call look like a hung one.
(
    sleep "$seconds" 2> /dev/null
    kill -9 "$child" 2> /dev/null
) < /dev/null > /dev/null 2> /dev/null &
watcher=$!
status=0
wait "$child" || status=$?
kill "$watcher" 2> /dev/null
wait "$watcher" 2> /dev/null
exit "$status"
EOF
chmod 755 "$path"
    printf '%s' "$path"
}

# Stands in for the dynamic loader report. The contract asks about two paths,
# the GUI binary and the packaged runtime, and each answer is shaped by its own
# flag: `ok` is a clean report, `missing` a report with unresolved entries, and
# `fail` a loader that could not produce a report at all — which the contract
# must record as unknown, never as a zero.
make_ldd() {
    local path=$workspace/fake-ldd
    cat > "$path" <<'EOF'
#!/usr/bin/env bash
set -eu
case "${1:-}" in
    */ChatGPT)
        case "$FAKE_LDD_GUI" in
            ok) printf 'libm.so.6 => /lib/x86_64-linux-gnu/libm.so.6 (0x0000f00d)\n' ;;
            missing) printf 'libgone.so => not found\n' ;;
            *) exit 1 ;;
        esac
        ;;
    */resources/codex)
        case "$FAKE_LDD_RUNTIME" in
            ok) printf 'libc.so.6 => /lib/x86_64-linux-gnu/libc.so.6 (0x0000f00e)\n' ;;
            missing)
                printf 'libgone.so => not found\nlibalsogone.so => not found\n' ;;
            *) exit 1 ;;
        esac
        ;;
    *) exit 1 ;;
esac
EOF
    chmod 755 "$path"
    printf '%s' "$path"
}

# Stands in for the process table. Rows are "pid ppid pgid stat comm", the
# column order the contract parses. A tick counter replays a lifecycle:
#   none     the packaged binary never appears
#   app      the binary appears inside the probe group, then the checker leaves
#   survivor the binary outlives the probe group in another process group
#   episodes three separate binary episodes inside one longer checker life,
#            a synthetic shape that exercises the counter; it attributes no
#            episode to any particular probe
make_ps() {
    local path=$workspace/fake-ps
    cat > "$path" <<'EOF'
#!/usr/bin/env bash
set -eu
counter="$FAKE_PS_TICK_FILE"
tick=$(($(cat -- "$counter") + 1))
printf '%s\n' "$tick" > "$counter"
checker=""
for _ in $(seq 1 40); do
    [[ -s "$FAKE_CHECKER_PID_FILE" ]] && break
    sleep 0.05
done
[[ -s "$FAKE_CHECKER_PID_FILE" ]] || exit 0
checker=$(cat -- "$FAKE_CHECKER_PID_FILE")
# Ticks 1 to 4 replay the attempt; from tick 5 the checker is gone, which is
# how the contract learns to stop sampling. The life is bounded by
# FAKE_PS_CHECKER_TICKS so a scenario can stretch one timeline across what
# would be several sequential probe attempts.
if [[ $tick -le "${FAKE_PS_CHECKER_TICKS:-4}" ]]; then
    printf '%s 1 %s S fake-checker\n' "$checker" "$checker"
    if [[ $tick -ge 2 ]]; then
        printf '31337 %s %s S fake-checker\n' "$checker" "$FAKE_WORKER_PGID"
    fi
fi
case "$FAKE_PS_SCENARIO" in
    app)
        if [[ $tick -ge 2 && $tick -le 4 ]]; then
            printf '4242 31337 %s S ChatGPT\n' "$FAKE_WORKER_PGID"
        fi
        ;;
    episodes)
        # Three two-tick episodes separated by idle ticks. The probe count
        # stays unattributed: one probe whose binary restarts three times
        # makes the same shape, so this exercises episode counting only.
        case "$tick" in
            3 | 4 | 8 | 9 | 13 | 14)
                printf '4242 31337 %s S ChatGPT\n' "$FAKE_WORKER_PGID" ;;
        esac
        ;;
    survivor)
        printf '4242 31337 %s S ChatGPT\n' "$FAKE_OUTSIDE_PGID"
        ;;
    # A packaged helper name proves part of the application ran without being
    # the exact name the checker attributes; a name that merely contains the
    # application name is neither.
    helper-only)
        if [[ $tick -ge 2 && $tick -le 4 ]]; then
            printf '4243 31337 %s S ChatGPT Helper\n' "$FAKE_WORKER_PGID"
            printf '4244 1 %s S desktop-chatgpt-launch\n' "$FAKE_WORKER_PGID"
        fi
        ;;
    # The same helper, outliving the attempt inside its own process group.
    helper-survivor)
        printf '4243 31337 %s S ChatGPT Helper\n' "$FAKE_OUTSIDE_PGID"
        ;;
esac
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
  "nanhIdentity": { "version": "0.1.4", "sha256": "$(printf 'f%.0s' $(seq 1 64))" },
  "apps": [ {
    "app": "chatgpt-desktop",
    "executable": { "path": "$executable", "sha256": "$(printf 'e%.0s' $(seq 1 64))" },
    "appVersion": "26.903.71938",
    "runtimeVersion": "0.153.4"
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
        nanHarness: { version: "0.1.4", sha256: "7176b0dd5bc6e5fe8258579f157dd07136aa29d18e35dbd75b6d1993e97c5176" },
        results: [ {
            app: "chatgpt-desktop", appVersion: "26.903.71938", runtimeVersion: "0.153.4",
            deterministic: [ range(0; $probes) | {
                status: $status,
                reason: (if $reason == "none" then null else $reason end),
                steps: [],
                durationMilliseconds: (8236 + .)
            } ],
            live: { status: "skipped", reason: "missing-key", steps: [], durationMilliseconds: 0 },
            cleanup: "passed"
        } ],
        cleanup: "passed"
    }' > "$path"
    printf '%s' "$path"
}

# A scenario owns a private observation directory with the mode the contract
# enforces, plus a separate public directory for the artifacts it may publish.
# The closed facts `environment` would leave behind are written here so the
# classifications under test are decided by the observations and not by this
# host's loader tools.
scenario() {
    local name=$1
    diagnose_dir="$workspace/diag-$name"
    install -d -m 700 "$diagnose_dir"
    public_dir="$workspace/public-$name"
    install -d -m 755 "$public_dir"
    report="$public_dir/report.json"
    evidence="$public_dir/evidence.json"
    FAKE_PS_TICK_FILE="$diagnose_dir/ps-tick"
    fake_checker_pid_file="$diagnose_dir/checker.pid"
    printf '0\n' > "$FAKE_PS_TICK_FILE"
    : > "$fake_checker_pid_file"
    printf '0 0\n' > "$diagnose_dir/fact-libraries"
    printf '1 max 1\n' > "$diagnose_dir/fact-namespace"
    printf 'absent\n' > "$diagnose_dir/fact-sandbox-sibling"
    printf 'yes\n' > "$diagnose_dir/fact-runtime-file"
    printf 'yes\n' > "$diagnose_dir/fact-resources-dir"
    FAKE_PS_SCENARIO=none
    FAKE_INVENTORY=empty
}

prepare_run() {
    check 0 "$1 helper" run_contract helper-assert
    check 0 "$1 source" run_contract source "$REVISION"
    check 0 "$1 environment facts" run_contract environment --receipt "$receipt"
    # Every scenario below is classified from observations that include the
    # launcher kind, so the closed word must appear on that line.
    expect_contains "$1 environment" 'executable=elf'
    printf '0 0\n' > "$diagnose_dir/fact-libraries"
    printf '1 max 1\n' > "$diagnose_dir/fact-namespace"
}

[[ -f "$contract" ]] || fail 'the wave10 diagnostic contract is missing'
[[ -f "$session_contract" ]] || fail 'the reusable session script must stay in place'

fake_checker=$(make_checker)
fake_session=$(make_session)
fake_helper=$(make_helper)
fake_ps=$(make_ps)
fake_timeout=$(make_timeout)
fake_ldd=$(make_ldd)
FAKE_DIGEST=$(printf 'a%.0s' $(seq 1 64))
export FAKE_DIGEST
REVISION=$(printf '1%.0s' $(seq 1 40))
FAKE_RUN_STATUS=0
FAKE_VALIDATE_STATUS=0
FAKE_WRITE_REPORT=yes
FAKE_INVENTORY=empty
FAKE_PS_SCENARIO=none
FAKE_HELPER_VERSION=yes
FAKE_REPORT_TEMPLATE=$(make_template failed failed application-exited)

app_root="$workspace/run/install-chatgpt/application/usr/lib/chatgpt"
mkdir -p "$app_root/resources/app"
# The packaged GUI binary, identified by its leading bytes. The contract reads
# only the magic, never the content, so the fixture needs no real image.
printf '\177ELF' > "$app_root/ChatGPT"
chmod 755 "$app_root/ChatGPT"
printf 'synthetic runtime\n' > "$app_root/resources/codex"
receipt_gui="$app_root/ChatGPT"
receipt=$(make_receipt gui "$receipt_gui")

# The identity assertions need no observations, but the contract always
# receives a private directory to write them to.
scenario identity

# The prepared executable is the package's one GUI binary, named by path shape.
check 0 'prepared GUI executable' run_contract prepare-assert --receipt "$receipt"
expect_contains 'prepared GUI executable' 'version=26.903.71938'

# A launcher alias or a bundled runtime is a different binary and is refused.
launcher=$(make_receipt launcher "$app_root/codex-launcher")
check 1 'launcher alias' run_contract prepare-assert --receipt "$launcher"
runtime=$(make_receipt runtime "$app_root/resources/codex")
check 1 'bundled runtime' run_contract prepare-assert --receipt "$runtime"
check 1 'absent receipt' run_contract prepare-assert --receipt "$workspace/none.json"

# The launcher kind comes from the leading bytes only, as one closed word: a
# native image and a script wrapper fail in different ways at this boundary.
scenario kind
check 0 'elf kind' run_contract environment --receipt "$receipt"
expect_contains 'elf kind' 'executable=elf'
printf '#!/bin/sh\n' > "$app_root/ChatGPT"
check 0 'script kind' run_contract environment --receipt "$receipt"
expect_contains 'script kind' 'executable=script'
: > "$app_root/ChatGPT"
check 0 'no magic kind' run_contract environment --receipt "$receipt"
expect_contains 'no magic kind' 'executable=other'
printf '\177ELF' > "$app_root/ChatGPT"

# The loader report is a closed observation in both directions: a report that
# could not be produced is unknown (-1), a report that was produced counts its
# unresolved entries. Neither binary's failure may read as the other's zero,
# because the library classification is decided by exactly these integers.
scenario loader
FAKE_LDD_GUI=ok FAKE_LDD_RUNTIME=ok
check 0 'clean loader reports' run_contract environment --receipt "$receipt"
expect_contains 'clean loader reports' 'libraries=0 0'
FAKE_LDD_GUI=missing FAKE_LDD_RUNTIME=ok
check 0 'gui loader gap counted' run_contract environment --receipt "$receipt"
expect_contains 'gui loader gap counted' 'libraries=1 0'
FAKE_LDD_GUI=ok FAKE_LDD_RUNTIME=missing
check 0 'runtime loader gap counted' run_contract environment --receipt "$receipt"
expect_contains 'runtime loader gap counted' 'libraries=0 2'
FAKE_LDD_GUI=fail FAKE_LDD_RUNTIME=ok
check 0 'gui loader failure is unknown' run_contract environment --receipt "$receipt"
expect_contains 'gui loader failure is unknown' 'libraries=-1 0'
[[ "$(cat -- "$diagnose_dir/fact-libraries")" == "-1 0" ]] ||
    fail 'a failed gui loader report was recorded as a number it never observed'
FAKE_LDD_GUI=ok FAKE_LDD_RUNTIME=fail
check 0 'runtime loader failure is unknown' run_contract environment --receipt "$receipt"
expect_contains 'runtime loader failure is unknown' 'libraries=0 -1'
[[ "$(cat -- "$diagnose_dir/fact-libraries")" == "0 -1" ]] ||
    fail 'a failed runtime loader report was recorded as a silent zero'
FAKE_LDD_GUI=ok FAKE_LDD_RUNTIME=ok

# Asserting the helper is one claim; asking it for windows is another, and it
# only means something inside the graphical session the probe will run in.
scenario helper
check 0 'helper asserted' run_contract helper-assert
expect_contains 'helper asserted' 'status=helper digest='
# Recording a digest is not an inventory: a helper-assert that also printed a
# window claim would be asserting a fact it never observed.
[[ ! -e "$diagnose_dir/inventory-session" ]] ||
    fail 'helper-assert claimed a window inventory it never asked for'
check 0 'session preflight' run_contract session-preflight
expect_contains 'session preflight' 'session-inventory=usable'
[[ $(cut -d' ' -f1 < "$diagnose_dir/inventory-session") == 0 ]] ||
    fail 'an empty inventory must report zero windows'

FAKE_HELPER_VERSION=no
check 1 'helper without version' run_contract helper-assert
FAKE_HELPER_VERSION=yes
# A different binary answering in session is a different run, whatever it says.
printf 'a%.0s' $(seq 1 64) > "$diagnose_dir/helper-digest"
check 1 'helper swapped after assertion' run_contract session-preflight
check 0 'helper re-asserted' run_contract helper-assert
FAKE_INVENTORY=fail
check 1 'refused inventory blocks the session' run_contract session-preflight
expect_absent 'refused inventory blocks the session' 'session-inventory=usable'
# Every shape the shipped parser would refuse has to block the session too,
# including a reply that exits zero without saying anything at all.
for broken in truncated blank-line no-foreground no-display unknown-record \
    oversized-x zero-extent fractional-x huge-pid malformed-owner empty-reply \
    stray-record oversized-owner many-displays many-windows exit-zero-empty; do
    FAKE_INVENTORY="$broken"
    check 0 'helper re-asserted for '"$broken" run_contract helper-assert
    check 1 'unreadable inventory: '"$broken" run_contract session-preflight
done
check 0 'helper re-asserted' run_contract helper-assert
# A hung inventory is stopped by a deadline instead of holding the run open, and
# what it records is an unknown observation, never a zero.
FAKE_INVENTORY=timeout-hang
export INVENTORY_DEADLINE=1s
check 1 'hung inventory blocks the session' run_contract session-preflight
[[ "$(cut -d' ' -f1,2,3,4,5 < "$diagnose_dir/inventory-session")" == "-1 -1 -1 -1 -1" ]] ||
    fail 'a timed-out inventory was recorded as an observation'
[[ "$(cat "$diagnose_dir/inventory-session-exit")" != 0 ]] ||
    fail 'a timed-out inventory reported a successful helper'
unset INVENTORY_DEADLINE
FAKE_INVENTORY=empty
check 1 'helper-assert without a helper' env DIAG_DIR="$diagnose_dir" \
    CHECKER="$fake_checker" bash "$contract" helper-assert
check 1 'helper-assert without a private directory' env NATIVE_HELPER="$fake_helper" \
    CHECKER="$fake_checker" bash "$contract" helper-assert

check 1 'short revision refused' run_contract source deadbeef
check 1 'path-shaped revision refused' run_contract source '/home/runner/x'

# ---------------------------------------------------------------------------
# Scenario: the packaged binary never appears. The launcher exited first, so
# this must classify as a wrapper-side exit and not as an app or window fault.
# ---------------------------------------------------------------------------
scenario never-app
prepare_run never-app
FAKE_RUN_STATUS=1
check 1 'never-app probe' run_contract run --receipt "$receipt" --report "$report"
expect_contains 'never-app probe' 'status=probed exit=1 probes=3 observations=5'
expect_contains 'never-app probe' 'app-process-samples=0 app-window-samples=0 survivors=0'
# Nothing the checker wrote privately may reach the workflow log: the fixture
# stderr line names a runner path and a window title-shaped value.
expect_absent 'never-app probe' 'home'
expect_absent 'never-app probe' 'runner'
expect_absent 'never-app probe' 'Sign in'
expect_absent 'never-app probe' 'private note'
grep -qF -- 'forwarded' "$calls" || fail 'the probe skipped the session wrapper'
grep -qF -- 'run --yes --non-interactive --ephemeral --mode deterministic' "$calls" ||
    fail 'the probe changed the deterministic checker command'
[[ -s "$report" ]] || fail 'a failed probe must still preserve its canonical report'

check 0 'never-app evidence' run_contract evidence --report "$report" \
    --out "$evidence" --receipt "$receipt"
expect_contains 'never-app evidence' 'classification=launcher-exited-before-app'
expect_contains 'never-app evidence' 'app-process-samples=0'
[[ "$(jq -r '.startup.probeWorkers, .launcher.launcherExits, .launcher.unmatchedPrivateLines' "$evidence")" == "1
1
1" ]] || fail 'the closed launcher observations are wrong'
[[ "$(jq -r '.qualification | to_entries | map("\(.key)=\(.value)") | join(",")' "$evidence")" == \
    'nativeInventoryWired=true,postRunInventoryAnswered=true,probeWorkersObserved=true,appProcessObserved=false,appWindowObserved=false,appTestableWindowObserved=false,survivorObserved=false' ]] ||
    fail "the qualification booleans are wrong: $(jq -c '.qualification' "$evidence")"
[[ "$(jq -r '.identity | .appVersion, .runtimeVersion' "$evidence")" == "26.903.71938
0.153.4" ]] || fail 'package identity was not carried through'
[[ "$(jq -r '[.probes[].reason] | unique | join(",")' "$evidence")" == "application-exited" ]] ||
    fail 'probe reasons were not carried through'
[[ "$(jq -r '[.probes[].index] | join(",")' "$evidence")" == "1,2,3" ]] || fail 'probe numbering'
[[ "$(jq -r '.environment.launcherKind' "$evidence")" == "elf" ]] ||
    fail 'the launcher kind was not carried through as a closed word'
# Three kernel switches, three published integers. The third one is the switch
# this runner's distribution gates on, and a bundle that lost it silently would
# read as "no policy problem" instead of "nobody looked".
[[ "$(jq -r '.environment | (.unprivilegedUserns, .maxUserNamespacesZero,
    .apparmorUsernsRestriction)' "$evidence")" == "1
0
1" ]] ||
    fail 'the namespace switches were not carried through as closed numbers'

# Privacy: the private fixture line names a runner path and a window title.
# Neither may appear in the evidence, nor in anything the contract printed.
expect_absent 'never-app evidence' 'Sign in'
expect_absent 'never-app evidence' 'home'
expect_absent 'never-app evidence' 'runner'
grep -qF -- "$RAW_TITLE" "$evidence" && fail 'evidence leaked an owner name' || true
grep -qF -- '/home/runner' "$evidence" && fail 'evidence leaked a path' || true
grep -qF -- "$CHATGPT_HEX" "$evidence" && fail 'evidence carries an encoded owner name' || true

keys=$(jq -r 'keys_unsorted | join(",")' "$evidence")
expected_keys='schemaVersion,app,source,identity,probes,startup,inventory,launcher,environment,cleanup,qualification,classification'
[[ "$keys" == "$expected_keys" ]] || fail "unexpected evidence keys: $keys"
check 0 'never-app qualify' run_contract qualify --evidence "$evidence"
expect_contains 'never-app qualify' 'status=evidence-complete'
expect_contains 'never-app qualify' 'app-outcome=all-failed'

# ---------------------------------------------------------------------------
# Scenario: the binary runs inside the probe group and no window ever appears.
# ---------------------------------------------------------------------------
scenario ran-no-window
prepare_run ran-no-window
FAKE_PS_SCENARIO=app
FAKE_RUN_STATUS=1
check 1 'ran-no-window probe' run_contract run --receipt "$receipt" --report "$report"
expect_contains 'ran-no-window probe' 'app-process-samples=3 app-window-samples=0 survivors=0'
expect_contains 'ran-no-window probe' 'app-process-episodes=1 app-named-process-episodes=1'
check 0 'ran-no-window evidence' run_contract evidence --report "$report" \
    --out "$evidence" --receipt "$receipt"
expect_contains 'ran-no-window evidence' 'classification=app-exited-before-window'
# The clock is host-dependent (centisecond uptime on Linux, nominal ticks
# elsewhere), so the episode is bounded rather than pinned to one tick number.
[[ "$(jq -r '.startup.appProcessMax' "$evidence")" == 1 ]] ||
    fail 'the app process episode was not measured'
[[ "$(jq -r '.startup.appProcessEpisodes, .startup.appNamedProcessEpisodes' \
    "$evidence")" == "1
1" ]] || fail 'one contiguous app episode was not counted as one'
first_app=$(jq -r '.startup.appProcessFirstSampleMilliseconds' "$evidence")
elapsed=$(jq -r '.startup.elapsedMilliseconds' "$evidence")
[[ "$first_app" =~ ^[0-9]+$ && "$elapsed" =~ ^[0-9]+$ &&
    $first_app -le $elapsed ]] ||
    fail "the app process episode was not measured on a monotonic clock: $first_app/$elapsed"
[[ "$(jq -r '.qualification.appProcessObserved' "$evidence")" == true ]] ||
    fail 'an observed app process must be reported'

# ---------------------------------------------------------------------------
# Scenario: three separate application episodes inside one sampling window.
# Sample totals cannot tell this apart from one long episode; episode counts
# can. The count still names no probe: multiple episodes can occur within one
# probe, so three episodes are three separately observable lifetimes, not
# three proven probe launches, and one episode beside three launcher exits
# names no faulty boundary.
# ---------------------------------------------------------------------------
scenario three-episodes
prepare_run three-episodes
FAKE_PS_SCENARIO=episodes
FAKE_PS_CHECKER_TICKS=16
FAKE_RUN_STATUS=1
check 1 'three-episode probe' run_contract run --receipt "$receipt" --report "$report"
expect_contains 'three-episode probe' 'app-process-samples=6'
expect_contains 'three-episode probe' 'app-process-episodes=3 app-named-process-episodes=3'
check 0 'three-episode evidence' run_contract evidence --report "$report" \
    --out "$evidence" --receipt "$receipt"
expect_contains 'three-episode evidence' 'classification=app-exited-before-window'
[[ "$(jq -r '.startup.appProcessEpisodes' "$evidence")" == 3 ]] ||
    fail 'three separated app episodes did not count as three observed lifetimes'
[[ "$(jq -r '.startup.appProcessSamples > .startup.appProcessEpisodes' \
    "$evidence")" == true ]] || fail 'episodes must stay below their samples'
check 0 'three-episode qualify' run_contract qualify --evidence "$evidence"
expect_contains 'three-episode qualify' 'status=evidence-complete'
FAKE_PS_SCENARIO=none
FAKE_PS_CHECKER_TICKS=4

# ---------------------------------------------------------------------------
# Scenario: an attributed window exists on every app tick but stays below the
# 300x200 test minimum. This is complete evidence of its own closed state, so
# it must classify as window-undersized and still qualify.
# ---------------------------------------------------------------------------
scenario undersized
FAKE_INVENTORY=small-window
FAKE_PS_SCENARIO=app
prepare_run undersized
FAKE_RUN_STATUS=1
check 1 'undersized probe' run_contract run --receipt "$receipt" --report "$report"
# The static inventory fixture answers the same on every tick, so the window
# sample count is the observation count, while the testable subset stays zero.
expect_contains 'undersized probe' 'app-window-samples=5'
expect_contains 'undersized probe' 'app-testable-window-samples=0'
check 0 'undersized evidence' run_contract evidence --report "$report" \
    --out "$evidence" --receipt "$receipt"
expect_contains 'undersized evidence' 'classification=window-undersized'
check 0 'undersized qualify' run_contract qualify --evidence "$evidence"
expect_contains 'undersized qualify' 'status=evidence-complete'
FAKE_INVENTORY=empty
FAKE_PS_SCENARIO=none

# ---------------------------------------------------------------------------
# Scenario: the packaged runtime reports unresolved dependencies. The library
# gap outranks the exit ordering, and the run still qualifies on evidence.
# ---------------------------------------------------------------------------
scenario library-gap
prepare_run library-gap
printf '0 2\n' > "$diagnose_dir/fact-libraries"
FAKE_PS_SCENARIO=app
FAKE_RUN_STATUS=1
check 1 'library-gap probe' run_contract run --receipt "$receipt" --report "$report"
check 0 'library-gap evidence' run_contract evidence --report "$report" \
    --out "$evidence" --receipt "$receipt"
expect_contains 'library-gap evidence' 'classification=environment-libraries'
check 0 'library-gap qualify' run_contract qualify --evidence "$evidence"
expect_contains 'library-gap qualify' 'status=evidence-complete'
# A two-token namespace fact is an older reading of the kernel, not a smaller
# answer: the builder must refuse it rather than publish a bundle that quietly
# drops the switch it stopped looking at.
printf '1 max\n' > "$diagnose_dir/fact-namespace"
check 1 'truncated namespace fact' run_contract evidence --report "$report" \
    --out "$evidence" --receipt "$receipt"
printf '1 max 1\n' > "$diagnose_dir/fact-namespace"
check 0 'restored namespace fact' run_contract evidence --report "$report" \
    --out "$evidence" --receipt "$receipt"

# ---------------------------------------------------------------------------
# Scenario: the binary outlives the probe group in another process group.
# ---------------------------------------------------------------------------
scenario detached
prepare_run detached
FAKE_PS_SCENARIO=survivor
FAKE_RUN_STATUS=1
check 1 'detached probe' run_contract run --receipt "$receipt" --report "$report"
check 0 'detached evidence' run_contract evidence --report "$report" \
    --out "$evidence" --receipt "$receipt"
expect_contains 'detached evidence' 'classification=detached-descendant'
expect_contains 'detached evidence' 'survivors=1'
[[ "$(jq -r '.startup.survivorsOutsideProbeGroup' "$evidence")" == 1 ]] ||
    fail 'a survivor outside the probe group must be counted'
[[ "$(jq -r '.qualification.survivorObserved' "$evidence")" == true ]] ||
    fail 'a survivor must qualify as detached, whatever else was seen'

# ---------------------------------------------------------------------------
# Inventory reduction: owner names are matched as hex and geometry uses the
# checker's own minimum test window. Anything else in the field is ignored.
# ---------------------------------------------------------------------------
scenario window-present
FAKE_INVENTORY=app-window
FAKE_PS_SCENARIO=app
prepare_run window-present
FAKE_RUN_STATUS=1
check 1 'window probe' run_contract run --receipt "$receipt" --report "$report"
check 0 'window evidence' run_contract evidence --report "$report" \
    --out "$evidence" --receipt "$receipt"
expect_contains 'window evidence' 'classification=window-discovery'
expect_contains 'window evidence' 'app-window-samples=5'
[[ "$(jq -r '.inventory.session | [.windows, .appWindows, .appTestableWindows,
    .foregroundIsApp, .exit] | join(",")' "$evidence")" == "1,1,1,1,0" ]] ||
    fail 'a matching application window was not inventoried correctly'
[[ "$(jq -r '.inventory.afterRun | length' "$evidence")" == 3 ]] ||
    fail 'the after-run inventories must all be recorded'

# Counting windows and claiming them are separate jobs: everything on the
# display is counted, and only an owner name that equals the public application
# name is claimed. Columns are usability, windows, claimed, testable, focused.
scenario decoy-window
prepare_run decoy-window
FAKE_INVENTORY=decoy
check 0 'decoy session' run_contract session-preflight
[[ "$(cut -d' ' -f1,2,3,4 < "$diagnose_dir/inventory-session")" == "0 3 0 0" ]] ||
    fail "a foreign, packaged or title-shaped owner was claimed: $(
        cat -- "$diagnose_dir/inventory-session")"

scenario small-window
prepare_run small-window
FAKE_INVENTORY=small-window
check 0 'small window session' run_contract session-preflight
[[ "$(cut -d' ' -f1,2,3,4 < "$diagnose_dir/inventory-session")" == "0 1 1 0" ]] ||
    fail 'a window below the checker minimum must not count as testable'

# A lower-cased owner is still this application, and an active window that the
# root-level enumeration never lists is still attributed by its owner pid.
scenario lowered-window
prepare_run lowered-window
FAKE_INVENTORY=lowered-window
check 0 'lowered owner session' run_contract session-preflight
[[ "$(cut -d' ' -f1,2,3,4,5 < "$diagnose_dir/inventory-session")" == "0 1 1 1 1" ]] ||
    fail "an owner-name case variant or a child focus id was not attributed: $(
        cat -- "$diagnose_dir/inventory-session")"
FAKE_INVENTORY=empty

# ---------------------------------------------------------------------------
# Scenario: only a packaged helper name ever appears. Part of the application
# ran, yet no process carried the exact name the checker attributes, so the
# run must not be reported as "the binary never started".
# ---------------------------------------------------------------------------
scenario helper-only
prepare_run helper-only
FAKE_PS_SCENARIO=helper-only
FAKE_RUN_STATUS=1
check 1 'helper-only probe' run_contract run --receipt "$receipt" --report "$report"
expect_contains 'helper-only probe' 'app-process-samples=0'
expect_contains 'helper-only probe' 'app-named-process-samples=3'
check 0 'helper-only evidence' run_contract evidence --report "$report" \
    --out "$evidence" --receipt "$receipt"
expect_contains 'helper-only evidence' 'classification=app-named-process-without-main'
# Exactly three named samples also proves the prefix is anchored: the fixture
# writes a third row whose name merely contains the application name.
[[ "$(jq -r '.startup.appNamedProcessSamples, .startup.appNamedProcessMax,
    .startup.namedSurvivors' "$evidence")" == "3
1
0" ]] || fail 'the helper episode was not measured apart from the attributed name'
check 0 'helper-only qualify' run_contract qualify --evidence "$evidence"
expect_contains 'helper-only qualify' 'status=evidence-complete'

# A surviving application-named helper is recorded even when the attributed
# name never existed, because the two integers answer different questions.
scenario helper-survivor
prepare_run helper-survivor
FAKE_PS_SCENARIO=helper-survivor
FAKE_RUN_STATUS=1
check 1 'helper-survivor probe' run_contract run --receipt "$receipt" --report "$report"
check 0 'helper-survivor evidence' run_contract evidence --report "$report" \
    --out "$evidence" --receipt "$receipt"
[[ "$(jq -r '.startup.survivors, .startup.namedSurvivors,
    .startup.survivorsOutsideProbeGroup' "$evidence")" == "0
1
0" ]] || fail 'a surviving helper must count as named, never as attributed'
expect_contains 'helper-survivor evidence' 'classification=app-named-process-without-main'

# ---------------------------------------------------------------------------
# Fail-closed paths: nothing uploadable may come from an unvalidated,
# out-of-shape or private-value-carrying report.
# ---------------------------------------------------------------------------
scenario gate
prepare_run gate
FAKE_RUN_STATUS=1
check 1 'gate probe' run_contract run --receipt "$receipt" --report "$report"
check 0 'gate evidence' run_contract evidence --report "$report" \
    --out "$evidence" --receipt "$receipt"

FAKE_VALIDATE_STATUS=3
check 1 'unvalidated report' run_contract evidence --report "$report" \
    --out "$public_dir/none.json" --receipt "$receipt"
[[ ! -e "$public_dir/none.json" && ! -e "$public_dir/none.json.tmp" ]] ||
    fail 'an unvalidated report produced evidence'
FAKE_VALIDATE_STATUS=0

FAKE_REPORT_TEMPLATE=$(make_template narrow failed application-exited 2)
narrow="$public_dir/narrow-report.json"
# The synthetic checker still exits 1; only the probe count changed here.
printf '0\n' > "$FAKE_PS_TICK_FILE"
check 1 'two probe run' run_contract run --receipt "$receipt" --report "$narrow"
check 1 'short probe list' run_contract evidence --report "$narrow" \
    --out "$public_dir/narrow-evidence.json" --receipt "$receipt"
[[ ! -e "$public_dir/narrow-evidence.json" ]] ||
    fail 'an out-of-shape report produced evidence'

# A path-shaped value anywhere in the report must be refused before upload.
printf '%s\n' "$(jq '.results[0].appVersion = "/home/runner/install-chatgpt/ChatGPT"' \
    "$(make_template leaked failed application-exited)")" > "$public_dir/leaky-report.json"
check 1 'path in report' run_contract evidence --report "$public_dir/leaky-report.json" \
    --out "$public_dir/leaky-evidence.json" --receipt "$receipt"
[[ ! -e "$public_dir/leaky-evidence.json" ]] || fail 'evidence accepted a path-shaped value'

# A private observation that stops being a closed classification is refused
# here too, because it reaches the evidence through the same channel.
printf '/usr/libexec/chatgpt helper\n' > "$diagnose_dir/fact-executable-kind"
check 1 'free-form launcher kind' run_contract evidence --report "$report" \
    --out "$public_dir/kind-evidence.json" --receipt "$receipt"
[[ ! -e "$public_dir/kind-evidence.json" ]] ||
    fail 'evidence accepted a launcher kind outside the closed set'
printf 'elf\n' > "$diagnose_dir/fact-executable-kind"

# A failed probe that recorded no report is a closed failure, not an empty bundle.
FAKE_WRITE_REPORT=no
check 1 'probe without report' run_contract run --receipt "$receipt" \
    --report "$public_dir/absent.json"
[[ ! -e "$public_dir/absent.json" ]] || fail 'the wrapper must not invent a report'
FAKE_WRITE_REPORT=yes
FAKE_REPORT_TEMPLATE=$(make_template failed failed application-exited)

# Two different refusals belong to two different gates. A closed evidence file
# that never saw a probe worker is incomplete: its shape is fine, so the
# qualification gate has to say so. An evidence file whose observation count was
# removed contradicts its own sample counts, so the closed-schema gate refuses
# it first and nothing is presented as qualifying.
broken="$public_dir/broken-evidence.json"
jq '.startup.probeWorkers = 0' "$evidence" > "$broken"
check 1 'no probe workers' run_contract qualify --evidence "$broken"
expect_contains 'no probe workers' 'status=evidence-incomplete'
jq '.qualification.nativeInventoryWired = false' "$evidence" > "$broken"
check 1 'unwired inventory' run_contract qualify --evidence "$broken"
expect_contains 'unwired inventory' 'status=evidence-incomplete'
jq '.startup.observations = 0' "$evidence" > "$broken"
check 1 'zero observations' run_contract qualify --evidence "$broken"
expect_contains 'zero observations' 'not closed'
[[ -s "$broken" ]] || fail 'qualification must not rewrite the file it reads'
jq '.classification = "guessed-cause"' "$evidence" > "$broken"
check 1 'invented classification' run_contract qualify --evidence "$broken"
jq '.identity.nativeHelperSha256 = "not-a-digest"' "$evidence" > "$broken"
check 1 'malformed helper digest' run_contract qualify --evidence "$broken"
check 1 'absent evidence' run_contract qualify --evidence "$public_dir/none-two.json"
# An unknown key is refused wherever it is nested, and a value that merely looks
# like an identifier is refused where the field's own domain is a number. These
# are the shapes a forged or drifted bundle arrives in; "safe-looking" is not a
# schema, so each one has to fail before anything is published or qualified.
jq '.startup.inventoryUsableExtra = 1' "$evidence" > "$broken"
check 1 'unknown nested key' run_contract qualify --evidence "$broken"
jq '.probes[0].extra = "passed"' "$evidence" > "$broken"
check 1 'unknown probe key' run_contract qualify --evidence "$broken"
jq '.classification = "window-discovery"' "$evidence" > "$broken"
check 1 'classification ahead of its own evidence' run_contract qualify --evidence "$broken"
jq '.startup.elapsedMilliseconds = "3600000"' "$evidence" > "$broken"
check 1 'string where a duration belongs' run_contract qualify --evidence "$broken"
jq '.environment.launcherKind = "elf-ish"' "$evidence" > "$broken"
check 1 'string outside a closed enum' run_contract qualify --evidence "$broken"
# The namespace switches keep their own contradiction rules: the policy file
# answers 0, 1 or "absent or unreadable" (-1), and the key must be present.
jq '.environment.apparmorUsernsRestriction = 2' "$evidence" > "$broken"
check 1 'namespace switch outside its three answers' run_contract qualify \
    --evidence "$broken"
jq 'del(.environment.apparmorUsernsRestriction)' "$evidence" > "$broken"
check 1 'bundle without the namespace switch' run_contract qualify \
    --evidence "$broken"

# An unreadable inventory is unknown, and unknown never carries counts: the two
# directions of that rule are separate claims, so each is refused apart from the
# other. A window count that exceeds the windows it was found among is the same
# class of contradiction.
jq '.inventory.session = {usable: -1, windows: 3, appWindows: 3,
    appTestableWindows: 3, foregroundIsApp: -1, exit: 5}' "$evidence" > "$broken"
check 1 'unknown inventory with counts' run_contract qualify --evidence "$broken"
jq '.inventory.session.appWindows = (.inventory.session.windows + 1)' "$evidence" > "$broken"
check 1 'contradictory window counts' run_contract qualify --evidence "$broken"

# Episode counts live under the same contradiction rules: an episode is cut
# from samples, so it cannot exceed them, cannot exist without them, and a
# bundle that predates them (schema version 1) is not re-validated by a
# version-2 gate.
jq '.startup.appProcessEpisodes = (.startup.appProcessSamples + 1)' "$evidence" \
    > "$broken"
check 1 'episodes beyond their samples' run_contract qualify --evidence "$broken"
jq '.startup.appProcessSamples = 2' "$evidence" > "$broken"
check 1 'samples without any episode' run_contract qualify --evidence "$broken"
jq '.schemaVersion = 1' "$evidence" > "$broken"
check 1 'version-1 bundle against the version-2 gate' run_contract qualify \
    --evidence "$broken"
jq '.schemaVersion = 3' "$evidence" > "$broken"
check 1 'unknown evidence version' run_contract qualify --evidence "$broken"

# Same rule on the way in: a receipt or report with one key this contract never
# writes is not the artifact it claims to be, whatever the extra value says.
leaky_receipt="$public_dir/leaky-receipt.json"
jq '.apps[0].escalated = true' "$receipt" > "$leaky_receipt"
check 1 'unknown receipt key' run_contract prepare-assert --receipt "$leaky_receipt"
leaky_report="$public_dir/leaky-input-report.json"
jq '.results[0].live.reasonOverride = "passed"' \
    "$(make_template forged failed application-exited)" > "$leaky_report"
check 1 'forged probe field evidence' run_contract evidence \
    --report "$leaky_report" --out "$public_dir/forged-evidence.json" \
    --receipt "$receipt"
[[ ! -e "$public_dir/forged-evidence.json" ]] ||
    fail 'a report with an invented field produced evidence'

# A public artifact must not be redirected into the private directory, and a
# flag cannot be repeated to move it there after the first answer was given.
check 1 'report inside the private directory' run_contract run --receipt "$receipt" \
    --report "$diagnose_dir/report.json"
check 1 'repeated report flag' run_contract run --receipt "$receipt" \
    --report "$report" --report "$diagnose_dir/report.json"
check 1 'unknown flag' run_contract run --receipt "$receipt" --report "$report" \
    --upload-raw-names yes


# ---------------------------------------------------------------------------
# The workflow's own invariant audit gets the same treatment as the shell
# contract: the embedded Python is extracted from the YAML and run against the
# real file, then against synthetic copies that inject one expression GitHub
# cannot resolve into each position it scans. A leak that passes this audit is
# the difference between one rejected push and two, and this experiment only
# ever gets one run. Positions without an expression of their own under test
# (an action input) stay covered by the real file alone. The audit is skipped
# when no python3 with a YAML parser exists, and reported loudly when it does.
# ---------------------------------------------------------------------------
workflow_yml=".github/workflows/desktop-check-chatgpt-wave10.yml"
if command -v python3 > /dev/null 2>&1 && python3 -c 'import yaml' > /dev/null 2>&1; then
    python3 - "$workflow_yml" "$workspace/audit.py" <<'PY'
import pathlib, re, sys, textwrap
text = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
match = re.search(r"python3 - <<'PY'\n(.*?)\n[ ]{10}PY\n", text, re.S)
assert match, "the invariant audit step is missing from the workflow"
audit = textwrap.dedent(match.group(1))
audit = audit.replace(
    'pathlib.Path(".github/workflows/desktop-check-chatgpt-wave10.yml")',
    'pathlib.Path(sys.argv[1])')
pathlib.Path(sys.argv[2]).write_text(audit)
PY
    check 0 'audit extracts from the workflow' python3 "$workspace/audit.py" \
        "$workflow_yml"
    audit_variant() {
        local name=$1 old=$2 new=$3
        local file="$workspace/audit-$name.yml"
        python3 - "$workflow_yml" "$file" "$old" "$new" <<'PY'
import pathlib, sys
text = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
path = pathlib.Path(sys.argv[2])
path.write_text(text.replace(sys.argv[3], sys.argv[4]))
assert path.read_text(encoding="utf-8") != text, sys.argv[2]
PY
        check 1 "audit refuses $name" python3 "$workspace/audit.py" "$file"
    }
    audit_variant 'a runner context in the workflow env' \
        '  DIAG_DIRECTORY: desktop-chatgpt-wave10-diagnostics' \
        '  DIAG_DIRECTORY: ${{ runner.temp }}/desktop-chatgpt-wave10-diagnostics'
    audit_variant 'an env context in the concurrency group' \
        'desktop-chatgpt-wave10-${{ github.ref }}' \
        'desktop-chatgpt-wave10-${{ env.DIAG_DIRECTORY }}'
    audit_variant 'a runner context in a job env block' \
        'jobs:
  linux-x64:
    name: ChatGPT Desktop (linux-x64 startup diagnostic)' \
        'jobs:
  linux-x64:
    name: ChatGPT Desktop (linux-x64 startup diagnostic)
    env:
      DIAG_DIR: ${{ runner.temp }}/x'
    audit_variant 'a steps context in the workflow env' \
        '  APP: chatgpt-desktop' \
        '  APP: ${{ steps.evidence.outcome }}'
else
    printf 'note: the workflow audit regression was skipped (no python3+yaml)\n' >&2
fi

printf 'chatgpt wave10 startup contracts passed (%s checks)\n' "$pass_count"
