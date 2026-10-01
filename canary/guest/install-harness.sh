#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 3 ]; then
  printf 'usage: %s <harness-id> [exact-version [source-commit]]\n' "$0" >&2
  exit 2
fi

harness="$1"
version="${2:-latest}"
ref="${3:-}"
if [ -n "$ref" ]; then
  if [ "$harness" != hermes ] || [ "$version" = latest ] \
    || ! printf '%s' "$ref" | grep -Eqx '[0-9a-f]{40}'; then
    printf 'an installer ref must be a frozen Hermes source commit\n' >&2
    exit 2
  fi
elif [ "$harness" = hermes ] && [ "$version" != latest ]; then
  printf 'an exact Hermes version requires its frozen source commit\n' >&2
  exit 2
fi
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
legacy_prefix="${NAN_CANARY_LEGACY_PATHS:-/opt/homebrew/bin:/usr/local/bin}"
if [ "${NAN_CANARY_HOSTED:-0}" = 1 ]; then
  # Hosted setup-node's PATH is authoritative. Keep isolated install bins
  # available, but append them so a legacy Homebrew/Tart Node cannot win.
  export PATH="${PATH:-}:$HOME/.local/bin:$HOME/.kimi-code/bin:$HOME/.hermes/bin"
else
  export PATH="$HOME/.local/bin:$HOME/.kimi-code/bin:$HOME/.hermes/bin:$legacy_prefix:${PATH:-}"
fi

verify_hosted_node() {
  [ "${NAN_CANARY_HOSTED:-0}" = 1 ] || return 0
  expected="${NAN_CANARY_EXPECTED_NODE_VERSION:-}"
  actual="$(node -p 'process.versions.node' 2>/dev/null || true)"
  if [ -z "$expected" ] || [ -z "$actual" ]; then
    printf 'hosted Node runtime is missing\n' >&2
    return 125
  fi
  if [ "$actual" != "$expected" ]; then
    printf 'hosted Node runtime version mismatch\n' >&2
    return 125
  fi
  command -v npm >/dev/null 2>&1 || {
    printf 'hosted npm runtime could not be found\n' >&2
    return 125
  }
}

download() {
  curl --fail --silent --show-error --location \
    --proto '=https' --tlsv1.2 --connect-timeout 15 --max-time 180 \
    --retry 4 --retry-all-errors --retry-max-time 240 \
    "$1" --output "$2"
}

global_npm_install() {
  verify_hosted_node
  npm install --global "$@"
  verify_hosted_node
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
    arguments=(--skip-setup --skip-browser --non-interactive)
    if [ "$version" = latest ]; then
      download 'https://hermes-agent.nousresearch.com/install.sh' "$installer"
    else
      download "https://raw.githubusercontent.com/NousResearch/hermes-agent/$ref/scripts/install.sh" "$installer"
      arguments+=(--commit "$ref" --force-commit)
    fi
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
    download \
      "https://github.com/can1357/oh-my-pi/releases/$release_path/$asset" \
      "$binary"
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
    arguments=(--allow-scripts='@deepseek-ai/dsh-subprocess-local,koffi,node-pty,@google/genai,protobufjs')
    # rc.2 ranges admit rc.3, whose dependency publication is incomplete.
    # Bound only this affected version to the last complete registry snapshot.
    if [ "$version" = '0.1.5-rc.2' ]; then
      arguments+=(--before=2026-09-22T00:00:00Z)
    fi
    global_npm_install \
      "${arguments[@]}" \
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
    arguments=()
    if [ "$version" != latest ]; then arguments=("v$version"); fi
    FX_INSTALL_DIR="$HOME/.local/bin" bash "$installer" "${arguments[@]}"
    ;;
  *)
    printf 'unsupported canary harness: %s\n' "$harness" >&2
    exit 2
    ;;
esac
