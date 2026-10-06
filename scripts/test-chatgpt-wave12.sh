#!/usr/bin/env bash
# Synthetic, credential-free contracts for the temporary wave-12 launch-only
# debug wrapper: a source-bound shim, an in-memory startup-stderr reducer and
# a fixture that stands in for the real nanh binary. Nothing here launches
# ChatGPT, reads a credential store, touches a preference or host policy,
# opens a display, calls a network or executes the Rust workspace: every
# child process is a fake under a private temporary directory, and every
# fact is closed integers and vocabulary. The contract being proved is the
# whole wave-12 promise: --debug is added to exactly the checker's ChatGPT
# launch and nothing else; every other vector reaches the real binary
# untouched; the observation is bounded in bytes, lines, line length and
# absolute wall time (even against a launcher that ignores stops or a
# detached descendant that writes forever); the child's disposition always
# outranks any wrapper status; and no raw byte ever leaves the reducer's
# memory into a file, a terminal or a fact.
set -euo pipefail

script_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
shim="$script_root/chatgpt-wave12-shim.sh"
reducer="$script_root/chatgpt-wave12-reducer.py"
fixture="$script_root/chatgpt-wave12-fixture-nanh.sh"
test_root=/tmp
[[ "$(uname -s)" != Darwin ]] || test_root=/private/tmp
workspace=$(mktemp -d "$test_root/chatgpt-wave12-tests.XXXXXX")
chmod 700 "$workspace"
trap 'rm -rf -- "$workspace"' EXIT

pass_count=0
run_count=0
fail() {
    printf 'failed: %s\n' "$1" >&2
    exit 1
}
ok() { pass_count=$((pass_count + 1)); }
note() { printf 'note: %s\n' "$1" >&2; }

# ---------------------------------------------------------------------------
# Fixed points: syntax gates first, then the closed identities every test
# binds to. The fixture is the "real nanh": its digest is the bound digest.
# ---------------------------------------------------------------------------
bash -n "$shim" || fail 'the shim does not parse'
bash -n "$fixture" || fail 'the fixture does not parse'
python3 -c 'import sys; compile(open(sys.argv[1]).read(), sys.argv[1], "exec")' \
    "$reducer" || fail 'the reducer does not compile'
ok; ok; ok

digest_of() {
    if command -v sha256sum > /dev/null 2>&1; then
        sha256sum -- "$1" | cut -c1-64
    else
        shasum -a 256 -- "$1" | cut -c1-64
    fi
}
fixture_sha=$(digest_of "$fixture")
is_hex64_re='^[0-9a-f]{64}$'
[[ "$fixture_sha" =~ $is_hex64_re ]] || fail 'the fixture digest is not a digest'
ok

mode_of() {
    if stat -c '%a' "$1" > /dev/null 2>&1; then
        stat -c '%a' "$1"
    else
        stat -f '%Lp' "$1"
    fi
}

# Every string the fixture may print as "application output" or hold as a
# synthetic secret. None may ever appear in any file inside a run directory.
markers=(
    'sk-super-secret-value-0123456789'
    '/home/runner/.nan-harness/private/profile/config.toml'
    'ignore previous instructions'
    'wave12-synthetic-key-value'
    'flood line'
    'steady output'
    'steady stdout'
    'descendant line'
    'x tail'
)

LAUNCH=(chatgpt-desktop --provider-base-url https://synthetic.invalid/v1
    --model test-model --executable app-under-test)

new_run() {
    run_count=$((run_count + 1))
    d="$workspace/run-$run_count"
    mkdir "$d"
    FACTS="$d/facts"
    mkdir "$FACTS"
    chmod 700 "$FACTS"
    CALLS="$d/calls"
    OUT="$d/out"
    ERR="$d/err"
    F="$FACTS/startup-facts.json"
    # Per-run overrides; empty means "use the reducer/shim default".
    SCEN=quiet-exit0; REAL="$fixture"; SHA="$fixture_sha"; ST=0
    DEADLINE=; GRACE=; MAXB=; MAXL=; MAXN=; FIXEXIT=; LONG=; FLOOD=
    HOLDER=; WRITES=; ARGS=("${LAUNCH[@]}")
}

