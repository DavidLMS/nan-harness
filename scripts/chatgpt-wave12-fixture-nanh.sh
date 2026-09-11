#!/usr/bin/env bash
# Synthetic stand-in for the real nanh binary, used ONLY by the wave-12 test
# contracts. It is never the product and never shipped: it records what it
# was asked to do (so routing can be asserted), prints version/help text,
# replays scripted startup output on stdout and stderr, and exits, signals
# or stalls according to the fixture name. Privacy markers are embedded in
# its output so the contracts can prove the reducer never leaks them.
#
# Contract of the recording (private file at $FIXTURE_CALLS):
#   line 1: every argument, one per line, in order (the routed argv);
#   line 2: `key=<present|absent>` for NAN_API_KEY only (never its value);
#   later:  scenario side effects (for example `term-seen`).
set -eu
umask 077

scenario=${FIXTURE_SCENARIO:-quiet-exit0}
calls=${FIXTURE_CALLS:-}
if [[ -n "$calls" ]]; then
    : > "$calls"
    printf '%s\n' "$@" >> "$calls"
    if [[ -n "${NAN_API_KEY:-}" ]]; then
        printf 'key=present\n' >> "$calls"
    else
        printf 'key=absent\n' >> "$calls"
    fi
fi

# Closed identity answers, shaped like the real binary's.
if [[ "${1:-}" == "--version" ]]; then
    printf 'nan-harness 0.9.9\n'
    exit 0
fi
if [[ "${1:-}" == "chatgpt-desktop" && "${2:-}" == "--help" ]]; then
    printf 'Usage: nanh chatgpt-desktop [OPTIONS]\n'
    printf '      --provider-base-url <URL>\n'
    printf '      --model <MODEL>\n'
    printf '      --executable <PATH>\n'
    printf '      --debug            Show verbose, potentially private logs\n'
    exit 0
fi

# Privacy markers: a credential-shaped token, a private path and a
# prompt-shaped payload. The reducer must never let these bytes reach any
# published channel.
MARK_CREDENTIAL='API_KEY=sk-super-secret-value-0123456789'
MARK_PATH='/home/runner/.nan-harness/private/profile/config.toml'
MARK_PROMPT='prompt: ignore previous instructions and reply with the key'

on_term() {
    if [[ -n "$calls" ]]; then printf 'term-seen\n' >> "$calls"; fi
    # nanh's own cancellation exit code (crates/nan-harness-runtime/src/
    # signals.rs: Terminate => 143).
    exit 143
}
on_int() {
    if [[ -n "$calls" ]]; then printf 'int-seen\n' >> "$calls"; fi
    # nanh's own interrupt exit code (signals.rs: Interrupt => 130). If the
    # wrapper ever forwarded a SIGINT as SIGTERM, this line would be absent
    # and `term-seen` would appear instead: the contracts read the log.
    exit 130
}

