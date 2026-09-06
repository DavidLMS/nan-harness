#!/usr/bin/env bash
set -euo pipefail
umask 077

usage() {
  printf 'usage: %s --trigger <daily|weekly|release|manual> --nan-harness-version <version> --release-tag <tag> --linux-binary <path> --linux-canary-binary <path> --macos-binary <path> --macos-canary-binary <path> --output-dir <path> [--repository <owner/name>] [--harness <id>] [--guest <linux|macos>] [--publish-feed]\n' "$0" >&2
  exit 2
}

trigger=''
nan_harness_version=''
linux_binary=''
linux_canary_binary=''
macos_binary=''
macos_canary_binary=''
output_directory=''
release_tag=''
harness_filter=''
guest_filter=''
publish_feed=false
release_repository="${NAN_CANARY_RELEASE_REPOSITORY:-DavidLMS/nan-harness}"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --trigger) trigger="${2:-}"; shift 2 ;;
    --nan-harness-version) nan_harness_version="${2:-}"; shift 2 ;;
    --linux-binary) linux_binary="${2:-}"; shift 2 ;;
    --linux-canary-binary) linux_canary_binary="${2:-}"; shift 2 ;;
    --macos-binary) macos_binary="${2:-}"; shift 2 ;;
    --macos-canary-binary) macos_canary_binary="${2:-}"; shift 2 ;;
    --output-dir) output_directory="${2:-}"; shift 2 ;;
    --release-tag) release_tag="${2:-}"; shift 2 ;;
    --repository) release_repository="${2:-}"; shift 2 ;;
    --harness) harness_filter="${2:-}"; shift 2 ;;
    --guest) guest_filter="${2:-}"; shift 2 ;;
    --publish-feed) publish_feed=true; shift ;;
    *) usage ;;
  esac
done

case "$trigger" in
  daily|weekly|release|manual) ;;
  *) usage ;;
esac
[ -n "$nan_harness_version" ] && [ -n "$release_tag" ] && [ -n "$output_directory" ] || usage
[ -n "$release_repository" ] || usage
[ "$release_tag" = "v$nan_harness_version" ] || {
  printf 'release tag must exactly match the nan-harness version as v%s\n' "$nan_harness_version" >&2
  exit 2
}
release_asset_path() {
  local path="$1"
  local expected_name="$2"
  [ -f "$path" ] || usage
  [ "$(basename "$path")" = "$expected_name" ] || {
    printf 'release asset path must use the canonical name %s\n' "$expected_name" >&2
    exit 2
  }
}
release_asset_path "$linux_binary" nan-harness-aarch64-unknown-linux-musl
release_asset_path "$linux_canary_binary" nan-harness-canary-aarch64-unknown-linux-musl
release_asset_path "$macos_binary" nan-harness-aarch64-apple-darwin
release_asset_path "$macos_canary_binary" nan-harness-canary-aarch64-apple-darwin
if [ "$trigger" = manual ]; then
  [ -n "$harness_filter" ] && [ -n "$guest_filter" ] || usage
elif [ -n "$harness_filter" ] || [ -n "$guest_filter" ]; then
  usage
fi
if [ -n "$guest_filter" ]; then
  case "$guest_filter" in
    linux|macos) ;;
    *) usage ;;
  esac
fi
network="${NAN_CANARY_NETWORK:-shared}"
case "$network" in
  shared|softnet) ;;
  *) printf 'NAN_CANARY_NETWORK must be shared or softnet\n' >&2; exit 2 ;;
esac

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repository_root/canary/host/lib.sh"
source "$repository_root/canary/host/suite-cleanup.sh"
source "$repository_root/canary/host/suite-run-lane.sh"
output_directory="$(mkdir -p "$output_directory" && cd "$output_directory" && pwd)"
for generated_path in run reports private-logs verifications compatibility-updates compatibility-base.json compatibility.json summary.json; do
  if [ -e "$output_directory/$generated_path" ]; then
    printf 'canary output directory must not contain a previous %s artifact: %s\n' \
      "$generated_path" "$output_directory" >&2
    exit 2
  fi
done
state_directory="${NAN_CANARY_STATE_DIR:-$HOME/Library/Application Support/nan-harness-canary}"
mkdir -p "$state_directory"
suite_lock="$state_directory/suite.lock"
case "$trigger" in
  daily|weekly) default_lock_wait_seconds=7200 ;;
  release|manual) default_lock_wait_seconds=0 ;;
esac
lock_wait_seconds="${NAN_CANARY_LOCK_WAIT_SECONDS:-$default_lock_wait_seconds}"
case "$lock_wait_seconds" in
  ''|*[!0-9]*) printf 'NAN_CANARY_LOCK_WAIT_SECONDS must be a non-negative integer\n' >&2; exit 2 ;;
