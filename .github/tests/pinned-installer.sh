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
