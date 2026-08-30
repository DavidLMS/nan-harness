# Compatibility Canary

The nan-harness compatibility canary combines a deterministic source/main
detector with disposable Linux and macOS Tart VMs on a private Apple Silicon
host. It tests all 14 supported harnesses without adding commands to the public
`nan` binary.

GitHub only detects deterministic latest-source regressions and never performs
live provider calls or feed publication. The operator host downloads the exact
release-matched `nan-harness` and `nan-harness-canary` ARM64 assets, verifies
the signed `SHA256SUMS` metadata and every required ARM64 checksum before host
execution or guest staging, runs clean installation and doctor checks,
performs deterministic conformance, and runs real `qwen3.6` probes only in the
private scheduled tiers.

| Trigger | Platforms | Coverage |
| --- | --- | --- |
| Source/main detector | Linux x86-64 | Latest installation, doctor, and deterministic conformance for all 14 harnesses; no feed writes |
| Daily scheduled | Linux ARM64 | Clean install, doctor, and deterministic conformance for all 14; exactly two deterministic rotating `qwen3.6` probes |
| Weekly scheduled | Linux and macOS ARM64 | Deterministic conformance plus live `qwen3.6` probes for all 14 on both platforms |
| Release gate | Linux and macOS ARM64 | The same full cross-platform pass; only then initialize both evidence tiers and promote the draft |

Compatibility evidence is release-scoped schema v2. A daily Linux deterministic
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

This runbook configures and operates the compatibility canary host.
It does not change the public `nan` command surface.

## Prerequisites

- Apple Silicon Mac running macOS 13 or newer.
- At least 100 GB free before downloading Linux and macOS base images; the
  preflight requires 50 GB once both images are already cached.
- Homebrew, Rustup, GitHub CLI with `gh attestation verify` support, Tart,
  OpenSSH, and `sshpass`.
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

## Schedules

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
non-empty schema-v2 JSON, preserves every prior release record, stages a
uniquely named candidate, keeps a separate validated backup asset, and verifies
or restores the stable `compatibility.json` replacement. An interrupted run
with a missing stable asset restores that backup before continuing.

Run one pending draft gate explicitly:

```sh
canary/host/run-release-gate.sh --tag vX.Y.Z
canary/host/run-release-gate.sh --tag vX.Y.Z --repo owner/name
canary/host/run-release-gate.sh --tag vX.Y.Z --force
```

The gate refuses an omitted tag, a missing release, or a release that is not a
draft. It runs the orchestration committed in that tag from a temporary detached
worktree and records an atomic per-tag receipt for asset verification, suite
success, feed publication, and promotion. A retry resumes after the last
completed phase, but revalidates the tag and signed assets first.

Only a real suite failure starts the six-hour cooldown. Download, checksum,
attestation, feed, or promotion failures can be retried immediately after
correction. Use `--force` only to bypass a suite cooldown after correcting its
cause. A fully green gate publishes the release-scoped feed, promotes the draft,
and marks it as latest.

## Expected duration and retention

| Operation | Cached duration | Global budget | Purpose |
| --- | --- | --- | --- |
| Manual cell | 2-5 minutes | 60 minutes | Reproduce one harness/platform without publication |
| Daily | 20-30 minutes | 60 minutes | Detect Linux installation and deterministic regressions every non-Sunday day |
| Weekly | 45-60 minutes (20-30 with a validated two-lane host) | 120 minutes | Verify every harness live on Linux and macOS |
| Release gate | 45-60 minutes (20-30 with a validated two-lane host) | 120 minutes | Verify a named draft, publish evidence, and promote it |

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
