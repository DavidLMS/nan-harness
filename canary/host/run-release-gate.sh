#!/usr/bin/env bash
set -euo pipefail
umask 077

usage() {
  printf 'usage: %s --tag <vX.Y.Z> [--repo <owner/name>] [--force]\n' "$0" >&2
  exit 2
}

tag=''
release_repository="${NAN_CANARY_RELEASE_REPOSITORY:-DavidLMS/nan-harness}"
force=false
while [ "$#" -gt 0 ]; do
  case "$1" in
    --tag) tag="${2:-}"; shift 2 ;;
    --repo) release_repository="${2:-}"; shift 2 ;;
    --force) force=true; shift ;;
    *) usage ;;
  esac
done
[[ "$tag" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$ ]] || usage
[[ "$release_repository" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] \
  && [ "${release_repository%%/*}" != . ] \
  && [ "${release_repository%%/*}" != .. ] \
  && [ "${release_repository#*/}" != . ] \
  && [ "${release_repository#*/}" != .. ] || usage

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repository_root/canary/host/lib.sh"
source "$repository_root/canary/host/release-channel.sh"
state_directory="${NAN_CANARY_STATE_DIR:-$HOME/Library/Application Support/nan-harness-canary}"
mkdir -p "$state_directory"
prune_state="${NAN_CANARY_PRUNE_STATE_COMMAND:-$repository_root/canary/host/prune-state.sh}"
verify_assets="${NAN_CANARY_VERIFY_ASSETS_COMMAND:-$repository_root/canary/host/verify-release-assets.sh}"
run_suite="${NAN_CANARY_RUN_SUITE_COMMAND:-$repository_root/canary/host/run-suite.sh}"
publish_compatibility="${NAN_CANARY_PUBLISH_COMPATIBILITY_COMMAND:-$repository_root/canary/host/publish-compatibility.sh}"
publish_available="${NAN_CANARY_PUBLISH_AVAILABLE_COMMAND:-$repository_root/canary/host/publish-available-release.sh}"
"$prune_state"

# Use the release tag's policy instead of arbitrary newer host code.
if [ "${NAN_CANARY_TAG_WORKTREE:-}" != 1 ]; then
  tag_commit="$(git -C "$repository_root" rev-parse --verify "$tag^{commit}" 2>/dev/null)" || {
    printf 'release tag is not present in the local repository: %s\n' "$tag" >&2
    exit 2
  }
  remote_object="$(gh api "repos/$release_repository/git/ref/tags/$tag" --jq '[.object.type,.object.sha] | @tsv')"
  remote_type="${remote_object%%$'\t'*}"
  remote_commit="${remote_object#*$'\t'}"
  while [ "$remote_type" = tag ]; do
    remote_object="$(gh api "repos/$release_repository/git/tags/$remote_commit" --jq '[.object.type,.object.sha] | @tsv')"
    remote_type="${remote_object%%$'\t'*}"
    remote_commit="${remote_object#*$'\t'}"
  done
  [ "$remote_type" = commit ] && [ "$remote_commit" = "$tag_commit" ] || {
    printf 'local tag %s does not match the GitHub release tag in %s\n' "$tag" "$release_repository" >&2
    exit 1
  }
  temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/nan-harness-release-gate.XXXXXX")"
  tag_worktree="$temporary_root/worktree"
  cleanup_worktree() {
    git -C "$repository_root" worktree remove --force "$tag_worktree" >/dev/null 2>&1 || true
    rm -rf "$temporary_root"
  }
  trap cleanup_worktree EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
  git -C "$repository_root" worktree add --detach "$tag_worktree" "$tag_commit" >/dev/null
  worktree_arguments=(--tag "$tag" --repo "$release_repository")
  if [ "$force" = true ]; then
    worktree_arguments+=(--force)
  fi
  NAN_CANARY_TAG_WORKTREE=1 NAN_CANARY_TAG_COMMIT="$tag_commit" \
    "$tag_worktree/canary/host/run-release-gate.sh" "${worktree_arguments[@]}"
  exit $?
fi

release_json="$(gh release view "$tag" --repo "$release_repository" --json tagName,isDraft)" || {
  printf 'could not read release %s from %s\n' "$tag" "$release_repository" >&2
  exit 1
}
[ "$(jq -r '.tagName' <<<"$release_json")" = "$tag" ] || {
  printf 'release response did not match requested tag %s\n' "$tag" >&2
  exit 1
}

repo_key="${release_repository//\//__}"
receipt_directory="$state_directory/receipts/$repo_key"
receipt="$receipt_directory/$tag.json"
attempt_marker="$state_directory/release-gate-$repo_key-$tag.attempted"
assets="$state_directory/assets/$tag"
cooldown_seconds="${NAN_CANARY_RELEASE_RETRY_SECONDS:-21600}"
case "$cooldown_seconds" in
  ''|*[!0-9]*) printf 'NAN_CANARY_RELEASE_RETRY_SECONDS must be a non-negative integer\n' >&2; exit 2 ;;
