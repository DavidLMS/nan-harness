#!/usr/bin/env bash
set -euo pipefail
umask 077

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  printf 'usage: %s <harness-id> [linux|macos]\n' "$0" >&2
  exit 2
fi

harness="$1"
guest="${2:-linux}"
case "$guest" in
  linux|macos) ;;
  *) printf 'guest must be linux or macos\n' >&2; exit 2 ;;
esac

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repository_root/canary/host/lib.sh"
state_directory="${NAN_CANARY_STATE_DIR:-$HOME/Library/Application Support/nan-harness-canary}"
tag="$(gh release view --json tagName --jq '.tagName')"
version="${tag#v}"
assets="$state_directory/assets/$tag"
output="$state_directory/runs/$(date -u +%Y%m%dT%H%M%SZ)-manual-$guest-$harness"
mkdir -p "$assets" "$output"

patterns=(
  --pattern nan-harness-aarch64-unknown-linux-musl
  --pattern nan-harness-canary-aarch64-unknown-linux-musl
  --pattern nan-harness-canary-aarch64-apple-darwin
)
arguments=(
  --trigger manual
  --nan-harness-version "$version"
  --linux-binary "$assets/nan-harness-aarch64-unknown-linux-musl"
  --linux-canary-binary "$assets/nan-harness-canary-aarch64-unknown-linux-musl"
  --macos-canary-binary "$assets/nan-harness-canary-aarch64-apple-darwin"
  --output-dir "$output"
  --release-tag "$tag"
  --harness "$harness"
  --guest "$guest"
)
if [ "$guest" = macos ]; then
  patterns+=(--pattern nan-harness-aarch64-apple-darwin)
  arguments+=(--macos-binary "$assets/nan-harness-aarch64-apple-darwin")
fi

retry 4 5 gh release download "$tag" "${patterns[@]}" --dir "$assets" --clobber
"$repository_root/canary/host/run-suite.sh" "${arguments[@]}"
