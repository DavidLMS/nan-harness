#!/usr/bin/env bash
set -euo pipefail
repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
python3 "$repository_root/canary/tests/hosted-actions.py"
python3 "$repository_root/canary/tests/desktop-publication.py"
python3 "$repository_root/canary/tests/desktop-session.py"

gate="$repository_root/.github/workflows/cli-release-gate.yml"
writer="$repository_root/.github/workflows/compatibility-publisher.yml"
approval="$repository_root/.github/workflows/compatibility-approve.yml"
desktop="$repository_root/.github/workflows/desktop-check.yml"
for workflow in "$gate" "$writer" "$approval" "$desktop"; do
  events="$(sed -n '/^on:/,/^permissions:/p' "$workflow")"
  if grep -Eq '^  (schedule|issues|issue_comment|pull_request_target):' <<<"$events"; then
    printf 'publication workflows must not add schedules or issue-triggered execution\n' >&2
    exit 1
  fi
done
grep -Fq 'group: compatibility-publication-writer' "$writer"
grep -Fq 'cancel-in-progress: false' "$writer"
grep -Fq 'Persist successful gate before entering writer mutex' "$gate"
cells="$(sed -n '/^  cells:/,/^  enqueue:/p' "$gate")"
if grep -Fq 'contents: write' <<<"$cells"; then
  printf 'third-party harnesses must never receive release write permissions\n' >&2
  exit 1
fi
if [ "$(grep -c 'NAN_API_KEY:' <<<"$cells")" -ne 1 ]; then
  printf 'the provider key belongs only to the live cell step\n' >&2
  exit 1
fi
grep -Fq -- '--expected-commit "$RELEASE_COMMIT"' <<<"$cells"
# Coverage selection owns which commit supplies cell code; only release coverage
# may reach the durable request, the writer, or the resume path.
grep -Fq 'ref: ${{ needs.matrix.outputs.source }}' <<<"$cells"
grep -Fq 'from cell import select_coverage' "$gate"
grep -Fq "options: [smoke, daily, weekly, release]" "$gate"
grep -Fq -- "--pattern 'nan-harness-x86_64-unknown-linux-musl'" "$gate"
grep -Fq -- '--include-x86-linux' "$gate"
grep -Fq 'matrix.binary_asset' "$gate"
enqueue="$(sed -n '/^  enqueue:/,/^  publish:/p' "$gate")"
grep -Fq "needs.matrix.outputs.trigger == 'release'" <<<"$enqueue"
grep -Fq "if: steps.matrix.outputs.trigger == 'release'" "$gate"
sed -n '/^  publish:/,$p' "$gate" | grep -Fq 'needs: enqueue'
if sed -n '/^on:/,/^permissions:/p' "$gate" | grep -Eq '^    secrets:'; then
  printf 'callers must not forward a provider key; it is a canary-live environment secret\n' >&2
  exit 1
fi
if grep -Fq 'NAN_API_KEY' "$repository_root/.github/workflows/release.yml"; then
  printf 'the release workflow must not hold or forward the provider key\n' >&2
  exit 1
fi
grep -Fq 'workflow_dispatch:' "$desktop"
grep -Fq 'name: desktop-report-' "$desktop"
grep -Fq 'path: ${{ runner.temp }}/desktop-report/*.json' "$desktop"
if [ "$(grep -c 'NAN_API_KEY:' "$desktop")" -ne 1 ]; then
  printf 'the Desktop provider key belongs only to the live step\n' >&2
  exit 1
fi
grep -Fq -- '--mode deterministic --prepared' "$desktop"
grep -Fq -- '--mode live --prepared' "$desktop"
grep -Fq 'contents: read' "$desktop"
if grep -Eq '(contents|issues): write|--publish-feed' "$desktop"; then
  printf 'Desktop execution must not publish compatibility or receive write permissions\n' >&2
  exit 1
fi
