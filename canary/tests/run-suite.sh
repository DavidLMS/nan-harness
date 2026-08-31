#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
grep -Fq '.harness == \$harness and .outcome == "passed"' \
  "$repository_root/canary/host/run-suite.sh"
grep -Fq "cat '{{output}}/conformance.json' >&2" \
  "$repository_root/canary/host/run-suite.sh"
grep -Fq 'NAN_HARNESS_CONFORMANCE_DIAGNOSTICS=1' \
  "$repository_root/canary/host/run-suite.sh"
grep -Fq 'NAN_CANARY_MAX_PARALLEL_CELLS:-1' \
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
  case "$asset" in
    nan-harness-canary-*)
      cat >"$assets_directory/$asset" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
  capabilities)
    printf '%s\n' '{"schemaVersion":1,"preparedImageOverride":false}'
    ;;
  cell)
    shift
    spec=''
    output=''
    while [ "$#" -gt 0 ]; do
      case "$1" in
        --spec) spec="$2"; shift 2 ;;
        --output) output="$2"; shift 2 ;;
        --private-log-dir) shift 2 ;;
        *) shift ;;
      esac
    done
    printf executed >"$CANARY_EXECUTION_MARKER"
    if [ -n "${CANARY_TEST_STATE:-}" ]; then
      guest="$(basename "$spec")"
      guest="${guest%%-*}"
      while ! mkdir "$CANARY_TEST_STATE/active-lock" 2>/dev/null; do sleep 0.01; done
      active=0
      [ ! -f "$CANARY_TEST_STATE/active" ] || active="$(cat "$CANARY_TEST_STATE/active")"
      active=$((active + 1))
      printf '%s\n' "$active" >"$CANARY_TEST_STATE/active"
      maximum=0
      [ ! -f "$CANARY_TEST_STATE/maximum" ] || maximum="$(cat "$CANARY_TEST_STATE/maximum")"
      if [ "$active" -gt "$maximum" ]; then printf '%s\n' "$active" >"$CANARY_TEST_STATE/maximum"; fi
      rmdir "$CANARY_TEST_STATE/active-lock"
      if ! mkdir "$CANARY_TEST_STATE/guest-$guest" 2>/dev/null; then
        printf '%s\n' "$guest" >>"$CANARY_TEST_STATE/guest-overlap"
      fi
      sleep 0.05
      rmdir "$CANARY_TEST_STATE/guest-$guest" 2>/dev/null || true
      while ! mkdir "$CANARY_TEST_STATE/active-lock" 2>/dev/null; do sleep 0.01; done
      active="$(cat "$CANARY_TEST_STATE/active")"
      printf '%s\n' "$((active - 1))" >"$CANARY_TEST_STATE/active"
      rmdir "$CANARY_TEST_STATE/active-lock"
    fi
    mkdir -p "$(dirname "$output")"
    printf '{}\n' >"$output"
    ;;
  aggregate)
    shift
    summary=''
    while [ "$#" -gt 0 ]; do
      if [ "$1" = --summary ]; then summary="$2"; shift 2; else shift; fi
    done
    printf '{"alerts":[]}\n' >"$summary"
    ;;
  validate-report) exit 0 ;;
  *) exit 0 ;;
esac
EOF
      ;;
    *)
      printf '#!/usr/bin/env bash\nprintf executed >%q\n' "$execution_marker" >"$assets_directory/$asset"
      ;;
  esac
  chmod 755 "$assets_directory/$asset"
done

cat >"$bin_directory/shlock" <<'EOF'
#!/usr/bin/env bash
exit "${SHLOCK_STATUS:-0}"
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
cat >"$bin_directory/publish-compatibility" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod 755 "$bin_directory/publish-compatibility"

run_suite() {
  local output_directory="$1"
  shift
    GH_LOG="$gh_log" \
    CANARY_EXECUTION_MARKER="$execution_marker" \
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

run_full_weekly_suite() {
  local output_directory="$1"
  local concurrency="$2"
  local concurrency_state="$3"
  mkdir -p "$output_directory" "$concurrency_state"
  GH_LOG="$gh_log" \
    CANARY_EXECUTION_MARKER="$execution_marker" \
    CANARY_TEST_STATE="$concurrency_state" \
    NAN_CANARY_STATE_DIR="$state_directory" \
    NAN_CANARY_MAX_PARALLEL_CELLS="$concurrency" \
    NAN_CANARY_PUBLISH_COMPATIBILITY_COMMAND="$bin_directory/publish-compatibility" \
    PATH="$bin_directory:$PATH" \
    "$repository_root/canary/host/run-suite.sh" \
    --trigger weekly \
    --nan-harness-version 0.0.6 \
    --release-tag v0.0.6 \
    --linux-binary "$assets_directory/nan-harness-aarch64-unknown-linux-musl" \
    --linux-canary-binary "$assets_directory/nan-harness-canary-aarch64-unknown-linux-musl" \
    --macos-binary "$assets_directory/nan-harness-aarch64-apple-darwin" \
    --macos-canary-binary "$assets_directory/nan-harness-canary-aarch64-apple-darwin" \
    --output-dir "$output_directory" >/dev/null
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

lock_output="$temporary_directory/output-lock"
mkdir -p "$lock_output"
set +e
SHLOCK_STATUS=1 NAN_CANARY_LOCK_WAIT_SECONDS=0 run_suite "$lock_output"
[ "$?" -eq 75 ]
set -e

budget_output="$temporary_directory/output-budget"
mkdir -p "$budget_output"
set +e
NAN_CANARY_SUITE_DEADLINE_EPOCH=1 run_suite "$budget_output"
[ "$?" -eq 1 ]
set -e
[ ! -f "$execution_marker" ]

parallel_output="$temporary_directory/output-parallel"
parallel_state="$temporary_directory/concurrency-parallel"
run_full_weekly_suite "$parallel_output" 2 "$parallel_state"
[ "$(find "$parallel_output/reports" -type f -name '*.json' | wc -l | tr -d ' ')" = 30 ]
[ "$(cat "$parallel_state/maximum")" = 2 ]
[ ! -f "$parallel_state/guest-overlap" ]

serial_output="$temporary_directory/output-serial"
serial_state="$temporary_directory/concurrency-serial"
run_full_weekly_suite "$serial_output" 1 "$serial_state"
[ "$(cat "$serial_state/maximum")" = 1 ]
