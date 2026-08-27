#!/usr/bin/env bash
set -euo pipefail
umask 077

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
state_root="${NAN_CANARY_STATE_DIR:-$HOME/Library/Application Support/nan-harness-canary}"
agents="$HOME/Library/LaunchAgents"
ntfy_url="${NAN_CANARY_NTFY_URL:-}"
mkdir -p "$state_root" "$agents"

xml_escape() {
  local value="$1"
  value="${value//&/\\&amp;}"
  value="${value//</\\&lt;}"
  value="${value//>/\\&gt;}"
  value="${value//|/\\|}"
  printf '%s' "$value"
}

repository_xml="$(xml_escape "$repository_root")"
state_xml="$(xml_escape "$state_root")"
ntfy_xml="$(xml_escape "$ntfy_url")"

launchctl bootout "gui/$(id -u)/dev.nan-harness.release-gate" 2>/dev/null || true
rm -f "$agents/dev.nan-harness.release-gate.plist"

for label in dev.nan-harness.canary-daily dev.nan-harness.canary-weekly; do
  template="$repository_root/canary/launchd/$label.plist.in"
  destination="$agents/$label.plist"
  sed \
    -e "s|__REPOSITORY_ROOT__|$repository_xml|g" \
    -e "s|__STATE_ROOT__|$state_xml|g" \
    -e "s|__NTFY_URL__|$ntfy_xml|g" \
    "$template" >"$destination"
  plutil -lint "$destination"
  launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
  launchctl bootstrap "gui/$(id -u)" "$destination"
done
