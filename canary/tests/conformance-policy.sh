#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
evaluator="$repository_root/canary/guest/evaluate-conformance.sh"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
report="$temporary_directory/conformance.json"

write_report() {
  local inventory_status="$1"
  local round_trip_status="$2"
  local sentinel_status="$3"
  local external_status="$4"
  cat >"$report" <<EOF
{
  "schemaVersion": 1,
  "harness": "hermes",
  "scenarios": [
    {"name":"inventory","status":"$inventory_status","checks":[{"name":"contract","status":"$inventory_status","durationMilliseconds":1}],"durationMilliseconds":1},
    {"name":"tool-round-trip","status":"$round_trip_status","checks":[{"name":"contract","status":"$round_trip_status","durationMilliseconds":1}],"durationMilliseconds":1},
    {"name":"sentinel","status":"$sentinel_status","checks":[{"name":"contract","status":"$sentinel_status","durationMilliseconds":1}],"durationMilliseconds":1},
    {"name":"external-prerequisite","status":"$external_status","checks":[{"name":"contract","status":"$external_status","durationMilliseconds":1}],"durationMilliseconds":1}
  ],
  "outcome": "failed",
  "durationMilliseconds": 4
}
EOF
}

assert_rejected() {
  if "$evaluator" "$report" hermes >/dev/null 2>&1; then
    printf 'conformance policy accepted a blocking failure\n' >&2
    exit 1
  fi
}

write_report failed passed passed skipped
drift="$($evaluator "$report" hermes 2>&1)"
jq -e '.kind == "inventory-drift" and .harness == "hermes"' <<<"$drift" >/dev/null

write_report passed passed passed skipped
[ -z "$($evaluator "$report" hermes 2>&1)" ]

write_report passed failed passed skipped
assert_rejected
write_report passed passed failed skipped
assert_rejected
write_report passed passed passed failed
assert_rejected

write_report failed passed passed skipped
if "$evaluator" "$report" codex >/dev/null 2>&1; then
  printf 'conformance policy accepted a mismatched harness\n' >&2
  exit 1
fi
jq 'del(.scenarios[-1])' "$report" >"$temporary_directory/incomplete.json"
mv "$temporary_directory/incomplete.json" "$report"
assert_rejected
printf '{}\n' >"$report"
assert_rejected

write_report failed passed passed skipped
jq '.schemaVersion = 2' "$report" >"$temporary_directory/schema-v2.json"
mv "$temporary_directory/schema-v2.json" "$report"
"$evaluator" "$report" hermes >/dev/null 2>&1

# Codex cannot qualify from a report that omitted either always-on model.
jq '.harness = "codex" | .scenarios[1].checks += [{"name":"reasoning-qwen38","status":"passed"},{"name":"reasoning-deepseek-v4","status":"passed"}]' "$report" >"$temporary_directory/codex.json"
"$evaluator" "$temporary_directory/codex.json" codex >/dev/null 2>&1
jq 'del(.scenarios[1].checks[-1])' "$temporary_directory/codex.json" >"$temporary_directory/missing.json"
if "$evaluator" "$temporary_directory/missing.json" codex >/dev/null 2>&1; then
  printf 'conformance policy accepted missing Codex reasoning evidence\n' >&2
  exit 1
fi