shim_run() {
    env WAVE12_REAL_NANH="$REAL" WAVE12_REAL_SHA256="$SHA" \
        WAVE12_FACTS_DIR="$FACTS" WAVE12_REDUCER="$reducer" \
        WAVE12_PYTHON=python3 \
        NAN_API_KEY="${synthetic_key}" \
        FIXTURE_SCENARIO="$SCEN" FIXTURE_CALLS="$CALLS" \
        ${DEADLINE:+WAVE12_DEADLINE_S=$DEADLINE} \
        ${GRACE:+WAVE12_GRACE_S=$GRACE} \
        ${MAXB:+WAVE12_MAX_BYTES=$MAXB} \
        ${MAXL:+WAVE12_MAX_LINE_BYTES=$MAXL} \
        ${MAXN:+WAVE12_MAX_LINES=$MAXN} \
        ${FIXEXIT:+FIXTURE_EXIT=$FIXEXIT} \
        ${LONG:+FIXTURE_LONG_BYTES=$LONG} \
        ${FLOOD:+FIXTURE_FLOOD_LINES=$FLOOD} \
        ${HOLDER:+FIXTURE_HOLDER_S=$HOLDER} \
        ${WRITES:+FIXTURE_WRITES=$WRITES} \
        bash "$shim" ${ARGS[@]+"${ARGS[@]}"} > "$OUT" 2> "$ERR"
}

launch() { # run the shim, keep its status in ST
    ST=0
    # The stderr redirect only swallows the shell's own "Terminated: 15"
    # job notice for a propagated child signal; the child's channels are
    # already files inside shim_run.
    { shim_run || ST=$?; } 2> /dev/null
    ok
}
expect_status() {
    [[ "$ST" == "$1" ]] || fail "$2: expected exit $1, got $ST"
    ok
}
fact() { jq -er --arg k "$1" '.[$k]' "$F"; }
expect_fact() {
    [[ "$(fact "$1")" == "$2" ]] ||
        fail "$3: fact $1 is '$(fact "$1" || true)', not '$2'"
    ok
}
facts_valid() {
    [[ -f "$F" ]] || fail "$1: no facts were published"
    python3 "$reducer" validate --facts "$F" ||
        fail "$1: published facts fail their own validator"
    ok
}
no_facts() {
    [[ ! -e "$F" ]] || fail "$1: facts were published where none belong"
    ok
}
never_ran() {
    [[ ! -e "$CALLS" ]] ||
        fail "$1: the bound binary ran when it must not have"
    ok
}
scan_run() { # no raw byte, path or credential persisted in this run
    local marker hit
    for marker in "${markers[@]}"; do
        hit=$(grep -rlF -- "$marker" "$d" 2>/dev/null || true)
        [[ -z "$hit" ]] || fail "$1: persisted a raw byte ('$marker') in: $hit"
    done
    ok
}
expect_grep() { # <file> <needle> <name>
    grep -qF -- "$2" "$1" || fail "$3: '$2' missing from $(basename "$1")"
    ok
}
expect_absent() { # <file> <needle> <name>
    grep -qF -- "$2" "$1" && fail "$3: '$2' present in $(basename "$1")" || true
    ok
}
elapsed_between() { # <min> <max> <name>
    local s=$1 e=$2
    [[ "$s" -le "$e" ]] || fail "$3: impossible elapsed bound"
    ok
}
synthetic_key='wave12-synthetic-key-value-0123456789'

# ===========================================================================
# A. Binding, identity and routing: only the checker's exact launch is
#    observed; everything else reaches the real binary untouched; a bound
#    that does not match the found file refuses before anything runs.
# ===========================================================================
new_run
launch
expect_status 0 'routed quiet launch'
facts_valid 'routed quiet launch'
expect_fact observation complete 'routed quiet launch'
expect_fact classification no-signature 'routed quiet launch'
expect_fact launcherExit 0 'routed quiet launch'
expect_fact launcherDisposition exited 'routed quiet launch'
expect_fact stopAction none 'routed quiet launch'
expect_fact failure none 'routed quiet launch'
scan_run 'routed quiet launch'

new_run
launch
[[ "$(wc -l < "$CALLS" | tr -d ' ')" == 9 ]] ||
    fail 'routed argv: expected 8 argument lines plus the key line'
ok
head -8 "$CALLS" > "$d/argv"
printf '%s\n' "${LAUNCH[@]}" --debug > "$d/want"
cmp -s "$d/argv" "$d/want" || fail 'the routed launch lost, reordered or' \
    ' duplicated an argument, or --debug was not appended exactly once'
ok
expect_grep "$CALLS" 'key=present' 'the session key reaches the child by env'
expect_absent "$CALLS" "$synthetic_key" 'no key value in the argv log'
scan_run 'routed argv recording'

