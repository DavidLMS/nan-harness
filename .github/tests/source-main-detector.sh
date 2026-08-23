#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
cleanup_test() {
  if [ "${SOURCE_MAIN_TEST_KEEP_TEMP:-}" = 1 ]; then
    printf 'source-main fixture retained at %s\n' "$temporary_directory" >&2
  else
    rm -rf "$temporary_directory"
  fi
}
trap cleanup_test EXIT
bin_directory="$temporary_directory/bin"
reports_directory="$temporary_directory/reports"
mkdir -p "$bin_directory" "$reports_directory"

cat >"$bin_directory/nan-harness" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = --version ]; then
  printf '%s\n' 'nan-harness 0.0.6'
  exit 0
fi
if [ "${1:-}" = doctor ]; then
  if [ "${SOURCE_MAIN_DOCTOR_FAILURE:-}" = "${2:-}" ]; then
    exit 1
  fi
  printf '%s\n' '{"version":"1.2.3"}'
  exit 0
fi
exit 1
EOF
chmod 755 "$bin_directory/nan-harness"

cat >"$bin_directory/install-pinned-harness.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [ "${SOURCE_MAIN_INSTALL_FAILURE:-}" = "${1:-}" ]; then
  exit 1
fi
EOF
chmod 755 "$bin_directory/install-pinned-harness.sh"

cat >"$bin_directory/nan-harness-canary" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
command="${1:-}"
shift
if [ "$command" = conformance ]; then
  harness=''
  while [ "$#" -gt 0 ]; do
    if [ "$1" = --harness ]; then
      harness="$2"
      shift 2
    else
      shift
    fi
  done
  if [ "${SOURCE_MAIN_CONFORMANCE_FAILURE:-}" = "$harness" ]; then
    exit 1
  fi
  printf '{"harness":"%s","outcome":"passed"}\n' "$harness"
  exit 0
fi
if [ "$command" = record ]; then
  record_arguments="$*"
  output=''
  while [ "$#" -gt 0 ]; do
    if [ "$1" = --output ]; then
      output="$2"
      shift 2
    else
      shift
    fi
  done
  printf '%s\n' "$record_arguments" >>"$SOURCE_MAIN_RECORD_LOG"
  : >"$output"
  if [ "${SOURCE_MAIN_RECORD_FAILURE:-}" = 1 ]; then
    exit 1
  fi
  exit 0
fi
exit 1
EOF
chmod 755 "$bin_directory/nan-harness-canary"

run_case() {
  local name="$1"
  local expected_phase="$2"
  : >"$temporary_directory/record.log"
  rm -f "$reports_directory"/*
  set +e
  SOURCE_MAIN_HARNESSES="$name" \
    SOURCE_MAIN_REPORTS_DIR="$reports_directory" \
    SOURCE_MAIN_NAN_HARNESS_BINARY="$bin_directory/nan-harness" \
    SOURCE_MAIN_CANARY_BINARY="$bin_directory/nan-harness-canary" \
    SOURCE_MAIN_INSTALL_SCRIPT="$bin_directory/install-pinned-harness.sh" \
    SOURCE_MAIN_INSTALL_FAILURE="${SOURCE_MAIN_TEST_INSTALL_FAILURE:-}" \
    SOURCE_MAIN_DOCTOR_FAILURE="${SOURCE_MAIN_TEST_DOCTOR_FAILURE:-}" \
    SOURCE_MAIN_CONFORMANCE_FAILURE="${SOURCE_MAIN_TEST_CONFORMANCE_FAILURE:-}" \
    SOURCE_MAIN_RECORD_FAILURE="${SOURCE_MAIN_TEST_RECORD_FAILURE:-}" \
    SOURCE_MAIN_RECORD_LOG="$temporary_directory/record.log" \
    GITHUB_RUN_ID=fixture GITHUB_RUN_ATTEMPT=1 GITHUB_SHA=fixture \
    bash "$repository_root/.github/scripts/run-source-main-detector.sh" \
    >"$temporary_directory/$name.stdout" 2>"$temporary_directory/$name.stderr"
  local status=$?
  set -e
  [ "$status" -ne 0 ]
  [ "$(wc -l <"$temporary_directory/record.log" | tr -d ' ')" -eq 1 ]
  if [ -n "$expected_phase" ]; then
    grep -F -- "--failure-phase $expected_phase" "$temporary_directory/record.log" >/dev/null
  fi
}

SOURCE_MAIN_TEST_INSTALL_FAILURE=install-fixture run_case install-fixture install
SOURCE_MAIN_TEST_DOCTOR_FAILURE=doctor-fixture run_case doctor-fixture doctor
SOURCE_MAIN_TEST_CONFORMANCE_FAILURE=conformance-fixture run_case conformance-fixture conformance
SOURCE_MAIN_TEST_RECORD_FAILURE=1 run_case record-fixture ''
