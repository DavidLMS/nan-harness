#!/usr/bin/env bash
set -euo pipefail
umask 077

usage() {
  printf 'usage: %s --trigger <daily|weekly|release|manual> --nan-harness-version <version> --release-tag <tag> --reports <directory> --output-dir <directory> --state-dir <directory> --report-validator <path> [--repository <owner/name>] [--publish-feed]\n' "$0" >&2
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
harnesses=(
  claude-code codex opencode hermes pi prime-agent deepseek-harness
  openclaw cline qwen-code kimi-code aider goose fx
)
[ -n "$release_repository" ] || usage
updates_directory="$output_directory/compatibility-updates"
candidate="$output_directory/compatibility.json"
mkdir -p "$updates_directory"

feed_lock="$state_directory/compatibility-feed.lock"
lock_owner="$feed_lock/owner.json"
lock_held=false
lock_token=''
lock_host="$(hostname 2>/dev/null || printf unknown)"
lock_stale_seconds="${NAN_CANARY_LOCK_STALE_SECONDS:-21600}"
case "$lock_stale_seconds" in
  ''|*[!0-9]*)
    printf 'NAN_CANARY_LOCK_STALE_SECONDS must be a non-negative integer\n' >&2
    exit 2
    ;;
esac

lock_mtime() {
  local path="$1"
  local value
  if value="$(stat -f %m "$path" 2>/dev/null)" && case "$value" in ''|*[!0-9]*) false ;; *) true ;; esac; then
    printf '%s\n' "$value"
    return 0
  fi
  stat -c %Y "$path" 2>/dev/null
}

retire_stale_lock() {
  local stale_path="$state_directory/.compatibility-feed.lock.stale.$lock_token"
  if ! mv "$feed_lock" "$stale_path" 2>/dev/null; then
    return 1
  fi
  rm -f "$stale_path/owner.json" "$stale_path/.owner.tmp"
  rmdir "$stale_path"
}

acquire_feed_lock() {
  mkdir -p "$state_directory"
  lock_token="$(date -u +%Y%m%dT%H%M%SZ)-$$-${RANDOM:-0}"
  if mkdir "$feed_lock" 2>/dev/null; then
    if ! jq -n \
      --argjson pid "$$" \
      --arg host "$lock_host" \
      --arg token "$lock_token" \
      --argjson started_at "$(date +%s)" \
      '{pid:$pid,host:$host,token:$token,startedAt:$started_at}' >"$feed_lock/.owner.tmp" \
      || ! mv "$feed_lock/.owner.tmp" "$lock_owner"; then
      rm -f "$feed_lock/.owner.tmp"
      rmdir "$feed_lock" 2>/dev/null || true
      return 1
    fi
    lock_held=true
    return 0
  fi

  local owner_pid=''
  local owner_host=''
  local owner_started=''
  if [ -f "$lock_owner" ]; then
    owner_pid="$(jq -er '.pid | numbers' "$lock_owner" 2>/dev/null || true)"
    owner_host="$(jq -er '.host | strings' "$lock_owner" 2>/dev/null || true)"
    owner_started="$(jq -er '.startedAt | numbers' "$lock_owner" 2>/dev/null || true)"
  fi
  if [ "$owner_host" = "$lock_host" ] && [ -n "$owner_pid" ] && [ "$owner_pid" -gt 0 ] 2>/dev/null && kill -0 "$owner_pid" 2>/dev/null; then
    printf 'another compatibility feed publication is already running (pid %s)\n' "$owner_pid" >&2
    return 1
  fi
  if [ "$owner_host" != '' ] && [ "$owner_host" != "$lock_host" ]; then
    printf 'compatibility feed lock belongs to another host: %s\n' "$owner_host" >&2
    return 1
  fi
  local now
  local lock_age
  now="$(date +%s)"
  if [ -n "$owner_started" ]; then
    lock_age=$((now - owner_started))
  else
    lock_age=$((now - $(lock_mtime "$feed_lock")))
  fi
  [ "$lock_age" -ge 0 ] || lock_age=0
  if [ "$lock_age" -lt "$lock_stale_seconds" ]; then
    printf 'compatibility feed lock is not stale (age %ss)\n' "$lock_age" >&2
    return 1
  fi
  if ! retire_stale_lock; then
    printf 'compatibility feed lock changed while recovering a stale owner\n' >&2
    return 1
  fi
  acquire_feed_lock
}

