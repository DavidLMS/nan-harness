#!/usr/bin/env bash
set -euo pipefail

require_schedules=false
if [ "${1:-}" = --require-schedules ] && [ "$#" -eq 1 ]; then
  require_schedules=true
elif [ "$#" -ne 0 ]; then
  printf 'usage: %s [--require-schedules]\n' "$0" >&2
  exit 2
fi

state_directory="${NAN_CANARY_STATE_DIR:-$HOME/Library/Application Support/nan-harness-canary}"
minimum_free_gb="${NAN_CANARY_MIN_FREE_GB:-}"
if [ -z "$minimum_free_gb" ]; then
  tart_inventory="$(tart list 2>/dev/null || true)"
  if grep -Fq 'ghcr.io/cirruslabs/ubuntu' <<<"$tart_inventory" \
    && grep -Fq 'ghcr.io/cirruslabs/macos-tahoe-base' <<<"$tart_inventory"; then
    minimum_free_gb=50
  else
    minimum_free_gb=100
  fi
fi
case "$minimum_free_gb" in
  ''|*[!0-9]*) printf 'NAN_CANARY_MIN_FREE_GB must be a non-negative integer\n' >&2; exit 2 ;;
esac
failures=0
security_command="${NAN_CANARY_SECURITY_COMMAND:-/usr/bin/security}"
launchctl_command="${NAN_CANARY_LAUNCHCTL_COMMAND:-launchctl}"

check_command() {
  if command -v "$1" >/dev/null 2>&1; then
    printf 'ok   command %s\n' "$1"
  else
    printf 'fail command %s\n' "$1" >&2
    failures=$((failures + 1))
  fi
}

for command in gh jq tart sshpass shlock curl; do
  check_command "$command"
done
if gh auth status >/dev/null 2>&1; then
  printf 'ok   GitHub authentication\n'
else
  printf 'fail GitHub authentication\n' >&2
  failures=$((failures + 1))
fi
if "$security_command" find-generic-password -s dev.nan-harness.canary -a NAN_API_KEY >/dev/null 2>&1; then
  printf 'ok   NAN_API_KEY Keychain item\n'
else
  printf 'fail NAN_API_KEY Keychain item\n' >&2
  failures=$((failures + 1))
fi
if [ -n "${NAN_CANARY_NTFY_URL:-}" ]; then
  if "$security_command" find-generic-password -s dev.nan-harness.canary -a NTFY_TOKEN >/dev/null 2>&1; then
    printf 'ok   ntfy URL and Keychain token\n'
  else
    printf 'fail ntfy URL is configured but its Keychain token is missing\n' >&2
    failures=$((failures + 1))
  fi
else
  printf 'warn ntfy URL is not configured in this shell\n'
fi

mkdir -p "$state_directory"
available_kb="$(df -Pk "$state_directory" | awk 'NR == 2 {print $4}')"
required_kb="$((minimum_free_gb * 1024 * 1024))"
if [ "$available_kb" -ge "$required_kb" ]; then
  printf 'ok   disk space %s GiB available (%s GiB required)\n' \
    "$((available_kb / 1024 / 1024))" "$minimum_free_gb"
else
  printf 'fail disk space %s GiB available; %s GiB required\n' \
    "$((available_kb / 1024 / 1024))" "$minimum_free_gb" >&2
  failures=$((failures + 1))
fi

for label in dev.nan-harness.canary-daily dev.nan-harness.canary-weekly; do
  if "$launchctl_command" print "gui/$(id -u)/$label" >/dev/null 2>&1; then
    printf 'ok   launch agent %s\n' "$label"
  elif [ "$require_schedules" = true ]; then
    printf 'fail launch agent %s is not loaded\n' "$label" >&2
    failures=$((failures + 1))
  else
    printf 'warn launch agent %s is not loaded\n' "$label"
  fi
done
if "$launchctl_command" print "gui/$(id -u)/dev.nan-harness.release-gate" >/dev/null 2>&1; then
  printf 'fail obsolete periodic release gate is loaded\n' >&2
  failures=$((failures + 1))
else
  printf 'ok   no periodic release gate\n'
fi

last_run="$(find "$state_directory/runs" -mindepth 1 -maxdepth 1 -type d -print 2>/dev/null | sort | tail -n 1)"
if [ -n "$last_run" ]; then
  printf 'info last run %s\n' "$(basename "$last_run")"
else
  printf 'info no previous run found\n'
fi
exit "$failures"
