#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 1 ]; then
  printf 'usage: %s <harness-id>\n' "$0" >&2
  exit 2
fi

harness="$1"
export PATH="$HOME/.local/bin:$HOME/.kimi-code/bin:$HOME/.hermes/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
nan_command="${NAN_CANARY_NAN_COMMAND:-nanh}"
model="${NAN_CANARY_MODEL:?NAN_CANARY_MODEL must be resolved before a live probe}"
workspace="$(mktemp -d)"
output=''
stderr_output=''
probe_stage='setup'
cleanup() {
  result="$?"
  trap - EXIT
  if [ "$result" -ne 0 ]; then
    printf 'live probe failed during %s\n' "$probe_stage" >&2
  fi
  if [ "$result" -ne 0 ] && [ "${NAN_CANARY_REDACT_FAILURE_OUTPUT:-}" != 1 ] \
    && [ -n "$output" ] && [ -f "$output" ]; then
    cat "$output" >&2
    if [ -n "$stderr_output" ] && [ -f "$stderr_output" ]; then
      cat "$stderr_output" >&2
    fi
  fi
  cleanup_attempt=0
  while [ -e "$workspace" ] && [ "$cleanup_attempt" -lt 10 ]; do
    rm -rf "$workspace" 2>/dev/null || true
    cleanup_attempt="$((cleanup_attempt + 1))"
    if [ -e "$workspace" ]; then
      sleep 1
    fi
  done
  if [ -e "$workspace" ]; then
    printf 'could not remove the ephemeral live-probe workspace\n' >&2
    result=1
    probe_stage='cleanup'
  fi
  # The cell classifies only this closed stage marker, never harness output.
  if [ -n "${NAN_CANARY_PROBE_RESULT:-}" ]; then
    probe_status='failed'
    if [ "$result" -eq 0 ]; then probe_status='passed'; fi
    printf '{"schemaVersion":1,"stage":"%s","status":"%s"}\n' "$probe_stage" "$probe_status" \
      >"$NAN_CANARY_PROBE_RESULT" 2>/dev/null || true
  fi
  exit "$result"
}
trap cleanup EXIT
cd "$workspace"
mkdir -p "$workspace/home"
export HOME="$workspace/home"
export NAN_HARNESS_CONFIG_DIR="$workspace/nan-state"
usage_evidence="$workspace/usage-evidence.json"
export NAN_HARNESS_INTERNAL_CANARY_USAGE_FILE="$usage_evidence"
marker="NAN_CANARY_READ_$(date +%s)_$RANDOM"
printf '%s\n' "$marker" > read-target.txt
prompt="Use the available file-reading tool to read '$workspace/read-target.txt'. Include the exact file content, then reply exactly NAN_CANARY_OK. Do not answer before the tool succeeds."
output="$workspace/harness-output.txt"
stderr_output="$workspace/harness-stderr.txt"
verify_read_marker=true
probe_stage='harness-run'

