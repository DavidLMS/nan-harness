#!/usr/bin/env bash
set -euo pipefail

# Covers the available-release feed: its append-only log, its derived pointer, and the recovery
# paths a real `gh release upload --clobber` can leave behind. Every GitHub call is mocked.

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repository_root/canary/tests/release-channel-mock.sh"
source "$repository_root/canary/tests/host-lock-fixture.sh"

temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
bin_directory="$temporary_directory/bin"
assets="$temporary_directory/assets"
github="$temporary_directory/github"
repository=Acme/Fork
mkdir -p "$bin_directory" "$assets" "$github"
write_github_mock "$bin_directory"

publish_available() {
  GITHUB_ROOT="$github" \
  NAN_CANARY_STATE_DIR="$temporary_directory/state" \
  NAN_CANARY_RETRY_DELAY_SECONDS=0 \
  PATH="$bin_directory:$PATH" \
    "$repository_root/canary/host/publish-available-release.sh" \
      --assets-dir "$assets" --repository "$repository" "$@"
}

release_available() {
  publish_github_release "$github" "$repository" "$1"
  stage_release_assets "$github" "v$1" "$assets"
}

expect_failure() {
  set +e
  "$@" >/dev/null 2>&1
  local status=$?
  set -e
  [ "$status" -ne 0 ] || {
    printf 'expected failure from: %s\n' "$*" >&2
    exit 1
  }
}

logged_assets() {
  (cd "$github/releases/available/assets" && ls -1)
}

# A prerelease tag never reaches the available-release feed.
expect_failure publish_available --tag v9.8.7-rc.1

