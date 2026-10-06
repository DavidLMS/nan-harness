#!/usr/bin/env bash
# Temporary wave-10 contract for the ChatGPT Linux startup-diagnostic probe.
#
# Wave 9 established that the checker reports `application-exited` for all three
# deterministic probes of the official amd64 package. That reason means only
# that the launcher the checker spawned was gone before a test window became
# available; it does not say who exited, whether a detached descendant survived,
# whether any window existed, or whether the environment refused the request.
# This contract observes that boundary instead of repeating the same conclusion.
#
# Public output discipline (the whole point of the exercise):
#   * every `status=` line, and everything inside evidence.json, is a fixed
#     closed classification, bounded count, version, run id or SHA-256 digest;
#     both channels are checked against an explicit key allowlist and a value
#     domain before anything is uploaded, because a private value can easily be
#     made to look like an ordinary identifier;
#   * the native `--windows` inventory is read privately and reduced to counts
#     by matching owner names, case-insensitively, against the checker's own
#     public application-name list, so no process name, window field, path,
#     argument, environment entry or native message is ever echoed or packaged;
#   * an inventory that cannot be parsed is published as unknown (-1), never as
#     a zero count, and the same rule applies to every unavailable host fact;
#   * process observations are attributed by ancestry from the launch this
#     script owns. A name that merely looks like the application is recorded as
#     an unattributed observation and never as a cause;
#   * the checker's stderr goes to a private file with a closed exit status;
#     only whitelisted closed patterns cross into the evidence.
#
# Periodic sampling has a detection limit: an interval with no observation is
# not proof that nothing existed between two ticks, so every count here is an
# observation count and nothing claims a process never existed.
#
# Nothing here repairs a startup: no sandbox flag, no helper creation, no
# privilege, no system or AppArmor policy change, no maintainer script, no
# package or product edit, and no credential.
set -euo pipefail
# Private observations still describe local process and window state, so every
# file this script writes is owner-only (SECURITY.md: covered files 0600 and
# private directories 0700; `need_diag` enforces the directory mode).
umask 077

SELF=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &&
    printf '%s/%s' "$PWD" "$(basename -- "${BASH_SOURCE[0]}")")

CHECKER="${CHECKER:-target/debug/nanh-desktop-check}"
NANH="${NANH:-target/debug/nan-harness}"
SESSION="${SESSION_SCRIPT:-scripts/run-desktop-check-session.sh}"
HELPER="${NATIVE_HELPER:-}"
DIAG="${DIAG_DIR:-}"
APP="chatgpt-desktop"
SAMPLE_INTERVAL_MS="${SAMPLE_INTERVAL_MS:-250}"
SAMPLE_MAX_TICKS="${SAMPLE_MAX_TICKS:-800}"
# Bounded sampling window: whichever of the tick count and the wall clock comes
# first ends the observation, so a slow inventory cannot run away with it.
SAMPLE_BUDGET_MS="${SAMPLE_BUDGET_MS:-180000}"
# Owner names the shipped checker accepts for this app. `gui.rs` keeps
# app_names(ChatGpt) and `matches_app()` compares with eq_ignore_ascii_case
# after stripping a trailing ".exe", so an owner belongs to this application
# when its bytes equal any casing of a public name -- not merely the three
# casings this script could enumerate by hand. The whole rule therefore lives
# in one awk matcher that folds case the same way, shared by the window
# inventory and the process table. Nothing decoded is ever printed.
app_names_public() { printf '%s\n' ChatGPT Codex; }

# Lower-cased public names as one space-separated argument for awk.
app_name_lowers() { app_names_public | tr 'A-Z' 'a-z' | awk '{ printf "%s ", $1 }'; }

fail() {
    printf 'wave10: %s\n' "$1" >&2
    exit 1
}

need() {
    local label=$1 value=$2
    [[ -n "$value" ]] || fail "$label is required"
}

# Every published count is either a real observation or unknown, never a silent
# zero, and every bound below is the value the shipped helper or the checker
# itself refuses to exceed. -1 is the only value that means "not observed".
UNKNOWN=-1
MAX_WINDOWS=1024
MAX_DISPLAYS=32
MAX_PROCESS_ID=4294967295
MAX_PROCESSES=4096
MAX_DURATION_MS=3600000

# The one matcher shared by every observation. `NAMES` holds the lower-cased
# public application names; a value matches the application when it equals one
# of them case-insensitively without a trailing `.exe`, exactly as
# `matches_app()` does, and is package-named when it merely starts with one.
# A native owner field arrives hex encoded: `hex_ok` mirrors the helper's own
# name decoder (even-length hex, at most 512 hex characters, or the empty "-"), and
# `hex_ascii` folds it to comparable bytes. Nothing decoded is ever printed.
AWK_MATCH=$(cat <<'AWK'
BEGIN { DIGITS = "0123456789abcdef"; known_count = split(NAMES, known, " ") }
function matches_exact(raw,    value, slot) {
    value = tolower(raw)
    sub(/\.exe$/, "", value)
    for (slot = 1; slot <= known_count; slot += 1)
        if (value == known[slot]) return 1
    return 0
}
function matches_named(raw,    value, slot) {
    value = tolower(raw)
    for (slot = 1; slot <= known_count; slot += 1)
        if (substr(value, 1, length(known[slot])) == known[slot]) return 1
    return 0
}
function hex_ok(value) {
    if (value == "-") return 1
    if (value == "" || length(value) > 512) return 0
    if (length(value) % 2 == 1 || value !~ /^[0-9a-f]+$/) return 0
    return 1
}
function hex_digit(character) { return index(DIGITS, character) - 1 }
function hex_ascii(value,    out, position, code, high, low, digits) {
    if (value == "-") return ""
    value = tolower(value)
    out = ""
    for (position = 1; position <= length(value); position += 2) {
        high = hex_digit(substr(value, position, 1))
        low = hex_digit(substr(value, position + 1, 1))
        code = high * 16 + low
        if (code < 33 || code > 126) code = 63
        out = out sprintf("%c", code)
    }
    return out
}
AWK
)

