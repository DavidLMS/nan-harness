# Changelog

All notable changes to NaN Harness are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.6] - 2026-08-21

### Added

- Native Codex configuration profiles apply NaN routing per launch while
  preserving the user's normal configuration, sessions, hooks, and policies.
- Codex model and reasoning selections made through `/model` are remembered for
  later NaN launches without modifying the user's base Codex configuration.

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
- Managed uninstall removes NaN and reverses persistent harness configuration
  created by the installation.

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

- Rust CLI with `nan` as the primary command and `nan-harness` as an installer-provided alias.
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

[Unreleased]: https://github.com/DavidLMS/nan-harness/compare/v0.0.6...HEAD
[0.0.6]: https://github.com/DavidLMS/nan-harness/compare/v0.0.5...v0.0.6
[0.0.5]: https://github.com/DavidLMS/nan-harness/compare/v0.0.4...v0.0.5
[0.0.4]: https://github.com/DavidLMS/nan-harness/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/DavidLMS/nan-harness/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/DavidLMS/nan-harness/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/DavidLMS/nan-harness/releases/tag/v0.0.1
