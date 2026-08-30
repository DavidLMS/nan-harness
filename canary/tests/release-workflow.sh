#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
workflow="$repository_root/.github/workflows/release.yml"
ci_workflow="$repository_root/.github/workflows/ci.yml"

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
grep -Fq 'actions: read' "$workflow"
grep -Fq 'require-green-ci.sh "$GITHUB_REPOSITORY" "$GITHUB_SHA"' "$workflow"
preflight_block="$(sed -n '/^  preflight:/,/^  pinned-conformance:/p' "$workflow")"
if grep -Eq 'cargo (fmt|clippy|test|doc|deny)' <<<"$preflight_block"; then
  printf 'release workflow must reuse exact-SHA CI instead of repeating Rust gates\n' >&2
  exit 1
fi
build_block="$(sed -n '/^  build:/,/^  publish:/p' "$workflow")"
grep -Fq -- '- preflight' <<<"$build_block"
if grep -Fq -- '- pinned-conformance' <<<"$build_block"; then
  printf 'release builds must run concurrently with pinned conformance\n' >&2
  exit 1
fi
publish_block="$(sed -n '/^  publish:/,$p' "$workflow")"
grep -Fq -- '- pinned-conformance' <<<"$publish_block"
grep -Fq 'branches:' "$ci_workflow"
grep -Fq 'cancel-in-progress: true' "$ci_workflow"
if grep -Fq 'tags:' "$ci_workflow"; then
  printf 'CI workflow must not run for release tags\n' >&2
  exit 1
fi
