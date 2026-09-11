#!/usr/bin/env bash
# Fake-backend tests for the wave12 environment orchestrator and staging.
#
# Isolation (README.md, "Fake backend isolation"):
#  * Every orchestrator and staging process starts under `env -i` with PATH set
#    to a private toolbox of non-environment tools. No host environment command
#    (defaults, killall, pgrep, uname, ...) resolves there, and the orchestrator
#    itself refuses fake mode if one did.
#  * Fake mode is selected only by an explicit `--fake-backend`; every fake
#    driver carries a marker line that real binaries lack.
#  * Negative routing tests place logging sentinels named like host commands on
#    PATH, inside a backend, as an imported shell function and as the checker.
#    The suite finally proves that no sentinel ever executed.
# These tests establish script wiring only, never native desktop qualification.
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
orchestrator="$root/run-experiment.sh"
bin="$root/fixtures/bin"
commit=0123456789abcdef0123456789abcdef01234567
run_url=https://github.com/example/repo/actions/runs/1
host_commands="defaults killall pkill pgrep uname sw_vers osascript open launchctl screencapture nanh-desktop-check nan-harness"

tmp="$(mktemp -d)"
tmp="$(cd -P -- "$tmp" && pwd -P)"
trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/home"

# The only PATH any fake-mode process sees.
toolbox="$tmp/toolbox"
mkdir "$toolbox"
for tool in bash mkdir sleep shasum python3; do
  path="$(type -P "$tool")" || { echo "missing tool: $tool" >&2; exit 1; }
  ln -s "$path" "$toolbox/$tool"
done
for name in $host_commands; do
  if (PATH="$toolbox"; hash -r; command -v "$name" >/dev/null 2>&1); then
    echo "toolbox resolves host command: $name" >&2; exit 1
  fi
done

# Sentinels only append their own name to a log; executing one is a failure.
sentinel_log="$tmp/sentinel.log"
: > "$sentinel_log"
make_sentinel() {
  { printf '#!/bin/bash\n'
    printf 'printf "%%s\\n" %q >> %q\n' "$1" "$sentinel_log"
    printf 'exit 99\n'; } > "$2"
  chmod +x "$2"
}
sentinels="$tmp/sentinels"
mkdir "$sentinels"
for name in $host_commands runner-environment; do
  make_sentinel "$name" "$sentinels/$name"
done

fail=0
pass=0
ok()  { pass=$((pass+1)); echo "  ok  - $1"; }
bad() { fail=$((fail+1)); echo "  FAIL- $1"; }
chk() { local d="$1"; shift; if "$@"; then ok "$d"; else bad "$d"; fi; }
chkrc() { chk "$1 (exit=$RUNRC, want $2)" test "$RUNRC" = "$2"; }

NEXT=0
prepare() {
  NEXT=$((NEXT+1))
  case_dir="$tmp/case-$NEXT"
  mkdir "$case_dir"
  LOG="$case_dir/log"; : > "$LOG"
  STATE="$case_dir/state"; printf 'false\n' > "$STATE"
  MARKER="$case_dir/dock-alive"; : > "$MARKER"
  OUT="$case_dir/out"
  PREPARED="$case_dir/prepared.json"; printf '{ "prepared": true }\n' > "$PREPARED"
}

# run_with [VAR=value...] -- <orchestrator arguments>; sets RUNRC.
RUNRC=0
run_with() {
  local env_args=()
  while [ "$1" != -- ]; do env_args+=("$1"); shift; done
  shift
  set +e
  env -i PATH="$toolbox" HOME="$tmp/home" WAVE12_FIXTURE_LOG="$LOG" \
    WAVE12_FIXTURE_STATE="$STATE" WAVE12_FIXTURE_DOCK_MARKER="$MARKER" \
    WAVE12_READY_TIMEOUT_SECS=2 WAVE12_READY_POLL_SECS=1 \
    ${env_args[@]+"${env_args[@]}"} \
    "$toolbox/bash" "$orchestrator" "$@" 2> "$case_dir/stderr"
  RUNRC=$?
  set -e
}
# invoke <condition> [VAR=value...]: the standard fake-backend invocation.
invoke() {
  local condition="$1"; shift
  run_with "$@" -- "$condition" --fake-backend "$bin" \
    --checker "$bin/nanh-desktop-check" --prepared "$PREPARED" \
    --output-dir "$OUT" --source-commit "$commit" --run-url "$run_url"
}
STAGERC=0
stage() {
  set +e
  env -i PATH="$toolbox" HOME="$tmp/home" WAVE12_FIXTURE_LOG="$LOG" \
    "$toolbox/bash" "$root/stage-artifacts.sh" "$1" "$2" "$bin/nanh-desktop-check" 2>/dev/null
  STAGERC=$?
  set -e
}

