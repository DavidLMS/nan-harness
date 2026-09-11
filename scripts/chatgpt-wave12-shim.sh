#!/usr/bin/env bash
# Temporary wave-12 launch-only debug wrapper for nanh (Linux diagnostic).
#
# What this is: a stand-in `nanh` for ONE disposable startup observation.
# When it is asked the exact ChatGPT desktop launch the shipped checker
# builds (`chatgpt-desktop --provider-base-url <url> --model <m>
# --executable <path>...`, see crates/nan-harness-desktop-check/src/
# probe.rs launch_command), it routes that single invocation through the
# bounded reducer, which adds the pre-existing `--debug` flag
# (crates/nan-harness-cli/src/app/args/desktop.rs) so the application's
# own startup stderr becomes observable instead of discarded. Every other
# argument vector is forwarded to the real binary untouched by `exec`, so
# `--version`, `--help`, `--restore`, `--dry-run`, other subcommands and
# unknown shapes behave exactly as direct calls to the real nanh -- once
# the binding below has verified. A missing or malformed binding refuses
# every call closed before any exec: `--version` from a half-configured
# observation wrapper must not masquerade as a healthy binary. The facts
# directory's own mode and ownership are enforced by the reducer again
# before anything is observed or written.
#
# What this is not: a product change, a new flag, or a new identity. No
# normal CLI behavior changes: only the explicit, hidden checker binding
# (`nanh-desktop-check --launch-wrapper <this file> --launch-wrapper-sha256
# <its digest> --launch-wrapper-facts <0700 dir>`) makes the checker run
# this file, and then only as the program of the ChatGPT launch command.
# The checker verifies this file's digest before any process runs, sets
# WAVE12_REAL_NANH, WAVE12_REAL_SHA256 and a fresh per-probe
# WAVE12_FACTS_DIR itself, pins the deadline and removes every reducer and
# bound override, so the reducer beside this file runs. Its version, help
# and restore calls still execute the real binary directly, never this
# file. It must never be passed to the shipped checker as `--nan-harness`:
# the checker reports `binary_digest(<launched file>)` as the tested
# nanh identity, so pointing that slot at a wrapper would substitute the
# wrapper's digest for the product's. The real binary's digest stays the
# source of truth here: it is required as an input, asserted before
# anything runs or is forwarded, and refused closed (never launched,
# facts written or version/help output produced) when it does not match.
#
# Exit statuses 76, 77 and 78 are reserved refusals (see the reducer): a
# transparent `exec` forwards the real binary's own status whatever it
# is, including those numbers; a routed launch forwards the child's own
# disposition exactly (an external stop is forwarded as the same signal
# it arrived as -- SIGINT stays SIGINT, SIGTERM stays SIGTERM -- a fatal
# child signal is re-raised, and if the launcher exited with 76/77/78
# itself the facts, not the status, are the authority). The shim adds no
# status of its own except on refusal paths, and it never writes to
# stdout or stderr itself: whatever bytes the transparent case emits are
# the real binary's bytes, and the routed case's terminal stays silent
# because the reducer keeps the captured output out of every channel.
#
# Privacy: environment values, arguments and captured output are never
# echoed, logged or persisted by this script. The facts directory must be
# an existing 0700 directory (the reducer enforces it again before any
# write, per SECURITY.md private-file rules).
set -euo pipefail
umask 077

REFUSE_RUNTIME=78
REFUSE_IDENTITY=77

die() {
    # Status only: the reason is a closed refusal word, not observed data.
    exit "$1"
}

refused-facts() {
    # Record the closed refusal fact through the reducer itself, so the
    # document is validated before it lands (the reducer trusts only its
    # own vocabulary, never this caller). If even recording fails -- no
    # python, no reducer, an unusable directory -- the refusal still
    # surfaces through the reserved exit status; nothing here ever
    # publishes a digest, path, argument or observed byte.
    "$PYTHON" "$REDUCER" refuse --facts "$WAVE12_FACTS_DIR" \
        --reason "$1" || true
}

is_hex64() {
    [[ "$1" =~ ^[0-9a-f]{64}$ ]]
}

