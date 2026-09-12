# Linux ChatGPT Desktop composer investigation

## Scope and evidence

- Repository: `DavidLMS/desktop-integration-wave8`, HEAD
  `f4456c61ffd902a57143eb0c499fbde466d7974f`.
- Observed run: GitHub Actions run `34682387160`, one Ubuntu 24.04 x64 job.
  ChatGPT Desktop `26.908.40834`, bundled runtime `0.154.0-alpha.6.2`.
- Durable local evidence: `plans/dh14-linux-official-sandbox-evidence/desktop-chatgpt-wave12/startup-facts-{0,1,2}.json` and
  `plans/dh14-linux-official-sandbox-evidence/desktop-chatgpt-wave12-sandbox-setup/sandbox-setup.json`.
- Each of the three deterministic probes recorded `launched`, then failed after
  24041, 23040 and 23086 ms with `reason: action-unsupported` and
  `guiStage: composer-input`. Cleanup passed. Live was skipped for the missing
  key. The official system package and AppArmor profile were loaded; the user
  namespace restriction was `1` both before and after. This report does not
  revisit or weaken that startup correction.
- Startup wrapper facts are cancelled observations with forwarded stops. Their
  zero sandbox signatures are therefore not an absence proof. The positive
  `launched` steps are the useful startup evidence.

No diagnostic envelope, raw application output, prompt, credential or native
GUI session was inspected or published.

## Call chain

The deterministic probe calls `submit` at
`crates/nan-harness-desktop-check/src/probe.rs:417-433`; `submit` records only
the closed GUI stage from `GuiFailure` and then discards the backend error at
`probe.rs:470-474`. The ChatGPT path is:

1. `Gui::submit` (`src/gui.rs:200-281`) creates the input-stage failure and
   calls `input` (`src/gui.rs:284-330`).
2. `input` requires an app-bound locator, rejects visible login controls,
   tries ChatGPT labels `Message`, `Ask anything` and `Ask for follow-up
   changes`, then falls back to exactly one editable `text_area` or
   `text_field` (`src/gui.rs:284-329`).
3. A visual fallback is used only when accessibility selection returns
   `SelectorNotMatched` (`src/gui.rs:209-215`). It OCR-finds the composer,
   clicks it, selects all, types, OCR-verifies the prompt, and presses Enter
   (`src/gui/visual.rs:216-264`).
4. On the accessibility path, `set_value` is attempted first. Only
   `TextValueNotSupported` or `ActionNotSupported` enters the fallback that
   focuses, waits for focus, creates `input_sim`, selects all with Ctrl+A on
   Linux, and calls `type_text` (`src/gui.rs:217-247`). The value is then
   verified by `wait_until` and a guard before send (`src/gui.rs:250-257`).
5. Send is a later stage: a uniquely located Send button is pressed, otherwise
   the field is focused and Enter is simulated (`src/gui.rs:258-280`). The
   observed `composer-input` stage rules out only a reported send-stage error;
   it does not identify the earlier input operation.

## Why `action-unsupported` is non-specific

`map_error` maps all of the following xa11y variants to the same public reason
`Reason::ActionUnsupported` (`src/gui.rs:510-527`):
`TextValueNotSupported`, `ActionNotSupported`, `InvalidActionData`,
`Unsupported`, `InvalidSelector`, `InvalidConfig`, and
`AccessibilityNotEnabled`. The report serializes that closed enum as
`action-unsupported` and serializes `ComposerInput` as `composer-input`
(`src/report.rs:63-118` and `src/report.rs:476-497`). No operation name or
backend error survives this boundary.

The possible input-stage sources include:

- accessibility discovery/count/wait: selector parsing, count, visibility,
  login-control detection, or the editable fallback (`src/gui.rs:285-329`);
- an accessibility text operation: `set_value`, then `focus` or
  `wait_focused` (`src/gui.rs:217-227`);
- Linux input construction or input events: `input_sim`, Ctrl+A, or
  `type_text` (`src/gui.rs:227-245`);
- the foreground/window guard before or between those actions
  (`src/gui.rs:217-240`, `src/gui/visual.rs:107-118`); or
- the visual fallback's screenshot/native helper, click, Ctrl+A, or typing
  path (`src/gui/visual.rs:225-243`).

The native helper itself can also produce this reason for process spawn,
pipe/thread, timeout/non-zero-exit, or image-write failures
(`src/native/process.rs:14-73,76-85`). It is private and bounded: the helper
receives pixels through stdin and returns bounded TSV (`native/main.cpp:31-56`),
with extraction and OCR isolated in a private temporary directory
(`src/native.rs:23-83`; `native/README.md:1-29`).

