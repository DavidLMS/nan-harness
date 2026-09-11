# Wave 11 — review of the measured ChatGPT Linux startup failure

Task: `NEXT_DESKTOP_WAVE11_TASK.md`. Base and working tree: `de16611` on
`DavidLMS/desktop-chatgpt-probe-wave10` in this checkout. The measured
material is hosted run
[34519896130](https://github.com/DavidLMS/nan-harness/actions/runs/34519896130)
at `023ace238ce2f44f0ab62b7746b41405c309d4a1`.

Scope kept: no push, no new or re-run Actions run, no workflow dispatch, no
release, no merge of `main` (`8217252`), no Rust or native product edit, no
change to any file owned by another wave, no credential, private-store,
SSH-agent, sandbox-bypass, `--no-sandbox`, setuid or host-policy action, and
no Linux app launch on the shared Mac. Everything below was obtained by
read-only GitHub API calls, by unpacking the already-sanitized artifact this
run published, and by local synthetic contracts.

## 1. The run happened, and it consumed the allowance

Two runs exist on this branch. Both facts were re-read from the API during
this wave, not quoted from the previous report.

| Run | Number | Head SHA | Jobs | Outcome |
| --- | --- | --- | --- | --- |
| `34516978480` | 1 | `fd94915c25eafd6e207b04dd9379b6ebf40701e3` | 0 | failed before any runner started |
| `34519896130` | 2 | `023ace238ce2f44f0ab62b7746b41405c309d4a1` | 1 (`103014502814`) | job failed on the probe step, after real measurement |

Run 2 ran 2026-09-10T19:21:14Z to 19:28:24Z on `ubuntu-24.04` and its step
list is the proof that the native lifecycle was observed. Steps 1 to 11 and
13 to 15 succeeded; step 12 `Observe one deterministic probe lifecycle`
failed, which is the intended outcome of a diagnostic that measures a failing
startup:

```text
7  Verify workflow invariants and startup contracts         success
8  Prepare the official package without credentials         success
9  Assert the embedded native inventory helper              success
10 Assert the native inventory answers in a graphical …     success
11 Record the measured source and closed environment facts  success
12 Observe one deterministic probe lifecycle                failure
13 Validate sanitized evidence                             success
14 upload-artifact                                          success
15 Assert qualification separately                          success
```

So the "Hosted wave10 run count: 0" and "no run URL" statements in
`WAVE10_REPORT.md` section 6 are obsolete, and the earlier `fd94915` failure is
not the same event: with zero jobs it was rejected while the workflow file was
being parsed, and section 4 explains why the unpushed `de16611` audit change
neither caused nor explains it.

## 2. Independently verified evidence

Artifact `10169461531` (`desktop-chatgpt-wave10`, 1845 bytes, expires
2026-09-24) was downloaded and reduced. The chain of custody closes:

- the downloaded zip hashes to
  `81f3f856c9b7eae0f8ce3e2843a9129838ac0094f9c52df6692e07fbe098abe7`, equal to
  the `digest` the API reports for the artifact;
- the `report.json` inside it hashes to
  `32fda1e460ac63e2e0c96640e0638e8147aab62777bbe335964a2fbe7084713d`, equal to
  `identity.reportSha256` in `evidence.json`, so the published claim really is
  about that validated report;
- `evidence.json` records `source.revision` as the executed head SHA, and its
  `appVersion`/`runtimeVersion` (`26.903.71938` / `0.153.4`) match the checker
  report;
- the pre-change contract (`de16611`, the tree that produced the artifact)
  re-qualifies the artifact and re-derives its classification from the
  published integers alone:

```text
status=evidence-complete classification=app-exited-before-window app-outcome=all-failed probes=3 observations=273
```

That is line for line what the coordinator quoted. The classification is a pure
function of the numbers below, so this is not a second source of information —
it is a check that the closed rule and the published integers agree.

What was actually measured, in closed integers only:

- three probes, all `failed` with reason `application-exited`, taking
  8570 ms, 8358 ms and 8365 ms; `cleanup` passed for the app and the report;
- 273 observations in 77190 ms, every one with a readable process table
  (`inventoryUsableSamples=273`) and zero helper failures;
- the attributed application process appeared in 4 ticks with a maximum of 1
  concurrent instance, first seen at 44260 ms into the window; the
  application-named count is the same 4 ticks with the same maximum, so a
  packaged helper never ran without the main image running;
- 0 ticks showed a window attributable to that process, and 0 showed one at the
  checker's 300x200 test minimum; `appWindowFirstSampleMilliseconds=-1` means
  "never", not "unmeasured"; the session did have windows (`windowMax=1`, one
  pre-existing window from another process), so the inventory was not looking at
  an empty display server;
- the probe group peaked at 3 processes, `probeWorkers=3`, `checkerExit=1`, and
  no survivor of any kind after teardown (0 in group, 0 outside, 0 named);
- the launcher boundary was reached three times and exited with a real code each
  time: `launcherExits=3`, `launcherExitCodes=[1,1,1]`, `launcherExitsUnknown=0`,
  `unmatchedPrivateLines=8`;
- environment: an ELF executable (`launcherKind=elf`), no unresolved library
  reported for either binary, `resources/codex` and `resources/` both present,
  `unprivilegedUserns=1`, `maxUserNamespacesZero=0`, and **no `chrome-sandbox`
  sibling** in the package directory.

## 3. Script review: what the measurement can and cannot say

**The checker does not spawn the packaged binary; it spawns the `nanh` CLI
shim.** `crates/nan-harness-desktop-check/src/probe.rs:603` builds the command
from `spec.nan_harness` with `--provider-base-url`, `--model` and
`--executable <path>` (`launch_command`, on top of `isolated_command`), and
`crates/nan-harness-cli/src/commands/chatgpt_desktop/process.rs:46` starts the
real application with `Stdio::null()` on both output streams unless the shim was
given `--debug` (the flag is `ChatGptDesktopArgs::debug`,
`crates/nan-harness-cli/src/app/args/desktop.rs:26`). Two consequences follow,
and both are limits of the contract rather than bugs in it:

- the recorded `[1,1,1]` are the **shim's** exit codes. They prove the shim
  reached its own exit path, not what the Electron binary returned;
- the application's own error text is unrecoverable through this contract.
  `unmatchedPrivateLines=8` counts private lines that matched none of the four
  `runner.rs` diagnostic channels; app stderr never reached either stream.
  Getting it requires a product-side change, described in section 6 and not made
  here.

**A failed loader report used to be a silent zero.** `cmd_environment` counts
`not found` lines for the GUI binary and for `resources/codex`. If `ldd` itself
failed, the GUI count became `-1` but the runtime count stayed `0`. The
classifier reads `> 0` as a library gap and a negative as unknown, so on the
executed run `unresolvedRuntimeLibraries=0` is indistinguishable from "the
loader could not answer about the runtime". Fixed as bug A.

**The qualification set was missing one member of the classification domain.**
The builder can emit `window-undersized` (an attributable window that never
reached 300x200) and `startup_classification` ranks it above the library and
display rules, yet `cmd_qualify` refused it: a valid, complete capture of that
state exited 1 with "the evidence file is not closed" and no actionable reason.
Fixed as bug B.

**Episode counting was the one question the numbers could not answer.** The
probes run sequentially and `appProcessSamples=4` is a sum over the whole
window, so the bundle cannot say whether an instance ran in each of the three
attempts or four times inside one attempt. A first sample at 44260 ms and a
maximum of 1 say a process was seen late and never twice at once; they do not
attribute any of it to a specific probe. Section 5 closes that gap.

**Measured facts separated from hypotheses.** Nothing here proves a sandbox,
dependency or detached-launcher cause. A library gap is excluded only in the
weak sense that the loader reported none while it was answering, and the runtime
half of that reading is ambiguous for the executed run. The absent
`chrome-sandbox` sibling, `unprivilegedUserns=1` and `maxUserNamespacesZero=0`
are layout and switch values, not outcomes: on `ubuntu-24.04` the distribution
can confine unprivileged user-namespace creation through AppArmor, and the
switch that decides it was **never read by wave 10**. The strongest statement
the current evidence supports is that the binary was reached, a process carrying
its name was observed in 4 of 273 ticks, and it never produced a window the
checker could attribute to it.

Upstream material was reviewed read-only and narrows nothing further. The
current Electron *Process Sandboxing* document covers renderer sandbox
configuration and `--no-sandbox` and states no Linux launcher prerequisite, so
it can neither confirm nor refute a sandbox-related startup failure here. The
Chromium `suid_sandbox_development.md` page is marked "mostly out-of-date" and
addresses developer builds, not a shipped package. Two candidate Ubuntu wiki
page names for the user-namespace restriction resolve to "this page does not
exist", so this wave refused to cite a distribution policy value it could not
read from an authoritative page — which is exactly why that switch is now
measured on the runner itself (section 5).

## 4. Corrections, with local behavioural tests

`bash scripts/test-chatgpt-wave10-startup.sh` → **160 checks pass** on bash
3.2, credential-free, with the checker, graphical session, native helper,
process table, loader and (new) policy switch stubbed or reached through
injectable paths. `bash -n` is clean on both scripts, and the workflow
invariant audit was re-extracted from the YAML and re-run.

| # | Demonstrated defect | Fix | Proof |
| --- | --- | --- | --- |
| A | Runtime loader failure recorded a silent `0` while the GUI half recorded `-1` | `LDD_COMMAND` is injectable and both counts start at unknown (`-1`) until a loader report succeeds | loader fixture with `ok`/`missing`/`fail` per binary: `0 0`, `1 0`, `0 2`, `-1 0`, `0 -1`, plus a `library-gap` scenario classified as `environment-libraries` |
| B | `window-undersized` was publishable but not qualifiable | added to the `cmd_qualify` classification set | new `undersized` scenario: small attributable windows plus a running app give `app-window-samples=5`, `app-testable-window-samples=0`, `classification=window-undersized`, `status=evidence-complete` |
| C | The `de16611` audit never looked at the workflow-level `env` block, the position that rejected push 1 | `only_contexts(...)` now scans `str(document.get("env") or {})` against `{github, inputs, vars}` | suite case injects `${{ runner.temp }}` there; the extracted audit exits 1 with `unresolvable workflow context in the workflow env block: runner.temp` |
| D | The audit allowed `env` inside the concurrency group, which GitHub does not permit and which would have produced the same whole-file rejection | concurrency allow-set narrowed to `{github, inputs, vars}` | suite case injects `${{ env.DIAG_DIRECTORY }}` into the group; the audit exits 1 |

**Audit reviewed against the real executed workflow, as instructed.** The audit
extracted from the current file passes on the `023ace2` workflow and on the
present tree, so the unpushed `de16611` change does **not** invalidate the
executed run. Run 1 is refused by the pre-existing job-level rule
(`no job-level env: the runner temp path is a step concern`). For reviewers: the
run-block rule stays deliberately stricter than GitHub, which does allow `env`
in `run:` steps; that is a project policy choice, not a compatibility claim.

## 5. Smallest diagnostic extension, prepared and tested, deliberately unrun

Two additions, both inside the wave10 files, both synthetic-tested, neither run
against a hosted runner in this wave.

1. **Episodes.** `startup` gains `appProcessEpisodes` and
   `appNamedProcessEpisodes`: the number of maximal runs of consecutive ticks
   with a positive count. Sequential probes are separated by seconds of ticks in
   which no application process exists, so episodes separate "started on each of
   the three attempts" from "started at least once", which the sample total
   cannot. They stay a **lower bound on starts**: an instance that started and
   exited entirely between two ticks is invisible to every observer, and this
   extension does not change that. The `status=probed` line and the closed gate
   carry the new fields, with contradictions refused (an episode beyond its own
   samples, samples with no episode, an invented schema version). Evidence
   `schemaVersion` moves 1 → 2 because `startup` gained required keys; the gate
   now rejects the **executed version-1 bundle**, and that rejection is a
   covered regression case, so any re-qualification of run 2 must keep using the
   `de16611` contract, as section 2 did.
2. **The unread kernel switch.** `environment` gains
   `apparmorUsernsRestriction` (0, 1, or -1 for "absent or unreadable"), read
   read-only from `/proc/sys/kernel/apparmor_restrict_unprivileged_userns` on
   the pinned `ubuntu-24.04` runner. It is published as an integer and is
   deliberately **not** wired into `startup_classification`: a policy switch
   cannot select a root cause by itself, and inventing that rule would be the
   speculation this wave exists to avoid. Tests cover the published switch triple
   (`1`, `0`, `1`), a truncated older two-token private fact being refused
   outright, a value outside the three answers, and a bundle missing the key.

Detection limits, stated plainly: with episodes and the switch added, a future
run still cannot see the application's own error text, still cannot observe
anything that happened between ticks, and still classifies a
windowless-but-running attempt as `app-exited-before-window`. The extension
narrows *who exited and when* and *whether the runner policy was consulted at
all*; it does not identify a failure inside the app.

**Proposed single-cell native validation plan (coordinator's decision; this wave
did not spend it).** One push of exactly the three code files named in section 7
to `DavidLMS/desktop-chatgpt-probe-wave10` triggers exactly one run, because the
workflow's `push` filter names that branch and those paths only. Judge that run
on the new `status=probed` line and the new environment integers:

- `app-process-episodes=3` with `app-named-process-episodes=3` says the packaged
  image started on every attempt and exited before mapping an attributable
  window — the discriminator the executed evidence lacked;
- `apparmorUsernsRestriction=1` on that runner would be the first direct
  measurement of the confinement question wave 10 left open; `0` or `-1` removes
  or leaves it open, respectively, and neither result would by itself name a root
  cause;
- episodes of 1 alongside three launcher exits moves the leading story to a
  launcher/shim boundary that stopped spawning, and the next diagnostic should
  target that boundary rather than the app.

## 6. Product-path finding, described not repaired

The only way to recover the application's own startup error from a hosted run is
to stop discarding it, and that code sits outside this wave's ownership:
`crates/nan-harness-cli/src/commands/chatgpt_desktop/process.rs:46` nulls the
app's stdout and stderr unless `--debug` was passed, and
`crates/nan-harness-desktop-check/src/probe.rs:603` never passes it. A
coordinator who wants app-side text would change the checker's launch path (or
its stdio policy) in those files; the wave10 scripts would then reduce the extra
lines into `fact-stderr`, and `unmatchedPrivateLines` is already the counter that
shows how much of a capture matched no known channel. This wave records the
evidence gap — `launcherExitCodes=[1,1,1]` at the shim boundary and
`unmatchedPrivateLines=8`, with app stderr never forwarded — and stops there.

