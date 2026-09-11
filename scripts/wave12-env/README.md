# Wave12 disposable macOS environment experiment (prepared, reviewable)

This directory is the LOCAL, reviewable preparation for ONE future disposable
GitHub-hosted macOS ARM64 run that compares the deterministic checker outcome
under a **baseline** condition versus a **dock-hidden** condition. It is not
authorized to run here and must never run on a shared Mac. It is exercised only
against the fixture command shims by the synthetic test suite.

## What is measured and what is not

The experiment configures the ephemeral runner's Dock auto-hide user preference
(`com.apple.dock autohide`) and then records the closed guard/occlusion outcome
under each condition. The preference change **is not** evidence that the Dock
stopped overlapping; the guard, focus, and same-process ownership rejections are
unchanged. Only the recorded closed report/occlusion facts are compared. A
blocked/failed condition outcome is recorded honestly rather than made to pass.

## Files

- `run-experiment.sh` — orchestrator for one condition (`baseline` or
  `dock-hidden`). Validates host disposability and identity, saves/sets/restores
  the Dock auto-hide preference (dock-hidden only) with a bounded state-observed
  readiness wait, runs the deterministic probe, validates the canonical report
  and closed occlusion diagnostic, and writes per-condition artifacts. Fails
  closed on a non-disposable/unsupported host and on missing/invalid helper or
  report identity.
- `fixtures/bin/` — command shims (`defaults`, `killall`, `pgrep`, `uname`) and a
  fixture `nanh-desktop-check`. These shadow only the narrow system driver set;
  they never touch a real macOS preference, window, or process.
- `tests/run-synthetic-tests.sh` — executable synthetic test suite (uses the
  fixture shims only).
- `stage-artifacts.sh` / `stage_artifacts.py` — accept the condition directory,
  a new staging directory and the source-built checker. They validate closed
  metadata, snapshot reports privately, compare both recorded SHA-256 values,
  and run the checker's read-only validators on those exact snapshots. Status
  files alone authorize nothing. Publication is atomic after all checks pass;
  a missing condition produces an empty allowlist. The workflow summary also
  reads staged metadata only.
- `tests/test_staging.py` — isolated publication regressions using a mocked
  validator; this suite never invokes environment drivers or a native checker.
- `.github/workflows/desktop-check-macos-wave12.yml` — the prepared, single-job
  disposable run. Not run, pushed, or dispatched here.
- `scripts/desktop-check-macos-wave12-contract.sh` — static workflow contract
  (YAML parse, run-block Bash syntax, pipefail, whitespace).

## Fixture routing & the host guard

Every orchestrator invocation in the test suite is built by the single `invoke`
helper, which unconditionally passes `--fixture-bin "$bin"`. The `driver()` helper
**refuses to run anything** unless a fixture bin is supplied, and additionally
rejects a missing fixture executable, a symlinked fixture, a driver command that
is not in the narrow allowlist, and a checker path that is not the fixture
checker. There is no fall-through to a real host command in fixture mode. The
fixture tests therefore never reach a real `defaults`/`killall`/`pgrep`/`uname`
or the real checker.

The host guard requires `GITHUB_ACTIONS=true` and
`RUNNER_ENVIRONMENT=github-hosted` — the same signal the checker's
`SessionMode::GithubHosted` accepts — before the orchestrator will touch a Dock
preference. In fixture mode these variables are set so the guard path is
exercised. Note that these environment variables are the *alignment signal* the
checker already trusts; they are NOT independently "proof of a disposable VM".
Real disposability is a property of the GitHub-hosted runner itself, which is why
the orchestrator refuses to touch any Dock preference outside that signal and
fails closed on a personal/shared session.

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
assumption; the fixture suite cannot reproduce the Rust journal semantics and
therefore does NOT claim to prove the native lifecycle — only the orchestrator
wiring.

This directory's tests therefore do **not** establish any native qualification.
They establish command/script contract wiring only.

## Running the synthetic tests locally

The environment drivers remain quarantined. The staging-only suite can run
without exercising the orchestrator or touching host preferences:

```sh
python3 -B scripts/wave12-env/tests/test_staging.py
```

The broader fixture suite is separate and remains subject to the isolation
review described above:

```sh
bash scripts/wave12-env/tests/run-synthetic-tests.sh
```

This runs the orchestrator under fixture mode only. It asserts command wiring,
non-disposable/unsupported-host fail closure, missing/failed state confirmation,
preference restoration on failure, bounded readiness (no blind sleep), separate
per-condition artifacts (no overwrite), schema rejection, and the failure-report
upload gate. It establishes script contract wiring only and does **not**
establish any native desktop qualification.
