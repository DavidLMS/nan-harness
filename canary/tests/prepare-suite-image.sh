#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
bin_directory="$temporary_directory/bin"
mkdir -p "$bin_directory"
operation_log="$temporary_directory/operations.log"
run_pid_file="$temporary_directory/run.pid"
copied_bootstrap="$temporary_directory/copied-bootstrap.sh"

cat >"$bin_directory/tart" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'tart %s\n' "$*" >>"$PREPARE_OPERATION_LOG"
case "${1:-}" in
  clone|delete) exit 0 ;;
  ip) printf '192.0.2.10\n' ;;
  run)
    printf '%s\n' "$$" >"$PREPARE_RUN_PID_FILE"
    trap 'exit 0' TERM INT
    while true; do sleep 1; done
    ;;
  stop)
    if [ -f "$PREPARE_RUN_PID_FILE" ]; then
      kill "$(cat "$PREPARE_RUN_PID_FILE")" >/dev/null 2>&1 || true
    fi
    ;;
  *) exit 1 ;;
esac
EOF
cat >"$bin_directory/sshpass" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'sshpass %s\n' "$*" >>"$PREPARE_OPERATION_LOG"
[ "${PREPARE_SSH_FAILURE:-0}" != 1 ] || exit 1
case "$*" in
  *'cat > /tmp/nan-harness-bootstrap.sh'*) cat >"$PREPARE_COPIED_BOOTSTRAP" ;;
esac
exit 0
EOF
chmod 755 "$bin_directory/tart" "$bin_directory/sshpass"

common_environment=(
  PATH="$bin_directory:/usr/bin:/bin"
  PREPARE_OPERATION_LOG="$operation_log"
  PREPARE_RUN_PID_FILE="$run_pid_file"
  PREPARE_COPIED_BOOTSTRAP="$copied_bootstrap"
)
helper="$repository_root/canary/host/prepare-suite-image.sh"
bootstrap="$repository_root/canary/guest/bootstrap.sh"

prepared="$(env "${common_environment[@]}" "$helper" \
  linux source-image nhc-suite-linux-test "$bootstrap" "$temporary_directory/private.log")"
[ "$prepared" = nhc-suite-linux-test ]
cmp -s "$bootstrap" "$copied_bootstrap"
grep -Fq 'tart clone source-image nhc-suite-linux-test' "$operation_log"
grep -Fq 'tart stop nhc-suite-linux-test' "$operation_log"
if grep -Fq 'NAN_API_KEY' "$operation_log"; then
  printf 'prepared image helper exposed the provider credential name\n' >&2
  exit 1
fi

: >"$operation_log"
if env "${common_environment[@]}" PREPARE_SSH_FAILURE=1 \
  NAN_CANARY_PREPARE_BOOT_TIMEOUT_SECONDS=0 \
  "$helper" macos source-image nhc-suite-macos-failed "$bootstrap" \
  "$temporary_directory/failed.log" >/dev/null 2>&1; then
  printf 'prepared image helper accepted an unreachable VM\n' >&2
  exit 1
fi
grep -Fq 'tart delete nhc-suite-macos-failed' "$operation_log"

if env "${common_environment[@]}" "$helper" \
  linux source-image 'nhc-suite-linux/unsafe' "$bootstrap" \
  "$temporary_directory/invalid.log" >/dev/null 2>&1; then
  printf 'prepared image helper accepted an unsafe image name\n' >&2
  exit 1
fi
