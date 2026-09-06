#!/usr/bin/env bash

# Sourced by run-suite.sh. These functions clean the suite-owned lane_pids and
# prepared images; the caller retains the EXIT trap and host lock lifecycle.

cleanup_canary_vms() {
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

stop_lane_workers() {
  local pid
  if [ "${#lane_pids[@]}" -eq 0 ]; then
    return
  fi
  for pid in "${lane_pids[@]}"; do
    if kill -0 "$pid" >/dev/null 2>&1; then
      kill "$pid" >/dev/null 2>&1 || true
    fi
  done
  for pid in "${lane_pids[@]}"; do
    wait "$pid" >/dev/null 2>&1 || true
  done
  lane_pids=()
}

delete_prepared_images() {
  local prepared
  for prepared in "$prepared_linux_image" "$prepared_macos_image"; do
    if [ -n "$prepared" ]; then
      tart stop "$prepared" >/dev/null 2>&1 || true
      tart delete "$prepared" >/dev/null 2>&1 || true
    fi
  done
}
