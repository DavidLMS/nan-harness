#!/usr/bin/env bash
set -euo pipefail

title="${1:-}"
message="${2:-}"
if [ -z "${NAN_CANARY_NTFY_URL:-}" ] || [ -z "$title" ] || [ -z "$message" ]; then
  exit 0
fi
token="$(/usr/bin/security find-generic-password -s dev.nan-harness.canary -a NTFY_TOKEN -w 2>/dev/null || true)"
if [ -z "$token" ]; then
  exit 0
fi
curl --fail --silent --show-error \
  --connect-timeout 10 --max-time 30 \
  --header "Title: $title" \
  --header "Tags: test_tube" \
  --data-binary "$message" \
  --config - \
  "$NAN_CANARY_NTFY_URL" >/dev/null <<EOF
header = "Authorization: Bearer $token"
EOF
