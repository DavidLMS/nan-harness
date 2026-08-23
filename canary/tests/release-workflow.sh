#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
workflow="$repository_root/.github/workflows/release.yml"

if grep -Fq 'Publish initial compatibility feed' "$workflow"; then
  printf 'hosted release workflow must not publish the initial compatibility feed\n' >&2
  exit 1
fi
if grep -Eq 'gh release (create|upload|delete-asset|edit)[[:space:]].*compatibility' "$workflow"; then
  printf 'hosted release workflow must not mutate the compatibility release\n' >&2
  exit 1
fi
if grep -Fq -- '--publish-feed' "$workflow"; then
  printf 'hosted release workflow must not request compatibility feed publication\n' >&2
  exit 1
fi
grep -F 'gh release create "$GITHUB_REF_NAME" dist/*' "$workflow" >/dev/null