evidence() { grep -qx -- "$2" "$OUT/$1/evidence.txt" 2>/dev/null; }
logged() { grep -qF -- "$1" "$LOG"; }
not_logged() { ! grep -qF -- "$1" "$LOG"; }
state_is() { [ "$(cat "$STATE")" = "$1" ]; }
# Compare the call sequence; checker calls are reduced to their subcommand.
sequence_is() {
  local actual
  actual="$(sed -E 's/^(checker [a-z-]+).*/\1/' "$LOG")"
  [ "$actual" = "$1" ] || { printf 'actual sequence:\n%s\n' "$actual" >&2; return 1; }
}
backend_copy() {
  local dir="$1" name
  mkdir "$dir"
  for name in defaults killall pgrep uname runner-environment nanh-desktop-check; do
    cp -p "$bin/$name" "$dir/$name"
  done
}

guarded="runner-environment
uname -s
uname -m"

echo "== Negative routing: no route reaches a host command =="
prepare
run_with PATH="$sentinels:$toolbox" GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted -- \
  dock-hidden --checker "$sentinels/nanh-desktop-check" --prepared "$PREPARED" --output-dir "$OUT"
chkrc "no fake backend: real drivers stay quarantined despite spoofed runner" 78
chk "quarantine creates no output" test ! -e "$OUT"

prepare
run_with WAVE12_FIXTURE_BIN="$bin" GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted -- \
  baseline --checker "$bin/nanh-desktop-check" --prepared "$PREPARED" --output-dir "$OUT"
chkrc "legacy WAVE12_FIXTURE_BIN no longer selects a backend" 78

prepare
run_with -- baseline --fixture-bin "$bin" --checker "$bin/nanh-desktop-check" \
  --prepared "$PREPARED" --output-dir "$OUT"
chkrc "legacy --fixture-bin is rejected" 2

prepare
run_with -- dock-hidden --fake-backend "$sentinels" --checker "$sentinels/nanh-desktop-check" \
  --prepared "$PREPARED" --output-dir "$OUT"
chkrc "backend of unmarked host-like executables refused" 78

prepare
invoke dock-hidden PATH="$sentinels:$toolbox"
chkrc "host commands resolvable on PATH: fake mode refused" 78
chk "PATH refusal ran no fake driver" test ! -s "$LOG"

prepare
invoke dock-hidden "BASH_FUNC_defaults%%=() { printf 'function\n' >> '$sentinel_log'; }"
chkrc "imported shell function named like a host command refused" 78

prepare
ln -s "$bin" "$case_dir/bin-link"
run_with -- baseline --fake-backend "$case_dir/bin-link" \
  --checker "$case_dir/bin-link/nanh-desktop-check" --prepared "$PREPARED" --output-dir "$OUT"
chkrc "symlinked backend directory refused" 78

prepare
backend_copy "$case_dir/b"; rm "$case_dir/b/uname"
run_with -- baseline --fake-backend "$case_dir/b" --checker "$case_dir/b/nanh-desktop-check" \
  --prepared "$PREPARED" --output-dir "$OUT"
chkrc "missing fake driver refused before any call" 78
chk "missing driver ran no fake driver" test ! -s "$LOG"

prepare
backend_copy "$case_dir/b"; rm "$case_dir/b/uname"; ln -s "$bin/uname" "$case_dir/b/uname"
run_with -- baseline --fake-backend "$case_dir/b" --checker "$case_dir/b/nanh-desktop-check" \
  --prepared "$PREPARED" --output-dir "$OUT"
