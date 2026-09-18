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

# Hosted cells pass a frozen manifest version; exercise the real guest script
# with a synthetic package manager and retain the legacy one-argument path above.
cat >"$bin_directory/npm" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >"$NPM_TEST_ARGS"
EOF
chmod 755 "$bin_directory/npm"
NPM_TEST_ARGS="$temporary_directory/npm-args" \
NAN_CANARY_LEGACY_PATHS="$temporary_directory/empty-legacy" \
HOME="$temporary_directory/version-home" \
PATH="$bin_directory:/usr/bin:/bin" \
bash "$repository_root/canary/guest/install-harness.sh" codex 1.2.3
grep -Fx -- 'install --global @openai/codex@1.2.3' "$temporary_directory/npm-args" >/dev/null

# Hosted mode preserves setup-node's incoming priority over a legacy
# Homebrew/Tart-style Node path and checks the npm-launched runtime.
setup_node_directory="$temporary_directory/setup-node/bin"
legacy_node_directory="$temporary_directory/legacy-node/bin"
mkdir -p "$setup_node_directory" "$legacy_node_directory"
cat >"$setup_node_directory/node" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
test "${1:-}" = '-p' && test "${2:-}" = 'process.versions.node'
printf '24.20.0\n'
EOF
cat >"$legacy_node_directory/node" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
test "${1:-}" = '-p' && test "${2:-}" = 'process.versions.node'
printf '22.23.1\n'
EOF
cat >"$setup_node_directory/npm" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
test "$(node -p 'process.versions.node')" = '24.20.0'
printf '%s\n' "$*" >"$NPM_TEST_ARGS"
EOF
chmod 755 "$setup_node_directory/node" "$setup_node_directory/npm" "$legacy_node_directory/node"
NAN_CANARY_HOSTED=1 NAN_CANARY_EXPECTED_NODE_VERSION=24.20.0 \
NAN_CANARY_LEGACY_PATHS="$legacy_node_directory" \
NPM_TEST_ARGS="$temporary_directory/hosted-npm-args" \
HOME="$temporary_directory/hosted-home" \
PATH="$setup_node_directory:$legacy_node_directory:/usr/bin:/bin" \
bash "$repository_root/canary/guest/install-harness.sh" codex 1.2.3
grep -Fx -- 'install --global @openai/codex@1.2.3' "$temporary_directory/hosted-npm-args" >/dev/null

# Missing hosted runtime identity fails closed before invoking npm.
if NAN_CANARY_HOSTED=1 NAN_CANARY_EXPECTED_NODE_VERSION= \
  HOME="$temporary_directory/missing-runtime-home" PATH="$legacy_node_directory:/usr/bin:/bin" \
  bash "$repository_root/canary/guest/install-harness.sh" codex 1.2.3 >/dev/null 2>&1; then
  printf 'missing hosted Node identity unexpectedly passed\n' >&2
  exit 1
fi

# Hermes exact versions require the independently frozen source commit; do not
# let a malformed or incomplete identity silently fall back to latest.
if bash "$repository_root/canary/guest/install-harness.sh" hermes 1.2.3 >/dev/null 2>&1; then
  printf 'Hermes exact version without source ref unexpectedly passed\n' >&2
  exit 1
fi
if bash "$repository_root/canary/guest/install-harness.sh" hermes 1.2.3 invalid >/dev/null 2>&1; then
  printf 'Hermes malformed source ref unexpectedly passed\n' >&2
  exit 1
fi