release_feed_lock() {
  if [ "$lock_held" != true ] || [ ! -f "$lock_owner" ]; then
    return 0
  fi
  current_token="$(jq -er '.token | strings' "$lock_owner" 2>/dev/null || true)"
  if [ "$current_token" = "$lock_token" ]; then
    rm -f "$lock_owner"
    rmdir "$feed_lock" 2>/dev/null || true
  fi
  lock_held=false
}

if ! acquire_feed_lock; then
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
  release_feed_lock
}
trap cleanup EXIT

semver_regex='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-((0|[1-9][0-9]*)|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)(\.((0|[1-9][0-9]*)|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*)?(\+([0-9A-Za-z-]+)(\.[0-9A-Za-z-]+)*)?$'
validator_failed=false

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
      .schemaVersion == 1 and
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

base_directory="$(mktemp -d "$output_directory/.compatibility-base.XXXXXX")"
base="$base_directory/compatibility.json"
release_assets_json="$base_directory/release-assets.json"
first_publication=false
stable_asset_exists=false
restored_backup_name=''

prove_release_absent() {
  local response="$base_directory/release-api-response"
  if gh api "repos/$release_repository/releases/tags/compatibility" --include --silent >"$response" 2>&1; then
    return 1
  fi
  grep -Eq '^HTTP/[0-9.]+[[:space:]]+404([[:space:]]|$)' "$response"
}

if gh release view compatibility --repo "$release_repository" --json assets >"$release_assets_json" 2>/dev/null; then
  jq -e 'type == "object" and (.assets | type == "array")' "$release_assets_json" >/dev/null
  release_exists=true
else
  if ! prove_release_absent; then
    printf 'could not prove whether the compatibility release exists\n' >&2
    exit 1
  fi
  release_exists=false
fi

if [ "$release_exists" = true ]; then
  stable_asset_exists="$(jq -r --arg name compatibility.json 'if any(.assets[]; .name == $name) then "true" else "false" end' "$release_assets_json")"
  if [ "$stable_asset_exists" = true ]; then
    if ! retry 4 5 gh release download compatibility \
      --repo "$release_repository" \
      --pattern compatibility.json \
      --output "$base"; then
      printf 'could not read the established compatibility feed\n' >&2
      exit 1
    fi
    [ -s "$base" ] || {
      printf 'the established compatibility feed is empty\n' >&2
      exit 1
    }
  else
    backup_name="$(jq -r '.assets[] | select(.name | startswith("compatibility.json.backup.")) | [.createdAt // "", .name] | @tsv' "$release_assets_json" | sort -r | awk -F '\t' 'NR == 1 { print $2 }')"
    if [ -n "$backup_name" ]; then
      backup_download="$base_directory/backup.json"
      if ! retry 4 5 gh release download compatibility \
        --repo "$release_repository" \
        --pattern "$backup_name" \
        --output "$backup_download"; then
        printf 'could not read the validated compatibility backup\n' >&2
        exit 1
      fi
      cargo xtask validate-compatibility-feed "$backup_download" >/dev/null
      cp "$backup_download" "$base"
      restored_backup_name="$backup_name"
      restore_upload_directory="$base_directory/restore"
      mkdir -p "$restore_upload_directory"
      cp "$backup_download" "$restore_upload_directory/compatibility.json"
      if ! gh release upload compatibility "$restore_upload_directory/compatibility.json" \
        --repo "$release_repository"; then
        printf 'could not restore the compatibility feed from its backup\n' >&2
        exit 1
      fi
      if ! retry 4 5 gh release download compatibility \
        --repo "$release_repository" \
        --pattern compatibility.json \
        --output "$base_directory/restored.json" \
        || ! cmp -s "$backup_download" "$base_directory/restored.json"; then
        printf 'restored compatibility feed did not match its validated backup\n' >&2
        exit 1
      fi
      stable_asset_exists=true
    else
      printf 'compatibility release has no stable feed or validated backup\n' >&2
      exit 1
    fi
  fi
else
  first_publication=true
  jq -n '{schemaVersion: 2, releases: []}' >"$base"
fi

