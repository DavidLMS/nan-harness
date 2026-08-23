# Changelog

All notable changes to nan-harness are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.7] - 2026-08-23

### Added

- `nan config` can install, inspect, refresh, and safely remove native NaN
  provider configuration for 11 harnesses, including copied saved credentials,
  dynamic model catalogs, and reversible default selections.
- Credential health distinguishes the effective environment key, the key saved
  by nan-harness, and native configurations that still contain an older key.
- `nan doctor --json` provides stable, safe-to-share machine-readable
  diagnostics for support and automated compatibility checks.
- Clean Linux and macOS compatibility canaries cover installation and real tool
  use for every supported harness before a draft release can be promoted.
- Release-scoped schema-v2 compatibility publication records deterministic and
  live evidence per harness with monotonic versions and timestamps.
- The canary UX catalog covers native configuration consent, stale copied keys,
  user-edited files, and credential-aware logout behavior.

### Changed

- Compatibility canary publication now verifies exact-tag release attestations
  and checksums before execution, stages only verified release assets, fails
  closed on unvalidated reports and ambiguous feed reads, and preserves
  recoverable remote candidates and last-known-good feed backups during
  replacement; release-gate cooldowns begin only after asset verification.
- Runtime compatibility refresh rejects empty schema-v2 release lists, and
  compatibility evidence preserves the newer timestamp when a newer version
  advances the same record.
- Compatibility evidence now requires an overall passed report, preserves
  historical release records exactly, and keeps compatibility-release creation
  and updates out of hosted release CI.
- Source/main detector failures and report serialization failures now fail the
  detector workflow while still attempting a safe report for every harness.
- Harness launch commands no longer expose `--persist` or `--unpersist`;
  long-lived native setup is an explicit configuration workflow that never
  launches the harness or copies `NAN_API_KEY` from the environment.
- Successful launch-time API-key verification is cached for one hour, while
  authentication and native-configuration commands always validate against the
  current NaN model catalog.
- `nan auth logout` and `nan uninstall` can remove every configuration managed
  by nan-harness before deleting its saved API key.
- `nan-harness` is the canonical executable and release artifact name; `nan`
  remains the shorter command alias.
- User-facing copy now distinguishes the nan-harness application from the NaN
  provider.
- Scheduled regressions require two consecutive matching failures before a
  public issue is created, and recoveries close the existing issue.
- Manual canary runs are dry runs by default; only scheduled daily, weekly, and
  release-gate wrappers publish validated compatibility-feed updates.

### Fixed

- Release canary specifications preserve jq variables when validating each
  deterministic conformance report.
- Codex conformance accepts the version-dependent native tools introduced in
  Codex CLI 0.149.0 without weakening the required core inventory.
- Qwen Code 0.22.0 keeps `list_directory` available in managed launches and
  native setup while preserving a user's previous tool setting on removal.
- Release installers preserve unrelated `nan` commands and only migrate known
  legacy nan-harness installations.
- Actionable local runtime requirements are presented as setup guidance instead
  of nan-harness errors, do not trigger reports, and include commands to fix and
  retry.
- Automatic update checks refresh cached "no update" results after one hour
  instead of delaying newly published releases for up to a day.

## [0.0.6] - 2026-08-21

### Added

- Native Codex configuration profiles apply NaN routing per launch while
  preserving the user's normal configuration, sessions, hooks, and policies.
- Codex model and reasoning selections made through `/model` are remembered for
  later nan-harness launches without modifying the user's base Codex
  configuration.

### Fixed

- DeepSeek Harness dry runs no longer require Node.js, and post-install runtime
  verification no longer writes state into the user's home directory.

## [0.0.5] - 2026-08-21

### Added

- Public landing page and documentation site with an accessible, continuously
  scrolling harness selector.

### Changed

- Kimi Code compatibility now covers its 0.38 configuration and tool behavior.
- Website installation guidance is shorter and harness branding uses canonical
  project assets.

### Fixed

- Installer downloads now report failures clearly and use bounded retries
  instead of potentially waiting indefinitely.
- Codex launches from the user's home directory isolate project-local Codex
  configuration without discarding existing user settings.

## [0.0.4] - 2026-08-20

### Fixed

- Usage telemetry distinguishes harness launches from local management
  operations without collecting prompts, outputs, or tool payloads.

## [0.0.3] - 2026-08-20

### Added

- Guided `NAN_API_KEY` onboarding securely stores credentials for later runs.
- `nan doctor` can produce a safe whole-system diagnostic report for support.
- Managed uninstall removes nan-harness and reverses persistent harness
  configuration created by the installation.

### Changed

- CLI harness names align with their executable commands, and the internal
  launch-plan validation command is no longer exposed to users.

### Fixed

- Harness version detection retries transient process failures before reporting
  an error.

## [0.0.2] - 2026-08-20

### Fixed

- Compatibility metadata refresh follows trusted redirects from GitHub release
  assets.
- Release version synchronization includes the repository maintenance tool.

## [0.0.1] - 2026-08-20

### Added

- Rust CLI with `nan-harness` as the project command and `nan` as its short alias.
- NaN routing for Claude Code, Codex, OpenCode, Hermes, Pi, Prime Agent,
  DeepSeek Harness, OpenClaw, Cline, Qwen Code, Kimi Code, Aider, Goose, and fx.
- Dynamic NaN model discovery, model selection, compatibility diagnostics, and
  reversible persistent configuration where supported by each harness.
- Anthropic Messages, OpenAI Responses, and fx AI Gateway protocol bridges.
- Supervised process lifecycle, private temporary overlays, secret redaction,
  resumable native harness behavior, and deterministic tool conformance tests.
- Opt-in usage and error telemetry with one-time error reporting when telemetry
  is disabled.
- Signed release attestations, checksum-verified installers, native self-update,
  startup update prompts, daily compatibility canaries, and a monotonic remote
  verification feed.

[Unreleased]: https://github.com/DavidLMS/nan-harness/compare/v0.0.7...HEAD
[0.0.7]: https://github.com/DavidLMS/nan-harness/compare/v0.0.6...v0.0.7
[0.0.6]: https://github.com/DavidLMS/nan-harness/compare/v0.0.5...v0.0.6
[0.0.5]: https://github.com/DavidLMS/nan-harness/compare/v0.0.4...v0.0.5
[0.0.4]: https://github.com/DavidLMS/nan-harness/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/DavidLMS/nan-harness/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/DavidLMS/nan-harness/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/DavidLMS/nan-harness/releases/tag/v0.0.1
