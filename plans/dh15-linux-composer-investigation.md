# Linux ChatGPT Desktop composer investigation

## Proven result

Baseline `f4456c61ffd902a57143eb0c499fbde466d7974f` reported
`composer-input/action-unsupported` for all three deterministic probes without
identifying the operation. Closed instrumentation in `gui.rs:36-76` and
`probe.rs:220-245` proved the initial failure was the accessible login-control
count (`accessible-login-check/action-unsupported`). This occurred before
composer selection, focus, text entry, visual fallback or send.

The Linux ChatGPT launcher now passes
`--force-renderer-accessibility` (`crates/nan-harness-cli/src/commands/chatgpt_desktop/process.rs:48-75`).
The prior checker-only `ACCESSIBILITY_ENABLED=1` attempt was removed because
run `34686416348` showed it did not change the native result. The argument
correction is demonstrated: run `34688523156` reached
`input-submitted` in all three probes with `inputMode: accessibility-and-keyboard`.

The composer objective is not fully qualified. The same run still failed later
with public `action-unsupported`; response-boundary instrumentation then showed
`verify-response-guard/action-unsupported` in two probes. A final split run
`34689167431` showed two probes at that same response guard and one probe at
`verify-input/timeout` (`input-mismatch`) before input submission. No response,
tool or recovery probe reached a verified result.

## Safety and regression correction

`native/window.rs:16-36,96-137` now keeps closed guard failures distinct:
identity missing, bounds changed, foreground changed, same-process window,
off-display and occluded. Existing `Reason` mapping and all safety checks are
unchanged. `gui/visual.rs:138-178` performs a pre-input reacquisition only when
the same window id and pid are present, the candidate is foreground, stable for
two snapshots, on a display, and unoccluded. It never repeats input after an
uncertain side effect. The confirmed Linux `window-bounds-changed` result from
run `34688255780` was the basis for this correction.

The facts writer regression is fixed in `probe.rs:220-245,282-299`: it writes
`composer-diagnostic.json` only after `prepare_launch_wrapper` has successfully
created and validated the private facts directory. A rejected/reused facts
directory is never modified; a legitimate owned run that fails early still
gets an empty diagnostic. The regression test
`missing_or_mismatched_bindings_refuse_before_any_process_runs` passes.

## Call chain and boundaries

The deterministic flow is `probe.rs:482-563` -> `Gui::submit` (`gui.rs:263-386`)
-> `Gui::input` (`gui.rs:414-493`). Accessibility selection checks login
controls, named composer selectors and editable controls. Only
`SelectorNotMatched` enters `Visual::submit` (`gui/visual.rs:259-334`), which
uses OCR click/select-all/type/verify/send. The accessibility path sets the
value, optionally focuses and uses `input_sim`/Ctrl+A/`type_text`, verifies the
value, then sends. `Gui::wait_text` (`gui.rs:515-574`) now attributes response
guard, response accessibility count and response visual polling separately.

`map_error` (`gui.rs:666-684`) intentionally maps xa11y
`AccessibilityNotEnabled`, `ActionNotSupported`, invalid selector/config and
other unsupported variants to public `Reason::ActionUnsupported`. The pinned
xa11y Linux source documents Chromium/Electron renderer accessibility remedies
at `xa11y-linux-0.13.0/src/atspi.rs:204-239`, but the original broad category
did not prove which backend variant occurred. The native helper can also map
process spawn, pipe, timeout, image-write and non-zero-exit failures to the
same public reason (`native/process.rs:14-73`). The final
`verify-response-guard/action-unsupported` therefore identifies the checker
operation boundary, not a specific helper failure or app accessibility error.

## Evidence

- `34685323422` / `7e5152249cc696c3b9083bee02bafea0a1af501e`: all three
  `locate-accessible/action-unsupported`.
- `34685698219` / `217e5b75c201012caef07a819731aa1b949db089`: all three
  `accessible-login-check/action-unsupported`.
- `34686416348` / `a0cf632532b3b2ddd62953f3f8585c2a787a96fd`: same result;
  environment-only candidate rejected.
- `34686838473` / `194cc56fd83cd444a6a03d11048d017c1e83e175`: launcher argument
  correction reached `guard/window-changed`.
- `34688255780` / `f1a30ed73889222a8ef311a2fcefa970342cd8c5`: exact category
  `guard/window-bounds-changed`; cleanup passed.
- `34688523156` / `10cb2997a93cce7bede2f9df80d8036b462886bb`: all three reached
  `input-submitted`; all later failed `action-unsupported`; cleanup passed.
- `34688843313` / `390724b1de47d79d33531f3a3e9f068d425710f8`: all three
  `verify-response/action-unsupported`; cleanup passed.
- `34689167431` / `fa7be1fb5d0d99d3ebd58c237be4ab5ae52da7ec`: two
  `verify-response-guard/action-unsupported`, one `verify-input/timeout`;
  cleanup passed; live skipped for `missing-key`.

Final closed artifact digests for `34689167431` are
`d7571e596eb3f0cf73f844729890962faff5688e5b27b456eb90c7b12c87f6b4`,
`98b3d7da16d38b4fed2bdc139efe03469beac83c2c6c295a4d189d135c6ce0bb`, and
`43b16f08b05b4cffcffa96a565d333007bb07cd1bd7d7579cd56115e3e2a106e`.
Its setup digest is
`8e22fef914bd6f73d2f98299aecc364222c8a99c95f09c2f1fab4cf127376bfa`:
official system package `26.908.40834`, official AppArmor profile loaded,
namespace restriction `1` before and after. No sandbox policy was weakened.

## Verification and remaining boundary

Passed: focused clippy with `-D warnings`, the facts regression test, closed
guard-category test, composer privacy test, `bash scripts/test-chatgpt-wave12-stage.sh`
(`11` tests), formatting and diff checks. The prior final-tree `cargo
check-all` had `123` passing tests and one restricted-host synthetic wrapper
assertion failure; the earlier full desktop-check library run had 113 passing
and 11 restricted-host failures. The root-reported facts failure was not
environmental: it was an entry-count regression caused by writing into a
rejected directory, and it now passes locally.

The smallest privacy-preserving next observation, if further authority is
granted, is a closed native-helper boundary category distinguishing
`windows()` helper spawn/pipe/timeout/non-zero-exit from the guard invariant;
it must contain only a fixed enum and preserve all guards. No safe product fix
for `verify-response-guard/action-unsupported` is proven by the current
artifacts. Composer input/submission is demonstrated reachable, but response,
tool and recovery qualification remains unresolved. No raw app output,
selectors, coordinates, pixels, prompts or credentials were published.