schema_version="$(jq -er '.schemaVersion' "$base" 2>/dev/null || true)"
case "$schema_version" in
  2)
    if [ "$first_publication" != true ]; then
      cargo xtask validate-compatibility-feed "$base" >/dev/null
    fi
    ;;
  1)
    migrated="$base_directory/migrated.json"
    jq -e --arg release_version "$nan_harness_version" '
      if .schemaVersion == 1 and (.harnesses | type == "array") and
        all(.harnesses[];
          type == "object" and
          (.id | type == "string" and length > 0) and
          (.lastCompatibleVersion | type == "string" and length > 0) and
          (.compatibleAt | type == "string" and length > 0) and
          ((has("lastLiveVerifiedVersion") and has("liveVerifiedAt")) or
           ((has("lastLiveVerifiedVersion") | not) and (has("liveVerifiedAt") | not))))
      then {
        schemaVersion: 2,
        releases: [{
          nanHarnessVersion: $release_version,
          verifications: [
            .harnesses[] |
            {id: .id, lastCompatibleVersion: .lastCompatibleVersion, compatibleAt: .compatibleAt} +
            (if has("lastLiveVerifiedVersion") then
              {lastLiveVerifiedVersion: .lastLiveVerifiedVersion, liveVerifiedAt: .liveVerifiedAt}
             else {} end)
          ]
        }]
      }
      else error("invalid schema-v1 compatibility feed")
      end' "$base" >"$migrated"
    mv "$migrated" "$base"
    cargo xtask validate-compatibility-feed "$base" >/dev/null
    ;;
  *)
    printf 'established compatibility feed has an unsupported or malformed schema\n' >&2
    exit 1
    ;;
esac

cargo xtask merge-compatibility-feed "$base" "$updates_directory" "$candidate"
cargo xtask validate-compatibility-feed "$candidate"
jq -e 'type == "object" and .schemaVersion == 2 and (.releases | type == "array" and length > 0) and (tostring | length > 2)' "$candidate" >/dev/null

if ! jq -e \
  --arg target_version "$nan_harness_version" \
  --slurpfile base_manifest "$base" \
  '($base_manifest[0].releases | map(select(.nanHarnessVersion != $target_version))) as $historical |
   (.releases) as $candidate_releases |
   all($historical[]; . as $expected |
     any($candidate_releases[];
       .nanHarnessVersion == $expected.nanHarnessVersion and
       (del(.. | nulls) == ($expected | del(.. | nulls))))) and
   all($candidate_releases[] | select(.nanHarnessVersion != $target_version);
     . as $actual |
     any($historical[];
       .nanHarnessVersion == $actual.nanHarnessVersion and
       (del(.. | nulls) == ($actual | del(.. | nulls)))))' \
  "$candidate" >/dev/null; then
  printf 'candidate changed an established historical release record\n' >&2
  exit 1
fi

preserved_candidate="$candidate.preserved"
jq \
  --arg target_version "$nan_harness_version" \
  --slurpfile base_manifest "$base" \
  '($base_manifest[0].releases | map(select(.nanHarnessVersion != $target_version))) as $historical |
   .releases |= map(
     if .nanHarnessVersion == $target_version then .
     else . as $actual | $historical[] | select(.nanHarnessVersion == $actual.nanHarnessVersion)
     end)' \
  "$candidate" >"$preserved_candidate"
mv "$preserved_candidate" "$candidate"

if ! jq -e \
  --arg target_version "$nan_harness_version" \
  --slurpfile base_manifest "$base" \
  '($base_manifest[0].releases | map(select(.nanHarnessVersion != $target_version))) as $historical |
   (.releases) as $candidate_releases |
   all($historical[]; . as $expected |
     any($candidate_releases[];
       .nanHarnessVersion == $expected.nanHarnessVersion and . == $expected))' \
  "$candidate" >/dev/null; then
  printf 'candidate did not preserve an established historical release record exactly\n' >&2
  exit 1
fi
cargo xtask validate-compatibility-feed "$candidate" >/dev/null

