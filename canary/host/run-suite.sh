#!/usr/bin/env bash
set -euo pipefail
umask 077

usage() {
  printf 'usage: %s --trigger <daily|weekly|release|manual> --nan-version <version> --linux-binary <path> [--macos-binary <path>] --output-dir <path> [--release-tag <tag>] [--harness <id>] [--guest <linux|macos>] [--promote]\n' "$0" >&2
  exit 2
}

trigger=''
nan_version=''
linux_binary=''
macos_binary=''
output_directory=''
release_tag=''
harness_filter=''
guest_filter=''
promote=false
while [ "$#" -gt 0 ]; do
  case "$1" in
    --trigger) trigger="${2:-}"; shift 2 ;;
    --nan-version) nan_version="${2:-}"; shift 2 ;;
    --linux-binary) linux_binary="${2:-}"; shift 2 ;;
    --macos-binary) macos_binary="${2:-}"; shift 2 ;;
    --output-dir) output_directory="${2:-}"; shift 2 ;;
    --release-tag) release_tag="${2:-}"; shift 2 ;;
    --harness) harness_filter="${2:-}"; shift 2 ;;
    --guest) guest_filter="${2:-}"; shift 2 ;;
    --promote) promote=true; shift ;;
    *) usage ;;
  esac
done

case "$trigger" in
  daily|weekly|release|manual) ;;
  *) usage ;;
esac
[ -n "$nan_version" ] && [ -f "$linux_binary" ] && [ -n "$output_directory" ] || usage
if [ "$trigger" = weekly ] || [ "$trigger" = release ] || [ "$guest_filter" = macos ]; then
  [ -f "$macos_binary" ] || usage
fi
if [ "$promote" = true ]; then
  [ "$trigger" = release ] && [ -n "$release_tag" ] && [ -z "$harness_filter" ] && [ -z "$guest_filter" ] || usage
fi
if [ "$trigger" = manual ]; then
  [ -n "$harness_filter" ] && [ -n "$guest_filter" ] || usage
elif [ -n "$harness_filter" ] || [ -n "$guest_filter" ]; then
  usage
fi
if [ -n "$guest_filter" ]; then
  case "$guest_filter" in
    linux|macos) ;;
    *) usage ;;
  esac
fi
network="${NAN_CANARY_NETWORK:-shared}"
case "$network" in
  shared|softnet) ;;
  *) printf 'NAN_CANARY_NETWORK must be shared or softnet\n' >&2; exit 2 ;;
esac

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repository_root/canary/host/lib.sh"
output_directory="$(mkdir -p "$output_directory" && cd "$output_directory" && pwd)"
for generated_path in run reports private-logs verifications compatibility-base.json compatibility.json summary.json; do
  if [ -e "$output_directory/$generated_path" ]; then
    printf 'canary output directory must not contain a previous %s artifact: %s\n' \
      "$generated_path" "$output_directory" >&2
    exit 2
  fi
done
state_directory="${NAN_CANARY_STATE_DIR:-$HOME/Library/Application Support/nan-harness-canary}"
mkdir -p "$state_directory"
suite_lock="$state_directory/suite.lock"
if ! shlock -p "$$" -f "$suite_lock"; then
  printf 'another NaN canary suite is already running\n'
  exit 0
fi

cleanup_canary_vms() {
  local vm
  while IFS= read -r vm; do
    case "$vm" in
      nan-canary-*)
        tart stop "$vm" >/dev/null 2>&1 || true
        tart delete "$vm" >/dev/null 2>&1 || true
        ;;
    esac
  done < <(tart list --source local --quiet 2>/dev/null || true)
}

release_suite_lock() {
  cleanup_canary_vms
  rm -f "$suite_lock"
}

trap release_suite_lock EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
cleanup_canary_vms
run_directory="$output_directory/run"
reports_directory="$output_directory/reports"
mkdir -p "$run_directory" "$reports_directory"
cp "$linux_binary" "$run_directory/nan-aarch64-unknown-linux-musl"
if [ -n "$macos_binary" ]; then
  cp "$macos_binary" "$run_directory/nan-aarch64-apple-darwin"
