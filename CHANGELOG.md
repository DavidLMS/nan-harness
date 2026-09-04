# Changelog

All notable changes to nan-harness are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Managed request coordination now uses a global protocol-v2 capacity window
  with foreground/background priority and separates model discovery from
  inference capacity learning.
- Managed provider requests wait up to 60 minutes for coordinated capacity
  without falling through to an uncoordinated request. Initial-response and
  stream-inactivity budgets remain independent, and Responses clients receive
  protocol progress while queueing or waiting on upstream work.
- Provider capacity starts at two concurrent requests, grows gradually to a
  maximum of ten, and preserves temporary growth penalties across restarts.

### Fixed

- Responses streams that stall, truncate, or finish with reasoning but no
  visible output or tool call are retried up to twice with coordinated backoff
  and a temporary capacity reduction, without exposing incomplete reasoning,
  then fail with a typed protocol event if recovery is exhausted.
- Long Responses reasoning phases emit protocol-level progress events so Codex
  does not mistake active upstream work for an idle SSE connection.
- Responses, Anthropic, and fx streams now report `[DONE]` as a successful
  coordinator attempt instead of appearing as abandoned requests.
- Timeout and empty-response diagnostics retain sanitized phase, retry, and
  priority context for GlitchTip without including request contents.
- Repeated empty completions with the same provider response ID make one final
  cache-bypassing attempt with a unique recovery instruction in the provider
  request body.
- Initial-response timeouts and transport disconnects retry within the managed
  request before they are surfaced to the harness.

## [0.0.20] - 2026-09-04

### Added

- Managed bridge traffic is now coordinated per user, so concurrent harnesses
  share upstream provider requests instead of competing for them. The
  coordinator adapts to observed provider behavior and bounds its retries, and
  it fails open to direct local requests whenever it is unavailable, so a
  coordinator problem never blocks a launch.
- A private, opt-in diagnostic capture can now record bridge traffic locally
  when troubleshooting, with explicit enable, disable, status, and purge
  steps. It stays disabled by default, redacts structured credential and
  authentication fields, and never uploads anything. Captures remain on the
  machine until they are purged, and the recorded prompts, model output, tool
  data, and attachments stay sensitive: review them before sharing.

## [0.0.19] - 2026-09-03

### Added

- Added an experimental, launch-only Zed desktop integration backed by the
  authenticated Chat Completions gateway and reversible JSONC settings. Zed
  1.18.0 is live-verified on macOS; Linux and Windows remain contract-only, and
  the integration remains outside the stable release surface.

### Fixed

- Upstream requests now allow 90 seconds for the initial response and two
  minutes between response chunks, reducing premature disconnections during
  extended model reasoning.
- Model preferences now preserve selections for newer harnesses and
  experimental desktop integrations when another selection is saved during a
  staged upgrade.

## [0.0.18] - 2026-09-03

### Added

- GLM 5.3 now has a bundled 1M-context multimodal profile with low, medium,
  and high reasoning-effort controls.

### Changed

- The shorter public command alias is now `nanh`; `nan-harness` remains the
  canonical executable and product name, and the previous `nan` alias is no
  longer installed. Existing managed `nan` receipts remain uninstallable, and
  rerunning the installer migrates only aliases that point to nan-harness while
  preserving unrelated `nan` commands.
- Claude Code 2.1.243 and newer now show explicit standard and 1M-context
  variants for eligible discovered models, with standard context remaining the
  default.
- DeepSeek V4 Flash now exposes and forwards its newly available vision input
  across supported harnesses.

### Fixed

- Release canary live probes now invoke the installed `nanh` alias.

## [0.0.17] - 2026-09-02

### Added

- Interactive managed harness sessions now show rotating startup and successful-session messages.

### Changed

- Managed harness launch announcements now focus on harness and model selection,
  while newer-version compatibility warnings lead with the detected and confirmed
  versions before a rotating nerd-culture sign-off.
- Explicit native headless modes now suppress interactive wrapper prompts and
  session messages even when launched from a terminal.
