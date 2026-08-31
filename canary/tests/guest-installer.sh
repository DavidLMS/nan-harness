#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT

bin_directory="$temporary_directory/bin"
curl_log="$temporary_directory/curl.log"
mkdir -p "$bin_directory" "$temporary_directory/home"

cat >"$bin_directory/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

printf '%s\n' "$*" >>"$PRIME_TEST_CURL_LOG"
destination=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output|-o)
      destination="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done

if [ -n "$destination" ]; then
  cat >"$destination" <<'INSTALLER'
curl -fsSL 'https://downloads.example.invalid/prime-agent' -o "$HOME/prime-agent"
INSTALLER
  chmod 755 "$destination"
fi
EOF
chmod 755 "$bin_directory/curl"

HOME="$temporary_directory/home" \
PRIME_TEST_CURL_LOG="$curl_log" \
PATH="$bin_directory:/usr/bin:/bin" \
bash "$repository_root/canary/guest/install-harness.sh" prime-agent

tail -n 1 "$curl_log" | grep -F -- '--connect-timeout 15' >/dev/null
tail -n 1 "$curl_log" | grep -F -- '--max-time 120' >/dev/null
tail -n 1 "$curl_log" | grep -F -- '--retry 4' >/dev/null
tail -n 1 "$curl_log" | grep -F -- '--retry-all-errors' >/dev/null
tail -n 1 "$curl_log" | grep -F -- '--retry-max-time 180' >/dev/null

cat >"$bin_directory/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
destination=''
url=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output|-o)
      destination="$2"
      shift 2
      ;;
    https://*)
      url="$1"
      shift
      ;;
    *)
      shift
      ;;
  esac
done
test -n "$destination"
test -n "$url"
printf '%s\n' "$url" >"$OMP_TEST_URL_FILE"
cat >"$destination" <<'BINARY'
#!/usr/bin/env bash
set -euo pipefail
test "${1:-}" = '--version'
printf 'omp/18.0.11\n'
BINARY
chmod 755 "$destination"
EOF
chmod 755 "$bin_directory/curl"

cat >"$bin_directory/uname" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
  -s) printf 'Linux\n' ;;
  -m) printf 'aarch64\n' ;;
  *) exit 2 ;;
esac
EOF
chmod 755 "$bin_directory/uname"

OMP_TEST_URL_FILE="$temporary_directory/omp-url" \
HOME="$temporary_directory/omp-home" \
PATH="$bin_directory:/usr/bin:/bin" \
bash "$repository_root/canary/guest/install-harness.sh" omp

test "$(cat "$temporary_directory/omp-url")" = \
  'https://github.com/can1357/oh-my-pi/releases/latest/download/omp-linux-arm64'
test "$("$temporary_directory/omp-home/.local/bin/omp" --version)" = 'omp/18.0.11'
