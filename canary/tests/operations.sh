#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
home_directory="$temporary_directory/home"
bin_directory="$temporary_directory/bin"
state_directory="$temporary_directory/state"
mkdir -p "$home_directory" "$bin_directory" "$state_directory"

cat >"$bin_directory/launchctl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$LAUNCHCTL_LOG"
case "$*" in
  *dev.nan-harness.release-gate*) exit 1 ;;
  *) exit 0 ;;
esac
EOF
cat >"$bin_directory/plutil" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat >"$bin_directory/security" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat >"$bin_directory/tool" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod 755 "$bin_directory"/*
for tool in gh jq tart sshpass shlock curl; do
  ln -s tool "$bin_directory/$tool"
done

repository_link="$temporary_directory/repository-link"
ln -s "$repository_root" "$repository_link"
LAUNCHCTL_LOG="$temporary_directory/launchctl.log" \
HOME="$home_directory" \
NAN_CANARY_STATE_DIR="$state_directory" \
NAN_CANARY_NTFY_URL='https://ntfy.example.test/private-topic' \
PATH="$bin_directory:$PATH" \
  "$repository_link/canary/host/install-launchd.sh"

daily="$home_directory/Library/LaunchAgents/dev.nan-harness.canary-daily.plist"
weekly="$home_directory/Library/LaunchAgents/dev.nan-harness.canary-weekly.plist"
[ -f "$daily" ] && [ -f "$weekly" ]
[ ! -f "$home_directory/Library/LaunchAgents/dev.nan-harness.release-gate.plist" ]
[ "$(grep -c '<key>Weekday</key>' "$daily")" -eq 6 ]
[ "$(grep -c '<key>Weekday</key>' "$weekly")" -eq 1 ]
grep -Fq 'https://ntfy.example.test/private-topic' "$daily"
grep -Fq "$repository_root/canary/host/run-scheduled.sh" "$daily"
if grep -Fq "$repository_link" "$daily"; then
  printf 'launchd configuration retained the symlinked repository path\n' >&2
  exit 1
fi
grep -Fq 'bootout gui/' "$temporary_directory/launchctl.log"

mkdir -p \
  "$state_directory/runs/old/run" \
  "$state_directory/runs/old/private-logs" \
  "$state_directory/runs/old/reports" \
  "$state_directory/runs/kept/private-logs" \
  "$state_directory/assets/v1.0.0" \
  "$state_directory/assets/v1.0.1" \
  "$state_directory/assets/v1.0.2" \
  "$state_directory/assets/v1.0.3"
touch "$state_directory/runs/kept/KEEP"
touch -t 202001010000 "$state_directory/runs/old" "$state_directory/runs/old/run" "$state_directory/runs/old/private-logs"
touch -t 202001010000 "$state_directory/runs/kept" "$state_directory/runs/kept/private-logs"
touch -t 202001010000 "$state_directory/assets/v1.0.0"
touch -t 202002010000 "$state_directory/assets/v1.0.1"
touch -t 202003010000 "$state_directory/assets/v1.0.2"
touch -t 202004010000 "$state_directory/assets/v1.0.3"
HOME="$home_directory" NAN_CANARY_STATE_DIR="$state_directory" \
  "$repository_root/canary/host/prune-state.sh"
[ ! -d "$state_directory/runs/old" ]
[ -d "$state_directory/runs/kept/private-logs" ]
[ ! -d "$state_directory/assets/v1.0.0" ]
[ -d "$state_directory/assets/v1.0.1" ]
[ -d "$state_directory/assets/v1.0.2" ]
[ -d "$state_directory/assets/v1.0.3" ]

LAUNCHCTL_LOG="$temporary_directory/preflight-launchctl.log" \
HOME="$home_directory" \
NAN_CANARY_STATE_DIR="$state_directory" \
NAN_CANARY_NTFY_URL='https://ntfy.example.test/private-topic' \
NAN_CANARY_MIN_FREE_GB=0 \
NAN_CANARY_SECURITY_COMMAND="$bin_directory/security" \
NAN_CANARY_LAUNCHCTL_COMMAND="$bin_directory/launchctl" \
PATH="$bin_directory:$PATH" \
  "$repository_root/canary/host/preflight.sh" --require-schedules >/dev/null
