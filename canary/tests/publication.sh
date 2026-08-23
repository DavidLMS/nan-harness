#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
reports_directory="$temporary_directory/reports"
state_directory="$temporary_directory/state"
output_directory="$temporary_directory/output"
bin_directory="$temporary_directory/bin"
mkdir -p "$reports_directory" "$state_directory" "$output_directory" "$bin_directory"

printf '%s\n' '#!/usr/bin/env bash' \
  'printf "%s\\n" "$*" >> "$GH_LOG"' \
  'if [ "$1" = release ] && [ "$2" = download ]; then' \
  '  while [ "$#" -gt 0 ]; do' \
  '    if [ "$1" = --output ]; then cp "$TEST_BASE" "$2"; fi' \
  '    shift' \
  '  done' \
  '  exit 0' \
  'fi' \
  'exit 1' >"$bin_directory/gh"
chmod 755 "$bin_directory/gh"

cat >"$reports_directory/linux-claude-code.json" <<'EOF'
{
  "schemaVersion": 1,
  "trigger": "daily",
  "tier": "deterministic",
  "nanHarness": {"version": "0.0.6"},
  "harness": {"id": "claude-code", "version": "9.9.9"},
  "completedAt": "2026-08-23T10:00:00Z",
  "checks": [
    {"name": "install-and-diagnose", "status": "passed"},
    {"name": "deterministic-conformance", "status": "passed"}
  ]
}
EOF
cat >"$reports_directory/linux-codex.json" <<'EOF'
{
  "schemaVersion": 1,
  "trigger": "daily",
  "tier": "deterministic",
  "nanHarness": {"version": "0.0.6"},
  "harness": {"id": "codex", "version": "99.0.0"},
  "completedAt": "2026-08-23T10:00:00Z",
  "checks": [
    {"name": "install-and-diagnose", "status": "passed"},
    {"name": "deterministic-conformance", "status": "failed"}
  ]
}
EOF

cargo xtask compatibility-feed "$temporary_directory/base.json" >/dev/null
GH_LOG="$temporary_directory/gh.log" TEST_BASE="$temporary_directory/base.json" \
  PATH="$bin_directory:$PATH" \
  "$repository_root/canary/host/publish-compatibility.sh" \
  --trigger daily \
  --nan-harness-version 0.0.6 \
  --release-tag v0.0.6 \
  --reports "$reports_directory" \
  --output-dir "$output_directory" \
  --state-dir "$state_directory"

jq -e '.schemaVersion == 2 and ([.releases[] | select(.nanHarnessVersion == "0.0.6") | .verifications[] | select(.id == "claude-code")][0].lastCompatibleVersion == "9.9.9") and ([.releases[] | select(.nanHarnessVersion == "0.0.6") | .verifications[] | select(.id == "codex")][0].lastCompatibleVersion == "0.146.0")' "$output_directory/compatibility.json" >/dev/null
if grep -Eq '(^| )(upload|create)( |$)' "$temporary_directory/gh.log"; then
  exit 1
fi

old_feed="$temporary_directory/old-feed.json"
old_output="$temporary_directory/old-output"
old_state="$temporary_directory/old-state"
printf '%s\n' '{"schemaVersion":1,"harnesses":[]}' >"$old_feed"
mkdir -p "$old_output" "$old_state"
GH_LOG="$temporary_directory/gh.log" TEST_BASE="$old_feed" \
  PATH="$bin_directory:$PATH" \
  "$repository_root/canary/host/publish-compatibility.sh" \
  --trigger daily \
  --nan-harness-version 0.0.6 \
  --release-tag v0.0.6 \
  --reports "$reports_directory" \
  --output-dir "$old_output" \
  --state-dir "$old_state"
jq -e '.schemaVersion == 2 and (.releases | length == 1) and (.releases[0].verifications | length == 1) and .releases[0].verifications[0].id == "claude-code"' "$old_output/compatibility.json" >/dev/null

grep -F -- '--linux-canary-binary' "$repository_root/canary/host/run-suite.sh" >/dev/null
grep -F -- '--pattern nan-harness-canary-aarch64-unknown-linux-musl' "$repository_root/canary/host/run-scheduled.sh" >/dev/null
grep -F -- '--pattern nan-harness-canary-aarch64-apple-darwin' "$repository_root/canary/host/run-release-gate.sh" >/dev/null
grep -F -- '--publish-feed' "$repository_root/canary/host/run-scheduled.sh" >/dev/null
grep -F -- '--publish-feed' "$repository_root/canary/host/run-release-gate.sh" >/dev/null
if grep -F -- '--publish-feed' "$repository_root/canary/host/run-manual.sh" >/dev/null; then
  exit 1
fi
