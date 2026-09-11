#!/usr/bin/env bash
# Wave12 disposable macOS environment experiment (prepared, reviewable).
#
# This orchestrator is the LOCAL, reviewable preparation for ONE future
# disposable GitHub-hosted macOS ARM64 run that compares the deterministic
# checker outcome under a "baseline" condition versus a "dock-hidden"
# condition. It must never run against a shared Mac.
#
# Driver backends:
#   fake  Selected ONLY by an explicit `--fake-backend DIR`. Every environment
#         command (Dock preference, Dock process, host identity, checker) is
#         served by a marked fake driver file inside DIR. Fake mode refuses to
#         start when any host environment command is resolvable through PATH,
#         so no route (driver, typo, PATH fallback) can reach the host.
#   real  The default. Refuses before any host fact is read unless the explicit
#         disposable-run opt-in and GitHub-hosted runner signals are present.
#         Only the reviewed absolute-path driver calls are allowed.
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
CHECKER=""
PREPARED=""
APP="zed-desktop"
FAKE_BACKEND=""
READY_TIMEOUT="${WAVE12_READY_TIMEOUT_SECS:-30}"
READY_POLL="${WAVE12_READY_POLL_SECS:-1}"
SOURCE_COMMIT="${SOURCE_COMMIT:-unknown}"
RUN_URL="${RUN_URL:-none}"

while [ $# -gt 0 ]; do
  case "$1" in
    --output-dir) OUTPUT_DIR="${2:?}"; shift 2 ;;
    --checker) CHECKER="${2:?}"; shift 2 ;;
    # Accepted for workflow compatibility; the checker resolves nanh itself.
    --nan-harness) shift 2 ;;
    --prepared) PREPARED="${2:?}"; shift 2 ;;
    --app) APP="${2:?}"; shift 2 ;;
    --fake-backend) FAKE_BACKEND="${2:?}"; shift 2 ;;
    --source-commit) SOURCE_COMMIT="${2:?}"; shift 2 ;;
    --run-url) RUN_URL="${2:?}"; shift 2 ;;
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

# Source identity and run URL use the exact shapes staging publishes, so the
# orchestrator never records evidence that publication would later refuse.
if [ "$SOURCE_COMMIT" != "unknown" ] \
  && ! [[ "$SOURCE_COMMIT" =~ ^[0-9a-f]{40}$ ]]; then
  echo "invalid SOURCE_COMMIT" >&2; exit 1
fi
if [ "$RUN_URL" != "none" ] \
  && ! [[ "$RUN_URL" =~ ^https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/actions/runs/[0-9]+$ ]]; then
  echo "invalid RUN_URL" >&2; exit 1
fi

# ---------------------------------------------------------------------------
# Driver backend selection (before any other action).
# ---------------------------------------------------------------------------
# Environment commands that fake mode must be unable to resolve through PATH.
HOST_COMMANDS="defaults killall pkill pgrep uname sw_vers osascript open launchctl screencapture nanh-desktop-check nan-harness"
FAKE_DRIVERS="defaults killall pgrep uname runner-environment nanh-desktop-check"
FAKE_MARKER="# wave12-fake-driver"

# A fake driver is a regular, non-symlinked, executable bash script whose second
# line is the fake marker. Real host binaries never carry it, so a backend
# directory pointed at /usr/bin or a build output is refused, not executed.
fake_file_ok() {
  local path="$1" shebang="" marker=""
  [ -f "$path" ] && [ ! -L "$path" ] && [ -x "$path" ] || return 1
  { IFS= read -r shebang && IFS= read -r marker; } < "$path" 2>/dev/null || return 1
  [ "$shebang" = "#!/usr/bin/env bash" ] && [ "$marker" = "$FAKE_MARKER" ]
}

# Canonical directory path via builtins only; fails for a missing directory.
canonical_dir() { (cd -P -- "$1" 2>/dev/null && pwd -P); }

