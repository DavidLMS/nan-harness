#!/usr/bin/env bash

# Sourced by the CLI and Desktop publishers under the single publication writer.
# Older assets are never changed when adopting exact-version Desktop evidence.
recover_versioned_base_feed() {
  versioned_first_publication=false
  versioned_restored_backup_name=''
  if [ "$release_exists" = true ] &&
    jq -e 'any(.assets[]; .name == "compatibility-v4.json")' "$release_assets_json" >/dev/null; then
    retry 4 5 gh release download compatibility --repo "$release_repository" \
      --pattern compatibility-v4.json --output "$base_v4"
    cargo_xtask validate-versioned-compatibility-feed "$base_v4" >/dev/null
    return
  fi
  local backup_name=''
  if [ "$release_exists" = true ]; then
    backup_name="$(jq -r '[.assets[] | select(.name | startswith("compatibility-v4.json.backup."))] | sort_by(.createdAt, .name) | last | .name // empty' "$release_assets_json")"
  fi
  if [ -n "$backup_name" ]; then
    retry 4 5 gh release download compatibility --repo "$release_repository" \
      --pattern "$backup_name" --output "$base_v4"
    cargo_xtask validate-versioned-compatibility-feed "$base_v4" >/dev/null
    versioned_restored_backup_name="$backup_name"
    if [ "${publish_feed:-true}" != true ]; then
      return
    fi
    local restored="$base_directory/versioned-restore/compatibility-v4.json"
    mkdir -p "$(dirname "$restored")"
    cp "$base_v4" "$restored"
    gh release upload compatibility "$restored" --repo "$release_repository"
    verify_remote_asset compatibility-v4.json "$base_v4"
    return
  fi
  # The v3 feed is the lossless migration source: no legacy evidence gains an
  # architecture or a new success timestamp simply because the schema changes.
  jq '.schemaVersion = 4' "$base_v3" >"$base_v4"
  # A first CLI publication starts from an empty migration seed; its candidate
  # is validated after merging the accepted run, before any upload.
  if jq -e '.releases | length > 0' "$base_v4" >/dev/null; then
    cargo_xtask validate-versioned-compatibility-feed "$base_v4" >/dev/null
  fi
  versioned_first_publication=true
}

build_validated_versioned_candidate() {
  cargo_xtask merge-versioned-compatibility-feed "$base_v4" "$updates_directory" "$candidate_v4"
  cargo_xtask validate-versioned-compatibility-feed "$candidate_v4"
}
