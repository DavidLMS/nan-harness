# CLI live validation — 2026-09-14

This report records the hosted, manual-only ARM64 CLI validation evidence
available on 2026-09-14. It is an evidence record, not a release, publication,
cutover, or main-branch merge record.

## Result

The evidence covers 29 of 30 real live CLI cells: macOS is 15/15 and Linux is
14/15. Linux Aider remains unresolved after a live completion-marker failure;
the later Aider experiment included a probe edit but does not prove that the
prompt change fixes the failure. The historical deterministic campaign is a
separate 30/30 result and must not be conflated with this live 29/30 result.
The earlier [migration checkpoint](canary-migration-checkpoint-2026-09-14.md)
records a 27/30 deterministic snapshot; that snapshot is historical and is
superseded by the later 30/30 deterministic result, not rewritten in place.

All successful cells completed installation/diagnosis, deterministic
conformance, and the live tool check. The reports contain only bounded metadata
and statuses; raw prompts, model output, tool output, credentials, and private
logs are intentionally excluded.

## Cell matrix

The version in each cell is the harness version recorded by its safe JSON
artifact. A source SHA shown in a cell is the exact NaN Harness checkout used
for that run; these are not interchangeable source identities.

| Harness | Linux ARM64 live evidence | macOS ARM64 live evidence |
| --- | --- | --- |
| `codex` | PASS — `0.154.0`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812055025](https://github.com/DavidLMS/nan-harness/actions/runs/34812055025) | PASS — `0.154.0`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812509130](https://github.com/DavidLMS/nan-harness/actions/runs/34812509130) |
| `claude-code` | PASS — `2.1.270`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812467129](https://github.com/DavidLMS/nan-harness/actions/runs/34812467129) | PASS — `2.1.270`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812509130](https://github.com/DavidLMS/nan-harness/actions/runs/34812509130) |
| `opencode` | PASS — `1.18.30`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812467129](https://github.com/DavidLMS/nan-harness/actions/runs/34812467129) | PASS — `1.18.30`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812509130](https://github.com/DavidLMS/nan-harness/actions/runs/34812509130) |
| `hermes` | PASS — `0.21.2`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812467129](https://github.com/DavidLMS/nan-harness/actions/runs/34812467129) | PASS — `0.21.2`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812509130](https://github.com/DavidLMS/nan-harness/actions/runs/34812509130) |
| `pi` | PASS — `0.85.1`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812467129](https://github.com/DavidLMS/nan-harness/actions/runs/34812467129) | PASS — `0.85.1`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812509130](https://github.com/DavidLMS/nan-harness/actions/runs/34812509130) |
| `omp` | PASS — `18.1.21`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812467129](https://github.com/DavidLMS/nan-harness/actions/runs/34812467129) | PASS — `18.1.21`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812509130](https://github.com/DavidLMS/nan-harness/actions/runs/34812509130) |
| `prime-agent` | PASS — `0.9.4`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812467129](https://github.com/DavidLMS/nan-harness/actions/runs/34812467129) | PASS — `0.9.4`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812509130](https://github.com/DavidLMS/nan-harness/actions/runs/34812509130) |
| `deepseek-harness` | PASS — `0.1.5-rc.1`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812467129](https://github.com/DavidLMS/nan-harness/actions/runs/34812467129) | PASS — `0.1.5-rc.1`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812509130](https://github.com/DavidLMS/nan-harness/actions/runs/34812509130) |
| `openclaw` | PASS — `2026.9.4`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812467129](https://github.com/DavidLMS/nan-harness/actions/runs/34812467129) | PASS — `2026.9.4`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812509130](https://github.com/DavidLMS/nan-harness/actions/runs/34812509130) |
| `cline` | PASS — `3.0.61`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812467129](https://github.com/DavidLMS/nan-harness/actions/runs/34812467129) | PASS — `3.0.61`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812509130](https://github.com/DavidLMS/nan-harness/actions/runs/34812509130) |
| `qwen-code` | PASS — `0.23.3`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812467129](https://github.com/DavidLMS/nan-harness/actions/runs/34812467129) | PASS — `0.23.3`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812509130](https://github.com/DavidLMS/nan-harness/actions/runs/34812509130) |
| `kimi-code` | PASS — `0.42.0`; source `92e7c34768892badd62efc87feceb4ae25b04f4e`; run [34814314118](https://github.com/DavidLMS/nan-harness/actions/runs/34814314118) | PASS — `0.42.0`; source `92e7c34768892badd62efc87feceb4ae25b04f4e`; run [34814783580](https://github.com/DavidLMS/nan-harness/actions/runs/34814783580) |
| `aider` | UNRESOLVED — `0.86.2`; source `28f53e468554cf1b1b55810254ca7fb0b2a894c0`; run [34814488169](https://github.com/DavidLMS/nan-harness/actions/runs/34814488169); install and deterministic passed, live completion-marker exited 1 | PASS — `0.86.2`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812509130](https://github.com/DavidLMS/nan-harness/actions/runs/34812509130) |
| `goose` | PASS — `1.50.0`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812467129](https://github.com/DavidLMS/nan-harness/actions/runs/34812467129) | PASS — `1.50.0`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812509130](https://github.com/DavidLMS/nan-harness/actions/runs/34812509130) |
| `fx` | PASS — `0.0.10`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812467129](https://github.com/DavidLMS/nan-harness/actions/runs/34812467129) | PASS — `0.0.10`; source `ac80891f0316acfbc10eee1b2df1f9a17940e236`; run [34812509130](https://github.com/DavidLMS/nan-harness/actions/runs/34812509130) |

The Linux baseline artifact directory is
`/tmp/nan-run-34812467129.hRYdrz`; the macOS baseline artifact directory is
`/private/tmp/nan-macos-live-vCvbTB`. The later Kimi reports are under
`/tmp/nan-run-34814314118.EGVTE4` and `/tmp/nan-run-34814783580.Tmdqeh`.
The Linux Codex smoke report is in `/tmp/nan-run-34812055025.fFVbUs`, and the
Aider experiment report is in `/tmp/nan-run-34814488169.Ff4COr`. These local
paths are temporary evidence pointers and may expire.

## Workflow and security boundary

`.github/workflows/cli-release-gate.yml` is manual-only. Its dispatch inputs
are `platforms` (`linux`, `macos`, or `both`), `harnesses` (`all` or a
comma-separated list), `mode` (`deterministic` or `live`), `model`, and the
explicit lowercase 40-character `source_ref`. Live mode is accepted only for
an explicit `workflow_dispatch`; the selected source is checked out by exact
SHA before building and running each independent cell.

Live cells use the `canary-live` GitHub Environment. `NAN_API_KEY` remains
GitHub-only: the workflow checks presence before the expensive build, passes it
only to the live cell, and does not expose it to deterministic steps or this
report. There is no main merge, schedule, Tart disablement, cutover, or
Desktop resumption in this evidence.

## Evidence limits and next boundary

The Aider Linux report is a concrete closed diagnostic (`live-tool`,
`live-completion-marker-exit-1`) but does not establish root cause or prove the
subsequent prompt change as a fix. Any rerun or fix review should retain the
exact source identity and inspect only bounded reports. This report does not
claim completion of the broader migration plan.
