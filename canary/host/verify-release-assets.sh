#!/usr/bin/env bash
set -euo pipefail
umask 077

usage() {
  printf 'usage: %s --release-tag <tag> --assets-dir <directory>\n' "$0" >&2
  exit 2
}

release_tag=''
assets_directory=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --release-tag) release_tag="${2:-}"; shift 2 ;;
    --assets-dir) assets_directory="${2:-}"; shift 2 ;;
    *) usage ;;
  esac
done
[ -n "$release_tag" ] && [ -d "$assets_directory" ] || usage

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repository_root/canary/host/lib.sh"
release_repository='DavidLMS/nan-harness'
checksum_manifest="$assets_directory/SHA256SUMS"
required_assets=(
  nan-harness-aarch64-unknown-linux-musl
  nan-harness-canary-aarch64-unknown-linux-musl
  nan-harness-aarch64-apple-darwin
  nan-harness-canary-aarch64-apple-darwin
)

retry 4 5 gh release download "$release_tag" \
  --repo "$release_repository" \
  --pattern SHA256SUMS \
  --dir "$assets_directory" --clobber
[ -f "$checksum_manifest" ] || {
  printf 'release checksum metadata was not downloaded for %s\n' "$release_tag" >&2
  exit 1
}

gh attestation verify "$checksum_manifest" \
  --repo "$release_repository" \
  --signer-workflow .github/workflows/release.yml \
  --source-ref "refs/tags/$release_tag" >/dev/null

checksum_for() {
  local asset="$1"
  local matches
  matches="$(awk -v asset="$asset" '$2 == asset { print $1 }' "$checksum_manifest")"
  [ "$(printf '%s\n' "$matches" | sed '/^$/d' | wc -l | tr -d ' ')" -eq 1 ] || {
    printf 'checksum metadata must contain exactly one entry for %s\n' "$asset" >&2
    return 1
  }
  case "$matches" in
    ''|*[!0123456789abcdefABCDEF]*)
      printf 'checksum metadata for %s is not a SHA-256 digest\n' "$asset" >&2
      return 1
      ;;
  esac
  [ "${#matches}" -eq 64 ] || {
    printf 'checksum metadata for %s is not a SHA-256 digest\n' "$asset" >&2
    return 1
  }
  printf '%s\n' "$matches"
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

for asset in "${required_assets[@]}"; do
  asset_path="$assets_directory/$asset"
  [ -f "$asset_path" ] || {
    printf 'required release asset is missing: %s\n' "$asset" >&2
    exit 1
  }
  expected="$(checksum_for "$asset")"
  actual="$(sha256_file "$asset_path")"
  if [ "$actual" != "$expected" ]; then
    printf 'release asset checksum mismatch for %s\n' "$asset" >&2
    exit 1
  fi
done
