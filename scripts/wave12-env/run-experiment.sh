#!/usr/bin/env bash
# Wave12 disposable macOS environment experiment (prepared, reviewable).
#
# This orchestrator is the LOCAL, reviewable preparation for ONE future
# disposable GitHub-hosted macOS ARM64 run that compares the deterministic
# checker outcome under a "baseline" condition versus a "dock-hidden"
# condition. It is designed to be safe to review and to fail closed when
# invoked outside a disposable GitHub-hosted macOS ARM64 environment.
#
# It is NOT authorized to run on the shared Mac and is NOT run here. It is
# exercised ONLY against the fixture command shims in ./fixtures/bin by the
# synthetic test suite (tests/run-synthetic-tests.sh). The fixture shims
# shadow only the narrow set of system driver commands the orchestrator uses
# (defaults, killall, pgrep, uname) and a fixture checker, so the same code
# path is exercised without touching any real macOS preference or Dock.
#
# Invariant: the occlusion/focus/ownership guards are never disabled, and no
# guard result is altered. This script only captures per-condition outcomes.

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
CONDITION="${1:-}"
[ -n "$CONDITION" ] && shift || { echo "usage: run-experiment.sh <baseline|dock-hidden> [options]" >&2; exit 2; }
case "$CONDITION" in
  baseline|dock-hidden) ;;
  *) echo "unknown condition: $CONDITION" >&2; exit 2 ;;
esac

OUTPUT_DIR=""
CHECKER="${WAVE12_CHECKER:-}"
NAN_HARNESS="${WAVE12_NAN_HARNESS:-}"
PREPARED="${WAVE12_PREPARED:-}"
APP="zed-desktop"
FIXTURE_BIN="${WAVE12_FIXTURE_BIN:-}"
READY_TIMEOUT="${WAVE12_READY_TIMEOUT_SECS:-30}"
READY_POLL="${WAVE12_READY_POLL_SECS:-1}"
SOURCE_COMMIT="${SOURCE_COMMIT:-unknown}"
RUN_URL="${RUN_URL:-none}"

while [ $# -gt 0 ]; do
  case "$1" in
    --output-dir) OUTPUT_DIR="${2:?}"; shift 2 ;;
    --checker) CHECKER="${2:?}"; shift 2 ;;
    --nan-harness) NAN_HARNESS="${2:-}"; shift 2 ;;
    --prepared) PREPARED="${2:?}"; shift 2 ;;
    --app) APP="${2:-}"; shift 2 ;;
    --fixture-bin) FIXTURE_BIN="${2:-}"; shift 2 ;;
    --source-commit) SOURCE_COMMIT="${2:-}"; shift 2 ;;
    --run-url) RUN_URL="${2:-}"; shift 2 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

: "${OUTPUT_DIR:?--output-dir is required}"

# Bound the readiness wait so a pathological value cannot spin forever.
# Values must be positive integers; anything else fails closed.
case "$READY_TIMEOUT" in
  ''|*[!0-9]*) echo "invalid WAVE12_READY_TIMEOUT_SECS=$READY_TIMEOUT" >&2; exit 1 ;;
esac
case "$READY_POLL" in
  ''|*[!0-9]*) echo "invalid WAVE12_READY_POLL_SECS=$READY_POLL" >&2; exit 1 ;;
esac
[ "$READY_TIMEOUT" -ge 1 ] && [ "$READY_TIMEOUT" -le 300 ] \
  || { echo "WAVE12_READY_TIMEOUT_SECS out of range: $READY_TIMEOUT" >&2; exit 1; }
[ "$READY_POLL" -ge 1 ] && [ "$READY_POLL" -le 60 ] \
  || { echo "WAVE12_READY_POLL_SECS out of range: $READY_POLL" >&2; exit 1; }

# The source commit must be a full lowercase git object id when it is supplied,
# so the scoped comparison records only a verifiable source identity.
if [ "$SOURCE_COMMIT" != "unknown" ]; then
  case "$SOURCE_COMMIT" in
    [0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]) ;;
    *) echo "invalid SOURCE_COMMIT=$SOURCE_COMMIT" >&2; exit 1 ;;
  esac
fi
# The run URL, when supplied, must be an https URL so the recorded evidence is a
# verifiable closed fact rather than an arbitrary echoed string.
if [ "$RUN_URL" != "none" ]; then
  case "$RUN_URL" in
    https://*) ;;
    *) echo "invalid RUN_URL=$RUN_URL" >&2; exit 1 ;;
  esac
fi

