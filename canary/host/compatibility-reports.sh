#!/usr/bin/env bash

# Sourced by publish-compatibility.sh. These functions turn the harness reports
# of one run into per-harness feed updates in the caller's updates directory,
# using its trigger, expected nan-harness version and report validator. They
# fail closed: a report the validator rejects sets validator_failed, which
# discards every update. The caller keeps the lock, traps and temporary
# directory lifecycle.

safe_report() {
  local report="$1"
  local expected_tier="$2"
  local expected_harness="$3"
  [ -f "$report" ] || return 1
  if ! "$report_validator" validate-report "$report" >/dev/null 2>&1; then
    validator_failed=true
    return 1
  fi
  jq -e \
    --arg expected_version "$nan_harness_version" \
    --arg expected_tier "$expected_tier" \
    --arg expected_trigger "$trigger" \
    --arg expected_harness "$expected_harness" \
    --arg semver_regex "$semver_regex" \
    'type == "object" and
      (.schemaVersion == 1 or .schemaVersion == 2) and
      .outcome == "passed" and
      .nanHarness.version == $expected_version and
      .trigger == $expected_trigger and
      .tier == $expected_tier and
      .harness.id == $expected_harness and
      (.harness.version | type == "string" and length > 0 and . != "unknown" and test($semver_regex)) and
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

# Rewrites the updates directory for the current trigger. Only reports that pass
# complete validation for the tier the trigger requires become updates; the
# release trigger publishes all harnesses or none.
select_compatibility_updates() {
  local harness report linux_report macos_report release_ready
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
}

# Stops the publication unless the selection produced a usable candidate set. An
# empty selection is only an error for the release trigger.
require_publishable_updates() {
  if [ "$validator_failed" = true ]; then
    rm -f "$updates_directory"/*.json
    printf 'at least one report failed complete validation; no compatibility feed candidate was produced\n' >&2
    exit 1
  fi

  if ! compgen -G "$updates_directory/*.json" >/dev/null; then
    printf 'no safe positive compatibility updates were produced\n'
    if [ "$trigger" = release ] || [ "$validator_failed" = true ]; then
      exit 1
    fi
    exit 0
  fi
}
