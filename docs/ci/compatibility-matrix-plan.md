# Compatibility evidence matrix

The compatibility feed is schema version 2 and is keyed first by the exact
`nanHarnessVersion` release, then by harness identifier. Every evidence pair is
monotonic: a lower harness version or older timestamp can never replace an
existing value. Release records remain in the feed when later releases are
published.

## Verification tiers

| Tier | Environment | Required checks | Feed effect |
| --- | --- | --- | --- |
| Source/main deterministic | GitHub Linux x86-64 | Latest harness install, doctor, and deterministic conformance for all 14 | Detector reports only; no feed write, no live provider call |
| Daily deterministic | Tart Linux ARM64 | Clean install, doctor, and deterministic conformance for all 14 | A passed harness may advance only its compatible pair |
| Daily live rotation | Tart Linux ARM64 | The daily deterministic checks plus `qwen3.6` live probe for exactly two deterministic rotating harnesses | Live evidence is not advanced by this tier |
| Weekly live | Tart Linux and macOS ARM64 | Deterministic and `qwen3.6` live checks for all 14 on both platforms; observed versions must match per harness | A harness may advance both pairs only after both platforms pass |
| Release gate | Tart Linux and macOS ARM64 | The weekly checks for the draft release, for all 14 harnesses | Both pairs are initialized only after the complete cross-platform pass; then the draft is promoted |

The daily rotation uses the UTC day number and two adjacent positions in the
stable 14-harness ordering. A live-step failure does not erase an independent
successful deterministic result from another cell or from the same report.

## Artifact and security boundary

The Mac host downloads the matching release pair for each ARM64 target and the
release `SHA256SUMS` metadata:

- `nan-harness-aarch64-unknown-linux-musl` and
  `nan-harness-canary-aarch64-unknown-linux-musl` for Linux guests;
- `nan-harness-aarch64-apple-darwin` and
  `nan-harness-canary-aarch64-apple-darwin` for macOS guests and the host
  orchestrator.

Before any host execution or guest staging, `gh attestation verify` must accept
`SHA256SUMS` from `DavidLMS/nan-harness`, signed by
`.github/workflows/release.yml` for `refs/tags/<tag>`. The four ARM64 binaries
are then checked against that exact-tag manifest. A current GitHub CLI with
artifact-attestation verification is therefore a canary prerequisite.

The canary runner and guest helpers are release assets, not locally rebuilt
substitutes. GitHub Actions is limited to the deterministic source/main
detector. It must not receive `NAN_API_KEY`, execute hosted live calls, or
publish a compatibility feed. The only NAN API key is supplied by the Mac host
environment during setup and retained in the login Keychain; it must not enter
reports, images, logs, artifacts, issues, or notifications. Safe reports contain
versions, digests, statuses, durations, and stable failure fingerprints only.

## Publication contract

Manual invocations are dry runs by default. `run-manual.sh` and direct
`run-suite.sh` calls may create a validated local candidate at
`compatibility.json`, but they do not upload it unless an operator explicitly
passes `--publish-feed`. Scheduled daily, weekly, and release-gate wrappers
explicitly pass `--publish-feed`.

Publication performs the following checks under an exclusive host lock:

1. collect every safe positive per-harness update that satisfies the tier;
2. prove the compatibility release and asset state, then download the
   established feed; seed only after a proven first publication or a successful
   schema-v1 migration;
3. merge updates into the exact release record without dropping other release
   records or regressing versions/timestamps;
4. validate non-empty JSON and atomically replace the local candidate before
   staging a uniquely named remote candidate and a separate last-known-good
   backup asset;
5. replace and re-download the stable `compatibility.json` asset once, then
   restore the backup if replacement or verification fails. A stale owner lock
   is recovered only after the recorded local process is no longer alive.

Publication is attempted before aggregation and alerts. The suite still fails
when any cell or aggregation fails, and independent successful updates remain
eligible for publication.

## Operational commands

Prepare the private host and keychain entry:

```sh
export NAN_API_KEY='<existing-key>'
cargo run --locked -p nan-harness-canary -- setup
canary/host/spike-tart.sh
canary/host/install-launchd.sh
```

Run the scheduled tiers (these publish on successful positive evidence):

```sh
canary/host/run-scheduled.sh daily
canary/host/run-scheduled.sh weekly
canary/host/run-release-gate.sh --force
```

Run a dry-run cell and inspect safe output:

```sh
canary/host/run-manual.sh codex linux
cargo run --locked -p nan-harness-canary -- validate-report \
  '/path/to/report.json'
cargo xtask validate-compatibility-feed \
  '/path/to/compatibility.json'
```

Private logs may contain model or tool output and remain local for diagnosis.
Do not attach them to issues or send them through notifications.
