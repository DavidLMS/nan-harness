# Local Tart retirement record — 2026-09-14

This record documents the local Tart retirement after the hosted release gate
was integrated into `main`. It is a local operational receipt, not release
publication evidence; the hosted run was verification-only and did not invoke
the trusted publisher.

## Validation retained before cleanup

- Hosted run: [34830826982](https://github.com/DavidLMS/nan-harness/actions/runs/34830826982)
- Workflow commit: `89accfe19da487dcbd9a6b9d8b5e667eb33674c4`
- Release tag: `v0.1.6`, resolved annotated-tag commit:
  `34b74e57705976577cfe00d0db5ff0107eea80d6`
- Inputs: `mode=live`, `verification_only=true`, `model=qwen3.6`
- Result: 30/30 real ARM64 cells passed (15 Linux and 15 macOS), including
  `install-and-diagnose`, `deterministic-conformance`, and `live-tool` in
  every report; aggregate evidence validation passed.
- Asset evidence: the four canonical ARM64 release assets and signed
  `SHA256SUMS` were verified. The handoff recorded 30 reports and the exact
  workflow, tag, source, and run identities.
- Publication boundary: the trusted publisher job was skipped. No release,
  compatibility feed, available-release feed, or recommendation was mutated by
  this run. The existing public, non-prerelease `v0.1.6` release remains the
  release under inspection.

## Pre-removal inventory

The inspected repo-owned launch agents were:

- `~/Library/LaunchAgents/dev.nan-harness.canary-daily.plist`
- `~/Library/LaunchAgents/dev.nan-harness.canary-weekly.plist`

Both plists pointed to the repository's scheduled runner and the
`nan-harness-canary` state root. The launchd labels were loaded for the current
user. `dev.nan-harness.release-gate.plist` was absent. No Tart VM was listed by
`tart list`; no Tart or nan-harness process was running. The shared Tart binary
`/opt/homebrew/bin/tart` was present at version `2.32.1` and was not removed.

The Tart storage directory `~/.tart` existed but was empty (`0B`, including
`cache`, `tmp`, and `vms`), so there was no positively identified repo-owned VM
or disposable Tart storage to remove. The repo state/evidence directory
`~/Library/Application Support/nan-harness-canary` occupied approximately
`3.0G`, including release assets, reports, receipts, failed-gate diagnostics,
and historical runs; it was preserved to retain evidence.

## Retirement action and receipt

The repository uninstall helper was used for the exact canary labels above;
the absent release-gate label remained absent. No VM, shared Tart binary,
shared cache, release evidence, credential, unrelated worktree, or user main
checkout was removed. Space freed: `0B` of Tart VM/cache storage; the launchd
plist files and loaded schedules were retired.

No recovery copies of the generated plists were created: their exact paths are
absent, and the tracked `canary/launchd/*.plist.in` templates remain available
if a separately approved recovery is ever needed.

After retirement, `launchctl list` no longer reports either canary label and
the two plist paths are absent. Any future local Tart recovery procedure is
historical only; the supported qualification route is the manual hosted
release gate. A later task may remove or archive obsolete local Tart scripts
after a separate review, but this record does not delete historical diagnostics
or operational evidence.
