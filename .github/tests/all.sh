#!/usr/bin/env bash
set -euo pipefail

tests_directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
for test_script in \
  pinned-installer.sh \
  pinned-conformance-shards.sh \
  source-main-detector.sh \
  release-ci-gate.sh; do
  printf '==> .github/tests/%s\n' "$test_script"
  bash "$tests_directory/$test_script"
done
