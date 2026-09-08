#!/usr/bin/env bash

# Sourced by publish-compatibility.sh. These functions stage, verify, replace,
# restore and prune the compatibility release assets from the caller's upload
# and base directories. They run only when the caller asked for a real
# publication; the caller keeps the lock, traps and temporary directory
# lifecycle. Failed replacement attempts restoration; if that also fails, the
# backup remains available for recovery on a later run.
#
# Both published assets use the same staging order. The legacy CLI-only feed is
# replaced first and the unified feed second, so a failure while publishing the
# unified asset never leaves existing clients without a valid legacy feed.

publication_checkpoint() {
  local phase="$1"
  if [ "$publication_failure_phase" = "$phase" ] &&
    { [ -z "$publication_failure_asset" ] || [ "$publication_failure_asset" = "$asset_name" ]; }; then
    printf 'injected publication failure at %s for %s\n' "$phase" "$asset_name" >&2
    return 1
  fi
  if [ "$publication_interrupt_phase" = "$phase" ] &&
    { [ -z "$publication_interrupt_asset" ] || [ "$publication_interrupt_asset" = "$asset_name" ]; }; then
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
  if remote_asset_exists "$asset_name"; then
    gh release delete-asset compatibility "$asset_name" \
      --repo "$release_repository" --yes || return 1
  fi
  publication_checkpoint restore-upload || return 1
  restore_source="$base_directory/restore-feed/$asset_name"
  mkdir -p "$(dirname "$restore_source")"
  cp "$asset_base" "$restore_source"
  gh release upload compatibility "$restore_source" \
    --repo "$release_repository" || return 1
  verify_remote_asset "$asset_name" "$asset_base"
}

