# Compatibility Canary

The nan-harness compatibility canary combines a deterministic source/main
detector with disposable Linux and macOS Tart VMs on a private Apple Silicon
host. It tests all 14 supported harnesses without adding commands to the public
`nan` binary.

GitHub only detects deterministic latest-source regressions and never performs
live provider calls or feed publication. The Mac mini downloads the exact
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

This runbook configures and operates the private Mac mini compatibility canary.
It does not change the public `nan` command surface.

## Prerequisites

- Apple Silicon Mac running macOS 13 or newer.
- At least 100 GB free before downloading Linux and macOS base images.
- Homebrew, Rustup, GitHub CLI with `gh attestation verify` support, Tart,
  OpenSSH, and `sshpass`.
- GitHub CLI authenticated with release, issue, and contents access to this
  repository.
- A current GitHub CLI that can verify `SHA256SUMS` attestations from
  `DavidLMS/nan-harness`'s `.github/workflows/release.yml` for
  `refs/tags/<tag>`; the canary rejects unverified or mismatched assets.
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

Then run one manual installation cell through `nan-harness-canary cell`. A cell
specification is TOML and references the matching release `nan-harness` and
`nan-harness-canary` assets plus the guest helper scripts. The runner clones the
image, starts it headlessly, mounts read-only input and writable output
directories, runs bounded steps over SSH, writes safe evidence, and destroys the
VM.

When `--private-log-dir` is set, raw step output is copied there for local
diagnosis. These logs can contain model or tool output: never upload them to
GitHub, attach them to issues, or send them through ntfy. Host runners use a
private umask so new state and diagnostic files are readable only by the canary
user.

The single-cell wrapper downloads both matching ARM64 asset pairs and runs a
clean deterministic-plus-live probe for one harness. It is a dry run and never
writes the compatibility feed:

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

Install the three user launch agents only after the Tart spike passes:

```sh
export NAN_CANARY_NTFY_URL='https://ntfy.example.com/nan-harness-canary'
canary/host/install-launchd.sh
```

The jobs are:

| Label | Schedule | Work |
| --- | --- | --- |
| `dev.nan-harness.canary-daily` | Daily at 03:17 | All Linux clean installs, doctor checks, deterministic conformance, and two rotating live tool probes |
| `dev.nan-harness.canary-weekly` | Sunday at 04:17 | All Linux and macOS deterministic plus live tool probes |
| `dev.nan-harness.release-gate` | Every 15 minutes | Detect and verify a draft release before initializing feed evidence |

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
Every feed write takes an owner-aware crash-recoverable host lock, validates
non-empty schema-v2 JSON, preserves every prior release record, stages a
uniquely named candidate, keeps a separate validated backup asset, and verifies
or restores the stable `compatibility.json` replacement. An interrupted run
with a missing stable asset restores that backup before continuing.

Run a pending draft gate:

```sh
canary/host/run-release-gate.sh
canary/host/run-release-gate.sh --force
```

The release command exits successfully without work when no draft exists. A
failed suite leaves the draft untouched and waits six hours before retrying the
same tag, preventing an unchanged draft from continuously consuming the Mac.
Use `--force` after correcting a transient host or canary problem. A fully green
release suite publishes the exact release-scoped feed, promotes the draft, and
marks it as latest.

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