The pinned Linux xa11y implementation is `0.13.0`, locked to the crates.io
checksums in `Cargo.lock:3883-3925`, and the checker disables default features
in `crates/nan-harness-desktop-check/Cargo.toml:36`. Its Linux AT-SPI backend
returns `ActionNotSupported` when a requested action index is absent or for
unsupported roles (`xa11y-linux-0.13.0/src/atspi.rs:999-1010,1660-1687`), and
implements text setting/typing through AT-SPI `EditableText`
(`atspi.rs:1887-1965`). Its Linux input facade chooses X11 when `DISPLAY` is
set and otherwise uinput/Wayland (`xa11y-linux-0.13.0/src/input.rs:1-19`);
X11 `type_text` can return `Unsupported` when the active keymap lacks a
character (`input.rs:293-323`). These are credible backend/app boundaries,
but the current report cannot tell which one occurred.

## Diagnosis, implementation and native verification

The first instrumented candidate was commit `7e5152249cc696c3b9083bee02bafea0a1af501e8`,
run `34685323422`. Its three probes still failed at
`composer-input/action-unsupported`, but the closed observation was
`locate-accessible/action-unsupported`. A second candidate split that boundary
into the actual accessibility queries. Commit
`217e5b75c201012caef07a819731aa1b949db089`, run `34685698219`, recorded the
same result in all three probes:

```json
{"operation":"accessible-login-check","errorCategory":"action-unsupported"}
```

Both runs used the official package, exact `/usr/lib/chatgpt/ChatGPT`, loaded
official AppArmor profile and unchanged global user-namespace restriction `1`.
Both completed cleanup successfully; both skipped live because no provider key
was present. The startup wrapper remained a cancelled observation with zero
sandbox signatures, so it is not a sandbox absence proof. The run-level green
status means collection/staging completed, not app compatibility.

The operation is now exact: the failure occurs on the pre-composer login-button
accessibility count, before named-composer selection, focus, text input, visual
fallback or send. The public reason remains broad because `map_error` closes
xa11y `AccessibilityNotEnabled`, `ActionNotSupported`, invalid selector/config
and unsupported-input variants into `Reason::ActionUnsupported` at
`src/gui.rs:510-527`. The pinned xa11y Linux source explicitly detects an
empty Chromium/Electron tree and returns `AccessibilityNotEnabled` with the
supported remedies `--force-renderer-accessibility` or
`ACCESSIBILITY_ENABLED=1` (`xa11y-linux-0.13.0/src/atspi.rs:204-239`). This
matches the observed operation and is the demonstrated cause: the official
ChatGPT renderer accessibility bridge was not enabled for the checker launch.

Implemented correction: `src/probe.rs` now sets `ACCESSIBILITY_ENABLED=1`
only when the disposable checker launches `chatgpt-desktop` on Linux. It is
not set for other harnesses, does not modify the installed package or AppArmor,
does not weaken foreground/window ownership guards, and does not alter public
compatibility report schema. The closed diagnostic remains private to the
experimental wave12 envelope; unknown operations, categories, extra fields and
raw data are rejected by Rust and Python tests.

Local verification before corrective native run:

- `cargo check --locked -p nan-harness-desktop-check --all-features`: passed.
- Rust closed-diagnostic/privacy and GUI tests: passed; the Linux-only
  environment assertion is compiled here but runs only on Linux.
- `bash scripts/test-chatgpt-wave12.sh`: 306 checks passed.
- `bash scripts/test-chatgpt-wave12-stage.sh`: 11 tests passed.
- `cargo fmt --all` and `git diff --check`: passed.

The corrective candidate is not yet natively qualified at this report point.
The required next evidence is one bounded Linux run on the correction commit:
all three deterministic probes must show `input-submitted` and, if the app
continues, response/tool/error-recovery steps; cleanup and the preserved setup
must also pass. If a later boundary fails, that result must remain separate
from the corrected accessibility cause.

Closed artifact digests for the two diagnostic runs are retained in the private
temporary review area. Run `34685323422` files:
`39ffda86705ce4ba053561dfefabd4415ed9bc6b7caf7f71b569686653aafec7`,
`135459b87d1977eaa584c734ee255e5d73f24d512e5fe5ab5a059288377e810a`,
`048bd56a2fa9ae1164603e71095420cdeeb4514d73803a563828837ee8767887`.
Run `34685698219` files:
`60f189b4bdd9986a1120a2f99bc97772ee1cace0aa2e3cd12acda6a8d91ef32e`,
`fd38a1340c3fe02bf88d31c28ae07f4425b0cfa4115f3c7bea89a9588f2d5556`,
`be81cc02781a4019993e9d076f6353a239a08c11658d91f77b117b4421a81e7f`.
