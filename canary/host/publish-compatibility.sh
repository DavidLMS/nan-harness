#!/usr/bin/env bash
set -euo pipefail
umask 077

usage() {
  printf 'usage: %s --trigger <daily|weekly|release|manual> --nan-harness-version <version> --release-tag <tag> --reports <directory> --output-dir <directory> --state-dir <directory> --report-validator <path> [--repository <owner/name>] [--publish-feed] [--verified-updates <directory>]\n' "$0" >&2
  exit 2
}

trigger=''
nan_harness_version=''
release_tag=''
reports_directory=''
output_directory=''
state_directory=''
report_validator=''
publish_feed=false
verified_updates=''
release_repository="${NAN_CANARY_COMPATIBILITY_REPOSITORY:-${NAN_CANARY_RELEASE_REPOSITORY:-DavidLMS/nan-harness}}"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --trigger) trigger="${2:-}"; shift 2 ;;
    --nan-harness-version) nan_harness_version="${2:-}"; shift 2 ;;
    --release-tag) release_tag="${2:-}"; shift 2 ;;
    --reports) reports_directory="${2:-}"; shift 2 ;;
    --output-dir) output_directory="${2:-}"; shift 2 ;;
    --state-dir) state_directory="${2:-}"; shift 2 ;;
    --report-validator) report_validator="${2:-}"; shift 2 ;;
    --repository) release_repository="${2:-}"; shift 2 ;;
    --publish-feed) publish_feed=true; shift ;;
    --verified-updates) verified_updates="${2:-}"; shift 2 ;;
    *) usage ;;
  esac
done

case "$trigger" in
  daily|weekly|release|manual) ;;
  *) usage ;;
esac
[ -n "$nan_harness_version" ] && [ -n "$release_tag" ] && [ -d "$reports_directory" ] && [ -n "$output_directory" ] && [ -n "$state_directory" ] || usage
[ -n "$report_validator" ] && [ -f "$report_validator" ] && [ -x "$report_validator" ] || {
  printf 'a usable executable --report-validator is required\n' >&2
  exit 2
}

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repository_root/canary/host/lib.sh"
source "$repository_root/canary/host/host-lock.sh"
source "$repository_root/canary/host/compatibility-reports.sh"
source "$repository_root/canary/host/compatibility-candidate.sh"
source "$repository_root/canary/host/compatibility-publication.sh"
cd "$repository_root"
cargo_command="${NAN_CANARY_CARGO_COMMAND:-}"
if [ -z "$cargo_command" ]; then
  cargo_command="$(command -v cargo 2>/dev/null || true)"
fi
if [ -z "$cargo_command" ] && [ -x "$HOME/.cargo/bin/cargo" ]; then
  cargo_command="$HOME/.cargo/bin/cargo"
fi
if [ -z "$cargo_command" ]; then
  printf 'cargo is required to validate and merge the compatibility feed\n' >&2
  exit 1
fi
cargo_xtask() {
  "$cargo_command" xtask "$@"
}
harnesses=(
  claude-code codex opencode hermes pi omp prime-agent deepseek-harness
  openclaw cline qwen-code kimi-code aider goose fx
)
[ -n "$release_repository" ] || usage
updates_directory="$output_directory/compatibility-updates"
candidate="$output_directory/compatibility.json"
candidate_v3="$output_directory/compatibility-v3.json"
mkdir -p "$updates_directory"

feed_lock="$state_directory/compatibility-feed.lock"

if ! host_lock_acquire "$feed_lock" 'compatibility feed publication'; then
  exit 1
fi

base_directory=''
upload_directory=''
cleanup() {
  if [ -n "$upload_directory" ]; then
    rm -rf "$upload_directory"
  fi
  if [ -n "$base_directory" ]; then
    rm -rf "$base_directory"
  fi
  host_lock_release
}
trap cleanup EXIT

semver_regex='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-((0|[1-9][0-9]*)|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)(\.((0|[1-9][0-9]*)|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*)?(\+([0-9A-Za-z-]+)(\.[0-9A-Za-z-]+)*)?$'
validator_failed=false

# The phases below run in this shell on purpose. Calling one from a subshell, a
# condition or a pipeline would hide its exit status from errexit and lose the
# state it records, and only this shell owns the feed lock and the cleanup trap.
if [ -n "$verified_updates" ]; then
  # The trusted daily aggregator binds reports to all required native platforms,
  # this workflow execution and the signed release before supplying this input.
  [ "$trigger" = daily ] && [ -d "$verified_updates" ] || usage
  if compgen -G "$updates_directory/*.json" >/dev/null; then
    printf 'daily updates require a fresh output directory\n' >&2
    exit 1
  fi
  for update in "$verified_updates"/*.json; do
    [ -f "$update" ] || continue
    jq -e --arg version "$nan_harness_version" '
      (keys | sort) == (["nanHarnessVersion", "id", "lastCompatibleVersion", "compatibleAt", "lastLiveVerifiedVersion", "liveVerifiedAt"] | sort) and
      .nanHarnessVersion == $version and
      (.id as $id | ["claude-code","codex","opencode","hermes","pi","omp","prime-agent","deepseek-harness","openclaw","cline","qwen-code","kimi-code","aider","goose","fx"] | index($id) != null) and
      .lastCompatibleVersion == .lastLiveVerifiedVersion and .compatibleAt == .liveVerifiedAt
    ' "$update" >/dev/null
    update_target="$updates_directory/$(jq -r .id "$update").json"
    [ ! -e "$update_target" ] || { printf 'duplicate daily harness update\n' >&2; exit 1; }
    cp "$update" "$update_target"
  done
else
  select_compatibility_updates
fi
require_publishable_updates

base_directory="$(mktemp -d "$output_directory/.compatibility-base.XXXXXX")"
base="$base_directory/compatibility.json"
base_v3="$base_directory/compatibility-v3.json"
recover_base_feed
migrate_base_feed
build_validated_candidate
recover_unified_base_feed
build_validated_unified_candidate

if [ "$publish_feed" = true ]; then
  if [ -n "$verified_updates" ] && [ "$first_publication" = false ] && [ "$unified_first_publication" = false ] \
    && cmp -s <(jq -S . "$base") <(jq -S . "$candidate") \
    && cmp -s <(jq -S . "$base_v3") <(jq -S . "$candidate_v3"); then
    printf 'compatibility evidence is already published\n'
    exit 0
  fi
  upload_directory="$(mktemp -d "$output_directory/.compatibility-upload.XXXXXX")"
  publish_compatibility_feeds
else
  printf 'dry-run compatibility feed: %s\n' "$candidate"
  printf 'dry-run unified compatibility feed: %s\n' "$candidate_v3"
fi
