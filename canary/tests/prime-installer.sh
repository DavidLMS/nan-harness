#!/usr/bin/env bash
set -euo pipefail
repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
mkdir -p "$temporary_directory/bin" "$temporary_directory/home"
cat >"$temporary_directory/bin/curl" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output|-o) destination="$2"; shift 2 ;;
    https://*) printf '%s\n' "$1" >"$PRIME_TEST_URL"; shift ;;
    *) shift ;;
  esac
done
cat >"$destination" <<'INSTALLER'
set -eu
test "$#" = 0
printf '%s\n' "${PRIME_AGENT_VERSION:-latest}" >"$PRIME_TEST_VERSION"
test "$PRIME_AGENT_DOWNLOAD_BASE_URL" = 'https://pub-728493de92a943e2a9b2d17b4719f318.r2.dev'
test "$PRIME_AGENT_INSTALLER_NONINTERACTIVE" = 1
INSTALLER
MOCK
chmod +x "$temporary_directory/bin/curl"
export PATH="$temporary_directory/bin:$PATH"
export HOME="$temporary_directory/home"
export NAN_CANARY_LEGACY_PATHS="$temporary_directory/bin"
export PRIME_TEST_URL="$temporary_directory/url" PRIME_TEST_VERSION="$temporary_directory/version"
for version in 0.9.3 0.10.0 latest; do
  bash "$repository_root/canary/guest/install-harness.sh" prime-agent "$version"
  test "$(cat "$PRIME_TEST_VERSION")" = "$version"
  if [ "$version" = 0.9.3 ]; then
    grep -F 'cf07c5a3f5eca98e7744f2df83050044c920252a/install.sh' "$PRIME_TEST_URL" >/dev/null
  else
    grep -Fx 'https://app.primeintellect.ai/prime-agent/install.sh' "$PRIME_TEST_URL" >/dev/null
  fi
done
bash "$repository_root/.github/scripts/install-pinned-harness.sh" prime-agent
test "$(cat "$PRIME_TEST_VERSION")" = 0.9.3
grep -F 'cf07c5a3f5eca98e7744f2df83050044c920252a/install.sh' "$PRIME_TEST_URL" >/dev/null
bash "$repository_root/.github/scripts/install-pinned-harness.sh" prime-agent --latest
test "$(cat "$PRIME_TEST_VERSION")" = latest
grep -Fx 'https://app.primeintellect.ai/prime-agent/install.sh' "$PRIME_TEST_URL" >/dev/null
