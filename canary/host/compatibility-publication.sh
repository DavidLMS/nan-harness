#!/usr/bin/env bash

# Sourced by publish-compatibility.sh. These functions stage, verify, replace,
# restore and prune the compatibility release assets from the caller's upload
# and base directories. They run only when the caller asked for a real
# publication; the caller keeps the lock, traps and temporary directory
# lifecycle, and every failure here returns to it or exits, so the feed is
# either replaced and verified or restored to its last known good state.

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

cleanup_compatibility_assets() {
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

  if ! jq -r '
    .assets[] |
    select(.name | startswith("compatibility.json.candidate.")) |
    .name
  ' "$assets_path" >"$candidates_path" \
    || ! jq -r '
      [.assets[] |
        select(.name | startswith("compatibility.json.backup.")) |
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

# Replaces the published feed with the validated candidate. The order is the
# recovery contract: stage the candidate, keep the last known good feed as a
# backup, then swap the stable asset and verify it, restoring the backup if the
# swap does not end in a feed that matches the candidate.
publish_compatibility_feed() {
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
    cleanup_compatibility_assets
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
    cleanup_compatibility_assets
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
  cleanup_compatibility_assets
  printf 'published schema-v2 compatibility feed: %s\n' "$candidate"
}
