#!/usr/bin/env bash
set -euo pipefail
umask 077

# Called only by the serialized publication worker, after report approval and
# conversion by the trusted checker. Neither report data nor release code runs.
usage() {
  printf 'usage: %s --report <file> --updates <directory> --repository <owner/name>\n' "$0" >&2
  exit 2
}
report=''
updates_directory=''
release_repository=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --report) report="${2:-}"; shift 2 ;;
    --updates) updates_directory="${2:-}"; shift 2 ;;
    --repository) release_repository="${2:-}"; shift 2 ;;
    *) usage ;;
  esac
done
[ -f "$report" ] && [ -d "$updates_directory" ] || usage
[[ "$release_repository" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || usage
[[ "${release_repository#*/}" != . && "${release_repository#*/}" != .. ]] || usage
repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repository_root/canary/host/lib.sh"
source "$repository_root/canary/host/release-channel.sh"
source "$repository_root/canary/host/compatibility-publication.sh"
source "$repository_root/canary/host/compatibility-versioned.sh"
source "$repository_root/canary/host/publication-writer.sh"
require_publication_writer "$release_repository"
cd "$repository_root"
cargo_xtask() { cargo run --locked --quiet -p xtask -- "$@"; }

version="$(jq -er '.nanHarness.version' "$report")"
[[ "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || {
  printf 'Desktop publication requires a stable official nan-harness release\n' >&2
  exit 1
}
release_tag="v$version"
binary_sha="$(jq -er '.nanHarness.sha256' "$report")"
[[ "$binary_sha" =~ ^[0-9a-f]{64}$ ]] || usage
platform="$(jq -er '.platform' "$report")"
architecture="$(jq -er '.architecture' "$report")"
case "$platform/$architecture" in
  macos/aarch64|macos/x86_64) target="$architecture-apple-darwin" ;;
  linux/aarch64|linux/x86_64) target="$architecture-unknown-linux-musl" ;;
  windows/x86_64) target=x86_64-pc-windows-msvc ;;
  *) printf 'no official binary exists for this platform and architecture\n' >&2; exit 1 ;;
esac
binary_name="nan-harness-$target"
[ "$platform" != windows ] || binary_name="$binary_name.exe"
base_directory="$(mktemp -d "${TMPDIR:-/tmp}/nan-desktop-publication.XXXXXX")"
upload_directory="$base_directory/upload"
mkdir -p "$upload_directory"
trap 'rm -rf "$base_directory"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

gh release view "$release_tag" --repo "$release_repository" \
  --json tagName,isDraft,isPrerelease >"$base_directory/release.json"
jq -e --arg tag "$release_tag" '.tagName == $tag and .isDraft == false and .isPrerelease == false' \
  "$base_directory/release.json" >/dev/null
release_commit="$(channel_remote_tag_commit "$release_repository" "$release_tag")"
[[ "$release_commit" =~ ^[0-9a-f]{40}$ ]] || exit 1
retry 4 5 gh release download "$release_tag" --repo "$release_repository" \
  --pattern SHA256SUMS --output "$base_directory/SHA256SUMS"
gh attestation verify "$base_directory/SHA256SUMS" --repo "$release_repository" \
  --signer-workflow "$release_repository/.github/workflows/release.yml" \
  --source-ref "refs/tags/$release_tag" --source-digest "$release_commit" \
  --deny-self-hosted-runners >/dev/null
expected_sha="$(awk -v asset="$binary_name" '$2 == asset {print $1}' "$base_directory/SHA256SUMS")"
[[ "$expected_sha" =~ ^[0-9a-f]{64}$ ]] && [ "$expected_sha" = "$binary_sha" ] || {
  printf 'reported binary does not match the attested official release asset\n' >&2
  exit 1
}
# Retrieve only the data registry at the attested commit. Older or modified
# binaries without this metadata remain reports, not automatic certifications.
gh api "repos/$release_repository/contents/crates/nan-harness-runtime/resources/desktop-compatibility.json?ref=$release_commit" \
  -H 'Accept: application/vnd.github.raw+json' >"$base_directory/registry.json"

presence="$(channel_release_presence "$release_repository" compatibility "$base_directory/presence.json")"
release_exists=false
release_assets_json="$base_directory/assets.json"
base_v3="$base_directory/compatibility-v3.json"
base_v4="$base_directory/compatibility-v4.json"
candidate_v4="$base_directory/candidate.json"
if [ "$presence" = present ]; then
  release_exists=true
  gh release view compatibility --repo "$release_repository" --json assets >"$release_assets_json"
  if jq -e 'any(.assets[]; .name == "compatibility-v3.json")' "$release_assets_json" >/dev/null; then
    retry 4 5 gh release download compatibility --repo "$release_repository" \
      --pattern compatibility-v3.json --output "$base_v3"
    cargo_xtask validate-unified-compatibility-feed "$base_v3"
  elif jq -e 'any(.assets[]; .name | startswith("compatibility-v3.json.backup."))' "$release_assets_json" >/dev/null; then
    legacy_backup="$(jq -r '[.assets[] | select(.name | startswith("compatibility-v3.json.backup."))] | sort_by(.createdAt, .name) | last | .name' "$release_assets_json")"
    retry 4 5 gh release download compatibility --repo "$release_repository" \
      --pattern "$legacy_backup" --output "$base_v3"
    cargo_xtask validate-unified-compatibility-feed "$base_v3"
  elif jq -e 'any(.assets[]; .name == "compatibility.json")' "$release_assets_json" >/dev/null; then
    retry 4 5 gh release download compatibility --repo "$release_repository" \
      --pattern compatibility.json --output "$base_directory/legacy.json"
    cargo_xtask validate-compatibility-feed "$base_directory/legacy.json"
    jq '.schemaVersion = 3' "$base_directory/legacy.json" >"$base_v3"
  else
    cargo_xtask unified-compatibility-feed "$base_v3"
  fi
else
  cargo_xtask unified-compatibility-feed "$base_v3"
fi
recover_versioned_base_feed
cargo_xtask merge-desktop-checks "$base_v4" "$updates_directory" \
  "$base_directory/registry.json" "$version" "$candidate_v4"
cargo_xtask validate-versioned-compatibility-feed "$candidate_v4"
[ "$(channel_remote_tag_commit "$release_repository" "$release_tag")" = "$release_commit" ] || {
  printf 'release tag changed during Desktop publication\n' >&2
  exit 1
}
publication_id="${NAN_CANARY_PUBLICATION_ID:-$(date -u +%Y%m%dT%H%M%SZ)-$$-${RANDOM:-0}}"
publication_failure_phase="${NAN_CANARY_PUBLICATION_FAIL_PHASE:-}"
publication_failure_asset="${NAN_CANARY_PUBLICATION_FAIL_ASSET:-}"
publication_interrupt_phase="${NAN_CANARY_PUBLICATION_INTERRUPT_PHASE:-}"
publication_interrupt_asset="${NAN_CANARY_PUBLICATION_INTERRUPT_ASSET:-}"
publish_feed_asset compatibility-v4.json "$base_v4" "$candidate_v4" \
  "$versioned_first_publication" "$versioned_restored_backup_name" 4
