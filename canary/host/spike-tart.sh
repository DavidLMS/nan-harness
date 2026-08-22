#!/usr/bin/env bash
set -euo pipefail

image="${1:-ghcr.io/cirruslabs/ubuntu:latest}"
output="${2:-}"
network="${NAN_CANARY_NETWORK:-shared}"
case "$network" in
  shared) ;;
  softnet) ;;
  *) printf 'NAN_CANARY_NETWORK must be shared or softnet\n' >&2; exit 2 ;;
esac
vm="nan-canary-spike-$$-$(date +%s)"
run_pid=''
cleaned=false

cleanup() {
  if [ "$cleaned" = true ]; then
    return
  fi
  if [ -n "$run_pid" ]; then
    kill "$run_pid" >/dev/null 2>&1 || true
    wait "$run_pid" >/dev/null 2>&1 || true
  fi
  tart stop "$vm" >/dev/null 2>&1 || true
  tart delete "$vm" >/dev/null 2>&1 || true
  cleaned=true
}

trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

clone_started="$SECONDS"
tart clone "$image" "$vm"
clone_seconds="$((SECONDS - clone_started))"

boot_started="$SECONDS"
run_arguments=(run --no-graphics)
if [ "$network" = softnet ]; then
  run_arguments+=(--net-softnet)
fi
run_arguments+=("$vm")
tart "${run_arguments[@]}" >/dev/null 2>&1 &
run_pid="$!"
deadline="$((SECONDS + 300))"
ip=''
while [ "$SECONDS" -lt "$deadline" ]; do
  if ! kill -0 "$run_pid" >/dev/null 2>&1; then
    printf 'Tart exited before the VM became reachable\n' >&2
    exit 1
  fi
  ip="$(tart ip "$vm" 2>/dev/null || true)"
  if [ -n "$ip" ] && sshpass -p admin ssh \
    -o StrictHostKeyChecking=no \
    -o UserKnownHostsFile=/dev/null \
    -o LogLevel=ERROR \
    -o ConnectTimeout=5 \
    "admin@$ip" true >/dev/null 2>&1; then
    break
  fi
  ip=''
  sleep 2
done
if [ -z "$ip" ]; then
  printf 'the Tart VM did not become reachable within 300 seconds\n' >&2
  exit 1
fi
boot_seconds="$((SECONDS - boot_started))"

guest="$(sshpass -p admin ssh \
  -o StrictHostKeyChecking=no \
  -o UserKnownHostsFile=/dev/null \
  -o LogLevel=ERROR \
  -o ConnectTimeout=10 \
  "admin@$ip" 'uname -srm')"
host_rss_kib="$(ps -o rss= -p "$run_pid" | tr -d ' ')"
guest_memory_kib="$(sshpass -p admin ssh \
  -o StrictHostKeyChecking=no \
  -o UserKnownHostsFile=/dev/null \
  -o LogLevel=ERROR \
  -o ConnectTimeout=10 \
  "admin@$ip" "grep -Eo '[0-9]+' /proc/meminfo | head -n 1")"
storage_kib="$(du -sk "$HOME/.tart" | awk '{print $1}')"
cleanup_started="$SECONDS"
cleanup
cleanup_seconds="$((SECONDS - cleanup_started))"
trap - EXIT

report="$(jq --null-input \
  --arg image "$image" \
  --arg tartVersion "$(tart --version)" \
  --arg network "$network" \
  --arg guest "$guest" \
  --argjson cloneSeconds "$clone_seconds" \
  --argjson bootSeconds "$boot_seconds" \
  --argjson cleanupSeconds "$cleanup_seconds" \
  --argjson hostRssKib "$host_rss_kib" \
  --argjson guestMemoryKib "$guest_memory_kib" \
  --argjson tartStorageKib "$storage_kib" \
  '{schemaVersion: 1, image: $image, tartVersion: $tartVersion, network: $network, guest: $guest, cloneSeconds: $cloneSeconds, bootSeconds: $bootSeconds, cleanupSeconds: $cleanupSeconds, hostRssKib: $hostRssKib, guestMemoryKib: $guestMemoryKib, tartStorageKib: $tartStorageKib, outcome: "passed"}')"

if [ -n "$output" ]; then
  directory="$(dirname "$output")"
  mkdir -p "$directory"
  temporary="$(mktemp "$directory/.tart-spike.XXXXXX")"
  printf '%s\n' "$report" >"$temporary"
  mv "$temporary" "$output"
fi
printf '%s\n' "$report"
