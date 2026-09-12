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
test "${1:-latest}" = "${PRIME_TEST_VERSION:-latest}"
test "$PRIME_AGENT_INSTALLER_NONINTERACTIVE" = 1
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

HOME="$temporary_directory/home" \
PRIME_TEST_VERSION=1.2.3 \
PRIME_TEST_CURL_LOG="$curl_log" \
PATH="$bin_directory:/usr/bin:/bin" \
bash "$repository_root/canary/guest/install-harness.sh" prime-agent 1.2.3

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

# Hermes exact versions install the frozen release commit with the installer
# from that same commit; the forced pin survives a fresh main clone.
cat >"$bin_directory/curl" <<'EOF_CURL'
#!/usr/bin/env bash
set -euo pipefail
destination=''
url=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output|-o) destination="$2"; shift 2 ;;
    https://*) url="$1"; shift ;;
    *) shift ;;
  esac
done
printf '%s\n' "$url" >"$HERMES_TEST_URL_FILE"
cat >"$destination" <<'INSTALLER'
printf '%s\n' "$@" >"$HERMES_TEST_ARGUMENTS_FILE"
INSTALLER
EOF_CURL
chmod 755 "$bin_directory/curl"

hermes_commit='939e45c91d751fadd94dcd1b873ac3cb44846213'
hermes_env=(HOME="$temporary_directory/hermes-home" PATH="$bin_directory:/usr/bin:/bin"
  HERMES_TEST_URL_FILE="$temporary_directory/hermes-url"
  HERMES_TEST_ARGUMENTS_FILE="$temporary_directory/hermes-arguments")
env "${hermes_env[@]}" bash "$repository_root/canary/guest/install-harness.sh" hermes 0.21.2 "$hermes_commit"
test "$(cat "$temporary_directory/hermes-url")" = \
  "https://raw.githubusercontent.com/NousResearch/hermes-agent/$hermes_commit/scripts/install.sh"
test "$(tr '\n' ' ' <"$temporary_directory/hermes-arguments")" = \
  "--skip-setup --skip-browser --non-interactive --commit $hermes_commit --force-commit "

for rejected in "hermes 0.21.2" "hermes 0.21.2 v2026.9.11" "hermes 0.21.2 ${hermes_commit:0:12}" \
  "codex 1.2.3 $hermes_commit"; do
  rm -f "$temporary_directory/hermes-url"
  # shellcheck disable=SC2086 # the case deliberately splits into installer arguments
  if env "${hermes_env[@]}" bash "$repository_root/canary/guest/install-harness.sh" $rejected \
      >/dev/null 2>&1; then
    printf 'installer accepted an unfrozen Hermes source: %s\n' "$rejected" >&2
    exit 1
  fi
  test ! -e "$temporary_directory/hermes-url"
done