- Managed harness and Desktop sessions now show a compact community-style token
  usage summary with per-model totals when more than one model is used.

### Fixed

- Canary compatibility validation now treats tool-inventory drift as advisory
  when the harness still passes its required runtime scenarios, avoiding
  unnecessary forward-compatibility warnings after non-breaking tool changes.

## [0.0.16] - 2026-09-01

### Added

- Added experimental Pen Desktop support on macOS, Windows, and Linux. `nan pen`
  injects the account's current text-model catalogue through an authenticated
  launch-scoped Chat Completions gateway, restores Pen's prior configuration on
  exit, reports token usage, and supports receipt-backed recovery. `nan config
  pen` provides an optional persistent native provider lifecycle.

### Fixed

- Codex conformance now accepts `update_plan` as version-dependent, matching
  Codex 0.152.0 while preserving coverage for versions that expose the tool.
- Text-model discovery and gateway `/v1/models` responses now exclude
  `minimax-h3`, a text-to-video model, using the same central non-coding filter
  as Whisper, embeddings, reranking, speech, and image generation models.

## [0.0.15] - 2026-08-31

### Added

- Added Oh My Pi (OMP) 18.0.11 as the fifteenth supported harness, including
  managed and native NaN routing, interactive installation, pinned
  conformance, canary coverage, and an authenticated-provider-aware
  `web_search` fallback to NaN.
- `nan chatgpt-desktop` now supports the official ChatGPT Desktop app for
  Linux (preview, `.deb` and `.rpm` packages) using the same Responses bridge,
  managed profile, and recovery contracts as macOS and Windows, with
  contract-only compatibility evidence pending a live Linux verification.

### Fixed

- Direct Chat Completions harnesses now attribute provider-reported token usage
  to the model requested on each turn, so in-session model changes are shown
  separately in the final usage summary.

## [0.0.14] - 2026-08-31

### Added

- Added visible experimental previews for ChatGPT Desktop (`codex-desktop` as an
  alias), Claude Desktop, and Hermes Desktop, with inert dry runs, isolated
  receipt-backed recovery, independent model memory, local compatibility
  evidence, Desktop-safe telemetry identity, and provider usage summaries.
- `nan doctor` now reports experimental Desktop harnesses separately from the
  14 stable harnesses, and `nan config hermes-desktop` is an exact alias for the
  existing shared Hermes native configuration lifecycle.

### Fixed

- OpenClaw 2026.8.1 conformance now tracks its current built-in tool inventory
  and environment-bound tool behavior.
- Installer lifecycle checks now keep every Desktop home and configuration path
  inside their temporary test environment and ignore unrelated running apps.
- Pi and Prime Agent now defer automatic NaN `web_search` registration until
  package extensions have loaded, preventing conflicts with `pi-web-access` or
  any other package that exposes the same tool; forced search retains NaN
  precedence, and native configurations migrate from the managed search MCP.

## [0.0.13] - 2026-08-30

### Added

- Supported harnesses now receive an authenticated NaN web-search fallback when
  no recognized search provider is configured, with `--no-search` and
  `--force-search` overrides for managed launches.
- `nan completions <shell>` now generates completion scripts for Bash, Zsh,
  Fish, and PowerShell directly from the CLI definition.

### Changed

- Native `nan config` setup now applies and records the same web-search policy,
  preserving user-owned providers and reusing an existing managed NaN search.
- CLI help now provides quickstart examples, wraps to the terminal, suggests
  nearby commands for typos, and replaces the terse bare-`nan` error with the
  full help while preserving its exit status.
- Harnesses now remember successful model selections independently, validate
  explicit choices against one live catalog with clear warnings, safely recover
  vanished implicit choices, and show actionable model-selection guidance.
- Harness launches now reuse one credential-scoped model catalog snapshot across
  credential verification, runtime preparation, and same-launch fallback attempts.
- Streaming bridges now share fail-closed typed SSE parsing, while fx avoids
  retaining unused response text or cloning JSON deltas.
- Fx reasoning-effort forwarding now follows catalog policy instead of model
  family names.
