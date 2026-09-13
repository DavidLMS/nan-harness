# Canary migration checkpoint — 2026-09-14

This is a durable coordination checkpoint, not a qualification, release, or
publication record. It preserves the Desktop/GUI observations requested before
the CLI migration work resumes. GitHub pushes, hosted CI, live keys, and
cutover remain blocked pending explicit user confirmation.

## Provenance and limits

- The shared worktree is `desktop-claude-window-liveness-wave31`; the current
  branch is retained as-is. No branch switch, reset, checkout, installation,
  GUI action, provider call, push, or CI dispatch was performed for this
  checkpoint.
- Historical Desktop source inspection included immutable commit
  `698f9cc386851c55f9f89e47d5d88d87021c5bae` (`fix(desktop-check): complete
  Windows acquisition failure tuple`). The requested
  `plans/desktop-actions-completion.md` was not present in the inspected
  checkout or reachable history, so no claim is made from that missing plan.
- Desktop observations below came from closed JSON only. The temporary
  directories are evidence pointers, not durable artifacts and may expire:
  `/private/tmp/nan-wave-run34779342815` and
  `/private/tmp/nan-wave-native-34777094717`. Raw logs, prompts, paths,
  credentials, screenshots, titles, and process identifiers are intentionally
  omitted.
- These observations identify what the diagnostics recorded; they do not prove
  application compatibility, an activation cause, GUI readiness, or a fix.
  Absence of an event is not treated as evidence of absence.

## Desktop observations retained

The older safe evidence set under `nan-wave-run34779342815` recorded these
bounded outcomes:

- ChatGPT Desktop prepare emitted a version-resource observation with a spawn
  failure (`osError` 5) and an installation-unreadable category.
- Claude Desktop reached window acquisition, then recorded cleanup failure
  after a tool mismatch.
- Hermes Desktop recorded one bounded input-accessibility timeout and two
  native-helper foreground-mismatch/action-unsupported results.
- Pen Desktop recorded one selector-not-matched result for a missing composer
  anchor and two native-helper foreground-mismatch/action-unsupported results.

The native evidence set is run `34777094717`, source `591b799` as supplied for
that experiment, under `nan-wave-native-34777094717`:

- macOS ChatGPT acquired a window but recorded `focus-changed` with a
  foreground-process-different guard observation. This does not establish why
  focus changed or that refocus is safe.
- macOS Claude recorded matching-process-present with no visible-window
  inventory and `desktop-unavailable` observations. Readiness fields were
  unavailable; this does not prove activation, hidden state, or GUI readiness.
- macOS Hermes did not reach probes; installation recorded a bounded pip
  nonzero exit (`return_code` 1, pip failure hint `other`, Python 3.14 and pip
  26.2). The result does not identify dependency, interpreter, network, or
  build cause.
- macOS Pen recorded two selector-not-matched observations and one
  desktop-unavailable observation. These are separate bounded outcomes, not
  proof of a common cause.

The related immutable diagnostic pointers are preserved for future review:

- `ebb03ff30686cff242f281b63dd0e8723d6fbd97` — synthetic ChatGPT stdout
  read-failure fixture.
- `2e6d5d736043bfe0b6f47564bcae020bcc07afb5` — Linux owned-window diagnosis.
- `fb93664e2d605b8170306424c74e9a09ab029575` — enforcement of owned-window
  facts.

Those commits and the temporary evidence remain historical inputs. The paused
Desktop work was not resumed, and no native result is promoted to a passing
qualification.

## CLI checkpoint

The deterministic CLI checkpoint used exact source
`abb2ad8d5a86ea5cd2001457e5e4c76e0870f4d1` in run `34783137385`: 25 of 28
non-Codex cells passed. The two prior Codex cells in run `34782781630` passed,
giving 27 of 30 deterministic cells across the two runs. The remaining
failures were OpenClaw on Linux and macOS, plus Goose on macOS resolution;
they are not silently counted as Desktop results.

The safe CLI reports and resolver diagnostics remain subject to their own
review. This checkpoint does not claim the OpenClaw or Goose failures are
fixed, nor that hosted reruns are authorized.

## Integration state and next boundary

Desktop diagnostic work, CLI migration, live parity, release cutover,
publication, and main-branch merge are all incomplete. The current evidence
supports preserving the following boundaries:

1. Keep Desktop diagnostics advisory and failure-only; do not infer readiness,
   activation, ownership, or compatibility from readability or candidate
   absence alone.
2. Keep the CLI deterministic route tied to the exact source SHA and closed
   per-cell reports; do not dispatch hosted or live work from this checkpoint.
3. Before any native or hosted continuation, obtain explicit user confirmation,
   review the immutable source/report pair, and record a new evidence pointer
   rather than overwriting this historical checkpoint.
