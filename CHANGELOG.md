# Changelog

All notable changes to nan-harness are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.11] - 2026-08-27

### Added

- Qwen 3.8 Flash and GLM 5.3 Flash now receive bundled 1M-context, vision,
  and model-specific reasoning profiles whenever they appear in the
  credential-scoped NaN catalog, while newly discovered unknown models remain
  dynamically usable.

### Changed

- The website now uses shorter workflow copy, an updated page title, and social
  preview metadata for richer link previews.
- Release binaries now use Thin LTO, a single codegen unit, and stripped symbols,
  reducing the measured `nan-harness` binary size by about 36%.
- Startup now refreshes update and compatibility information concurrently,
  reducing launch latency while preserving the existing warning and fallback
  behavior.
- Streaming bridges now parse each upstream SSE frame once, reducing repeated
  JSON work without changing translated events.

### Fixed

- Windows private credential files, managed configurations, temporary launch
  configurations, and telemetry files now use a protected DACL for the current
  process user and `SYSTEM` instead of relying on inherited ACLs; file-backed
  persistence fails closed when hardening cannot be applied.
- Repeated Ctrl+C now force-stops an unresponsive harness without waiting for the
  graceful shutdown period, while interrupts racing a child exit remain normal
  cancellation outcomes.
- Remote provider and GlitchTip endpoints now require HTTPS, while loopback
  HTTP remains available for local development and testing.
- The fx gateway now rejects truncated upstream SSE streams and incomplete tool
  calls instead of emitting a successful finish event.
- Dry-run launch plans now derive bundled-model metadata from the shared catalog,
  so GLM 5.2 is reported consistently as bundled and qualified.

## [0.0.10] - 2026-08-25

### Fixed

- Upstream bridges now bound both the wait for an initial response and inactivity
  between streamed chunks, while allowing healthy long-running streams to continue.
- Responses web-search references are now isolated by session and bounded in
  memory, preventing concurrent sessions from resolving the same ref ID to
  different URLs.
- Anthropic and Responses bridges now reject upstream SSE streams that end before
  the required `[DONE]` marker instead of completing partial text or tool calls.
- Non-fatal automatic update failures are now delivered to GlitchTip when anonymous
  telemetry is enabled, while the requested harness still starts normally.

## [0.0.9] - 2026-08-24

### Fixed

- Unix self-updates now replace the canonical `nan-harness` executable when the
  process was started through the installed relative `nan` command alias.

## [0.0.8] - 2026-08-24

### Added

- GLM 5.2 exposes low, medium, and high reasoning effort where harnesses support
  native reasoning controls.
- Local bridge failures can be reported through the existing GlitchTip telemetry
  flow without including provider responses, prompts, or credentials.
- Error reports use a typed schema v3 with actionable reasons, safe
  family-specific details, normalized model context for model-specific failures,
  and anonymous installation correlation.

### Changed

- Chat bridge requests retry bounded transient transport failures and HTTP 502,
  503, and 504 responses before returning an error to the harness.
- GlitchTip delivery allows ten seconds per attempt and retries one transient
  transport or server failure before retaining the report for later delivery.
- Draft GitHub releases use the matching version section from `CHANGELOG.md` as
  their release notes.
- Telemetry-off installations retain their random diagnostic identifier without
  enabling analytics or automatic delivery, so separately consented reports can
  be correlated safely.

### Fixed

- Clean canaries retry bounded SSH transport failures independently from
  harness-step attempts, including transient macOS authentication drops.
- Release canaries verify Hermes, OpenClaw, and fx native tool-success evidence
  through durable side effects or structured metadata even when those harnesses
  omit the read result from their final visible response.
- Expected `--dry-run` validation failures and development builds without an
  update channel no longer create actionable telemetry reports.
- Claude Code prioritizes GLM 5.2 over Gemma 4 in its curated four-slot picker and
  switches to complete gateway discovery when a credential exposes a model outside
  that verified set, while preserving Qwen 3.6 native Auto mode.
- Codex Plan mode no longer fails when it applies `medium` reasoning to models
  with toggle-only, always-on, or unprofiled reasoning capabilities.
- Disabled telemetry asks for consent at most once for a batch of related launch
  errors, while non-fatal compatibility refresh warnings are reported only when
  telemetry is enabled.
- Local bridge failures no longer attach synthetic HTTP response statuses that
  violate the telemetry contract and silently discard consented reports.
- Telemetry now explains when an internal contract violation prevents a report
  from being prepared safely instead of returning without delivery status.
- Bridge diagnostics use an event queue and are drained during shutdown instead
  of overwriting earlier failures.
- OpenClaw deterministic tool checks accept its punctuation-free call and result
  identifiers while retaining strict tool, argument, result, and filesystem
  assertions.
- The latest-harness detector activates managed Kimi and Hermes installation
  paths in its own process and preserves bounded `doctor` diagnostics in CI logs.
- Release note extraction recognizes dated Keep a Changelog headings and fails
  instead of silently publishing an empty section.

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
- Failed deterministic canaries retain their bounded conformance JSON in the
  private host log so the failing scenario can be diagnosed safely.
- Codex conformance accepts the version-dependent native tools introduced in
  Codex CLI 0.149.0 without weakening the required core inventory.
- Hermes conformance accepts only the declared environment-dependent browser
  tool variants instead of rejecting every non-empty declared variant.
- OpenClaw conformance preserves strict tool-call validation while accepting
  the punctuation-free result identifiers emitted by the current harness.
- DeepSeek Harness live canaries verify a real file-writing side effect instead
  of relying on a read marker that its headless output does not expose.
- Prime Agent canary installations bound and retry every nested download made
  by the upstream installer instead of waiting indefinitely on an unavailable
  endpoint.
- Disposable Tart guests ignore host SSH-agent identities and authenticate only
  with the canary password, preventing macOS connection failures on busy agents.
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

[Unreleased]: https://github.com/DavidLMS/nan-harness/compare/v0.0.11...HEAD
[0.0.11]: https://github.com/DavidLMS/nan-harness/compare/v0.0.10...v0.0.11
[0.0.10]: https://github.com/DavidLMS/nan-harness/compare/v0.0.9...v0.0.10
[0.0.9]: https://github.com/DavidLMS/nan-harness/compare/v0.0.8...v0.0.9
[0.0.8]: https://github.com/DavidLMS/nan-harness/compare/v0.0.7...v0.0.8
[0.0.7]: https://github.com/DavidLMS/nan-harness/compare/v0.0.6...v0.0.7
[0.0.6]: https://github.com/DavidLMS/nan-harness/compare/v0.0.5...v0.0.6
[0.0.5]: https://github.com/DavidLMS/nan-harness/compare/v0.0.4...v0.0.5
[0.0.4]: https://github.com/DavidLMS/nan-harness/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/DavidLMS/nan-harness/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/DavidLMS/nan-harness/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/DavidLMS/nan-harness/releases/tag/v0.0.1
