#!/usr/bin/env bash
set -euo pipefail

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
