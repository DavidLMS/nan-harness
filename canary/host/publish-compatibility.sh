#!/usr/bin/env bash
set -euo pipefail
umask 077

usage() {
  printf 'usage: %s --trigger <daily|weekly|release|manual> --nan-harness-version <version> --release-tag <tag> --reports <directory> --output-dir <directory> --state-dir <directory> [--publish-feed]\n' "$0" >&2
  exit 2
}

trigger=''
nan_harness_version=''
release_tag=''
reports_directory=''
output_directory=''
state_directory=''
publish_feed=false
while [ "$#" -gt 0 ]; do
  case "$1" in
    --trigger) trigger="${2:-}"; shift 2 ;;
    --nan-harness-version) nan_harness_version="${2:-}"; shift 2 ;;
    --release-tag) release_tag="${2:-}"; shift 2 ;;
    --reports) reports_directory="${2:-}"; shift 2 ;;
    --output-dir) output_directory="${2:-}"; shift 2 ;;
    --state-dir) state_directory="${2:-}"; shift 2 ;;
    --publish-feed) publish_feed=true; shift ;;
    *) usage ;;
  esac
done

case "$trigger" in
  daily|weekly|release|manual) ;;
  *) usage ;;
esac
[ -n "$nan_harness_version" ] && [ -n "$release_tag" ] && [ -d "$reports_directory" ] && [ -n "$output_directory" ] && [ -n "$state_directory" ] || usage

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repository_root/canary/host/lib.sh"
harnesses=(
  claude-code codex opencode hermes pi prime-agent deepseek-harness
  openclaw cline qwen-code kimi-code aider goose fx
)
updates_directory="$output_directory/compatibility-updates"
candidate="$output_directory/compatibility.json"
mkdir -p "$updates_directory"

feed_lock="$state_directory/compatibility-feed.lock"
mkdir -p "$state_directory"
if ! mkdir "$feed_lock" 2>/dev/null; then
  printf 'another compatibility feed publication is already running\n' >&2
  exit 1
fi
release_feed_lock() {
  rm -rf "$feed_lock"
}
trap release_feed_lock EXIT

safe_report() {
  local report="$1"
  local expected_tier="$2"
  local expected_harness="$3"
  [ -f "$report" ] || return 1
  jq -e \
    --arg expected_version "$nan_harness_version" \
    --arg expected_tier "$expected_tier" \
    --arg expected_trigger "$trigger" \
    --arg expected_harness "$expected_harness" \
    'type == "object" and
      .schemaVersion == 1 and
      .nanHarness.version == $expected_version and
      .trigger == $expected_trigger and
      .tier == $expected_tier and
      .harness.id == $expected_harness and
      (.harness.version | type == "string" and length > 0 and . != "unknown") and
      (.harness.version | test("^[0-9]+\\.[0-9]+\\.[0-9]+([+-][0-9A-Za-z.-]+)?$")) and
      (.checks | type == "array") and
      any(.checks[]; .name == "install-and-diagnose" and .status == "passed") and
      any(.checks[]; .name == "deterministic-conformance" and .status == "passed")' \
    "$report" >/dev/null
}

passed_live_report() {
  local report="$1"
  safe_report "$report" "$2" "$3" || return 1
  jq -e '(.outcome == "passed") and any(.checks[]; .name == "live-tool" and .status == "passed")' "$report" >/dev/null
}

write_update() {
  local output="$1"
  local report="$2"
  local live_report="${3:-}"
  local timestamp
  local version
  version="$(jq -er '.harness.version' "$report")"
  timestamp="$(jq -er '.completedAt' "$report")"
  if [ -n "$live_report" ]; then
    timestamp="$(jq -sr 'max_by(.completedAt).completedAt' "$report" "$live_report")"
  fi
  jq -n \
    --arg nan_harness_version "$nan_harness_version" \
    --arg id "$(jq -er '.harness.id' "$report")" \
    --arg version "$version" \
    --arg compatible_at "$timestamp" \
    --arg live_at "$timestamp" \
    --argjson include_live "$([ -n "$live_report" ] && printf true || printf false)" \
    '({nanHarnessVersion: $nan_harness_version, id: $id,
       lastCompatibleVersion: $version, compatibleAt: $compatible_at} |
      if $include_live then
        .lastLiveVerifiedVersion = $version | .liveVerifiedAt = $live_at
      else . end)' >"$output"
}

