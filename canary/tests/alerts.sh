#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
bin_directory="$temporary_directory/bin"
gh_log="$temporary_directory/gh.log"
notify_log="$temporary_directory/notify.log"
summary="$temporary_directory/summary.json"
mkdir -p "$bin_directory"

cat >"$bin_directory/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$GH_LOG"
if [ "${1:-}" = issue ] && [ "${2:-}" = list ] && [ "${GH_EXISTING_ISSUE:-}" = 1 ]; then
  printf '42\n'
fi
EOF
cat >"$bin_directory/notify" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\t%s\n' "$1" "$2" >>"$NOTIFY_LOG"
EOF
chmod 755 "$bin_directory/gh" "$bin_directory/notify"

run_alerts() {
  GH_LOG="$gh_log" NOTIFY_LOG="$notify_log" \
  NAN_CANARY_NOTIFY_COMMAND="$bin_directory/notify" \
  PATH="$bin_directory:$PATH" \
    "$repository_root/canary/host/publish-alerts.sh" "$summary"
}

printf '%s\n' '{"alerts":[{"subject":"inventory-drift","kind":"suspected","harness":"hermes","harnessVersion":"0.21.0","tier":"deterministic","scenario":"clean-install","consecutiveOccurrences":1,"fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]}' >"$summary"
run_alerts
[ "$(wc -l <"$notify_log" | tr -d ' ')" = 1 ]
[ ! -e "$gh_log" ]

printf '%s\n' '{"alerts":[{"subject":"inventory-drift","kind":"confirmed","harness":"hermes","harnessVersion":"0.21.0","tier":"deterministic","scenario":"clean-install","consecutiveOccurrences":2,"fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]}' >"$summary"
run_alerts
grep -Fq 'issue create' "$gh_log"
grep -Fq '[canary] hermes inventory drift' "$gh_log"
[ "$(wc -l <"$notify_log" | tr -d ' ')" = 2 ]

rm -f "$gh_log" "$notify_log"
printf '%s\n' '{"alerts":[{"subject":"inventory-drift","kind":"recovered","harness":"hermes","harnessVersion":"0.21.0","tier":"deterministic","scenario":"clean-install","consecutiveOccurrences":2,"fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]}' >"$summary"
GH_EXISTING_ISSUE=1 run_alerts
grep -Fq 'issue comment 42' "$gh_log"
grep -Fq 'issue close 42' "$gh_log"
[ "$(wc -l <"$notify_log" | tr -d ' ')" = 1 ]

rm -f "$gh_log" "$notify_log"
printf '%s\n' '{"alerts":[{"subject":"compatibility","kind":"suspected","harness":"hermes","harnessVersion":"0.21.0","tier":"deterministic","scenario":"clean-install","consecutiveOccurrences":1,"failureClass":"harness","fingerprint":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}]}' >"$summary"
run_alerts
[ ! -e "$gh_log" ]
[ ! -e "$notify_log" ]
