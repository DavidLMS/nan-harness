# Compatibility Canary

The NaN Harness compatibility canary combines deterministic GitHub Actions
with disposable Linux and macOS Tart VMs on a private Apple Silicon host. It
tests all 14 supported harnesses without adding commands to the public `nan`
binary.

GitHub runs latest-version installation and native conformance without provider
credentials. The Mac mini runs clean installation and real `qwen3.6` tool
probes, stores typed safe reports, confirms repeated regressions, and promotes
draft releases only after every release cell passes.

| Trigger | Platforms | Coverage |
| --- | --- | --- |
| Daily GitHub | Linux x86-64 | Latest installation and deterministic native conformance for all 14 harnesses |
| Daily local | Linux ARM64 | All clean installs and two rotating live tool probes |
| Weekly local | Linux and macOS ARM64 | Live `qwen3.6` tool probes for all 14 harnesses |
| Release gate | Linux and macOS ARM64 | Every cell blocks publication of the draft release |

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
- Homebrew, Rustup, GitHub CLI, Tart, OpenSSH, and `sshpass`.
- GitHub CLI authenticated with release, issue, and contents access to this
  repository.
- The existing `NAN_API_KEY` exported in the interactive setup shell.
- An unlocked login Keychain whenever launchd starts a VM.

Install Tart and the SSH password helper with the currently supported Homebrew
formulae, then verify the exact commands before enabling launchd:

```sh
brew install cirruslabs/cli/tart
brew install cirruslabs/cli/sshpass
tart --version
sshpass -V
gh auth status
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

Then run one manual installation cell through `nan-canary cell`. A cell
specification is TOML and references a local NaN artifact plus the guest helper
scripts. The runner clones the image, starts it headlessly, mounts read-only
input and writable output directories, runs bounded steps over SSH, writes safe
evidence, and destroys the VM.

When `--private-log-dir` is set, raw step output is copied there for local
diagnosis. These logs can contain model or tool output: never upload them to
GitHub, attach them to issues, or send them through ntfy. Host runners use a
private umask so new state and diagnostic files are readable only by the canary
user.

The single-cell wrapper downloads the latest published NaN artifact and runs a
clean live tool probe for one harness:

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
| `dev.nan-harness.canary-daily` | Daily at 03:17 | All Linux clean installs and two rotating live tool probes |
| `dev.nan-harness.canary-weekly` | Sunday at 04:17 | All Linux and macOS live tool probes |
| `dev.nan-harness.release-gate` | Every 15 minutes | Detect and verify a draft release |

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

Run the published release:

```sh
canary/host/run-scheduled.sh daily
canary/host/run-scheduled.sh weekly
```

Run a pending draft gate:

```sh
canary/host/run-release-gate.sh
canary/host/run-release-gate.sh --force
```

The release command exits successfully without work when no draft exists. A
failed suite leaves the draft untouched and waits six hours before retrying the
same tag, preventing an unchanged draft from continuously consuming the Mac.
Use `--force` after correcting a transient host or canary problem. A fully green
release suite updates the compatibility feed, promotes the draft, and marks it
as latest.

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

Setup requirements have no `NH-*` code and never offer telemetry. NaN failures
retain a code and use the configured consent-aware reporting path.

## Recovery

If Tart or the host is interrupted:

1. Check `tart list` for a `nan-canary-*` VM.
2. Stop and delete only the stale canary VM.
3. Inspect the safe report and launchd log.
4. Run the exact failed cell manually.
5. Re-enable the launch agent only after the manual cell is green.

If GitHub authentication expires, re-authenticate `gh` interactively before
restarting release or scheduled jobs. Never place a GitHub token in a plist.