select_backend() {
  local name
  if [ -z "$FAKE_BACKEND" ]; then
    # Real drivers run only on a disposable GitHub-hosted runner, and only when
    # a reviewed condition step opts in. Anything else stays quarantined.
    if [ "${WAVE12_REAL_DRIVERS_OPT_IN:-}" != disposable-github-hosted-macos ] \
      || [ "${GITHUB_ACTIONS:-}" != true ] \
      || [ "${RUNNER_ENVIRONMENT:-}" != github-hosted ]; then
      echo 'real environment drivers are disabled pending review' >&2
      exit 78
    fi
    DRIVER_BACKEND=real
    return 0
  fi
  if [ ! -d "$FAKE_BACKEND" ] || [ -L "$FAKE_BACKEND" ]; then
    echo 'fake backend must be a real directory' >&2; exit 78
  fi
  FAKE_BACKEND="$(canonical_dir "$FAKE_BACKEND")" \
    || { echo 'fake backend cannot be resolved' >&2; exit 78; }
  for name in $FAKE_DRIVERS; do
    fake_file_ok "$FAKE_BACKEND/$name" \
      || { echo "fake driver is missing or unmarked: $name" >&2; exit 78; }
  done
  # No host environment command may be reachable by name in fake mode:
  # not through PATH, an alias, or an imported shell function.
  hash -r
  for name in $HOST_COMMANDS; do
    if command -v "$name" >/dev/null 2>&1; then
      echo "host command resolvable in fake mode: $name" >&2; exit 78
    fi
  done
  DRIVER_BACKEND=fake
}

DRIVER_BACKEND=""
select_backend
readonly DRIVER_BACKEND FAKE_BACKEND

# ---------------------------------------------------------------------------
# Drivers. Every environment command goes through `driver`.
# ---------------------------------------------------------------------------
real_driver() {
  # Exact argument shapes only, by absolute path; anything else is refused and
  # never forwarded. Reached only after the opt-in checks in select_backend.
  local name="$1"; shift
  case "$name:$*" in
    "defaults:read com.apple.dock autohide" \
    | "defaults:write com.apple.dock autohide -bool true" \
    | "defaults:write com.apple.dock autohide -bool false" \
    | "defaults:delete com.apple.dock autohide")
      /usr/bin/defaults "$@" ;;
    "killall:Dock") /usr/bin/killall Dock ;;
    "pgrep:-x Dock") /usr/bin/pgrep -x Dock ;;
    "uname:-s"|"uname:-m") /usr/bin/uname "$@" ;;
    "checker:run "*|"checker:validate-report "*|"checker:validate-occlusion "*) "$CHECKER" "$@" ;;
    *) echo 'real driver call is not allowed' >&2; return 78 ;;
  esac
}

fake_driver() {
  local name="$1"; shift
  case "$name" in
    defaults|killall|pgrep|uname|runner-environment) ;;
    checker) name=nanh-desktop-check ;;
    *) echo 'fake driver is not allowed' >&2; return 78 ;;
  esac
  # Re-validate at use: a driver replaced during the run is refused.
  fake_file_ok "$FAKE_BACKEND/$name" \
    || { echo 'fake driver is missing or invalid' >&2; return 78; }
  "$FAKE_BACKEND/$name" "$@"
}

driver() {
  if [ "$DRIVER_BACKEND" = fake ]; then
    fake_driver "$@"
  else
    real_driver "$@"
  fi
}

# ---------------------------------------------------------------------------
# Host gating (fail closed on non-disposable / unsupported host).
# Returns: 0 = ready, 1 = non-disposable, 2 = not Darwin, 3 = not arm64, or
# the driver's own refusal code.
# ---------------------------------------------------------------------------
guard_host() {
  local runner os arch rs
  # Disposability uses the signal the checker's SessionMode::GithubHosted
  # accepts. In fake mode it comes from the fake backend, never from runner
  # variables, so fake runs need no spoofed GitHub environment.
  if [ "$DRIVER_BACKEND" = fake ]; then
    set +e; runner="$(driver runner-environment)"; rs=$?; set -e
    [ "$rs" -eq 0 ] || return "$rs"
  else
    [ "${GITHUB_ACTIONS:-}" = true ] || return 1
    runner="${RUNNER_ENVIRONMENT:-}"
  fi
  [ "$runner" = github-hosted ] || return 1
  set +e; os="$(driver uname -s)"; rs=$?; set -e
  [ "$rs" -eq 0 ] || return "$rs"
  set +e; arch="$(driver uname -m)"; rs=$?; set -e
  [ "$rs" -eq 0 ] || return "$rs"
  [ "$os" = Darwin ] || return 2
  [ "$arch" = arm64 ] || return 3
  return 0
}

