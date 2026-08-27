#!/usr/bin/env bash
set -euo pipefail
umask 077

state_directory="${NAN_CANARY_STATE_DIR:-$HOME/Library/Application Support/nan-harness-canary}"
runs="$state_directory/runs"
assets="$state_directory/assets"
case "$state_directory" in
  ''|'/'|"$HOME")
    printf 'refusing unsafe canary state directory: %s\n' "$state_directory" >&2
    exit 2
    ;;
esac

if [ -d "$runs" ]; then
  while IFS= read -r run; do
    [ -f "$run/KEEP" ] && continue
    rm -rf "$run"
  done < <(find "$runs" -mindepth 1 -maxdepth 1 -type d -mtime +90 -print)

  while IFS= read -r run; do
    [ -f "$run/KEEP" ] && continue
    find "$run" -mindepth 1 -maxdepth 1 -type d \
      \( -name run -o -name private-logs -o -name verifications -o -name compatibility-updates \) \
      -exec rm -rf {} +
  done < <(find "$runs" -mindepth 1 -maxdepth 1 -type d -mtime +30 -print)
fi

if [ -d "$assets" ]; then
  asset_mtime() {
    stat -f %m "$1" 2>/dev/null || stat -c %Y "$1"
  }
  kept=0
  while IFS= read -r asset_directory; do
    [ -n "$asset_directory" ] || continue
    if [ -f "$asset_directory/KEEP" ]; then
      continue
    fi
    kept=$((kept + 1))
    if [ "$kept" -gt 3 ]; then
      rm -rf "$asset_directory"
    fi
  done < <(
    {
      while IFS= read -r asset_directory; do
        printf '%s\t%s\n' "$(asset_mtime "$asset_directory")" "$asset_directory"
      done < <(find "$assets" -mindepth 1 -maxdepth 1 -type d -print)
    } | sort -rn | cut -f2-
  )
fi
