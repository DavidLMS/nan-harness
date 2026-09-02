#!/usr/bin/env bash
set -euo pipefail

summary="${1:-}"
[ -f "$summary" ] || exit 2
release_repository="${NAN_CANARY_RELEASE_REPOSITORY:-DavidLMS/nan-harness}"
notify_command="${NAN_CANARY_NOTIFY_COMMAND:-$(dirname "$0")/notify.sh}"
while IFS=$'\t' read -r subject kind harness version tier scenario failure_class; do
  if [ "$subject" = compatibility ] && [ "$kind" = suspected ]; then
    continue
  fi
  if [ "$subject" = inventory-drift ] && [ "$kind" = suspected ]; then
    "$notify_command" \
      "nan-harness inventory drift: $harness" \
      "$version exposed a changed tool inventory during $tier; core compatibility passed."
    continue
  fi
  case "$subject" in
    compatibility) title="[canary] $harness compatibility regression" ;;
    inventory-drift) title="[canary] $harness inventory drift" ;;
    *) continue ;;
  esac
  issue="$(gh issue list --repo "$release_repository" --state open --search "$title in:title" --json number,title \
    --jq ".[] | select(.title == \"$title\") | .number" | head -n 1)"
  if [ "$kind" = confirmed ]; then
    if [ "$subject" = inventory-drift ]; then
      body="$(printf 'The same tool inventory drift was observed twice consecutively while core compatibility continued to pass.\n\n- Harness: `%s`\n- Version: `%s`\n- Tier: `%s`\n- Scenario: `%s`\n\nTool names, prompts, model responses, paths, and credentials are excluded.\n' \
        "$harness" "$version" "$tier" "$scenario")"
    else
      body="$(printf 'The same safe local canary failure was observed twice consecutively.\n\n- Harness: `%s`\n- Version: `%s`\n- Tier: `%s`\n- Scenario: `%s`\n- Failure class: `%s`\n\nPrompts, model responses, tool output, paths, and credentials are excluded.\n' \
        "$harness" "$version" "$tier" "$scenario" "$failure_class")"
    fi
    if [ -z "$issue" ]; then
      gh issue create --repo "$release_repository" --title "$title" --body "$body" >/dev/null
    else
      gh issue edit "$issue" --repo "$release_repository" --body "$body" >/dev/null
    fi
    if [ "$subject" = inventory-drift ]; then
      "$notify_command" \
        "nan-harness inventory drift confirmed: $harness" \
        "$version exposed the same changed inventory twice; core compatibility still passed."
    else
      "$notify_command" \
        "nan-harness canary confirmed: $harness" \
        "$version failed twice during $tier ($failure_class)."
    fi
  elif [ "$kind" = recovered ]; then
    if [ -n "$issue" ]; then
      gh issue comment "$issue" --repo "$release_repository" --body 'Recovered in the local Tart canary.' >/dev/null
      gh issue close "$issue" --repo "$release_repository" --reason completed >/dev/null
    fi
    if [ "$subject" = inventory-drift ]; then
      "$notify_command" \
        "nan-harness inventory restored: $harness" \
        "$version matched the maintained inventory during $tier."
    else
      "$notify_command" \
        "nan-harness canary recovered: $harness" \
        "$version passed during $tier after a confirmed failure."
    fi
  fi
done < <(jq --raw-output '.alerts[] | [(.subject // "compatibility"), .kind, .harness, .harnessVersion, .tier, .scenario, (.failureClass // "unknown")] | @tsv' "$summary")