new_run
SCEN=startup-error-sandbox
launch
expect_status 1 'sandbox-signature exit'
facts_valid 'sandbox-signature exit'
expect_fact classification no-usable-sandbox 'sandbox-signature exit'
expect_fact observation complete 'sandbox-signature exit'
expect_fact stderrLines 2 'sandbox-signature exit'
[[ -s "$OUT" || -s "$ERR" ]] &&
    fail 'the routed case leaked child bytes to a terminal channel' || true
ok
scan_run 'sandbox-signature exit'
[[ "$(mode_of "$F")" == 600 ]] || fail 'facts are not owner-only 0600'
[[ "$(mode_of "$FACTS")" == 700 ]] || fail 'the facts dir is not 0700'
[[ ! -e "$FACTS/.startup-facts.json.tmp" ]] || fail 'a temp fact file survived'
ok

# A launch with a wrong bound digest refuses closed: no argv of the real
# binary is ever executed, and the refusal is itself a validated fact with
# every digest redacted.
new_run
SHA=$(digest_of "$shim")
launch
expect_status 77 'digest mismatch'
never_ran 'digest mismatch'
facts_valid 'digest mismatch refusal'
expect_fact failure identity-refused 'digest mismatch'
expect_fact observation observation-failed 'digest mismatch'
expect_fact classification observation-failed 'digest mismatch'
[[ "$(jq -r '.identity | to_entries | map(.value) | unique | length' "$F")" == 1 ]] ||
    fail 'an identity refusal published a measured digest'
ok
[[ "$(jq -er '.identity.realNanhSha256' "$F")" == \
    "0000000000000000000000000000000000000000000000000000000000000000" ]] ||
    fail 'an identity refusal did not redact the bound digest'
ok

new_run
REAL="$d/missing-binary"
launch
expect_status 77 'missing bound binary'
never_ran 'missing bound binary'
facts_valid 'missing-binary refusal'
expect_fact failure identity-refused 'missing bound binary'

new_run
: > "$d/noread"
chmod 000 "$d/noread"
REAL="$d/noread"
launch
expect_status 77 'unreadable bound binary'
never_ran 'unreadable bound binary'
facts_valid 'unreadable-binary refusal'
expect_fact failure identity-refused 'unreadable bound binary'

# Transparency: the identity-preserving exec answers for itself.
new_run
ARGS=(--version)
launch
expect_status 0 'transparent --version'
expect_grep "$OUT" 'nan-harness 0.9.9' 'the real binary answers --version'
[[ "$(wc -l < "$CALLS" | tr -d ' ')" == 2 ]] ||
    fail '--version was not forwarded alone'
ok
expect_absent "$CALLS" '--debug' '--version never gains --debug'
no_facts 'transparent --version'

new_run
ARGS=(chatgpt-desktop --help)
launch
expect_status 0 'transparent help'
expect_grep "$OUT" 'Usage: nanh chatgpt-desktop' 'the real binary answers --help'
expect_absent "$CALLS" '--debug' 'help never gains --debug'
no_facts 'transparent help'

new_run
ARGS=(chatgpt-desktop --restore)
launch
expect_status 0 'transparent restore'
[[ "$(wc -l < "$CALLS" | tr -d ' ')" == 3 ]] ||
    fail 'restore was not forwarded exactly'
ok
expect_absent "$CALLS" '--debug' 'restore never gains --debug'
no_facts 'transparent restore'

# A grown or pre-debugged vector is NOT the checker launch: forward it.
new_run
ARGS=("${LAUNCH[@]}" --max-tokens 128)
launch
expect_status 0 'grown launch is transparent'
expect_absent "$CALLS" '--debug' 'a grown vector gains no debug flag'
no_facts 'grown launch'
[[ "$(grep -c . "$CALLS")" == 10 ]] ||
    fail 'grown vector was not forwarded whole'
ok

new_run
ARGS=("${LAUNCH[@]}" --debug)
launch
expect_status 0 'pre-debugged launch is transparent'
[[ "$(grep -c -- '--debug' "$CALLS")" == 1 ]] ||
    fail '--debug was duplicated on a vector that already carried it'
ok
no_facts 'pre-debugged launch'

