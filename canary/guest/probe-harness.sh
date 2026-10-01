#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 1 ]; then
  printf 'usage: %s <harness-id>\n' "$0" >&2
  exit 2
fi

harness="$1"
original_home="$HOME"
if [ "${NAN_CANARY_HOSTED:-}" = 1 ]; then
  export PATH="$original_home/.local/bin:$original_home/.kimi-code/bin:$original_home/.hermes/bin:$PATH"
else
  export PATH="$original_home/.local/bin:$original_home/.kimi-code/bin:$original_home/.hermes/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
fi
nan_command="${NAN_CANARY_NAN_COMMAND:-nanh}"
model="${NAN_CANARY_MODEL:-qwen3.6}"
workspace="$(mktemp -d)"
output=''
stderr_output=''
probe_stage='setup'
probe_diagnostic=''
marker_path="${NAN_CANARY_PROBE_RESULT:-}"
write_marker() {
  marker_stage="$1"
  marker_status="$2"
  marker_diagnostic="${3:-}"
  [ -n "$marker_path" ] || return 0
  marker_parent="$(dirname "$marker_path")"
  marker_tmp=''
  marker_tmp="$(mktemp "$marker_parent/.probe-result.XXXXXX")" || return 1
  if ! chmod 600 "$marker_tmp" \
    || ! if [ -n "$marker_diagnostic" ]; then
         printf '{"schemaVersion":1,"stage":"%s","status":"%s","diagnostic":"%s"}\n' \
           "$marker_stage" "$marker_status" "$marker_diagnostic" > "$marker_tmp"
       else
         printf '{"schemaVersion":1,"stage":"%s","status":"%s"}\n' \
           "$marker_stage" "$marker_status" > "$marker_tmp"
       fi \
    || ! mv -f "$marker_tmp" "$marker_path"; then
    rm -f "$marker_tmp" 2>/dev/null || true
    marker_tmp=''
    return 1
  fi
  marker_tmp=''
}
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
    probe_stage='cleanup'
    probe_diagnostic=''
    result=1
  fi
  if [ "$result" -eq 0 ]; then
    probe_stage='complete'
    if ! write_marker complete passed; then
      printf 'could not write the live-probe result marker\n' >&2
      result=1
    fi
  elif ! write_marker "$probe_stage" failed "$probe_diagnostic"; then
    printf 'could not write the live-probe result marker\n' >&2
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
prompt="Use the available file-reading tool to read '$workspace/read-target.txt'. After the read succeeds, respond with two lines: the exact file content on the first line and NAN_CANARY_OK on the second line. Do not answer before the tool succeeds."
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
      --message 'Replace the entire file content with exactly AIDER_CANARY_TOOL_OK. After the edit succeeds, respond with the standalone token NAN_CANARY_OK as the final line of your response.' \
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
if ! grep -F 'NAN_CANARY_OK' "$output" "$stderr_output" >/dev/null; then
  if [ "$harness" = aider ]; then
    stdout_empty=true
    stderr_empty=true
    [ -s "$output" ] && stdout_empty=false
    [ -s "$stderr_output" ] && stderr_empty=false
    if [ "$stdout_empty" = true ] && [ "$stderr_empty" = true ]; then
      probe_diagnostic='aider-completion-marker-stdout-empty-stderr-empty'
    elif [ "$stdout_empty" = true ]; then
      probe_diagnostic='aider-completion-marker-stdout-empty-stderr-nonempty'
    elif [ "$stderr_empty" = true ]; then
      probe_diagnostic='aider-completion-marker-stdout-nonempty-stderr-empty'
    else
      probe_diagnostic='aider-completion-marker-stdout-nonempty-stderr-nonempty'
    fi
    printf '%s\n' "$probe_diagnostic" >&2
  fi
  exit 1
fi
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

if [ "$harness" = hermes ] || [ "$harness" = openclaw ]; then
  probe_stage='media-plan'
  media_plan="$workspace/media-plan.json"
  "$nan_command" "$harness" --model "$model" --dry-run --force-media \
    --allow-unsupported --allow-untested >"$media_plan" 2>/dev/null
  jq -e '[.. | strings | select(test("nan-whisper|nan-kokoro|image_gen/nan_harness|nan-harness-media"))] | length >= 3' \
    "$media_plan" >/dev/null
  media_directory="$workspace/media"
  mkdir -p "$media_directory"
  printf '%s\n' 'NaN media canary speech' >"$media_directory/tts-input.txt"
  probe_stage='media-tts'
  "$nan_command" __media tts --input "$media_directory/tts-input.txt" \
    --output "$media_directory/tts-output.mp3" >/dev/null 2>/dev/null
  test -s "$media_directory/tts-output.mp3"
  python3 - "$media_directory/stt-input.wav" <<'PY'
import sys
import wave

with wave.open(sys.argv[1], "wb") as stream:
    stream.setnchannels(1)
    stream.setsampwidth(2)
    stream.setframerate(16_000)
    stream.writeframes(b"\0\0" * 16_000)
PY
  probe_stage='media-stt'
  "$nan_command" __media stt --input "$media_directory/stt-input.wav" \
    --output "$media_directory/stt-output.txt" >/dev/null 2>/dev/null
  test -f "$media_directory/stt-output.txt"
  if [ "${NAN_CANARY_MEDIA_MODE:-}" = weekly ]; then
    probe_stage='media-image'
    "$nan_command" __media image --prompt 'A simple blue square on a white background' \
      --output "$media_directory/image-output.png" >/dev/null 2>/dev/null
    test -s "$media_directory/image-output.png"
  fi
fi
