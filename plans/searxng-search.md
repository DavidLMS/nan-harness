# Optional SearXNG web search

Status: **Integrated on `DavidLMS/searxng-search`; documentation follow-up.**
The `main` branch is not changed by this work.

## Scope

Document the optional SearXNG backend and its public lifecycle without changing
Rust code or manifests. The user-facing contract covers:

- explicit `nanh search setup --local`, `--docker`, and `--url https://...`
  modes;
- `status --json`, `disable`, `update`, and `remove` lifecycle commands;
- automatic, forced, and disabled launch policy, while preserving an existing
  search provider;
- privacy semantics for the endpoint, NaN credential, and explicit opt-in;
- isolated manual checks, including Windows executable and platform limits.

## Agent deliveries

| Delivery | Artifact or boundary | Result |
| --- | --- | --- |
| Search contracts and client | `nan-harness-search` and runtime search policy | Present on the integrated branch; untouched by this documentation work |
| Lifecycle management | `nanh search` setup/status/disable/update/remove | Present on the integrated branch; untouched by this documentation work |
| Backend recipes | [Docker contract](../docs/viability/searxng-docker.md) and [Windows recipe](../docs/viability/searxng-windows.md) | Linked and described with their contract-only limits |
| Public documentation | [README](../README.md), [CHANGELOG](../CHANGELOG.md), and [manual runbook](../docs/viability/searxng-manual-test.md) | Added in this documentation change |

## Verification evidence

- The CLI source and deterministic tests were inspected to match the public
  command names, backend modes, URL rules, and `NAN_HARNESS_CONFIG_DIR`
  isolation override.
- `cargo test --locked -p nan-harness-cli --all-features search` passed: 26
  matching unit tests, 5 CLI search tests, and 1 search MCP test passed; the
  live-provider checks remained ignored as designed.
- The isolated state-only runbook sequence passed with
  `NAN_HARNESS_CONFIG_DIR`, `NAN_NO_UPDATE_CHECK=1`, and
  `NAN_NO_COMPATIBILITY_CHECK=1`: status started unconfigured, remote setup
  canonicalized the example URL, and disable returned status to unconfigured.
- `git diff --check` passed against the final documentation tree.
- Relative documentation links are checked against the repository tree.

## Limitations

- No live Docker daemon, local SearXNG process, HTTPS endpoint, or Windows host
  is used as evidence for this documentation change.
- The CLI now uses the common installer and supervisor for Windows x64 local
  search. The earlier standalone Windows recipe remains a separate contract-only
  API. Native Windows execution requires its own recorded validation.
- The feature is documented on `DavidLMS/searxng-search`; `main` remains
  unchanged until the integrated branch is deliberately merged.
