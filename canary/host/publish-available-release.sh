#!/usr/bin/env bash
set -euo pipefail
umask 077

# Publishes the newest validated public release to the available-release feed: a standing
# prerelease-marked `available` release. Only explicit `nan-harness update` reads it; the
# maintainer-recommended release stays GitHub's `latest` pointer and is not touched here.
#
# The feed keeps one immutable `update-manifest-<version>.json` per published release, which is
# the durable log, plus the `update-manifest.json` clients read. That pointer is derived: it must
# always hold the contents of the highest logged version. Every run recomputes it and repairs it,
# so a crash or a failed upload between deleting and re-uploading the pointer is recovered by the
# next run instead of leaving the feed without a manifest for ever.

usage() {
  printf 'usage: %s --tag <vX.Y.Z> --assets-dir <directory> [--repository <owner/name>]\n' "$0" >&2
  exit 2
}

tag=''
assets_directory=''
release_repository="${NAN_CANARY_RELEASE_REPOSITORY:-DavidLMS/nan-harness}"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --tag) tag="${2:-}"; shift 2 ;;
    --assets-dir) assets_directory="${2:-}"; shift 2 ;;
    --repository) release_repository="${2:-}"; shift 2 ;;
    *) usage ;;
  esac
done
[[ "$tag" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || usage
[[ "$release_repository" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || usage
[ -d "$assets_directory" ] || usage

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repository_root/canary/host/lib.sh"
source "$repository_root/canary/host/release-channel.sh"

state_directory="${NAN_CANARY_STATE_DIR:-$HOME/Library/Application Support/nan-harness-canary}"
feed_tag="${NAN_CANARY_AVAILABLE_FEED_TAG:-available}"
manifest_name=update-manifest.json
version="${tag#v}"
checksum_manifest="$assets_directory/SHA256SUMS"
[ -f "$checksum_manifest" ] || {
  printf 'attested release checksums are required before publishing the available feed\n' >&2
  exit 1
}

work_directory="$(mktemp -d "${TMPDIR:-/tmp}/nan-harness-available-feed.XXXXXX")"
trap 'channel_lock_release; rm -rf "$work_directory"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
candidate="$work_directory/$manifest_name"
feed_json="$work_directory/feed.json"

versioned_name() {
  printf 'update-manifest-%s.json\n' "$1"
}

verify_candidate_manifest() {
  local expected
  expected="$(awk -v asset="$manifest_name" '$2 == asset { print $1 }' "$checksum_manifest")"
  [ "${#expected}" -eq 64 ] || {
    printf 'attested checksums do not describe exactly one %s\n' "$manifest_name" >&2
    return 1
  }
  [ "$(channel_sha256_file "$candidate")" = "$expected" ] || {
    printf 'the release update manifest does not match its attested checksum\n' >&2
    return 1
  }
  channel_manifest_describes_release "$candidate" "$version" "$release_repository" || {
    printf 'the release update manifest does not describe %s\n' "$tag" >&2
    return 1
  }
}

download_feed_asset() {
  retry 4 5 gh release download "$feed_tag" \
    --repo "$release_repository" --pattern "$1" --output "$2" --clobber
}

upload_feed_asset() {
  local source="$1"
  local verification="$work_directory/verification.json"
  retry 4 5 gh release upload "$feed_tag" "$source" --repo "$release_repository" --clobber \
    || return 1
  retry 4 5 gh release download "$feed_tag" \
    --repo "$release_repository" \
    --pattern "$(basename "$source")" --output "$verification" --clobber || return 1
  cmp -s "$source" "$verification"
}

# Appends this release's immutable manifest to the feed log, or confirms the logged copy is
# byte-identical. A logged manifest that differs is a release-immutability violation.
record_candidate_in_log() {
  local name logged
  name="$(versioned_name "$version")"
  if ! channel_feed_has_asset "$feed_json" "$name"; then
    upload_feed_asset "$work_directory/$name" || {
      printf 'could not record release %s in the available-release feed\n' "$version" >&2
      return 1
    }
    return 0
  fi
  logged="$work_directory/logged-$name"
  download_feed_asset "$name" "$logged" || {
    printf 'could not read the recorded manifest for release %s\n' "$version" >&2
    return 1
  }
  cmp -s "$work_directory/$name" "$logged" || {
    printf 'the available-release feed already records a different manifest for %s\n' "$version" >&2
    return 1
  }
}

# Repairs the derived pointer so it carries the highest logged release.
publish_pointer() {
  local target="$1"
  local target_document="$work_directory/target-$manifest_name"
  local current="$work_directory/current-$manifest_name"
  if [ "$target" = "$version" ]; then
    cp "$candidate" "$target_document"
  else
    download_feed_asset "$(versioned_name "$target")" "$target_document" || {
      printf 'could not read the recorded manifest for release %s\n' "$target" >&2
      return 1
    }
    channel_manifest_describes_release "$target_document" "$target" "$release_repository" || {
      printf 'the recorded manifest for release %s does not describe that release\n' "$target" >&2
      return 1
    }
  fi
  if channel_feed_has_asset "$feed_json" "$manifest_name" \
    && download_feed_asset "$manifest_name" "$current" \
    && cmp -s "$target_document" "$current"; then
    printf 'available release %s is already published\n' "$target"
    return 0
  fi
  cp "$target_document" "$work_directory/pointer/$manifest_name"
  upload_feed_asset "$work_directory/pointer/$manifest_name" || {
    printf 'could not publish release %s to the available-release feed\n' "$target" >&2
    return 1
  }
  printf 'published available release %s\n' "$target"
}

channel_lock_acquire "$state_directory" "$release_repository" || exit 1

retry 4 5 gh release download "$tag" \
  --repo "$release_repository" \
  --pattern "$manifest_name" \
  --output "$candidate"
verify_candidate_manifest
mkdir -p "$work_directory/pointer"
cp "$candidate" "$work_directory/$(versioned_name "$version")"

feed_presence="$(channel_release_presence "$release_repository" "$feed_tag" "$feed_json")" || exit 1
if [ "$feed_presence" = absent ]; then
  gh release create "$feed_tag" \
    "$work_directory/$(versioned_name "$version")" "$candidate" \
    --repo "$release_repository" \
    --prerelease \
    --title 'nan-harness available release' \
    --notes 'Newest published and validated nan-harness release, for explicit `nan-harness update`.' \
    || {
    printf 'could not create the available-release feed\n' >&2
    exit 1
  }
  printf 'published available release %s\n' "$version"
  exit 0
fi

record_candidate_in_log
channel_release_presence "$release_repository" "$feed_tag" "$feed_json" >/dev/null || exit 1
target_version="$(channel_feed_target_version "$feed_json")"
[ -n "$target_version" ] || {
  printf 'the available-release feed records no release manifest\n' >&2
  exit 1
}
if channel_version_is_newer "$target_version" "$version"; then
  printf 'available-release feed already holds newer release %s; release %s stays recorded only\n' \
    "$target_version" "$version"
fi
publish_pointer "$target_version"
