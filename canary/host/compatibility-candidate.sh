#!/usr/bin/env bash

# Sourced by publish-compatibility.sh. These functions recover the established
# feed into "$base", bring it to the current schema and merge the selected
# updates into the validated "$candidate". They read and write inside the
# caller's base directory and set the state the publication phase needs
# (release_exists, first_publication, restored_backup_name); the caller keeps
# the lock, traps and temporary directory lifecycle.

prove_release_absent() {
  local response="$base_directory/release-api-response"
  if gh api "repos/$release_repository/releases/tags/compatibility" --include --silent >"$response" 2>&1; then
    return 1
  fi
  grep -Eq '^HTTP/[0-9.]+[[:space:]]+404([[:space:]]|$)' "$response"
}

# Reads the established feed into "$base", or starts an empty schema-v2 feed
# when the compatibility release does not exist yet. A release whose stable
# asset is missing is repaired from its newest validated backup before the
# publication continues, so a run interrupted mid-replacement never becomes the
# reason to drop the feed's history.
recover_base_feed() {
  local backup_name backup_download
  release_assets_json="$base_directory/release-assets.json"
  first_publication=false
  stable_asset_exists=false
  restored_backup_name=''

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
        cargo_xtask validate-compatibility-feed "$backup_download" >/dev/null
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
}

# Accepts the recovered feed only at a schema this publisher understands,
# migrating a schema-v1 feed into the current release-indexed shape.
migrate_base_feed() {
  local migrated
  schema_version="$(jq -er '.schemaVersion' "$base" 2>/dev/null || true)"
  case "$schema_version" in
    2)
      if [ "$first_publication" != true ]; then
        cargo_xtask validate-compatibility-feed "$base" >/dev/null
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
      cargo_xtask validate-compatibility-feed "$base" >/dev/null
      ;;
    *)
      printf 'established compatibility feed has an unsupported or malformed schema\n' >&2
      exit 1
      ;;
  esac
}

# Merges the selected updates into the candidate and proves that it changed
# nothing but the target release: established history must retain the same JSON values.
build_validated_candidate() {
  local preserved_candidate
  cargo_xtask merge-compatibility-feed "$base" "$updates_directory" "$candidate"
  cargo_xtask validate-compatibility-feed "$candidate"
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
  cargo_xtask validate-compatibility-feed "$candidate" >/dev/null
}
