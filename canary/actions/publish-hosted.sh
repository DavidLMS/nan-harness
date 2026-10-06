#!/usr/bin/env bash
set -euo pipefail
umask 077

# Inputs are produced by the trusted publication reader. This script executes
# only checkout code and defaults to a local candidate; it never runs artifacts.
updates_directory=''
registry=''
version=''
release_repository=''
output=''
publish_feed=false
while [ "$#" -gt 0 ]; do
  case "$1" in
    --updates) updates_directory="${2:-}"; shift 2 ;;
    --registry) registry="${2:-}"; shift 2 ;;
    --version) version="${2:-}"; shift 2 ;;
    --repository) release_repository="${2:-}"; shift 2 ;;
    --output) output="${2:-}"; shift 2 ;;
    --publish) publish_feed=true; shift ;;
    *) exit 2 ;;
  esac
done
[ -d "$updates_directory" ] && [ -f "$registry" ] && [ -n "$output" ] || exit 2
[[ "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || exit 2
[[ "$release_repository" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || exit 2
repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repository_root/canary/host/lib.sh"
source "$repository_root/canary/host/release-channel.sh"
source "$repository_root/canary/host/compatibility-publication.sh"
source "$repository_root/canary/host/publication-writer.sh"
if [ "$publish_feed" = true ]; then
  # The hosted-only path deliberately has no emergency/local writer fallback.
  [ "${NAN_CANARY_WRITER:-}" = actions ] || exit 1
  require_publication_writer "$release_repository"
fi
cd "$repository_root"
cargo_xtask() { cargo run --locked --quiet -p xtask -- "$@"; }
base_directory="$(mktemp -d "${TMPDIR:-/tmp}/nan-hosted-publication.XXXXXX")"
trap 'rm -rf "$base_directory"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
upload_directory="$base_directory/upload"
mkdir -p "$upload_directory"
base="$base_directory/base.json"
candidate="$base_directory/candidate.json"
release_exists=false
first_publication=true
restored_backup_name=''
presence="$(channel_release_presence "$release_repository" compatibility "$base_directory/presence.json")"
if [ "$presence" = present ]; then
  release_exists=true
  gh release view compatibility --repo "$release_repository" --json assets >"$base_directory/assets.json"
  # Prefer the newest schema; an interrupted swap's backup wins over migrating
  # an older feed, which would silently discard already-published observations.
  for schema in 5 4 3 2; do
    name="compatibility-v$schema.json"
    [ "$schema" != 2 ] || name=compatibility.json
    selected="$(jq -r --arg name "$name" '
      if any(.assets[]; .name == $name) then $name else
        [.assets[] | select(.name | startswith($name + ".backup."))] |
        sort_by(.createdAt, .name) | last | .name // empty end' "$base_directory/assets.json")"
    [ -n "$selected" ] || continue
    retry 4 5 gh release download compatibility --repo "$release_repository" \
      --pattern "$selected" --output "$base"
    case "$schema" in
      5) validator=validate-hosted-compatibility-feed ;;
      4) validator=validate-versioned-compatibility-feed ;;
      3) validator=validate-unified-compatibility-feed ;;
      2) validator=validate-compatibility-feed ;;
    esac
    cargo_xtask "$validator" "$base"
    if [ "$schema" = 5 ]; then
      if [ "$selected" = "$name" ]; then
        first_publication=false
      else
        restored_backup_name="$selected"
      fi
    else
      jq '.schemaVersion = 5' "$base" >"$base_directory/migrated.json"
      mv "$base_directory/migrated.json" "$base"
    fi
    break
  done
fi
if [ ! -f "$base" ]; then
  cargo_xtask hosted-compatibility-feed "$base"
fi
cargo_xtask merge-hosted-checks "$base" "$updates_directory" "$registry" "$version" "$candidate"
cargo_xtask validate-hosted-compatibility-feed "$candidate"
cp "$candidate" "$output"
if [ "$publish_feed" != true ]; then
  printf 'validated schema-v5 candidate; remote feeds are unchanged\n'
  exit 0
fi
publication_id="${NAN_CANARY_PUBLICATION_ID:-$(date -u +%Y%m%dT%H%M%SZ)-$$-${RANDOM:-0}}"
publication_failure_phase="${NAN_CANARY_PUBLICATION_FAIL_PHASE:-}"
publication_failure_asset="${NAN_CANARY_PUBLICATION_FAIL_ASSET:-}"
publication_interrupt_phase="${NAN_CANARY_PUBLICATION_INTERRUPT_PHASE:-}"
publication_interrupt_asset="${NAN_CANARY_PUBLICATION_INTERRUPT_ASSET:-}"
publish_feed_asset compatibility-v5.json "$base" "$candidate" \
  "$first_publication" "$restored_backup_name" 5