# ---------------------------------------------------------------------------
# Dock preference primitives (macOS user-defaults behaviour).
# ---------------------------------------------------------------------------
# Read the Dock auto-hide preference. Emits one of:
#   absent      `defaults` reported that the key does not exist
#   true|false  the key holds a recognised boolean literal
#   unreadable  `defaults` failed for another reason: the prior value is UNKNOWN
#   invalid     the key exists but holds a value we do not understand
# Only absent/true/false are ever mutated, because only they can be restored
# and verified; an unknown prior value is never deleted or overwritten.
dock_current() {
  local out rc
  set +e
  out="$(driver defaults read com.apple.dock autohide 2>&1)"
  rc=$?
  set -e
  if [ "$rc" -ne 0 ]; then
    case "$out" in
      *"does not exist"*) echo absent ;;
      *) echo unreadable ;;
    esac
    return 0
  fi
  case "$out" in
    1|true|TRUE|True) echo true ;;
    0|false|FALSE|False) echo false ;;
    *) echo invalid ;;
  esac
}
dock_write() { driver defaults write com.apple.dock autohide -bool "$1"; }
dock_delete() { driver defaults delete com.apple.dock autohide; }
dock_restart() { driver killall Dock; }

# Bounded readiness that observes the intended STATE, not a fixed sleep.
# dock-hidden: the preference reads back true AND the Dock process is alive.
#   This confirms the intervention was applied; it does NOT claim the Dock
#   stopped overlapping (that is measured by the recorded occlusion outcome).
# baseline: the prepared target and checker are present.
wait_ready_dock_hidden() {
  local deadline=$(( SECONDS + READY_TIMEOUT ))
  while [ "$SECONDS" -lt "$deadline" ]; do
    if [ "$(dock_current)" = true ] && driver pgrep -x Dock >/dev/null 2>&1; then
      return 0
    fi
    sleep "$READY_POLL"
  done
  return 1
}
wait_ready_baseline() {
  local deadline=$(( SECONDS + READY_TIMEOUT ))
  while [ "$SECONDS" -lt "$deadline" ]; do
    [ -s "$PREPARED" ] && [ -x "$CHECKER" ] && return 0
    sleep "$READY_POLL"
  done
  return 1
}

# ---------------------------------------------------------------------------
# Per-run state, restoration and evidence (written once, from the EXIT trap).
# ---------------------------------------------------------------------------
g_cond_dir=""
g_prior_state=none
g_changed_pref=0
g_restore_status=none
g_probe_exit=not-run
g_report=absent
g_report_sha=""
g_occlusion=absent
g_occlusion_sha=""

# Restore the exact prior value and verify it by reading it back. A failed
# write/delete or a mismatching read-back is recorded as `failed`, never
# suppressed. Returns nonzero on failure.
state_restore() {
  local rc readback
  [ "$g_changed_pref" = 1 ] || { g_restore_status=none; return 0; }
  set +e
  if [ "$g_prior_state" = absent ]; then
    dock_delete
  else
    dock_write "$g_prior_state"
  fi
  rc=$?
  # Restarting the Dock only applies the value; the value is what we verify.
  dock_restart >/dev/null 2>&1
  set -e
  readback="$(dock_current)"
  if [ "$rc" -ne 0 ] || [ "$readback" != "$g_prior_state" ]; then
    g_restore_status=failed
    echo "Dock preference restoration did not verify for $CONDITION" >&2
    return 1
  fi
  g_restore_status=ok
}

record_evidence() {
  local dir="$1"
  printf 'probe_exit=%s\n' "$g_probe_exit" > "$dir/probe-status.txt"
  {
    printf 'source_commit=%s\n' "$SOURCE_COMMIT"
    printf 'condition=%s\n' "$CONDITION"
    printf 'run_url=%s\n' "$RUN_URL"
    printf 'probe_exit=%s\n' "$g_probe_exit"
    printf 'report=%s\n' "$g_report"
    printf 'occlusion=%s\n' "$g_occlusion"
    [ -z "$g_report_sha" ] || printf 'report_sha256=%s\n' "$g_report_sha"
    [ -z "$g_occlusion_sha" ] || printf 'occlusion_sha256=%s\n' "$g_occlusion_sha"
    printf 'docks_changed=%s\n' "$g_changed_pref"
    printf 'prior_state=%s\n' "$g_prior_state"
    printf 'restore_status=%s\n' "$g_restore_status"
  } > "$dir/evidence.txt"
}

# EXIT handler: restore first, then record the final closed facts. A failed
# restoration overrides every other outcome with exit 5.
finish() {
  local rc=$?
  trap - EXIT
  set +e
  if ! state_restore; then
    rc=5
  fi
  if [ -n "$g_cond_dir" ] && ! record_evidence "$g_cond_dir"; then
    echo "evidence could not be recorded for $CONDITION" >&2
    [ "$rc" -ne 0 ] || rc=4
  fi
  exit "$rc"
}
trap finish EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

