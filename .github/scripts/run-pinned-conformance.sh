#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -eq 0 ]; then
  printf 'usage: %s <harness-id>...\n' "$0" >&2
  exit 2
fi

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
installer="${NAN_PINNED_INSTALLER:-$repository_root/.github/scripts/install-pinned-harness.sh}"
export PATH="$HOME/.local/bin:$HOME/.kimi-code/bin:$HOME/.hermes/bin:$PATH"

for harness in "$@"; do
  bash "$installer" "$harness"
  case "$harness" in
    claude-code)
      cargo run --locked --quiet -- doctor claude
      cargo test --locked -p nan-harness-cli --test conformance_claude claude_code_inventory_matches_the_conformance_manifest -- --ignored --exact
      cargo test --locked -p nan-harness-cli --test conformance_claude claude_code_tools_complete_their_conformance_scenarios -- --ignored --exact
      cargo test --locked -p nan-harness-cli --test conformance_claude claude_code_external_tools_report_their_authentication_prerequisites -- --ignored --exact
      ;;
    codex)
      cargo run --locked --quiet -- doctor codex
      cargo test --locked -p nan-harness-cli --test conformance_codex codex_native_inventory_crosses_the_responses_bridge -- --ignored --exact
      ;;
    opencode|pi|omp|openclaw|cline|qwen-code|deepseek-harness|hermes|kimi-code|aider|prime-agent|goose)
      case "$harness" in
        qwen-code) command_name=qwen; test_filter=qwen_code_ ;;
        deepseek-harness) command_name=deepseek; test_filter=deepseek_harness_ ;;
        kimi-code) command_name=kimi; test_filter=kimi_code_ ;;
        prime-agent) command_name=prime; test_filter=prime_agent_ ;;
        omp) command_name=omp; test_filter=omp_ ;;
        *) command_name="$harness"; test_filter="${harness//-/_}_" ;;
      esac
      (
        if [ "$harness" = 'cline' ]; then
          export CLINE_NO_AUTO_UPDATE=1
        fi
        cargo run --locked --quiet -- doctor "$command_name"
        cargo test --locked -p nan-harness-cli --test conformance_direct "$test_filter" -- --ignored
      )
      ;;
    fx)
      cargo run --locked --quiet -- doctor fx
      cargo test --locked -p nan-harness-cli --test conformance_fx fx_ -- --ignored
      ;;
    *)
      printf 'unsupported pinned conformance harness: %s\n' "$harness" >&2
      exit 2
      ;;
  esac
done
