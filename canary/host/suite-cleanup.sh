#!/usr/bin/env bash

suite_cleanup_canary_vms() {
  local vm
  while IFS= read -r vm; do
    case "$vm" in
      nan-harness-canary-*)
        tart stop "$vm" >/dev/null 2>&1 || true
        tart delete "$vm" >/dev/null 2>&1 || true
        ;;
    esac
  done < <(tart list --source local --quiet 2>/dev/null || true)
}

suite_stop_lane_workers() {
  local pid
  for pid in "$@"; do
    if kill -0 "$pid" >/dev/null 2>&1; then
      kill "$pid" >/dev/null 2>&1 || true
    fi
  done
  for pid in "$@"; do
    wait "$pid" >/dev/null 2>&1 || true
  done
}

suite_delete_prepared_images() {
  local prepared
  for prepared in "$@"; do
    if [ -n "$prepared" ]; then
      tart stop "$prepared" >/dev/null 2>&1 || true
      tart delete "$prepared" >/dev/null 2>&1 || true
    fi
  done
}

suite_remove_staging_directory() {
  local staging_directory="$1"
  if [ -n "$staging_directory" ]; then
    rm -rf "$staging_directory"
  fi
}

suite_release_lock() {
  rm -f "$1"
}
