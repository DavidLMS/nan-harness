#!/usr/bin/env bash
set -euo pipefail

# Covers the explicit maintainer recommendation: it moves GitHub's `latest` pointer only with
# complete, revalidated gate evidence, and never on an uncertain read. Every GitHub call is mocked.

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repository_root/canary/tests/release-channel-mock.sh"
source "$repository_root/canary/tests/host-lock-fixture.sh"

temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
bin_directory="$temporary_directory/bin"
github="$temporary_directory/github"
state="$temporary_directory/state"
repository=Acme/Fork
receipts="$state/receipts/Acme__Fork"
mkdir -p "$bin_directory" "$github" "$receipts"
write_github_mock "$bin_directory"

recommend_release() {
  GITHUB_ROOT="$github" \
  NAN_CANARY_STATE_DIR="$state" \
  NAN_CANARY_RETRY_DELAY_SECONDS=0 \
  PATH="$bin_directory:$PATH" \
    "$repository_root/canary/host/recommend-release.sh" \
      --repository "$repository" "$@"
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

# Writes the receipt a completed release gate leaves behind for one published release.
write_gate_receipt() {
  local version="$1"
  local tag="v$version"
  jq -n \
    --arg repository "$repository" \
    --arg tag "$tag" \
    --arg version "$version" \
    --arg tag_commit "$(cat "$github/tags/$tag")" \
    --arg digest "$(shasum -a 256 "$github/releases/$tag/assets/SHA256SUMS" | awk '{print $1}')" \
    '{schemaVersion:2,repository:$repository,tag:$tag,tagCommit:$tag_commit,
      assetManifestSha256:$digest,outputDirectory:"/dev/null",availableFeedVersion:$version,
      phases:{assetsVerified:true,suitePassed:true,compatibilityFeedPublished:true,
              releasePublished:true,availableFeedPublished:true}}' \
    >"$receipts/$tag.json"
}

gated_release() {
  publish_github_release "$github" "$repository" "$1"
  write_gate_receipt "$1"
}

recommended_tag() {
  cat "$github/latest" 2>/dev/null || printf 'none\n'
}

gated_release 9.9.0
gated_release 9.9.1

# A release with no gate receipt at all is never recommended.
publish_github_release "$github" "$repository" 9.9.2
expect_failure recommend_release --tag v9.9.2
[ "$(recommended_tag)" = none ]

# An incomplete receipt is never accepted, one phase at a time.
for phase in assetsVerified suitePassed compatibilityFeedPublished releasePublished \
  availableFeedPublished; do
  write_gate_receipt 9.9.0
  jq --arg phase "$phase" '.phases[$phase] = false' "$receipts/v9.9.0.json" >"$receipts/tmp.json"
  mv "$receipts/tmp.json" "$receipts/v9.9.0.json"
  expect_failure recommend_release --tag v9.9.0
done

# A receipt whose booleans are strings, whose evidence is missing, or which belongs to another
# repository or release is refused.
for mutation in '.phases.releasePublished = "true"' '.assetManifestSha256 = null' \
  '.tagCommit = "not-a-commit"' '.repository = "Other/Fork"' '.availableFeedVersion = null' \
  '.schemaVersion = 1'; do
  write_gate_receipt 9.9.0
  jq "$mutation" "$receipts/v9.9.0.json" >"$receipts/tmp.json"
  mv "$receipts/tmp.json" "$receipts/v9.9.0.json"
  expect_failure recommend_release --tag v9.9.0
done
[ "$(recommended_tag)" = none ]

# A tag that no longer points at the gated commit is refused.
write_gate_receipt 9.9.0
printf '%s\n' 0000000000000000000000000000000000000000 >"$github/tags/v9.9.0"
expect_failure recommend_release --tag v9.9.0
publish_github_release "$github" "$repository" 9.9.0

# Changed release assets are refused even though the receipt is complete.
write_gate_receipt 9.9.0
printf 'tampered\n' >>"$github/releases/v9.9.0/assets/SHA256SUMS"
expect_failure recommend_release --tag v9.9.0
publish_github_release "$github" "$repository" 9.9.0
write_gate_receipt 9.9.0

# A release that no longer passes attestation is refused.
GH_ATTESTATION_FAILURE=1 expect_failure recommend_release --tag v9.9.0
[ "$(recommended_tag)" = none ]

# The attested checksum document is a list of expectations, not evidence about the artifacts
# themselves. Each of these leaves SHA256SUMS byte-identical and still attested, changing only what
# a client would actually download, and none of them may reach a recommendation.
assert_artifacts_refused() {
  : >"$github/log"
  set +e
  recommend_release --tag v9.9.0 >/dev/null 2>&1
  local status=$?
  set -e
  [ "$status" -ne 0 ] || {
    printf 'a release with changed artifacts must not be recommended: %s\n' "$1" >&2
    exit 1
  }
  if grep -Fq 'release edit' "$github/log"; then
    printf 'a release with changed artifacts must not be edited: %s\n' "$1" >&2
    exit 1
  fi
  [ "$(recommended_tag)" = "${2:-none}" ]
  publish_github_release "$github" "$repository" 9.9.0
  write_gate_receipt 9.9.0
}

release_assets="$github/releases/v9.9.0/assets"
attested_digest="$(shasum -a 256 "$release_assets/SHA256SUMS" | awk '{print $1}')"

