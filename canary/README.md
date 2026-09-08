# Compatibility Canary

The compatibility canary tests all 15 CLI harnesses on disposable GitHub-hosted
Linux and macOS ARM64 runners. The existing source/main detector remains
independent. Desktop checks are manual and do not block release publication.
Tart remains a manual emergency execution backend, not a second publisher.

The hosted workflows still require native qualification and operator setup
before operational cutover; local contract tests do not certify real apps or
live provider behavior on those runners.

Hosted cells download the exact release-matched `nan-harness` and
`nan-harness-canary` ARM64 assets and verify the attested checksum manifest,
source tag, source commit, and binaries before execution. Every harness has a
fresh runner. Installation and deterministic stages receive no provider key;
only the live step receives `NAN_API_KEY`. Child process output stays private,
including failures and timeouts. Only validated bounded reports are uploaded.

Deterministic compatibility covers installation, launch, provider traffic,
process cleanup, sentinel behavior, and one representative tool round-trip.
The complete native tool inventory is observed for maintenance but does not
block compatibility when those functional contracts pass.

| Trigger | Platforms | Coverage |
| --- | --- | --- |
| Source/main detector | Linux x86-64 | Latest installation, doctor, and deterministic conformance for all 15 harnesses; no feed writes |
| Manual daily coverage | Linux ARM64 | Clean install, doctor, and deterministic conformance for all 15; exactly two rotating `qwen3.6` probes; evidence only |
| Manual weekly coverage | Linux and macOS ARM64 | Deterministic conformance plus live `qwen3.6` probes for all 15 on both platforms; evidence only |
| Release gate | Linux and macOS ARM64 | The same full cross-platform pass; only then initialize both evidence tiers and publish the draft |

Compatibility evidence is release-scoped, with v2/v3 legacy readers and v4
independent, exact-platform Desktop checks. A daily Linux deterministic
pass can advance only that harness's `lastCompatibleVersion` and `compatibleAt`.
Weekly live evidence advances only when Linux and macOS deterministic and live
checks pass for the same observed harness version. Release-gate publication is
all-or-nothing for the release's initial two evidence tiers, while preserving
older release records.

Safe reports follow
[`crates/nan-harness-canary/resources/canary-report.schema.json`](../crates/nan-harness-canary/resources/canary-report.schema.json).
They include versions, digests, bounded check outcomes, durations, and a stable
failure fingerprint. They exclude credentials, prompts, responses, tool
payloads, command output, and local paths. Raw output is retained only in
private local logs when explicitly requested.

## Operations

Normal operation resides in GitHub Actions:

1. Configure `NAN_API_KEY` for the restricted `canary-live` environment. No key
   is added to workflow YAML, report files, or repository state.
2. Review protection rules for `compatibility-publication` and the data-only
   `compatibility-state` branch. The writer requires contents write; test jobs
   remain read-only. Do not execute or check out the state branch as code.
3. Run **Approve compatibility evidence**, operation `initialize`, once to
   create the empty data branch. Missing or unreadable state thereafter fails
   closed; it is never silently recreated.
4. The release workflow creates its draft, then calls **Hosted CLI compatibility
   gate** directly. All thirty deterministic/live cells and matching harness
   versions are required before a durable publication request is committed.
5. **Compatibility publication writer** drains approved requests under one
   repository-wide mutex. It can also be run manually to retry pending work.

The state branch holds immutable `requests/`, `completed/` acknowledgements,
monotonic `receipts/`, and `recommendations/` bound to tag, commit, checksum-manifest digest, and
sanitized suite evidence. Compare-and-swap Git updates preserve concurrent
enqueues. Requests are persisted before workflow concurrency applies, so a
replaced pending workflow cannot lose an approval. A cancelled publication
resumes from its receipt; unchanged attested assets and durable passed suite
evidence avoid repeating model calls.

For Desktop, run the checker workflow or review a contributed issue. Select
`desktop-issue` with its issue number and exact report SHA-256, or `desktop-run`
with its run ID, artifact name, and SHA-256. The workflow freezes those exact
bytes, validates the report and official binary identity, and enqueues it.
No author allowlist is required: launching the review workflow is approval.
Issues never trigger code execution or publication. Empty positive evidence
is a recorded no-op, not a certification.

Use the local wrappers from any authenticated machine to dispatch and wait for
the matching hosted run:

```sh
canary/host/run-release-gate.sh --tag vX.Y.Z
canary/host/recommend-release.sh --tag vX.Y.Z
```

