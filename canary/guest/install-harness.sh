#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  printf 'usage: %s <harness-id> [exact-version]\n' "$0" >&2
  exit 2
fi

harness="$1"
version="${2:-latest}"
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

omp_binary_asset() {
  local system
  local architecture
  local platform
  system="$(uname -s)"
  architecture="$(uname -m)"

  case "$system" in
    Linux) platform='linux' ;;
    Darwin)
      platform='darwin'
      if [ "$(sysctl -in hw.optional.arm64 2>/dev/null || true)" = '1' ]; then
        architecture='arm64'
      fi
      ;;
    *)
      printf 'unsupported OMP platform: %s\n' "$system" >&2
      return 1
      ;;
  esac
  case "$architecture" in
    x86_64|amd64) architecture='x64' ;;
    arm64|aarch64) architecture='arm64' ;;
    *)
      printf 'unsupported OMP architecture: %s\n' "$architecture" >&2
      return 1
      ;;
  esac
  if [ "$platform" = 'linux' ] &&
    { [ -f /etc/alpine-release ] || ldd --version 2>&1 | grep -qi musl; }
  then
    platform='linux-musl'
  fi
  printf 'omp-%s-%s' "$platform" "$architecture"
}

case "$harness" in
  claude-code)
    global_npm_install "@anthropic-ai/claude-code@$version"
    ;;
  codex)
    global_npm_install "@openai/codex@$version"
    ;;
  opencode)
    global_npm_install "opencode-ai@$version"
    ;;
  hermes)
    installer="$temporary_directory/hermes-install.sh"
    download 'https://hermes-agent.nousresearch.com/install.sh' "$installer"
    arguments=(--skip-setup --skip-browser)
    if [ "$version" != latest ]; then arguments+=(--branch "v$version"); fi
    bash "$installer" "${arguments[@]}"
    ;;
  pi)
    global_npm_install --ignore-scripts "@earendil-works/pi-coding-agent@$version"
    ;;
  omp)
    asset="$(omp_binary_asset)"
    binary="$temporary_directory/$asset"
    release_path=latest/download
    if [ "$version" != latest ]; then release_path="download/v$version"; fi
    download "https://github.com/can1357/oh-my-pi/releases/$release_path/$asset" "$binary"
    chmod 755 "$binary"
    "$binary" --version >/dev/null
    mkdir -p "$HOME/.local/bin"
    cp "$binary" "$HOME/.local/bin/omp"
    chmod 755 "$HOME/.local/bin/omp"
    ;;
  prime-agent)
    installer="$temporary_directory/prime-agent-install.sh"
    download 'https://app.primeintellect.ai/prime-agent/install.sh' "$installer"
    if [ "$version" = latest ]; then
      PRIME_AGENT_INSTALLER_NONINTERACTIVE=1 run_with_bounded_curl sh "$installer"
    else
      PRIME_AGENT_INSTALLER_NONINTERACTIVE=1 run_with_bounded_curl sh "$installer" "$version"
    fi
    ;;
  deepseek-harness)
    global_npm_install \
      --allow-scripts='@deepseek-ai/dsh-subprocess-local,koffi,node-pty,@google/genai,protobufjs' \
      "@deepseek-ai/dsh@$version"
    ;;
  openclaw)
    global_npm_install \
      --allow-scripts='openclaw,@google/genai,protobufjs,tree-sitter-bash' \
      "openclaw@$version"
    ;;
  cline)
    global_npm_install "cline@$version"
    ;;
  qwen-code)
    global_npm_install "@qwen-code/qwen-code@$version"
    ;;
  kimi-code)
    installer="$temporary_directory/kimi-install.sh"
    download 'https://code.kimi.com/kimi-code/install.sh' "$installer"
    arguments=()
    if [ "$version" != latest ]; then arguments=(--version "$version"); fi
    KIMI_NO_MODIFY_PATH=1 bash "$installer" "${arguments[@]}"
    ;;
  aider)
    if ! command -v uv >/dev/null 2>&1; then
      python3 -m venv "$HOME/.local/share/nan-harness-canary-uv"
      "$HOME/.local/share/nan-harness-canary-uv/bin/python" -m pip install 'uv==0.11.31'
      export PATH="$HOME/.local/share/nan-harness-canary-uv/bin:$PATH"
    fi
    package=aider-chat
    if [ "$version" != latest ]; then package="aider-chat==$version"; fi
    uv tool install --python 3.12 "$package"
    ;;
  goose)
    installer="$temporary_directory/goose-install.sh"
    release_ref=stable
    if [ "$version" != latest ]; then release_ref="v$version"; fi
    download "https://github.com/aaif-goose/goose/releases/download/$release_ref/download_cli.sh" "$installer"
    goose_version=''
    if [ "$version" != latest ]; then goose_version="$version"; fi
    GOOSE_VERSION="$goose_version" GOOSE_BIN_DIR="$HOME/.local/bin" CONFIGURE=false bash "$installer"
    ;;
  fx)
    installer="$temporary_directory/fx-install.sh"
    download 'https://fx.sh/setup.sh' "$installer"
    # The official CDN uses v-prefixed directory names; doctor reports semver.
    arguments=()
    if [ "$version" != latest ]; then arguments=("v$version"); fi
    FX_INSTALL_DIR="$HOME/.local/bin" bash "$installer" "${arguments[@]}"
    ;;
  *)
    printf 'unsupported canary harness: %s\n' "$harness" >&2
    exit 2
    ;;
esac
