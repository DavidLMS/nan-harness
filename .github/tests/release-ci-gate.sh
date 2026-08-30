#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
bin_directory="$temporary_directory/bin"
mkdir -p "$bin_directory"

cat >"$bin_directory/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "${CI_GATE_SCENARIO:-success}" in
  success)
    cat <<JSON
{"workflow_runs":[{"id":7,"head_sha":"$CI_GATE_SHA","head_branch":"main","event":"push","status":"completed","conclusion":"success","html_url":"https://example.test/7"}]}
JSON
    ;;
  pending-then-success)
    count=0
    [ ! -f "$CI_GATE_COUNT_FILE" ] || count="$(cat "$CI_GATE_COUNT_FILE")"
    count=$((count + 1))
    printf '%s\n' "$count" >"$CI_GATE_COUNT_FILE"
    if [ "$count" -eq 1 ]; then status=in_progress; conclusion=null; else status=completed; conclusion='"success"'; fi
    printf '{"workflow_runs":[{"id":8,"head_sha":"%s","head_branch":"main","event":"push","status":"%s","conclusion":%s,"html_url":"https://example.test/8"}]}\n' "$CI_GATE_SHA" "$status" "$conclusion"
    ;;
  failure)
    printf '{"workflow_runs":[{"id":9,"head_sha":"%s","head_branch":"main","event":"push","status":"completed","conclusion":"failure","html_url":"https://example.test/9"}]}\n' "$CI_GATE_SHA"
    ;;
  wrong-branch)
    printf '{"workflow_runs":[{"id":10,"head_sha":"%s","head_branch":"feature","event":"push","status":"completed","conclusion":"success","html_url":"https://example.test/10"}]}\n' "$CI_GATE_SHA"
    ;;
  malformed) printf '{not-json\n' ;;
esac
EOF
chmod 755 "$bin_directory/gh"

gate="$repository_root/.github/scripts/require-green-ci.sh"
sha='0123456789abcdef0123456789abcdef01234567'
common_environment=(
  CI_GATE_SHA="$sha"
  CI_GATE_COUNT_FILE="$temporary_directory/count"
  NAN_RELEASE_CI_POLL_SECONDS=1
  PATH="$bin_directory:$PATH"
)

env "${common_environment[@]}" CI_GATE_SCENARIO=success \
  "$gate" Acme/Example "$sha" >/dev/null
env "${common_environment[@]}" CI_GATE_SCENARIO=pending-then-success \
  NAN_RELEASE_CI_WAIT_SECONDS=2 "$gate" Acme/Example "$sha" >/dev/null

for scenario in failure wrong-branch malformed; do
  if env "${common_environment[@]}" CI_GATE_SCENARIO="$scenario" \
    NAN_RELEASE_CI_WAIT_SECONDS=0 "$gate" Acme/Example "$sha" >/dev/null 2>&1; then
    printf 'release CI gate unexpectedly accepted %s\n' "$scenario" >&2
    exit 1
  fi
done

for invalid_environment in \
  'NAN_RELEASE_CI_WAIT_SECONDS=' \
  'NAN_RELEASE_CI_POLL_SECONDS=' \
  'NAN_RELEASE_CI_POLL_SECONDS=0'; do
  if env "${common_environment[@]}" CI_GATE_SCENARIO=success \
    "$invalid_environment" "$gate" Acme/Example "$sha" >/dev/null 2>&1; then
    printf 'release CI gate accepted invalid timing configuration: %s\n' \
      "$invalid_environment" >&2
    exit 1
  fi
done
