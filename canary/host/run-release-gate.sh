#!/usr/bin/env bash
set -euo pipefail
umask 077

force=false
if [ "${1:-}" = --force ]; then
  force=true
  shift
fi
if [ "$#" -ne 0 ]; then
  printf 'usage: %s [--force]\n' "$0" >&2
  exit 2
fi

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repository_root/canary/host/lib.sh"
state_directory="${NAN_CANARY_STATE_DIR:-$HOME/Library/Application Support/nan-harness-canary}"
tag="$(gh release list --json tagName,isDraft --limit 20 --jq '[.[] | select(.isDraft)][0].tagName // empty')"
if [ -z "$tag" ]; then
  exit 0
fi
attempt_marker="$state_directory/release-gate-$tag.attempted"
cooldown_seconds="${NAN_CANARY_RELEASE_RETRY_SECONDS:-21600}"
case "$cooldown_seconds" in
  ''|*[!0-9]*)
    printf 'NAN_CANARY_RELEASE_RETRY_SECONDS must be a non-negative integer\n' >&2
    exit 2
    ;;
esac
if [ "$force" = false ] && [ -f "$attempt_marker" ]; then
  marker_age="$(( $(date +%s) - $(stat -f %m "$attempt_marker") ))"
  if [ "$marker_age" -lt "$cooldown_seconds" ]; then
    exit 0
  fi
fi
version="${tag#v}"
assets="$state_directory/assets/$tag"
output="$state_directory/runs/$(date -u +%Y%m%dT%H%M%SZ)-release"
mkdir -p "$assets" "$output"
retry 4 5 gh release download "$tag" \
  --pattern nan-harness-aarch64-unknown-linux-musl \
  --pattern nan-harness-aarch64-apple-darwin \
  --dir "$assets" --clobber
touch "$attempt_marker"
"$repository_root/canary/host/run-suite.sh" \
  --trigger release \
  --nan-harness-version "$version" \
  --linux-binary "$assets/nan-harness-aarch64-unknown-linux-musl" \
  --macos-binary "$assets/nan-harness-aarch64-apple-darwin" \
  --output-dir "$output" \
  --release-tag "$tag" \
  --promote
rm -f "$attempt_marker"
