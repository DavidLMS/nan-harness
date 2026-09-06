# Agent guidance

Build for the [NaN builders community](https://nan.builders/): dependable
provider routing and compatibility with existing coding harnesses. Preserve
the harness's own experience; prefer useful feedback and less user friction.

## Working principles

- Carry the user's authorized task through implementation and verification.
  Resolve routine choices from current code and the sources below; ask only
  when missing information materially changes the result. Current user
  direction overrides historical plans and skill defaults.
- Make focused changes with clear names and types, explicit ownership and
  failure boundaries, and simple control flow. Follow nearby crate conventions.
  Comments explain invariants or external constraints; avoid abstractions that
  add indirection without making behavior easier to maintain.
- Aim for the project's metric targets in new and changed code. Use findings
  to improve behavior and readability; never split code, weaken tests, hide
  measurements or add lint exceptions merely to improve a score. Record a
  justified exception under the policy below. Quality targets are not an
  automatic demand to repair unrelated legacy code or delay every release.
- Preserve user-owned state and recover safely on failure. Keep credentials,
  prompts and output private. Distinguish actionable compatibility failures
  from advisory ecosystem drift. Test observable behavior and meaningful
  failure cases, rather than implementation details or incidental wording.
- Keep code, documentation, user-facing copy and Git history in English.
  Use focused imperative Conventional Commits (`type(scope): summary`).
  Match the repository's author identity; do not add AI/coauthor/session trailers.

## Sources of truth

- Code workflow and conventions: [CONTRIBUTING.md](CONTRIBUTING.md#development-workflow).
- Verification: [Local quality gates](CONTRIBUTING.md#local-quality-gates).
  Use focused locked checks while editing; complete the required final gate
  once on the final tree. Repeat only for changes, failures or unresolved risks.
- Metric policy and accepted exceptions: [Metric interpretation](CONTRIBUTING.md#metric-interpretation).
  For metric work, read the local [metrics roadmap](docs/metrics-roadmap.md)
  when available; measurement scripts and dated artifacts establish results.
- Harness changes: [Adding a new harness](CONTRIBUTING.md#adding-a-new-harness).
- Privacy and security boundaries: [SECURITY.md](SECURITY.md).
- Releases: [Preparing a release](CONTRIBUTING.md#preparing-a-release),
  [canary runbook](canary/README.md) and [.github/workflows/](.github/workflows/).
- Improvement planning: the local [plans index](plans/README.md), when present.
  Historical plans are evidence, not current instructions or authorization.

Keep detailed procedures in these sources; avoid copying them into this router.