esac
mkdir -p "$receipt_directory" "$assets"

receipt_write() {
  local filter="$1"
  shift
  local temporary="$receipt.tmp.$$"
  jq "$@" "$filter" "$receipt" >"$temporary"
  mv "$temporary" "$receipt"
}

tag_commit="${NAN_CANARY_TAG_COMMIT:-$(git -C "$repository_root" rev-parse HEAD)}"
if [ ! -f "$receipt" ]; then
  jq -n \
    --arg repository "$release_repository" \
    --arg tag "$tag" \
    --arg tag_commit "$tag_commit" \
    '{schemaVersion:2,repository:$repository,tag:$tag,tagCommit:$tag_commit,
      assetManifestSha256:null,outputDirectory:null,availableFeedVersion:null,
      phases:{assetsVerified:false,suitePassed:false,compatibilityFeedPublished:false,
              releasePublished:false,availableFeedPublished:false}}' \
    >"$receipt"
fi
# Every phase must be a real boolean, and the phases are monotonic: a later phase can only be
# true when every phase this gate completes before it is also true. An inconsistent receipt is
# evidence of tampering or of a foreign writer, and never resumes.
jq -e \
  --arg repository "$release_repository" --arg tag "$tag" --arg tag_commit "$tag_commit" \
  '.schemaVersion == 2 and .repository == $repository and .tag == $tag and
   .tagCommit == $tag_commit and
   (.assetManifestSha256 == null or (.assetManifestSha256 | test("^[0-9a-f]{64}$"))) and
   (.phases | keys == ["assetsVerified","availableFeedPublished",
                       "compatibilityFeedPublished","releasePublished","suitePassed"]) and
   (.phases | all(.[]; type == "boolean")) and
   (.phases | .availableFeedPublished <= .releasePublished
     and .releasePublished <= .compatibilityFeedPublished
     and .compatibilityFeedPublished <= .suitePassed
     and .suitePassed <= .assetsVerified) and
   (.phases.assetsVerified == false or .assetManifestSha256 != null)' \
  "$receipt" >/dev/null || {
  printf 'release receipt does not match the expected schema, repository, tag, commit, or phase order: %s\n' \
    "$receipt" >&2
  exit 1
}

work_directory="$(mktemp -d "${TMPDIR:-/tmp}/nan-harness-release-gate-run.XXXXXX")"
trap 'channel_lock_release; rm -rf "$work_directory"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# The release assets are immutable once published; a changed checksum manifest means the release
# is no longer the one this gate validated.
assert_recorded_assets_unchanged() {
  local recorded checksum_manifest="$work_directory/SHA256SUMS"
  recorded="$(jq -r '.assetManifestSha256 // empty' "$receipt")"
  [ -n "$recorded" ] || return 0
  retry 4 5 gh release download "$tag" --repo "$release_repository" \
    --pattern SHA256SUMS --output "$checksum_manifest" --clobber || {
    printf 'could not re-read the checksum manifest of release %s\n' "$tag" >&2
    return 1
  }
  [ "$(channel_sha256_file "$checksum_manifest")" = "$recorded" ] || {
    printf 'release assets changed after the gate recorded its first phase\n' >&2
    return 1
  }
}