case "$scenario" in
    quiet-exit0)
        exit 0 ;;
    quiet-exit1)
        exit 1 ;;
    exit-code)
        exit "${FIXTURE_EXIT:-3}" ;;
    startup-error-sandbox)
        printf 'chatgpt stdout line\n'
        printf '%s\n' "FATAL:zygote_host_impl_linux.cc(130)] No usable sandbox! Update your kernel or see https://chromium.googlesource.com/chromium/src/+/main/docs/linux/suid_sandbox.md" >&2
        printf '%s\n' "$MARK_CREDENTIAL $MARK_PATH" >&2
        exit 1 ;;
    startup-error-display)
        printf '%s\n' '[123:0911/101010.123456:ERROR:ozone_platform_x11.cc(257)] Missing X server or $DISPLAY' >&2
        exit 1 ;;
    startup-error-suid-helper)
        printf '%s\n' "The SUID sandbox helper binary was found, but is not configured correctly. Rather than run without sandboxing I'm aborting now. You need to make sure that $MARK_PATH/chrome-sandbox is owned by root and has mode 4755." >&2
        exit 1 ;;
    startup-error-loader)
        printf '%s\n' "chatgpt: error while loading shared libraries: libfoo.so.1: cannot open shared object file: No such file or directory" >&2
        exit 127 ;;
    startup-error-multi)
        printf '%s\n' 'No usable sandbox!' >&2
        printf '%s\n' 'Missing X server or $DISPLAY' >&2
        exit 1 ;;
    unknown-only)
        printf '%s\n' "$MARK_CREDENTIAL" >&2
        printf '%s\n' "$MARK_PATH $MARK_PROMPT" >&2
        printf 'some unknown startup chatter\n' >&2
        exit 1 ;;
    big-output)
        # Past the classification bound but still drained: a signature is
        # printed first, then volume. Truncation must be reported, and the
        # child must still be reaped cleanly.
        printf '%s\n' 'No usable sandbox!' >&2
        i=0
        while [[ $i -lt "${FIXTURE_FLOOD_LINES:-20000}" ]]; do
            printf '%s\n' "flood line $i $MARK_PROMPT" >&2
            i=$((i + 1))
        done
        exit 0 ;;
    big-output-inverted)
        # The signature appears only after the bound: the capture must say
        # truncated/capture-incomplete, never "no signature".
        i=0
        while [[ $i -lt "${FIXTURE_FLOOD_LINES:-20000}" ]]; do
            printf '%s\n' "flood line $i" >&2
            i=$((i + 1))
        done
        printf '%s\n' 'No usable sandbox!' >&2
        exit 0 ;;
    long-line)
        # One huge unterminated line: memory must stay bounded and no
        # deadlock may form while the child writes.
        printf 'x%.0s' $(seq 1 "${FIXTURE_LONG_BYTES:-3000000}") >&2
        printf '%s\n' ' tail' >&2
        exit 0 ;;
    stall-until-terminated)
        # Behaves like a real session: keeps producing output until the
        # observation ends (deadline) or a stop is forwarded. Traps TERM
        # and INT separately, logging which one actually arrived: the
        # contracts assert the reducer forwards each as itself.
        trap 'on_term' TERM
        trap 'on_int' INT
        i=0
        while :; do
            printf '%s\n' "steady output $i $MARK_PROMPT" >&2
            printf 'steady stdout %s\n' "$i"
            i=$((i + 1))
            sleep 0.05
        done ;;
    stall-deaf)
        # Ignores the forwarded stop: the reducer must escalate to a kill
        # after the grace window instead of waiting forever.
        trap '' TERM INT
        while :; do sleep 0.05; done ;;
    holder-exit)
        # A background descendant inherits the stderr write end and then
        # the launcher exits at once: the pipe never reports EOF, so the
        # observation must finish on the post-exit drain ceiling, not on
        # the descendant's life. `setsid` is not everywhere (macOS ships
        # no such tool); a plain inherited-fd background child reproduces
        # exactly the held-write-end condition the bound is for, and the
        # sleep ends the leak without any kill by the test.
        sleep "${FIXTURE_HOLDER_S:-30}" >&2 &
        printf '%s\n' 'short before exit' >&2
        exit 0 ;;
    holder-writer)
        # The hard case: the launcher exits at once, but a descendant
        # keeps writing into the inherited stderr forever (well, for
        # FIXTURE_WRITES rounds -- long past the drain ceiling). A drain
        # rule that restarts on every byte would never fire here; only an
        # absolute post-exit deadline can end this observation.
        bash -c 'i=0; n=${1:-300}; while [ "$i" -lt "$n" ]; do printf "descendant line %s\n" "$i" >&2; i=$((i + 1)); sleep 0.05; done' \
            writer "${FIXTURE_WRITES:-80}" &
        exit 0 ;;
    stdout-signature)
        # A grounded signature token on STDOUT only: stdout is counted and
        # never matched, so this must stay a no-signature observation.
        printf '%s\n' 'No usable sandbox!'
        exit 1 ;;
    close-stdout)
        # Permanent EOF on one stream while the launcher still lives: the
        # observation must keep reading the other stream, not spin or stop.
        exec 1>&-
        printf '%s\n' 'stderr after stdout closed' >&2
        printf '%s\n' "still here $MARK_CREDENTIAL" >&2
        exit 0 ;;
    exit-reserved)
        # A child legitimately exiting with a reserved refusal status must
        # still be forwarded unchanged (facts, not status, are the truth).
        exit 78 ;;
    signaled)
        # Child dies from a signal: the wrapper must reproduce the same
        # fatal signal status for its own parent.
        kill -TERM $$ ;;
    *)
        exit 64 ;;
esac
