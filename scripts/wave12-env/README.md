# Wave12 disposable macOS environment experiment (prepared, reviewable)

This directory is the LOCAL, reviewable preparation for ONE future disposable
GitHub-hosted macOS ARM64 run that compares the deterministic checker outcome
under a **baseline** condition versus a **dock-hidden** condition. It is not
authorized to run here and must never run on a shared Mac. Real environment
drivers remain quarantined unless an explicit opt-in is set on both condition
steps of the reviewed disposable GitHub-hosted runner.

## What is measured and what is not

The experiment configures the ephemeral runner's Dock auto-hide user preference
(`com.apple.dock autohide`) and then records the closed guard/occlusion outcome
under each condition. The preference change **is not** evidence that the Dock
stopped overlapping; the guard, focus, and same-process ownership rejections are
unchanged, and there is no Dock exemption. Only the recorded closed
report/occlusion facts are compared. A blocked/failed condition outcome is
recorded honestly rather than made to pass.

## Files

- `run-experiment.sh` — orchestrator for one condition (`baseline` or
  `dock-hidden`). Selects a driver backend, validates host disposability and
  identity, saves/sets/restores the Dock auto-hide preference (dock-hidden
  only) with a bounded state-observed readiness wait, runs the deterministic
  probe, validates the canonical report and closed occlusion diagnostic, and
  records closed evidence from its EXIT handler.
- `fixtures/bin/` — the fake backend: marked fake drivers for `defaults`,
  `killall`, `pgrep`, `uname`, the runner identity and `nanh-desktop-check`.
  They never touch a real preference, window, or process.
- `tests/run-synthetic-tests.sh` — fake-backend suite, including negative
  routing tests.
- `stage-artifacts.sh` / `stage_artifacts.py` — accept the condition directory,
  a new staging directory and the source-built checker. They validate closed and
  mutually consistent metadata, snapshot reports privately, compare both
  recorded SHA-256 values, and run the checker's read-only validators on those
  exact snapshots. Status files alone authorize nothing. Publication is atomic
  after all checks pass; a missing condition produces an empty allowlist. The
  workflow summary also reads staged metadata only.
- `tests/test_staging.py` — isolated publication regressions using a mocked
  validator; this suite never invokes environment drivers or a native checker.
- `.github/workflows/desktop-check-macos-wave12.yml` — the prepared, single-job
  disposable run. Not run, pushed, or dispatched here.
- `scripts/desktop-check-macos-wave12-contract.sh` — static contract: YAML and
  run-block syntax, whitespace, staged-only uploads, static fake-backend
  isolation, and workflow quarantine.

## Fake backend isolation

- **Explicit selection.** Fake mode exists only with `--fake-backend DIR`. No
  environment variable, runner variable or PATH content selects it. Without it,
  the orchestrator is in real mode, which refuses with exit 78 before reading
  any host fact, and `real_driver` executes nothing.
- **Marked drivers.** Every fake driver must be a regular, non-symlinked,
  executable file whose first two lines are `#!/usr/bin/env bash` and
  `# wave12-fake-driver`. Real binaries lack the marker, so a backend pointed at
  `/usr/bin` or a build output is refused, not executed. Drivers are checked at
  startup and again before each call.
- **No host name resolution.** Fake mode refuses to start if any host
  environment command (`defaults`, `killall`, `pkill`, `pgrep`, `uname`,
  `sw_vers`, `osascript`, `open`, `launchctl`, `screencapture`,
  `nanh-desktop-check`, `nan-harness`) resolves via PATH, an alias or an
  imported shell function. A typo or a missing `driver` prefix therefore fails
  instead of reaching the host.
- **Fake host facts.** In fake mode, disposability comes from the fake
  `runner-environment` driver. `GITHUB_ACTIONS` and `RUNNER_ENVIRONMENT` are
  ignored, so tests never spoof runner variables, and the workflow must not
  override them either.
- **Fake checker only.** In fake mode `--checker` must be the backend's own
  fake checker.