# ---------------------------------------------------------------------------
# Driver: run a system command, shadowed by a fixture shim when requested.
# Only the narrow driver set is fixture-shimmed; shared tools (printf, cat,
# mkdir, date, sleep, grep) always run from the real PATH, so a fixture run
# still exercises the identical control flow.
# ---------------------------------------------------------------------------
driver() {
  local cmd="$1"; shift
  # Coordinator quarantine after a synthetic run reached the shared Mac.
  # Real drivers are intentionally unavailable until separately reviewed.
  # Missing or incomplete fixtures must never fall through to host commands.
  [ -n "$FIXTURE_BIN" ] || { echo 'real environment drivers are disabled pending review' >&2; return 78; }
  case "$cmd" in
    defaults|killall|pgrep|uname) ;;
    "$FIXTURE_BIN/nanh-desktop-check") cmd=nanh-desktop-check ;;
    *) echo 'fixture driver is not allowed' >&2; return 78 ;;
  esac
  [ -x "$FIXTURE_BIN/$cmd" ] && [ ! -L "$FIXTURE_BIN/$cmd" ] || {
    echo 'fixture driver is missing or invalid' >&2; return 78;
  }
  "$FIXTURE_BIN/$cmd" "$@"
}

# ---------------------------------------------------------------------------
# Host gating (fail closed on non-disposable / unsupported host).
# Returns: 0 = ready, 1 = non-disposable, 2 = not Darwin, 3 = not arm64.
# ---------------------------------------------------------------------------
guard_host() {
  # Disposability requires the same trusted GitHub-hosted signal the checker's
  # SessionMode::GithubHosted accepts: GITHUB_ACTIONS=true and
  # RUNNER_ENVIRONMENT=github-hosted. A personal/shared session never matches,
  # so this orchestrator refuses to touch any Dock/preference there.
  if [ "${GITHUB_ACTIONS:-}" != "true" ] || [ "${RUNNER_ENVIRONMENT:-}" != "github-hosted" ]; then
    return 1
  fi
  # Propagate a driver refusal explicitly: with the quarantine in place a missing
  # or invalid fixture must surface as the driver's own fail-closed code rather
  # than being misread as "not Darwin"/"not arm64". The exact exit code is the
  # observable closed fact for the negative fixture contract.
  local os arch rs
  set +e
  os="$(driver uname -s)"; rs=$?
  set -e
  if [ "$rs" -ne 0 ]; then return "$rs"; fi
  set +e
  arch="$(driver uname -m)"; rs=$?
  set -e
  if [ "$rs" -ne 0 ]; then return "$rs"; fi
  if [ "$os" != "Darwin" ]; then return 2; fi
  if [ "$arch" != "arm64" ]; then return 3; fi
  return 0
}

# ---------------------------------------------------------------------------
# Dock preference primitives (macOS, supported user-defaults behaviour).
# ---------------------------------------------------------------------------
# Read the Dock auto-hide preference, distinguishing the states the orchestrator
# must not conflate. Emits one of:
#   absent      the key is genuinely not present (`defaults` said "does not exist")
#   true        the key reads as a truthy numeric/boolean literal
#   false       the key reads as a falsy numeric/boolean literal
#   unreadable  `defaults` failed for a reason OTHER than "does not exist" (e.g.
#               a permission/domain error): the prior value is UNKNOWN
#   invalid     the key exists but holds a value we do not understand
# A non-absent/unreadable state means we may only mutate it if we can verify the
# restoration afterwards; an unknown prior value must never be deleted or
# overwritten on a guess.
dock_current() {
  local v rc err
  set +e
  err="$(driver defaults read com.apple.dock autohide 2>&1)"
  rc=$?
  set -e
  if [ "$rc" -ne 0 ]; then
    if printf '%s' "$err" | grep -qi 'does not exist'; then
      echo absent
    else
      echo unreadable
    fi
    return 0
  fi
  v="$err"
  case "$v" in
    1|true|TRUE|True) echo true ;;
    0|false|FALSE|False) echo false ;;
    *) echo invalid ;;
  esac
}
# Decide whether a read-back value matches the exact prior value we intend to
# restore to. Returns 0 on a byte-for-byte match, 1 otherwise. The value must be
# stable and known (absent/true/false) for this to be meaningful.
current_matches() {
  local want="$1" got
  got="$(dock_current)"
  [ "$got" = "$want" ]
}
dock_write() { driver defaults write com.apple.dock autohide -bool "$1"; }
dock_delete() { set +e; driver defaults delete com.apple.dock autohide; local rc=$?; set -e; return "$rc"; }
dock_restart() { set +e; driver killall Dock; local rc=$?; set -e; return "$rc"; }
dock_write() { driver defaults write com.apple.dock autohide -bool "$1"; }
dock_delete() { set +e; driver defaults delete com.apple.dock autohide; set -e; true; }
dock_restart() { set +e; driver killall Dock; local rc=$?; set -e; return "$rc"; }