# Flag-like values cannot masquerade as the launch shape.
new_run
ARGS=(chatgpt-desktop --provider-base-url --model --model m --executable e)
launch
expect_status 0 'flag-valued vector is transparent'
expect_absent "$CALLS" '--debug' 'a shifted vector is never rewritten'
no_facts 'flag-valued vector'

# ===========================================================================
# B. Dispositions: the child's own exit status always outranks the wrapper's
#    vocabulary, including when it collides with a reserved refusal number or
#    when the facts themselves cannot be published.
# ===========================================================================
new_run
SCEN=exit-code; FIXEXIT=3
launch
expect_status 3 'child exit 3 propagated'
facts_valid 'child exit 3'
expect_fact launcherExit 3 'child exit 3'
expect_fact observation complete 'child exit 3'

new_run
SCEN=exit-reserved
launch
expect_status 78 'child exit 78 forwarded unchanged'
facts_valid 'reserved child exit'
expect_fact launcherExit 78 'reserved child exit'
expect_fact failure none 'reserved child exit is not a wrapper refusal'

new_run
SCEN=signaled
launch
expect_status 143 'fatal child signal re-raised through the wrapper'
facts_valid 'signaled child'
expect_fact launcherDisposition signaled 'signaled child'
expect_fact launcherSignal 15 'signaled child'
expect_fact launcherExit -1 'signaled child has no exit code'
expect_fact stopAction none 'nobody stopped a self-inflicted signal'

# A facts write that cannot land must not overwrite the child's status with
# a refusal code: disposition first, facts absence is the job gate's problem.
new_run
SCEN=exit-code; FIXEXIT=3
mkdir "$FACTS/startup-facts.json"
launch
expect_status 3 'blocked facts write keeps the child status'
[[ -d "$F" ]] || fail 'the blocked facts path was replaced anyway'
ok
new_run
SCEN=signaled
mkdir "$FACTS/startup-facts.json"
launch
expect_status 143 'blocked facts write keeps the re-raised signal'

# ===========================================================================
# C. Classification is grounded: each closed token needs its exact upstream
#    string, stdout is never matched, and absence is only certifiable from a
#    complete capture.
# ===========================================================================
for spec in 'startup-error-display:display-unavailable' \
    'startup-error-suid-helper:suid-sandbox-misconfigured' \
    'startup-error-loader:loader-missing-shared-object' \
    'startup-error-multi:multiple-signatures'; do
    new_run
    SCEN=${spec%%:*}
    launch
    facts_valid "$SCEN"
    expect_fact classification "${spec##*:}" "$SCEN"
    scan_run "$SCEN"
done
new_run
SCEN=startup-error-loader
launch
expect_fact launcherExit 127 'the loader refusal exit survives observation'

new_run
SCEN=startup-error-multi
launch
[[ "$(jq -er '[.signatures[]] | map(select(. > 0)) | length' "$F")" == 2 ]] ||
    fail 'multiple-signatures must still publish both closed counts'
ok

new_run
SCEN=unknown-only
launch
expect_status 1 'unknown output exit'
facts_valid 'unknown output'
expect_fact classification no-signature 'unknown output is not renamed'
expect_fact observation complete 'unknown output was fully read'
expect_fact stderrLines 3 'unknown output'
expect_fact unmatchedStderrLines 3 'unknown output'
scan_run 'unknown output'

new_run
SCEN=stdout-signature
launch
expect_status 1 'stdout-only signature exit'
facts_valid 'stdout-only signature'
expect_fact classification no-signature 'stdout is counted, never matched'
expect_fact stdoutLines 1 'stdout-only signature'
expect_fact stderrLines 0 'stdout-only signature'
scan_run 'stdout-only signature'

# ===========================================================================
# D. Bounds: bytes, lines, line length and two absolute deadlines. A
#    signature seen before the bound stays evidence; one after the bound
#    must leave the capture incomplete, never "absent".
# ===========================================================================
new_run
SCEN=big-output; FLOOD=5000; MAXB=4096; MAXN=50
launch
expect_status 0 'flood exit'
facts_valid 'flood'
expect_fact observation truncated 'flood past the byte bound'
expect_fact stderrTruncated 1 'flood'
expect_fact classification no-usable-sandbox \
    'a signature seen before truncation is still evidence'
[[ "$(fact stderrBytes)" -le 4096 ]] || fail 'byte count passed its bound'
ok
scan_run 'flood'

new_run
SCEN=big-output-inverted; FLOOD=5000; MAXB=4096; MAXN=50
launch
facts_valid 'inverted flood'
expect_fact observation truncated 'signature after the bound'
expect_fact classification capture-incomplete \
    'a truncated capture cannot certify absence'