The suite runs every orchestrator and staging process under `env -i` with PATH
set to a private toolbox (`bash`, `mkdir`, `sleep`, `shasum`, `python3`).
Negative tests place logging sentinels named like host commands on PATH, in a
backend, as an imported function and as the checker. The suite ends by proving
that no sentinel ever executed. The contract script checks the same properties
statically.

## Restoration and evidence

Only a readable prior value (`absent`, `true`, `false`) is ever mutated. An
`unreadable` or `invalid` prior value is recorded and the condition refuses
(exit 3) without writing or deleting. After a mutation the EXIT handler always
restores the exact prior value (delete for `absent`) and verifies it by reading
it back, including after failures and `HUP`/`INT`/`TERM`. A failed or
unverified restoration is recorded as `restore_status=failed` and exits 5,
overriding every other outcome. `evidence.txt` is written once, after
restoration, with `prior_state`, `docks_changed`, `restore_status` and
`probe_exit` (`not-run` when a failure precedes the probe). Staging refuses
metadata whose closed facts contradict each other.

Exit codes: 0 completed (the probe may be blocked), 1 non-disposable host or
invalid identity argument, 2 not Darwin / baseline not ready / usage, 3 not arm64,
unknown prior state, state confirmation failure or evidence reuse, 4 missing
identity or an absent/invalid canonical report, 5 restoration failed, 78 driver
quarantine or refused fake backend.

## Enabling real drivers (reviewed disposable-run opt-in)

Running the experiment requires the reviewed workflow opt-in that (1) replaces
the `select_backend` refusal and `real_driver` body with absolute-path drivers
restricted to the exact argument shapes above, gated by an explicit opt-in
variable plus `GITHUB_ACTIONS=true` and `RUNNER_ENVIRONMENT=github-hosted`;
(2) sets that opt-in only on the two condition steps; and (3) updates contract
sections 8 and 9 to allow exactly that. See the DH-13 delivery report for the
exact patch and the one-run protocol.

## Prepared installation lifecycle (both conditions reuse one prepared set)

Both conditions run against the SAME `--prepared` receipt produced by the checker
`prepare` step. This is a justified owned-lifecycle design, based on reading the
checker's actual implementation (`crates/nan-harness-desktop-check/src/runner.rs`
and `runner/prepared.rs`):

* `prepare` writes a private receipt whose `owner` is a Journal that owns the
  prepared installation.
* `run --prepared <receipt>` calls `prepared::load`, which returns that prepared
  inventory and the owner Journal. The run then creates its OWN Journal
  (`Journal::create`) and discards the prepared owner (`_prepared_owner`).
* `run_app` reuses an existing installation when `found` is `Ok(Some(...))`
  (which is the prepared case) without calling `journal.reserve`, so the prepared
  installation is NOT a resource of the run's Journal.
* `finish_report` runs `journal.cleanup(args.ephemeral)` on the RUN's Journal
  only, removing resources the run itself reserved — not the prepared install.
  Even with default `--ephemeral` (false), the prepared install is untouched.

So the first condition's run never uninstalls the prepared installation, and the
second condition reuses the same prepared install validly. `prepare` opens no GUI
and makes no provider call; the run never reinstalls or mutates the prepared
installation. This is documented as a source-bound justification rather than an
assumption; the fake suite cannot reproduce the Rust journal semantics and
therefore does NOT claim to prove the native lifecycle — only the orchestrator
wiring.

## Running the tests locally

```sh
python3 -B scripts/wave12-env/tests/test_staging.py
bash scripts/desktop-check-macos-wave12-contract.sh
bash scripts/wave12-env/tests/run-synthetic-tests.sh
```

The fake suite runs the orchestrator only through the fake backend. It covers
negative routing, prior absent/true/false/unreadable/invalid state, restoration
verification and failure reporting, interruption, bounded readiness,
per-condition isolation, schema rejection and staged-only publication. It
establishes script wiring only and does **not** establish any native desktop
qualification.
