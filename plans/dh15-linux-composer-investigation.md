# Linux ChatGPT Desktop composer investigation

## Result

Baseline `f4456c61ffd902a57143eb0c499fbde466d7974f` failed all three
deterministic probes at `composer-input/action-unsupported`, but did not say
which operation failed. Closed operation evidence identified the cause:
xa11y's Linux AT-SPI backend rejected the pre-composer login-control count
because the Chromium/Electron renderer accessibility bridge was disabled.

The correction is implemented in `crates/nan-harness-cli/src/commands/chatgpt_desktop/process.rs:48-75`:
Linux ChatGPT launches now pass `--force-renderer-accessibility`. This is
scoped to ChatGPT's native Linux launcher; it does not change the package,
AppArmor profile, namespace policy, foreground/window guards, or public report
schema. The ineffective checker-only `ACCESSIBILITY_ENABLED=1` workaround was
removed from `crates/nan-harness-desktop-check/src/probe.rs`.

The accessibility cause is fixed and natively reproduced as fixed, but the
full composer is not qualified: the corrected run advanced to the first
post-selection window guard and stopped with closed `guard/window-changed`.
No input was repeated and no guard was weakened. Response, tool and recovery
probes therefore remain unverified.

## Call chain and reason mapping

The deterministic path is `probe.rs:417-433` -> `probe.rs:470-474` ->
`Gui::submit` (`gui.rs:253-349`) -> `Gui::input` (`gui.rs:391-466`). The
accessibility path first checks login controls, then counts named composer
selectors and editable controls. Only `SelectorNotMatched` falls back to
`Visual::submit` (`gui/visual.rs:216-275`), which finds the composer by OCR,
clicks, selects all, types, verifies, and presses Enter. Otherwise `Gui::submit`
sets the value, optionally focuses and uses `input_sim`/Ctrl+A/`type_text`,
verifies the value, and sends (`gui.rs:253-349`).

The closed diagnostic attributes discovery, focus, value, keyboard, guard,
verification and send separately through `ComposerOperation` and
`ComposerErrorCategory` (`gui.rs:31-110`, `gui.rs:691-729`). `map_error`
still intentionally maps xa11y's `AccessibilityNotEnabled`,
`ActionNotSupported`, `TextValueNotSupported`, invalid selector/config and
unsupported variants to public `Reason::ActionUnsupported`
(`gui.rs:648-679`); raw backend text is not exposed.

The pinned `xa11y-linux 0.13.0` implementation detects an empty Chromium tree
and reports `AccessibilityNotEnabled`, recommending
`--force-renderer-accessibility` or `ACCESSIBILITY_ENABLED=1`
(`~/.cargo/registry/src/.../xa11y-linux-0.13.0/src/atspi.rs:204-239`). Its
action and editable-text paths can also produce `ActionNotSupported`
(`atspi.rs:999-1010,1660-1687,1887-1965`). The native helper can independently
map bounded process, pipe, timeout, image-write and non-zero-exit failures to
`ActionUnsupported` (`native/process.rs:14-73`).

## Native evidence

- Instrumented selection run `34685323422`, commit
  `7e5152249cc696c3b9083bee02bafea0a1af501e`: all three probes recorded
  `locate-accessible/action-unsupported`.
- Split-selection run `34685698219`, commit
  `217e5b75c201012caef07a819731aa1b949db089`: all three recorded exactly
  `accessible-login-check/action-unsupported`.
- Checker-only environment candidate `34686416348`, commit
  `a0cf632532b3b2ddd62953f3f8585c2a787a96fd`: all three still recorded
  `accessible-login-check/action-unsupported`; this candidate is not a fix.
- Native correction run `34686838473`, commit
  `194cc56fd83cd444a6a03d11048d017c1e83e175`: the one collected deterministic
  result recorded `guard/window-changed` at `composer-input`, after
  `launched`; the other deterministic cases were not run after that blocked
  result. Cleanup passed. Live remained skipped for `missing-key`.

The final run's closed startup fact digest is
`f21de907a5c0cac0cede14512be587a012aab2388029038c9b67b64ccc4bf74d`.
Its setup digest is
`8e22fef914bd6f73d2f98299aecc364222c8a99c95f09c2f1fab4cf127376bfa` and
records the official system package, version `26.908.40834`, loaded official
AppArmor profile, and user-namespace restriction `1` before and after. The
startup wrapper's cancelled observations remain bounded evidence only; empty
sandbox signatures do not certify absence. The green Actions status certifies
collection/staging, not app success.

## Verification and remaining work

Passed locally: `cargo fmt --all`, `git diff --check`, focused clippy with
`-D warnings` for the affected crates, the closed diagnostic privacy test,
the Linux launcher-argument test, `bash scripts/test-chatgpt-wave12.sh`
(`306` checks), and `bash scripts/test-chatgpt-wave12-stage.sh` (`11` tests).
The earlier full desktop-check library run had `113` passing tests and `11`
sandbox-denied failures (`Operation not permitted`) in unrelated synthetic
process/provider cases. The first final-tree `cargo check-all` also exposed
and then the refactor fixed two `too_many_lines` lints; the affected-crate
clippy gate now passes. The final `cargo check-all` reached `123` passing
tests and one environment-dependent synthetic wrapper failure
(`missing_or_mismatched_bindings_refuse_before_any_process_runs`, process
count assertion under the restricted host); it did not report a compile or
clippy failure. No native app was launched on the user's machine.

The smallest next correction is not established by this evidence. A future
authorized run must observe the closed native window identity/bounds state at
the failed guard (without names, coordinates, pixels or app output), or add a
synthetic transition test that proves a safe re-acquisition point. It must
preserve the ownership/foreground guard and must not repeat input after an
uncertain side effect. Until then, do not claim composer submission,
response, tool or recovery qualification.

No credentials, raw diagnostic envelopes or raw application output were
published. The validation branch contains the corrective candidate; this
report is the local English handoff.
