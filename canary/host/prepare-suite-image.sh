#!/usr/bin/env bash
set -euo pipefail
umask 077

if [ "$#" -ne 5 ]; then
  printf 'usage: %s <linux|macos> <source-image> <prepared-name> <bootstrap-script> <private-log>\n' "$0" >&2
  exit 2
fi

guest="$1"
source_image="$2"
prepared_name="$3"
bootstrap_script="$4"
private_log="$5"
case "$guest" in
  linux|macos) ;;
  *) exit 2 ;;
esac
case "$prepared_name" in
  nhc-suite-*) ;;
  *) printf 'prepared Tart image name is invalid\n' >&2; exit 2 ;;
esac
case "$prepared_name" in
  *[!A-Za-z0-9._-]*) printf 'prepared Tart image name is invalid\n' >&2; exit 2 ;;
esac
[ "${#prepared_name}" -le 128 ] || {
  printf 'prepared Tart image name is invalid\n' >&2
  exit 2
}
[ -f "$bootstrap_script" ] || exit 2
mkdir -p "$(dirname "$private_log")"

network="${NAN_CANARY_NETWORK:-shared}"
case "$network" in
  shared|softnet) ;;
  *) exit 2 ;;
esac
ssh_options=(
  -o StrictHostKeyChecking=no
  -o UserKnownHostsFile=/dev/null
  -o LogLevel=ERROR
  -o IdentitiesOnly=yes
  -o PreferredAuthentications=password
)
run_pid=''
complete=false

cleanup_failed_preparation() {
  if [ "$complete" = true ]; then
    return
  fi
  tart stop "$prepared_name" >/dev/null 2>&1 || true
  if [ -n "$run_pid" ]; then
    kill "$run_pid" >/dev/null 2>&1 || true
    wait "$run_pid" >/dev/null 2>&1 || true
  fi
  tart delete "$prepared_name" >/dev/null 2>&1 || true
}
trap cleanup_failed_preparation EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

tart clone "$source_image" "$prepared_name" >>"$private_log" 2>&1
run_arguments=(run --no-graphics)
if [ "$network" = softnet ]; then
  run_arguments+=(--net-softnet)
fi
run_arguments+=("$prepared_name")
tart "${run_arguments[@]}" >>"$private_log" 2>&1 &
run_pid="$!"

boot_timeout_seconds="${NAN_CANARY_PREPARE_BOOT_TIMEOUT_SECONDS:-300}"
case "$boot_timeout_seconds" in
  ''|*[!0-9]*) exit 2 ;;
esac
deadline="$((SECONDS + boot_timeout_seconds))"
ip=''
while [ "$SECONDS" -lt "$deadline" ]; do
  if ! kill -0 "$run_pid" >/dev/null 2>&1; then
    printf 'Tart exited while preparing %s\n' "$guest" >&2
    exit 1
  fi
  ip="$(tart ip "$prepared_name" 2>/dev/null || true)"
  if [ -n "$ip" ] && sshpass -p admin ssh \
    "${ssh_options[@]}" -o ConnectTimeout=5 "admin@$ip" true >/dev/null 2>&1; then
    break
  fi
  ip=''
  sleep 2
done
[ -n "$ip" ] || { printf 'prepared %s image did not become reachable\n' "$guest" >&2; exit 1; }

sshpass -p admin ssh "${ssh_options[@]}" -o ConnectTimeout=10 "admin@$ip" \
  'umask 077; cat > /tmp/nan-harness-bootstrap.sh; chmod 700 /tmp/nan-harness-bootstrap.sh' \
  <"$bootstrap_script"
sshpass -p admin ssh "${ssh_options[@]}" -o ConnectTimeout=10 "admin@$ip" \
  'bash /tmp/nan-harness-bootstrap.sh; rm -f /tmp/nan-harness-bootstrap.sh' \
  >>"$private_log" 2>&1

tart stop "$prepared_name" >>"$private_log" 2>&1
wait "$run_pid" >/dev/null 2>&1 || true
run_pid=''
complete=true
trap - EXIT INT TERM
printf '%s\n' "$prepared_name"
