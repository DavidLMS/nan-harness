# Compatibility Canary

For the Windows integration candidate, explicit platform skips and remaining
qualification work, see [Windows integration](windows-integration.md).

The nan-harness compatibility canary tests all 15 supported CLI harnesses on
GitHub-hosted native runners without adding commands to the public `nanh`
binary. Daily checks refresh compatibility evidence for published binaries;
the separate release gate qualifies new releases.

GitHub Actions provides modular CLI checks and protected live provider probes.
Each new draft release automatically dispatches verification of its exact
signed Linux/macOS ARM64 and Windows x64 assets. Publication remains a separate,
explicit operation after all 43 supported live cells pass; Windows Prime Agent
and FX are skipped because no official Windows distribution is available.
Historical 30-cell evidence remains valid only for recommending an already
published stable release with its complete, bound publication receipt. It cannot
qualify a new publication. The Tart procedures below remain recovery references.

Deterministic compatibility covers installation, launch, provider traffic,
process cleanup, sentinel behavior, and one representative tool round-trip.
The complete native tool inventory is observed for maintenance but does not
block compatibility when those functional contracts pass.

| Trigger | Platforms | Coverage |
| --- | --- | --- |
| Daily hosted | Linux/macOS ARM64 and Windows x64 | Pending upstream versions: clean install, doctor, deterministic conformance and live `qwen3.6`; partial feed publication by harness |
| Hosted release gate | Linux/macOS ARM64 and Windows x64 | Automatic draft verification of 43 supported live cells; publication remains explicit |

Compatibility evidence is release-scoped, with CLI schema v2 and unified CLI /
Desktop schema v3 assets. Daily publication requires every supported native
platform to pass for the same upstream version and exact nan-harness release.
Release-gate publication remains all-or-nothing for its full matrix.

## Daily hosted compatibility

`.github/workflows/harness-canary.yml` runs at 05:00 `Europe/Madrid`, including
daylight-saving changes. GitHub may delay scheduled execution. It replaces the
source/main detector, whose reports never updated the feed.

The workflow selects the stable release named by the `available` feed and the
GitHub-recommended `latest` release, deduplicating equal tags. It verifies unique
tags, exact commits, signed checksums and all six native CLI/canary assets.
Missing or invalid assets leave that release pending; they do not certify a
replacement built from `main` or block an independently usable release.

Official upstream versions are frozen before native cells run. Only versions
newer than each release's live evidence, or missing that evidence, are tested.
Unresolved metadata and failed cells are retried on the next daily run. Each
harness must pass installation, diagnosis, deterministic conformance and live
`qwen3.6` on Linux, macOS and Windows; Prime Agent and FX remain unavailable on
Windows. At most three cells run concurrently. Desktop apps are not selected.

The aggregator binds reports to the run attempt, trusted workflow, exact release
binary, platform and frozen version. Missing or failing cells hold back only
their harness. Invalid provenance rejects the affected release batch. Successful
independent results update both `compatibility.json` and `compatibility-v3.json`;
other CLI entries, Desktop evidence and historical releases are preserved. The
workflow reports failure after publishing independent successes when any work
remains pending. Reports and the per-harness summary are retained for 30 days;
raw child output and credentials are never uploaded.

Manual dispatch defaults to `verification_only=true`. Set `force=true` to
reverify current versions, including those already certified. A verification-only
run generates candidates without remote writes, including during backup recovery.
Scheduled runs publish automatically. All runs share the release-channel
concurrency group with release publication and recommendation.

Before enabling the schedule, configure `compatibility-live` and
`compatibility-publication` environments with deployment branch policies allowing
only the repository's default branch and no per-run reviewer requirement. Store
the existing provider credential as `NAN_API_KEY` in `compatibility-live` through
the normal secret-management channel. The live job has read-only GitHub access;
only the publisher has `contents: write` and it receives no provider credential.
Do not weaken the separate release-publication environment to enable daily runs.

For rollout, run a manual verification-only pass first, inspect its summary,
then dispatch with `verification_only=false` and inspect both remote feed assets.
No new nan-harness release is needed to distribute refreshed evidence.

Safe reports follow
[`crates/nan-harness-canary/resources/canary-report.schema.json`](../crates/nan-harness-canary/resources/canary-report.schema.json).
They include versions, digests, bounded check outcomes, durations, and a stable
failure fingerprint. They exclude credentials, prompts, responses, tool
payloads, command output, and local paths. Raw output is retained only in
private local logs when explicitly requested.

