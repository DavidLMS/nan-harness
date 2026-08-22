#!/usr/bin/env bash
set -euo pipefail
umask 077

trigger="${1:-}"
case "$trigger" in
  daily|weekly) ;;
  *) printf 'usage: %s <daily|weekly>\n' "$0" >&2; exit 2 ;;
esac

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repository_root/canary/host/lib.sh"
state_directory="${NAN_CANARY_STATE_DIR:-$HOME/Library/Application Support/nan-harness-canary}"
tag="$(gh release view --json tagName --jq '.tagName')"
version="${tag#v}"
assets="$state_directory/assets/$tag"
output="$state_directory/runs/$(date -u +%Y%m%dT%H%M%SZ)-$trigger"
mkdir -p "$assets" "$output"
retry 4 5 gh release download "$tag" \
  --pattern nan-harness-aarch64-unknown-linux-musl \
  --pattern nan-harness-aarch64-apple-darwin \
  --dir "$assets" --clobber

arguments=(
  --trigger "$trigger"
  --nan-harness-version "$version"
  --linux-binary "$assets/nan-harness-aarch64-unknown-linux-musl"
  --output-dir "$output"
  --release-tag "$tag"
)
if [ "$trigger" = weekly ]; then
  arguments+=(--macos-binary "$assets/nan-harness-aarch64-apple-darwin")
fi
"$repository_root/canary/host/run-suite.sh" "${arguments[@]}"