scan_run 'inverted flood'

new_run
SCEN=long-line; LONG=200000; MAXL=256
t0=$(date +%s)
launch
t1=$(date +%s)
expect_status 0 'long line exit'
facts_valid 'long line'
expect_fact observation truncated 'an over-long line is unknown text'
expect_fact classification capture-incomplete 'long line'
[[ $((t1 - t0)) -le 20 ]] || fail 'the long line was not bounded in time'
ok

new_run
SCEN=close-stdout
launch
expect_status 0 'one stream closed early'
facts_valid 'early EOF'
expect_fact observation complete 'the other stream was still fully read'
expect_fact stderrLines 2 'early EOF'
scan_run 'early EOF'

new_run
SCEN=unknown-only; MAXN=2
launch
facts_valid 'line-count bound'
expect_fact stderrLines 2 'the line-count bound held'
expect_fact observation truncated 'line-bound truncation'
expect_fact classification capture-incomplete 'line-bound truncation'
[[ "$(jq -er '.bounds.maxLines' "$F")" == 2 ]] ||
    fail 'the published bounds must be the bounds that ran'
ok
scan_run 'line-count bound'

for bad in 0 abc 9000000 -1; do
    new_run
    MAXB=$bad
    launch
    expect_status 78 "bad byte bound ($bad) refused"
    never_ran "bad byte bound ($bad)"
    no_facts "bad byte bound ($bad)"
done

# ===========================================================================
# E. Time: a deadline that fires, a stop that is forwarded as the same
#    signal it arrived as, escalation past an ignoring launcher, and two
#    absolute ceilings no child behavior can push later.
# ===========================================================================
new_run
SCEN=stall-until-terminated; DEADLINE=1
t0=$(date +%s)
launch
t1=$(date +%s)
expect_status 143 'deadline stop is propagated as the child status'
facts_valid 'deadline'
expect_fact observation timeout 'deadline'
expect_fact stopAction forwarded 'deadline'
expect_fact classification observation-failed \
    'a timed-out window cannot certify absence'
expect_grep "$CALLS" 'term-seen' 'the launcher received SIGTERM at the deadline'
[[ $((t1 - t0)) -le 8 ]] || fail 'the deadline did not bound the observation'
ok
scan_run 'deadline'

# Cancellation signals the wrapper's real pid. A backgrounded bash function
# runs in a subshell whose job pid is not what we need, so the launch here
# is one simple `env` command: env execs bash, the shim execs the reducer
# (its last command, same pid), so $! is exactly the reducer's pid and the
# only process that receives the signal is the observation owner. The whole
# sequence sits in a subshell with stderr closed so bash's asynchronous
# "Terminated" job notice for a re-raised signal cannot mix into the suite
# output; the status crosses back through a file.
cancel_run() { # <TERM|INT> — stop the routed wrapper mid-observation
    local sig=$1
    (
        status=0
        env WAVE12_REAL_NANH="$REAL" WAVE12_REAL_SHA256="$SHA" \
            WAVE12_FACTS_DIR="$FACTS" WAVE12_REDUCER="$reducer" \
            WAVE12_PYTHON=python3 NAN_API_KEY="$synthetic_key" \
            FIXTURE_SCENARIO="$SCEN" FIXTURE_CALLS="$CALLS" \
            ${DEADLINE:+WAVE12_DEADLINE_S=$DEADLINE} \
            ${GRACE:+WAVE12_GRACE_S=$GRACE} \
            ${MAXB:+WAVE12_MAX_BYTES=$MAXB} \
            ${MAXL:+WAVE12_MAX_LINE_BYTES=$MAXL} \
            ${MAXN:+WAVE12_MAX_LINES=$MAXN} \
            bash "$shim" "${LAUNCH[@]}" > "$OUT" 2> "$ERR" &
        pid=$!
        sleep 0.6
        kill -"$sig" "$pid" 2>/dev/null || true
        wait "$pid" || status=$?
        printf '%s\n' "$status" > "$d/status"
    ) 2> /dev/null
    [[ -s "$d/status" ]] ||
        fail "$sig cancellation: the wrapper exited before it could be stopped"
    ST=$(cat "$d/status")
    ok
}

