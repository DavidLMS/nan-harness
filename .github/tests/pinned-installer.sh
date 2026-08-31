#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT

bin_directory="$temporary_directory/bin"
attempt_file="$temporary_directory/attempts"
mkdir -p "$bin_directory"

cat >"$bin_directory/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
destination=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output)
      destination="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done
test -n "$destination"
cat >"$destination" <<'INSTALLER'
#!/usr/bin/env bash
set -euo pipefail
attempt=0
if [ -f "$KIMI_TEST_ATTEMPT_FILE" ]; then
  attempt="$(cat "$KIMI_TEST_ATTEMPT_FILE")"
fi
attempt=$((attempt + 1))
printf '%s\n' "$attempt" >"$KIMI_TEST_ATTEMPT_FILE"
printf '%s\n' "${KIMI_VERSION:-latest}" >"$KIMI_TEST_VERSION_FILE"
if [ "$attempt" -lt 2 ]; then
  exit 28
fi
INSTALLER
chmod 755 "$destination"
EOF
chmod 755 "$bin_directory/curl"

HOME="$temporary_directory/home" \
KIMI_TEST_ATTEMPT_FILE="$attempt_file" \
KIMI_TEST_VERSION_FILE="$temporary_directory/version" \
NAN_PINNED_INSTALL_RETRY_DELAY_SECONDS=0 \
PATH="$bin_directory:$PATH" \
bash "$repository_root/.github/scripts/install-pinned-harness.sh" kimi-code

test "$(cat "$attempt_file")" = '2'
test "$(cat "$temporary_directory/version")" = '0.38.0'

cat >"$bin_directory/npm" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = 'root' ] && [ "${2:-}" = '--global' ]; then
  printf '%s\n' "$NPM_TEST_ROOT"
  exit 0
fi
printf '%s\n' "$@" >"$NPM_TEST_ARGUMENTS_FILE"
EOF
chmod 755 "$bin_directory/npm"

cat >"$bin_directory/node" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
postinstall="$1"
printf '%s\n' "$postinstall" >"$NODE_TEST_ARGUMENTS_FILE"
cache_directory="$(dirname "$postinstall")/bin"
mkdir -p "$cache_directory"
printf '#!/usr/bin/env bash\n' >"$cache_directory/.cline"
chmod 755 "$cache_directory/.cline"
EOF
chmod 755 "$bin_directory/node"

mkdir -p "$temporary_directory/npm-root/cline"
printf '// fixture\n' >"$temporary_directory/npm-root/cline/postinstall.mjs"

NPM_TEST_ARGUMENTS_FILE="$temporary_directory/npm-arguments" \
NPM_TEST_ROOT="$temporary_directory/npm-root" \
NODE_TEST_ARGUMENTS_FILE="$temporary_directory/node-arguments" \
PATH="$bin_directory:$PATH" \
bash "$repository_root/.github/scripts/install-pinned-harness.sh" cline

cat >"$temporary_directory/expected-npm-arguments" <<'EOF'
install
--global
--allow-scripts=cline,protobufjs
cline@3.0.55
EOF
cmp "$temporary_directory/expected-npm-arguments" "$temporary_directory/npm-arguments"
test "$(cat "$temporary_directory/node-arguments")" = \
  "$temporary_directory/npm-root/cline/postinstall.mjs"

cat >"$bin_directory/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
destination=''
url=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output)
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
  -m) printf 'x86_64\n' ;;
  *) exit 2 ;;
esac
EOF
chmod 755 "$bin_directory/uname"

OMP_TEST_URL_FILE="$temporary_directory/omp-url" \
HOME="$temporary_directory/omp-home" \
PATH="$bin_directory:$PATH" \
bash "$repository_root/.github/scripts/install-pinned-harness.sh" omp

test "$(cat "$temporary_directory/omp-url")" = \
  'https://github.com/can1357/oh-my-pi/releases/download/v18.0.11/omp-linux-x64'
test "$("$temporary_directory/omp-home/.local/bin/omp" --version)" = 'omp/18.0.11'