if [ "$publish_feed" = true ]; then
  upload_directory="$(mktemp -d "$output_directory/.compatibility-upload.XXXXXX")"
  publication_id="${NAN_CANARY_PUBLICATION_ID:-$(date -u +%Y%m%dT%H%M%SZ)-$$-${RANDOM:-0}}"
  stage_name="compatibility.json.candidate.$publication_id"
  backup_name="${restored_backup_name:-compatibility.json.backup.$publication_id}"
  stage_source="$upload_directory/$stage_name"
  backup_source="$upload_directory/$backup_name"
  stable_source="$upload_directory/compatibility.json"
  cp "$candidate" "$stage_source"
  cp "$base" "$backup_source"
  cp "$candidate" "$stable_source"
  publication_failure_phase="${NAN_CANARY_PUBLICATION_FAIL_PHASE:-}"
  publication_interrupt_phase="${NAN_CANARY_PUBLICATION_INTERRUPT_PHASE:-}"

  publication_checkpoint() {
    local phase="$1"
    if [ "$publication_failure_phase" = "$phase" ]; then
      printf 'injected publication failure at %s\n' "$phase" >&2
      return 1
    fi
    if [ "$publication_interrupt_phase" = "$phase" ]; then
      kill -KILL "$$"
    fi
    return 0
  }

  remote_asset_exists() {
    local name="$1"
    local assets_path="$base_directory/current-assets.json"
    gh release view compatibility --repo "$release_repository" --json assets >"$assets_path" 2>/dev/null \
      && jq -e --arg name "$name" 'any(.assets[]; .name == $name)' "$assets_path" >/dev/null
  }

  verify_remote_asset() {
    local name="$1"
    local expected="$2"
    local downloaded="$base_directory/verify-${name//[^A-Za-z0-9_.-]/_}"
    retry 4 5 gh release download compatibility \
      --repo "$release_repository" \
      --pattern "$name" \
      --output "$downloaded" \
      || return 1
    cmp -s "$expected" "$downloaded"
  }

  restore_previous_feed() {
    if remote_asset_exists compatibility.json; then
      gh release delete-asset compatibility compatibility.json \
        --repo "$release_repository" --yes || return 1
    fi
    publication_checkpoint restore-upload || return 1
    restore_source="$base_directory/restore-feed/compatibility.json"
    mkdir -p "$(dirname "$restore_source")"
    cp "$base" "$restore_source"
    gh release upload compatibility "$restore_source" \
      --repo "$release_repository" || return 1
    verify_remote_asset compatibility.json "$base"
  }

  if [ "$release_exists" != true ]; then
    publication_checkpoint first-create || exit 1
    if ! gh release create compatibility "$stable_source" \
      --repo "$release_repository" \
      --prerelease \
      --target "$(git -C "$repository_root" rev-parse HEAD)" \
      --title "nan-harness compatibility feed" \
      --notes "Machine-readable results from successful scheduled harness conformance runs."; then
      printf 'could not create the compatibility release\n' >&2
      exit 1
    fi
    if ! verify_remote_asset compatibility.json "$candidate"; then
      printf 'newly created compatibility feed did not match the candidate\n' >&2
      exit 1
    fi
    printf 'published schema-v2 compatibility feed: %s\n' "$candidate"
    exit 0
  fi

  if [ "$first_publication" = true ]; then
    publication_checkpoint first-upload || exit 1
    if ! gh release upload compatibility "$stable_source" \
      --repo "$release_repository"; then
      printf 'could not publish the first compatibility feed asset\n' >&2
      exit 1
    fi
    if ! verify_remote_asset compatibility.json "$candidate"; then
      printf 'first compatibility feed upload did not match the candidate\n' >&2
      exit 1
    fi
    printf 'published schema-v2 compatibility feed: %s\n' "$candidate"
    exit 0
  fi

  publication_checkpoint stage-upload || exit 1
  if ! gh release upload compatibility "$stage_source" \
    --repo "$release_repository"; then
    printf 'could not stage the validated compatibility candidate\n' >&2
    exit 1
  fi
  if ! verify_remote_asset "$stage_name" "$candidate"; then
    printf 'staged compatibility candidate did not match the local candidate\n' >&2
    exit 1
  fi

  if [ -z "$restored_backup_name" ]; then
    publication_checkpoint backup-upload || exit 1
    if ! gh release upload compatibility "$backup_source" \
      --repo "$release_repository"; then
      printf 'could not preserve the last-known-good compatibility feed\n' >&2
      exit 1
    fi
    if ! verify_remote_asset "$backup_name" "$base"; then
      printf 'compatibility backup did not match the last-known-good feed\n' >&2
      exit 1
    fi
  fi

  publication_checkpoint stable-delete || exit 1
  if ! gh release delete-asset compatibility compatibility.json \
    --repo "$release_repository" --yes; then
    printf 'could not remove the stable compatibility feed before replacement\n' >&2
    exit 1
  fi
  publication_checkpoint after-stable-delete || exit 1
  if ! gh release upload compatibility "$stable_source" \
    --repo "$release_repository"; then
    printf 'stable compatibility feed upload failed; restoring the last-known-good feed\n' >&2
    restore_previous_feed || printf 'last-known-good compatibility feed restoration also failed\n' >&2
    exit 1
  fi
  if ! publication_checkpoint stable-verify || ! verify_remote_asset compatibility.json "$candidate"; then
    printf 'stable compatibility feed did not match the candidate; restoring the last-known-good feed\n' >&2
    restore_previous_feed || printf 'last-known-good compatibility feed restoration also failed\n' >&2
    exit 1
  fi
  printf 'published schema-v2 compatibility feed: %s\n' "$candidate"
else
  printf 'dry-run compatibility feed: %s\n' "$candidate"
fi
