#!/usr/bin/env bash
# Synthetic (fixture) tests for the wave12 environment orchestrator.
#
# These drive `run-experiment.sh` with the fixture command shims so the SAME
# control flow (host gating, preference save/set/restore, bounded readiness,
# per-condition artifact isolation, schema gating, upload gating) is exercised
# without touching any real macOS Dock, preference, window, checker, or runner.
# They establish command/script contract wiring only and do NOT establish any
# native desktop qualification.
#
# SAFETY: every orchestrator invocation is built by the single `invoke`
# function, which unconditionally passes `--fixture-bin $bin`. The orchestrator
# only uses real `defaults`/`killall`/`pgrep`/`uname` when no fixture bin is
# supplied, so this guarantees a test run never reaches a real system driver.
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
orchestrator="$root/run-experiment.sh"
bin="$root/fixtures/bin"

fail=0
pass=0
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

NEXT=0
prepare() {
  LOG="$tmp/log-$NEXT.txt"; : > "$LOG"
  STATE="$tmp/state-$NEXT.txt"; : > "$STATE"
  MARKER="$tmp/marker-$NEXT.txt"; : > "$MARKER"
  OUT="$tmp/out-$NEXT"
  PREPARED="$tmp/prepared-$NEXT.json"
  printf '{ "prepared": true }\n' > "$PREPARED"
  NEXT=$((NEXT+1))
}