case "$harness" in
  claude-code)
    "$nan_command" claude --model "$model" -- \
      -p "$prompt" --output-format stream-json --verbose --no-session-persistence \
      --max-turns 4 --tools Read --allowedTools Read >"$output" 2>"$stderr_output"
    probe_stage='tool-evidence'
    grep -F '"name":"Read"' "$output" "$stderr_output" >/dev/null
    ;;
  codex)
    verify_read_marker=false
    target="$workspace/codex-tool.txt"
    codex_prompt="Use exec_command to run printf NAN_CODEX_TOOL_OK > '$target'. After the command succeeds, reply exactly NAN_CANARY_OK."
    "$nan_command" codex --model "$model" -- \
      exec --skip-git-repo-check --ephemeral --json \
      --dangerously-bypass-approvals-and-sandbox "$codex_prompt" \
      >"$output" 2>"$stderr_output"
    probe_stage='tool-evidence'
    grep -Fx 'NAN_CODEX_TOOL_OK' "$target" >/dev/null
    ;;
  opencode)
    "$nan_command" opencode --model "$model" -- \
      run --pure --format json --auto "$prompt" >"$output" 2>"$stderr_output"
    probe_stage='tool-evidence'
    grep -F '"tool":"read"' "$output" "$stderr_output" >/dev/null \
      || grep -F '"read"' "$output" "$stderr_output" >/dev/null
    ;;
  hermes)
    verify_read_marker=false
    target="$workspace/hermes-tool.txt"
    hermes_prompt="You must call write_file exactly once to create '$target' with exactly NAN_HERMES_TOOL_OK. Do not reply before the tool succeeds. Then reply exactly NAN_CANARY_OK."
    export BFL_API_KEY='' ELEVENLABS_API_KEY='' FAL_KEY='' OPENAI_API_KEY='' XAI_API_KEY=''
    "$nan_command" hermes --model "$model" -- \
      chat --query "$hermes_prompt" --toolsets file --quiet --yolo --safe-mode \
      --source tool --max-turns 5 \
      >"$output" 2>"$stderr_output"
    probe_stage='tool-evidence'
    grep -Fx 'NAN_HERMES_TOOL_OK' "$target" >/dev/null
    ;;
  pi)
    "$nan_command" pi --model "$model" -- \
      --mode json --print --no-session --no-extensions --no-skills \
      --no-prompt-templates --no-themes --no-context-files --tools read "$prompt" \
      >"$output" 2>"$stderr_output"
    probe_stage='tool-evidence'
    grep -F '"toolName":"read"' "$output" "$stderr_output" >/dev/null \
      || grep -F '"read"' "$output" "$stderr_output" >/dev/null
    ;;
  omp)
    "$nan_command" omp --model "$model" -- \
      --mode json --print --no-session --no-extensions --no-skills \
      --no-rules --no-lsp --no-title --tools read "$prompt" \
      >"$output" 2>"$stderr_output"
    probe_stage='tool-evidence'
    grep -F '"toolName":"read"' "$output" "$stderr_output" >/dev/null \
      || grep -F '"read"' "$output" "$stderr_output" >/dev/null
    ;;
  prime-agent)
    verify_read_marker=false
    target="$workspace/prime-tool.txt"
    prime_prompt="Use the ipython tool to write exactly NAN_PRIME_TOOL_OK to '$target'. After it succeeds, reply exactly NAN_CANARY_OK."
    "$nan_command" prime --model "$model" -- \
      --mode json --print --no-session --no-extensions --no-skills \
      --no-prompt-templates --no-themes --no-context-files --tools ipython "$prime_prompt" \
      >"$output" 2>"$stderr_output"
    probe_stage='tool-evidence'
    grep -Fx 'NAN_PRIME_TOOL_OK' "$target" >/dev/null
    ;;
  deepseek-harness)
    verify_read_marker=false
    target="$workspace/deepseek-tool.txt"
    deepseek_prompt="Use the write tool to create '$target' with exactly NAN_DEEPSEEK_TOOL_OK. After the tool succeeds, reply exactly NAN_CANARY_OK."
    export DSH_PERMISSION_MODE=danger-full-access
    "$nan_command" dsh --model "$model" -- --profile headless "$deepseek_prompt" \
      >"$output" 2>"$stderr_output"
    probe_stage='tool-evidence'
    grep -Fx 'NAN_DEEPSEEK_TOOL_OK' "$target" >/dev/null
    ;;
  openclaw)
    verify_read_marker=false
    "$nan_command" openclaw --model "$model" -- \
      agent --local --session-id nan-harness-canary --message "$prompt" --json \
      >"$output" 2>"$stderr_output"
    probe_stage='tool-evidence'
    openclaw_json="$workspace/openclaw-output.json"
    sed -n '/^{/,/^}$/p' "$output" > "$openclaw_json"
    jq -e \
      '.meta.toolSummary.calls > 0 and
       .meta.toolSummary.failures == 0 and
       (.meta.toolSummary.tools | index("read") != null)' \
      "$openclaw_json" >/dev/null
    ;;
  cline)
    "$nan_command" cline --model "$model" -- --json --timeout 120 "$prompt" \
      >"$output" 2>"$stderr_output"
    probe_stage='tool-evidence'
    grep -F 'read_files' "$output" "$stderr_output" >/dev/null
    ;;
  qwen-code)
    "$nan_command" qwen --model "$model" -- \
      --safe-mode --prompt "$prompt" --output-format stream-json \
      >"$output" 2>"$stderr_output"
    probe_stage='tool-evidence'
    grep -F '"name":"read_file"' "$output" "$stderr_output" >/dev/null
    ;;
  kimi-code)
    "$nan_command" kimi --model "$model" -- \
      --prompt "$prompt" --output-format stream-json >"$output" 2>"$stderr_output"
    probe_stage='tool-evidence'
    grep -F 'Read' "$output" "$stderr_output" >/dev/null
    ;;
  aider)
    verify_read_marker=false
    printf '%s\n' 'AIDER_CANARY_BEFORE' > edit-target.txt
    "$nan_command" aider --model "$model" -- \
      --message 'Replace the entire file content with exactly AIDER_CANARY_TOOL_OK, then reply exactly NAN_CANARY_OK.' \
      --yes-always --no-auto-commits --no-git --edit-format whole \
      --no-show-model-warnings --no-check-update --map-tokens 0 edit-target.txt \
      >"$output" 2>"$stderr_output"
    probe_stage='tool-evidence'
    grep -Fx 'AIDER_CANARY_TOOL_OK' edit-target.txt >/dev/null
    ;;
  goose)
    "$nan_command" goose --model "$model" -- \
      run --no-profile --no-session --with-builtin developer --output-format json \
      --text "$prompt" >"$output" 2>"$stderr_output"
    probe_stage='tool-evidence'
    grep -Eq '"name"[[:space:]]*:[[:space:]]*"shell"' "$output" "$stderr_output"
    ;;
  fx)
    verify_read_marker=false
    "$nan_command" fx --model "$model" -- \
      ask --yolo --no-save --no-color "$prompt" >"$output" 2>"$stderr_output"
    probe_stage='tool-evidence'
    grep -F "Reading $workspace/read-target.txt" "$output" "$stderr_output" >/dev/null
    ;;
  *)
    printf 'unsupported canary harness: %s\n' "$harness" >&2
    exit 2
    ;;
esac

if [ "$verify_read_marker" = true ]; then
  probe_stage='read-marker'
  grep -F "$marker" "$output" "$stderr_output" >/dev/null
fi
probe_stage='completion-marker'
grep -F 'NAN_CANARY_OK' "$output" "$stderr_output" >/dev/null
probe_stage='bridge-sentinel'
if grep -F 'NH-BRIDGE-' "$output" "$stderr_output" >/dev/null; then
  exit 1
fi
probe_stage='usage-evidence'
jq -e '.schemaVersion == 1 and .status == "observed"' "$usage_evidence" >/dev/null
probe_stage='usage-summary'
if ! grep -E '^(🔥 Tokens burned — this session|NaN usage \()' "$stderr_output" >/dev/null; then
  if grep -E '^(🔥 Tokens burned — this session|NaN usage \()' "$output" >/dev/null; then
    printf 'usage summary was written to stdout\n' >&2
  fi
  exit 1
fi
probe_stage='complete'
