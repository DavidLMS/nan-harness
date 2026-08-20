#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 1 ]; then
  printf 'usage: %s <harness-id>\n' "$0" >&2
  exit 2
fi

harness_id="$1"
repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
manifest="$repository_root/crates/nan-harness-runtime/resources/compatibility.json"
version="$(jq --exit-status --raw-output --arg id "$harness_id" \
  '.harnesses[] | select(.id == $id) | .lastVerifiedVersion' "$manifest")"

temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT

append_path() {
  local directory="$1"
  export PATH="$directory:$PATH"
  if [ -n "${GITHUB_PATH:-}" ]; then
    printf '%s\n' "$directory" >> "$GITHUB_PATH"
  fi
}

download() {
  local url="$1"
  local destination="$2"
  curl --fail --silent --show-error --location \
    --proto '=https' --tlsv1.2 --connect-timeout 15 --max-time 120 \
    "$url" --output "$destination"
}

uv_tool_install() {
  local python_version="$1"
  local package="$2"
  local uv_environment="${RUNNER_TEMP:-$temporary_directory}/nan-harness-uv"
  if [ ! -x "$uv_environment/bin/uv" ]; then
    python3 -m venv "$uv_environment"
    "$uv_environment/bin/python" -m pip install \
      --disable-pip-version-check --quiet 'uv==0.11.31'
  fi
  "$uv_environment/bin/uv" tool install --python "$python_version" "$package"
  append_path "$HOME/.local/bin"
}

case "$harness_id" in
  claude-code)
    npm install --global "@anthropic-ai/claude-code@$version"
    ;;
  codex)
    npm install --global "@openai/codex@$version"
    ;;
  opencode)
    npm install --global "opencode-ai@$version"
    ;;
  hermes)
    if [ "$version" != '0.20.2' ]; then
      printf 'Hermes %s has no pinned source revision in this installer\n' "$version" >&2
      exit 2
    fi
    hermes_commit='06b9141109fbd320b14b8c88645ab37fc4f42c9d'
    installer="$temporary_directory/hermes-install.sh"
    download "https://raw.githubusercontent.com/NousResearch/hermes-agent/$hermes_commit/scripts/install.sh" "$installer"
    bash "$installer" --commit "$hermes_commit" --force-commit --skip-setup --skip-browser
    append_path "$HOME/.local/bin"
    append_path "$HOME/.hermes/bin"
    ;;
  pi)
    npm install --global --ignore-scripts "@earendil-works/pi-coding-agent@$version"
    ;;
  prime-agent)
    installer="$temporary_directory/prime-agent-install.sh"
    download 'https://app.primeintellect.ai/prime-agent/install.sh' "$installer"
    sh "$installer" "$version"
    append_path "$HOME/.local/bin"
    ;;
  deepseek-harness)
    uv_tool_install 3.12 "deepseek-harness-cli==$version"
    ;;
  openclaw)
    npm install --global "openclaw@$version"
    ;;
  cline)
    npm install --global "cline@$version"
    ;;
  qwen-code)
    npm install --global "@qwen-code/qwen-code@$version"
    ;;
  kimi-code)
    uv_tool_install 3.13 "kimi-cli==$version"
    append_path "$HOME/.kimi-code/bin"
    ;;
  aider)
    uv_tool_install 3.12 "aider-chat==$version"
    ;;
  goose)
    installer="$temporary_directory/goose-install.sh"
    download "https://github.com/aaif-goose/goose/releases/download/v$version/download_cli.sh" "$installer"
    GOOSE_BIN_DIR="$HOME/.local/bin" GOOSE_VERSION="$version" CONFIGURE=false \
      bash "$installer"
    append_path "$HOME/.local/bin"
    ;;
  fx)
    installer="$temporary_directory/fx-install.sh"
    download 'https://fx.sh/setup.sh' "$installer"
    FX_INSTALL_DIR="$HOME/.local/bin" bash "$installer" "v$version"
    append_path "$HOME/.local/bin"
    ;;
  *)
    printf 'unsupported pinned harness: %s\n' "$harness_id" >&2
    exit 2
    ;;
esac