# The manifest clients read is replaced by a valid-looking but different document.
printf '{"schemaVersion":1,"version":"9.9.0","notesUrl":"https://example.test/","artifacts":[]}\n' \
  >"$release_assets/update-manifest.json"
[ "$(shasum -a 256 "$release_assets/SHA256SUMS" | awk '{print $1}')" = "$attested_digest" ]
assert_artifacts_refused 'replaced update manifest'

# The manifest is deleted outright.
rm -f "$release_assets/update-manifest.json"
assert_artifacts_refused 'deleted update manifest'

# An installable binary is replaced with different content.
printf 'not the gated binary\n' >"$release_assets/nan-harness-aarch64-apple-darwin"
assert_artifacts_refused 'replaced installable binary'

# An installable binary is deleted.
rm -f "$release_assets/nan-harness-aarch64-unknown-linux-musl"
assert_artifacts_refused 'deleted installable binary'

# The manifest offers an artifact that belongs to a different release.
jq '.artifacts[0].url = "https://github.com/Acme/Fork/releases/download/v9.9.1/nan-harness-aarch64-apple-darwin"' \
  "$release_assets/update-manifest.json" >"$temporary_directory/manifest.json"
mv "$temporary_directory/manifest.json" "$release_assets/update-manifest.json"
printf '%s  update-manifest.json\n' \
  "$(shasum -a 256 "$release_assets/update-manifest.json" | awk '{print $1}')" \
  >"$temporary_directory/sums"
grep -v ' update-manifest.json$' "$release_assets/SHA256SUMS" >>"$temporary_directory/sums"
mv "$temporary_directory/sums" "$release_assets/SHA256SUMS"
write_gate_receipt 9.9.0
assert_artifacts_refused 'artifact from another release'

# Attested checksums that do not cover the manifest clients read prove nothing about it.
grep -v ' update-manifest.json$' "$release_assets/SHA256SUMS" >"$temporary_directory/sums"
mv "$temporary_directory/sums" "$release_assets/SHA256SUMS"
write_gate_receipt 9.9.0
assert_artifacts_refused 'attested checksums without the manifest'

# Complete evidence recommends the same immutable tag, and records its own receipt.
: >"$github/log"
recommend_release --tag v9.9.0 >/dev/null
[ "$(recommended_tag)" = v9.9.0 ]
grep -Fq -- 'release edit v9.9.0 --repo Acme/Fork --latest' "$github/log"
jq -e '.schemaVersion == 1 and .tag == "v9.9.0" and .version == "9.9.0" and
  (.tagCommit | test("^[0-9a-f]{40}$"))' \
  "$state/recommendations/Acme__Fork/v9.9.0.json" >/dev/null

# Recommending the current release again changes nothing.
: >"$github/log"
recommend_release --tag v9.9.0 >/dev/null
if grep -Fq 'release edit' "$github/log"; then
  printf 'recommending the current release again must not edit it\n' >&2
  exit 1
fi

# Recommendation is forward-only, even with complete evidence for the older release.
gated_release 9.8.7
set +e
forward_error="$(recommend_release --tag v9.8.7 2>&1 >/dev/null)"
forward_status=$?
set -e
[ "$forward_status" -ne 0 ]
grep -Fq 'recommendation is forward-only' <<<"$forward_error"
[ "$(recommended_tag)" = v9.9.0 ]

# A failed or uncertain read of the current recommendation must not let an older tag overwrite a
# newer one, and must not edit anything.
: >"$github/log"
GH_LATEST_STATUS=500 expect_failure recommend_release --tag v9.9.1
GH_LATEST_TRANSPORT_FAILURE=1 expect_failure recommend_release --tag v9.9.1
if grep -Fq 'release edit' "$github/log"; then
  printf 'an uncertain recommendation read must not edit a release\n' >&2
  exit 1
fi
[ "$(recommended_tag)" = v9.9.0 ]

# The same read succeeding afterwards recommends normally.
recommend_release --tag v9.9.1 >/dev/null
[ "$(recommended_tag)" = v9.9.1 ]

# Drafts and prereleases are never recommended.
gated_release 9.9.3
printf 'draft\n' >"$github/releases/v9.9.3/state"
expect_failure recommend_release --tag v9.9.3
printf 'prerelease\n' >"$github/releases/v9.9.3/state"
expect_failure recommend_release --tag v9.9.3
[ "$(recommended_tag)" = v9.9.1 ]

# A real concurrent channel writer on this host blocks the recommendation instead of racing it,
# and naming the lock in the environment does not enter it either.
printf 'public\n' >"$github/releases/v9.9.3/state"
lock="$state/release-channel-Acme__Fork.lock"
lock_fixture_prepare "$temporary_directory/fixture"
lock_fixture_hold "$repository_root/canary/host/host-lock.sh" "$lock"
: >"$github/log"
expect_failure recommend_release --tag v9.9.3
NAN_CANARY_RELEASE_CHANNEL_LOCK="$lock" expect_failure recommend_release --tag v9.9.3
if grep -Fq 'release edit' "$github/log"; then
  printf 'a blocked recommendation must not edit a release\n' >&2
  exit 1
fi
[ "$(recommended_tag)" = v9.9.1 ]
lock_fixture_release
recommend_release --tag v9.9.3 >/dev/null
[ "$(recommended_tag)" = v9.9.3 ]
[ ! -s "$lock" ]