new_run
SCEN=stall-until-terminated; DEADLINE=60
t0=$(date +%s)
cancel_run TERM
t1=$(date +%s)
expect_status 143 'cancelled launch exits as the child exited (143)'
facts_valid 'external SIGTERM'
expect_fact observation cancelled 'external SIGTERM'
expect_fact stopAction forwarded 'external SIGTERM'
expect_grep "$CALLS" 'term-seen' 'SIGTERM was forwarded as SIGTERM'
expect_absent "$CALLS" 'int-seen' 'SIGTERM was not forwarded as anything else'
[[ $((t1 - t0)) -le 10 ]] || fail 'the cancellation did not end the run'
ok
scan_run 'external SIGTERM'

new_run
SCEN=stall-until-terminated; DEADLINE=60
cancel_run INT
expect_status 130 'an interrupt exits as the child exited (130)'
facts_valid 'external SIGINT'
expect_fact observation cancelled 'external SIGINT'
expect_fact stopAction forwarded 'external SIGINT'
expect_grep "$CALLS" 'int-seen' 'SIGINT was forwarded as SIGINT, not SIGTERM'
expect_absent "$CALLS" 'term-seen' 'SIGINT did not silently become SIGTERM'
scan_run 'external SIGINT'

new_run
SCEN=stall-deaf; DEADLINE=60; GRACE=1
t0=$(date +%s)
cancel_run TERM
t1=$(date +%s)
expect_status 137 'an ignoring launcher is killed: status re-raised from SIGKILL'
facts_valid 'deaf external'
expect_fact observation cancelled 'deaf external'
expect_fact stopAction escalated-kill 'deaf external'
expect_fact launcherDisposition signaled 'the escalated kill is the disposition'
expect_fact launcherSignal 9 'escalation used SIGKILL'
[[ $((t1 - t0)) -le 10 ]] || fail 'escalation did not bound the observation'
ok

new_run
SCEN=stall-deaf; DEADLINE=1; GRACE=1
t0=$(date +%s)
launch
t1=$(date +%s)
expect_status 137 'the deadline path escalates too'
facts_valid 'deaf deadline'
expect_fact observation timeout 'deaf deadline'
expect_fact stopAction escalated-kill 'deaf deadline'
[[ $((t1 - t0)) -le 8 ]] || fail 'the deaf deadline was not bounded'
ok

# A detached descendant that outlives the launcher and keeps writing into
# an inherited pipe must not extend anything: the post-exit drain ceiling
# is absolute from the moment the launcher was first observed dead, and
# the far-off deadline must not be what saves the observation either.
new_run
SCEN=holder-exit; DEADLINE=60; HOLDER=25
t0=$(date +%s)
launch
t1=$(date +%s)
expect_status 0 "the launcher's own exit stands"
facts_valid 'fd holder'
expect_fact observation truncated 'a held pipe cannot conclude the capture'
expect_fact classification capture-incomplete 'fd holder'
[[ $((t1 - t0)) -ge 1 && $((t1 - t0)) -le 12 ]] ||
    fail 'the post-exit ceiling, not the holder or the deadline, must end this'
ok

new_run
SCEN=holder-writer; DEADLINE=60; WRITES=100; MAXB=65536
t0=$(date +%s)
launch
t1=$(date +%s)
expect_status 0 'launcher exit stands past a writing descendant'
facts_valid 'writing descendant'
expect_fact observation truncated \
    'continuous descendant writes cannot push the ceiling out'
expect_fact classification capture-incomplete 'writing descendant'
[[ "$(fact stderrBytes)" -gt 0 ]] || fail 'the descendant bytes went uncounted'
ok
[[ $((t1 - t0)) -le 12 ]] ||
    fail 'a last-byte-restartable drain starved the observation to its deadline'
ok
scan_run 'writing descendant'

# A bound binary that exists, matches its digest and is executable, but
# cannot be spawned at all: there is no child status to propagate, so the
# reserved 76 carries the fact that no disposition was ever observed.
new_run
printf '#!/nonexistent/interpreter-for-wave12\n' > "$d/badinterp"
chmod 755 "$d/badinterp"
REAL="$d/badinterp"
SHA=$(digest_of "$REAL")
launch
expect_status 76 'a binary that cannot be spawned reports 76'
facts_valid 'launch-failed'
expect_fact observation launch-failed 'launch-failed'
expect_fact launcherDisposition never-started 'launch-failed'
expect_fact classification observation-failed 'launch-failed'
never_ran 'launch-failed never executed anything'

