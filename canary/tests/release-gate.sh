#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
bin_directory="$temporary_directory/bin"
asset_source="$temporary_directory/source-assets"
mkdir -p "$bin_directory" "$asset_source"

for asset in \
  nan-harness-aarch64-unknown-linux-musl \
  nan-harness-canary-aarch64-unknown-linux-musl \
  nan-harness-aarch64-apple-darwin \
  nan-harness-canary-aarch64-apple-darwin; do
  printf '%s fixture\n' "$asset" >"$asset_source/$asset"
done

cat >"$bin_directory/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$GH_LOG"
if [ "${1:-}" = release ] && [ "${2:-}" = view ]; then
  printf '{"tagName":"%s","isDraft":%s}\n' "$GATE_TAG" "${GH_DRAFT:-true}"
  exit 0
fi
if [ "${1:-}" = release ] && [ "${2:-}" = download ]; then
  directory=''
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --dir) directory="$2"; shift 2 ;;
      *) shift ;;
    esac
  done
  cp "$ASSET_SOURCE"/* "$directory/"
  exit 0
fi
if [ "${1:-}" = release ] && [ "${2:-}" = edit ]; then
  printf promoted >>"$PROMOTION_LOG"
  exit "${PROMOTION_STATUS:-0}"
fi
if [ "${1:-}" = api ] && [ "${2:-}" = repos/Acme/Fork/releases/latest ]; then
  printf '%s\n' "${GH_LATEST_TAG:-$GATE_TAG}"
  exit 0
fi
exit 1
EOF

cat >"$bin_directory/prune" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat >"$bin_directory/verify" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
assets=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --assets-dir) assets="$2"; shift 2 ;;
    *) shift ;;
  esac
done
[ "${VERIFY_FAIL:-0}" != 1 ] || exit 1
printf 'verified manifest\n' >"$assets/SHA256SUMS"
EOF
cat >"$bin_directory/suite" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'suite\n' >>"$SUITE_LOG"
output=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output-dir) output="$2"; shift 2 ;;
    *) shift ;;
  esac
done
if [ "${SUITE_STATUS:-0}" -ne 0 ]; then
  exit "$SUITE_STATUS"
fi
mkdir -p "$output/reports" "$output/run"
printf '{}\n' >"$output/reports/fixture.json"
printf '#!/usr/bin/env bash\nexit 0\n' >"$output/run/nan-harness-canary-aarch64-apple-darwin"
chmod 755 "$output/run/nan-harness-canary-aarch64-apple-darwin"
EOF
cat >"$bin_directory/publish" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'publish\n' >>"$PUBLISH_LOG"
if [ -n "${PUBLISH_FAIL_ONCE_FILE:-}" ] && [ ! -f "$PUBLISH_FAIL_ONCE_FILE" ]; then
  touch "$PUBLISH_FAIL_ONCE_FILE"
  exit 1
fi
EOF
chmod 755 "$bin_directory"/*

run_gate() {
  local state="$1"
  shift
  mkdir -p "$state"
  ASSET_SOURCE="$asset_source" \
  GATE_TAG="${GATE_TAG:-v9.8.7}" \
  GH_LOG="$temporary_directory/gh.log" \
  SUITE_LOG="$temporary_directory/suite.log" \
  PUBLISH_LOG="$temporary_directory/publish.log" \
  PROMOTION_LOG="$temporary_directory/promotion.log" \
  NAN_CANARY_STATE_DIR="$state" \
  NAN_CANARY_TAG_WORKTREE=1 \
  NAN_CANARY_TAG_COMMIT=0123456789abcdef \
  NAN_CANARY_RETRY_DELAY_SECONDS=0 \
  NAN_CANARY_PRUNE_STATE_COMMAND="$bin_directory/prune" \
  NAN_CANARY_VERIFY_ASSETS_COMMAND="$bin_directory/verify" \
  NAN_CANARY_RUN_SUITE_COMMAND="$bin_directory/suite" \
  NAN_CANARY_PUBLISH_COMPATIBILITY_COMMAND="$bin_directory/publish" \
  PATH="$bin_directory:$PATH" \
    "$repository_root/canary/host/run-release-gate.sh" \
      --tag "${GATE_TAG:-v9.8.7}" --repo Acme/Fork "$@"
}

set +e
"$repository_root/canary/host/run-release-gate.sh" >/dev/null 2>&1
[ "$?" -eq 2 ]
set -e

success_state="$temporary_directory/state-success"
run_gate "$success_state"
receipt="$success_state/receipts/Acme__Fork/v9.8.7.json"
jq -e '.phases == {assetsVerified:true,suitePassed:true,feedPublished:true,releasePromoted:true}' "$receipt" >/dev/null
grep -Fq -- '--repo Acme/Fork' "$temporary_directory/gh.log"
[ "$(wc -l <"$temporary_directory/suite.log" | tr -d ' ')" -eq 1 ]
[ "$(wc -l <"$temporary_directory/publish.log" | tr -d ' ')" -eq 1 ]
[ -s "$temporary_directory/promotion.log" ]

: >"$temporary_directory/suite.log"
: >"$temporary_directory/publish.log"
cooldown_state="$temporary_directory/state-cooldown"
set +e
SUITE_STATUS=1 run_gate "$cooldown_state"
[ "$?" -eq 1 ]
run_gate "$cooldown_state"
[ "$?" -eq 75 ]
set -e
[ "$(wc -l <"$temporary_directory/suite.log" | tr -d ' ')" -eq 1 ]
run_gate "$cooldown_state" --force
[ "$(wc -l <"$temporary_directory/suite.log" | tr -d ' ')" -eq 2 ]

: >"$temporary_directory/suite.log"
: >"$temporary_directory/publish.log"
resume_state="$temporary_directory/state-resume"
publish_failure="$temporary_directory/publish-failed"
set +e
PUBLISH_FAIL_ONCE_FILE="$publish_failure" run_gate "$resume_state"
[ "$?" -eq 1 ]
set -e
jq -e '.phases.suitePassed == true and .phases.feedPublished == false' \
  "$resume_state/receipts/Acme__Fork/v9.8.7.json" >/dev/null
PUBLISH_FAIL_ONCE_FILE="$publish_failure" run_gate "$resume_state"
[ "$(wc -l <"$temporary_directory/suite.log" | tr -d ' ')" -eq 1 ]
[ "$(wc -l <"$temporary_directory/publish.log" | tr -d ' ')" -eq 2 ]

: >"$temporary_directory/suite.log"
: >"$temporary_directory/publish.log"
promotion_recovery_state="$temporary_directory/state-promotion-recovery"
set +e
PROMOTION_STATUS=1 run_gate "$promotion_recovery_state"
[ "$?" -eq 1 ]
set -e
jq -e '.phases.feedPublished == true and .phases.releasePromoted == false' \
  "$promotion_recovery_state/receipts/Acme__Fork/v9.8.7.json" >/dev/null
GH_DRAFT=false GH_LATEST_TAG=v9.8.7 run_gate "$promotion_recovery_state"
jq -e '.phases.releasePromoted == true' \
  "$promotion_recovery_state/receipts/Acme__Fork/v9.8.7.json" >/dev/null
[ "$(wc -l <"$temporary_directory/suite.log" | tr -d ' ')" -eq 1 ]
[ "$(wc -l <"$temporary_directory/publish.log" | tr -d ' ')" -eq 1 ]

set +e
GH_DRAFT=false run_gate "$temporary_directory/state-not-draft"
[ "$?" -eq 1 ]
VERIFY_FAIL=1 run_gate "$temporary_directory/state-verifier"
[ "$?" -eq 1 ]
set -e
[ ! -f "$temporary_directory/state-verifier/release-gate-Acme__Fork-v9.8.7.attempted" ]

# The outer wrapper must execute the gate implementation committed in the tag,
# not a newer implementation from the operator's working tree.
worktree_repository="$temporary_directory/worktree-repository"
mkdir -p "$worktree_repository/canary/host" "$temporary_directory/worktree-bin"
cp "$repository_root/canary/host/lib.sh" "$worktree_repository/canary/host/lib.sh"
cat >"$worktree_repository/canary/host/run-release-gate.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'commit=%s args=%s\n' "$NAN_CANARY_TAG_COMMIT" "$*" >"$WORKTREE_EXECUTION_LOG"
EOF
chmod 755 "$worktree_repository/canary/host/run-release-gate.sh"
git -C "$worktree_repository" init -q
git -C "$worktree_repository" config user.name canary-test
git -C "$worktree_repository" config user.email canary@example.test
git -C "$worktree_repository" add .
git -C "$worktree_repository" commit -qm tagged-gate
git -C "$worktree_repository" tag v1.2.3
tagged_commit="$(git -C "$worktree_repository" rev-parse v1.2.3^{commit})"
cp "$repository_root/canary/host/run-release-gate.sh" \
  "$worktree_repository/canary/host/run-release-gate.sh"
cat >"$temporary_directory/worktree-bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = api ]; then
  printf 'commit\t%s\n' "$REMOTE_TAG_COMMIT"
  exit 0
fi
exit 1
EOF
chmod 755 "$temporary_directory/worktree-bin/gh"
worktree_log="$temporary_directory/worktree-execution.log"
WORKTREE_EXECUTION_LOG="$worktree_log" \
REMOTE_TAG_COMMIT="$tagged_commit" \
NAN_CANARY_STATE_DIR="$temporary_directory/worktree-state" \
NAN_CANARY_PRUNE_STATE_COMMAND="$bin_directory/prune" \
PATH="$temporary_directory/worktree-bin:$PATH" \
  "$worktree_repository/canary/host/run-release-gate.sh" \
    --tag v1.2.3 --repo Acme/Fork --force
grep -Fq "commit=$tagged_commit" "$worktree_log"
grep -Fq 'args=--tag v1.2.3 --repo Acme/Fork --force' "$worktree_log"
