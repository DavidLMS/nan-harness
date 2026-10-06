#!/usr/bin/env bash
set -euo pipefail
repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
python3 "$repository_root/canary/tests/hosted-actions.py"
python3 -B "$repository_root/canary/tests/hosted-cell-isolation.py"
python3 -B "$repository_root/canary/tests/hosted-cli-classification.py"
python3 -B "$repository_root/canary/tests/hosted-producer.py"
python3 -B "$repository_root/canary/tests/hosted-detector.py"
python3 "$repository_root/canary/tests/hosted-selection.py"
python3 "$repository_root/canary/tests/hosted-evidence.py"
python3 "$repository_root/canary/tests/hosted-provenance.py"
python3 -B "$repository_root/canary/tests/hosted-ingest.py"
python3 -B "$repository_root/canary/tests/native-release-matrix.py"
python3 -B "$repository_root/canary/tests/desktop-release.py"
python3 -B "$repository_root/canary/tests/desktop-suite.py"
python3 -B "$repository_root/canary/tests/desktop-install.py"
python3 -B "$repository_root/canary/tests/desktop-diagnostics.py"
python3 -B "$repository_root/canary/tests/desktop-diagnostic-workflow.py"
python3 -B "$repository_root/canary/tests/stage-cleanup.py"
python3 "$repository_root/canary/tests/hosted-publication.py"
python3 "$repository_root/canary/tests/desktop-publication.py"
python3 "$repository_root/canary/tests/desktop-session.py"
python3 -B "$repository_root/scripts/test-chatgpt-wave12-install.py"

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
grep -Fq 'group: release-channel-${{ github.repository }}' "$writer"
grep -Fq 'cancel-in-progress: false' "$writer"
# Main owns the CLI branch workflow and exact-release writer. Preserve the
# third-party execution boundary without asserting the retired queue layout.
cli="$(sed -n '/^  cli:/,$p' "$gate")"
if grep -Fq 'contents: write' <<<"$cli"; then
  printf 'third-party harnesses must never receive release write permissions\n' >&2
  exit 1
fi
grep -Fq 'ref: ${{ needs.select.outputs.source_ref }}' <<<"$cli"
grep -Fq -- '--source-kind branch --source-sha "$SOURCE_SHA"' <<<"$cli"
grep -Fq -- '--package nan-harness-cli --bin nan-harness' <<<"$cli"
grep -Fq -- '--package nan-harness-canary --bin nan-harness-canary' <<<"$cli"
grep -Fq 'environment: ${{ matrix.mode' <<<"$cli"
grep -Fq 'permissions:' "$gate"
grep -Fq 'contents: read' "$gate"
if grep -Fq 'publication.py' "$gate"; then
  printf 'branch CLI qualification must not invoke the retired publisher queue\n' >&2
  exit 1
fi
python3 -B "$repository_root/canary/tests/hosted-cli-workflow.py"
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

# Retired queue workflows must never become a competing writer after integration.
for retired in "$writer" "$approval" "$repository_root/.github/workflows/hosted-evidence-ingest.yml"; do
  python3 - "$retired" <<'PY_CHECK'
import pathlib, re, sys
text = pathlib.Path(sys.argv[1]).read_text()
jobs = re.split(r"(?m)^  [a-z][a-z-]*:\n", text.split("jobs:\n", 1)[1])[1:]
assert jobs and all("    if: ${{ false }}\n" in job for job in jobs)
PY_CHECK
done
