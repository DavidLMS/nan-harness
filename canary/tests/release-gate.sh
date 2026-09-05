#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
bin_directory="$temporary_directory/bin"
asset_source="$temporary_directory/source-assets"
mkdir -p "$bin_directory" "$asset_source"

for asset in \
  nan-harness-aarch64-unknown-linux-musl \
  nan-harness-canary-aarch64-unknown-linux-musl \
  nan-harness-aarch64-apple-darwin \
  nan-harness-canary-aarch64-apple-darwin; do
  printf '%s fixture\n' "$asset" >"$asset_source/$asset"
done

cat >"$bin_directory/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$GH_LOG"
if [ "${1:-}" = release ] && [ "${2:-}" = view ]; then
  printf '{"tagName":"%s","isDraft":%s}\n' "$GATE_TAG" "${GH_DRAFT:-true}"
  exit 0
fi
if [ "${1:-}" = release ] && [ "${2:-}" = download ]; then
  tag="$3"
  directory=''
  output=''
  pattern=''
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --dir) directory="$2"; shift 2 ;;
      --output) output="$2"; shift 2 ;;
      --pattern) pattern="$2"; shift 2 ;;
      *) shift ;;
    esac
  done
  if [ -n "$directory" ]; then
    cp "$ASSET_SOURCE"/* "$directory/"
    exit 0
  fi
  if [ "$tag" = available ]; then
    [ -f "$FEED_MANIFEST" ] || exit 1
    cp "$FEED_MANIFEST" "$output"
    exit 0
  fi
  [ "$pattern" = SHA256SUMS ] && [ -f "$REMOTE_CHECKSUMS" ] || exit 1
  cp "$REMOTE_CHECKSUMS" "$output"
  exit 0
fi
if [ "${1:-}" = release ] && [ "${2:-}" = edit ]; then
  printf '%s\n' "$*" >>"$PROMOTION_LOG"
  exit "${PUBLICATION_STATUS:-0}"
fi
exit 1
EOF

cat >"$bin_directory/prune" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat >"$bin_directory/verify" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
assets=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --assets-dir) assets="$2"; shift 2 ;;
    *) shift ;;
  esac
done
[ "${VERIFY_FAIL:-0}" != 1 ] || exit 1
printf 'verified manifest\n' >"$assets/SHA256SUMS"
cp "$assets/SHA256SUMS" "$REMOTE_CHECKSUMS"
EOF
cat >"$bin_directory/suite" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'suite\n' >>"$SUITE_LOG"
output=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output-dir) output="$2"; shift 2 ;;
    *) shift ;;
  esac
done
if [ "${SUITE_STATUS:-0}" -ne 0 ]; then
  exit "$SUITE_STATUS"
fi
mkdir -p "$output/reports" "$output/run"
printf '{}\n' >"$output/reports/fixture.json"
printf '#!/usr/bin/env bash\nexit 0\n' >"$output/run/nan-harness-canary-aarch64-apple-darwin"
chmod 755 "$output/run/nan-harness-canary-aarch64-apple-darwin"
EOF
cat >"$bin_directory/available" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$AVAILABLE_LOG"
[ "${AVAILABLE_STATUS:-0}" -eq 0 ] || exit "$AVAILABLE_STATUS"
tag=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --tag) tag="$2"; shift 2 ;;
    *) shift ;;
  esac
done
printf '{"schemaVersion":1,"version":"%s"}\n' "${tag#v}" >"$FEED_MANIFEST"
[ -z "${AVAILABLE_LOCK_ASSERTION:-}" ] || [ -d "$NAN_CANARY_RELEASE_CHANNEL_LOCK" ]
EOF
cat >"$bin_directory/publish" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'publish\n' >>"$PUBLISH_LOG"
if [ -n "${PUBLISH_FAIL_ONCE_FILE:-}" ] && [ ! -f "$PUBLISH_FAIL_ONCE_FILE" ]; then
  touch "$PUBLISH_FAIL_ONCE_FILE"
  exit 1
fi
EOF
chmod 755 "$bin_directory"/*

run_gate() {
  local state="$1"
  shift
  mkdir -p "$state"
  ASSET_SOURCE="$asset_source" \
  GATE_TAG="${GATE_TAG:-v9.8.7}" \
  GH_LOG="$temporary_directory/gh.log" \
  SUITE_LOG="$temporary_directory/suite.log" \
  PUBLISH_LOG="$temporary_directory/publish.log" \
  AVAILABLE_LOG="$temporary_directory/available.log" \
  FEED_MANIFEST="${FEED_MANIFEST:-$temporary_directory/feed-manifest.json}" \
  REMOTE_CHECKSUMS="${REMOTE_CHECKSUMS:-$temporary_directory/remote-checksums}" \
  AVAILABLE_LOCK_ASSERTION=1 \
  PROMOTION_LOG="$temporary_directory/promotion.log" \
  NAN_CANARY_STATE_DIR="$state" \
  NAN_CANARY_TAG_WORKTREE=1 \
  NAN_CANARY_TAG_COMMIT=0123456789abcdef \
  NAN_CANARY_RETRY_DELAY_SECONDS=0 \
  NAN_CANARY_PRUNE_STATE_COMMAND="$bin_directory/prune" \
  NAN_CANARY_VERIFY_ASSETS_COMMAND="$bin_directory/verify" \
  NAN_CANARY_RUN_SUITE_COMMAND="$bin_directory/suite" \
  NAN_CANARY_PUBLISH_COMPATIBILITY_COMMAND="$bin_directory/publish" \
  NAN_CANARY_PUBLISH_AVAILABLE_COMMAND="$bin_directory/available" \
  PATH="$bin_directory:$PATH" \
    "$repository_root/canary/host/run-release-gate.sh" \
      --tag "${GATE_TAG:-v9.8.7}" --repo Acme/Fork "$@"
}

set +e
"$repository_root/canary/host/run-release-gate.sh" >/dev/null 2>&1
[ "$?" -eq 2 ]
set -e

success_state="$temporary_directory/state-success"
run_gate "$success_state"
receipt="$success_state/receipts/Acme__Fork/v9.8.7.json"
jq -e '.schemaVersion == 2 and .availableFeedVersion == "9.8.7" and
  .phases == {assetsVerified:true,suitePassed:true,compatibilityFeedPublished:true,
              releasePublished:true,availableFeedPublished:true}' "$receipt" >/dev/null
grep -Fq -- '--repo Acme/Fork' "$temporary_directory/gh.log"
[ "$(wc -l <"$temporary_directory/suite.log" | tr -d ' ')" -eq 1 ]
[ "$(wc -l <"$temporary_directory/publish.log" | tr -d ' ')" -eq 1 ]
[ "$(wc -l <"$temporary_directory/available.log" | tr -d ' ')" -eq 1 ]
grep -Fq -- '--tag v9.8.7' "$temporary_directory/available.log"

# Publication must never recommend: the release is published explicitly non-latest.
grep -Fq -- '--draft=false --latest=false' "$temporary_directory/promotion.log"
if grep -Eq -- '--latest([^=]|$)' "$temporary_directory/promotion.log"; then
  printf 'the release gate must not mark a release as latest\n' >&2
  exit 1
fi
if grep -Fq -- '--latest' "$temporary_directory/gh.log" \
  && ! grep -Fq -- '--latest=false' "$temporary_directory/gh.log"; then
  printf 'the release gate must not mark a release as latest\n' >&2
  exit 1
fi

# A prerelease is published, but never enters the available-release feed.
: >"$temporary_directory/available.log"
prerelease_state="$temporary_directory/state-prerelease"
GATE_TAG=v9.8.8-rc.1 run_gate "$prerelease_state"
jq -e '.availableFeedVersion == null and .phases.availableFeedPublished == true' \
  "$prerelease_state/receipts/Acme__Fork/v9.8.8-rc.1.json" >/dev/null
[ ! -s "$temporary_directory/available.log" ]

: >"$temporary_directory/suite.log"
: >"$temporary_directory/publish.log"
cooldown_state="$temporary_directory/state-cooldown"
set +e
SUITE_STATUS=1 run_gate "$cooldown_state"
[ "$?" -eq 1 ]
run_gate "$cooldown_state"
[ "$?" -eq 75 ]
set -e
[ "$(wc -l <"$temporary_directory/suite.log" | tr -d ' ')" -eq 1 ]
run_gate "$cooldown_state" --force
[ "$(wc -l <"$temporary_directory/suite.log" | tr -d ' ')" -eq 2 ]

: >"$temporary_directory/suite.log"
: >"$temporary_directory/publish.log"
resume_state="$temporary_directory/state-resume"
publish_failure="$temporary_directory/publish-failed"
set +e
PUBLISH_FAIL_ONCE_FILE="$publish_failure" run_gate "$resume_state"
[ "$?" -eq 1 ]
set -e
jq -e '.phases.suitePassed == true and .phases.compatibilityFeedPublished == false' \
  "$resume_state/receipts/Acme__Fork/v9.8.7.json" >/dev/null
PUBLISH_FAIL_ONCE_FILE="$publish_failure" run_gate "$resume_state"
[ "$(wc -l <"$temporary_directory/suite.log" | tr -d ' ')" -eq 1 ]
[ "$(wc -l <"$temporary_directory/publish.log" | tr -d ' ')" -eq 2 ]

: >"$temporary_directory/suite.log"
: >"$temporary_directory/publish.log"
: >"$temporary_directory/available.log"
publication_recovery_state="$temporary_directory/state-publication-recovery"
set +e
PUBLICATION_STATUS=1 run_gate "$publication_recovery_state"
[ "$?" -eq 1 ]
set -e
jq -e '.phases.compatibilityFeedPublished == true and .phases.releasePublished == false' \
  "$publication_recovery_state/receipts/Acme__Fork/v9.8.7.json" >/dev/null
GH_DRAFT=false run_gate "$publication_recovery_state"
jq -e '.phases.releasePublished == true and .phases.availableFeedPublished == true' \
  "$publication_recovery_state/receipts/Acme__Fork/v9.8.7.json" >/dev/null
[ "$(wc -l <"$temporary_directory/suite.log" | tr -d ' ')" -eq 1 ]
[ "$(wc -l <"$temporary_directory/publish.log" | tr -d ' ')" -eq 1 ]
[ "$(wc -l <"$temporary_directory/available.log" | tr -d ' ')" -eq 1 ]

# A release published without its feed is resumed by publishing only the feed.
: >"$temporary_directory/suite.log"
: >"$temporary_directory/publish.log"
: >"$temporary_directory/available.log"
: >"$temporary_directory/promotion.log"
feed_recovery_state="$temporary_directory/state-feed-recovery"
set +e
AVAILABLE_STATUS=1 run_gate "$feed_recovery_state"
[ "$?" -eq 1 ]
set -e
jq -e '.phases.releasePublished == true and .phases.availableFeedPublished == false' \
  "$feed_recovery_state/receipts/Acme__Fork/v9.8.7.json" >/dev/null
GH_DRAFT=false run_gate "$feed_recovery_state"
jq -e '.phases.availableFeedPublished == true' \
  "$feed_recovery_state/receipts/Acme__Fork/v9.8.7.json" >/dev/null
[ "$(wc -l <"$temporary_directory/suite.log" | tr -d ' ')" -eq 1 ]
[ "$(wc -l <"$temporary_directory/publish.log" | tr -d ' ')" -eq 1 ]
[ "$(wc -l <"$temporary_directory/available.log" | tr -d ' ')" -eq 2 ]
[ "$(wc -l <"$temporary_directory/promotion.log" | tr -d ' ')" -eq 1 ]

# A finished tag is a no-op, and never republishes or recommends.
: >"$temporary_directory/promotion.log"
GH_DRAFT=false run_gate "$feed_recovery_state"
[ ! -s "$temporary_directory/promotion.log" ]

# A receipt whose phases are not consistent booleans never resumes, however it was damaged.
receipt_state="$temporary_directory/state-receipt"
for mutation in '.phases.suitePassed = "true"' \
  '.phases.releasePublished = true' \
  '.phases.availableFeedPublished = true | .phases.releasePublished = false' \
  '.assetManifestSha256 = "not-a-digest"' \
  '.phases.assetsVerified = true | .assetManifestSha256 = null'; do
  rm -rf "$receipt_state"
  mkdir -p "$receipt_state/receipts/Acme__Fork"
  jq "$mutation" "$success_state/receipts/Acme__Fork/v9.8.7.json" \
    >"$receipt_state/receipts/Acme__Fork/v9.8.7.json"
  set +e
  run_gate "$receipt_state" >/dev/null 2>&1
  status=$?
  set -e
  [ "$status" -eq 1 ] || {
    printf 'a receipt damaged by %s must not resume\n' "$mutation" >&2
    exit 1
  }
done

# A finished receipt is only trusted while the release still carries the assets the gate
# validated, and while the feed still offers the release the receipt recorded.
finished_state="$temporary_directory/state-finished"
mkdir -p "$finished_state/receipts/Acme__Fork"
cp "$success_state/receipts/Acme__Fork/v9.8.7.json" \
  "$finished_state/receipts/Acme__Fork/v9.8.7.json"
: >"$temporary_directory/promotion.log"
printf '{"schemaVersion":1,"version":"9.8.7"}\n' >"$temporary_directory/feed-manifest.json"
GH_DRAFT=false run_gate "$finished_state" >/dev/null
[ ! -s "$temporary_directory/promotion.log" ]

printf '{"schemaVersion":1,"version":"9.8.6"}\n' >"$temporary_directory/feed-manifest.json"
set +e
GH_DRAFT=false run_gate "$finished_state" >/dev/null 2>&1
[ "$?" -eq 1 ]
set -e
printf '{"schemaVersion":1,"version":"9.8.7"}\n' >"$temporary_directory/feed-manifest.json"

changed_checksums="$temporary_directory/changed-checksums"
printf 'a different manifest\n' >"$changed_checksums"
set +e
REMOTE_CHECKSUMS="$changed_checksums" GH_DRAFT=false run_gate "$finished_state" >/dev/null 2>&1
[ "$?" -eq 1 ]
set -e

# A release published before its receipt was written resumes only with every preceding phase.
partial_state="$temporary_directory/state-partial"
mkdir -p "$partial_state/receipts/Acme__Fork"
jq '.phases.compatibilityFeedPublished = false | .phases.releasePublished = false |
    .phases.availableFeedPublished = false' \
  "$success_state/receipts/Acme__Fork/v9.8.7.json" \
  >"$partial_state/receipts/Acme__Fork/v9.8.7.json"
set +e
GH_DRAFT=false run_gate "$partial_state" >/dev/null 2>&1
[ "$?" -eq 1 ]
set -e
jq '.phases.releasePublished = false | .phases.availableFeedPublished = false' \
  "$success_state/receipts/Acme__Fork/v9.8.7.json" \
  >"$partial_state/receipts/Acme__Fork/v9.8.7.json"
: >"$temporary_directory/promotion.log"
GH_DRAFT=false run_gate "$partial_state" >/dev/null
jq -e '.phases.releasePublished == true and .phases.availableFeedPublished == true' \
  "$partial_state/receipts/Acme__Fork/v9.8.7.json" >/dev/null
[ ! -s "$temporary_directory/promotion.log" ]

# The channel lock serializes the gate against the maintainer recommendation.
locked_state="$temporary_directory/state-locked"
mkdir -p "$locked_state"
lock_directory="$locked_state/release-channel-Acme__Fork.lock"
mkdir -p "$lock_directory"
jq -n --argjson pid "$$" --arg host "$(hostname)" --arg token held \
  --argjson started_at "$(date +%s)" \
  '{pid:$pid,host:$host,token:$token,startedAt:$started_at}' >"$lock_directory/owner.json"
: >"$temporary_directory/promotion.log"
: >"$temporary_directory/available.log"
set +e
run_gate "$locked_state" >/dev/null 2>&1
[ "$?" -eq 1 ]
set -e
[ ! -s "$temporary_directory/promotion.log" ]
[ ! -s "$temporary_directory/available.log" ]
rm -rf "$lock_directory"
run_gate "$locked_state" >/dev/null
[ ! -d "$lock_directory" ]

set +e
GH_DRAFT=false run_gate "$temporary_directory/state-not-draft"
[ "$?" -eq 1 ]
VERIFY_FAIL=1 run_gate "$temporary_directory/state-verifier"
[ "$?" -eq 1 ]
set -e
[ ! -f "$temporary_directory/state-verifier/release-gate-Acme__Fork-v9.8.7.attempted" ]

# The outer wrapper must execute the gate implementation committed in the tag,
# not a newer implementation from the operator's working tree.
worktree_repository="$temporary_directory/worktree-repository"
mkdir -p "$worktree_repository/canary/host" "$temporary_directory/worktree-bin"
cp "$repository_root/canary/host/lib.sh" "$worktree_repository/canary/host/lib.sh"
cp "$repository_root/canary/host/release-channel.sh" \
  "$worktree_repository/canary/host/release-channel.sh"
cat >"$worktree_repository/canary/host/run-release-gate.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'commit=%s args=%s\n' "$NAN_CANARY_TAG_COMMIT" "$*" >"$WORKTREE_EXECUTION_LOG"
EOF
chmod 755 "$worktree_repository/canary/host/run-release-gate.sh"
git -C "$worktree_repository" init -q
git -C "$worktree_repository" config user.name canary-test
git -C "$worktree_repository" config user.email canary@example.test
git -C "$worktree_repository" add .
git -C "$worktree_repository" commit -qm tagged-gate
git -C "$worktree_repository" tag v1.2.3
tagged_commit="$(git -C "$worktree_repository" rev-parse v1.2.3^{commit})"
cp "$repository_root/canary/host/run-release-gate.sh" \
  "$worktree_repository/canary/host/run-release-gate.sh"
cat >"$temporary_directory/worktree-bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = api ]; then
  printf 'commit\t%s\n' "$REMOTE_TAG_COMMIT"
  exit 0
fi
exit 1
EOF
chmod 755 "$temporary_directory/worktree-bin/gh"
worktree_log="$temporary_directory/worktree-execution.log"
WORKTREE_EXECUTION_LOG="$worktree_log" \
REMOTE_TAG_COMMIT="$tagged_commit" \
NAN_CANARY_STATE_DIR="$temporary_directory/worktree-state" \
NAN_CANARY_PRUNE_STATE_COMMAND="$bin_directory/prune" \
PATH="$temporary_directory/worktree-bin:$PATH" \
  "$worktree_repository/canary/host/run-release-gate.sh" \
    --tag v1.2.3 --repo Acme/Fork --force
grep -Fq "commit=$tagged_commit" "$worktree_log"
grep -Fq 'args=--tag v1.2.3 --repo Acme/Fork --force' "$worktree_log"
