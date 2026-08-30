#!/usr/bin/env bash
set -euo pipefail

bootstrap_source="${BASH_SOURCE[0]}"
if command -v sha256sum >/dev/null 2>&1; then
  bootstrap_sha256="$(sha256sum "$bootstrap_source" | awk '{print $1}')"
else
  bootstrap_sha256="$(shasum -a 256 "$bootstrap_source" | awk '{print $1}')"
fi
marker_directory="$HOME/.cache/nan-harness-canary/bootstrap"
marker="$marker_directory/$bootstrap_sha256"
if [ -f "$marker" ] \
  && command -v jq >/dev/null 2>&1 \
  && command -v node >/dev/null 2>&1 \
  && command -v npm >/dev/null 2>&1 \
  && command -v python3 >/dev/null 2>&1 \
  && [ "$(node -p 'process.versions.node.split(".")[0]')" = 24 ]; then
  exit 0
fi

case "$(uname -s)" in
  Linux)
    sudo apt-get update
    sudo apt-get install --yes bzip2 ca-certificates curl git jq python3 python3-venv
    node_setup="$(mktemp)"
    curl --fail --silent --show-error --location \
      --proto '=https' --tlsv1.2 --connect-timeout 15 --max-time 120 \
      https://deb.nodesource.com/setup_24.x --output "$node_setup"
    sudo -E bash "$node_setup"
    rm -f "$node_setup"
    sudo apt-get install --yes nodejs
    ;;
  Darwin)
    export PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"
    brew update
    brew install jq node@24 uv
    brew link --overwrite node@24
    ;;
  *)
    printf 'unsupported canary guest: %s\n' "$(uname -s)" >&2
    exit 2
    ;;
esac

mkdir -p "$HOME/.local/bin"
npm config set prefix "$HOME/.local"
node --version
npm --version
python3 --version
jq --version
mkdir -p "$marker_directory"
printf '%s\n' "$bootstrap_sha256" >"$marker"