# Cold start: the feed release does not exist yet, and is created with both the pointer and the
# immutable record of that release.
release_available 9.8.7
publish_available --tag v9.8.7 >/dev/null
[ "$(feed_version "$github")" = 9.8.7 ]
[ "$(logged_assets)" = 'update-manifest-9.8.7.json
update-manifest.json' ]

# Re-running the same publication uploads nothing.
: >"$github/log"
publish_available --tag v9.8.7 >/dev/null
if grep -Eq 'release (upload|create)' "$github/log"; then
  printf 'republishing an identical available release must not upload\n' >&2
  exit 1
fi

# A newer release moves the pointer and keeps the older record.
release_available 9.9.0
publish_available --tag v9.9.0 >/dev/null
[ "$(feed_version "$github")" = 9.9.0 ]
[ -f "$github/releases/available/assets/update-manifest-9.8.7.json" ]

# An older, out-of-order gate completes honestly: it records its own release and leaves the newer
# pointer alone rather than failing for ever or moving the feed backwards.
release_available 9.8.9
publish_available --tag v9.8.9 >/dev/null
[ "$(feed_version "$github")" = 9.9.0 ]
[ -f "$github/releases/available/assets/update-manifest-9.8.9.json" ]

# A manifest that does not match its attested checksum is refused.
release_available 9.9.1
printf '%s  update-manifest.json\n' "$(printf 'a%.0s' $(seq 64))" >"$assets/SHA256SUMS"
expect_failure publish_available --tag v9.9.1
[ "$(feed_version "$github")" = 9.9.0 ]

# A manifest that describes another tag is refused.
release_available 9.9.2
expect_failure publish_available --tag v9.9.3
[ "$(feed_version "$github")" = 9.9.0 ]

# An uncertain read of the feed release never mutates anything.
release_available 9.9.4
: >"$github/log"
GH_TAGS_STATUS=500 expect_failure publish_available --tag v9.9.4
if grep -Eq 'release (upload|create)' "$github/log"; then
  printf 'an uncertain feed read must not upload\n' >&2
  exit 1
fi
GH_TAGS_TRANSPORT_FAILURE=1 expect_failure publish_available --tag v9.9.4
[ "$(feed_version "$github")" = 9.9.0 ]

# A crash between the delete and the upload of the pointer leaves the feed without its manifest,
# which is exactly what `gh release upload --clobber` does. The next run repairs it from the log.
set +e
GH_UPLOAD_KILL=update-manifest.json publish_available --tag v9.9.4 >/dev/null 2>&1
crash_status=$?
set -e
[ "$crash_status" -ne 0 ]
[ ! -f "$github/releases/available/assets/update-manifest.json" ]
[ -f "$github/releases/available/assets/update-manifest-9.9.4.json" ]
publish_available --tag v9.9.4 >/dev/null
[ "$(feed_version "$github")" = 9.9.4 ]

# A failed pointer upload also destroys the published pointer; re-running recovers the newest
# recorded release without any temporary backup.
release_available 9.9.5
GH_UPLOAD_FAIL=update-manifest.json expect_failure publish_available --tag v9.9.5
[ ! -f "$github/releases/available/assets/update-manifest.json" ]
publish_available --tag v9.9.5 >/dev/null
[ "$(feed_version "$github")" = 9.9.5 ]

# Recovery after a lost pointer republishes the newest recorded release even when the run that
# repairs it is an older one.
rm -f "$github/releases/available/assets/update-manifest.json"
release_available 9.8.9
publish_available --tag v9.8.9 >/dev/null
[ "$(feed_version "$github")" = 9.9.5 ]

# A feed that records a manifest under a version it does not describe is never promoted.
release_available 9.9.6
cp "$github/releases/available/assets/update-manifest-9.9.5.json" \
  "$github/releases/available/assets/update-manifest-9.9.9.json"
expect_failure publish_available --tag v9.9.6
[ "$(feed_version "$github")" = 9.9.5 ]
rm -f "$github/releases/available/assets/update-manifest-9.9.9.json"

# A release whose recorded manifest was changed after the fact is refused.
release_available 9.9.7
publish_available --tag v9.9.7 >/dev/null
printf '{"schemaVersion":1}\n' >"$github/releases/available/assets/update-manifest-9.9.7.json"
expect_failure publish_available --tag v9.9.7

# A second writer on this host is refused while a real first writer holds the channel lock.
rm -f "$github/releases/available/assets/update-manifest-9.9.7.json"
lock="$temporary_directory/state/release-channel-Acme__Fork.lock"
lock_fixture_prepare "$temporary_directory/fixture"
lock_fixture_hold "$repository_root/canary/host/host-lock.sh" "$lock"
release_available 9.9.8
expect_failure publish_available --tag v9.9.8
[ "$(feed_version "$github")" = 9.9.7 ]

# The lock cannot be entered by naming it in the environment either: only the descriptor a caller
# actually holds re-enters a transaction.
NAN_CANARY_RELEASE_CHANNEL_LOCK="$lock" expect_failure publish_available --tag v9.9.8
[ "$(feed_version "$github")" = 9.9.7 ]
lock_fixture_release

# A lock whose note records another host is refused rather than reclaimed: this protocol does not
# claim cross-host atomicity.
printf '{"pid":1,"host":"other-publication-host","startedAt":0}\n' >"$lock"
expect_failure publish_available --tag v9.9.8
: >"$lock"

# With the lock free the same writer completes, and leaves no owner behind.
publish_available --tag v9.9.8 >/dev/null
[ "$(feed_version "$github")" = 9.9.8 ]
[ ! -s "$lock" ]

# A writer that already owns the lock re-enters it instead of deadlocking, which is how the gate
# invokes the feed publisher inside its own transaction. Ownership travels through the inherited
# locked descriptor, so this runs the publisher from inside a real holding transaction.
release_available 9.9.9
cat >"$temporary_directory/nested.sh" <<'NESTED'
set -euo pipefail
source "$1"
host_lock_acquire "$2" 'release channel' || exit 3
shift 2
"$@"
host_lock_release
NESTED
bash "$temporary_directory/nested.sh" "$repository_root/canary/host/host-lock.sh" "$lock" \
  env GITHUB_ROOT="$github" NAN_CANARY_STATE_DIR="$temporary_directory/state" \
  NAN_CANARY_RETRY_DELAY_SECONDS=0 PATH="$bin_directory:$PATH" \
  "$repository_root/canary/host/publish-available-release.sh" \
    --assets-dir "$assets" --repository "$repository" --tag v9.9.9 >/dev/null
[ "$(feed_version "$github")" = 9.9.9 ]