esac
explicit_suite_deadline="${NAN_CANARY_SUITE_DEADLINE_EPOCH:-}"
if [ -n "$explicit_suite_deadline" ]; then
  suite_deadline="$explicit_suite_deadline"
else
  case "$trigger" in
    daily) default_budget=3600 ;;
    weekly|release) default_budget=7200 ;;
    manual) default_budget=3600 ;;
  esac
  budget_seconds="${NAN_CANARY_SUITE_BUDGET_SECONDS:-$default_budget}"
  case "$budget_seconds" in
    ''|*[!0-9]*) printf 'NAN_CANARY_SUITE_BUDGET_SECONDS must be a positive integer\n' >&2; exit 2 ;;
  esac
  [ "$budget_seconds" -gt 0 ] || { printf 'NAN_CANARY_SUITE_BUDGET_SECONDS must be positive\n' >&2; exit 2; }
  suite_deadline=''
fi
if [ -n "$suite_deadline" ]; then
  case "$suite_deadline" in
    *[!0-9]*) printf 'NAN_CANARY_SUITE_DEADLINE_EPOCH must be an epoch integer\n' >&2; exit 2 ;;
  esac
fi
lock_deadline="$(( $(date +%s) + lock_wait_seconds ))"
until shlock -p "$$" -f "$suite_lock"; do
  if [ "$(date +%s)" -ge "$lock_deadline" ]; then
    printf 'another nan-harness canary suite is already running\n' >&2
    exit 75
  fi
  sleep 60
done
if [ -z "$suite_deadline" ]; then
  # Lock contention is governed by its own bounded wait. Start the execution
  # budget only after this suite actually owns the host.
  suite_deadline="$(( $(date +%s) + budget_seconds ))"
fi

staging_directory=''
prepared_linux_image=''
prepared_macos_image=''
lane_pids=()

release_suite_lock() {
  stop_lane_workers
  cleanup_canary_vms
  delete_prepared_images
  if [ -n "$staging_directory" ]; then
    rm -rf "$staging_directory"
  fi
  rm -f "$suite_lock"
}

trap release_suite_lock EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
cleanup_canary_vms
run_directory="$output_directory/run"
reports_directory="$output_directory/reports"
staging_directory="$(mktemp -d "$output_directory/.verified-release-assets.XXXXXX")"
mkdir -p "$run_directory" "$reports_directory"
cp "$linux_binary" "$staging_directory/nan-harness-aarch64-unknown-linux-musl"
cp "$linux_canary_binary" "$staging_directory/nan-harness-canary-aarch64-unknown-linux-musl"
cp "$macos_binary" "$staging_directory/nan-harness-aarch64-apple-darwin"
cp "$macos_canary_binary" "$staging_directory/nan-harness-canary-aarch64-apple-darwin"
if ! "$repository_root/canary/host/verify-release-assets.sh" \
  --release-tag "$release_tag" \
  --assets-dir "$staging_directory" \
  --repository "$release_repository"; then
  printf 'release assets failed verification; canary execution and publication were blocked\n' >&2
  exit 1
fi
verified_linux_binary="$staging_directory/nan-harness-aarch64-unknown-linux-musl"
verified_linux_canary_binary="$staging_directory/nan-harness-canary-aarch64-unknown-linux-musl"
verified_macos_binary="$staging_directory/nan-harness-aarch64-apple-darwin"
verified_macos_canary_binary="$staging_directory/nan-harness-canary-aarch64-apple-darwin"
chmod 755 \
  "$verified_linux_binary" \
  "$verified_linux_canary_binary" \
  "$verified_macos_binary" \
  "$verified_macos_canary_binary"