chkrc "symlinked fake driver refused" 78

prepare
backend_copy "$case_dir/b"; make_sentinel defaults "$case_dir/b/defaults"
run_with -- dock-hidden --fake-backend "$case_dir/b" --checker "$case_dir/b/nanh-desktop-check" \
  --prepared "$PREPARED" --output-dir "$OUT"
chkrc "unmarked driver inside a backend refused" 78

prepare
backend_copy "$case_dir/b"; chmod -x "$case_dir/b/defaults"
run_with -- dock-hidden --fake-backend "$case_dir/b" --checker "$case_dir/b/nanh-desktop-check" \
  --prepared "$PREPARED" --output-dir "$OUT"
chkrc "non-executable driver refused" 78

prepare
run_with -- baseline --fake-backend "$bin" --checker "$sentinels/nanh-desktop-check" \
  --prepared "$PREPARED" --output-dir "$OUT"
chkrc "checker outside the fake backend refused" 4
chk "foreign checker refusal ran no driver" test ! -s "$LOG"

prepare
cp -p "$bin/nanh-desktop-check" "$case_dir/nanh-desktop-check"
run_with -- baseline --fake-backend "$bin" --checker "$case_dir/nanh-desktop-check" \
  --prepared "$PREPARED" --output-dir "$OUT"
chkrc "marked checker copy outside the backend refused" 4

prepare
invoke baseline WAVE12_FIXTURE_RUNNER_ENVIRONMENT=self-hosted \
  GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted
chkrc "fake mode ignores spoofed runner variables" 1
chk "non-disposable host creates no output" test ! -e "$OUT"
chk "non-disposable host never touches the Dock" not_logged defaults

echo "== Baseline condition =="
prepare
invoke baseline
chkrc "baseline exit" 0
chk "baseline call sequence" sequence_is "$guarded
checker run
checker validate-report
checker validate-occlusion"
chk "baseline report validated" evidence baseline report=validated
chk "baseline occlusion validated" evidence baseline occlusion=validated
chk "baseline prior state not read" evidence baseline prior_state=none
chk "baseline changes nothing" evidence baseline docks_changed=0
chk "baseline has no restoration" evidence baseline restore_status=none
chk "baseline digest recorded" test -s "$OUT/baseline/report.sha256"
chk "baseline leaves preference untouched" state_is false

echo "== Dock-hidden: prior false, true and absent are restored and verified =="
prepare
invoke dock-hidden
chkrc "dock-hidden from false" 0
chk "dock-hidden call sequence" sequence_is "$guarded
defaults read com.apple.dock autohide
defaults write com.apple.dock autohide -bool true
killall Dock
defaults read com.apple.dock autohide
pgrep -x Dock
checker run
checker validate-report
checker validate-occlusion
defaults write com.apple.dock autohide -bool false
killall Dock
defaults read com.apple.dock autohide"
chk "prior false recorded" evidence dock-hidden prior_state=false
chk "mutation recorded" evidence dock-hidden docks_changed=1
chk "restoration verified" evidence dock-hidden restore_status=ok
chk "preference ends false" state_is false

prepare
printf 'true\n' > "$STATE"
invoke dock-hidden
chkrc "dock-hidden from true" 0
chk "prior true recorded" evidence dock-hidden prior_state=true
chk "restored true verified" evidence dock-hidden restore_status=ok
chk "preference ends true" state_is true

prepare
: > "$STATE"
invoke dock-hidden
chkrc "dock-hidden from absent" 0
chk "prior absent recorded" evidence dock-hidden prior_state=absent
chk "absent prior restored by delete" logged "defaults delete com.apple.dock autohide"
chk "absent prior never rewritten as false" not_logged "-bool false"
chk "absent restoration verified" evidence dock-hidden restore_status=ok
chk "preference ends absent" test ! -s "$STATE"

prepare
invoke dock-hidden WAVE12_FIXTURE_KILLALL_EXIT=1
chkrc "Dock restart failure is best-effort" 0

