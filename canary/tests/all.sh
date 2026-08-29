#!/usr/bin/env bash
set -euo pipefail

tests_directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
for test_script in \
  guest-installer.sh \
  operations.sh \
  tart-spike.sh \
  publication.sh \
  release-assets.sh \
  release-gate.sh \
  release-workflow.sh \
  probe-harness.sh \
  run-suite.sh; do
  printf '==> canary/tests/%s\n' "$test_script"
  bash "$tests_directory/$test_script"
done