# Bounded readiness that observes the intended STATE, not a fixed sleep.
# dock-hidden: returns 0 only once the preference reads back as the requested
#   value AND the Dock process is alive. This confirms the intervention was
#   applied; it does NOT claim the Dock stopped overlapping (that is measured
#   separately by the recorded guard/occlusion outcome).
# baseline: returns 0 once the prepared target and checker are present.
wait_ready_dock_hidden() {
  local deadline=$(( $(date +%s) + READY_TIMEOUT ))
  while [ "$(date +%s)" -lt "$deadline" ]; do
    if [ "$(dock_current)" = "true" ]; then
      local alive=0
      set +e; driver pgrep -x Dock >/dev/null 2>&1; alive=$?; set -e
      if [ "$alive" -eq 0 ]; then return 0; fi
    fi
    sleep "$READY_POLL"
  done
  return 1
}
wait_ready_baseline() {
  local deadline=$(( $(date +%s) + READY_TIMEOUT ))
  while [ "$(date +%s)" -lt "$deadline" ]; do
    [ -s "$PREPARED" ] && [ -x "$CHECKER" ] && return 0
    sleep "$READY_POLL"
  done
  return 1
}

# ---------------------------------------------------------------------------
# Restoration (always runs, on success and on failure, via the EXIT trap).
# We only ever restore a prior state we could read and understand (absent, true,
# or false). If the prior value was unknown (unreadable/invalid) we never
# mutated it, so there is nothing to restore. Restoration is verified by
# re-reading the preference; a failed restoration is represented explicitly
# rather than suppressed.
# ---------------------------------------------------------------------------
g_saved_pref=""
g_changed_pref=0
g_restore_status="none"
g_cond_dir=""
state_restore() {
  if [ "$g_changed_pref" != "1" ]; then
    g_restore_status="none"
    return 0
  fi

  local readback rwrc
  local restore_val=0
  [ "$g_saved_pref" = "true" ] && restore_val=1
  local want=false
  [ "$restore_val" = "1" ] && want=true

  set +e
  if [ "$g_saved_pref" = "absent" ]; then
    dock_delete
    rwrc=$?
  else
    dock_write "$restore_val"
    rwrc=$?
  fi
  # Restart is best-effort; the preference value itself is the observable state.
  dock_restart || true
  readback="$(dock_current)"
  set -e

  if [ "$rwrc" -ne 0 ]; then
    g_restore_status="failed"
    echo "preference restoration write/delete failed for $CONDITION" >&2
    return 1
  fi
  if [ "$readback" != "$g_saved_pref" ]; then
    g_restore_status="failed"
    echo "preference restoration did not verify for $CONDITION" >&2
    return 1
  fi
  [ "$g_saved_pref" = "absent" ] || [ "$readback" = "$want" ] || {
    g_restore_status="failed"
    echo "preference restoration returned an unexpected value for $CONDITION" >&2
    return 1
  }
  g_restore_status="ok"
  # Record the final, verified restoration outcome on the evidence path. This
  # runs in the EXIT trap AFTER main returns, so it reflects real restoration.
  [ -n "$g_cond_dir" ] && printf 'restore_status=%s\n' "$g_restore_status" >> "$g_cond_dir/evidence.txt"
  return 0
}

# ---------------------------------------------------------------------------
# Probe + per-condition validation (closed facts only; no guard change).
# ---------------------------------------------------------------------------
run_probe() {
  local cond_dir="$1"
  local rc
  set +e
  NAN_DESKTOP_OCCLUSION_DIAGNOSTIC="$cond_dir/occlusion.json" \
  ZED_EXPERIMENTAL_A11Y=1 \
  ZED_ALLOW_EMULATED_GPU=1 \
    driver "$CHECKER" run --yes --non-interactive --mode deterministic \
      --session github-hosted --app "$APP" --prepared "$PREPARED" \
      --output "$cond_dir/report.json"
  rc=$?
  set -e
  printf 'probe_exit=%s\n' "$rc" > "$cond_dir/probe-status.txt"
}