# One closed domain definition, shared by the report gate, the evidence gate and
# the qualification gate. A value is publishable only when it is inside a named
# domain, so no string is accepted just because it looks harmless: a path, a
# window title, a process name or an argument cannot survive these checks, and
# neither can a key this contract never wrote.
JQ_CLOSED=$(cat <<'JQ'
def within($set): . as $value | ($set | index($value)) != null;
# Set difference rather than a scan with `has`: inside `all($set[]; has($key))`
# the dot is the key itself, and asking a string for a key is an error, not a
# false answer. These three define a closed object: no key outside the set, no
# required key missing, and for the exact form, nothing extra either.
def keys_within($set): type == "object" and (((keys_unsorted // []) - $set) | length == 0);
def required_keys($set): type == "object" and (($set - (keys_unsorted // [])) | length == 0);
def keys_exact($set): type == "object" and keys_within($set) and required_keys($set);
def integer($lo; $hi): type == "number" and (. == floor) and (. >= $lo) and (. <= $hi);
def count($max): integer(0; $max);
def maybe_count($max): integer(-1; $max);
def digest: type == "string" and test("^[0-9a-f]{64}$");
def revision: type == "string" and test("^[0-9a-f]{40}$");
def run_identifier: type == "string" and test("^[0-9a-f]{32}$");
def semver: type == "string" and test("^[0-9]+(\\.[0-9A-Za-z-]+){1,6}$");
def timestamp:
    type == "string" and
    test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\\.[0-9]{1,9})?(Z|[+-][0-9]{2}:[0-9]{2})$");
# The closed enums are the shipped report types, spelled exactly as the checker
# serialises them. Anything outside them was not produced by that code path.
def status_values: ["passed", "failed", "blocked", "skipped"];
def reason_values: [
    "missing-key", "invalid-key", "missing-model", "installation-unavailable",
    "installation-failed", "installation-ambiguous", "installation-unreadable",
    "version-unknown", "unsupported-version", "unsupported-architecture",
    "already-running", "permission-required", "login-required", "timeout",
    "isolation-unavailable", "focus-changed", "window-changed", "window-occluded",
    "desktop-unavailable", "application-exited", "selector-not-matched",
    "action-unsupported", "input-mismatch", "response-mismatch", "tool-mismatch",
    "provider-failed", "budget-exceeded", "cancelled", "cleanup-conflict",
    "cleanup-failed", "not-run"
];
def step_values: ["launched", "input-submitted", "response-verified", "tool-verified",
    "error-recovered"];
def stage_values: ["trust-dialog-discovery", "trust-dialog-action",
    "trust-dialog-dismissal", "agent-panel", "composer-input", "composer-send"];
def input_mode_values: ["accessibility", "accessibility-and-keyboard",
    "visual-and-keyboard"];
def verification_values: ["accessibility", "local-ocr"];
def classification_values: ["detached-descendant", "window-discovery",
    "window-undersized",
    "environment-libraries", "environment-display", "launcher-exited-before-app",
    "app-named-process-without-main", "app-exited-before-window", "inconclusive"];
# One probe, as the shipped serialiser writes it: the required fields are always
# present, an optional one is either absent or a known value, and a passed probe
# carries no reason while any other outcome must carry one.
def probe:
    required_keys(["status", "steps", "durationMilliseconds"]) and
    keys_within(["status", "reason", "steps", "inputMode", "guiStage",
        "responseVerification", "durationMilliseconds"]) and
    (.status | within(status_values)) and
    (.steps | type == "array" and (length <= 5) and (all(.[]; within(step_values))) and
        ((unique | length) == length)) and
    (.durationMilliseconds | count(3600000)) and
    ((.status == "passed") == (.reason == null)) and
    ((.reason == null) or (.reason | within(reason_values))) and
    ((.inputMode == null) or (.inputMode | within(input_mode_values))) and
    ((.guiStage == null) or (.guiStage | within(stage_values))) and
    ((.responseVerification == null) or
        (.responseVerification | within(verification_values)));
# One window inventory reduction: counts first, then their own consistency.
def inventory_record:
    keys_exact(["usable", "windows", "appWindows", "appTestableWindows",
        "foregroundIsApp", "exit"]) and
    (.usable | within([1, -1])) and
    (.windows | maybe_count(1024)) and
    (.appWindows | maybe_count(1024)) and
    (.appTestableWindows | maybe_count(1024)) and
    (.foregroundIsApp | within([-1, 0, 1])) and
    (.exit | integer(0; 255)) and
    ((.usable == -1) == (.windows == -1)) and
    ((.usable == -1) == (.appWindows == -1)) and
    ((.usable == -1) == (.appTestableWindows == -1)) and
    ((.usable == -1) == (.foregroundIsApp == -1)) and
    (.appWindows <= .windows) and (.appTestableWindows <= .appWindows);
# The classification is a function of the published numbers and nothing else, so
# a run cannot report a cause its own evidence does not show. An attributed
# window is only "discovered" at the size the shipped checker will drive; a
# smaller one is its own closed state rather than a claim the evidence
# contradicts.
def startup_classification:
    (.startup.survivors > 0) as $detached |
    (.startup.appTestableWindowSamples > 0) as $testable |
    (.startup.appWindowSamples > 0) as $windowed |
    ((.environment.unresolvedLibraries > 0) or (.environment.unresolvedRuntimeLibraries > 0))
        as $library_gap |
    ((.startup.observations > 0) and (.startup.helperFailures == .startup.observations))
        as $display_gap |
    (.launcher.launcherExits > 0) as $launched |
    (.startup.appNamedProcessSamples > 0) as $named |
    if $detached then "detached-descendant"
    elif $testable then "window-discovery"
    elif $windowed then "window-undersized"
    elif $library_gap then "environment-libraries"
    elif $display_gap then "environment-display"
    elif ($launched | not) then "inconclusive"
    elif .startup.appProcessSamples == 0 then
        (if $named then "app-named-process-without-main" else "launcher-exited-before-app" end)
    else "app-exited-before-window" end;
JQ
)

# The process table, the coreutils deadline and the dynamic loader report are
# the only three external tools this contract cannot replace with its own
# logic, so all three are named here. The synthetic contracts point them at
# fixtures; a hosted run uses the real ones.
PS_COMMAND="${PS_COMMAND:-ps}"
TIMEOUT_BIN="${TIMEOUT_BIN:-$(command -v timeout || true)}"
LDD_COMMAND="${LDD_COMMAND:-ldd}"
# One inventory must not hold the observation window open, so every ask carries
# this wall-clock deadline; both bounds are refused unless they are bounded
# numbers, because an unbounded observation is not a diagnostic.
INVENTORY_DEADLINE="${INVENTORY_DEADLINE:-20s}"
[[ "$INVENTORY_DEADLINE" =~ ^[1-9][0-9]{0,2}s$ ]] ||
    fail 'INVENTORY_DEADLINE must be a bounded duration in seconds'
[[ "$SAMPLE_BUDGET_MS" =~ ^[0-9]+$ && "$SAMPLE_BUDGET_MS" -le "$MAX_DURATION_MS" ]] ||
    fail 'SAMPLE_BUDGET_MS must be a bounded duration in milliseconds'

# A failed table is not an empty table: the caller must know the difference, so
# the exit status is returned rather than swallowed. Row shape is checked where
# the rows are read.
process_table() {
    local output='' status=0
    output=$("$PS_COMMAND" -eo pid=,ppid=,pgid=,stat=,comm= 2> /dev/null) || status=$?
    [[ -z "$output" ]] || printf '%s\n' "$output"
    return "$status"
}

# A deadline is a failure boundary, not a nicety: without `timeout` a hung
# inventory would hold the whole observation window, so the caller treats the
# missing tool as an unusable observation instead of running unbounded.
with_timeout() {
    local deadline=$1
    shift
    [[ -n "$TIMEOUT_BIN" ]] || return 127
    "$TIMEOUT_BIN" "$deadline" "$@"
}

# bash 3.2 has no sub-second arithmetic, so the tick delay is rendered by hand.
interval_sleep_arg() {
    local milliseconds=$SAMPLE_INTERVAL_MS whole fraction
    [[ "$milliseconds" =~ ^[0-9]+$ ]] || fail 'SAMPLE_INTERVAL_MS must be numeric'
    whole=$((milliseconds / 1000))
    fraction=$((milliseconds % 1000))
    printf '%d.%03d\n' "$whole" "$fraction"
}

# Tick spacing is nominal, not guaranteed: one inventory can take longer than
# the interval, so the elapsed column has to say when something was really
# seen. /proc/uptime is monotonic and centisecond-precise on Linux; a host
# without it advances by the nominal interval instead. The value is assigned to
# CLOCK_MS by the caller's shell, because a subshell would lose the fallback.
CLOCK_MS=0
read_clock() {
    local seconds centiseconds
    if [[ -r /proc/uptime ]] && read -r seconds _ < /proc/uptime; then
        centiseconds=${seconds/./}
        if [[ "$centiseconds" =~ ^[0-9]+$ ]]; then
            CLOCK_MS=$((centiseconds * 10))
            return
        fi
    fi
    CLOCK_MS=$((CLOCK_MS + SAMPLE_INTERVAL_MS))
}

digest_of() {
    local path=$1
    [[ -f "$path" ]] || fail "$2 is missing"
    if command -v sha256sum > /dev/null 2>&1; then
        sha256sum -- "$path" | cut -c1-64
    else
        shasum -a 256 -- "$path" | cut -c1-64
    fi
}

is_digest() {
    [[ "$1" =~ ^[0-9a-f]{64}$ ]]
}

# SECURITY.md makes a diagnostic directory private data (0700), and every fact
# this script writes is private until an allowlisted reduction says otherwise.
# A caller that forgot `install -d -m 700` must not turn this contract into a
# world-readable credential-adjacent drop, so the mode is checked here instead
# of assumed. GNU and BSD `stat` disagree on flags, so both are attempted.
directory_mode() {
    local mode
    mode=$(stat -c '%a' -- "$1" 2> /dev/null) || mode=''
    [[ -n "$mode" ]] || mode=$(stat -f '%Lp' -- "$1" 2> /dev/null) || mode=''
    printf '%s' "$mode"
}

need_diag() {
    [[ -n "$DIAG" && -d "$DIAG" ]] || fail 'DIAG_DIR must be an existing private directory'
    case "$(directory_mode "$DIAG")" in
        700 | 0700) ;;
        *) fail 'DIAG_DIR is not an owner-only (0700) directory' ;;
    esac
}

write_fact() {
    need_diag
    printf '%s\n' "$2" > "$DIAG/$1"
}

read_fact() {
    local file="$DIAG/$1" value=''
    if [[ -s "$file" ]]; then
        # Keep single inner separators; drop only the trailing newline.
        value=$(tr -d '\r\n' < "$file") || true
    fi
    printf '%s' "$value"
}

# One argument loop shared by every subcommand: `--flag value` pairs only, each
# flag at most once. A repeated flag used to be resolved by "last one wins",
# which lets `--report /tmp/a --report /tmp/b` quietly redirect a public
# artifact; an unknown flag used to be accepted and then ignored.
collect_options() {
    option_names=()
    option_values=()
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --*)
                [[ $# -ge 2 ]] || fail "$1 needs a value"
                local wanted=${1#--} index=0
                while [[ $index -lt ${#option_names[@]} ]]; do
                    [[ "${option_names[$index]}" == "$wanted" ]] &&
                        fail "$1 may be given only once"
                    index=$((index + 1))
                done
                option_names+=("${1#--}")
                option_values+=("$2")
                shift 2
                ;;
            *) fail "unexpected argument $1" ;;
        esac
    done
}

# Each command states the options it understands; anything else is refused
# before it can influence a path, a digest or an upload gate.
only_options() {
    local index=0 name known matched
    while [[ $index -lt ${#option_names[@]} ]]; do
        name=${option_names[$index]}
        matched=0
        for known in "$@"; do
            [[ "$name" == "$known" ]] && matched=1
        done
        [[ $matched == 1 ]] || fail "this command does not accept --${name}"
        index=$((index + 1))
    done
}

# Public artifacts (the checker report, the evidence file) must never be
# written inside the private observation directory, which also keeps the
# workflow's "the diagnostic directory is never an upload path" gate honest.
need_public_path() {
    local label=$1 path=$2 parent real_diag
    need "$label" "$path"
    [[ "$path" != */ ]] || fail "$label must name a file, not a directory"
    [[ "${path//$'\n'/}" == "$path" ]] || fail "$label must not contain a newline"
    parent=${path%/*}
    [[ "$parent" != "$path" ]] || parent=.
    [[ -d "$parent" ]] || fail "$label must live in an existing directory"
    parent=$(cd -P -- "$parent" && pwd -P) || fail "$label parent is unreadable"
    real_diag=$(cd -P -- "$DIAG" && pwd -P) || fail 'the private directory is unreadable'
    case "$parent" in
        "$real_diag" | "$real_diag"/*)
            fail "$label may not be written inside the private directory" ;;
    esac
}

option() {
    local wanted=$1 index=0 found=''
    while [[ $index -lt ${#option_names[@]} ]]; do
        if [[ "${option_names[$index]}" == "$wanted" ]]; then
            found=${option_values[$index]}
        fi
        index=$((index + 1))
    done
    printf '%s' "$found"
}

# ---------------------------------------------------------------------------
# assert_closed_receipt <path>: the private preparation receipt, read with the
# closed shape the checker itself serialises. Two digests from it are republished
# as evidence, so a receipt carrying a field this contract never heard of is
# refused rather than passed through.
# ---------------------------------------------------------------------------
assert_closed_receipt() {
    local file=$1
    [[ -s "$file" ]] || return 1
    [[ $(wc -c < "$file") -le 16384 ]] || return 1
    jq -e --arg app "$APP" "$JQ_CLOSED"'
        required_keys(["schemaVersion", "runId", "platform", "architecture", "checker",
            "nanh", "nanhIdentity", "apps"]) and
        keys_exact(["schemaVersion", "runId", "platform", "architecture", "checker",
            "nanh", "nanhIdentity", "apps"]) and
        (.schemaVersion | within([1])) and
        (.runId | run_identifier) and
        (.platform == "linux") and (.architecture == "x86_64") and
        (.checker | keys_exact(["path", "sha256"]) and (.path | type == "string") and
            (.sha256 | digest)) and
        (.nanh | keys_exact(["path", "sha256"]) and (.path | type == "string") and
            (.sha256 | digest)) and
        (.nanhIdentity | keys_exact(["version", "sha256"]) and (.version | semver) and
            (.sha256 | digest)) and
        (.apps | type == "array" and length == 1) and
        all(.apps[];
            required_keys(["app", "executable"]) and
            keys_within(["app", "executable", "appVersion", "runtimeVersion"]) and
            (.app == $app) and
            (.executable | keys_exact(["path", "sha256"]) and
                (.path | type == "string") and (.sha256 | digest)) and
            ((.appVersion == null) or (.appVersion | semver)) and
            ((.runtimeVersion == null) or (.runtimeVersion | semver)))
    ' "$file" > /dev/null
}

# ---------------------------------------------------------------------------
# prepare-assert: the receipt must identify the package's direct GUI binary.
# ---------------------------------------------------------------------------
cmd_prepare_assert() {
    collect_options "$@"
    only_options receipt
    local receipt
    receipt=$(option receipt)
    need --receipt "$receipt"
    [[ -s "$receipt" ]] || fail 'the preparation receipt is empty'
    assert_closed_receipt "$receipt" ||
        fail 'the preparation receipt is not a closed document'
    local executable
    executable=$(jq -er '.apps[0].executable.path' "$receipt")

    # The official package ships one GUI executable at this in-run relative path.
    [[ "$executable" == */usr/lib/chatgpt/ChatGPT ]] ||
        fail 'the prepared executable is not the package GUI executable'
    # The launcher alias, the bundled runtime and the resources root are each a
    # different binary or tree; none of them may be mistaken for the GUI.
    case "${executable##*/}" in
        ChatGPT) ;;
        *) fail 'the prepared executable basename is not the GUI binary' ;;
    esac
    local version
    version=$(jq -r '.apps[0].appVersion // "unknown"' "$receipt")
    printf 'status=prepared app=%s version=%s\n' "$APP" "$version"
}

# ---------------------------------------------------------------------------
# helper-assert: the native Linux inventory helper must be the one Cargo built
# and embedded, identified by digest. This step runs outside any X session, so
# it deliberately claims nothing about windows: "does the inventory answer in
# the session" is `session-preflight`, inside the same session as the probe.
# ---------------------------------------------------------------------------
cmd_helper_assert() {
    collect_options "$@"
    only_options
    need NATIVE_HELPER "$HELPER"
    need_diag
    [[ -x "$HELPER" ]] || fail 'the native helper is missing or not executable'
    local helper_digest
    helper_digest=$(digest_of "$HELPER" 'native helper')
    is_digest "$helper_digest" || fail 'the native helper digest is malformed'
    write_fact helper-digest "$helper_digest"
    "$HELPER" --version > /dev/null 2>&1 ||
        fail 'the native helper did not report a version'
    printf 'status=helper digest=%s version-ok=true wired=true\n' "$helper_digest"
}

# ---------------------------------------------------------------------------
# session-preflight: inside the graphical session that will own the probe, the
# helper must be the same binary the workflow asserted by digest and must answer
# with a structurally valid inventory. A successful exit with an empty or
# malformed snapshot is a failure here, not a valid zero.
# ---------------------------------------------------------------------------
cmd_session_preflight() {
    collect_options "$@"
    only_options
    need NATIVE_HELPER "$HELPER"
    need_diag
    [[ -s "$DIAG/helper-digest" ]] || fail 'the helper was never asserted by digest'
    local asserted now
    asserted=$(read_fact helper-digest)
    is_digest "$asserted" || fail 'the asserted helper digest is malformed'
    now=$(digest_of "$HELPER" 'native helper')
    [[ "$now" == "$asserted" ]] ||
        fail 'the helper answering in session is not the helper the run asserted'
    run_inventory session
    inventory_usable session ||
        fail 'the native inventory did not answer structurally in session'
    printf 'status=preflight digest=%s session-inventory=usable\n' "$asserted"
}

# ---------------------------------------------------------------------------
# run_inventory <context>: ask the native helper for the window inventory and
# keep only counts. An unusable snapshot keeps the exit code as a fact and turns
# every count into -1 (unknown), because "the helper answered with something we
# could not parse" is not evidence of an empty desktop.
# ---------------------------------------------------------------------------
run_inventory() {
    local context=$1 raw='' exit_code=0
    need_diag
    [[ -n "$context" && "$context" != */* ]] || fail 'inventory context is invalid'
    [[ -x "$HELPER" ]] || fail 'the native helper is missing or not executable'
    raw=$(with_timeout "$INVENTORY_DEADLINE" "$HELPER" --windows 2> /dev/null) ||
        exit_code=$?
    write_fact "inventory-$context-exit" "$exit_code"
    reduce_inventory > "$DIAG/inventory-$context" <<< "$raw"
}

# True when the reduced inventory for <context> is a real observation.
inventory_usable() {
    local file="$DIAG/inventory-$1"
    [[ -s "$file" ]] || return 1
    [[ "$(cut -d' ' -f1 < "$file")" == 0 ]]
}

# Reduce a native inventory on stdin to five integers: usability (0 = parsed,
# -1 = not), viewable windows, windows whose owner name is the one this
# application is attributed by, the subset at least as large as the checker's
# own minimum test window, and whether the foreground owner is one of those
# processes. Unknown is never reported as zero, because "the helper answered
# with something we could not read" is not evidence of an empty desktop.
#
# The shape rules are the shipped parser's own grammar (`Snapshot::parse`), not
# a looser reading of it: a leading `FG` record, at least one `DISPLAY` and at
# most 32, at most 1024 `WIN` records, only `DISPLAY`/`WIN` records after the
# first, integer identifiers and process ids inside the helper's own width,
# geometry in the fixed three-decimal form the helper prints (bounded to 65536
# with a positive extent), and an owner field that is `-` or at most 512 hex
# characters. A blank line, a truncated record or an out-of-range field
# invalidates the whole snapshot instead of being skipped, so an exit-zero
# reply that is not a complete inventory cannot be mistaken for a valid one.
# UTF-8 validity of a decoded owner name is the one rule left to the checker
# itself, which validates the report that carries this evidence anyway; a name
# it rejects can only fail to match, never to be attributed.
#
# The native FG record is "FG <pid> <window id>", and on X11 the active window
# can be a child the root-level enumeration never lists, so the foreground is
# attributed by owner pid. Owner identity is matched case-insensitively and
# never printed.
reduce_inventory() {
    awk -v NAMES="$(app_name_lowers)" -v MAXW="$MAX_WINDOWS" \
        -v MAXD="$MAX_DISPLAYS" -v MAXPID="$MAX_PROCESS_ID" "$AWK_MATCH"'
        function whole(value) { return value ~ /^[0-9]{1,20}$/ }
        function process_id(value) { return whole(value) && value + 0 <= MAXPID }
        # The helper prints every geometry field with three decimals; the
        # shipped parser reads them as real numbers, so both spellings of a
        # bounded value are accepted and nothing else is.
        function coordinate(value) {
            return value ~ /^-?[0-9]{1,6}(\.[0-9]{1,3})?$/ && value + 0 >= -65536 &&
                value + 0 <= 65536
        }
        function extent(value) { return coordinate(value) && value + 0 > 0 }
        records >= 2048 { broken = 1; exit }
        { records += 1 }
        records == 1 {
            if (NF != 3 || $1 != "FG" || !process_id($2) || !whole($3)) broken = 1
            else foreground_owner = $2
            next
        }
        NF == 0 { broken = 1; exit }
        $1 == "DISPLAY" {
            if (NF != 5 || !coordinate($2) || !coordinate($3) || !extent($4) || !extent($5))
                broken = 1
            else displays += 1
            if (displays > MAXD) broken = 1
            next
        }
        $1 == "WIN" {
            if (NF != 8 || !whole($2) || !process_id($3) || !coordinate($4) ||
                !coordinate($5) ||
                !extent($6) || !extent($7) || !hex_ok($8)) { broken = 1; next }
            windows += 1
            if (windows > MAXW) { broken = 1; next }
            if (!matches_exact(hex_ascii($8))) next
            app_windows += 1
            if ($6 + 0 >= 300 && $7 + 0 >= 200) app_testable += 1
            if (foreground_owner != "" && foreground_owner != "0" &&
                $3 == foreground_owner) foreground_app = 1
            next
        }
        { broken = 1 }
        END {
            if (broken || records == 0 || displays == 0) {
                printf "%d %d %d %d %d\n", -1, -1, -1, -1, -1
                exit
            }
            printf "0 %d %d %d %d\n", windows, app_windows, app_testable,
                foreground_app
        }
    '
}

# ---------------------------------------------------------------------------
# Environment facts, collected before any app work and expressed as closed
# values only. `library` counts unresolved dynamic dependencies by name count,
# never by name; `namespace` reads two kernel switches without touching them.
# ---------------------------------------------------------------------------
cmd_environment() {
    collect_options "$@"
    only_options receipt
    local receipt
    receipt=$(option receipt)
    need --receipt "$receipt"
    need_diag
    local executable
    executable=$(jq -er '.apps[0].executable.path' "$receipt")
    [[ -f "$executable" ]] || fail 'the prepared executable is not on disk'
    # `ldd` treats a leading dash as an option and glibc's `ldd` rejects `--`,
    # so the path is required to be absolute and is then passed as a bare word.
    # The preparation receipt always records an in-run absolute path.
    [[ "$executable" == /* ]] || fail 'the prepared executable path is not absolute'
    local app_root
    app_root=$(dirname -- "$executable")
    # Package-layout facts the shipped Linux launcher depends on, as booleans.
    [[ -f "$app_root/resources/codex" ]] && runtime_file=yes || runtime_file=no
    write_fact fact-runtime-file "$runtime_file"
    [[ -d "$app_root/resources" ]] && resources_dir=yes || resources_dir=no
    write_fact fact-resources-dir "$resources_dir"
    # Unresolved dynamic dependencies: count only, names stay on the runner.
    # A failed loader report is unknown (-1) for both binaries, never a silent
    # zero: "the loader could not tell us" must not read as "the loader saw no
    # gap", because the library classification is decided by these integers.
    local missing_gui=-1 missing_runtime=-1 loader_output
    if loader_output=$("$LDD_COMMAND" "$executable" 2> /dev/null); then
        missing_gui=$(printf '%s\n' "$loader_output" | awk '/not found/ { n++ } END { print n + 0 }')
    fi
    if loader_output=$("$LDD_COMMAND" "$app_root/resources/codex" 2> /dev/null); then
        missing_runtime=$(printf '%s\n' "$loader_output" | awk '/not found/ { n++ } END { print n + 0 }')
    fi
    write_fact fact-libraries "$missing_gui $missing_runtime"
    # Read-only kernel switches that a sandboxed renderer depends on.
    local userns=-1
    if [[ -r /proc/sys/kernel/unprivileged_userns_clone ]]; then
        userns=$(tr -d ' \n' < /proc/sys/kernel/unprivileged_userns_clone)
        [[ "$userns" == 0 || "$userns" == 1 ]] || userns=-1
    fi
    local userns_range=max
    if [[ -r /proc/sys/user/max_user_namespaces ]]; then
        if [[ $(tr -d ' \n' < /proc/sys/user/max_user_namespaces) == 0 ]]; then
            userns_range=zero
        fi
    fi
    # The third switch is the one this runner's distribution actually gates on:
    # Ubuntu 24.04 confines unprivileged user-namespace creation with an AppArmor
    # policy, and `unprivileged_userns_clone=1` does not answer that question.
    # Wave 10 never read it, so a measured "userns enabled, no sibling helper"
    # could still not separate a sandbox failure from any other early exit.
    # Read-only again: this run changes no policy and opens no profile.
    # -1 means the file is absent or unreadable, which is its own closed answer.
    local apparmor_userns=-1
    if [[ -r /proc/sys/kernel/apparmor_restrict_unprivileged_userns ]]; then
        apparmor_userns=$(
            tr -d ' \n' < /proc/sys/kernel/apparmor_restrict_unprivileged_userns
        )
        [[ "$apparmor_userns" == 0 || "$apparmor_userns" == 1 ]] ||
            apparmor_userns=-1
    fi
    write_fact fact-namespace "$userns $userns_range $apparmor_userns"
    # An owned setuid helper is a layout fact, never a fix and never an
    # assumption here: this run changes nothing about it.
    local helper_bit=na
    # The mere presence of a sibling file is a layout fact: this run neither
    # creates it, changes its mode nor reads its contents.
    if [[ -e "$app_root/chrome-sandbox" ]]; then helper_bit=present; else helper_bit=absent; fi
    write_fact fact-sandbox-sibling "$helper_bit"
    # What the checker actually spawns, by leading bytes only: a native image
    # and a script wrapper fail in different ways, and wave 9 could not tell
    # them apart. The magic is reduced to one of three closed words, so no
    # interpreter line, argument or file content is ever recorded.
    local executable_kind=other
    case "$(head -c 4 -- "$executable" | od -An -tx1 | tr -d ' \n')" in
        7f454c46) executable_kind=elf ;;
        2321*) executable_kind=script ;;
    esac
    write_fact fact-executable-kind "$executable_kind"
    printf 'status=environment libraries=%s namespace=%s executable=%s\n' \
        "$(read_fact fact-libraries)" "$(read_fact fact-namespace)" "$executable_kind"
}

# ---------------------------------------------------------------------------
# source: record the exact public identity of the tree being measured. Only a
# commit object id and the workflow's own ref name are kept, both already
# public on the run page; nothing derived from a working copy is printed.
# ---------------------------------------------------------------------------
cmd_source() {
    need_diag
    local revision=${1:-}
    [[ -n "$revision" && "$revision" =~ ^[0-9a-f]{40}$ ]] ||
        fail 'a full source commit id is required'
    local branch=${GITHUB_REF_NAME:-}
    [[ "$branch" =~ ^[A-Za-z0-9._/-]{1,100}$ ]] || branch=unknown
    write_fact fact-source "$revision $branch"
    printf 'status=source revision=%s branch=%s\n' "$revision" "$branch"
}

# ---------------------------------------------------------------------------
# run: one owned lifecycle. Everything app-facing happens inside
# session-run so the sampler, the checker and the follow-up inventories all
# observe the same disposable X11 session.
# ---------------------------------------------------------------------------
cmd_run() {
    collect_options "$@"
    only_options receipt report
    local receipt report
    receipt=$(option receipt)
    report=$(option report)
    need --receipt "$receipt"
    need --report "$report"
    need_public_path --report "$report"
    [[ -s "$receipt" ]] || fail 'the preparation receipt is missing'
    [[ -x "$CHECKER" ]] || fail 'the source-built checker is not executable'
    [[ -f "$SESSION" ]] || fail 'the graphical session script is missing'
    need_diag
    [[ -s "$DIAG/helper-digest" ]] || fail 'the native helper digest was never asserted'
    [[ -s "$DIAG/fact-libraries" ]] || fail 'environment facts were never collected'
    local status=0
    bash "$SESSION" bash "$SELF" session-run --receipt "$receipt" --report "$report" ||
        status=$?
    # A session that never reached the sampler leaves no timeline at all. Say
    # so, because "no observations" is a different failure from "no window".
    [[ -s "$DIAG/timeline.csv" ]] ||
        fail "the graphical session recorded no observations (exit=$status)"
    # Reductions are pure file work and belong outside the display session, so
    # they still happen when the session itself ends badly.
    reduce_survivors
    reduce_timeline
    classify_stderr
    [[ -s "$report" ]] || fail "probe exit=$status recorded no canonical report to preserve"
    # The counts printed here are the counts the evidence file will publish, so
    # the run log cannot claim a window the inventory never showed: the readable
    # tick count and the testable-window subset are named apart from the rest.
    printf 'status=probed exit=%s probes=%s observations=%s app-process-samples=%s app-window-samples=%s survivors=%s app-named-process-samples=%s app-testable-window-samples=%s inventory-readable-samples=%s app-process-episodes=%s app-named-process-episodes=%s\n' \
        "$status" \
        "$(jq -r '.results[0].deterministic | length' "$report")" \
        "$(jq -r '.observations' "$DIAG/fact-timeline")" \
        "$(jq -r '.appProcessSamples' "$DIAG/fact-timeline")" \
        "$(jq -r '.appWindowSamples' "$DIAG/fact-timeline")" \
        "$(jq -r '.survivors' "$DIAG/fact-survivors")" \
        "$(jq -r '.appNamedProcessSamples' "$DIAG/fact-timeline")" \
        "$(jq -r '.appTestableWindowSamples' "$DIAG/fact-timeline")" \
        "$(jq -r '.inventoryUsableSamples' "$DIAG/fact-timeline")" \
        "$(jq -r '.appProcessEpisodes' "$DIAG/fact-timeline")" \
        "$(jq -r '.appNamedProcessEpisodes' "$DIAG/fact-timeline")"
    return "$status"
}

# Internal half of `run`, executed inside the graphical session.
cmd_session_run() {
    collect_options "$@"
    only_options receipt report
    local receipt report
    receipt=$(option receipt)
    report=$(option report)
    need --receipt "$receipt"
    need --report "$report"
    need_public_path --report "$report"
    # The inventory is only this attempt's evidence if the binary answering
    # inside the session is the one the run asserted by digest outside it, so
    # the two are compared here before either is asked anything.
    [[ -s "$DIAG/helper-digest" ]] || fail 'the native helper digest was never asserted'
    local asserted now
    asserted=$(read_fact helper-digest)
    is_digest "$asserted" || fail 'the asserted helper digest is malformed'
    now=$(digest_of "$HELPER" 'native helper')
    [[ "$now" == "$asserted" ]] ||
        fail 'the helper available in session is not the helper the run asserted'
    # Ask the inventory for real inside the session the application is about to
    # get, before the attempt starts. "No window was ever discovered" only means
    # something if the observer can discover windows in this session at all, and
    # a reply that is empty or malformed is recorded as unknown, never as zero.
    run_inventory session
    local stderr_file="$DIAG/checker-stderr.log"
    : > "$stderr_file"
    # The checker's own summary names the on-disk report path, so stdout is not
    # forwarded: the closed evidence already carries the validated report digest.
    "$CHECKER" run --yes --non-interactive --ephemeral \
        --mode deterministic --app "$APP" --prepared "$receipt" --output "$report" \
        > /dev/null 2> "$stderr_file" &
    local checker_pid=$! status=0
    sample_lifecycle "$checker_pid"
    wait "$checker_pid" || status=$?
    printf '%s\n' "$status" > "$DIAG/run-status"
    # Wave 9 never asked whether a window existed after the failure. Ask now,
    # three times, inside the same session that owned the attempt.
    local index=0
    while [[ $index -lt 3 ]]; do
        run_inventory "post$index"
        index=$((index + 1))
        sleep 0.2
    done
    return "$status"
}

# ---------------------------------------------------------------------------
# sample_lifecycle <checker pid>: one bounded observation window. Each tick
# takes exactly one process table and one native inventory, and keeps only
# integers. The checker's own probe workers are found by parent id and command
# name; their process groups are what later decides "detached" versus "owned".
# ---------------------------------------------------------------------------
sample_lifecycle() {
    local checker_pid=$1 tick=0
    local worker_pgids='' worker_count=0
    local worker_comm
    worker_comm=$(basename -- "$CHECKER")
    worker_comm=${worker_comm:0:15}
    local sleep_arg
    sleep_arg=$(interval_sleep_arg)
    : > "$DIAG/timeline.csv"
    read_clock
    local clock_start=$CLOCK_MS elapsed_ms
    while [[ $tick -lt $SAMPLE_MAX_TICKS ]]; do
        local snapshot row app_count group_count alive zombie tree_count new_pgids
        # An unreadable table is not an empty one: the window ends here with the
        # ticks already taken, instead of recording processes that were never seen.
        snapshot=$(process_table) || break
        row=$(printf '%s\n' "$snapshot" | awk \
            -v parent="$checker_pid" -v comm="$worker_comm" -v pgids="$worker_pgids" \
            -v NAMES="$(app_name_lowers)" "$AWK_MATCH"'
            # "pid ppid pgid stat comm", and a command name may contain spaces,
            # so everything after the fourth column is the name.
            function name_of(    column, value) {
                value = $5
                for (column = 6; column <= NF; column += 1) value = value " " $column
                return value
            }
            BEGIN {
                wanted_count = split(pgids, wanted, " ")
                for (slot = 1; slot <= wanted_count; slot += 1) wanted_group[wanted[slot]] = 1
            }
            $4 ~ /^Z/ { if ($1 == parent) zombie = 1; next }
            $1 == parent { alive = 1 }
            matches_exact(name_of()) { app += 1 }
            matches_named(name_of()) { tree += 1 }
            ($3 in wanted_group) { group += 1 }
            name_of() == comm && $2 == parent { fresh[$3] = 1 }
            END {
                joined = ""
                for (key in fresh) joined = joined " " key
                printf "%d %d %d %d %d%s\n", app + 0, group + 0, alive + 0, zombie + 0,
                    tree + 0, joined
            }
        ')
        read -r app_count group_count alive zombie tree_count new_pgids <<< "$row"
        local candidate
        for candidate in $new_pgids; do
            case " $worker_pgids " in
                *" $candidate "*) ;;
                *)
                    worker_pgids="$worker_pgids $candidate"
                    worker_count=$((worker_count + 1))
                    ;;
            esac
        done
        local raw='' helper_exit=0 inventory
        raw=$(with_timeout "$INVENTORY_DEADLINE" "$HELPER" --windows 2> /dev/null) ||
            helper_exit=$?
        inventory=$(printf '%s\n' "$raw" | reduce_inventory)
        # Columns: elapsed ms, application processes, probe-group processes,
        # viewable windows, application windows, native inventory exit status,
        # processes named after the application in any form, the application
        # windows big enough for the checker to drive, and whether this tick's
        # inventory was readable at all. A -1 count means the tick produced no
        # readable inventory, which is never recorded as a tick with zero
        # windows.
        read_clock
        elapsed_ms=$((CLOCK_MS - clock_start))
        printf '%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
            "$elapsed_ms" "$app_count" "$group_count" \
            "$(printf '%s' "$inventory" | cut -d' ' -f2)" \
            "$(printf '%s' "$inventory" | cut -d' ' -f3)" "$helper_exit" \
            "$tree_count" \
            "$(printf '%s' "$inventory" | cut -d' ' -f4)" \
            "$(printf '%s' "$inventory" | cut -d' ' -f1)" >> "$DIAG/timeline.csv"
        printf '%s\n' "$worker_pgids" > "$DIAG/fact-worker-pgids"
        [[ "$alive" == 1 && "$zombie" == 0 ]] || break
        # The tick count alone is not a bound: one slow inventory can stretch the
        # window far past its nominal length, so the wall clock ends it too.
        [[ $elapsed_ms -lt "$SAMPLE_BUDGET_MS" ]] || break
        sleep "$sleep_arg"
        tick=$((tick + 1))
    done
    printf '%s\n' "$worker_pgids" > "$DIAG/fact-worker-pgids"
    printf '%s\n' "$worker_count" > "$DIAG/fact-worker-count"
}

# Turn the private tick table into one closed JSON object of integers. Every
# window column keeps -1 when no tick produced a readable inventory: an
# inventory that never answered is not an inventory that saw zero windows.
#
# An episode is a maximal run of consecutive ticks with a positive count.
# Episodes carry no probe identity: several episodes can happen inside one
# probe, because a replacement process restarting after each exit begins a
# new run, and one episode can absorb several starts, because a replacement
# appearing on the next positive tick keeps the run unbroken. An episode
# count is therefore a lower bound on separately observable application
# lifetimes inside the sampled window and nothing more: it cannot show that
# the packaged binary started on every attempt, it cannot name a faulty
# launcher when it is small, and it is not a count of probe launches. The
# detection limit stays what it always was: an app that started and exited
# wholly between two ticks is no episode for anyone, and a sampled interval
# is only a sampled interval.
reduce_timeline() {
    local timeline="$DIAG/timeline.csv"
    [[ -s "$timeline" ]] || printf '{}\n' > "$DIAG/fact-timeline"
    awk -F, '
        BEGIN { app_first = -1; window_first = -1; window_max = -1;
            app_window_max = -1; app_testable_max = -1 }
        {
            observations += 1
            elapsed = $1
            if ($2 > app_max) app_max = $2
            if ($3 > group_max) group_max = $3
            # Column 9 is the readability of this tick, so a tick whose
            # inventory could not be read contributes no window evidence.
            if ($9 == 0) {
                usable += 1
                if ($4 > window_max) window_max = $4
                if ($5 > app_window_max) app_window_max = $5
                if ($8 > app_testable_max) app_testable_max = $8
                if ($5 > 0) { app_window_samples += 1; if (window_first < 0)
                    window_first = $1 }
                if ($8 > 0) app_testable_samples += 1
            }
            if ($2 > 0) {
                app_samples += 1
                if (app_first < 0) app_first = $1
                if (!in_app) { app_episodes += 1; in_app = 1 }
            } else in_app = 0
            if ($6 != 0) helper_failures += 1
            if ($7 > tree_max) tree_max = $7
            if ($7 > 0) {
                tree_samples += 1
                if (!in_tree) { tree_episodes += 1; in_tree = 1 }
            } else in_tree = 0
        }
        END {
            printf "{\"observations\":%d,\"appProcessSamples\":%d,\"appProcessMax\":%d,"\
                   "\"groupProcessMax\":%d,\"inventoryUsableSamples\":%d,"\
                   "\"windowMax\":%d,\"appWindowSamples\":%d,"\
                   "\"appWindowMax\":%d,\"helperFailures\":%d,"\
                   "\"appNamedProcessSamples\":%d,\"appNamedProcessMax\":%d,"\
                   "\"appTestableWindowSamples\":%d,\"appTestableWindowMax\":%d,"\
                   "\"appProcessFirstSampleMilliseconds\":%d," \
                   "\"appWindowFirstSampleMilliseconds\":%d," \
                   "\"appProcessEpisodes\":%d,\"appNamedProcessEpisodes\":%d," \
                   "\"elapsedMilliseconds\":%d}\n",
                observations, app_samples + 0, app_max + 0, group_max + 0, usable + 0,
                window_max, app_window_samples + 0, app_window_max + 0,
                helper_failures + 0, tree_samples + 0, tree_max + 0,
                app_testable_samples + 0, app_testable_max + 0,
                app_first + 0, window_first + 0, app_episodes + 0, tree_episodes + 0,
                elapsed + 0
        }
    ' "$timeline" > "$DIAG/fact-timeline"
    jq -e '.observations | type == "number"' "$DIAG/fact-timeline" > /dev/null ||
        fail 'the timeline did not reduce to closed numbers'
}

# Anything still alive after the checker stopped owning the attempt is a
# detached descendant, not a launcher exit. Owned means "same process group as
# one of the observed probe workers", which is the shipped checker's own rule.
reduce_survivors() {
    local worker_pgids table='' table_status=0
    worker_pgids=$(read_fact fact-worker-pgids)
    table=$(process_table) || table_status=$?
    if [[ $table_status -ne 0 ]]; then
        # A table that could not be produced leaves the survivor question
        # unanswered; -1 says so, where 0 would claim an absence nobody read.
        printf '{"survivors":-1,"survivorsOutsideProbeGroup":-1,"namedSurvivors":-1}\n' \
            > "$DIAG/fact-survivors"
        return 0
    fi
    printf '%s\n' "$table" |
        awk -v pgids="$worker_pgids" -v NAMES="$(app_name_lowers)" "$AWK_MATCH"'
            function name_of(    column, value) {
                value = $5
                for (column = 6; column <= NF; column += 1) value = value " " $column
                return value
            }
            BEGIN {
                count = split(pgids, wanted, " ")
                for (slot = 1; slot <= count; slot += 1) wanted_group[wanted[slot]] = 1
            }
            $4 ~ /^Z/ { next }
            matches_exact(name_of()) {
                survivors += 1
                if (!($3 in wanted_group)) outside += 1
            }
            matches_named(name_of()) { named += 1 }
            END {
                printf "{\"survivors\":%d,\"survivorsOutsideProbeGroup\":%d,"\
                       "\"namedSurvivors\":%d}\n",
                    survivors + 0, outside + 0, named + 0
            }
        ' > "$DIAG/fact-survivors"
    jq -e '.survivors | type == "number"' "$DIAG/fact-survivors" > /dev/null ||
        fail 'the survivor sweep did not reduce to closed numbers'
}

# The checker's own diagnostic channel accepts closed enums only; this reads
# that channel and nothing else. Any line outside the whitelist contributes a
# count, never content, so raw native output cannot reach the evidence.
classify_stderr() {
    local file="$DIAG/checker-stderr.log"
    [[ -f "$file" ]] || fail 'the private checker output was never recorded'
    awk '
        function code(raw,    value) {
            # A closed numeric exit status: an exit code as-is, a signal as its
            # negative, and "Unknown" counted separately instead of invented.
            if (raw ~ /^Code\(-?[0-9]+\)$/) {
                value = substr(raw, 6, length(raw) - 6)
                return value
            }
            if (raw ~ /^Signal\([0-9]+\)$/) {
                value = substr(raw, 8, length(raw) - 8)
                return "-" value
            }
            return ""
        }
        /^Desktop launch diagnostic: / {
            launches += 1
            raw = substr($0, index($0, ": ") + 2)
            value = code(raw)
            if (value == "") unknown += 1
            else if (listed < 8) { listed += 1; codes[listed] = value }
            matched = 1
        }
        /^Desktop cleanup diagnostic: / { cleanups += 1; matched = 1 }
        /^Desktop seal diagnostic: / { seals += 1; matched = 1 }
        /^Desktop worker diagnostic: / { workers += 1; matched = 1 }
        {
            if (NF == 0) next
            if (matched) { matched = 0; next }
            unmatched += 1
        }
        END {
            printf "{\"launcherExits\":%d,\"launcherExitCodes\":[", launches + 0
            for (slot = 1; slot <= listed; slot += 1)
                printf "%s%s", (slot > 1 ? "," : ""), codes[slot]
            printf "],\"launcherExitsUnknown\":%d,\"cleanupDiagnostics\":%d,"\
                   "\"sealDiagnostics\":%d,\"workerDiagnostics\":%d,"\
                   "\"unmatchedPrivateLines\":%d}\n",
                unknown + 0, cleanups + 0, seals + 0, workers + 0, unmatched + 0
        }
    ' "$file" > "$DIAG/fact-stderr"
    # The raw capture is private data and has served its purpose once the closed
    # counts exist, so it never outlives this command.
    rm -f -- "$file"
    jq -e '.launcherExits | type == "number"' "$DIAG/fact-stderr" > /dev/null ||
        fail 'the private diagnostics did not reduce to closed numbers'
}

# A reduced fact file feeds a public field, so it may only hold JSON numbers
# that are whole: a string, fraction, boolean or null here would mean a private
# observation reached a channel that is allowed to be public.
require_closed_fact() {
    local name=$1 file="$DIAG/$name"
    [[ -s "$file" ]] || fail "the closed fact $name is missing"
    # Objects, arrays and integers only: a string, fraction or null here would
    # mean a private value leaked into a channel that is allowed to be public.
    jq -e '
        ([.. | strings] | length == 0) and
        ([.. | nulls] | length == 0) and
        ([.. | booleans] | length == 0) and
        ([.. | numbers] | all(. == floor))
    ' "$file" > /dev/null || fail "the closed fact $name holds a value that is not an integer"
}

# The public report is the one foreign document this contract reads, and the
# checker's validator is only as strict as the schema it ships: an unknown key
# beside a known one is refused there, but a value that merely looks ordinary is
# not what worries a publisher. This is the closed shape this contract will
# accept, key by key, including the three deterministic probes, the live probe
# and the reason and stage enums the shipped serialiser can write.
assert_closed_report() {
    local file=$1
    [[ -s "$file" ]] || return 1
    # The shipped reader refuses a report larger than 48 KiB.
    [[ $(wc -c < "$file") -le 49152 ]] || return 1
    jq -e --arg app "$APP" "$JQ_CLOSED"'
        required_keys(["schemaVersion", "checkerVersion", "runId", "startedAt",
            "platform", "architecture", "results", "cleanup"]) and
        keys_within(["schemaVersion", "checkerVersion", "runId", "startedAt",
            "platform", "architecture", "nanHarness", "results", "cleanup"]) and
        (.schemaVersion | within([1, 2])) and
        (.checkerVersion | semver) and
        (.runId | run_identifier) and
        (.startedAt | timestamp) and
        (.platform == "linux") and
        (.architecture == "x86_64") and
        (.cleanup | within(status_values)) and
        ((has("nanHarness") | not) or
            (.nanHarness | keys_exact(["version", "sha256"]) and
                (.version | semver) and (.sha256 | digest))) and
        (.results | type == "array" and length == 1) and
        all(.results[];
            required_keys(["app", "deterministic", "live", "cleanup"]) and
            keys_within(["app", "appVersion", "runtimeVersion", "deterministic",
                "live", "cleanup"]) and
            (.app == $app) and
            (.cleanup | within(status_values)) and
            ((has("appVersion") | not) or (.appVersion | semver)) and
            ((has("runtimeVersion") | not) or (.runtimeVersion | semver)) and
            (.deterministic | type == "array" and length == 3 and all(.[]; probe)) and
            (.live | probe))
    ' "$file" > /dev/null
}

# The evidence file is what leaves this machine, so its schema is exhaustive:
# every key must be one this contract wrote, every value must sit inside the
# domain its name claims, the numbers must agree with each other, and the
# classification must be the one the published observations produce. A string
# that merely looks like an identifier is not enough — none of these fields is
# allowed to hold a name, a path, an argument, a title or a message.
assert_closed_evidence() {
    local file=$1
    [[ -s "$file" ]] || return 1
    [[ $(wc -c < "$file") -le 16384 ]] || return 1
    jq -e --arg app "$APP" "$JQ_CLOSED"'
        required_keys(["schemaVersion", "app", "source", "identity", "probes",
            "startup", "inventory", "launcher", "environment", "cleanup",
            "qualification", "classification"]) and
        keys_exact(["schemaVersion", "app", "source", "identity", "probes",
            "startup", "inventory", "launcher", "environment", "cleanup",
            "qualification", "classification"]) and
        # Version 2 adds the two episode counts to `startup`; every version-1
        # field keeps its meaning. A version-1 bundle is not re-validated here:
        # this gate describes the file this contract publishes now.
        (.schemaVersion == 2) and
        (.app == $app) and
        (.source | keys_exact(["revision", "checkerVersion", "runId", "startedAt",
            "platform", "architecture"]) and
            (.revision | revision) and (.checkerVersion | semver) and
            (.runId | run_identifier) and (.startedAt | timestamp) and
            (.platform == "linux") and (.architecture == "x86_64")) and
        (.identity | keys_exact(["appVersion", "runtimeVersion", "executableSha256",
            "nanhSha256", "nativeHelperSha256", "reportSha256"]) and
            (.appVersion | semver) and (.runtimeVersion | semver) and
            (.executableSha256 | digest) and (.nanhSha256 | digest) and
            (.nativeHelperSha256 | digest) and (.reportSha256 | digest)) and
        (.probes | type == "array" and length == 3 and
            all(to_entries[];
                (.key + 1 == .value.index) and
                (.value | keys_exact(["index", "status", "reason", "guiStage",
                    "durationMilliseconds"]) and
                    (.index | integer(1; 3)) and
                    (.status | within(status_values)) and
                    ((.reason == null) or (.reason | within(reason_values))) and
                    ((.guiStage == null) or (.guiStage | within(stage_values))) and
                    (.durationMilliseconds | count(3600000))))) and
        (.startup | keys_exact(["observations", "appProcessSamples", "appProcessMax",
            "groupProcessMax", "inventoryUsableSamples", "windowMax",
            "appWindowSamples", "appWindowMax", "helperFailures",
            "appNamedProcessSamples", "appNamedProcessMax",
            "appTestableWindowSamples", "appTestableWindowMax",
            "appProcessFirstSampleMilliseconds", "appWindowFirstSampleMilliseconds",
            "appProcessEpisodes", "appNamedProcessEpisodes",
            "elapsedMilliseconds", "probeWorkers", "checkerExit", "survivors",
            "survivorsOutsideProbeGroup", "namedSurvivors"]) and
            (.observations | count(65535)) and (.appProcessSamples | count(65535)) and
            (.appProcessMax | count(65535)) and (.groupProcessMax | count(65535)) and
            (.inventoryUsableSamples | count(65535)) and (.windowMax | maybe_count(1024)) and
            (.appWindowSamples | count(65535)) and (.appWindowMax | maybe_count(1024)) and
            (.helperFailures | count(65535)) and
            (.appNamedProcessSamples | count(65535)) and
            (.appNamedProcessMax | count(65535)) and
            (.appTestableWindowSamples | count(65535)) and
            (.appTestableWindowMax | maybe_count(1024)) and
            (.appProcessFirstSampleMilliseconds | maybe_count(3600000)) and
            (.appWindowFirstSampleMilliseconds | maybe_count(3600000)) and
            (.appProcessEpisodes | count(65535)) and
            (.appNamedProcessEpisodes | count(65535)) and
            (.elapsedMilliseconds | count(3600000)) and (.probeWorkers | count(4096)) and
            # A survivor sweep that could not read the process table reports
            # unknown, and unknown is never quietly promoted to "none found".
            (.checkerExit | integer(-1; 255)) and (.survivors | maybe_count(4096)) and
            (.survivorsOutsideProbeGroup | maybe_count(4096)) and
            (.namedSurvivors | maybe_count(4096)) and
            (.appProcessSamples <= .observations) and
            (.appWindowSamples <= .observations) and
            (.appNamedProcessSamples <= .observations) and
            (.inventoryUsableSamples <= .observations) and
            (.helperFailures <= .observations) and
            (.appNamedProcessMax >= .appProcessMax) and
            # An episode is a run of samples, so it never exceeds the samples it
            # was cut from, and it exists exactly when its samples do.
            (.appProcessEpisodes <= .appProcessSamples) and
            (.appNamedProcessEpisodes <= .appNamedProcessSamples) and
            ((.appProcessSamples == 0) == (.appProcessEpisodes == 0)) and
            ((.appNamedProcessSamples == 0) == (.appNamedProcessEpisodes == 0)) and
            (.appTestableWindowSamples <= .appWindowSamples) and
            (.appTestableWindowMax <= .appWindowMax) and
            (.windowMax >= .appWindowMax or .windowMax == -1) and
            (.survivorsOutsideProbeGroup <= .survivors)) and
        (.inventory | keys_exact(["session", "afterRun"]) and
            (.session | inventory_record) and
            (.afterRun | type == "array" and length == 3 and all(.[]; inventory_record))) and
        (.launcher | keys_exact(["launcherExits", "launcherExitCodes",
            "launcherExitsUnknown", "cleanupDiagnostics", "sealDiagnostics",
            "workerDiagnostics", "unmatchedPrivateLines"]) and
            (.launcherExits | count(64)) and
            # The cross-field bound is written at the `.launcher` scope: once a
            # pipe reaches the array, `.` is the array and no sibling field is
            # reachable from inside it.
            (.launcherExitCodes | type == "array" and (length <= 8) and
                (all(.[]; integer(-255; 255)))) and
            ((.launcherExitCodes | length) <= .launcherExits) and
            (.launcherExitsUnknown | count(64)) and
            ((.launcherExitCodes | length) + .launcherExitsUnknown <= .launcherExits) and
            (.cleanupDiagnostics | count(1024)) and (.sealDiagnostics | count(1024)) and
            (.workerDiagnostics | count(1024)) and
            (.unmatchedPrivateLines | count(65535))) and
        (.environment | keys_exact(["unresolvedLibraries", "unresolvedRuntimeLibraries",
            "launcherKind", "unprivilegedUserns", "maxUserNamespacesZero",
            "apparmorUsernsRestriction",
            "packageSandboxSibling", "runtimeFilePresent", "resourcesDirPresent"]) and
            (.unresolvedLibraries | maybe_count(64)) and
            (.unresolvedRuntimeLibraries | maybe_count(64)) and
            (.launcherKind | within(["elf", "script", "other"])) and
            (.unprivilegedUserns | within([-1, 0, 1])) and
            (.maxUserNamespacesZero | within([0, 1])) and
            (.apparmorUsernsRestriction | within([-1, 0, 1])) and
            (.packageSandboxSibling | within([0, 1])) and
            (.runtimeFilePresent | within([0, 1])) and
            (.resourcesDirPresent | within([0, 1]))) and
        (.cleanup | keys_exact(["app", "report"]) and (.app | within(status_values)) and
            (.report | within(status_values))) and
        (.qualification | keys_exact(["nativeInventoryWired", "postRunInventoryAnswered",
            "probeWorkersObserved", "appProcessObserved", "appWindowObserved",
            "appTestableWindowObserved", "survivorObserved"]) and
            (all(.[]; type == "boolean"))) and
        (.classification | within(classification_values)) and
        (.classification == startup_classification)
    ' "$file" > /dev/null
}

# Fold the four reduced inventories into one integer-only JSON object.
build_inventories_fact() {
    local context file usable windows app_windows app_minimum foreground exit_code
    local lines=''
    for context in session post0 post1 post2; do
        file="$DIAG/inventory-$context"
        [[ -s "$file" ]] || fail "the reduced inventory for $context is missing"
        read -r usable windows app_windows app_minimum foreground < "$file"
        exit_code=$(read_fact "inventory-$context-exit")
        # Each published count is either a bounded observation or -1 (unknown),
        # and the helper exit is a bounded status; anything else is a bug in
        # this contract, not a value to publish.
        local field
        for field in "$usable" "$windows" "$app_windows" "$app_minimum" \
            "$foreground" "$exit_code"; do
            [[ "$field" =~ ^-?[0-9]{1,10}$ ]] ||
                fail "the reduced inventory for $context is not numeric"
        done
        [[ "$usable" == 0 || "$usable" == -1 ]] ||
            fail "the reduced inventory for $context has an unknown usability flag"
        if [[ "$usable" == -1 ]]; then
            # Unreadable means every count is unknown together, never a mix.
            [[ "$windows" == -1 && "$app_windows" == -1 && "$app_minimum" == -1 &&
                "$foreground" == -1 ]] ||
                fail "an unreadable inventory for $context still reported counts"
        elif [[ "$windows" =~ ^[0-9]+$ && "$app_windows" =~ ^[0-9]+$ &&
            "$app_minimum" =~ ^[0-9]+$ &&
            (("$foreground" == 0 || "$foreground" == 1)) ]]; then
            # Counts stay inside the observation they came from.
            [[ "$app_windows" -le "$windows" && "$app_minimum" -le "$app_windows" ]] ||
                fail "the inventory for $context contradicts itself"
            [[ "$windows" -le "$MAX_WINDOWS" ]] ||
                fail "the inventory for $context exceeds the helper's own bound"
        else
            fail "the inventory for $context reports counts outside its domain"
        fi
        [[ "$exit_code" =~ ^[0-9]{1,3}$ ]] ||
            fail "the inventory for $context reported an unbounded helper exit"
        lines+="$context $usable $windows $app_windows $app_minimum $foreground "
        lines+="$exit_code"$'\n'
    done
    # Keyed by the fixed context names; every value is an integer. `usable` is
    # 1 when the helper's reply was a complete inventory and -1 when it was not,
    # which is what lets a reader tell "no windows" from "no answer".
    printf '%s' "$lines" | jq -Rn '
        [inputs | split(" ") | {key: .[0], value: {
            "usable": (if (.[1] | tonumber) == 0 then 1 else -1 end),
            "windows": (.[2] | tonumber),
            "appWindows": (.[3] | tonumber),
            "appTestableWindows": (.[4] | tonumber),
            "foregroundIsApp": (.[5] | tonumber),
            "exit": (.[6] | tonumber)}}] | from_entries' > "$DIAG/fact-inventories"
    [[ -s "$DIAG/fact-inventories" ]] || fail 'the inventory facts did not reduce to JSON'
    require_closed_fact fact-inventories
}

# ---------------------------------------------------------------------------
# evidence: validate the canonical report first, then publish only closed
# startup classifications derived from the private observations.
# ---------------------------------------------------------------------------
cmd_evidence() {
    collect_options "$@"
    only_options report out receipt
    local report out receipt
    report=$(option report)
    out=$(option out)
    receipt=$(option receipt)
    need --report "$report"
    need --out "$out"
    need --receipt "$receipt"
    need_public_path --report "$report"
    need_public_path --out "$out"
    need_diag
    local digest=''
    # The validator's own status gates publication; a pipeline would mask it.
    if ! digest="$("$CHECKER" validate-report "$report" 2> /dev/null)"; then
        fail 'the checker rejected the report; nothing is published from it'
    fi
    digest=${digest##*$'\n'}
    is_digest "$digest" || fail 'validation returned no digest'
    # A validated report is still only as closed as the reader that checked it.
    # This is the gate that knows every key and value this contract may publish
    # from it, including the ones a validator with a wider schema would accept.
    assert_closed_report "$report" ||
        fail 'the validated report does not match the closed schema'
    assert_closed_receipt "$receipt" ||
        fail 'the preparation receipt does not match the closed schema'
    local name
    for name in fact-timeline fact-survivors fact-stderr fact-libraries fact-namespace \
        fact-source fact-sandbox-sibling fact-runtime-file fact-resources-dir \
        fact-executable-kind \
        fact-worker-count helper-digest run-status; do
        [[ -s "$DIAG/$name" ]] || fail "the private observation $name is missing"
    done
    require_closed_fact fact-timeline
    require_closed_fact fact-survivors
    require_closed_fact fact-stderr
    build_inventories_fact
    local helper_digest source_revision libraries namespace executable_kind
    helper_digest=$(read_fact helper-digest)
    is_digest "$helper_digest" || fail 'the native helper digest is malformed'
    source_revision=$(read_fact fact-source | cut -d' ' -f1)
    [[ "$source_revision" =~ ^[0-9a-f]{40}$ ]] || fail 'the source revision is malformed'
    # A private fact file feeds a public field, so each pair is re-checked as
    # the bounded numbers it claims to be instead of being trusted as text.
    libraries=$(read_fact fact-libraries)
    [[ "$libraries" =~ ^-?[0-9]{1,3}\ -?[0-9]{1,3}$ ]] ||
        fail 'the library counts are not a closed pair'
    namespace=$(read_fact fact-namespace)
    [[ "$namespace" =~ ^(-1|[01])\ (max|zero)\ (-1|[01])$ ]] ||
        fail 'the namespace facts are not closed'
    local workers checker_exit
    workers=$(read_fact fact-worker-count)
    [[ "$workers" =~ ^[0-9]{1,5}$ ]] || fail 'the probe worker count is not bounded'
    checker_exit=$(read_fact run-status)
    [[ "$checker_exit" =~ ^[0-9]{1,3}$ ]] || fail 'the checker exit is not a bounded status'
    # A free-form value may not enter the evidence even from a private file, so
    # the launcher kind is re-checked against the three words this contract can
    # ever write.
    executable_kind=$(read_fact fact-executable-kind)
    case "$executable_kind" in
        elf | script | other) ;;
        *) fail 'the launcher kind is not a closed classification' ;;
    esac
    # `$JQ_CLOSED` supplies the shared domain definitions, including the one
    # classification rule the gate below re-checks, so no caller can invent a
    # cause that its own published numbers do not show.
    local temporary="$out.tmp"
    if ! jq -n \
        --slurpfile report_json "$report" \
        --slurpfile receipt_json "$receipt" \
        --slurpfile timeline "$DIAG/fact-timeline" \
        --slurpfile survivors "$DIAG/fact-survivors" \
        --slurpfile launcher "$DIAG/fact-stderr" \
        --slurpfile inventories "$DIAG/fact-inventories" \
        --arg digest "$digest" --arg helper "$helper_digest" \
        --arg revision "$source_revision" --arg libraries "$libraries" \
        --arg namespace "$namespace" --arg sandbox "$(read_fact fact-sandbox-sibling)" \
        --arg executable_kind "$executable_kind" \
        --arg runtime_file "$(read_fact fact-runtime-file)" \
        --arg resources "$(read_fact fact-resources-dir)" \
        --arg workers "$workers" --arg checker_exit "$checker_exit" \
        "$JQ_CLOSED"'
        $report_json[0] as $report | $receipt_json[0] as $receipt |
        $timeline[0] as $tl | $survivors[0] as $sv | $launcher[0] as $se |
        $inventories[0] as $inv |
        ($libraries | split(" ")) as $libs |
        ($namespace | split(" ")) as $ns |
        {
            schemaVersion: 2,
            app: $report.results[0].app,
            source: {
                revision: $revision,
                checkerVersion: $report.checkerVersion,
                runId: $report.runId,
                startedAt: $report.startedAt,
                platform: $report.platform,
                architecture: $report.architecture
            },
            identity: {
                appVersion: ($report.results[0].appVersion // "unknown"),
                runtimeVersion: ($report.results[0].runtimeVersion // "unknown"),
                executableSha256: $receipt.apps[0].executable.sha256,
                nanhSha256: $receipt.nanh.sha256,
                nativeHelperSha256: $helper,
                reportSha256: $digest
            },
            probes: [ $report.results[0].deterministic | to_entries[] | {
                index: (.key + 1),
                status: .value.status,
                reason: (.value.reason // null),
                guiStage: (.value.guiStage // null),
                durationMilliseconds: .value.durationMilliseconds
            } ],
            startup: ($tl + {
                probeWorkers: ($workers | tonumber),
                checkerExit: ($checker_exit | tonumber),
                survivors: $sv.survivors,
                survivorsOutsideProbeGroup: $sv.survivorsOutsideProbeGroup,
                namedSurvivors: $sv.namedSurvivors
            }),
            inventory: {
                session: $inv.session,
                afterRun: [ $inv.post0, $inv.post1, $inv.post2 ]
            },
            launcher: $se,
            environment: {
                unresolvedLibraries: ($libs[0] | tonumber),
                unresolvedRuntimeLibraries: ($libs[1] | tonumber),
                launcherKind: $executable_kind,
                unprivilegedUserns: ($ns[0] | tonumber),
                maxUserNamespacesZero: (if $ns[1] == "zero" then 1 else 0 end),
                apparmorUsernsRestriction: ($ns[2] | tonumber),
                packageSandboxSibling: (if $sandbox == "present" then 1 else 0 end),
                runtimeFilePresent: (if $runtime_file == "yes" then 1 else 0 end),
                resourcesDirPresent: (if $resources == "yes" then 1 else 0 end)
            },
            cleanup: { app: $report.results[0].cleanup, report: $report.cleanup }
        } |
        # Both remaining fields are derived here, from the published numbers and
        # from nothing else. A process named after the application that is never
        # the exact name the checker attributes says the package did start
        # without proving a main process existed, and an unknown count (-1) is
        # never read as a zero: both distinctions live in the shared rule.
        .qualification = {
            nativeInventoryWired: (.inventory.session.usable == 1),
            postRunInventoryAnswered: (all(.inventory.afterRun[]; .usable == 1)),
            probeWorkersObserved: (.startup.probeWorkers > 0),
            appProcessObserved: (.startup.appProcessSamples > 0),
            appWindowObserved: (.startup.appWindowSamples > 0),
            appTestableWindowObserved: (.startup.appTestableWindowSamples > 0),
            survivorObserved: (.startup.survivors > 0)
        } | .classification = startup_classification
        ' > "$temporary"; then
        rm -f -- "$temporary"
        fail 'the closed startup evidence could not be built from the validated report'
    fi
    # An unclosable value must never reach an uploadable file, not even as a
    # leftover temporary artifact beside it.
    if ! assert_closed_evidence "$temporary"; then
        rm -f -- "$temporary"
        fail 'the built evidence was not closed; nothing is published'
    fi
    mv -f -- "$temporary" "$out"
    printf 'status=validated digest=%s classification=%s app-process-samples=%s app-window-samples=%s survivors=%s\n' \
        "$digest" \
        "$(jq -r '.classification' "$out")" \
        "$(jq -r '.startup.appProcessSamples' "$out")" \
        "$(jq -r '.startup.appWindowSamples' "$out")" \
        "$(jq -r '.startup.survivors' "$out")"
}

# ---------------------------------------------------------------------------
# qualify: this experiment is qualified by evidence completeness, not by an
# application pass. A failed startup with closed evidence is the intended
# result; an incomplete observation is not, and an application pass would be
# reported as a changed outcome rather than silently accepted.
# ---------------------------------------------------------------------------
cmd_qualify() {
    collect_options "$@"
    only_options evidence
    local evidence
    evidence=$(option evidence)
    need --evidence "$evidence"
    [[ -s "$evidence" ]] || fail 'no validated evidence is available to qualify'
    assert_closed_evidence "$evidence" || fail 'the evidence file is not closed'
    # Every gate is one boolean: a broken identity, a missing observation or an
    # invented classification all refuse to qualify, whatever their order.
    if jq -e '
        (.app == "chatgpt-desktop") and
        ((.probes | length) == 3) and
        ((.source.revision | test("^[0-9a-f]{40}$"))) and
        (([.identity.executableSha256, .identity.nanhSha256, .identity.nativeHelperSha256,
            .identity.reportSha256] | all(test("^[0-9a-f]{64}$")))) and
        (([.identity.appVersion, .identity.runtimeVersion] |
            all(test("^[0-9]+(\\.[0-9A-Za-z-]+){1,6}$")))) and
        ((.startup.observations | type) == "number") and
        (.startup.observations > 0) and
        (.startup.probeWorkers > 0) and
        (.launcher.launcherExits > 0) and
        (.qualification.nativeInventoryWired) and
        (.qualification.postRunInventoryAnswered) and
        # The qualification set is exactly the classification domain: a run that
        # observed an attributed window below the shipped 300x200 test minimum
        # produced complete evidence too, and must not be refused qualification
        # for a word its own builder can publish.
        ((.classification | IN("detached-descendant", "window-discovery",
            "window-undersized",
            "environment-libraries", "environment-display", "launcher-exited-before-app",
            "app-named-process-without-main", "app-exited-before-window", "inconclusive")))
    ' "$evidence" > /dev/null; then
        printf 'status=evidence-complete classification=%s app-outcome=%s probes=%s observations=%s\n' \
            "$(jq -r '.classification' "$evidence")" \
            "$(jq -r '[.probes[].status] | if all(. == "passed") then "passed"
                    elif all(. != "passed") then "all-failed" else "mixed" end' "$evidence")" \
            "$(jq -r '.probes | length' "$evidence")" \
            "$(jq -r '.startup.observations' "$evidence")"
    else
        printf 'status=evidence-incomplete classification=%s observations=%s workers=%s\n' \
            "$(jq -r '.classification // "none"' "$evidence")" \
            "$(jq -r '.startup.observations // -1' "$evidence")" \
            "$(jq -r '.startup.probeWorkers // -1' "$evidence")" >&2
        exit 1
    fi
}

usage() {
    printf '%s\n' \
        "usage: $0 prepare-assert --receipt PATH" \
        "   or: $0 helper-assert" \
        "   or: $0 session-preflight" \
        "   or: $0 environment --receipt PATH" \
        "   or: $0 source REVISION" \
        "   or: $0 run --receipt PATH --report PATH" \
        "   or: $0 evidence --report PATH --out PATH --receipt PATH" \
        "   or: $0 qualify --evidence PATH" >&2
    exit 2
}

command_name=${1:-}
[[ -n "$command_name" ]] || usage
shift
case "$command_name" in
    prepare-assert) cmd_prepare_assert "$@" ;;
    helper-assert) cmd_helper_assert "$@" ;;
    session-preflight) cmd_session_preflight "$@" ;;
    environment) cmd_environment "$@" ;;
    source) cmd_source "${1:-}" ;;
    run) cmd_run "$@" ;;
    session-run) cmd_session_run "$@" ;;
    evidence) cmd_evidence "$@" ;;
    qualify) cmd_qualify "$@" ;;
    *) usage ;;
esac