fi
cp "$repository_root/canary/guest/bootstrap.sh" "$run_directory/bootstrap.sh"
cp "$repository_root/canary/guest/install-harness.sh" "$run_directory/install-harness.sh"
cp "$repository_root/canary/guest/probe-harness.sh" "$run_directory/probe-harness.sh"
chmod 755 "$run_directory"/*

cargo build --locked --package nan-harness-canary --bin nan-canary
canary="$repository_root/target/debug/nan-canary"
harnesses=(
  claude-code codex opencode hermes pi prime-agent deepseek-harness
  openclaw cline qwen-code kimi-code aider goose fx
)
if [ -n "$harness_filter" ]; then
  harness_found=false
  for harness in "${harnesses[@]}"; do
    if [ "$harness" = "$harness_filter" ]; then
      harness_found=true
      break
    fi
  done
  [ "$harness_found" = true ] || usage
fi
if [ -n "$guest_filter" ]; then
  guests=("$guest_filter")
else
  guests=(linux)
fi
if [ -z "$guest_filter" ] && [ "$trigger" != daily ] && [ "$trigger" != manual ]; then
  guests+=(macos)
fi
rotation="$(date +%j)"
failures=0

for guest in "${guests[@]}"; do
  for index in "${!harnesses[@]}"; do
    harness="${harnesses[$index]}"
    if [ -n "$harness_filter" ] && [ "$harness" != "$harness_filter" ]; then
      continue
    fi
    live=false
    if [ "$trigger" = manual ] || [ "$trigger" != daily ] || [ "$index" -eq "$((rotation % ${#harnesses[@]}))" ] || [ "$index" -eq "$(((rotation + 1) % ${#harnesses[@]}))" ]; then
      live=true
    fi
    case "$guest" in
      linux)
        image='ghcr.io/cirruslabs/ubuntu:latest'
        artifact='nan-aarch64-unknown-linux-musl'
        ;;
      macos)
        image='ghcr.io/cirruslabs/macos-tahoe-base:latest'
        artifact='nan-aarch64-apple-darwin'
        ;;
    esac
    case "$trigger" in
      daily)
        if [ "$live" = true ]; then tier='live-core'; scenario='clean-install-and-live-tool'; else tier='installation'; scenario='clean-install'; fi
        ;;
      weekly) tier='live-extended'; scenario='clean-install-and-live-tool' ;;
      release) tier='release-gate'; scenario='release-install-and-live-tool' ;;
      manual) tier='live-core'; scenario='manual-clean-install-and-live-tool' ;;
    esac
    spec="$run_directory/$guest-$harness.toml"
    cat >"$spec" <<EOF
schema_version = 1
id = "$guest-$harness-$trigger"
harness = "$harness"
trigger = "$trigger"
tier = "$tier"
scenario = "$scenario"
image = "$image"
guest = "$guest"
network = "$network"
profile = "clean-$guest"
harness_version_file = "versions/$harness.txt"
overall_timeout_seconds = 3600
clone_timeout_seconds = 1800
boot_timeout_seconds = 300
$(if [ "$live" = true ]; then printf 'model = "qwen3.6"\n'; fi)

[nan]
version = "$nan_version"
source = "$(if [ -n "$release_tag" ]; then printf 'release:%s' "$release_tag"; else printf 'latest-release'; fi)"
artifact = "$artifact"

[[artifacts]]
source = "bootstrap.sh"
name = "bootstrap.sh"

[[artifacts]]
source = "install-harness.sh"
name = "install-harness.sh"

[[artifacts]]
source = "probe-harness.sh"
name = "probe-harness.sh"

[[steps]]
name = "bootstrap"
script = "bash '{{input}}/bootstrap.sh'"
failure_class = "infrastructure"
timeout_seconds = 600
attempts = 2

[[steps]]
name = "install-and-diagnose"
script = """
set -euo pipefail
export PATH="\$HOME/.local/bin:\$HOME/.kimi-code/bin:\$HOME/.hermes/bin:/opt/homebrew/bin:/usr/local/bin:\$PATH"
mkdir -p "\$HOME/.local/bin" '{{output}}/versions'
cp '{{input}}/$artifact' "\$HOME/.local/bin/nan"
chmod 755 "\$HOME/.local/bin/nan"
bash '{{input}}/install-harness.sh' '$harness'
if nan doctor --help | grep --quiet -- '--json'; then
  nan doctor '$harness' --allow-unsupported --allow-untested --json > '{{output}}/doctor.json'
  jq --exit-status --raw-output '.version' '{{output}}/doctor.json' > '{{output}}/versions/$harness.txt'
else
  nan doctor '$harness' --allow-unsupported --allow-untested > '{{output}}/doctor.txt'
  sed -n 's/^Version output: //p' '{{output}}/doctor.txt' \
    | grep -Eo '[0-9]+[.][0-9]+[.][0-9]+(-[0-9A-Za-z.-]+)?([+][0-9A-Za-z.-]+)?' \
    | head -n 1 > '{{output}}/versions/$harness.txt'
fi
test -s '{{output}}/versions/$harness.txt'
test "\$(cat '{{output}}/versions/$harness.txt')" != null
"""
failure_class = "installation"
timeout_seconds = 900
attempts = 2
EOF
    if [ "$live" = true ]; then
      cat >>"$spec" <<EOF

[[steps]]
name = "live-tool"
script = "bash '{{input}}/probe-harness.sh' '$harness'"
failure_class = "harness"
requires_api_key = true
timeout_seconds = 600
attempts = 2
EOF
    fi

    report="$reports_directory/$guest-$harness.json"
    private_logs="$output_directory/private-logs/$guest-$harness"
    if ! "$canary" cell \
      --spec "$spec" \
      --output "$report" \
      --private-log-dir "$private_logs"; then
      failures=$((failures + 1))
    fi
  done
done

state="$state_directory/aggregate-state.json"
summary="$output_directory/summary.json"
if compgen -G "$reports_directory/*.json" >/dev/null; then
  if "$canary" aggregate --reports "$reports_directory" --state "$state" --summary "$summary"; then
    if ! "$repository_root/canary/host/publish-alerts.sh" "$summary"; then
      printf 'warning: canary alerts could not be published; safe reports remain available locally\n' >&2
    fi
  else
    failures=$((failures + 1))
  fi
else
  printf 'canary suite produced no safe reports\n' >&2
  failures=$((failures + 1))
fi

if [ "$failures" -ne 0 ]; then
  exit 1
fi

if [ "$trigger" = weekly ] || [ "$trigger" = release ]; then
  verification_directory="$output_directory/verifications"
  mkdir -p "$verification_directory"
  for harness in "${harnesses[@]}"; do
    jq --compact-output \
      '{id: .harness.id, lastVerifiedVersion: .harness.version}' \
      "$reports_directory/linux-$harness.json" > "$verification_directory/$harness.json"
  done
  base="$output_directory/compatibility-base.json"
  feed="$output_directory/compatibility.json"
  if ! retry 4 5 gh release download compatibility --pattern compatibility.json --output "$base"; then
    cargo xtask compatibility-feed "$base"
  fi
  cargo xtask merge-compatibility-feed "$base" "$verification_directory" "$feed"
  retry 4 5 gh release upload compatibility "$feed" --clobber
fi

if [ "$promote" = true ]; then
  retry 4 5 gh release edit "$release_tag" --draft=false --latest
fi
