#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
assets_directory="$temporary_directory/assets"
bin_directory="$temporary_directory/bin"
mkdir -p "$assets_directory" "$bin_directory"
assets=(
  nan-harness-aarch64-unknown-linux-musl
  nan-harness-canary-aarch64-unknown-linux-musl
  nan-harness-aarch64-apple-darwin
  nan-harness-canary-aarch64-apple-darwin
)
for asset in "${assets[@]}"; do
  printf '%s\n' "$asset fixture" >"$assets_directory/$asset"
done

cat >"$bin_directory/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = release ] && [ "${2:-}" = download ]; then
  output_directory=''
  while [ "$#" -gt 0 ]; do
    if [ "$1" = --dir ]; then
      output_directory="$2"
      shift 2
    else
      shift
    fi
  done
  {
    for asset in \
      nan-harness-aarch64-unknown-linux-musl \
      nan-harness-canary-aarch64-unknown-linux-musl \
      nan-harness-aarch64-apple-darwin \
      nan-harness-canary-aarch64-apple-darwin; do
      digest="$(sha256sum "$ASSETS_DIRECTORY/$asset" | awk '{print $1}')"
      if [ "${RELEASE_ASSET_CHECKSUM_MISMATCH:-}" = 1 ] && [ "$asset" = nan-harness-canary-aarch64-apple-darwin ]; then
        digest="0${digest:1}"
      fi
      printf '%s  %s\n' "$digest" "$asset"
    done
  } >"$output_directory/SHA256SUMS"
  exit 0
fi
if [ "${1:-}" = attestation ] && [ "${2:-}" = verify ]; then
  expected=(
    attestation verify "$ASSETS_DIRECTORY/SHA256SUMS"
    --repo DavidLMS/nan-harness
    --signer-workflow DavidLMS/nan-harness/.github/workflows/release.yml
    --source-ref refs/tags/v0.0.6
    --deny-self-hosted-runners
  )
  actual=("$@")
  [ "$#" -eq "${#expected[@]}" ] || exit 1
  for index in "${!expected[@]}"; do
    [ "${actual[$index]}" = "${expected[$index]}" ] || exit 1
  done
  printf '%s\n' "$*" >>"$GH_LOG"
  [ "${RELEASE_ASSET_ATTESTATION_FAILURE:-}" != 1 ]
  exit $?
fi
exit 1
EOF
chmod 755 "$bin_directory/gh"

GH_LOG="$temporary_directory/gh.log" ASSETS_DIRECTORY="$assets_directory" \
  PATH="$bin_directory:$PATH" \
  "$repository_root/canary/host/verify-release-assets.sh" \
  --release-tag v0.0.6 --assets-dir "$assets_directory"
grep -F -- 'attestation verify '"$assets_directory"'/SHA256SUMS --repo DavidLMS/nan-harness --signer-workflow DavidLMS/nan-harness/.github/workflows/release.yml --source-ref refs/tags/v0.0.6 --deny-self-hosted-runners' "$temporary_directory/gh.log" >/dev/null

set +e
RELEASE_ASSET_CHECKSUM_MISMATCH=1 GH_LOG="$temporary_directory/gh.log" ASSETS_DIRECTORY="$assets_directory" \
  PATH="$bin_directory:$PATH" \
  "$repository_root/canary/host/verify-release-assets.sh" \
  --release-tag v0.0.6 --assets-dir "$assets_directory"
checksum_status=$?
RELEASE_ASSET_ATTESTATION_FAILURE=1 GH_LOG="$temporary_directory/gh.log" ASSETS_DIRECTORY="$assets_directory" \
  PATH="$bin_directory:$PATH" \
  "$repository_root/canary/host/verify-release-assets.sh" \
  --release-tag v0.0.6 --assets-dir "$assets_directory"
attestation_status=$?
set -e
[ "$checksum_status" -ne 0 ]
[ "$attestation_status" -ne 0 ]
