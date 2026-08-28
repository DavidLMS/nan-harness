#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT

home_directory="$temporary_directory/home"
bin_directory="$temporary_directory/bin"
sshpass_log="$temporary_directory/sshpass.log"
output_file="$temporary_directory/report.json"
mkdir -p "$home_directory/.tart" "$bin_directory"

cat >"$bin_directory/tart" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

case "${1:-}" in
  clone)
    exit 0
    ;;
  run)
    exec sleep 600
    ;;
  ip)
    printf '%s\n' '192.168.64.2'
    ;;
  stop|delete)
    exit 0
    ;;
  --version)
    printf '%s\n' 'tart 2.32.1-test'
    ;;
  *)
    printf 'unexpected tart invocation: %s\n' "$*" >&2
    exit 2
    ;;
esac
EOF

cat >"$bin_directory/sshpass" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

printf '%s\n' "$*" >>"$SSHPASS_LOG"
case "${!#}" in
  true)
    ;;
  'uname -srm')
    printf '%s\n' 'Linux 7.0.0-28-generic aarch64'
    ;;
  *)
    printf '%s\n' '123456'
    ;;
esac
EOF
chmod 755 "$bin_directory/tart" "$bin_directory/sshpass"

HOME="$home_directory" \
SSHPASS_LOG="$sshpass_log" \
PATH="$bin_directory:$PATH" \
  "$repository_root/canary/host/spike-tart.sh" \
    fake-image "$output_file" >/dev/null

jq -e '.outcome == "passed" and .guest == "Linux 7.0.0-28-generic aarch64"' \
  "$output_file" >/dev/null
[ "$(wc -l <"$sshpass_log" | tr -d ' ')" -eq 3 ]
[ "$(grep -c -- '-o IdentitiesOnly=yes' "$sshpass_log")" -eq 3 ]
[ "$(grep -c -- '-o PreferredAuthentications=password' "$sshpass_log")" -eq 3 ]
[ "$(grep -c -- '-o ConnectTimeout=5' "$sshpass_log")" -eq 1 ]
[ "$(grep -c -- '-o ConnectTimeout=10' "$sshpass_log")" -eq 2 ]
[ "$(grep -c -- '-o IdentitiesOnly=yes.*-o IdentitiesOnly=yes' "$sshpass_log")" -eq 0 ]
[ "$(grep -c -- '-o PreferredAuthentications=password.*-o PreferredAuthentications=password' "$sshpass_log")" -eq 0 ]

readiness_invocation="$(grep -F -- 'admin@192.168.64.2 true' "$sshpass_log")"
uname_invocation="$(grep -F -- 'admin@192.168.64.2 uname -srm' "$sshpass_log")"
memory_invocation="$(grep -F -- 'admin@192.168.64.2 grep -Eo' "$sshpass_log")"
[ "$(grep -F -c -- 'admin@192.168.64.2 true' "$sshpass_log")" -eq 1 ]
[ "$(grep -F -c -- 'admin@192.168.64.2 uname -srm' "$sshpass_log")" -eq 1 ]
[ "$(grep -F -c -- 'admin@192.168.64.2 grep -Eo' "$sshpass_log")" -eq 1 ]
grep -F -- '-o ConnectTimeout=5' <<<"$readiness_invocation" >/dev/null
! grep -F -- '-o ConnectTimeout=10' <<<"$readiness_invocation" >/dev/null
grep -F -- '-o ConnectTimeout=10' <<<"$uname_invocation" >/dev/null
grep -F -- '-o ConnectTimeout=10' <<<"$memory_invocation" >/dev/null

while IFS= read -r invocation; do
  grep -F -- '-o IdentitiesOnly=yes' <<<"$invocation" >/dev/null
  grep -F -- '-o PreferredAuthentications=password' <<<"$invocation" >/dev/null
done <"$sshpass_log"
