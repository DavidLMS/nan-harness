#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 1 ]; then
  printf 'usage: %s <harness-id>\n' "$0" >&2
  exit 2
fi

harness="$1"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
export PATH="$HOME/.local/bin:$HOME/.kimi-code/bin:$HOME/.hermes/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

download() {
  curl --fail --silent --show-error --location \
    --proto '=https' --tlsv1.2 --connect-timeout 15 --max-time 180 \
    --retry 4 --retry-all-errors --retry-max-time 240 \
    "$1" --output "$2"
}

global_npm_install() {
  npm install --global "$@"
}

run_with_bounded_curl() {
  local real_curl bounded_bin
  real_curl="$(command -v curl)"
  bounded_bin="$temporary_directory/bounded-bin"
  mkdir -p "$bounded_bin"
  cat >"$bounded_bin/curl" <<EOF
#!/bin/sh
exec "$real_curl" --connect-timeout 15 --max-time 120 --retry 4 --retry-all-errors --retry-max-time 180 "\$@"
EOF
  chmod 755 "$bounded_bin/curl"
  PATH="$bounded_bin:$PATH" "$@"
}

case "$harness" in
  claude-code)
    global_npm_install '@anthropic-ai/claude-code@latest'
    ;;
  codex)
    global_npm_install '@openai/codex@latest'
    ;;
  opencode)
    global_npm_install 'opencode-ai@latest'
    ;;
  hermes)
    installer="$temporary_directory/hermes-install.sh"
    download 'https://hermes-agent.nousresearch.com/install.sh' "$installer"
    bash "$installer" --skip-setup --skip-browser
    ;;
  pi)
    global_npm_install --ignore-scripts '@earendil-works/pi-coding-agent@latest'
    ;;
  prime-agent)
    installer="$temporary_directory/prime-agent-install.sh"
    download 'https://app.primeintellect.ai/prime-agent/install.sh' "$installer"
    run_with_bounded_curl sh "$installer"
    ;;
  deepseek-harness)
    global_npm_install \
      --allow-scripts='@deepseek-ai/dsh-subprocess-local,koffi,node-pty,@google/genai,protobufjs' \
      '@deepseek-ai/dsh@latest'
    ;;
  openclaw)
    global_npm_install \
      --allow-scripts='openclaw,@google/genai,protobufjs,tree-sitter-bash' \
      'openclaw@latest'
    ;;
  cline)
    global_npm_install 'cline@latest'
    ;;
  qwen-code)
    global_npm_install '@qwen-code/qwen-code@latest'
    ;;
  kimi-code)
    installer="$temporary_directory/kimi-install.sh"
    download 'https://code.kimi.com/kimi-code/install.sh' "$installer"
    KIMI_NO_MODIFY_PATH=1 bash "$installer"
    ;;
  aider)
    if ! command -v uv >/dev/null 2>&1; then
      python3 -m venv "$HOME/.local/share/nan-harness-canary-uv"
      "$HOME/.local/share/nan-harness-canary-uv/bin/python" -m pip install 'uv==0.11.31'
      export PATH="$HOME/.local/share/nan-harness-canary-uv/bin:$PATH"
    fi
    uv tool install --python 3.12 aider-chat
    ;;
  goose)
    installer="$temporary_directory/goose-install.sh"
    download 'https://github.com/aaif-goose/goose/releases/download/stable/download_cli.sh' "$installer"
    GOOSE_BIN_DIR="$HOME/.local/bin" CONFIGURE=false bash "$installer"
    ;;
  fx)
    installer="$temporary_directory/fx-install.sh"
    download 'https://fx.sh/setup.sh' "$installer"
    FX_INSTALL_DIR="$HOME/.local/bin" bash "$installer"
    ;;
  *)
    printf 'unsupported canary harness: %s\n' "$harness" >&2
    exit 2
    ;;
esac