- Release verification now reuses the exact successful `main` CI result,
  shards pinned harness conformance, and can run the Linux and macOS
  compatibility lanes concurrently from suite-local bootstrapped Tart images
  while preserving the complete 28-cell gate.

### Fixed

- Pinned release conformance now recognizes DeepSeek Harness's managed
  `web_search` tool, permits only Cline's required npm install scripts, and
  refreshes its binary cache after platform dependencies are available.
- Release conformance now accepts the explicitly named managed-search MCP tool
  when OpenCode or Kimi Code connects it before the first model request, and
  records safe inventory diagnostics in private canary logs.
- Search transports now keep model discovery independent from search policy and
  enforce query, MCP-message, and chunked-response limits before buffering
  oversized payloads.
- Saved credentials and verification receipts now repair loosened private-file
  protection before reading, and fail closed when that repair cannot be applied.
- Private runtime configuration directories are now created with owner-only
  protection before any launch data can be written into them.
- Model discovery now rejects successful catalog responses larger than 1 MiB
  and bounds provider error bodies before producing safe diagnostics.

## [0.0.12] - 2026-08-29

### Added

- Harness sessions now finish with a local `stderr` summary of provider-reported
  input and output tokens, grouped by the model that served each request and
  marked when the available totals are partial.
- OpenAI Chat Completions harness commands now provide `--no-chat-gateway` as a
  diagnostic escape hatch for launching without the local gateway.
- `nan doctor --json` now reports the discovered coding-model capabilities and
  the text report includes a compact model catalog.
- Harness launches now announce the selected model and reasoning state, while
  non-zero exits suggest running `nan doctor` for setup diagnostics.
- Interactive API-key onboarding now links to https://nan.builders/ and suggests
  starting the first harness with `nan pi` after a successful save.

### Changed

- Pinned conformance now covers Claude Code 2.1.251 (including `ListAgents`)
  and fx 0.0.7's revised native tool inventory.
- Harnesses that speak OpenAI Chat Completions now connect through an
  authenticated loopback gateway by default, keeping the provider credential in
  nan-harness and enabling gateway-level usage accounting.
- `nan doctor` now discovers all harnesses concurrently with bounded worker
  execution while preserving stable output ordering.
- Compatibility canaries now separate scheduled health checks from an explicit,
  resumable release gate with bounded execution, signed-asset revalidation,
  private failure notifications, host preflight checks, and local retention.

### Fixed

- Session shutdown now waits for every in-flight provider response to confirm
  usage or be recorded as incomplete before printing the final summary.
- Pi installation on macOS now prefers Homebrew's Node.js in the installer
  subprocess when another Node version manager shadows Homebrew on `PATH`.
- Harness launches and doctor checks now detect an inaccessible terminal working
  directory before discovery or credential setup, show clear recovery guidance,
  and attach only fixed allowlisted guidance to diagnostic reports.

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

[Unreleased]: https://github.com/DavidLMS/nan-harness/compare/v0.0.20...HEAD
[0.0.20]: https://github.com/DavidLMS/nan-harness/compare/v0.0.19...v0.0.20
[0.0.19]: https://github.com/DavidLMS/nan-harness/compare/v0.0.18...v0.0.19
[0.0.18]: https://github.com/DavidLMS/nan-harness/compare/v0.0.17...v0.0.18
[0.0.17]: https://github.com/DavidLMS/nan-harness/compare/v0.0.16...v0.0.17
[0.0.16]: https://github.com/DavidLMS/nan-harness/compare/v0.0.15...v0.0.16
[0.0.15]: https://github.com/DavidLMS/nan-harness/compare/v0.0.14...v0.0.15
[0.0.14]: https://github.com/DavidLMS/nan-harness/compare/v0.0.13...v0.0.14
[0.0.13]: https://github.com/DavidLMS/nan-harness/compare/v0.0.12...v0.0.13
[0.0.12]: https://github.com/DavidLMS/nan-harness/compare/v0.0.11...v0.0.12
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
