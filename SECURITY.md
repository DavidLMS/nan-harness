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

## Local private files

nan-harness treats saved credentials, copied native configuration, launch-scoped
temporary configuration, coordinator state, local diagnostic captures, and
local telemetry state as private data. On Unix,
covered files use owner-only `0600` permissions and private directories use
`0700`. On Windows, each covered file and directory receives a protected DACL
granting full control only to the current process user and `SYSTEM`; directory
inheritance is limited to those principals. This guarantee also applies when a
configuration path is redirected. Protection is applied before any payload is
written; if it cannot be applied, the file-backed operation fails closed and
does not publish a replacement. nan-harness does not grant an ACE to
Administrators; OS-level ownership and recovery powers are separate.

Local diagnostic capture is off by default. When explicitly enabled through
the private troubleshooting interface, it stores unencrypted prompts, model
output, tool data, embedded attachments, and HTTP metadata on the user's
machine. Structured credential and authentication fields are redacted, but the
remaining content can still be highly sensitive. Captures are never uploaded
automatically, have no automatic retention or size limit, and remain until the
user purges them. They must never be attached to issues, committed, or shared
without deliberate review and further sanitization.

## Scope

Security reports may cover the nan-harness CLI, protocol bridges, provider routing,
temporary configuration, persistence, updater, installers, telemetry, and
release workflow. Vulnerabilities in an upstream harness should also be
reported to that project's security contact.
