#!/usr/bin/env bash
set -euo pipefail
umask 077

# Recommends an already published and validated release: it becomes GitHub's `latest`, which is
# what startup discovery, both installers, and every older client follow. The binaries, tag, and
# checksums are the ones the release gate already verified; nothing is rebuilt or re-versioned.
#
# Nothing is mutated until the complete gate evidence for this exact repository and tag is
# present, the remote tag still resolves to the commit the gate validated, and the release still
# carries the very artifacts the gate validated: the attested checksum document, every asset it
# names, and every installable artifact the consumer manifest offers.

usage() {
  printf 'usage: %s --tag <vX.Y.Z> [--repository <owner/name>]\n' "$0" >&2
  exit 2
}

tag=''
release_repository="${NAN_CANARY_RELEASE_REPOSITORY:-DavidLMS/nan-harness}"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --tag) tag="${2:-}"; shift 2 ;;
    --repository|--repo) release_repository="${2:-}"; shift 2 ;;
    *) usage ;;
  esac
done
[[ "$tag" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || usage
[[ "$release_repository" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || usage

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repository_root/canary/host/lib.sh"
source "$repository_root/canary/host/release-channel.sh"

state_directory="${NAN_CANARY_STATE_DIR:-$HOME/Library/Application Support/nan-harness-canary}"
repo_key="$(channel_repository_key "$release_repository")"
gate_receipt="$state_directory/receipts/$repo_key/$tag.json"
receipt_directory="$state_directory/recommendations/$repo_key"
receipt="$receipt_directory/$tag.json"
version="${tag#v}"

work_directory="$(mktemp -d "${TMPDIR:-/tmp}/nan-harness-recommend.XXXXXX")"
trap 'channel_lock_release; rm -rf "$work_directory"' EXIT INT TERM

# The gate must have finished every phase for this exact repository, tag and commit, and must have
# recorded the checksum-manifest digest it validated.
require_complete_gate_receipt() {
  [ -f "$gate_receipt" ] || {
    printf 'no release gate receipt for %s in %s; only a gated release can be recommended\n' \
      "$tag" "$release_repository" >&2
    return 1
  }
  jq -e \
    --arg repository "$release_repository" --arg tag "$tag" --arg version "$version" \
    '.schemaVersion == 2 and .repository == $repository and .tag == $tag and
     (.tagCommit | type == "string" and test("^[0-9a-f]{40}$")) and
     (.assetManifestSha256 | type == "string" and test("^[0-9a-f]{64}$")) and
     .availableFeedVersion == $version and
     (.phases | keys == ["assetsVerified","availableFeedPublished",
                         "compatibilityFeedPublished","releasePublished","suitePassed"]) and
     all(.phases[]; . == true)' \
    "$gate_receipt" >/dev/null || {
    printf 'the release gate receipt for %s is incomplete or does not describe this release\n' \
      "$tag" >&2
    return 1
  }
}

require_remote_tag_identity() {
  local expected actual
  expected="$(jq -r '.tagCommit' "$gate_receipt")"
  actual="$(channel_remote_tag_commit "$release_repository" "$tag")" || {
    printf 'could not resolve the remote tag %s; refusing to act on an uncertain answer\n' \
      "$tag" >&2
    return 1
  }
  [ "$actual" = "$expected" ] || {
    printf 'remote tag %s now points at %s, not at the gated commit %s\n' \
      "$tag" "$actual" "$expected" >&2
    return 1
  }
}

require_published_release() {
  local release_json
  release_json="$(gh release view "$tag" --repo "$release_repository" \
    --json tagName,isDraft,isPrerelease,assets)" || {
    printf 'could not read release %s from %s\n' "$tag" "$release_repository" >&2
    return 1
  }
  jq -e \
    --arg tag "$tag" \
    '.tagName == $tag and .isDraft == false and .isPrerelease == false and
     (.assets | type == "array") and
     any(.assets[]; .name == "SHA256SUMS") and
     any(.assets[]; .name == "update-manifest.json")' <<<"$release_json" >/dev/null || {
    printf 'release %s is not a published release with its verified metadata assets\n' "$tag" >&2
    return 1
  }
}

# Re-validates the artifacts themselves. The attested checksum document is only a list of
# expectations, so proving it unchanged proves nothing about the release's contents: every asset
# it names is downloaded and hashed, and so is every installable artifact the consumer manifest
# offers clients. A replaced or deleted manifest or binary is refused here, before `latest` moves.
require_unchanged_attested_assets() {
  local checksum_manifest="$work_directory/SHA256SUMS"
  local manifest="$work_directory/asset-update-manifest.json"
  retry 4 5 gh release download "$tag" \
    --repo "$release_repository" --pattern SHA256SUMS \
    --output "$checksum_manifest" --clobber || {
    printf 'could not read the checksum manifest of release %s\n' "$tag" >&2
    return 1
  }
  [ "$(channel_sha256_file "$checksum_manifest")" = "$(jq -r '.assetManifestSha256' "$gate_receipt")" ] || {
    printf 'release %s no longer carries the checksum manifest the gate validated\n' "$tag" >&2
    return 1
  }
  gh attestation verify "$checksum_manifest" \
    --repo "$release_repository" \
    --signer-workflow "$release_repository/.github/workflows/release.yml" \
    --source-ref "refs/tags/$tag" \
    --deny-self-hosted-runners >/dev/null || {
    printf 'release %s no longer passes attestation verification\n' "$tag" >&2
    return 1
  }
  channel_verify_attested_assets "$release_repository" "$tag" "$checksum_manifest" \
    "$work_directory" || return 1
  [ -f "$manifest" ] || {
    printf 'the attested checksums of release %s do not cover its update manifest\n' "$tag" >&2
    return 1
  }
  channel_manifest_describes_release "$manifest" "$version" "$release_repository" || {
    printf 'the update manifest of release %s does not describe that release\n' "$tag" >&2
    return 1
  }
  channel_verify_manifest_artifacts "$release_repository" "$tag" "$manifest" "$work_directory"
}

# Prints the recommended tag, or nothing when GitHub confirms there is none. An uncertain answer
# fails instead of being read as an absent recommendation.
current_recommendation() {
  local body="$work_directory/latest.json"
  local status
  status="$(channel_api "repos/$release_repository/releases/latest" "$body")" || {
    printf 'could not read the current recommendation in %s; refusing to move it\n' \
      "$release_repository" >&2
    return 1
  }
  case "$status" in
    200) jq -er '.tag_name | strings' "$body" || {
           printf 'the current recommendation response is not a release\n' >&2
           return 1
         } ;;
    404) ;;
    *)
      printf 'reading the current recommendation in %s answered HTTP %s; refusing to move it\n' \
        "$release_repository" "$status" >&2
      return 1
      ;;
  esac
}