# ===========================================================================
# F. The closed schema refuses every claim the counts do not show. Each
#    mutation below is a sentence the facts must not be allowed to say; the
#    v1 reader also refuses a future-version document (versioned readback).
# ===========================================================================
new_run
SCEN=startup-error-sandbox
launch
facts_valid 'schema base'
schemas="$workspace/schemas"
mkdir "$schemas"
cp "$F" "$schemas/base.json"
mutate() { # <name> <jq filter>
    local name=$1 filter=$2 out="$schemas/$1.json"
    jq "$filter" "$schemas/base.json" > "$out"
    if python3 "$reducer" validate --facts "$out"; then
        fail "the validator accepted the $name mutation"
    fi
    ok
}
mutate 'unknown-key' '. + {rawText: "not allowed"}'
mutate 'missing-key' 'del(.failure)'
mutate 'future-version' '.schemaVersion = 2'
mutate 'past-version' '.schemaVersion = 0'
mutate 'invented-class' '.classification = "loader-missing-shared-object"'
mutate 'complete-claims-truncation' '.stderrTruncated = 1 | .stdoutTruncated = 1'
mutate 'absence-from-partial' \
    '.observation = "truncated" | .classification = "no-signature"
     | .stderrTruncated = 1'
mutate 'incomplete-without-truncation' \
    '.classification = "capture-incomplete" | .observation = "complete"
     | .stderrTruncated = 0 | .stdoutTruncated = 0'
mutate 'signatures-beyond-lines' '.signatures["no-usable-sandbox"] = 9999'
mutate 'refusal-that-ran' '.failure = "identity-refused"'
mutate 'huge-exit' '.launcherExit = 256'
mutate 'unbounded-bytes' '.stdoutBytes = -1'
mutate 'zero-byte-bound' '.bounds.maxStreamBytes = 0'
mutate 'non-digest' '.identity.shimSha256 = "not-a-digest"'
mutate 'open-vocabulary' '.observation = "almost-complete"'
mutate 'unbounded-signal' '.launcherSignal = 65'
mutate 'stop-that-stopped-nothing' \
    '.observation = "timeout" | .stopAction = "none"'
mutate 'multiple-unreported' '.signatures["display-unavailable"] = 3'
for cause in no-usable-sandbox display-unavailable suid-sandbox-missing suid-sandbox-misconfigured loader-missing-shared-object multiple-signatures; do
    mutate "unsupported-cause-$cause" \
        ".signatures |= with_entries(.value = 0) | .classification = \"$cause\""
done

# The refuse subcommand writes closed preflight facts and nothing else.
new_run
python3 "$reducer" refuse --facts "$FACTS" --reason runtime; ok
facts_valid 'refuse runtime'
expect_fact failure runtime-refused 'refuse runtime'
expect_fact observation observation-failed 'refuse runtime'
new_run
if python3 "$reducer" refuse --facts "$FACTS" --reason 'because-i-said-so' \
    2>/dev/null; then
    fail 'refuse accepted an invented reason'
fi
ok
no_facts 'refuse with a bad reason'
chmod 750 "$FACTS"
if python3 "$reducer" refuse --facts "$FACTS" --reason identity 2>/dev/null; then
    fail 'refuse wrote into a group-readable directory'
fi
ok
chmod 700 "$FACTS"

new_run
FACTS_SYMLINK_BASE="$workspace/linktarget"
mkdir "$FACTS_SYMLINK_BASE"
chmod 700 "$FACTS_SYMLINK_BASE"
ln -s "$FACTS_SYMLINK_BASE" "$workspace/facts-link"
FACTS="$workspace/facts-link"
launch
expect_status 78 'a symlinked facts directory is refused'
never_ran 'symlinked facts directory'
[[ ! -e "$FACTS_SYMLINK_BASE/startup-facts.json" ]] ||
    fail 'the refusal escaped through the symlink'
ok
new_run
FACTS="$FACTS/never-created"
launch
expect_status 78 'a missing facts directory is refused'
never_ran 'missing facts directory'

# Direct reducer invocation (no shim): the identity assertion is repeated,
# so a bypassed wrapper still cannot launch an unverified binary.
new_run
python3 "$reducer" observe --real "$fixture" --sha256 "$fixture_sha" \
    --shim "$shim" --facts "$FACTS" -- "${LAUNCH[@]}" \
    > "$OUT" 2> "$ERR"; ok
