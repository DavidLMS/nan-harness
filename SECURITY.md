# Security Policy

## Supported versions

Security fixes are applied to the latest published release. Pre-1.0 releases
may include breaking changes when a safe fix requires one.

| Version | Supported |
| --- | --- |
| Latest release | Yes |
| Older releases | No |

## Reporting a vulnerability

Use GitHub's
[private vulnerability reporting](https://github.com/DavidLMS/nan-harness/security/advisories/new)
to report a suspected vulnerability. Do not open a public issue for a security
report and do not include API keys, prompts, model output, tool input/output,
credentials, or private configuration.

Include the affected version, platform, harness, impact, reproduction steps,
and any suggested mitigation. Reports should use placeholder credentials and
the smallest safe proof of concept.

You should receive an acknowledgement within three business days. Validated
reports are handled through a private advisory until a fix and coordinated
disclosure are ready.

## Scope

Security reports may cover the nan-harness CLI, protocol bridges, provider routing,
temporary configuration, persistence, updater, installers, telemetry, and
release workflow. Vulnerabilities in an upstream harness should also be
reported to that project's security contact.