cp "$verified_linux_binary" "$run_directory/nan-harness-aarch64-unknown-linux-musl"
cp "$verified_linux_canary_binary" "$run_directory/nan-harness-canary-aarch64-unknown-linux-musl"
cp "$verified_macos_binary" "$run_directory/nan-harness-aarch64-apple-darwin"
cp "$verified_macos_canary_binary" "$run_directory/nan-harness-canary-aarch64-apple-darwin"
cp "$repository_root/canary/guest/bootstrap.sh" "$run_directory/bootstrap.sh"
cp "$repository_root/canary/guest/install-harness.sh" "$run_directory/install-harness.sh"
cp "$repository_root/canary/guest/probe-harness.sh" "$run_directory/probe-harness.sh"
cp "$repository_root/canary/guest/evaluate-conformance.sh" "$run_directory/evaluate-conformance.sh"
chmod 755 "$run_directory"/*

canary="$verified_macos_canary_binary"
harnesses=(
  claude-code codex opencode hermes pi omp prime-agent deepseek-harness
  openclaw cline qwen-code kimi-code aider goose fx
)
if [ -n "$harness_filter" ]; then
  harness_found=false
  for harness in "${harnesses[@]}"; do
    if [ "$harness" = "$harness_filter" ]; then
      harness_found=true
      break
    fi
  done
  [ "$harness_found" = true ] || usage
fi
if [ -n "$guest_filter" ]; then
  guests=("$guest_filter")
else
  guests=(linux)
fi
if [ -z "$guest_filter" ] && [ "$trigger" != daily ] && [ "$trigger" != manual ]; then
  guests+=(macos)
fi
max_parallel_cells="${NAN_CANARY_MAX_PARALLEL_CELLS:-1}"
case "$max_parallel_cells" in
  1|2) ;;
  *) printf 'NAN_CANARY_MAX_PARALLEL_CELLS must be 1 or 2\n' >&2; exit 2 ;;
esac

capabilities="$($canary capabilities 2>/dev/null || true)"
if [ "$trigger" != manual ] && jq --exit-status \
  '.schemaVersion == 1 and .preparedImageOverride == true' \
  <<<"$capabilities" >/dev/null 2>&1; then
  for guest in "${guests[@]}"; do
    case "$guest" in
      linux) source_image='ghcr.io/cirruslabs/ubuntu:latest' ;;
      macos) source_image='ghcr.io/cirruslabs/macos-tahoe-base:latest' ;;
    esac
    prepared_name="nhc-suite-$guest-$$-$(date -u +%s)"
    prepared_log="$output_directory/private-logs/prepared-$guest.log"
    if prepared="$("$repository_root/canary/host/prepare-suite-image.sh" \
      "$guest" "$source_image" "$prepared_name" "$run_directory/bootstrap.sh" "$prepared_log")"; then
      case "$guest" in
        linux) prepared_linux_image="$prepared" ;;
        macos) prepared_macos_image="$prepared" ;;
      esac
    else
      printf 'warning: could not prepare the %s base; cells will bootstrap the canonical image\n' "$guest" >&2
    fi
  done
fi
rotation="$(( $(date -u +%s) / 86400 ))"
failures=0
publication_failed=false
notify_command="${NAN_CANARY_NOTIFY_COMMAND:-$repository_root/canary/host/notify.sh}"

if [ "$max_parallel_cells" -eq 2 ] && [ "${#guests[@]}" -eq 2 ]; then
  for guest in "${guests[@]}"; do
    run_guest_lane "$guest" &
    lane_pids+=("$!")
  done
  for pid in "${lane_pids[@]}"; do
    if ! wait "$pid"; then
      failures=$((failures + 1))
    fi
  done
  lane_pids=()
else
  for guest in "${guests[@]}"; do
    if ! run_guest_lane "$guest"; then
      failures=$((failures + 1))
    fi
  done
fi

publish_arguments=(
  --trigger "$trigger"
  --nan-harness-version "$nan_harness_version"
  --release-tag "${release_tag:-release-$nan_harness_version}"
  --reports "$reports_directory"
  --output-dir "$output_directory"
  --state-dir "$state_directory"
  --report-validator "$canary"
  --repository "$release_repository"
)
if [ "$publish_feed" = true ]; then
  publish_arguments+=(--publish-feed)
fi
publish_compatibility_command="${NAN_CANARY_PUBLISH_COMPATIBILITY_COMMAND:-$repository_root/canary/host/publish-compatibility.sh}"
if ! "$publish_compatibility_command" "${publish_arguments[@]}"; then
  failures=$((failures + 1))
  publication_failed=true
fi

state="$state_directory/aggregate-state.json"
summary="$output_directory/summary.json"
if compgen -G "$reports_directory/*.json" >/dev/null; then
  if "$canary" aggregate --reports "$reports_directory" --state "$state" --summary "$summary"; then
    if ! NAN_CANARY_RELEASE_REPOSITORY="$release_repository" \
      "$repository_root/canary/host/publish-alerts.sh" "$summary"; then
      printf 'warning: canary alerts could not be published; safe reports remain available locally\n' >&2
    fi
  else
    failures=$((failures + 1))
  fi
else
  printf 'canary suite produced no safe reports\n' >&2
  failures=$((failures + 1))
fi

if [ "$failures" -ne 0 ]; then
  failure_message="$trigger run finished with $failures failure(s); successful evidence was retained where possible."
  if [ "$publication_failed" = true ]; then
    failure_message="$trigger run could not publish compatibility evidence and finished with $failures failure(s); successful evidence was retained where possible."
  fi
  if [ "$trigger" != manual ]; then
    "$notify_command" \
      'nan-harness canary failed' \
      "$failure_message" || true
  fi
  exit 1
fi