validate_artifacts() {
  local cond_dir="$1"
  local report="$cond_dir/report.json"
  local occlusion="$cond_dir/occlusion.json"
  local digest rc

  : > "$cond_dir/uploads.txt"
  if [ -f "$report" ]; then
    set +e
    digest="$(driver "$CHECKER" validate-report "$report" 2>/dev/null)"
    rc=$?
    set -e
    if [ "$rc" -eq 0 ] && [ -n "$digest" ]; then
      printf '%s' "$digest" > "$cond_dir/report.sha256"
      printf 'validated\n' > "$cond_dir/report.status"
      printf '%s\n' "$cond_dir/report.json" >> "$cond_dir/uploads.txt"
    else
      printf 'invalid\n' > "$cond_dir/report.status"
    fi
  else
    printf 'absent\n' > "$cond_dir/report.status"
  fi

  if [ -f "$occlusion" ]; then
    set +e
    digest="$(driver "$CHECKER" validate-occlusion "$occlusion" 2>/dev/null)"
    rc=$?
    set -e
    if [ "$rc" -eq 0 ] && [ -n "$digest" ]; then
      printf '%s' "$digest" > "$cond_dir/occlusion.sha256"
      printf 'validated\n' > "$cond_dir/occlusion.status"
      printf '%s\n' "$cond_dir/occlusion.json" >> "$cond_dir/uploads.txt"
    else
      printf 'invalid\n' > "$cond_dir/occlusion.status"
    fi
  else
    printf 'absent\n' > "$cond_dir/occlusion.status"
  fi
}

record_evidence() {
  local cond_dir="$1"
  local probe_exit
  probe_exit="$(cut -d= -f2 "$cond_dir/probe-status.txt")"
  {
    printf 'source_commit=%s\n' "$SOURCE_COMMIT"
    printf 'condition=%s\n' "$CONDITION"
    printf 'run_url=%s\n' "$RUN_URL"
    printf 'probe_exit=%s\n' "$probe_exit"
    printf 'report=%s\n' "$(cat "$cond_dir/report.status")"
    printf 'occlusion=%s\n' "$(cat "$cond_dir/occlusion.status")"
    if [ -f "$cond_dir/report.sha256" ]; then
      printf 'report_sha256=%s\n' "$(cat "$cond_dir/report.sha256")"
    fi
    if [ -f "$cond_dir/occlusion.sha256" ]; then
      printf 'occlusion_sha256=%s\n' "$(cat "$cond_dir/occlusion.sha256")"
    fi
    printf 'docks_changed=%s\n' "$g_changed_pref"
  } > "$cond_dir/evidence.txt"
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
main() {
  # Identity / preparability checks before touching anything.
  if [ -z "$CHECKER" ]; then
    echo "missing checker identity" >&2; return 4
  fi
  if [ ! -x "$CHECKER" ]; then
    echo "checker is not an executable helper: $CHECKER" >&2; return 4
  fi
  if [ -z "$PREPARED" ]; then
    echo "missing prepared identity" >&2; return 4
  fi
  if [ ! -s "$PREPARED" ]; then
    echo "prepared target is missing or empty: $PREPARED" >&2; return 4
  fi

  # Fail closed on a non-disposable / unsupported host before any Dock change.
  guard_host || return $?

  mkdir -p "$OUTPUT_DIR"
  # Reject a symlinked output directory: evidence must land on a real path we
  # own, never through a symlink that could redirect or be substituted.
  if [ -L "$OUTPUT_DIR" ]; then
    echo "refusing symlinked output directory: $OUTPUT_DIR" >&2; return 3
  fi
  local cond_dir="$OUTPUT_DIR/$CONDITION"
  if [ -L "$cond_dir" ]; then
    echo "refusing symlinked condition directory: $cond_dir" >&2; return 3
  fi
  # Prevent accidental overwrite of prior evidence (per-condition isolation).
  if [ -e "$cond_dir/report.json" ] || [ -e "$cond_dir/occlusion.json" ]; then
    echo "refusing to overwrite existing evidence: $cond_dir" >&2; return 3
  fi
  mkdir -p "$cond_dir"
  g_cond_dir="$cond_dir"

  if [ "$CONDITION" = "dock-hidden" ]; then
    g_saved_pref="$(dock_current)"
    # Only mutate a preference whose prior value we understand and can restore.
    # An unknown prior value (unreadable/invalid) is never guessed or deleted.
    case "$g_saved_pref" in
      absent|true|false) ;;
      *)
        echo "prior Dock preference is $g_saved_pref; refusing to mutate unknown state" >&2
        g_changed_pref=0
        return 3
        ;;
    esac
    g_changed_pref=1
    trap state_restore EXIT
    dock_write true
    dock_restart || true
    if ! wait_ready_dock_hidden; then
      echo "state confirmation failed for dock-hidden" >&2; return 3
    fi
  else
    if ! wait_ready_baseline; then
      echo "baseline readiness failed" >&2; return 2
    fi
  fi

  run_probe "$cond_dir"
  validate_artifacts "$cond_dir"
  record_evidence "$cond_dir"

  # Honest outcome: probes may legitimately be blocked; that is recorded. We
  # fail closed only when the canonical report is absent or schema-invalid.
  local rep_status
  rep_status="$(cat "$cond_dir/report.status")"
  if [ "$rep_status" != "validated" ]; then
    echo "canonical report is $rep_status; evidence preserved, not uploadable" >&2
    return 4
  fi
  return 0
}

main "$@"