write_receipt() {
  [ ! -f "$receipt" ] || return 0
  mkdir -p "$receipt_directory"
  local temporary="$receipt.tmp.$$"
  jq -n \
    --arg repository "$release_repository" \
    --arg tag "$tag" \
    --arg version "$version" \
    --arg tag_commit "$(jq -r '.tagCommit' "$gate_receipt")" \
    --arg recommended_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    '{schemaVersion:1,repository:$repository,tag:$tag,version:$version,tagCommit:$tag_commit,
      recommendedAt:$recommended_at}' >"$temporary"
  mv "$temporary" "$receipt"
}

require_complete_gate_receipt
channel_lock_acquire "$state_directory" "$release_repository" || exit 1

# Everything below is re-read inside the lock, so a concurrent writer cannot make this decision
# stale between the check and the move.
require_remote_tag_identity
require_published_release
require_unchanged_attested_assets

recommended="$(current_recommendation)"
if [ "$recommended" = "$tag" ]; then
  printf 'release %s is already the recommended release\n' "$tag"
elif [ -n "$recommended" ] && ! channel_version_is_newer "$version" "${recommended#v}"; then
  printf 'recommended release %s is newer than %s; recommendation is forward-only\n' \
    "$recommended" "$tag" >&2
  exit 1
else
  retry 4 5 gh release edit "$tag" --repo "$release_repository" --latest
  [ "$(current_recommendation)" = "$tag" ] || {
    printf 'GitHub did not report %s as the recommended release\n' "$tag" >&2
    exit 1
  }
  printf 'recommended release %s\n' "$tag"
fi
write_receipt