echo "== Unknown prior state is never mutated =="
prepare
invoke dock-hidden WAVE12_FIXTURE_READ_UNREADABLE=1
chkrc "unreadable prior refused" 3
chk "unreadable prior never written" not_logged "defaults write"
chk "unreadable prior never deleted" not_logged "defaults delete"
chk "unreadable prior recorded" evidence dock-hidden prior_state=unreadable
chk "unreadable prior: no mutation" evidence dock-hidden docks_changed=0
chk "unreadable prior: probe not run" evidence dock-hidden probe_exit=not-run

prepare
printf 'maybe\n' > "$STATE"
invoke dock-hidden
chkrc "invalid prior refused" 3
chk "invalid prior never written" not_logged "defaults write"
chk "invalid prior recorded" evidence dock-hidden prior_state=invalid
chk "invalid prior value preserved" state_is maybe

echo "== Failure paths restore and record uncertainty =="
prepare
invoke dock-hidden WAVE12_FIXTURE_APPLY_FAILS=1
chkrc "unconfirmed state fails closed" 3
chk "unconfirmed state never probes" not_logged "checker"
chk "unconfirmed state still restores" logged "defaults write com.apple.dock autohide -bool false"
chk "unconfirmed state records no probe" evidence dock-hidden probe_exit=not-run
chk "unconfirmed state records no report" evidence dock-hidden report=absent
chk "unconfirmed state restoration verified" evidence dock-hidden restore_status=ok

prepare
printf 'true\n' > "$STATE"; rm "$MARKER"
invoke dock-hidden
chkrc "Dock process absent: readiness times out" 3
chk "readiness failure restoration verified" evidence dock-hidden restore_status=ok
chk "readiness failure preference ends true" state_is true

prepare
invoke dock-hidden WAVE12_FIXTURE_RESTORE=fail
chkrc "failed restoration write overrides success" 5
chk "failed restoration recorded" evidence dock-hidden restore_status=failed
chk "report still recorded beside the failure" evidence dock-hidden report=validated
chk "unrestored state is visible" state_is true

prepare
invoke dock-hidden WAVE12_FIXTURE_RESTORE=silent
chkrc "unverified restoration fails" 5
chk "unverified restoration recorded" evidence dock-hidden restore_status=failed

prepare
: > "$STATE"
invoke dock-hidden WAVE12_FIXTURE_RESTORE=fail
chkrc "failed delete of absent prior is reported" 5
chk "failed delete recorded" evidence dock-hidden restore_status=failed

prepare
rm "$MARKER"
invoke dock-hidden WAVE12_FIXTURE_RESTORE=fail
chkrc "restoration failure overrides readiness failure" 5

prepare
set +e
env -i PATH="$toolbox" HOME="$tmp/home" WAVE12_FIXTURE_LOG="$LOG" \
  WAVE12_FIXTURE_STATE="$STATE" WAVE12_FIXTURE_DOCK_MARKER="$MARKER" \
  WAVE12_FIXTURE_APPLY_FAILS=1 WAVE12_READY_TIMEOUT_SECS=30 \
  "$toolbox/bash" "$orchestrator" dock-hidden --fake-backend "$bin" \
    --checker "$bin/nanh-desktop-check" --prepared "$PREPARED" --output-dir "$OUT" \
    2> "$case_dir/stderr" &
pid=$!
for _ in $(seq 100); do logged "-bool true" && break; sleep 0.1; done
kill -TERM "$pid"
wait "$pid"
RUNRC=$?
set -e
chkrc "terminated run" 143
chk "terminated run still restores" evidence dock-hidden restore_status=ok
chk "terminated run preference ends false" state_is false

echo "== Probe and report outcomes =="
prepare
invoke dock-hidden WAVE12_FIXTURE_RUN_EXIT=1
chkrc "blocked probe is an honest validated result" 0
chk "blocked probe exit recorded" evidence dock-hidden probe_exit=1
chk "blocked probe restores" evidence dock-hidden restore_status=ok

prepare
invoke baseline WAVE12_FIXTURE_INVALID_REPORT=1
chkrc "invalid report fails closed" 4
chk "invalid report recorded" evidence baseline report=invalid
chk "invalid report has no digest" test ! -e "$OUT/baseline/report.sha256"

