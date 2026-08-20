# Changelog

All notable changes to NaN Harness are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/DavidLMS/nan-harness/compare/v0.0.1...HEAD
[0.0.1]: https://github.com/DavidLMS/nan-harness/releases/tag/v0.0.1
