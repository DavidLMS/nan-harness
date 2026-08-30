#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
bin_directory="$temporary_directory/bin"
home_directory="$temporary_directory/home"
operation_log="$temporary_directory/operations.log"
mkdir -p "$bin_directory" "$home_directory"
cp "$repository_root/canary/guest/bootstrap.sh" "$temporary_directory/bootstrap.sh"

cat >"$bin_directory/uname" <<'EOF'
#!/usr/bin/env bash
printf 'Linux\n'
EOF
cat >"$bin_directory/sudo" <<'EOF'
#!/usr/bin/env bash
printf 'sudo %s\n' "$*" >>"$BOOTSTRAP_OPERATION_LOG"
exit 0
EOF
cat >"$bin_directory/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
destination=''
while [ "$#" -gt 0 ]; do
  if [ "$1" = --output ]; then destination="$2"; shift 2; else shift; fi
done
printf '#!/usr/bin/env bash\nexit 0\n' >"$destination"
EOF
cat >"$bin_directory/node" <<'EOF'
#!/usr/bin/env bash
if [ "${1:-}" = -p ]; then printf '24\n'; else printf 'v24.0.0\n'; fi
EOF
for command in npm python3 jq; do
  cat >"$bin_directory/$command" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
done
chmod 755 "$bin_directory"/*

run_bootstrap() {
  HOME="$home_directory" \
  BOOTSTRAP_OPERATION_LOG="$operation_log" \
  PATH="$bin_directory:/usr/bin:/bin" \
    bash "$temporary_directory/bootstrap.sh"
}

run_bootstrap
first_operations="$(wc -l <"$operation_log" | tr -d ' ')"
[ "$first_operations" -gt 0 ]
run_bootstrap
[ "$(wc -l <"$operation_log" | tr -d ' ')" = "$first_operations" ]

printf '\n# invalidate bootstrap cache\n' >>"$temporary_directory/bootstrap.sh"
run_bootstrap
[ "$(wc -l <"$operation_log" | tr -d ' ')" -gt "$first_operations" ]
