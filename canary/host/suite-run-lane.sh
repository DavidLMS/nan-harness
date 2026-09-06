#!/usr/bin/env bash

# Sourced by run-suite.sh after loading its configuration. Lanes share the
# suite deadline, harness rotation, verified artifacts and output directories.
# Keep execution in the caller so its wait/termination lifecycle stays intact.

prepared_image_for_guest() {
  case "$1" in
    linux) printf '%s' "$prepared_linux_image" ;;
    macos) printf '%s' "$prepared_macos_image" ;;
  esac
}

run_guest_lane() {
  local guest="$1"
  local lane_failures=0
  local index harness remaining_seconds cell_timeout_seconds live image artifact
  local canary_artifact tier scenario spec report private_logs prepared_image
  local -a cell_command
  for index in "${!harnesses[@]}"; do
    harness="${harnesses[$index]}"
    if [ -n "$harness_filter" ] && [ "$harness" != "$harness_filter" ]; then
      continue
    fi
    remaining_seconds="$((suite_deadline - $(date +%s)))"
    if [ "$remaining_seconds" -le 0 ]; then
      printf 'canary suite exceeded its global time budget\n' >&2
      "$notify_command" \
        'nan-harness canary infrastructure failure' \
        "$trigger suite exceeded its global time budget." || true
      lane_failures=$((lane_failures + 1))
      break
    fi
    cell_timeout_seconds="$remaining_seconds"
    [ "$cell_timeout_seconds" -le 3600 ] || cell_timeout_seconds=3600
    live=false
    if [ "$trigger" = manual ] || [ "$trigger" != daily ] || [ "$index" -eq "$((rotation % ${#harnesses[@]}))" ] || [ "$index" -eq "$(((rotation + 1) % ${#harnesses[@]}))" ]; then
      live=true
    fi
    case "$guest" in
      linux)
        image='ghcr.io/cirruslabs/ubuntu:latest'
        artifact='nan-harness-aarch64-unknown-linux-musl'
        canary_artifact='nan-harness-canary-aarch64-unknown-linux-musl'
        ;;
      macos)
        image='ghcr.io/cirruslabs/macos-tahoe-base:latest'
        artifact='nan-harness-aarch64-apple-darwin'
        canary_artifact='nan-harness-canary-aarch64-apple-darwin'
        ;;
    esac
    case "$trigger" in
      daily)
        if [ "$live" = true ]; then tier='live-core'; scenario='clean-install-deterministic-and-live-tool'; else tier='deterministic'; scenario='clean-install-and-deterministic'; fi
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
overall_timeout_seconds = $cell_timeout_seconds
clone_timeout_seconds = 1800
boot_timeout_seconds = 300
$(if [ "$live" = true ]; then printf 'model = "qwen3.6"\n'; fi)

[nan_harness]
version = "$nan_harness_version"
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

[[artifacts]]
source = "evaluate-conformance.sh"
name = "evaluate-conformance.sh"

[[artifacts]]
source = "$canary_artifact"
name = "nan-harness-canary"

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
cp '{{input}}/$artifact' "\$HOME/.local/bin/nan-harness"
chmod 755 "\$HOME/.local/bin/nan-harness"
ln -sf nan-harness "\$HOME/.local/bin/nanh"
bash '{{input}}/install-harness.sh' '$harness'
if nanh doctor --help | grep --quiet -- '--json'; then
  nanh doctor '$harness' --allow-unsupported --allow-untested --json > '{{output}}/doctor.json'
  jq --exit-status --raw-output '.version' '{{output}}/doctor.json' > '{{output}}/versions/$harness.txt'
else
  nanh doctor '$harness' --allow-unsupported --allow-untested > '{{output}}/doctor.txt'
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
    cat >>"$spec" <<EOF

[[steps]]
name = "deterministic-conformance"
script = """
set -euo pipefail
export PATH="\$HOME/.local/bin:\$HOME/.kimi-code/bin:\$HOME/.hermes/bin:/opt/homebrew/bin:/usr/local/bin:\$PATH"
cp '{{input}}/nan-harness-canary' "\$HOME/.local/bin/nan-harness-canary"
chmod 755 "\$HOME/.local/bin/nan-harness-canary"
NAN_HARNESS_CONFORMANCE_DIAGNOSTICS=1 \
  "\$HOME/.local/bin/nan-harness-canary" conformance --nan-harness "\$HOME/.local/bin/nan-harness" --harness '$harness' --json > '{{output}}/conformance.json' || true
if ! bash '{{input}}/evaluate-conformance.sh' '{{output}}/conformance.json' '$harness'; then
  cat '{{output}}/conformance.json' >&2
  exit 1
fi
"""
failure_class = "harness"
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
    prepared_image="$(prepared_image_for_guest "$guest")"
    cell_command=("$canary")
    if [ -n "$prepared_image" ]; then
      cell_command=(env "NAN_CANARY_PREPARED_IMAGE=$prepared_image" "$canary")
    fi
    if ! "${cell_command[@]}" cell \
      --spec "$spec" \
      --output "$report" \
      --private-log-dir "$private_logs"; then
      lane_failures=$((lane_failures + 1))
      if [ ! -f "$report" ] || [ "$(jq -r '.failure.class // empty' "$report" 2>/dev/null)" = infrastructure ]; then
        "$notify_command" \
          'nan-harness canary infrastructure failure' \
          "$guest/$harness failed during $trigger; inspect the private host logs." || true
      fi
    fi
  done
  [ "$lane_failures" -eq 0 ]
}