# ---------------------------------------------------------------------------
# Required binding. Every value is an input the caller already knows from
# the build, never something discovered or guessed here.
# ---------------------------------------------------------------------------
[[ -n "${WAVE12_REAL_NANH:-}" && "$WAVE12_REAL_NANH" == /* ]] || die $REFUSE_RUNTIME
[[ -n "${WAVE12_REAL_SHA256:-}" ]] || die $REFUSE_RUNTIME
is_hex64 "$WAVE12_REAL_SHA256" || die $REFUSE_RUNTIME
[[ -n "${WAVE12_FACTS_DIR:-}" && "$WAVE12_FACTS_DIR" == /* ]] || die $REFUSE_RUNTIME
[[ -d "$WAVE12_FACTS_DIR" ]] || die $REFUSE_RUNTIME
SELF=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &&
    printf '%s/%s' "$PWD" "$(basename -- "${BASH_SOURCE[0]}")")
REDUCER="${WAVE12_REDUCER:-$(dirname -- "$SELF")/chatgpt-wave12-reducer.py}"
[[ -f "$REDUCER" ]] || die $REFUSE_RUNTIME
PYTHON="${WAVE12_PYTHON:-python3}"
command -v "$PYTHON" > /dev/null 2>&1 || die $REFUSE_RUNTIME
if command -v sha256sum > /dev/null 2>&1; then
    digest_of() { sha256sum -- "$1" | cut -c1-64; }
elif command -v shasum > /dev/null 2>&1; then
    digest_of() { shasum -a 256 -- "$1" | cut -c1-64; }
else
    die $REFUSE_RUNTIME
fi

# ---------------------------------------------------------------------------
# Identity assertion, before anything is forwarded or launched. A missing,
# unreadable or mismatched binary is one closed refusal: the reducer writes
# the refusal fact (with the found file's identity redacted, never
# published), and the reserved status 77 surfaces. Only after the tools
# needed to record that fact are known good can the check run.
# ---------------------------------------------------------------------------
if [[ ! -e "$WAVE12_REAL_NANH" || ! -f "$WAVE12_REAL_NANH" ]]; then
    refused-facts identity
    die $REFUSE_IDENTITY
fi
if ! REAL_DIGEST=$(digest_of "$WAVE12_REAL_NANH"); then
    # Unreadable means unverifiable: the same closed identity refusal.
    refused-facts identity
    die $REFUSE_IDENTITY
fi
if [[ "$REAL_DIGEST" != "$WAVE12_REAL_SHA256" ]]; then
    # The binary behind this wrapper is not the binary the run bound to.
    # Nothing is forwarded, nothing is launched, and no transparent output
    # is produced: an unverified binary may not even answer --version.
    # Record the closed identity refusal (no digests, no claims), then die
    # with the reserved status. A refusal fact document is validated by the
    # reducer itself before it is written.
    "$PYTHON" "$REDUCER" refuse --facts "$WAVE12_FACTS_DIR" \
        --reason identity || true
    die $REFUSE_IDENTITY
fi

# ---------------------------------------------------------------------------
# Routing. The launch is recognised by its exact shape, the seven-token
# vector `launch_command` builds: the first word is the `chatgpt-desktop`
# subcommand, then each of --provider-base-url, --model and --executable
# appears exactly once in any order with a value that is not itself a flag,
# and nothing else at all. A vector outside this shape -- extra arguments,
# repeated flags, flag-like values, or a transparent case (--help,
# --version, --restore, --dry-run, --debug already present) -- is forwarded
# untouched by `exec`. Refusing to guess is what keeps every other call
# exactly the real binary's behavior.
# ---------------------------------------------------------------------------
is_launch=0
if [[ "${1:-}" == chatgpt-desktop && $# -eq 7 ]]; then
    count_url=0 count_model=0 count_exec=0
    values_ok=1
    index=2
    while [[ $index -le $# ]]; do
        word=${!index}
        case "$word" in
            --provider-base-url) count_url=$((count_url + 1)) ;;
        esac
        case "$word" in
            --model) count_model=$((count_model + 1)) ;;
        esac
        case "$word" in
            --executable) count_exec=$((count_exec + 1)) ;;
        esac
        case "$word" in
            --help|--version|--restore|--dry-run|--debug|-h|-V|--*)
                if [[ "$word" != --provider-base-url && "$word" != --model \
                    && "$word" != --executable ]]; then
                    count_url=0 count_model=0 count_exec=0
                    break
                fi
                # A flag token must be followed by a value, never a flag.
                next=$((index + 1))
                if [[ $next -gt $# ]]; then
                    values_ok=0
                    break
                fi
                case "${!next}" in
                    -*) values_ok=0; break ;;
                esac
                ;;
        esac
        index=$((index + 1))
    done
    if [[ "$values_ok" == 1 && $count_url == 1 && $count_model == 1 \
        && $count_exec == 1 ]]; then
        is_launch=1
    fi
fi

if [[ "$is_launch" != 1 ]]; then
    # Transparent case: identity-preserving exec. The real binary answers
    # for itself on every channel, with every argument, environment entry,
    # working directory and terminal exactly as inherited from the caller.
    exec "$WAVE12_REAL_NANH" "$@"
fi

# Launch case: one bounded observation. The reducer appends --debug, owns
# both pipes, reduces in memory, writes closed facts, and reproduces the
# child's exit disposition for us to propagate by its exit status.
exec "$PYTHON" "$REDUCER" observe \
    --real "$WAVE12_REAL_NANH" --sha256 "$WAVE12_REAL_SHA256" \
    --shim "$SELF" --facts "$WAVE12_FACTS_DIR" -- "$@"
