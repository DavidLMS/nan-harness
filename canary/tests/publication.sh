#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
cleanup_test() {
  if [ "${NAN_CANARY_TEST_KEEP_TEMP:-}" = 1 ]; then
    printf 'publication fixture retained at %s\n' "$temporary_directory" >&2
  else
    rm -rf "$temporary_directory"
  fi
}
trap cleanup_test EXIT
bin_directory="$temporary_directory/bin"
remote_assets="$temporary_directory/remote-assets"
reports_directory="$temporary_directory/reports"
mkdir -p "$bin_directory" "$remote_assets" "$reports_directory"

cat >"$bin_directory/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
log_command() {
  printf '%s\n' "$*" >>"$GH_LOG"
}
release_exists() {
  [ -f "$REMOTE_ASSETS/.release" ]
}
asset_path() {
  printf '%s/%s\n' "$REMOTE_ASSETS" "$1"
}
if [ "${1:-}" = release ] && [ "${2:-}" = view ]; then
  log_command "$*"
  release_exists || exit 1
  assets='[]'
  for path in "$REMOTE_ASSETS"/*; do
    [ -f "$path" ] || continue
    name="$(basename "$path")"
    assets="$(jq --arg name "$name" '. + [{name:$name,createdAt:"2026-08-23T10:00:00Z"}]' <<<"$assets")"
  done
  jq -n --argjson assets "$assets" '{assets:$assets}'
  exit 0
fi
if [ "${1:-}" = api ]; then
  log_command "$*"
  if release_exists; then
    printf '%s\n' 'HTTP/2 200 OK'
    exit 0
  fi
  printf '%s\n' 'HTTP/2 404 Not Found'
  exit 1
fi
if [ "${1:-}" = release ] && [ "${2:-}" = download ]; then
  log_command "$*"
  pattern=''
  output=''
  directory='.'
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --pattern) pattern="$2"; shift 2 ;;
      --output) output="$2"; shift 2 ;;
      --dir) directory="$2"; shift 2 ;;
      *) shift ;;
    esac
  done
  [ "${REMOTE_DOWNLOAD_FAILURE:-}" != 1 ] || exit 1
  source_path="$(asset_path "$pattern")"
  [ -f "$source_path" ] || exit 1
  if [ -n "$output" ]; then
    cp "$source_path" "$output"
  else
    cp "$source_path" "$directory/$pattern"
  fi
  exit 0
fi
if [ "${1:-}" = release ] && [ "${2:-}" = upload ]; then
  log_command "$*"
  source=''
  shift 2
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --repo) shift 2 ;;
      --clobber|--yes) shift ;;
      *) source="$1"; shift ;;
    esac
  done
  [ -n "$source" ] || exit 1
  name="$(basename "$source")"
  if [ "${PUBLICATION_UPLOAD_FAILURE:-}" = 1 ] && [ "$name" = compatibility.json ] && [ ! -f "$REMOTE_ASSETS/.failed-once" ]; then
    touch "$REMOTE_ASSETS/.failed-once"
    exit 1
  fi
  cp "$source" "$(asset_path "$name")"
  exit 0
fi
if [ "${1:-}" = release ] && [ "${2:-}" = delete-asset ]; then
  log_command "$*"
  name="$4"
  source_path="$(asset_path "$name")"
  [ -f "$source_path" ] || exit 1
  rm -f "$source_path"
  exit 0
fi
if [ "${1:-}" = release ] && [ "${2:-}" = create ]; then
  log_command "$*"
  touch "$REMOTE_ASSETS/.release"
  source=''
  shift 2
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --repo|--target|--title|--notes) shift 2 ;;
      --prerelease) shift ;;
      *) source="$1"; shift ;;
    esac
  done
  [ -n "$source" ] || exit 1
  cp "$source" "$(asset_path "$(basename "$source")")"
  exit 0
fi
exit 1
EOF
chmod 755 "$bin_directory/gh"

cat >"$bin_directory/report-validator" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[ "${1:-}" = validate-report ] && [ -f "${2:-}" ] || exit 2
printf '%s\n' "$2" >>"$VALIDATOR_LOG"
[ "${REPORT_VALIDATOR_FAILURE_FOR:-}" != "$2" ] || exit 1
[ "${REPORT_VALIDATOR_FAILURE:-}" != 1 ]
EOF
chmod 755 "$bin_directory/report-validator"

write_report() {
  local directory="$1"
  local id="$2"
  local version="$3"
  mkdir -p "$directory"
  cat >"$directory/linux-$id.json" <<EOF
{
  "schemaVersion": 1,
  "trigger": "daily",
  "tier": "deterministic",
  "nanHarness": {"version": "0.0.6"},
  "harness": {"id": "$id", "version": "$version"},
  "completedAt": "2026-08-23T10:00:00Z",
  "checks": [
    {"name": "install-and-diagnose", "status": "passed"},
    {"name": "deterministic-conformance", "status": "passed"}
  ],
  "outcome": "passed"
}
EOF
}

write_reports() {
  rm -f "$reports_directory"/*.json
  write_report "$reports_directory" claude-code '9.9.9-rc.1+build.7'
  write_report "$reports_directory" codex '99.0.0'
}

invoke_publish() {
  GH_LOG="${GH_LOG_OVERRIDE:-$temporary_directory/gh.log}" \
    VALIDATOR_LOG="$temporary_directory/validator.log" \
    REMOTE_ASSETS="$remote_assets" \
    NAN_CANARY_RETRY_DELAY_SECONDS=0 \
    PATH="$bin_directory:$PATH" \
    "$repository_root/canary/host/publish-compatibility.sh" \
    --report-validator "$bin_directory/report-validator" "$@"
}

invoke_publish_without_validator() {
  GH_LOG="$temporary_directory/gh.log" \
    REMOTE_ASSETS="$remote_assets" \
    NAN_CANARY_RETRY_DELAY_SECONDS=0 \
    PATH="$bin_directory:$PATH" \
    "$repository_root/canary/host/publish-compatibility.sh" "$@"
}

prepare_base() {
  cargo xtask compatibility-feed "$temporary_directory/base.json" >/dev/null
  jq '.releases += [{"nanHarnessVersion":"0.0.5","verifications":[{"id":"fx","lastCompatibleVersion":"0.0.3","compatibleAt":"2026-08-20T00:00:00Z"}]}]' \
    "$temporary_directory/base.json" >"$temporary_directory/base-with-history.json"
  cp "$temporary_directory/base-with-history.json" "$remote_assets/compatibility.json"
  touch "$remote_assets/.release"
}

prepare_base
write_reports
fallback_home="$temporary_directory/fallback-home"
fallback_marker="$temporary_directory/fallback-cargo.marker"
fallback_output="$temporary_directory/fallback-output"
real_cargo="$(command -v cargo)"
real_cargo_home="${CARGO_HOME:-$HOME/.cargo}"
real_rustup_home="${RUSTUP_HOME:-$HOME/.rustup}"
mkdir -p "$fallback_home/.cargo/bin" "$fallback_output"
printf '#!/usr/bin/env bash\nset -euo pipefail\nprintf invoked >%q\nCARGO_HOME=%q RUSTUP_HOME=%q PATH=%q exec %q "$@"\n' \
  "$fallback_marker" "$real_cargo_home" "$real_rustup_home" "$PATH" "$real_cargo" \
  >"$fallback_home/.cargo/bin/cargo"
chmod 755 "$fallback_home/.cargo/bin/cargo"
ln -s "$(command -v jq)" "$bin_directory/jq"
HOME="$fallback_home" PATH='/usr/bin:/bin' invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$fallback_output" --state-dir "$temporary_directory/fallback-state"
[ -f "$fallback_marker" ]
jq -e '.schemaVersion == 2' "$fallback_output/compatibility.json" >/dev/null

missing_validator_output="$temporary_directory/missing-validator-output"
mkdir -p "$missing_validator_output"
set +e
invoke_publish_without_validator \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$missing_validator_output" --state-dir "$temporary_directory/missing-validator-state"
missing_validator_status=$?
set -e
[ "$missing_validator_status" -ne 0 ]
[ ! -f "$missing_validator_output/compatibility-updates/claude-code.json" ]

failed_validator_output="$temporary_directory/failed-validator-output"
mkdir -p "$failed_validator_output"
set +e
REPORT_VALIDATOR_FAILURE=1 invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$failed_validator_output" --state-dir "$temporary_directory/failed-validator-state"
failed_validator_status=$?
set -e
[ "$failed_validator_status" -ne 0 ]
[ ! -f "$failed_validator_output/compatibility-updates/claude-code.json" ]

mixed_validator_output="$temporary_directory/mixed-validator-output"
mixed_validator_log="$temporary_directory/mixed-validator-gh.log"
mkdir -p "$mixed_validator_output"
set +e
REPORT_VALIDATOR_FAILURE_FOR="$reports_directory/linux-codex.json" \
GH_LOG_OVERRIDE="$mixed_validator_log" invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$mixed_validator_output" --state-dir "$temporary_directory/mixed-validator-state" --publish-feed
mixed_validator_status=$?
set -e
[ "$mixed_validator_status" -ne 0 ]
[ ! -f "$mixed_validator_output/compatibility.json" ]
if compgen -G "$mixed_validator_output/compatibility-updates/*.json" >/dev/null; then
  exit 1
fi
if [ -f "$mixed_validator_log" ] && grep -Eq '(^| )(upload|create|delete-asset)( |$)' "$mixed_validator_log"; then
  exit 1
fi

invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$temporary_directory/validator-output" --state-dir "$temporary_directory/validator-state"
[ "$(wc -l <"$temporary_directory/validator.log" | tr -d ' ')" -ge 2 ]

prepare_base
write_reports
dry_output="$temporary_directory/dry-output"
mkdir -p "$dry_output"
invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$dry_output" --state-dir "$temporary_directory/dry-state"
jq -e --slurpfile base "$temporary_directory/base-with-history.json" '
  .schemaVersion == 2 and
  ([.releases[].nanHarnessVersion] | index("0.0.5") != null) and
  ([.releases[] | select(.nanHarnessVersion == "0.0.6") | .verifications[] | select(.id == "claude-code")][0].lastCompatibleVersion == "9.9.9-rc.1+build.7") and
  ([.releases[] | select(.nanHarnessVersion == "0.0.5")][0] == [$base[0].releases[] | select(.nanHarnessVersion == "0.0.5")][0])
' "$dry_output/compatibility.json" >/dev/null
if grep -Eq '(^| )(upload|create|delete-asset)( |$)' "$temporary_directory/gh.log"; then
  exit 1
fi

write_reports
jq '.schemaVersion = 2 | .observations = [{kind:"inventory-drift",fingerprint:([range(0;64)] | map("a") | join(""))}]' \
  "$reports_directory/linux-claude-code.json" >"$reports_directory/schema-v2.json"
mv "$reports_directory/schema-v2.json" "$reports_directory/linux-claude-code.json"
drift_output="$temporary_directory/drift-output"
mkdir -p "$drift_output"
invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$drift_output" --state-dir "$temporary_directory/drift-state"
jq -e '
  [.releases[] | select(.nanHarnessVersion == "0.0.6") | .verifications[] |
    select(.id == "claude-code")][0].lastCompatibleVersion == "9.9.9-rc.1+build.7"
' "$drift_output/compatibility.json" >/dev/null

write_reports
jq '.outcome = "failed"' "$reports_directory/linux-claude-code.json" >"$reports_directory/failed-overall.json"
mv "$reports_directory/failed-overall.json" "$reports_directory/linux-claude-code.json"
failed_overall_output="$temporary_directory/failed-overall-output"
mkdir -p "$failed_overall_output"
invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$failed_overall_output" --state-dir "$temporary_directory/failed-overall-state"
[ ! -f "$failed_overall_output/compatibility-updates/claude-code.json" ]

write_reports
jq '.harness.version = "01.2.3"' "$reports_directory/linux-claude-code.json" >"$reports_directory/invalid.json"
mv "$reports_directory/invalid.json" "$reports_directory/linux-claude-code.json"
rm -f "$reports_directory/linux-codex.json"
invalid_output="$temporary_directory/invalid-output"
mkdir -p "$invalid_output"
invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$invalid_output" --state-dir "$temporary_directory/invalid-state"
[ ! -f "$invalid_output/compatibility-updates/claude-code.json" ]

write_reports
set +e
REMOTE_DOWNLOAD_FAILURE=1 invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$temporary_directory/read-failure" --state-dir "$temporary_directory/read-failure-state"
read_failure_status=$?
set -e
[ "$read_failure_status" -ne 0 ]
jq -e '.schemaVersion == 2' "$remote_assets/compatibility.json" >/dev/null

printf '%s\n' '{"schemaVersion":1,"harnesses":[{"id":"fx","lastCompatibleVersion":"0.0.3","compatibleAt":"2026-08-20T00:00:00Z"}]}' >"$remote_assets/compatibility.json"
migration_output="$temporary_directory/migration-output"
mkdir -p "$migration_output"
write_reports
invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$migration_output" --state-dir "$temporary_directory/migration-state"
jq -e '.schemaVersion == 2 and (.releases | length == 1) and .releases[0].nanHarnessVersion == "0.0.6" and .releases[0].verifications[0].id == "fx"' "$migration_output/compatibility.json" >/dev/null

printf '%s\n' '{"schemaVersion":99,"releases":[]}' >"$remote_assets/compatibility.json"
set +e
invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$temporary_directory/malformed-output" --state-dir "$temporary_directory/malformed-state"
malformed_status=$?
set -e
[ "$malformed_status" -ne 0 ]

prepare_base
empty_release_output="$temporary_directory/empty-release-output"
mkdir -p "$empty_release_output"
rm -f "$remote_assets/compatibility.json" "$remote_assets"/compatibility.json.* "$remote_assets/.failed-once"
touch "$remote_assets/.release"
set +e
invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$empty_release_output" --state-dir "$temporary_directory/empty-release-state"
empty_release_status=$?
set -e
[ "$empty_release_status" -ne 0 ]
[ ! -f "$empty_release_output/compatibility.json" ]

prepare_base
replacement_output="$temporary_directory/replacement-output"
mkdir -p "$replacement_output"
write_reports
invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$replacement_output" --state-dir "$temporary_directory/replacement-state" --publish-feed
jq -e '.schemaVersion == 2 and ([.releases[].nanHarnessVersion] | index("0.0.5") != null)' "$remote_assets/compatibility.json" >/dev/null
backup_asset="$(find "$remote_assets" -name 'compatibility.json.backup.*' -type f | head -n 1)"
[ -n "$backup_asset" ]
compgen -G "$remote_assets/compatibility.json.candidate.*" >/dev/null

prepare_base
tampered_output="$temporary_directory/tampered-output"
mkdir -p "$tampered_output/compatibility-updates"
write_reports
printf '%s\n' '{"nanHarnessVersion":"0.0.5","id":"fx","lastCompatibleVersion":"0.0.4","compatibleAt":"2026-08-23T10:00:00Z"}' >"$tampered_output/compatibility-updates/historic.json"
set +e
invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$tampered_output" --state-dir "$temporary_directory/tampered-state"
tampered_status=$?
set -e
[ "$tampered_status" -ne 0 ]

prepare_base
rm -f "$remote_assets"/compatibility.json.candidate.* "$remote_assets"/compatibility.json.backup.* "$remote_assets/.failed-once"
write_reports
set +e
PUBLICATION_UPLOAD_FAILURE=1 invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$temporary_directory/failure-output" --state-dir "$temporary_directory/failure-state" --publish-feed
upload_failure_status=$?
set -e
[ "$upload_failure_status" -ne 0 ]
jq -e '.schemaVersion == 2 and ([.releases[].nanHarnessVersion] | index("0.0.5") != null)' "$remote_assets/compatibility.json" >/dev/null

prepare_base
rm -f "$remote_assets"/compatibility.json.candidate.* "$remote_assets"/compatibility.json.backup.* "$remote_assets/.failed-once"
write_reports
set +e
NAN_CANARY_PUBLICATION_INTERRUPT_PHASE=after-stable-delete invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$temporary_directory/interrupted-output" --state-dir "$temporary_directory/interrupted-state" --publish-feed
interrupted_status=$?
set -e
[ "$interrupted_status" -ne 0 ]
[ ! -f "$remote_assets/compatibility.json" ]
recovery_output="$temporary_directory/recovery-output"
mkdir -p "$recovery_output"
invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$recovery_output" --state-dir "$temporary_directory/recovery-state" --publish-feed
[ -f "$remote_assets/compatibility.json" ]

rm -f "$remote_assets/.release" "$remote_assets/compatibility.json"
first_output="$temporary_directory/first-output"
mkdir -p "$first_output"
invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$first_output" --state-dir "$temporary_directory/first-state" --publish-feed
[ -f "$remote_assets/.release" ]
[ -f "$remote_assets/compatibility.json" ]

prepare_base
mkdir -p "$temporary_directory/live-state/compatibility-feed.lock"
jq -n --argjson pid "$$" --arg host "$(hostname)" --arg token live --argjson startedAt "$(date +%s)" '{pid:$pid,host:$host,token:$token,startedAt:$startedAt}' >"$temporary_directory/live-state/compatibility-feed.lock/owner.json"
set +e
invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$temporary_directory/live-output" --state-dir "$temporary_directory/live-state"
live_lock_status=$?
set -e
[ "$live_lock_status" -ne 0 ]
[ -d "$temporary_directory/live-state/compatibility-feed.lock" ]

mkdir -p "$temporary_directory/stale-state/compatibility-feed.lock"
jq -n --argjson pid 999999 --arg host "$(hostname)" --arg token stale --argjson startedAt 1 '{pid:$pid,host:$host,token:$token,startedAt:$startedAt}' >"$temporary_directory/stale-state/compatibility-feed.lock/owner.json"
stale_output="$temporary_directory/stale-output"
mkdir -p "$stale_output"
invoke_publish \
  --trigger daily --nan-harness-version 0.0.6 --release-tag v0.0.6 \
  --reports "$reports_directory" --output-dir "$stale_output" --state-dir "$temporary_directory/stale-state"
