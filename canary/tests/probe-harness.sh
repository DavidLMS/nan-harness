#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT

fake_nan="$temporary_directory/nan"
cat >"$fake_nan" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

target=''
for argument in "$@"; do
  if [[ "$argument" =~ create\ \'([^\']+)\'\ with\ exactly\ NAN_HERMES_TOOL_OK ]]; then
    target="${BASH_REMATCH[1]}"
  fi
done
[ -n "$target" ]
printf '%s\n' 'NAN_HERMES_TOOL_OK' >"$target"
printf '%s\n' '{"schemaVersion":1,"status":"'"${FAKE_USAGE_STATUS:-observed}"'"}' \
  >"$NAN_HARNESS_INTERNAL_CANARY_USAGE_FILE"
printf '%s\n' 'NAN_CANARY_OK'
if [ "${FAKE_USAGE_STREAM:-stderr}" = stdout ]; then
  printf '%s\n' "${FAKE_USAGE_SUMMARY:-NaN usage (provider-reported) · qwen3.6 · 1 input · 1 output}"
else
  printf '%s\n' "${FAKE_USAGE_SUMMARY:-NaN usage (provider-reported) · qwen3.6 · 1 input · 1 output}" >&2
fi
EOF
chmod 755 "$fake_nan"

run_probe() {
  NAN_CANARY_NAN_COMMAND="$fake_nan" \
    NAN_CANARY_REDACT_FAILURE_OUTPUT=1 \
    bash "$repository_root/canary/guest/probe-harness.sh" hermes
}

run_probe
FAKE_USAGE_SUMMARY='NaN usage (provider-reported, partial) · qwen3.6 · 1 input · 1 output' \
  run_probe

stdout_failure="$temporary_directory/stdout-failure.txt"
if FAKE_USAGE_STREAM=stdout run_probe >"$stdout_failure" 2>&1; then
  printf 'probe unexpectedly accepted a usage summary on stdout\n' >&2
  exit 1
fi
grep -F 'live probe failed during usage-summary' "$stdout_failure" >/dev/null

evidence_failure="$temporary_directory/evidence-failure.txt"
if FAKE_USAGE_STATUS=not-observed run_probe >"$evidence_failure" 2>&1; then
  printf 'probe unexpectedly accepted missing provider usage\n' >&2
  exit 1
fi
grep -F 'live probe failed during usage-evidence' "$evidence_failure" >/dev/null