# A finished receipt is only trusted once the authoritative state still agrees with it.
if [ "$(jq -r '.phases.availableFeedPublished' "$receipt")" = true ]; then
  [ "$(jq -r '.isDraft' <<<"$release_json")" = false ] || {
    printf 'receipt says the release was published, but GitHub still reports a draft\n' >&2
    exit 1
  }
  assert_recorded_assets_unchanged
  recorded_feed_version="$(jq -r '.availableFeedVersion // empty' "$receipt")"
  if [ -n "$recorded_feed_version" ]; then
    published_feed_version="$(channel_feed_published_version \
      "$release_repository" "${NAN_CANARY_AVAILABLE_FEED_TAG:-available}" \
      "$work_directory/feed-manifest.json")" || {
      printf 'could not confirm the available-release feed for %s\n' "$tag" >&2
      exit 1
    }
    [ "$published_feed_version" = "$recorded_feed_version" ] \
      || channel_version_is_newer "$published_feed_version" "$recorded_feed_version" || {
      printf 'the available-release feed offers %s, not the recorded %s\n' \
        "$published_feed_version" "$recorded_feed_version" >&2
      exit 1
    }
  fi
  exit 0
fi
if [ "$(jq -r '.isDraft' <<<"$release_json")" != true ]; then
  # Publication and the local receipt cannot be one atomic transaction, and the
  # available feed can only cite a published release. Resume only when every phase
  # this gate necessarily completed before publishing is recorded; otherwise fail closed.
  jq -e '.phases.assetsVerified and .phases.suitePassed and .phases.compatibilityFeedPublished' \
    "$receipt" >/dev/null || {
    printf 'release %s is not a draft and was not verifiably published by this gate\n' "$tag" >&2
    exit 1
  }
  assert_recorded_assets_unchanged
  receipt_write '.phases.releasePublished = true'
fi

if [ "$force" = false ] && [ -f "$attempt_marker" ]; then
  marker_mtime="$(stat -f %m "$attempt_marker" 2>/dev/null || stat -c %Y "$attempt_marker")"
  marker_age="$(( $(date +%s) - marker_mtime ))"
  if [ "$marker_age" -lt "$cooldown_seconds" ]; then
    printf 'release suite cooldown is active for %s (%ss remaining)\n' \
      "$tag" "$((cooldown_seconds - marker_age))" >&2
    exit 75
  fi
fi

retry 4 5 gh release download "$tag" \
  --repo "$release_repository" \
  --pattern nan-harness-aarch64-unknown-linux-musl \
  --pattern nan-harness-aarch64-apple-darwin \
  --pattern nan-harness-canary-aarch64-unknown-linux-musl \
  --pattern nan-harness-canary-aarch64-apple-darwin \
  --dir "$assets" --clobber
"$verify_assets" \
  --release-tag "$tag" --assets-dir "$assets" --repository "$release_repository"
manifest_digest="$(shasum -a 256 "$assets/SHA256SUMS" | awk '{print $1}')"
recorded_digest="$(jq -r '.assetManifestSha256 // empty' "$receipt")"
if [ -n "$recorded_digest" ] && [ "$recorded_digest" != "$manifest_digest" ]; then
  printf 'release assets changed after the gate recorded its first phase\n' >&2
  exit 1
fi
if [ "$(jq -r '.phases.assetsVerified' "$receipt")" != true ]; then
  receipt_write '.assetManifestSha256 = $digest | .phases.assetsVerified = true' \
    --arg digest "$manifest_digest"
fi