expect_status_direct() { :; }
facts_valid 'direct observe'
expect_fact observation complete 'direct observe'
[[ "$(jq -er '.identity.shimSha256' "$F")" == "$(digest_of "$shim")" ]] ||
    fail 'the wrapper identity published in facts is not the bound wrapper'
ok
expect_absent "$OUT" 'sk-super' 'the reducer terminal stays silent'
scan_run 'direct observe'

# ===========================================================================
# G. The checker binding. `nanh-desktop-check --launch-wrapper` executes this
#    shim directly with only WAVE12_REAL_NANH, WAVE12_REAL_SHA256, a fresh
#    WAVE12_FACTS_DIR and a pinned WAVE12_DEADLINE_S, and removes every
#    reducer and bound override: the reducer beside the shim must run, and
#    the loopback, absolute-path vector the checker builds must be routed.
# ===========================================================================
CHECKER_LAUNCH=(chatgpt-desktop --provider-base-url http://127.0.0.1:43123/v1
    --model qwen3.6 --executable /opt/ChatGPT/chatgpt)
checker_run() {
    ST=0
    { env -u WAVE12_REDUCER -u WAVE12_PYTHON -u WAVE12_GRACE_S \
        -u WAVE12_MAX_BYTES -u WAVE12_MAX_LINE_BYTES -u WAVE12_MAX_LINES \
        WAVE12_REAL_NANH="$REAL" WAVE12_REAL_SHA256="$SHA" \
        WAVE12_FACTS_DIR="$FACTS" WAVE12_DEADLINE_S=300 \
        NAN_API_KEY="$synthetic_key" FIXTURE_SCENARIO="$SCEN" \
        FIXTURE_CALLS="$CALLS" \
        "$shim" "${CHECKER_LAUNCH[@]}" > "$OUT" 2> "$ERR" || ST=$?; } 2> /dev/null
    ok
}
new_run
checker_run
expect_status 0 'checker-shaped launch'
facts_valid 'checker-shaped launch'
expect_fact observation complete 'checker-shaped launch'
[[ "$(jq -er '.identity.reducerSha256' "$F")" == "$(digest_of "$reducer")" ]] ||
    fail 'the checker binding did not run the reducer beside the shim'
ok
[[ "$(jq -er '.identity.shimSha256' "$F")" == "$(digest_of "$shim")" ]] ||
    fail 'the facts do not name the bound wrapper'
ok
[[ "$(jq -er '.bounds.deadlineSeconds' "$F")" == 300 ]] ||
    fail 'the pinned checker deadline did not reach the reducer'
ok
head -8 "$CALLS" > "$d/argv"
printf '%s\n' "${CHECKER_LAUNCH[@]}" --debug > "$d/want"
cmp -s "$d/argv" "$d/want" || fail 'the checker-built vector was not routed exactly'
ok
scan_run 'checker-shaped launch'

# The same binding naming a stale real-binary digest refuses before launch.
new_run
SHA=$(digest_of "$reducer")
checker_run
expect_status 77 'checker binding with a stale digest'
never_ran 'checker binding with a stale digest'
facts_valid 'checker binding refusal'
expect_fact failure identity-refused 'checker binding refusal'

# ===========================================================================
# H. Whole-workspace privacy sweep and the pinned old validator.
# ===========================================================================
for marker in "${markers[@]}"; do
    hits=$(grep -rlF -- "$marker" "$workspace" 2>/dev/null \
        | grep -v -e '/schemas/' || true)
    [[ -z "$hits" ]] ||
        fail "a raw byte ('$marker') was persisted somewhere: $hits"
done
ok
for junk in 'API_KEY' 'chrome-sandbox' 'zygote_host' 'ozone_platform' \
    'libfoo.so' 'sign in' 'prompt:'; do
    hits=$(grep -rlF -- "$junk" "$workspace"/run-*/facts 2>/dev/null || true)
    [[ -z "$hits" ]] || fail "facts named raw content ('$junk')"
done
ok

# Versioned readback: run 34519896130 at 023ace2 must be revalidated with
# the validator of its own era, not the current v2 contract. The historical
# file has to at least still parse; WAVE12_REPORT.md carries the full
# readback command.
git --git-dir="$script_root/../.git" show \
    023ace2:scripts/chatgpt-wave10-startup.sh 2>/dev/null \
    | bash -n 2>/dev/null ||
    fail 'the pinned 023ace2 wave-10 validator no longer parses'
ok

printf 'chatgpt wave12 contracts passed (%s checks)\n' "$pass_count"
