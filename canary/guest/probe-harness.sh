#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 1 ]; then
  printf 'usage: %s <harness-id>\n' "$0" >&2
  exit 2
fi

harness="$1"
export PATH="$HOME/.local/bin:$HOME/.kimi-code/bin:$HOME/.hermes/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
workspace="$(mktemp -d)"
output=''
cleanup() {
  result="$?"
  trap - EXIT
  if [ "$result" -ne 0 ] && [ -n "$output" ] && [ -f "$output" ]; then
    cat "$output" >&2
  fi
  rm -rf "$workspace"
  exit "$result"
}
trap cleanup EXIT
cd "$workspace"
mkdir -p "$workspace/home"
export HOME="$workspace/home"
export NAN_HARNESS_CONFIG_DIR="$workspace/nan-state"
marker="NAN_CANARY_READ_$(date +%s)_$RANDOM"
printf '%s\n' "$marker" > read-target.txt
prompt="Use the available file-reading tool to read '$workspace/read-target.txt'. Include the exact file content, then reply exactly NAN_CANARY_OK. Do not answer before the tool succeeds."
output="$workspace/harness-output.txt"
verify_read_marker=true

case "$harness" in
  claude-code)
    nan claude --model qwen3.6 -- \
      -p "$prompt" --output-format stream-json --verbose --no-session-persistence \
      --max-turns 4 --tools Read --allowedTools Read >"$output" 2>&1
    grep -F '"name":"Read"' "$output" >/dev/null
    ;;
  codex)
    verify_read_marker=false
    target="$workspace/codex-tool.txt"
    codex_prompt="Use exec_command to run printf NAN_CODEX_TOOL_OK > '$target'. After the command succeeds, reply exactly NAN_CANARY_OK."
    nan codex --model qwen3.6 -- \
      exec --skip-git-repo-check --ephemeral --json \
      --dangerously-bypass-approvals-and-sandbox "$codex_prompt" >"$output" 2>&1
    grep -Fx 'NAN_CODEX_TOOL_OK' "$target" >/dev/null
    ;;
  opencode)
    nan opencode --model qwen3.6 -- \
      run --pure --format json --auto "$prompt" >"$output" 2>&1
    grep -F '"tool":"read"' "$output" >/dev/null || grep -F '"read"' "$output" >/dev/null
    ;;
  hermes)
    verify_read_marker=false
    target="$workspace/hermes-tool.txt"
    hermes_prompt="Use the write_file tool to create '$target' with exactly NAN_HERMES_TOOL_OK. After it succeeds, reply exactly NAN_CANARY_OK."
    export BFL_API_KEY='' ELEVENLABS_API_KEY='' FAL_KEY='' OPENAI_API_KEY='' XAI_API_KEY=''
    nan hermes --model qwen3.6 -- \
      chat --query "$hermes_prompt" --quiet --yolo --safe-mode --source tool --max-turns 5 \
      >"$output" 2>&1
    grep -Fx 'NAN_HERMES_TOOL_OK' "$target" >/dev/null
    ;;
  pi)
    nan pi --model qwen3.6 -- \
      --mode json --print --no-session --no-extensions --no-skills \
      --no-prompt-templates --no-themes --no-context-files --tools read "$prompt" \
      >"$output" 2>&1
    grep -F '"toolName":"read"' "$output" >/dev/null || grep -F '"read"' "$output" >/dev/null
    ;;
  prime-agent)
    verify_read_marker=false
    target="$workspace/prime-tool.txt"
    prime_prompt="Use the ipython tool to write exactly NAN_PRIME_TOOL_OK to '$target'. After it succeeds, reply exactly NAN_CANARY_OK."
    nan prime --model qwen3.6 -- \
      --mode json --print --no-session --no-extensions --no-skills \
      --no-prompt-templates --no-themes --no-context-files --tools ipython "$prime_prompt" \
      >"$output" 2>&1
    grep -Fx 'NAN_PRIME_TOOL_OK' "$target" >/dev/null
    ;;
  deepseek-harness)
    verify_read_marker=false
    target="$workspace/deepseek-tool.txt"
    deepseek_prompt="Use the write tool to create '$target' with exactly NAN_DEEPSEEK_TOOL_OK. After the tool succeeds, reply exactly NAN_CANARY_OK."
    export DSH_PERMISSION_MODE=danger-full-access
    nan dsh --model qwen3.6 -- --profile headless "$deepseek_prompt" >"$output" 2>&1
    grep -Fx 'NAN_DEEPSEEK_TOOL_OK' "$target" >/dev/null
    ;;
  openclaw)
    verify_read_marker=false
    nan openclaw --model qwen3.6 -- \
      agent --local --session-id nan-harness-canary --message "$prompt" --json >"$output" 2>&1
    openclaw_json="$workspace/openclaw-output.json"
    sed -n '/^{/,/^}$/p' "$output" > "$openclaw_json"
    jq -e \
      '.meta.toolSummary.calls > 0 and
       .meta.toolSummary.failures == 0 and
       (.meta.toolSummary.tools | index("read") != null)' \
      "$openclaw_json" >/dev/null
    ;;
  cline)
    nan cline --model qwen3.6 -- --json --timeout 120 "$prompt" >"$output" 2>&1
    grep -F 'read_files' "$output" >/dev/null
    ;;
  qwen-code)
    nan qwen --model qwen3.6 -- \
      --safe-mode --prompt "$prompt" --output-format stream-json >"$output" 2>&1
    grep -F '"name":"read_file"' "$output" >/dev/null
    ;;
  kimi-code)
    nan kimi --model qwen3.6 -- \
      --prompt "$prompt" --output-format stream-json >"$output" 2>&1
    grep -F 'Read' "$output" >/dev/null
    ;;
  aider)
    verify_read_marker=false
    printf '%s\n' 'AIDER_CANARY_BEFORE' > edit-target.txt
    nan aider --model qwen3.6 -- \
      --message 'Replace the entire file content with exactly AIDER_CANARY_TOOL_OK, then reply exactly NAN_CANARY_OK.' \
      --yes-always --no-auto-commits --no-git --edit-format whole \
      --no-show-model-warnings --no-check-update --map-tokens 0 edit-target.txt \
      >"$output" 2>&1
    grep -Fx 'AIDER_CANARY_TOOL_OK' edit-target.txt >/dev/null
    ;;
  goose)
    nan goose --model qwen3.6 -- \
      run --no-profile --no-session --with-builtin developer --output-format json \
      --text "$prompt" >"$output" 2>&1
    grep -Eq '"name"[[:space:]]*:[[:space:]]*"shell"' "$output"
    ;;
  fx)
    verify_read_marker=false
    nan fx --model qwen3.6 -- \
      ask --yolo --no-save --no-color "$prompt" >"$output" 2>&1
    grep -F "Reading $workspace/read-target.txt" "$output" >/dev/null
    ;;
  *)
    printf 'unsupported canary harness: %s\n' "$harness" >&2
    exit 2
    ;;
esac

if [ "$verify_read_marker" = true ]; then
  grep -F "$marker" "$output" >/dev/null
fi
grep -F 'NAN_CANARY_OK' "$output" >/dev/null
if grep -F 'NH-BRIDGE-' "$output" >/dev/null; then
  exit 1
fi