# ---------------------------------------------------------------------------
# Probe + per-condition validation (closed facts only; no guard change).
# ---------------------------------------------------------------------------
run_probe() {
  local cond_dir="$1" rc
  set +e
  NAN_DESKTOP_OCCLUSION_DIAGNOSTIC="$cond_dir/occlusion.json" \
  ZED_EXPERIMENTAL_A11Y=1 \
  ZED_ALLOW_EMULATED_GPU=1 \
    driver checker run --yes --non-interactive --mode deterministic \
      --session github-hosted --app "$APP" --prepared "$PREPARED" \
      --output "$cond_dir/report.json"
  rc=$?
  set -e
  g_probe_exit="$rc"
}

# Validate one artifact with the checker; emits the digest on success.
validated_digest() {
  local kind="$1" file="$2" digest rc
  [ -f "$file" ] || { echo absent; return 0; }
  set +e
  digest="$(driver checker "validate-$kind" "$file" 2>/dev/null)"
  rc=$?
  set -e
  if [ "$rc" -eq 0 ] && [[ "$digest" =~ ^[0-9a-f]{64}$ ]]; then
    printf '%s' "$digest" > "${file%.json}.sha256"
    echo "validated $digest"
  else
    echo invalid
  fi
}

validate_artifacts() {
  local cond_dir="$1" result
  result="$(validated_digest report "$cond_dir/report.json")"
  g_report="${result%% *}"
  [ "$g_report" != validated ] || g_report_sha="${result#* }"
  result="$(validated_digest occlusion "$cond_dir/occlusion.json")"
  g_occlusion="${result%% *}"
  [ "$g_occlusion" != validated ] || g_occlusion_sha="${result#* }"
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
main() {
  local cond_dir
  # Identity / preparability checks before touching anything.
  case "$CHECKER" in
    /*) ;;
    *) echo "checker must be an absolute path" >&2; return 4 ;;
  esac
  if [ ! -x "$CHECKER" ]; then
    echo "checker is not an executable helper" >&2; return 4
  fi
  # In fake mode the only executable checker is the backend's fake checker.
  if [ "$DRIVER_BACKEND" = fake ] \
    && [ "$(canonical_dir "${CHECKER%/*}")/${CHECKER##*/}" != "$FAKE_BACKEND/nanh-desktop-check" ]; then
    echo "checker is not the fake backend checker" >&2; return 4
  fi
  if [ -z "$PREPARED" ] || [ ! -s "$PREPARED" ]; then
    echo "prepared target is missing or empty" >&2; return 4
  fi

  # Fail closed on a non-disposable / unsupported host before any Dock change.
  guard_host || return $?

  mkdir -p "$OUTPUT_DIR"
  # Evidence must land on a real path we own, never through a symlink.
  if [ -L "$OUTPUT_DIR" ]; then
    echo "refusing symlinked output directory" >&2; return 3
  fi
  cond_dir="$OUTPUT_DIR/$CONDITION"
  # Per-condition isolation: never reuse or overwrite earlier evidence.
  if [ -e "$cond_dir" ] || [ -L "$cond_dir" ]; then
    echo "refusing to reuse an existing condition directory" >&2; return 3
  fi
  mkdir "$cond_dir"
  g_cond_dir="$cond_dir"

  if [ "$CONDITION" = dock-hidden ]; then
    g_prior_state="$(dock_current)"
    case "$g_prior_state" in
      absent|true|false) ;;
      *)
        echo "prior Dock preference is $g_prior_state; refusing to mutate unknown state" >&2
        return 3
        ;;
    esac
    # Marked before the write: a partially applied write is still restored.
    g_changed_pref=1
    dock_write true || { echo "Dock preference write failed" >&2; return 3; }
    dock_restart >/dev/null 2>&1 || true
    if ! wait_ready_dock_hidden; then
      echo "state confirmation failed for dock-hidden" >&2; return 3
    fi
  elif ! wait_ready_baseline; then
    echo "baseline readiness failed" >&2; return 2
  fi

  run_probe "$cond_dir"
  validate_artifacts "$cond_dir"

  # Honest outcome: probes may legitimately be blocked; that is recorded. We
  # fail closed only when the canonical report is absent or schema-invalid.
  if [ "$g_report" != validated ]; then
    echo "canonical report is $g_report; evidence preserved, not uploadable" >&2
    return 4
  fi
  return 0
}

main "$@"
