#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
grep -Fq '.harness == \$harness and .outcome == "passed"' \
  "$repository_root/canary/host/run-suite.sh"
grep -Fq "cat '{{output}}/conformance.json' >&2" \
  "$repository_root/canary/host/run-suite.sh"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
assets_directory="$temporary_directory/assets"
bin_directory="$temporary_directory/bin"
state_directory="$temporary_directory/state"
execution_marker="$temporary_directory/executed"
gh_log="$temporary_directory/gh.log"
mkdir -p "$assets_directory" "$bin_directory" "$state_directory"

assets=(
  nan-harness-aarch64-unknown-linux-musl
  nan-harness-canary-aarch64-unknown-linux-musl
  nan-harness-aarch64-apple-darwin
  nan-harness-canary-aarch64-apple-darwin
)
for asset in "${assets[@]}"; do
  printf '#!/usr/bin/env bash\nprintf executed >%q\n' "$execution_marker" >"$assets_directory/$asset"
  chmod 755 "$assets_directory/$asset"
done

cat >"$bin_directory/shlock" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat >"$bin_directory/tart" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
  list) exit 0 ;;
  stop|delete) exit 0 ;;
  *) exit 1 ;;
esac
EOF
cat >"$bin_directory/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$GH_LOG"
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
  [ "$pattern" = SHA256SUMS ] || exit 1
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
if [ "${1:-}" = attestation ] && [ "${2:-}" = verify ]; then
  [ "${RELEASE_ASSET_ATTESTATION_FAILURE:-}" != 1 ]
  exit $?
fi
exit 1
EOF
chmod 755 "$bin_directory/shlock" "$bin_directory/tart" "$bin_directory/gh"

run_suite() {
  local output_directory="$1"
  shift
  GH_LOG="$gh_log" \
    NAN_CANARY_STATE_DIR="$state_directory" \
    PATH="$bin_directory:$PATH" \
    "$repository_root/canary/host/run-suite.sh" \
    --trigger manual \
    --nan-harness-version 0.0.6 \
    --release-tag v0.0.6 \
    --linux-binary "$assets_directory/nan-harness-aarch64-unknown-linux-musl" \
    --linux-canary-binary "$assets_directory/nan-harness-canary-aarch64-unknown-linux-musl" \
    --macos-binary "$assets_directory/nan-harness-aarch64-apple-darwin" \
    --macos-canary-binary "$assets_directory/nan-harness-canary-aarch64-apple-darwin" \
    --output-dir "$output_directory" \
    --harness codex \
    --guest linux \
    --publish-feed \
    "$@"
}

run_rejected_case() {
  local name="$1"
  shift
  local output_directory="$temporary_directory/output-$name"
  mkdir -p "$output_directory"
  set +e
  run_suite "$output_directory" "$@"
  local status=$?
  set -e
  [ "$status" -ne 0 ]
  [ ! -f "$execution_marker" ]
  if [ -f "$gh_log" ] && grep -Eq 'release (create|upload|delete-asset|edit)' "$gh_log"; then
    printf 'rejected %s case reached remote publication\n' "$name" >&2
    exit 1
  fi
}

run_rejected_case missing --macos-binary "$assets_directory/missing"

renamed_asset="$temporary_directory/renamed-canary"
cp "$assets_directory/nan-harness-canary-aarch64-apple-darwin" "$renamed_asset"
run_rejected_case renamed --macos-canary-binary "$renamed_asset"

RELEASE_ASSET_CHECKSUM_MISMATCH=1 run_rejected_case checksum-mismatch
RELEASE_ASSET_ATTESTATION_FAILURE=1 run_rejected_case attestation-rejected
