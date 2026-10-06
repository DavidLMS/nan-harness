# Desktop compatibility checker

The hosted Desktop qualification checks Zed, Codex, Claude and Hermes on Linux
x64, macOS ARM64 and Windows x64. Pen is deferred. Each of the twelve cells
requires three fresh sessions with response verification, a real file-tool
round-trip, explicit UI recovery, application cleanup and global cleanup.

## Qualification status

The inspected baseline completed all twelve cells and 36 sessions on
`4b201c8e4151960050fc4011e0d06b502fdb5ebf` in
[qualification run 37494497343](https://github.com/DavidLMS/nan-harness/actions/runs/37494497343).
The [complete quality gate](https://github.com/DavidLMS/nan-harness/actions/runs/37492596237)
passed on the same commit. Claude and Hermes macOS required a second attempt;
Codex Linux completed six consecutive sessions across the focused and full runs.
Its earlier startup/profile-read failure did not recur; that does not establish
its cause. Closed observations remain available for future diagnosis.

| Application | Linux x64 | macOS ARM64 | Windows x64 |
| --- | --- | --- | --- |
| Zed | 3/3 | 3/3 | 3/3 |
| Codex | 3/3 | 3/3 | 3/3 |
| Claude | 3/3 | 3/3 | 3/3 |
| Hermes | 3/3 | 3/3 | 3/3 |

[desktop-baselines.json](desktop-baselines.json) records the exact inspected
versions and package digests or source revisions. A passing baseline is not a
claim about every subsequent upstream release. Each new source commit needs
its own qualification receipts; inspect the current workflow's aggregate.

## Semantic deterministic qualification

[Desktop full qualification](../.github/workflows/desktop-check-qualification.yml)
is the normal entry point. It runs manually and every Monday at 07:43
`Europe/Madrid`. It reuses
[Desktop native qualification and diagnostics](../.github/workflows/desktop-automation-feasibility.yml),
which owns installation, preparation, native sessions and closed evidence.
The reusable filename is retained for existing dispatch clients.

The aggregate artifact, `deterministic-qualification-matrix`, requires all twelve
cells from the exact tested source commit and explicitly excludes Pen. Missing,
duplicated, mismatched or incomplete cells fail the gate. Artifact selection
uses the latest attempt independently of its outcome, validates archive digests,
and never substitutes an older passing receipt. Initial failures remain in
prior attempt artifacts. Raw vendor output, UI text and private state are not
uploaded.

PRs touching Desktop code or automation run offline contracts without vendor
applications or credentials. Use manual native qualification for affected cells
when changing app behavior, then run the complete matrix before integration.
The full workflow also runs `cargo check-all` and the semantic adapter contracts.

## Daily maintenance

[Daily desktop compatibility](../.github/workflows/desktop-check-daily.yml) runs
at 06:23 `Europe/Madrid`. A single Linux planner uses the checker's official
resolver with `--target linux-x64`, `--target macos-arm64` and
`--target windows-x64`. Resolution inspects metadata and, where needed, package
bytes; it does not install or launch vendor applications. Downloads stay in
private temporary storage and are discarded after resolution.

The closed `desktop-maintenance-plan` distinguishes:

- `already-qualified`: exact supported baseline and passing current-commit evidence;
- `qualification-pending`: supported baseline missing valid evidence, selected for native checks;
- `adaptation-required`: a different upstream version, package digest or source revision;
- `resolution-failed`: official identity could not be established, which fails the daily result.

Upstream drift is advisory and cannot certify a new release or silently change
selectors. Adapt the source-bound contracts and update the baseline catalog in
a reviewed PR before qualifying new bytes. Independent supported cells continue
when another cell needs adaptation. The weekly run rechecks the entire pinned
baseline even when newer upstream versions require review.

Only closed artifacts from trusted default-branch manual/scheduled workflows,
with the exact source SHA and inspected upstream identity, can suppress work.
A newer failed run prevents falling back to older success. Missing, expired or
invalid evidence requests fresh checks. The planner does not update the public
compatibility feed or send messages/issues.

Manual daily runs default to `plan_only=true`. Set it to false to run selected
cells, and optionally set `force=true` to recheck supported current versions.
Schedule execution uses the default branch; it is not an exact-time SLA.

## Release scope

The release gate runs all twelve Desktop cells against the **exact attested
release binaries**. Release builds include `desktop-qualification`; its hooks
remain inactive unless explicitly enabled in a disposable hosted environment.
Private profiles, synthetic workspaces and fixed loopback fixtures remain
required. Normal launches do not activate these fixtures.

The checker and workflow execute from the trusted caller commit. Each closed
cell records that source SHA and the tested binary's SHA-256. The aggregate
independently downloads and verifies the release assets, then requires all
twelve passing cells to match those exact bytes. Its `release` field records
the tag, immutable release commit and per-platform asset hashes.

Publication requires both this deterministic Desktop gate and the existing
49-cell live CLI gate. Desktop deterministic evidence does not establish live
NaN/provider compatibility. Publication keeps the release non-latest;
recommendation is a separate, explicit operation.

## Diagnostics and history

The shared workflow retains explicit targeted manual diagnostics. Obsolete wave
workflows are stored under [archive/desktop-workflows](archive/desktop-workflows)
and no longer appear as active Actions. Offline sandbox contracts remain
available through [Desktop branch diagnostics](../.github/workflows/desktop-check-diagnostics.yml).
See [the investigation history](desktop-investigation-history.md) for dated
receipts and previous hypotheses; it is not the current operational checklist.

The independent `nanh-desktop-check` executable also supports opt-in local checks.
Personal-machine isolation, native self-updaters and live-provider behavior have
separate boundaries; see [the distribution catalog](checker-platforms.md).

## Run and inspect

After the independent checker release has been published:

```sh
nanh-desktop-check run --app zed-desktop
nanh-desktop-check run --app zed-desktop --nan-harness /absolute/path/to/nanh
nanh-desktop-check run --yes --non-interactive --ephemeral
```

Without `--yes`, the checker displays its inventory and asks before test or
installation operations. Noninteractive runs without `--yes` do not proceed.
Apps already running are left alone. Ambiguous or unreadable installations do
not trigger replacement downloads. An absent nanh is resolved through the
official `available` manifest, with the downloaded checksum and version checked.
An existing nanh that lacks the integration's `--provider-base-url` option is
reported as unsupported for the probe and remains installed unchanged.

Each selected app gets three separate deterministic probes. Set `NAN_API_KEY`
in the launching environment to add one live probe, using `--model` to override
the default `qwen3.6`. The checker does not look up saved provider credentials.
Never place a key in command arguments or in a report. The real key stays in
the probe's local provider boundary; nanh receives a random session token.

Live probes allow at most four generation requests, 2,048 output tokens per
request and 180 seconds per app, including shutdown. Deterministic probes have
a 240-second process deadline. Requests are not retried by the checker. The
report records elapsed probe time, not a billing total. Provider generation
limits are upper bounds, not measured cost estimates.

Passing evidence requires GUI input readback, visible response text, a real
tool result containing the synthetic fixture marker, and completed provider
output. Deterministic probes also require controlled error recovery. A tool
result or an assistant claim by itself does not establish success. Live and
deterministic evidence remain independent.

Schema v2 reports record both the input method and the response verification
method. Accessibility is preferred; incomplete app trees can use xa11y input and
owned-window captures with bundled local OCR. Captures and recognized text are
not written to public reports or artifacts. Focus changes, occlusion, unexpected
windows, geometry changes and unsupported executable architectures fail closed.
Schema v1 reports remain readable without attributing visual capabilities to them.

For hosted or explicitly separated execution, run `prepare` without `NAN_API_KEY`
and keep its private receipt and downloaded checker. Then use `run --prepared
<receipt> --mode deterministic`; a separate `--mode live` invocation requires a
nonempty key and makes no installation changes. Both invocations must select
the same applications as preparation. Each writes its own sanitized report.

The final report path and exact SHA-256 digest are printed. Exit status 0 means
all requested checks passed (or operations were declined); 1 means the report
contains a non-passing requested check or cleanup problem; 2 means the command
could not finish. A missing key is an explicit skipped live probe, not a failure
of a deterministic-only run.

## Cleanup and recovery

Default cleanup removes only new, unchanged resources recorded by this run.
Existing applications, nanh binaries and shared packages are never uninstalled.
`--ephemeral` retains owned installations and the bootstrap's downloaded copy;
it does not skip process shutdown, configuration restoration or key protection.

Private journals live under `XDG_STATE_HOME/nanh-desktop-check` on Unix, falling
back to `~/.local/state/nanh-desktop-check`; Windows uses
`LOCALAPPDATA/nanh-desktop-check`. Reports contain no recovery paths, raw errors,
prompts, model output or credentials. Journals and reports are separate files.

If cleanup fails, retain the private run directory and use:

```sh
nanh-desktop-check cleanup <run-id>
```

Close the tested application first. Recovery checks the selected nanh binary's
digest and retries native restoration before sealing interrupted probe files.
Changed binaries, replaced paths, mounted disk images and interrupted unsealed
installations require inspection; the checker preserves them. It does not claim
automatic recovery after every forced shutdown. Do not upload the private
journal or remove it while native configuration still needs restoration.

## Share evidence deliberately

```sh
nanh-desktop-check validate-report /path/to/report.json
nanh-desktop-check submit /path/to/report.json
```

Submission displays the sanitized report and requires a separate confirmation.
It uses existing GitHub CLI authentication, with a browser fallback, and never
installs GitHub CLI or requests release-write credentials. An issue is only an
inbox: creating or editing one does not execute tests or update compatibility.

The maintainer runs `Approve compatibility evidence`, selecting `desktop-issue`
or `desktop-run`, the source issue/run and the exact reviewed report digest.
For Actions, also select its `desktop-report-<platform>-<app>` artifact. The
workflow freezes the reviewed bytes and rejects edits that change the digest.
Hosted runs contain separate `deterministic.json` and, when requested,
`live.json` reports. The digest selects exactly one of them; approve each track
separately. Legacy artifacts containing `report.json` remain accepted.
Approval trusts the maintainer's dispatch, not an author allowlist. The central
publisher still verifies the tested nanh's official binary and historical
registry before accepting an exact app/runtime/platform/architecture tuple.

`Manual Desktop compatibility checks` uses the published checker in one fresh
runner per selected app, with read-only permissions and no schedule. Live calls
require the explicit workflow option and protected `canary-live` environment.
Reports need a separate approval. The exact-binary qualification described above gates
new releases independently of these opt-in published-checker reports.
See the [hosted canary runbook](README.md) for state initialization, CLI release
qualification, recommendation and the guarded Tart emergency path.
