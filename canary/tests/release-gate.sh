#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
bin_directory="$temporary_directory/bin"
asset_source_directory="$temporary_directory/source-assets"
mkdir -p "$bin_directory" "$asset_source_directory"

assets=(
  nan-harness-aarch64-unknown-linux-musl
  nan-harness-canary-aarch64-unknown-linux-musl
  nan-harness-aarch64-apple-darwin
  nan-harness-canary-aarch64-apple-darwin
)
for asset in "${assets[@]}"; do
  printf '%s fixture\n' "$asset" >"$asset_source_directory/$asset"
done

cat >"$bin_directory/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = release ] && [ "${2:-}" = list ]; then
  printf '%s\n' 'v0.0.6'
  exit 0
fi
if [ "${1:-}" = release ] && [ "${2:-}" = download ]; then
  directory='.'
  pattern=''
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --dir) directory="$2"; shift 2 ;;
      --pattern) pattern="$2"; shift 2 ;;
      *) shift ;;
    esac
  done
  if [ "$pattern" = SHA256SUMS ]; then
    {
      for asset in \
        nan-harness-aarch64-unknown-linux-musl \
        nan-harness-canary-aarch64-unknown-linux-musl \
        nan-harness-aarch64-apple-darwin \
        nan-harness-canary-aarch64-apple-darwin; do
        digest="$(sha256sum "$directory/$asset" | awk '{print $1}')"
        if [ "${RELEASE_ASSET_CHECKSUM_MISMATCH:-}" = 1 ] \
          && [ "$asset" = nan-harness-canary-aarch64-apple-darwin ]; then
          digest="0${digest:1}"
        fi
        printf '%s  %s\n' "$digest" "$asset"
      done
    } >"$directory/SHA256SUMS"
    exit 0
  fi
  for asset in \
    nan-harness-aarch64-unknown-linux-musl \
    nan-harness-canary-aarch64-unknown-linux-musl \
    nan-harness-aarch64-apple-darwin \
    nan-harness-canary-aarch64-apple-darwin; do
    if [ "${RELEASE_ASSET_MISSING:-}" = 1 ] \
      && [ "$asset" = nan-harness-canary-aarch64-apple-darwin ]; then
      continue
    fi
    cp "$ASSET_SOURCE_DIRECTORY/$asset" "$directory/$asset"
  done
  exit 0
fi
if [ "${1:-}" = attestation ] && [ "${2:-}" = verify ]; then
  [ "${RELEASE_ASSET_ATTESTATION_FAILURE:-}" != 1 ]
  exit $?
fi
exit 1
EOF
cat >"$bin_directory/shlock" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat >"$bin_directory/tart" <<'EOF'
#!/usr/bin/env bash
case "${1:-}" in
  list|stop|delete) exit 0 ;;
  *) exit 1 ;;
esac
EOF
chmod 755 "$bin_directory/gh" "$bin_directory/shlock" "$bin_directory/tart"

run_failure_case() {
  local name="$1"
  shift
  local state_directory="$temporary_directory/state-$name"
  mkdir -p "$state_directory"
  set +e
  ASSET_SOURCE_DIRECTORY="$asset_source_directory" \
    NAN_CANARY_STATE_DIR="$state_directory" \
    NAN_CANARY_RETRY_DELAY_SECONDS=0 \
    PATH="$bin_directory:$PATH" \
    "$repository_root/canary/host/run-release-gate.sh" --force
  local result=$?
  set -e
  [ "$result" -ne 0 ]
  [ ! -f "$state_directory/release-gate-v0.0.6.attempted" ]
}

RELEASE_ASSET_CHECKSUM_MISMATCH=1 run_failure_case checksum
RELEASE_ASSET_ATTESTATION_FAILURE=1 run_failure_case attestation
RELEASE_ASSET_MISSING=1 run_failure_case verifier