# invoke <condition> [extra-env...] ; always injects --fixture-bin, sets RUNRC.
# Usage: invoke dock-hidden WAVE12_FIXTURE_ARCH=x86_64
RUNRC=0
invoke() {
  local cond="$1"; shift
  local extra_env=()
  while [ $# -gt 0 ]; do extra_env+=("$1"); shift; done
  set +e
  env -i PATH="$PATH" HOME="${HOME:-$tmp}" \
    GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted \
    WAVE12_FIXTURE_LOG="$LOG" WAVE12_FIXTURE_STATE="$STATE" \
    WAVE12_FIXTURE_DOCK_MARKER="$MARKER" \
    ${extra_env[@]+"${extra_env[@]}"} \
    "$orchestrator" "$cond" --fixture-bin "$bin" \
      --checker "$bin/nanh-desktop-check" --prepared "$PREPARED" \
      --output-dir "$OUT" --source-commit "c$NEXT" \
      --run-url "https://example.test/run/$NEXT" \
      2>/dev/null
  RUNRC=$?
  set -e
}

ok()  { pass=$((pass+1)); echo "  ok  - $1"; }
bad() { fail=$((fail+1)); echo "  FAIL- $1"; }
chk() { local d="$1"; shift; if "$@"; then ok "$d"; else bad "$d"; fi; }
chkrc() { chk "$1 (exit=$RUNRC)" test "$RUNRC" = "$2"; }
chkfile() { local p="${@: -1}"; chk "file: $p" test -f "$p"; }
chknotfile() { local p="${@: -1}"; chk "no file: $p" test ! -f "$p"; }
chkval() { chk "$1 = [$2]" test "$(cat "$3" 2>/dev/null || true)" = "$2"; }
chkhas() { chk "$1 contains [$3]" grep -q -- "$3" "$2"; }
chkno() { chk "$1 has no [$3]" test -z "$(grep -- "$3" "$2" || true)"; }

echo "== A. baseline succeeds, writes no Dock preference =="
prepare; invoke baseline
chkrc "baseline exit" 0
chkfile "$OUT/baseline/report.json"; chkfile "$OUT/baseline/occlusion.json"
chkfile "$OUT/baseline/report.sha256"; chkfile "$OUT/baseline/occlusion.sha256"
chkfile "$OUT/baseline/evidence.txt"
chkval "baseline report status" validated "$OUT/baseline/report.status"
chkval "baseline occlusion status" validated "$OUT/baseline/occlusion.status"
chkhas "baseline uploads report" "$OUT/baseline/uploads.txt" "report.json"
chkhas "baseline uploads occlusion" "$OUT/baseline/uploads.txt" "occlusion.json"
chkno "baseline never writes Dock pref" "$LOG" "defaults write com.apple.dock"
chkhas "baseline probes zed via checker" "$LOG" "checker run"

echo "== B. dock-hidden saves, sets, waits, restores the original =="
prepare; printf '0\n' > "$STATE"
invoke dock-hidden
chkrc "dock-hidden exit" 0
chkfile "$OUT/dock-hidden/report.json"; chkfile "$OUT/dock-hidden/occlusion.json"
chkval "dock-hidden report status" validated "$OUT/dock-hidden/report.status"
chkval "dock-hidden occlusion status" validated "$OUT/dock-hidden/occlusion.status"
chkhas "saves original value" "$LOG" "defaults read com.apple.dock autohide"
chkhas "writes autohide true" "$LOG" "defaults write com.apple.dock autohide -bool true"
chkhas "restarts after set" "$LOG" "killall Dock"
chkhas "restores to false" "$LOG" "defaults write com.apple.dock autohide -bool 0"
chk "state ends restored to 0" test "$(cat "$STATE")" = 0

echo "== C. non-disposable host fails closed, no artifacts =="
prepare
invoke baseline RUNNER_ENVIRONMENT=
chkrc "non-disposable fails closed" 1
chk "non-disposable created no report" test ! -f "$OUT/baseline/report.json"

echo "== D. unsupported host (non-arm64) fails closed, no dock write =="
prepare
invoke dock-hidden WAVE12_FIXTURE_ARCH=x86_64
chkrc "unsupported host fails closed" 3
chkno "unsupported host writes no Dock pref" "$LOG" "defaults write com.apple.dock"

echo "== E. state confirmation fails: bounded wait times out, restore still runs =="
prepare; printf '0\n' > "$STATE"
invoke dock-hidden WAVE12_FIXTURE_APPLY_FAILS=1 WAVE12_READY_TIMEOUT_SECS=2 WAVE12_READY_POLL_SECS=1
chkrc "state confirmation fails closed (bounded)" 3
chknotfile "state fail produced no report" "$OUT/dock-hidden/report.json"
chk "state fail preserves evidence dir" test -d "$OUT/dock-hidden"
chkhas "state failure still restored value" "$LOG" "defaults write com.apple.dock autohide -bool 0"

echo "== F. blocked probe is an honest validated result and restores =="
prepare; printf '0\n' > "$STATE"
invoke dock-hidden WAVE12_FIXTURE_RUN_EXIT=1 WAVE12_FIXTURE_OCCLUDED=1
chkrc "blocked probe honest validated" 0
chkval "blocked report status" validated "$OUT/dock-hidden/report.status"
chkval "blocked occlusion status" validated "$OUT/dock-hidden/occlusion.status"
chkhas "blocked probe restores value" "$LOG" "defaults write com.apple.dock autohide -bool 0"

echo "== G. invalid canonical report fails closed, preserved, not uploadable =="
prepare
invoke baseline WAVE12_FIXTURE_INVALID_REPORT=1
chkrc "invalid report fails closed" 4
chkfile "invalid report preserved" "$OUT/baseline/report.json"
chkval "invalid report status" invalid "$OUT/baseline/report.status"
chk "invalid report excluded from upload set" test -z "$(grep 'report.json' "$OUT/baseline/uploads.txt" || true)"

echo "== H. per-condition isolation prevents overwrite =="
prepare; invoke baseline
chkrc "first baseline" 0
invoke baseline
chkrc "second baseline refuses overwrite" 3
chkfile "first baseline report preserved" "$OUT/baseline/report.json"

echo "== I. missing checker identity fails closed =="
prepare
set +e
env -i PATH="$PATH" HOME="${HOME:-$tmp}" GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted \
  WAVE12_FIXTURE_LOG="$LOG" WAVE12_FIXTURE_STATE="$STATE" WAVE12_FIXTURE_DOCK_MARKER="$MARKER" \
  "$orchestrator" baseline --fixture-bin "$bin" --checker "$tmp/does-not-exist" \
    --prepared "$PREPARED" --output-dir "$OUT" 2>/dev/null
RC=$?
set -e
chk "missing checker fails closed (exit=$RC)" test "$RC" = 4
chk "missing checker created no report" test ! -f "$OUT/baseline/report.json"

echo "== J. missing prepared identity fails closed =="
prepare
set +e
env -i PATH="$PATH" HOME="${HOME:-$tmp}" GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted \
  WAVE12_FIXTURE_LOG="$LOG" WAVE12_FIXTURE_STATE="$STATE" WAVE12_FIXTURE_DOCK_MARKER="$MARKER" \
  "$orchestrator" baseline --fixture-bin "$bin" --checker "$bin/nanh-desktop-check" \
    --prepared "$tmp/absent-prepared.json" --output-dir "$OUT" 2>/dev/null
RC=$?
set -e
chk "missing prepared fails closed (exit=$RC)" test "$RC" = 4

echo "== K. no fixture bin offered: driver refuses (fail closed), no real command =="
prepare
set +e
env -i PATH="$PATH" HOME="${HOME:-$tmp}" GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted \
  "$orchestrator" baseline --checker "$bin/nanh-desktop-check" \
    --prepared "$PREPARED" --output-dir "$OUT" 2>/dev/null
RC=$?
set -e
chk "no fixture bin fails closed (exit=$RC)" test "$RC" = 78
chk "no fixture bin created no report" test ! -f "$OUT/baseline/report.json"

echo "== L. missing driver file: driver refuses rather than falling through =="
prepare
missing_bin="$tmp/missing-bin"; mkdir -p "$missing_bin"
# Copy the fixture set EXCEPT uname, so guard_host's driver call is refused.
for f in defaults killall pgrep nanh-desktop-check; do cp "$bin/$f" "$missing_bin/$f"; done
set +e
env -i PATH="$PATH" HOME="${HOME:-$tmp}" GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted \
  "$orchestrator" baseline --fixture-bin "$missing_bin" \
    --checker "$missing_bin/nanh-desktop-check" --prepared "$PREPARED" \
    --output-dir "$OUT" 2>/dev/null
RC=$?
set -e
chk "missing driver fails closed (exit=$RC)" test "$RC" = 78
chk "missing driver created no report" test ! -f "$OUT/baseline/report.json"

echo "== M. symlinked driver: driver refuses rather than following the link =="
prepare
sym_bin="$tmp/sym-bin"; mkdir -p "$sym_bin"
for f in defaults killall pgrep nanh-desktop-check; do cp "$bin/$f" "$sym_bin/$f"; done
ln -s "$bin/uname" "$sym_bin/uname"
set +e
env -i PATH="$PATH" HOME="${HOME:-$tmp}" GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted \
  "$orchestrator" baseline --fixture-bin "$sym_bin" \
    --checker "$sym_bin/nanh-desktop-check" --prepared "$PREPARED" \
    --output-dir "$OUT" 2>/dev/null
RC=$?
set -e
chk "symlinked driver fails closed (exit=$RC)" test "$RC" = 78
chk "symlinked driver never created a report" test ! -f "$OUT/baseline/report.json"

echo "== N. non-fixture checker: never executed as a host command =="
prepare
sentinel="$tmp/sentinel-checker"
ran_marker="$tmp/sentinel-ran"
printf '#!/usr/bin/env bash\ntouch "%s"\nexit 0\n' "$ran_marker" > "$sentinel"
chmod +x "$sentinel"
set +e
env -i PATH="$PATH" HOME="${HOME:-$tmp}" GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted \
  WAVE12_FIXTURE_LOG="$LOG" WAVE12_FIXTURE_STATE="$STATE" WAVE12_FIXTURE_DOCK_MARKER="$MARKER" \
  "$orchestrator" baseline --fixture-bin "$bin" \
    --checker "$sentinel" --prepared "$PREPARED" --output-dir "$OUT" 2>/dev/null
RC=$?
set -e
chk "non-fixture checker fails closed (exit=$RC)" test "$RC" = 4
chk "non-fixture checker never executed the sentinel" test ! -f "$ran_marker"

echo "== O. unreadable prior preference: refuse mutation, never delete =="
prepare
set +e
env -i PATH="$PATH" HOME="${HOME:-$tmp}" GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted \
  WAVE12_FIXTURE_LOG="$LOG" WAVE12_FIXTURE_STATE="$STATE" WAVE12_FIXTURE_DOCK_MARKER="$MARKER" \
  WAVE12_FIXTURE_READ_UNREADABLE=1 \
  "$orchestrator" dock-hidden --fixture-bin "$bin" \
    --checker "$bin/nanh-desktop-check" --prepared "$PREPARED" --output-dir "$OUT" 2>/dev/null
RC=$?
set -e
chk "unreadable prior pref fails closed (exit=$RC)" test "$RC" = 3
chk "unreadable prior pref never wrote a value" test -z "$(grep 'defaults write com.apple.dock' "$LOG" || true)"
chk "unreadable prior pref never deleted a value" test -z "$(grep 'defaults delete com.apple.dock' "$LOG" || true)"

echo "== P. staging helper stages only validated artifacts =="
prepare; invoke baseline
stage="$tmp/stage-baseline"
WAVE12_FIXTURE_LOG="$LOG" bash "$root/stage-artifacts.sh" "$OUT/baseline" "$stage" "$bin/nanh-desktop-check"
chk "staging created staged.txt" test -f "$stage/staged.txt"
chk "staging included validated report" test -f "$stage/report.json"
chk "staging included validated occlusion" test -f "$stage/occlusion.json"
chk "staging included closed evidence" test -f "$stage/evidence.txt"
chk "staging included probe status" test -f "$stage/probe-status.txt"

echo "== Q. staging cannot smuggle a private marker payload =="
prepare; invoke baseline WAVE12_FIXTURE_INVALID_REPORT=1
# Place a private marker in the source dir; staging must never copy it.
printf 'secret\n' > "$OUT/baseline/invalid-report.json"
stage2="$tmp/stage-baseline-invalid"
WAVE12_FIXTURE_LOG="$LOG" bash "$root/stage-artifacts.sh" "$OUT/baseline" "$stage2" "$bin/nanh-desktop-check"
chk "invalid private-marked report absent from staging" test ! -f "$stage2/invalid-report.json"
chk "invalid report not staged as report.json" test ! -f "$stage2/report.json"
chk "invalid report not staged as report.sha256" test ! -f "$stage2/report.sha256"

echo "== R. staging mirrors an absent condition as an empty allowlist =="
stage3="$tmp/stage-absent"
WAVE12_FIXTURE_LOG="$LOG" bash "$root/stage-artifacts.sh" "$tmp/no-such-condition-dir" "$stage3" "$bin/nanh-desktop-check"
chk "absent condition staged.txt records absence" test -f "$stage3/staged.txt"
chk "absent condition staged no report" test ! -f "$stage3/report.json"
chk "absent condition staged no evidence" test ! -f "$stage3/evidence.txt"

echo ""
echo "Synthetic test result: $pass passed, $fail failed."
[ "$fail" -eq 0 ]