prepare
invoke dock-hidden WAVE12_FIXTURE_INVALID_REPORT=1
chkrc "invalid report in dock-hidden fails closed" 4
chk "invalid report still restores" evidence dock-hidden restore_status=ok

prepare
invoke baseline
invoke baseline
chkrc "second run refuses to reuse the condition directory" 3
chk "first evidence preserved" evidence baseline report=validated

prepare
rm "$PREPARED"
invoke baseline
chkrc "missing prepared identity fails closed" 4
prepare
invoke baseline WAVE12_FIXTURE_ARCH=x86_64
chkrc "non-arm64 host fails closed" 3
prepare
invoke baseline WAVE12_FIXTURE_OS=Linux
chkrc "non-Darwin host fails closed" 2
prepare
run_with -- baseline --fake-backend "$bin" --checker "$bin/nanh-desktop-check" \
  --prepared "$PREPARED" --output-dir "$OUT" --source-commit c1
chkrc "abbreviated source commit refused" 1
prepare
run_with -- baseline --fake-backend "$bin" --checker "$bin/nanh-desktop-check" \
  --prepared "$PREPARED" --output-dir "$OUT" --run-url https://example.test/run/1
chkrc "non-Actions run URL refused" 1

echo "== Staging publishes only validated, consistent facts =="
prepare
invoke baseline
stage "$OUT/baseline" "$case_dir/staged"
chk "baseline staged (exit=$STAGERC)" test "$STAGERC" = 0
chk "baseline staged manifest" test "$(cat "$case_dir/staged/staged.txt")" = "evidence.txt
occlusion.json
occlusion.sha256
probe-status.txt
report.json
report.sha256"

prepare
invoke dock-hidden WAVE12_FIXTURE_RESTORE=fail
stage "$OUT/dock-hidden" "$case_dir/staged"
chk "restoration failure staged (exit=$STAGERC)" test "$STAGERC" = 0
chk "restoration failure published" grep -qx restore_status=failed "$case_dir/staged/evidence.txt"

prepare
invoke dock-hidden WAVE12_FIXTURE_APPLY_FAILS=1
stage "$OUT/dock-hidden" "$case_dir/staged"
chk "unconfirmed state staged (exit=$STAGERC)" test "$STAGERC" = 0
chk "unconfirmed state publishes evidence only" test "$(cat "$case_dir/staged/staged.txt")" = "evidence.txt
probe-status.txt"

prepare
invoke dock-hidden WAVE12_FIXTURE_READ_UNREADABLE=1
stage "$OUT/dock-hidden" "$case_dir/staged"
chk "unreadable prior staged (exit=$STAGERC)" test "$STAGERC" = 0
chk "unreadable prior published" grep -qx prior_state=unreadable "$case_dir/staged/evidence.txt"

prepare
invoke baseline WAVE12_FIXTURE_INVALID_REPORT=1
printf 'synthetic-private-marker\n' > "$OUT/baseline/invalid-report.json"
stage "$OUT/baseline" "$case_dir/staged"
chk "invalid report staged (exit=$STAGERC)" test "$STAGERC" = 0
chk "invalid report never published" test ! -e "$case_dir/staged/report.json"
chk "private marker never published" test ! -e "$case_dir/staged/invalid-report.json"

prepare
invoke baseline
printf 'synthetic-private-marker\n' >> "$OUT/baseline/report.json"
stage "$OUT/baseline" "$case_dir/staged"
chk "tampered report refused (exit=$STAGERC)" test "$STAGERC" = 1
chk "tampered report publishes nothing" test ! -e "$case_dir/staged"

prepare
stage "$case_dir/no-such-condition" "$case_dir/staged"
chk "absent condition staged (exit=$STAGERC)" test "$STAGERC" = 0
chk "absent condition has an empty manifest" test ! -s "$case_dir/staged/staged.txt"

echo "== Isolation proof =="
chk "no host-command sentinel ever executed" test ! -s "$sentinel_log"

echo ""
echo "Fake-backend test result: $pass passed, $fail failed."
[ "$fail" -eq 0 ]
