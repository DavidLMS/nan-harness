#!/usr/bin/env bash
set -euo pipefail

summary="${1:-}"
[ -f "$summary" ] || exit 2
while IFS=$'\t' read -r kind harness version tier scenario failure_class; do
  if [ "$kind" = suspected ]; then
    continue
  fi
  title="[canary] $harness compatibility regression"
  issue="$(gh issue list --state open --search "$title in:title" --json number,title \
    --jq ".[] | select(.title == \"$title\") | .number" | head -n 1)"
  if [ "$kind" = confirmed ]; then
    body="$(printf 'The same safe local canary failure was observed twice consecutively.\n\n- Harness: `%s`\n- Version: `%s`\n- Tier: `%s`\n- Scenario: `%s`\n- Failure class: `%s`\n\nPrompts, model responses, tool output, paths, and credentials are excluded.\n' \
      "$harness" "$version" "$tier" "$scenario" "$failure_class")"
    if [ -z "$issue" ]; then
      gh issue create --title "$title" --body "$body" >/dev/null
    else
      gh issue edit "$issue" --body "$body" >/dev/null
    fi
    "$(dirname "$0")/notify.sh" \
      "NaN canary confirmed: $harness" \
      "$version failed twice during $tier ($failure_class)."
  elif [ "$kind" = recovered ]; then
    if [ -n "$issue" ]; then
      gh issue comment "$issue" --body 'Recovered in the local Tart canary.' >/dev/null
      gh issue close "$issue" --reason completed >/dev/null
    fi
    "$(dirname "$0")/notify.sh" \
      "NaN canary recovered: $harness" \
      "$version passed during $tier after a confirmed failure."
  fi
done < <(jq --raw-output '.alerts[] | [.kind, .harness, .harnessVersion, .tier, .scenario, (.failureClass // "unknown")] | @tsv' "$summary")
