#!/usr/bin/env bash
set -euo pipefail

report="${1:-}"
harness="${2:-}"
[ -f "$report" ] && [ -n "$harness" ] || exit 2

jq --exit-status --arg harness "$harness" '
  type == "object" and
  (.schemaVersion == 1 or .schemaVersion == 2) and
  .harness == $harness and
  (.scenarios | type == "array" and length == 4) and
  ([.scenarios[].name] | sort == ["external-prerequisite", "inventory", "sentinel", "tool-round-trip"]) and
  all(.scenarios[];
    (.checks | type == "array" and length > 0) and
    if .name == "inventory" then
      (.status == "passed" or .status == "failed")
    elif .name == "external-prerequisite" then
      (.status == "passed" or .status == "skipped")
    else
      .status == "passed"
    end
  )
' "$report" >/dev/null

jq --compact-output --arg harness "$harness" '
  .scenarios[] |
  select(.name == "inventory" and .status == "failed") |
  {kind: "inventory-drift", harness: $harness}
' "$report" >&2
