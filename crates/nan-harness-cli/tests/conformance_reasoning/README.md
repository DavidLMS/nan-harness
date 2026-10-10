# Native reasoning conformance

These ignored tests run installed clients against a disposable localhost provider, using actual nan-harness generated configuration. They do not contact NaN or measure model reasoning quality. Every case uses an allowlisted environment, temporary HOME/configuration directories, a synthetic credential, and a 60-second process timeout. Servers stop when each fixture drops.

Run with the required executables on PATH, or set any of:

- `NAN_REASONING_DSH_EXECUTABLE`
- `NAN_REASONING_QWEN_EXECUTABLE`
- `NAN_REASONING_PRIME_AGENT_EXECUTABLE` (Prime >=0.10)
- `NAN_REASONING_OPENCODE_EXECUTABLE`
- `NAN_REASONING_MIMO_EXECUTABLE`
- `NAN_REASONING_AIDER_EXECUTABLE`
- `NAN_REASONING_CODEX_EXECUTABLE`
- `NAN_REASONING_ZCODE_EXECUTABLE`

```sh
cargo test --locked -p nan-harness-cli --test conformance_reasoning -- --ignored --nocapture --test-threads=2
cargo test --locked -p nan-harness-cli --test conformance_direct qwen_code_native -- --ignored --nocapture
```

The second command discovers Qwen through PATH. ZCode requires Node >=24.14 on PATH even when its executable is selected explicitly.

## Executed versions, 2026-10-07

| Client | Version | Scope |
| --- | --- | --- |
| DeepSeek Harness | 0.2.0-rc.2 | First native boot, repeated launch, status/refresh/remove; Qwen/Gemma off and effort wire; GLM high/max; adaptive omission; current native inventory and tool sequence |
| Qwen Code | 0.25.0 | Persistent defaults/off/high/max; GLM mandatory reasoning; Gemma off; MiMo toggle; managed tool replay; full inventory and tool sequence |
| OpenCode | 1.18.31 | Managed defaults/off/max and nested MiMo thinking toggle; persisted off/max |
| MiMo Code | 0.1.15 | Managed defaults/off/max and nested thinking toggle |
| Aider | 0.86.2 | Managed Qwen/Gemma none/max; persistent Gemma none |
| Codex CLI | 0.160.0 | Adaptive model catalog with null default; off and xhigh-to-max translation |
| ZCode | 0.16.9 | Generated option-map execution for auto/off/low/max |

The complete native matrix uses Node 24.21.0. Qwen/OpenCode were additionally exercised on Node 22.23.1. These are local synthetic conformance observations, not live inference validation or evidence for other client versions.

## Assertions and native defaults

Qwen emits its declared defaults: high for Qwen/Gemma, medium for GLM, and enabled for MiMo's separate chat-template toggle. GLM refuses to disable reasoning and falls back to its declared default. Native controls still emit the explicit selected values where supported.

OpenCode and MiMo main requests omit an unselected effort. Their title helpers have different defaults and can also advertise tools. The tests identify the main request by its single exact normalized user task; neither a matching helper effort nor tool presence alone can satisfy the assertions. OpenCode may send high in a title request even when the main request omits it.

Codex can send a reasoning object containing only a summary preference for adaptive models. The bridge must preserve this as omitted effort rather than reject the request. Off and maximum selections are supplied through saved typed preferences because nan-harness intentionally owns native routing flags.

The Qwen 0.25 conformance baseline replaces the retired `todo_write` inventory entry with `tool_call`, exercises its dispatch after `tool_search`, and retains skill, subagent and goal checks. The isolated fixture-skill scenario omits `--safe-mode`, which would disable that custom skill; the inventory probe still uses safe mode.

DeepSeek Harness persists modern Cordis patches for the shipped dsh-base profiles (`acp`, `web`, `headless`, `sdk`) and existing profiles composed from those same bundled layers. `sdk-minimal` and profiles without dsh-base are left untouched. Unknown additional bundles fail persistent setup before publication with a localized managed-launch alternative, because Cordis replaces whole entry configs. Managed launch reads a bounded private native dump to compose the actual selected profile, including custom provider siblings. Newly created custom profiles require configuration refresh.

DSH uses `nan-harness-budgeted` for Qwen/Gemma to combine an explicit high default with an off selector. Original `nan-harness` model IDs remain available for stored sessions. GLM has no off selector; adaptive DeepSeek, Qwen Flash, and MiMo have no effort selector. DSH's current adapter cannot express MiMo's independent `chat_template_kwargs.enable_thinking` switch, so no MiMo toggle is exposed. DSH 0.2 no longer advertises `ralph` or `str_replace_editor`; the conformance inventory and scenario cover the remaining tools.

## Additional generated-plugin verification

Separate isolated native probes exercised Pi 0.85.1 and Prime Agent 0.9.3 with
continued sessions, restoring maximum effort, explicit off and provider-auto.
Hermes CLI 0.21.3 loaded the generated provider both with environment-only
authentication and with the Desktop-style provider configuration. OMP CLI
18.0.11 exercised MiMo default/off/auto and adaptive omission; its published
18.1.13 SDK and catalog exercised 48 generated runtime/persistent model cases,
including rejection of unsupported MiMo maximum effort and retained streamed
reasoning. These probes do not constitute whole-Desktop qualification.

The adapter `reasoning_plugins` tests retain executable JavaScript/Python
regressions for session intent, model boundaries, native defaults and provider
extras. Published sources at the existing minima (Pi 0.84.2, Prime 0.7.2 and
Hermes 0.20.0) contain the required hooks; the actual native runs above used the
listed newer versions.

## Prime Agent Rust compatibility, 2026-10-10

Prime 0.10 uses native models and MCP rather than TypeScript extensions. Its custom
model loader ignores per-model compatibility settings, so `nan-thinking` carries
MiMo with provider-level `qwen-chat-template` compatibility. The reasoning test
covers managed and persistent launches, effort/off, MiMo on/off, automatic models,
and preservation of existing credentials. Native controls retain Prime's defaults.

Local conformance also passed with Prime 0.9.3 and 0.10.0. A synthetic MCP probe
completed `rlm.mcp.list_tools` and `call_tool` against a local search fixture using
an explicitly selected private search configuration, without forwarding credentials.