for harness in "${harnesses[@]}"; do
  rm -f "$updates_directory/$harness.json"
done

case "$trigger" in
  daily|manual)
    for harness in "${harnesses[@]}"; do
      report="$reports_directory/linux-$harness.json"
      if safe_report "$report" deterministic "$harness" || safe_report "$report" live-core "$harness"; then
        write_update "$updates_directory/$harness.json" "$report"
      fi
    done
    ;;
  weekly)
    for harness in "${harnesses[@]}"; do
      linux_report="$reports_directory/linux-$harness.json"
      macos_report="$reports_directory/macos-$harness.json"
      if passed_live_report "$linux_report" live-extended "$harness" \
        && passed_live_report "$macos_report" live-extended "$harness" \
        && [ "$(jq -er '.harness.version' "$linux_report")" = "$(jq -er '.harness.version' "$macos_report")" ]; then
        write_update "$updates_directory/$harness.json" "$linux_report" "$macos_report"
      fi
    done
    ;;
  release)
    release_ready=true
    for harness in "${harnesses[@]}"; do
      linux_report="$reports_directory/linux-$harness.json"
      macos_report="$reports_directory/macos-$harness.json"
      if ! passed_live_report "$linux_report" release-gate "$harness" \
        || ! passed_live_report "$macos_report" release-gate "$harness" \
        || [ "$(jq -er '.harness.version' "$linux_report")" != "$(jq -er '.harness.version' "$macos_report")" ]; then
        release_ready=false
      fi
    done
    if [ "$release_ready" = true ]; then
      for harness in "${harnesses[@]}"; do
        write_update "$updates_directory/$harness.json" \
          "$reports_directory/linux-$harness.json" \
          "$reports_directory/macos-$harness.json"
      done
    fi
    ;;
esac

if ! compgen -G "$updates_directory/*.json" >/dev/null; then
  printf 'no safe positive compatibility updates were produced\n'
  if [ "$trigger" = release ]; then
    exit 1
  fi
  exit 0
fi

base_directory="$(mktemp -d "$output_directory/.compatibility-base.XXXXXX")"
base="$base_directory/compatibility.json"
cleanup_base() {
  rm -rf "$base_directory"
  release_feed_lock
}
trap cleanup_base EXIT
if ! retry 4 5 gh release download compatibility --pattern compatibility.json --output "$base"; then
  jq -n '{schemaVersion: 2, releases: []}' >"$base"
fi
if ! jq -e 'type == "object" and .schemaVersion == 2 and (.releases | type == "array" and length > 0)' "$base" >/dev/null; then
  jq -n '{schemaVersion: 2, releases: []}' >"$base"
fi

cargo xtask merge-compatibility-feed "$base" "$updates_directory" "$candidate"
cargo xtask validate-compatibility-feed "$candidate"
jq -e 'type == "object" and .schemaVersion == 2 and (.releases | type == "array" and length > 0) and (tostring | length > 2)' "$candidate" >/dev/null

if [ "$publish_feed" = true ]; then
  upload_directory="$(mktemp -d "$output_directory/.compatibility-upload.XXXXXX")"
  upload_source="$upload_directory/compatibility.json"
  cleanup_upload() {
    rm -rf "$upload_directory"
    rm -rf "$base_directory"
    release_feed_lock
  }
  trap cleanup_upload EXIT
  cp "$candidate" "$upload_source"
  if gh release view compatibility >/dev/null 2>&1; then
    retry 4 5 gh release upload compatibility "$upload_source" --clobber
  else
    retry 4 5 gh release create compatibility "$upload_source" \
      --prerelease \
      --target "$(git -C "$repository_root" rev-parse HEAD)" \
      --title "nan-harness compatibility feed" \
      --notes "Machine-readable results from successful scheduled harness conformance runs."
  fi
  printf 'published schema-v2 compatibility feed: %s\n' "$candidate"
else
  printf 'dry-run compatibility feed: %s\n' "$candidate"
fi
