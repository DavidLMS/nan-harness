#!/usr/bin/env bash
set -euo pipefail

tests_directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
for test_script in \
  guest-installer.sh \
  bootstrap-cache.sh \
  prepare-suite-image.sh \
  operations.sh \
  alerts.sh \
  tart-spike.sh \
  parallel-tart-spike.sh \
  host-lock.sh \
  publication.sh \
  release-assets.sh \
  release-gate.sh \
  release-channels.sh \
  release-recommendation.sh \
  release-workflow.sh \
  probe-harness.sh \
  conformance-policy.sh \
  run-suite.sh; do
  printf '==> canary/tests/%s\n' "$test_script"
  bash "$tests_directory/$test_script"
done

# Offline Python contracts. They cover the hosted platform table, the resolver, the
# cell driver, the release gate and the publisher; the Windows suites skip their
# live PowerShell fixtures where pwsh is unavailable.
for test_script in \
  hosted-cli-selection.py \
  hosted-cli-workflow.py \
  cli-resolution.py \
  cli-execution.py \
  release-gate.py \
  release-publish.py \
  release-publish-integration.py \
  windows-installer.py \
  windows-diagnostic.py \
  windows-summary.py \
  probe-harness.py \
  probe-harness-windows.py \
  codex-diagnostic.py \
  windows-diagnostic-workflow.py; do
  printf '==> canary/tests/%s\n' "$test_script"
  python3 "$tests_directory/$test_script"
done