## 7. Files, ownership and privacy

Changed in this wave, all already owned by the wave10 diagnostic:

- `.github/workflows/desktop-check-chatgpt-wave10.yml` — audit positions C and D.
- `scripts/chatgpt-wave10-startup.sh` — loader unknown propagation,
  qualification set, episode counting, namespace switch, gate and schema version 2.
- `scripts/test-chatgpt-wave10-startup.sh` — synthetic contracts for all of the
  above: 71 checks at wave 10, 156 after bugs A–D, **160** now.
- `WAVE11_REPORT.md` — this file.

Nothing else was touched. `WAVE10_REPORT.md`, `WAVE10_COORDINATOR.md`,
`WORKER_REPORT.md`, `LOCAL_READINESS_REPORT.md`, `CURRENT_DESKTOP_TASK.md` and
`NEXT_DESKTOP_WAVE11_TASK.md` stay untracked and byte-identical; no Rust, no
native edit, no `main` merge, `main` stays at `8217252`.

Published evidence keeps its boundaries: only integers, closed words,
identifiers, versions, timestamps and digests leave the runner; the bundle is
size-capped and gated on both ends; the private observation directory is reduced
and deleted rather than uploaded; and nothing in this report contains a process
list, window title, path, screenshot, prompt or model output. Retrieval used the
already-sanitized artifact only.

Limitations to carry forward from the older report: its "no run" conclusion and
its blocker section are obsolete (section 1); its section 2 classification table
is incomplete, because the shipped builder also produces `window-undersized`
(bug B); and its claim that `ldd -- <path>` "would have silently downgraded the
library evidence to unknown" understated the runtime half, which downgraded to
zero instead (bug A).

## 8. Remaining native evidence and status

What stays unknown after this wave, unchanged by it: why the packaged binary
exits without mapping an attributable window on `ubuntu-24.04`; whether an
instance started in each of the three measured attempts; whether the
distribution's user-namespace policy was involved at all; and what the
application itself printed. The one-run allowance is spent, the first executed
run's evidence is complete and correctly classified, and the two discriminating
observations are built and locally proven but unrun.

Operational note: the Orca lifecycle channel was unavailable in this session as
well (`runtime_unavailable`, "Restart Orca and try again"), so the completion
report is attempted once and this file stands as the delivery, per the task's
final instruction.
