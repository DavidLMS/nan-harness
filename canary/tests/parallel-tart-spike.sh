#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
bin_directory="$temporary_directory/bin"
mkdir -p "$bin_directory"

cat >"$bin_directory/prepare" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
sleep 0.1
printf '%s\n' "$3"
EOF
cat >"$bin_directory/sysctl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "${*: -1}" in
  vm.swapusage) printf 'total = 2048.00M  used = %sM  free = 1024.00M\n' "${SPIKE_SWAP_USED_MIB:-1000}" ;;
  kern.memorystatus_vm_pressure_level) printf '%s\n' "${SPIKE_PRESSURE_LEVEL:-1}" ;;
  *) exit 1 ;;
esac
EOF
cat >"$bin_directory/tart" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod 755 "$bin_directory"/*

spike="$repository_root/canary/host/spike-parallel-tart.sh"
report="$temporary_directory/report.json"
PATH="$bin_directory:$PATH" \
NAN_CANARY_PREPARE_IMAGE_COMMAND="$bin_directory/prepare" \
NAN_CANARY_SYSCTL_COMMAND="$bin_directory/sysctl" \
NAN_CANARY_PARALLEL_SPIKE_SAMPLE_SECONDS=1 \
  "$spike" "$report" >/dev/null
jq -e '.outcome == "passed" and .guests == ["linux", "macos"]' "$report" >/dev/null

if PATH="$bin_directory:$PATH" \
  SPIKE_PRESSURE_LEVEL=2 \
  NAN_CANARY_PREPARE_IMAGE_COMMAND="$bin_directory/prepare" \
  NAN_CANARY_SYSCTL_COMMAND="$bin_directory/sysctl" \
  NAN_CANARY_PARALLEL_SPIKE_SAMPLE_SECONDS=1 \
  "$spike" >/dev/null; then
  printf 'parallel Tart spike accepted warning memory pressure\n' >&2
  exit 1
fi