version="${tag#v}"
output="$(jq -r '.outputDirectory // empty' "$receipt")"
if [ "$(jq -r '.phases.suitePassed' "$receipt")" != true ]; then
  output="$state_directory/runs/$(date -u +%Y%m%dT%H%M%SZ)-release-$tag"
  mkdir -p "$output"
  receipt_write '.outputDirectory = $output' --arg output "$output"
  set +e
  "$run_suite" \
    --trigger release \
    --nan-harness-version "$version" \
    --linux-binary "$assets/nan-harness-aarch64-unknown-linux-musl" \
    --linux-canary-binary "$assets/nan-harness-canary-aarch64-unknown-linux-musl" \
    --macos-binary "$assets/nan-harness-aarch64-apple-darwin" \
    --macos-canary-binary "$assets/nan-harness-canary-aarch64-apple-darwin" \
    --output-dir "$output" \
    --release-tag "$tag" \
    --repository "$release_repository"
  suite_status=$?
  set -e
  if [ "$suite_status" -ne 0 ]; then
    if [ "$suite_status" -ne 75 ]; then
      touch "$attempt_marker"
    fi
    exit "$suite_status"
  fi
  receipt_write '.phases.suitePassed = true'
  rm -f "$attempt_marker"
fi

[ -n "$output" ] && [ -d "$output/reports" ] || {
  printf 'release receipt points to missing suite evidence\n' >&2
  exit 1
}
validator="$output/run/nan-harness-canary-aarch64-apple-darwin"
if [ "$(jq -r '.phases.compatibilityFeedPublished' "$receipt")" != true ]; then
  "$publish_compatibility" \
    --trigger release \
    --nan-harness-version "$version" \
    --release-tag "$tag" \
    --reports "$output/reports" \
    --output-dir "$output" \
    --state-dir "$state_directory" \
    --report-validator "$validator" \
    --repository "$release_repository" \
    --publish-feed
  receipt_write '.phases.compatibilityFeedPublished = true'
fi

# Publishing the release and moving the available feed is one channel transaction: both touch
# state that the maintainer recommendation also writes, so it runs under the per-repository
# channel lock and re-reads the release inside it. The feed publisher invoked below inherits the
# locked descriptor and re-enters this gate's own transaction; no marker grants that.
channel_lock_acquire "$state_directory" "$release_repository" || exit 1
release_json="$(gh release view "$tag" --repo "$release_repository" --json tagName,isDraft)" || {
  printf 'could not re-read release %s from %s under the channel lock\n' \
    "$tag" "$release_repository" >&2
  exit 1
}
[ "$(jq -r '.tagName' <<<"$release_json")" = "$tag" ] || {
  printf 'release response did not match requested tag %s\n' "$tag" >&2
  exit 1
}

# Publication is not recommendation: the release becomes public and immutable
# here, and stays out of GitHub's `latest` pointer until a maintainer runs
# recommend-release.sh. Publishing a draft defaults make_latest to true, so
# --latest=false is required rather than optional.
if [ "$(jq -r '.phases.releasePublished' "$receipt")" != true ]; then
  if [ "$(jq -r '.isDraft' <<<"$release_json")" = false ]; then
    printf 'release %s was published by another writer while this gate was running\n' "$tag" >&2
    exit 1
  fi
  retry 4 5 gh release edit "$tag" \
    --repo "$release_repository" --draft=false --latest=false
  receipt_write '.phases.releasePublished = true'
fi

case "$tag" in
  *-*)
    printf 'prerelease %s is published but stays out of the available-release feed\n' "$tag"
    receipt_write '.availableFeedVersion = null | .phases.availableFeedPublished = true'
    ;;
  *)
    "$publish_available" \
      --tag "$tag" --assets-dir "$assets" --repository "$release_repository"
    receipt_write '.availableFeedVersion = $version | .phases.availableFeedPublished = true' \
      --arg version "$version"
    ;;
esac
channel_lock_release
rm -f "$attempt_marker"
"$repository_root/canary/host/notify.sh" \
  'nan-harness release published' \
  "$tag passed the compatibility gate and is public. Recommend it explicitly when ready." || true
