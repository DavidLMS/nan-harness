#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  printf 'usage: %s <harness-id> [--latest]\n' "$0" >&2
  exit 2
fi

harness_id="$1"
install_mode="${2:---pinned}"
if [ "$install_mode" != '--pinned' ] && [ "$install_mode" != '--latest' ]; then
  printf 'unsupported install mode: %s\n' "$install_mode" >&2
  exit 2
fi
repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
manifest="$repository_root/crates/nan-harness-runtime/resources/compatibility.json"
version="$(jq --exit-status --raw-output --arg id "$harness_id" \
  '.harnesses[] | select(.id == $id) | .lastCompatibleVersion' "$manifest")"

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
    --retry 4 --retry-all-errors --retry-max-time 180 \
    "$url" --output "$destination"
}

retry_install() {
  local attempt
  local status=1
  for attempt in 1 2 3; do
    if "$@"; then
      return 0
    else
      status=$?
    fi
    if [ "$attempt" -lt 3 ]; then
      printf 'install attempt %s failed; retrying\n' "$attempt" >&2
      sleep "${NAN_PINNED_INSTALL_RETRY_DELAY_SECONDS:-5}"
    fi
  done
  return "$status"
}

uv_tool_install() {
  local python_version="$1"
  local package="$2"
  local prerelease="${3:-deny}"
  local uv_environment="${RUNNER_TEMP:-$temporary_directory}/nan-harness-uv"
  if [ ! -x "$uv_environment/bin/uv" ]; then
    python3 -m venv "$uv_environment"
    "$uv_environment/bin/python" -m pip install \
      --disable-pip-version-check --quiet 'uv==0.11.31'
  fi
  if [ "$prerelease" = 'allow' ]; then
    "$uv_environment/bin/uv" tool install --python "$python_version" --prerelease allow "$package"
  else
    "$uv_environment/bin/uv" tool install --python "$python_version" "$package"
  fi
  append_path "$HOME/.local/bin"
}

refresh_cline_binary_cache() {
  local global_root
  local postinstall
  local cached_binary
  global_root="$(npm root --global)"
  postinstall="$global_root/cline/postinstall.mjs"
  cached_binary="$global_root/cline/bin/.cline"
  if [ ! -f "$postinstall" ]; then
    printf 'Cline postinstall script not found: %s\n' "$postinstall" >&2
    return 1
  fi
  node "$postinstall"
  if [ ! -x "$cached_binary" ]; then
    printf 'Cline postinstall did not create an executable cache: %s\n' \
      "$cached_binary" >&2
    return 1
  fi
}

package_version() {
  if [ "$install_mode" = '--latest' ]; then
    printf 'latest'
  else
    printf '%s' "$version"
  fi
}

case "$harness_id" in
  claude-code)
    npm install --global "@anthropic-ai/claude-code@$(package_version)"
    ;;
  codex)
    npm install --global "@openai/codex@$(package_version)"
    ;;
  opencode)
    npm install --global "opencode-ai@$(package_version)"
    ;;
  hermes)
    if [ "$install_mode" = '--latest' ]; then
      installer="$temporary_directory/hermes-install.sh"
      download 'https://hermes-agent.nousresearch.com/install.sh' "$installer"
      bash "$installer" --skip-setup --skip-browser
    else
      if [ "$version" != '0.20.2' ]; then
        printf 'Hermes %s has no pinned source revision in this installer\n' "$version" >&2
        exit 2
      fi
      hermes_commit='06b9141109fbd320b14b8c88645ab37fc4f42c9d'
      installer="$temporary_directory/hermes-install.sh"
      download "https://raw.githubusercontent.com/NousResearch/hermes-agent/$hermes_commit/scripts/install.sh" "$installer"
      bash "$installer" --commit "$hermes_commit" --force-commit --skip-setup --skip-browser
    fi
    append_path "$HOME/.local/bin"
    append_path "$HOME/.hermes/bin"
    ;;
  pi)
    npm install --global --ignore-scripts "@earendil-works/pi-coding-agent@$(package_version)"
    ;;
  omp)
    installer="$temporary_directory/omp-install.sh"
    download 'https://omp.sh/install' "$installer"
    if [ "$install_mode" = '--latest' ]; then
      sh "$installer" --binary
    else
      sh "$installer" --binary --ref "v$version"
    fi
    append_path "$HOME/.local/bin"
    ;;
  prime-agent)
    installer="$temporary_directory/prime-agent-install.sh"
    download 'https://app.primeintellect.ai/prime-agent/install.sh' "$installer"
    if [ "$install_mode" = '--latest' ]; then
      sh "$installer"
    else
      sh "$installer" "$version"
    fi
    append_path "$HOME/.local/bin"
    ;;
  deepseek-harness)
    npm install --global \
      --allow-scripts='@deepseek-ai/dsh-subprocess-local,koffi,node-pty,@google/genai,protobufjs' \
      "@deepseek-ai/dsh@$(package_version)"
    ;;
  openclaw)
    npm install --global \
      --allow-scripts='openclaw,@google/genai,protobufjs,tree-sitter-bash' \
      "openclaw@$(package_version)"
    ;;
  cline)
    npm install --global \
      --allow-scripts='cline,protobufjs' \
      "cline@$(package_version)"
    refresh_cline_binary_cache
    ;;
  qwen-code)
    npm install --global "@qwen-code/qwen-code@$(package_version)"
    ;;
  kimi-code)
    installer="$temporary_directory/kimi-code-install.sh"
    download 'https://code.kimi.com/kimi-code/install.sh' "$installer"
    if [ "$install_mode" = '--latest' ]; then
      retry_install env KIMI_NO_MODIFY_PATH=1 bash "$installer"
    else
      retry_install env KIMI_NO_MODIFY_PATH=1 KIMI_VERSION="$version" bash "$installer"
    fi
    append_path "$HOME/.kimi-code/bin"
    ;;
  aider)
    if [ "$install_mode" = '--latest' ]; then
      uv_tool_install 3.12 'aider-chat'
    else
      uv_tool_install 3.12 "aider-chat==$version"
    fi
    ;;
  goose)
    installer="$temporary_directory/goose-install.sh"
    if [ "$install_mode" = '--latest' ]; then
      download 'https://github.com/aaif-goose/goose/releases/download/stable/download_cli.sh' "$installer"
      GOOSE_BIN_DIR="$HOME/.local/bin" CONFIGURE=false bash "$installer"
    else
      download "https://github.com/aaif-goose/goose/releases/download/v$version/download_cli.sh" "$installer"
      GOOSE_BIN_DIR="$HOME/.local/bin" GOOSE_VERSION="$version" CONFIGURE=false \
        bash "$installer"
    fi
    append_path "$HOME/.local/bin"
    ;;
  fx)
    installer="$temporary_directory/fx-install.sh"
    download 'https://fx.sh/setup.sh' "$installer"
    if [ "$install_mode" = '--latest' ]; then
      FX_INSTALL_DIR="$HOME/.local/bin" bash "$installer"
    else
      FX_INSTALL_DIR="$HOME/.local/bin" bash "$installer" "v$version"
    fi
    append_path "$HOME/.local/bin"
    ;;
  *)
    printf 'unsupported pinned harness: %s\n' "$harness_id" >&2
    exit 2
    ;;
esac