Publication remains separate from recommendation: successful gates publish
with `--latest=false`; only explicit recommendation moves GitHub's latest
pointer. No new schedule is installed. Manual daily/weekly coverage produces
reports without automatically updating compatibility.

### Tart emergency switchover

First disable `release.yml`, `cli-release-gate.yml`,
`compatibility-approve.yml`, and `compatibility-publisher.yml` in GitHub and
wait for their active/queued runs to finish. Then use
`NAN_CANARY_WRITER=tart-emergency` with the existing local gate or recommendation
command. Each entrypoint verifies that those hosted workflows are explicitly
disabled and idle, restores the same durable receipt, and checkpoints gate
progress back to the state branch. An uncertain API response blocks writing.
Do not re-enable hosted writers until local publication has exited. This is
an operational handover, never an automatic fallback.

The remaining Tart prerequisites, VM diagnostics, and private retention
procedures apply only to that manual alternative. They do not change the
public `nanh` command surface or configure the hosted runners.

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

In Tart mode the key is read by the Mac host and injected into an in-memory live-step
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

## Tart spike

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

## Retired Tart schedules

`run-scheduled.sh` no longer performs work by default. Do not install new
launch agents. After qualifying the hosted gate, an operator can remove the
old daily/weekly agents without deleting VM images or history:

```sh
canary/host/uninstall-launchd.sh
```

Logs and state default to:

```text
~/Library/Application Support/nan-harness-canary
```

Set `NAN_CANARY_STATE_DIR` for a different private emergency state location.

## Manual suites

Use the manual hosted workflow for daily/weekly coverage. In an explicit Tart
emergency, direct `run-suite.sh` and
`run-manual.sh` invocations do not. To publish a manually prepared suite, pass
`--publish-feed` explicitly to `run-suite.sh` after reviewing its safe reports.
The publication boundary requires an executable report validator and runs its
complete `validate-report` command for every report before applying policy
checks.
Every feed write takes an owner-aware crash-recoverable host lock, validates
non-empty JSON at its own schema, preserves every prior release record, stages a
uniquely named candidate, keeps a separate validated backup asset, and verifies
or restores the stable replacement. The publisher writes three assets under that
one lock: the legacy CLI-only `compatibility.json` first, then the unified
`compatibility-v3.json`, then `compatibility-v4.json`, preserving independent
Desktop checks. An interrupted run
with a missing stable asset restores that backup before continuing.
The [compatibility feed reference](compatibility-feed.md) describes these
schemas and what published evidence may change. After the
stable asset is verified, the publisher removes staged candidates and retains
the three newest backups. Cleanup failures do not invalidate a verified feed
and are retried by the next successful publication.

Run one pending draft gate explicitly:

```sh
canary/host/run-release-gate.sh --tag vX.Y.Z
canary/host/run-release-gate.sh --tag vX.Y.Z --repo owner/name
canary/host/run-release-gate.sh --tag vX.Y.Z --force
```

The backend gate refuses an omitted tag, a missing release, or a release that is not a
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

In Tart mode only a real suite failure starts the six-hour cooldown. Download, checksum,
attestation, feed, or publication failures can be retried immediately after
correction. Use `--force` only to bypass a suite cooldown after correcting its
cause.

A fully green gate publishes the release-scoped compatibility feed, publishes
the draft as a public release that is explicitly **not** latest, and copies that
tag's attested `update-manifest.json` into the standing `available` release.
That feed is what an explicit `nan-harness update` reads, so a validated release
is installable on request as soon as the gate finishes.

The gate never recommends. GitHub's `latest` release stays the recommended one,
which is what startup discovery, both installers, and older clients follow.
Recommend a published release explicitly:

```sh
canary/host/recommend-release.sh --tag vX.Y.Z
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

This lock is **local**. Actions supplies the common outer writer mutex; after an
explicit switchover it serializes writers on the single emergency host. It
provides no cross-host atomicity. A lock whose note records
another host is refused rather than reclaimed, so the boundary fails closed, but
a writer on another machine — or a manual `gh release edit` — is outside the
protocol. Never operate hosted and emergency writers concurrently.

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
| Release gate | 45-60 minutes (20-30 with a validated two-lane host) | 120 minutes | Verify a named draft, publish evidence, and publish it |

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
5. Resume the manual cell only after cleanup; do not re-enable retired schedules.

If GitHub authentication expires, re-authenticate `gh` interactively before
restarting a manual operation. Never place a GitHub token in a plist.
