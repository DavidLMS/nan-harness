#!/usr/bin/env bash
set -uo pipefail

reports_directory="${SOURCE_MAIN_REPORTS_DIR:-reports}"
nan_harness_binary="${SOURCE_MAIN_NAN_HARNESS_BINARY:-target/debug/nan-harness}"
canary_binary="${SOURCE_MAIN_CANARY_BINARY:-target/debug/nan-harness-canary}"
install_script="${SOURCE_MAIN_INSTALL_SCRIPT:-.github/scripts/install-pinned-harness.sh}"
mkdir -p "$reports_directory"
export PATH="$HOME/.kimi-code/bin:$HOME/.hermes/bin:$HOME/.local/bin:$PATH"

nan_harness_version="$($nan_harness_binary --version 2>/dev/null | awk '{print $2}')"
nan_harness_sha256="$(sha256sum "$nan_harness_binary" | awk '{print $1}')"
harnesses=(
  claude-code codex opencode hermes pi omp prime-agent deepseek-harness
  openclaw cline qwen-code kimi-code aider goose fx
)
if [ -n "${SOURCE_MAIN_HARNESSES:-}" ]; then
  read -r -a harnesses <<<"$SOURCE_MAIN_HARNESSES"
fi
failures=0

for harness in "${harnesses[@]}"; do
  install_outcome=failed
  doctor_outcome=skipped
  conformance_outcome=skipped
  harness_version=unknown
  detector_failed=0
  if bash "$install_script" "$harness" --latest; then
    install_outcome=success
    doctor_stderr="$(mktemp)"
    if doctor_report="$($nan_harness_binary doctor "$harness" --allow-unsupported --allow-untested --json 2>"$doctor_stderr")"; then
      doctor_outcome=success
      harness_version="$(jq -r '.version // "unknown"' <<<"$doctor_report")"
      if "$canary_binary" conformance \
        --nan-harness "$nan_harness_binary" \
        --harness "$harness" --json >"$reports_directory/conformance-$harness.json"; then
        conformance_outcome=success
      else
        conformance_outcome=failed
      fi
    else
      doctor_outcome=failed
      printf 'doctor failed for %s:\n' "$harness" >&2
      sed -n '1,40p' "$doctor_stderr" >&2
    fi
    rm -f "$doctor_stderr"
  fi
  if [ "$install_outcome" != success ] || [ "$doctor_outcome" != success ] || [ "$conformance_outcome" != success ]; then
    detector_failed=1
  fi
  spec_sha256="$(printf '%s' "$harness|deterministic|source-main" | sha256sum | awk '{print $1}')"
  arguments=(
    --output "$reports_directory/$harness.json"
    --run-id "${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-1}-$harness"
    --cell-id "github-$harness-deterministic"
    --spec-sha256 "$spec_sha256"
    --trigger daily
    --tier deterministic
    --scenario source-main-deterministic
    --nan-harness-version "$nan_harness_version"
    --nan-harness-source "commit:${GITHUB_SHA:-local}"
    --nan-harness-sha256 "$nan_harness_sha256"
    --operating-system linux
    --architecture "${RUNNER_ARCH:-x86_64}"
    --image ubuntu-24.04
    --profile source-main
    --harness "$harness"
    --harness-version "$harness_version"
    --check deterministic-conformance
  )
  if [ "$install_outcome" != success ]; then
    arguments+=(--failure-class installation --failure-phase install --failure-summary "latest harness installation failed")
  elif [ "$doctor_outcome" != success ]; then
    arguments+=(--failure-class harness --failure-phase doctor --failure-summary "harness diagnostics failed")
  elif [ "$conformance_outcome" != success ]; then
    arguments+=(--failure-class harness --failure-phase conformance --failure-summary "deterministic conformance failed")
  else
    arguments+=(--passed)
  fi
  if [ "$detector_failed" -ne 0 ]; then
    failures=$((failures + 1))
  fi
  if ! "$canary_binary" record "${arguments[@]}"; then
    failures=$((failures + 1))
  fi
done

exit "$failures"
