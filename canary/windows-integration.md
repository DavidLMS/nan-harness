# Windows CLI integration checkpoint

Tracking: [#19](https://github.com/DavidLMS/nan-harness/issues/19).
This is an integration candidate, not release qualification.

## Preserved inputs

- Base: origin/main at 525517143f523e79dcf9decc23fa28600fc0ab98.
- Native Windows runner: feat/windows-cli-diagnostic-final at d07f991.
- Cline correction: fix/cline-headless-startup at 35310d9 (PR #18).
- Source branches, desktop worktrees, uncommitted files and the existing stash
  are untouched. The integration has its own Orca worktree.

## Platform policy

Linux ARM64 and macOS ARM64 retain all 15 CLI harnesses. Windows x64 runs the
13 available harnesses. Prime Agent and FX are explicitly skipped with
official-windows-distribution-unavailable, including when selected directly.
Neither a skip nor an installation failure counts as successful qualification.
Remove the exclusions only when official native distributions can be installed
and verified. An all-platform diagnostic therefore has 43 runnable cells and
two Windows skips.

Use the Hosted CLI compatibility workflow for independent OS/harness cells,
or Native Windows CLI diagnostic for a bounded batch with a safe report.
Both remain opt-in; live mode uses the protected canary-live environment.
A selection containing only unavailable Windows harnesses produces explicit
skip evidence without executing a compatibility cell.

## Reconciled behavior

- Cline JSON launches select the local backend without injected data-dir/yolo.
- Native PowerShell writes must satisfy the unchanged run_commands filesystem
  contract. Tests execute the generated command, including paths with quotes.
- Windows environment isolation and detached-helper handle protection are
  preserved. Owned terminal jobs close descendants before joining pipe readers.
- An inventory-only failure is advisory only with positive process completion
  and no operational failure reasons; failed cleanup/provider work is not drift.
- The Windows canary joins release assets and checksum generation. Adding an
  asset alone does not qualify Windows or modify the publication requirements.

## Remaining acceptance work

1. Pass native CI against the integrated tree, including terminal ownership and
   Cline conformance. Prior branch passes do not substitute for this evidence.
2. Run the supported Windows batch against the exact integration SHA, first
   deterministically and then with live NaN inference; record only closed
   statuses and run links, never prompts, responses or raw harness logs.
3. Investigate any operational failures newly exposed by strict inventory
   classification, particularly DeepSeek Harness.
4. Extend and test the trusted exact-release-asset gate for the 13 Windows
   harnesses after native/live qualification. Until then the mandatory release
   gate remains the existing 30 Linux/macOS cells, not a claimed 43-cell gate.
5. Complete repository gates and review before merging through a PR. Do not
   publish a release or push directly to main during this checkpoint.
