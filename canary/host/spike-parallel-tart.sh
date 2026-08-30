#!/usr/bin/env bash
set -euo pipefail
umask 077

if [ "$#" -gt 1 ]; then
  printf 'usage: %s [output.json]\n' "$0" >&2
  exit 2
fi

output="${1:-}"
repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
prepare_command="${NAN_CANARY_PREPARE_IMAGE_COMMAND:-$repository_root/canary/host/prepare-suite-image.sh}"
sysctl_command="${NAN_CANARY_SYSCTL_COMMAND:-/usr/sbin/sysctl}"
bootstrap="$repository_root/canary/guest/bootstrap.sh"
temporary_directory="$(mktemp -d "${TMPDIR:-/tmp}/nan-harness-parallel-spike.XXXXXX")"
linux_name="nhc-suite-linux-spike-$$-$(date +%s)"
macos_name="nhc-suite-macos-spike-$$-$(date +%s)"
worker_pids=()
worker_status_files=(
  "$temporary_directory/linux.status"
  "$temporary_directory/macos.status"
)

cleanup() {
  local pid prepared
  if [ "${#worker_pids[@]}" -gt 0 ]; then
    for pid in "${worker_pids[@]}"; do kill "$pid" >/dev/null 2>&1 || true; done
    for pid in "${worker_pids[@]}"; do wait "$pid" >/dev/null 2>&1 || true; done
  fi
  for prepared in "$linux_name" "$macos_name"; do
    tart stop "$prepared" >/dev/null 2>&1 || true
    tart delete "$prepared" >/dev/null 2>&1 || true
  done
  rm -rf "$temporary_directory"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

swap_used_mib() {
  "$sysctl_command" -n vm.swapusage | awk '
    { for (i = 1; i <= NF; i++) if ($i == "used") { value = $(i + 2); break } }
    END {
      suffix = substr(value, length(value), 1)
      number = substr(value, 1, length(value) - 1) + 0
      if (suffix == "G") number *= 1024
      printf "%.0f\n", number
    }'
}

pressure_level() {
  "$sysctl_command" -n kern.memorystatus_vm_pressure_level
}

baseline_swap_mib="$(swap_used_mib)"
maximum_swap_mib="$baseline_swap_mib"
maximum_pressure="$(pressure_level)"
started="$SECONDS"

run_preparation_worker() {
  local status_file="$1"
  shift
  set +e
  "$@"
  local status="$?"
  printf '%s\n' "$status" >"$status_file"
  return "$status"
}

run_preparation_worker "${worker_status_files[0]}" \
  "$prepare_command" linux ghcr.io/cirruslabs/ubuntu:latest "$linux_name" \
  "$bootstrap" "$temporary_directory/linux.log" >"$temporary_directory/linux.out" &
worker_pids+=("$!")
run_preparation_worker "${worker_status_files[1]}" \
  "$prepare_command" macos ghcr.io/cirruslabs/macos-tahoe-base:latest "$macos_name" \
  "$bootstrap" "$temporary_directory/macos.log" >"$temporary_directory/macos.out" &
worker_pids+=("$!")

while [ ! -f "${worker_status_files[0]}" ] \
  || [ ! -f "${worker_status_files[1]}" ]; do
  current_pressure="$(pressure_level)"
  current_swap_mib="$(swap_used_mib)"
  [ "$current_pressure" -le "$maximum_pressure" ] || maximum_pressure="$current_pressure"
  [ "$current_swap_mib" -le "$maximum_swap_mib" ] || maximum_swap_mib="$current_swap_mib"
  sleep "${NAN_CANARY_PARALLEL_SPIKE_SAMPLE_SECONDS:-2}"
done

failures=0
for pid in "${worker_pids[@]}"; do
  if ! wait "$pid"; then failures=$((failures + 1)); fi
done
worker_pids=()
duration_seconds="$((SECONDS - started))"
swap_growth_mib="$((maximum_swap_mib - baseline_swap_mib))"
[ "$swap_growth_mib" -ge 0 ] || swap_growth_mib=0
maximum_swap_growth_mib="${NAN_CANARY_MAX_PARALLEL_SWAP_GROWTH_MIB:-1024}"
case "$maximum_swap_growth_mib" in ''|*[!0-9]*) exit 2 ;; esac

outcome=passed
if [ "$failures" -ne 0 ] \
  || [ "$maximum_pressure" -ne 1 ] \
  || [ "$swap_growth_mib" -gt "$maximum_swap_growth_mib" ]; then
  outcome=failed
fi
report="$(jq --null-input \
  --arg outcome "$outcome" \
  --argjson durationSeconds "$duration_seconds" \
  --argjson maximumMemoryPressureLevel "$maximum_pressure" \
  --argjson baselineSwapMiB "$baseline_swap_mib" \
  --argjson maximumSwapMiB "$maximum_swap_mib" \
  --argjson swapGrowthMiB "$swap_growth_mib" \
  '{schemaVersion:1,outcome:$outcome,durationSeconds:$durationSeconds,maximumMemoryPressureLevel:$maximumMemoryPressureLevel,baselineSwapMiB:$baselineSwapMiB,maximumSwapMiB:$maximumSwapMiB,swapGrowthMiB:$swapGrowthMiB,guests:["linux","macos"]}')"
if [ -n "$output" ]; then
  mkdir -p "$(dirname "$output")"
  temporary_output="$(mktemp "$(dirname "$output")/.parallel-spike.XXXXXX")"
  printf '%s\n' "$report" >"$temporary_output"
  mv "$temporary_output" "$output"
fi
printf '%s\n' "$report"
[ "$outcome" = passed ]