# Removes abandoned candidates and old backups of one published asset.
cleanup_compatibility_assets() {
  local name="$1"
  local assets_path="$base_directory/cleanup-assets.json"
  local pending_assets_path="$assets_path.pending"
  local candidates_path="$base_directory/cleanup-candidates.txt"
  local backups_path="$base_directory/cleanup-backups.txt"
  local asset_name
  local cleanup_failures=0

  list_compatibility_assets_for_cleanup() {
    rm -f "$pending_assets_path"
    if gh release view compatibility \
      --repo "$release_repository" \
      --json assets >"$pending_assets_path" 2>/dev/null; then
      mv "$pending_assets_path" "$assets_path"
      return 0
    fi
    rm -f "$pending_assets_path"
    return 1
  }

  if ! retry 4 5 list_compatibility_assets_for_cleanup \
    || ! jq -e 'type == "object" and (.assets | type == "array")' "$assets_path" >/dev/null; then
    printf 'warning: compatibility feed was published, but obsolete release assets could not be listed; cleanup will be retried on the next publication\n' >&2
    return 0
  fi

  if ! jq -r --arg prefix "$name.candidate." '
    .assets[] |
    select(.name | startswith($prefix)) |
    .name
  ' "$assets_path" >"$candidates_path" \
    || ! jq -r --arg prefix "$name.backup." '
      [.assets[] |
        select(.name | startswith($prefix)) |
        {name: .name, createdAt: (.createdAt // "")}] |
      sort_by(.createdAt, .name) |
      reverse |
      .[3:][] |
      .name
    ' "$assets_path" >"$backups_path"; then
    printf 'warning: compatibility feed was published, but obsolete release assets could not be selected; cleanup will be retried on the next publication\n' >&2
    return 0
  fi

  while IFS= read -r asset_name; do
    [ -n "$asset_name" ] || continue
    if ! retry 4 5 gh release delete-asset compatibility "$asset_name" \
      --repo "$release_repository" --yes; then
      cleanup_failures=$((cleanup_failures + 1))
    fi
  done <"$candidates_path"
  while IFS= read -r asset_name; do
    [ -n "$asset_name" ] || continue
    if ! retry 4 5 gh release delete-asset compatibility "$asset_name" \
      --repo "$release_repository" --yes; then
      cleanup_failures=$((cleanup_failures + 1))
    fi
  done <"$backups_path"

  if [ "$cleanup_failures" -ne 0 ]; then
    printf 'warning: compatibility feed was published, but %s obsolete release asset(s) could not be removed; cleanup will be retried on the next publication\n' "$cleanup_failures" >&2
  fi
}

# Publishes both validated candidates, legacy asset first.
publish_compatibility_feeds() {
  publication_id="${NAN_CANARY_PUBLICATION_ID:-$(date -u +%Y%m%dT%H%M%SZ)-$$-${RANDOM:-0}}"
  publication_failure_phase="${NAN_CANARY_PUBLICATION_FAIL_PHASE:-}"
  publication_failure_asset="${NAN_CANARY_PUBLICATION_FAIL_ASSET:-}"
  publication_interrupt_phase="${NAN_CANARY_PUBLICATION_INTERRUPT_PHASE:-}"
  publication_interrupt_asset="${NAN_CANARY_PUBLICATION_INTERRUPT_ASSET:-}"

  publish_feed_asset compatibility.json "$base" "$candidate" \
    "$first_publication" "$restored_backup_name" 2
  publish_feed_asset compatibility-v3.json "$base_v3" "$candidate_v3" \
    "$unified_first_publication" "$unified_restored_backup_name" 3
  publish_feed_asset compatibility-v4.json "$base_v4" "$candidate_v4" \
    "$versioned_first_publication" "$versioned_restored_backup_name" 4
}

# Replaces one published asset with its validated candidate. The order is the
# recovery contract: stage the candidate, keep the last known good feed as a
# backup, then swap the stable asset and verify it, attempting restoration if the
# swap does not end in a feed that matches the candidate.
publish_feed_asset() {
  asset_name="$1"
  asset_base="$2"
  local asset_candidate="$3"
  local asset_first_publication="$4"
  local asset_restored_backup="$5"
  local schema="$6"
  local stage_name backup_name stage_source backup_source stable_source

  stage_name="$asset_name.candidate.$publication_id"
  backup_name="${asset_restored_backup:-$asset_name.backup.$publication_id}"
  stage_source="$upload_directory/$stage_name"
  backup_source="$upload_directory/$backup_name"
  stable_source="$upload_directory/$asset_name"
  cp "$asset_candidate" "$stage_source"
  cp "$asset_base" "$backup_source"
  cp "$asset_candidate" "$stable_source"

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
    release_exists=true
    if ! verify_remote_asset "$asset_name" "$asset_candidate"; then
      printf 'newly created compatibility feed did not match the candidate\n' >&2
      exit 1
    fi
    cleanup_compatibility_assets "$asset_name"
    printf 'published schema-v%s compatibility feed: %s\n' "$schema" "$asset_candidate"
    return 0
  fi

  if [ "$asset_first_publication" = true ]; then
    publication_checkpoint first-upload || exit 1
    if ! gh release upload compatibility "$stable_source" \
      --repo "$release_repository"; then
      printf 'could not publish the first compatibility feed asset\n' >&2
      exit 1
    fi
    if ! verify_remote_asset "$asset_name" "$asset_candidate"; then
      printf 'first compatibility feed upload did not match the candidate\n' >&2
      exit 1
    fi
    cleanup_compatibility_assets "$asset_name"
    printf 'published schema-v%s compatibility feed: %s\n' "$schema" "$asset_candidate"
    return 0
  fi

  publication_checkpoint stage-upload || exit 1
  if ! gh release upload compatibility "$stage_source" \
    --repo "$release_repository"; then
    printf 'could not stage the validated compatibility candidate\n' >&2
    exit 1
  fi
  if ! verify_remote_asset "$stage_name" "$asset_candidate"; then
    printf 'staged compatibility candidate did not match the local candidate\n' >&2
    exit 1
  fi

  if [ -z "$asset_restored_backup" ]; then
    publication_checkpoint backup-upload || exit 1
    if ! gh release upload compatibility "$backup_source" \
      --repo "$release_repository"; then
      printf 'could not preserve the last-known-good compatibility feed\n' >&2
      exit 1
    fi
    if ! verify_remote_asset "$backup_name" "$asset_base"; then
      printf 'compatibility backup did not match the last-known-good feed\n' >&2
      exit 1
    fi
  fi

  publication_checkpoint stable-delete || exit 1
  if ! gh release delete-asset compatibility "$asset_name" \
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
  if ! publication_checkpoint stable-verify || ! verify_remote_asset "$asset_name" "$asset_candidate"; then
    printf 'stable compatibility feed did not match the candidate; restoring the last-known-good feed\n' >&2
    restore_previous_feed || printf 'last-known-good compatibility feed restoration also failed\n' >&2
    exit 1
  fi
  cleanup_compatibility_assets "$asset_name"
  printf 'published schema-v%s compatibility feed: %s\n' "$schema" "$asset_candidate"
}
