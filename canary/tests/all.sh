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
  publication.sh \
  release-assets.sh \
  release-gate.sh \
  release-workflow.sh \
  probe-harness.sh \
  conformance-policy.sh \
  run-suite.sh; do
  printf '==> canary/tests/%s\n' "$test_script"
  bash "$tests_directory/$test_script"
done