## Operations

This runbook configures and operates the compatibility canary host.
It does not change the public `nanh` command surface.

## Prerequisites

- Apple Silicon Mac running macOS 13 or newer.
- At least 100 GB free before downloading Linux and macOS base images; the
  preflight requires 50 GB once both images are already cached.
- Homebrew, Rustup, GitHub CLI with `gh attestation verify` support, Tart,
  OpenSSH, `sshpass`, and `perl` (a macOS base tool, used only to take the
  publication host lock; see [Publication host boundary](#publication-host-boundary)).
- GitHub CLI authenticated with release, issue, and contents access to this
  repository.
- A current GitHub CLI that can verify `SHA256SUMS` attestations from the fully
  qualified workflow identity `DavidLMS/nan-harness/.github/workflows/release.yml`
  for `refs/tags/<tag>`; the canary rejects unverified or mismatched assets.
- The existing `NAN_API_KEY` exported in the interactive setup shell. This is
  the only NAN API key used by the canary.
- An unlocked login Keychain whenever launchd starts a VM.

Install Tart and the SSH password helper with the currently supported Homebrew
formulae, then verify the exact commands before enabling launchd:

```sh
brew install cirruslabs/cli/tart
brew install cirruslabs/cli/sshpass
tart --version
sshpass -V
gh auth status
gh attestation verify --help
```

## Bootstrap

The provider permits one API key. Export the existing value; do not generate a
second credential:

```sh
export NAN_API_KEY='<existing-key>'
cargo run --locked -p nan-harness-canary -- setup
```

`setup` performs a real authenticated model discovery, requires `qwen3.6`,
checks the host tools, and copies the same API key into the
`dev.nan-harness.canary` Keychain service for launchd. The item trusts only
`/usr/bin/security`, so rebuilding the Rust runner does not trigger an access
prompt. The value is sent to Keychain through stdin and is never printed or
placed in a process argument.

The key is read only by the Mac host and injected into an in-memory live-step
environment. It must never be copied into a report, VM image, command output,
private log, GitHub artifact, issue, or notification.

To validate without changing Keychain:

```sh
cargo run --locked -p nan-harness-canary -- setup --check-only
```

Optional private ntfy setup:

```sh
export NAN_CANARY_NTFY_TOKEN='<write-only-token>'
export NAN_CANARY_NTFY_URL='https://ntfy.example.com/nan-harness-canary'
cargo run --locked -p nan-harness-canary -- setup \
  --ntfy-url "$NAN_CANARY_NTFY_URL"
```

The URL is stored in each launchd job. The token remains in Keychain under the
separate `NTFY_TOKEN` account.

## Legacy local Tart operations (history and recovery)

The following Tart and launchd procedures are retained for historical evidence,
receipt recovery, and one-off diagnosis. The hosted replacement is validated
and local repo-owned launchd schedules are retired; these procedures are not
the release publication path and must not be re-enabled without explicit
operational review. The shared Tart binary remains installed because ownership
is not exclusive to this repository.

Before installing schedules, run the automated Linux VM spike:

```sh
canary/host/spike-tart.sh
```

It clones a clean image, verifies SSH, records durations, resident memory, guest
memory, and Tart storage, then deletes the VM. The default `shared` network
avoids modifying root policy on the host and is compatible with unattended
launchd jobs. Cell contracts allow up to 30 minutes for an uncached image clone
but keep the VM boot timeout at five minutes, so a slow first download is not
misclassified as a boot regression.

Softnet is an explicit host-hardening option because it requires either SUID or
passwordless sudo for its privileged network setup. Configure that boundary
separately, verify it, then opt in for the spike and scheduled suites:

```sh
NAN_CANARY_NETWORK=softnet canary/host/spike-tart.sh
```

Also record:

- first image download duration and disk use;
- idle and peak memory;

Before enabling two execution lanes, run the parallel capacity spike twice:

```sh
canary/host/spike-parallel-tart.sh
canary/host/spike-parallel-tart.sh
```

Both runs must finish with two prepared VMs, memory-pressure level 1, and no
more than 1 GiB of additional swap. A failed spike keeps the default at one
lane; set `NAN_CANARY_MAX_PARALLEL_CELLS=2` only after both runs pass.

Then run one manual installation cell through `nan-harness-canary cell`. A cell
specification is TOML and references the matching release `nan-harness` and
`nan-harness-canary` assets plus the guest helper scripts. The runner clones the
image, starts it headlessly, mounts read-only input and writable output
directories, runs bounded steps over SSH, writes safe evidence, and destroys the
VM.

Daily, weekly, and release suites prepare one local base image per selected
platform, including the common guest bootstrap but no credentials or release
assets. Manual single-cell runs keep their direct path. Every cell is still
cloned from a clean image: cells on one platform remain sequential while the
Linux and macOS lanes may run concurrently. Preparation falls back to the
canonical image if the installed canary lacks the hidden capability or a
prepared base cannot be built. Set
`NAN_CANARY_MAX_PARALLEL_CELLS=1` for immediate serial rollback.

When `--private-log-dir` is set, raw step output is copied there for local
diagnosis. These logs can contain model or tool output: never upload them to
GitHub, attach them to issues, or send them through ntfy. Host runners use a
private umask so new state and diagnostic files are readable only by the canary
user.

DeepSeek `0.1.5-rc.2` installs use npm's `--before=2026-09-22T00:00:00Z`
registry cutoff. Its internal caret ranges otherwise select the incomplete
rc.3 publication, which requests an unpublished
`@deepseek-ai/dsh-client-ui-sidebar-documentpreview@^0.1.5-rc.3` and fails with
`ETARGET` before install scripts run. Only this exact version is bounded;
latest-version checks remain unrestricted. Reassess the workaround when adopting
a complete upstream release. The cutoff is not a lockfile and cannot protect
against registry removals. The `DeepSeek install diagnostic` workflow compares
the original and corrected installers in fresh Linux ARM64, macOS ARM64 and
Windows x64 cells. It exports only a closed diagnostic projection and verifies
the installed version; raw npm logs stay private and are deleted. Installation
success never substitutes for the complete live release gate.

The single-cell wrapper downloads both matching ARM64 asset pairs and runs a
clean deterministic-plus-live probe for one harness. The central suite wrapper
requires the exact `v<nan-harness-version>` release tag and all four assets by
their canonical names, verifies private staged copies, and uses only those
copies for execution and publication. Manual runs are dry runs and never write
the compatibility feed:

```sh
canary/host/run-manual.sh claude-code linux
```

Re-run a failed cell with the exact same contract:

```sh
cargo run --locked -p nan-harness-canary -- reproduce \
  --spec /path/to/cell.toml \
  --report /path/to/failed-report.json \
  --output /path/to/reproduced-report.json
```

## Legacy Tart schedules (retired)

Check the host before installing schedules:

```sh
export NAN_CANARY_NTFY_URL='https://ntfy.example.com/nan-harness-canary'
canary/host/preflight.sh
```

Install the two user launch agents only after the Tart spike passes:

```sh
export NAN_CANARY_NTFY_URL='https://ntfy.example.com/nan-harness-canary'
canary/host/install-launchd.sh
```

The jobs are:

| Label | Schedule | Work |
| --- | --- | --- |
| `dev.nan-harness.canary-daily` | Monday-Saturday at 03:17 | All Linux clean installs, doctor checks, deterministic conformance, and two rotating live tool probes |
| `dev.nan-harness.canary-weekly` | Sunday at 04:17 | All Linux and macOS deterministic plus live tool probes |

There is no scheduled release poller. A release gate runs only after an
operator explicitly names a draft tag.

Remove the jobs without deleting history:

```sh
canary/host/uninstall-launchd.sh
```

Logs and state default to:

```text
~/Library/Application Support/nan-harness-canary
```

Set `NAN_CANARY_STATE_DIR` before installing launchd to use a different
location.

## Manual suites

### Hosted ARM64 CLI selection

The registered `Hosted ARM64 CLI compatibility` workflow is a manual-only,
synthetic entry point for migrating the Tart CLI matrix to GitHub-hosted Linux
and macOS ARM64 runners. Its OS choice is `linux`, `macos`, or `both`, and its
harness input is `all` or a comma-separated subset of the 15 CLI harness
identifiers; Windows, desktop identifiers, empty selections, and duplicates
are rejected before a native runner is reserved. Select `deterministic` for
installation and conformance without a provider key, or `live` to run the
explicitly selected real-provider probe with the protected `NAN_API_KEY`
environment secret; live cells verify the secret is present before building,
and installation/deterministic steps never receive it.

Manual examples (the workflow dispatch UI supplies the source checkout):

```text
platforms=linux  harnesses=all  mode=deterministic
platforms=both   harnesses=codex,claude-code  mode=live
```

Each OS/harness cell is independent and uses the selected checkout's explicit
40-character commit identity (a branch or tag is not accepted). Live mode is
restricted to explicit manual dispatch and has no reusable-call, schedule,
publication, or cutover behavior. The retired daily and weekly Tart procedures
remain recovery references only; the daily hosted workflow owns scheduled feed
refreshes.

### Hosted release gate and publication

The [hosted release gate](../.github/workflows/release-gate.yml) runs automatically
from the default branch after `release.yml` creates a draft. This automatic run
uses live inference with `verification_only=true`: it never publishes the draft.
It can also be dispatched from the default branch with these exact inputs:

```text
tag=vX.Y.Z  tag_commit=<40 lowercase hex>  model=<bounded identifier>
mode=live  verification_only=true|false
```

It requires a draft for publication; verification-only mode also accepts an
already published stable release. It resolves the tag to the exact supplied
commit, verifies the signed `SHA256SUMS` and six canonical assets, and runs
43 unique CLI cells: 15 Linux ARM64, 15 macOS ARM64 and 13 Windows x64.
Prime Agent and FX are explicitly unavailable on Windows and are not counted
as passes. The workflow checks out only immutable `GITHUB_SHA` trusted-branch
code and never executes tag-controlled workflow code. `NAN_API_KEY` is exposed
only to live cells through the protected `canary-live` environment. The
`release-publication` environment must be configured with protection rules
before enabling publication; this documentation does not configure or assert
that environment.

The default `verification_only=true` mode is safe for testing and produces
verification evidence without publishing. Publication is allowed only for a
live run with `verification_only=false`, after the full 43-cell pass and
provenance handoff succeed. The publisher makes the release public and
non-latest, updates the compatibility and available-release feeds, and retains
durable evidence/receipts; a deterministic run never satisfies the live release
criterion.

There are no scheduled GitHub release jobs. The existing granular hosted CLI
workflow remains available for selected synthetic/deterministic or live cells;
it is not a release publication gate.

When a published release should become recommended, dispatch the separate
[recommendation workflow](../.github/workflows/recommend-release.yml) from the
default branch with `tag` and `tag_commit`. It recovers the original gate
identity from durable release evidence and receipts, revalidates the exact tag,
assets, attestation, and feed state, then explicitly moves `latest`; it does not
rebuild anything.

The hosted replacement was validated on main by run
[34830826982](https://github.com/DavidLMS/nan-harness/actions/runs/34830826982)
with 30/30 live ARM64 cells and aggregate evidence passing. Local repo-owned
launchd schedules were retired on 2026-09-14; see the
[retirement record](../docs/canary-tart-retirement-2026-09-14.md). No release
was published by that verification-only run.

The existing evidence record remains unchanged: the current CLI validation is
30/30 real live cells (15/15 per platform), while the historical deterministic
30/30 record remains separate. The Aider completion-marker intermittency and
its bounded diagnostic remain part of that evidence; a later pass does not
erase the earlier failure or prove its cause without a new exact-source run.

### Legacy host schedules and release commands (recovery only)

Retain the following procedures for historical evidence and recovery only.
They are not required to publish a release through the hosted replacement; do
not install the launchd schedules or run the legacy release gate or
recommendation command as part of normal post-cutover operations.

Run scheduled verification and publication:

```sh
canary/host/run-scheduled.sh daily
canary/host/run-scheduled.sh weekly
```

Scheduled wrappers pass `--publish-feed`; direct `run-suite.sh` and
`run-manual.sh` invocations do not. To publish a manually prepared suite, pass
`--publish-feed` explicitly to `run-suite.sh` after reviewing its safe reports.
The publication boundary requires an executable report validator and runs its
complete `validate-report` command for every report before applying policy
checks.
Every feed write takes an owner-aware crash-recoverable host lock, validates
non-empty JSON at its own schema, preserves every prior release record, stages a
uniquely named candidate, keeps a separate validated backup asset, and verifies
or restores the stable replacement. The publisher writes two assets under that
one lock: the legacy CLI-only `compatibility.json` first, then the unified
`compatibility-v3.json`, which also carries Desktop evidence. An interrupted run
with a missing stable asset restores that backup before continuing.
The [compatibility feed reference](compatibility-feed.md) describes both
schemas and what published evidence may change. After the
stable asset is verified, the publisher removes staged candidates and retains
the three newest backups. Cleanup failures do not invalidate a verified feed
and are retried by the next successful publication.

#### Archived local draft gate (recovery only)

```sh
canary/host/run-release-gate.sh --tag vX.Y.Z # legacy recovery only
canary/host/run-release-gate.sh --tag vX.Y.Z --repo owner/name
canary/host/run-release-gate.sh --tag vX.Y.Z --force
```

The gate refuses an omitted tag, a missing release, or a release that is not a
draft. It runs the orchestration committed in that tag from a temporary detached
worktree and records an atomic per-tag receipt for asset verification, suite
success, compatibility feed publication, release publication, and
available-release feed publication. A retry resumes after the last completed
phase, but revalidates the tag and signed assets first.

A rerun repairs a run that stopped between phases, but it does not repair a
lost receipt for a release that is already public: the gate refuses a
non-draft release with no recorded preceding phase rather than republishing
blind. Recovering the available-release feed for such a release is
`publish-available-release.sh`'s job; anything else is a deliberate maintainer
decision.

Only a real suite failure starts the six-hour cooldown. Download, checksum,
attestation, feed, or publication failures can be retried immediately after
correction. Use `--force` only to bypass a suite cooldown after correcting its
cause.

Historically, a fully green local gate published the release-scoped compatibility feed, published
the draft as a public release that is explicitly **not** latest, and copies that
tag's attested `update-manifest.json` into the standing `available` release.
That feed is what an explicit `nan-harness update` reads, so a validated release
is installable on request as soon as the gate finishes.

The gate never recommends. GitHub's `latest` release stays the recommended one,
which is what startup discovery, both installers, and older clients follow.
#### Archived local recommendation (recovery only)

```sh
canary/host/recommend-release.sh --tag vX.Y.Z # legacy recovery only
canary/host/recommend-release.sh --tag vX.Y.Z --repository owner/name
```

It mutates nothing until it has the complete evidence for that exact release:
a finished gate receipt for this repository, tag and commit; a remote tag that
still resolves to the commit the gate validated; a public, non-draft,
non-prerelease release carrying its metadata assets; and a checksum manifest
that still hashes to the digest the gate recorded and still passes
`gh attestation verify`. An unchanged checksum document is only a list of
expectations, so the release's contents are proven too: every asset that
document names is downloaded and hashed, and so is every installable artifact
the `update-manifest.json` clients read points at, which must belong to this
exact tag. A replaced or deleted manifest or binary is refused before `latest`
moves. It also refuses any recommendation that would move
`latest` backwards, records a per-tag receipt under `recommendations/`, and is a
no-op once the tag is already recommended. A prerelease tag is published but
never enters the available-release feed.

### Available-release feed layout

The standing `available` prerelease holds one immutable
`update-manifest-<version>.json` per published stable release plus the
`update-manifest.json` clients read. That pointer is derived: it must always
carry the contents of the highest recorded version. Every publication recomputes
it and repairs it, so a crash or a failed upload between deleting and
re-uploading the pointer — which is what `gh release upload --clobber` does — is
recovered by the next run rather than leaving the feed without a manifest. An
older, out-of-order gate run records its own release and leaves a newer pointer
untouched.

### Publication host boundary

`canary/host/host-lock.sh` gives every publication writer one exclusive lock per
resource, held by the kernel through `flock(2)` on a descriptor the script keeps
open: the release-channel lock of one repository, taken by the release gate
(across publishing the release and updating the feed) and by
`recommend-release.sh`, and the compatibility feed lock taken by
`publish-compatibility.sh`. Every channel read that gates a mutation happens
inside the lock, and only a confirmed `404` is read as absence: an uncertain
answer aborts without mutating.

Because the kernel owns the exclusion there is no owner document to consult, no
staleness window, and no reclamation step, so recovery can never retire a
replacement live owner. A writer that dies releases its lock automatically, once
the last short-lived `gh` or `jq` child that inherited the descriptor is gone —
never earlier. The feed publisher the gate invokes inherits that descriptor and
re-enters the gate's own transaction; no environment variable grants ownership.

This lock is **local legacy recovery state**. It serializes the supported writers on the single macOS
publication host and provides no cross-host atomicity. A lock whose note records
another host is refused rather than reclaimed, so the boundary fails closed, but
a writer on another machine — or a manual `gh release edit` — is outside the
protocol. Publish and recommend only from the supported host.

The lock is a regular file. A `compatibility-feed.lock` **directory** left behind
by the previous protocol is reported explicitly; delete it once while no
publication is running. `NAN_CANARY_LOCK_STALE_SECONDS` no longer exists: there
is nothing to time out.

`preflight.sh` requires `perl`, a macOS base tool, because `flock(2)` and
`fstat(2)` on an open descriptor cannot be called from bash alone.

## Expected duration and retention

| Operation | Cached duration | Global budget | Purpose |
| --- | --- | --- | --- |
| Manual cell | 2-5 minutes | 60 minutes | Reproduce one harness/platform without publication |
| Daily | 20-30 minutes | 60 minutes | Detect Linux installation and deterministic regressions every non-Sunday day |
| Weekly | 45-60 minutes (20-30 with a validated two-lane host) | 120 minutes | Verify every harness live on Linux and macOS |
| Hosted release gate | Hosted Linux/macOS ARM64 and Windows x64 matrix | 180 minutes per cell | Automatically verify each draft with the exact 43-cell live matrix; publication remains explicit |
| Legacy local release gate | 45-60 minutes (20-30 with a validated two-lane host) | 120 minutes | Historical Tart release evidence and recovery only |

The first uncached Tart image can add up to 30 minutes per platform. A suite
runs one VM by default and at most two after the capacity gate: cells remain
sequential within each platform lane and the lanes share one suite deadline.
Scheduled jobs wait up to two hours for the host suite lock; manual and release
commands return temporary-failure status 75 when another suite owns it. The
execution budget starts after acquiring the lock, and the suite passes its
remaining global budget into each cell.

`prune-state.sh` runs before scheduled and release operations. It removes
private execution artifacts after 30 days, complete safe run directories after
90 days, and retains the three newest release asset directories. A `KEEP` file
inside a run or asset directory exempts it from automatic removal.

After installing schedules, verify the complete host state without printing
credential values:

```sh
canary/host/preflight.sh --require-schedules
```

## Evidence and alerts

The report schema is strict and can be validated independently:

```sh
cargo run --locked -p nan-harness-canary -- \
  validate-report /path/to/report.json
```

The aggregator uses a stable cell identity and failure fingerprint:

```sh
cargo run --locked -p nan-harness-canary -- aggregate \
  --reports /path/to/reports \
  --state /path/to/aggregate-state.json \
  --summary /path/to/summary.json
```

Publication is attempted for every safe positive per-harness result before
aggregation and alerts. Cell or aggregation failures still fail the suite, but
they do not discard independent successful deterministic evidence. Reports and
alerts contain harness/version metadata, digests, bounded statuses, and stable
failure fingerprints only; prompts, responses, tool output, local paths, and
credentials are excluded. Private step logs stay on the Mac and must never be
uploaded or sent through notifications.

Alert transitions:

- first identical failure: stored as suspected, no public issue;
- second consecutive identical failure: private ntfy notification and one
  deterministic GitHub issue;
- later identical failures: state remains confirmed without duplicate issues;
- first success: recovery notification and issue closure.

Tool-inventory drift has an independent transition sequence. Its first
observation sends a private notification while compatibility evidence remains
publishable. A second identical observation opens a maintenance issue, later
identical observations do not duplicate it, and the first matching inventory
closes the issue. Tool names are retained only in private diagnostic logs; safe
reports and issues contain a fingerprint.

Every scheduled run that has a cell, publication, or aggregation failure also
sends one private run-failure notification. This is separate from the
per-cell transition above: an isolated harness failure does not open a public
issue, but it is still visible to the operator; successful independent
evidence is published when feed validation and publication succeed.

Do not attach guest logs to issues. Reproduce the cell locally when more detail
is required.

## UX diagnostics catalog

The same typed messages used by the CLI can be reviewed without forcing real
errors:

```sh
cargo run --locked -p nan-harness-canary -- ux --list
cargo run --locked -p nan-harness-canary -- ux \
  --html /tmp/nan-harness-ux/index.html
```

Setup requirements have no `NH-*` code and never offer telemetry. nan-harness failures
retain a code and use the configured consent-aware reporting path.

## Recovery

If Tart or the host is interrupted:

1. Check `tart list` for a `nan-harness-canary-*` VM.
2. Stop and delete only the stale canary VM.
3. Inspect the safe report and launchd log.
4. Run the exact failed cell manually.
5. Re-enable the launch agent only after the manual cell is green.

If GitHub authentication expires, re-authenticate `gh` interactively before
restarting release or scheduled jobs. Never place a GitHub token in a plist.
