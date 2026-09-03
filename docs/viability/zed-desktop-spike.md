# Zed desktop spike evidence

Status: **GO**

## Build identity

- Baseline commit: `7cc27dd`
- Branch: `spike/zed-desktop`
- Tracking issue: [#4](https://github.com/DavidLMS/nan-harness/issues/4)
- Validation date: 2026-09-03
- Host version: macOS 26.5.2 (build 25F84), arm64
- Zed version: 1.18.0+stable.351
- Zed build SHA: `49448af`
- Primary live validation duration: 18 minutes
- Interrupted-recovery validation duration: 15 seconds
- Applied settings SHA-256: `0bbb0d8621c2402a3eac6bcfe4d7c951aaeb61a63deab91d4ca80faf3b824734`
- Session receipt SHA-256: `b07ab84997e0fdcfdf181fa1e108c4f90331a14a83fc1dfbf730fb6ae444bf1d`
- Selective-restored settings SHA-256: `df03d1aca08f5d912cc8e793d065dcf52f51ab4138002829e918c5fd9c566bab`
- Tool-created output SHA-256: `358ff78b8d5075f900cea0432fcf24e3b09a4ecc8bdcc7bd2c97af14722c646c`
- Original settings absent before validation: true
- Original settings absent after cleanup: true

## Compatibility registration

| Platform | Transport | Evidence | Version bounds |
| --- | --- | --- | --- |
| macOS | Chat Completions gateway | `live-verified` | 1.18.0 only |
| Linux | Chat Completions gateway | `contract-only` | None |
| Windows | Chat Completions gateway | `contract-only` | None |

## Deterministic evidence

| Contract | Result |
| --- | --- |
| `zed` and `zed-desktop` command, help, completions, inert dry-run | PASS |
| Experimental registry remains separate; stable `HarnessKind::ALL` remains 15 | PASS |
| macOS, Linux, and Windows path and executable discovery simulation | PASS |
| Live catalog projection, image flags, and reasoning-policy mapping | PASS |
| Complete Zed 1.18 capability object generation | PASS |
| JSONC comments, trailing commas, invalid roots, and incompatible objects | PASS |
| Foreign `openai_compatible.nan` provider rejection | PASS |
| Exact-byte and selective restoration, including unrelated edits | PASS |
| Managed-value conflicts, invalid receipts, symlinks, and idempotent restore | PASS |
| Already-open/startup races, failed start, signal, close, relaunch, gateway exit | PASS |
| Session token isolation and secret-free plan, receipt, doctor, and telemetry | PASS |
| Pending Zed recovery blocks uninstall | PASS |
| No Zed entry in stable config, pinned conformance, canaries, or release feed | PASS |

## Quality gates

| Command | Result |
| --- | --- |
| `cargo test --locked -p nan-harness-core -p nan-harness-runtime -p nan-harness-cli -p nan-harness-telemetry --all-features` | PASS |
| `cargo fmt --check` | PASS |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --locked --workspace --all-features` | PASS |
| `cargo doc --locked --workspace --all-features --no-deps` | PASS |
| `cargo check-all` | PASS |

## Live macOS evidence

| Required observation | Result |
| --- | --- |
| Official stable Zed version identified | PASS |
| Current-account catalog appears under provider `nan` | PASS |
| Selected model reaches NaN | PASS |
| Streaming chat completes | PASS |
| Read and write tools complete | PASS |
| Cancellation stops generation and a subsequent request completes | PASS |
| Image input matches the advertised capability | PASS |
| Real provider credential remains outside Zed and recovery artifacts | PASS |
| Normal exit restores managed settings | PASS |
| Selective restore preserves unrelated settings | PASS |
| Interrupted-session `--restore` succeeds | PASS |
| Repeated `--restore` is idempotent | PASS |
| Relaunch supervision prevents premature restoration | PASS |
| Gateway usage summary appears | PASS |
| Prompts, responses, tool arguments, workspace contents, and credentials retained in this report | false |

## Limitations

- Live compatibility is limited to Zed 1.18.0 on macOS arm64.
- Linux and Windows have deterministic contract evidence only.
- The integration supports managed launch only and does not isolate or manage
  Zed history, profiles, search, MCP, edit predictions, or other application
  state outside the owned settings fields.
- Zed installation and updates remain explicit operator actions.
- The integration remains outside stable `HarnessKind`, pinned conformance,
  canaries, and the stable release gate.

## Verdict

**GO.** The experimental desktop integration satisfies the live protocol,
capability, credential-boundary, restoration, supervision, and recovery gates
for the exact macOS version above. It is a merge candidate as an experimental
surface only; it is not a stable harness registration.
