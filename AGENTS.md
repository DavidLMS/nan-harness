# Agent guidance

This file defines stable project-wide principles and routes detailed work;
linked sources remain authoritative.

## Project principles

- Build for the [NaN builders community](https://nan.builders/): people using
  open models to ship real work. Prefer practical user value—less friction,
  dependable compatibility, and useful feedback—without replacing the
  harness's own experience. Use NaN's confident, lightly playful builder voice
  where it helps, but never at the expense of clarity.
- Quality means small, focused, maintainable changes with clear names and
  types, simple control flow, explicit boundaries, safe failure modes, and
  deterministic coverage of behavior. Let code explain what it does; comments
  should explain only non-obvious rationale, invariants, or external
  constraints. Optimize for clarity and correctness, not metrics or
  abstractions for their own sake.
- Treat user trust as part of correctness. Preserve user-owned state, keep
  credentials, prompts, and output private, avoid noisy or misleading warnings,
  and distinguish functional compatibility failures from advisory ecosystem
  drift. Test stable behavior rather than incidental wording unless exact
  wording is itself the contract.
- Keep source identifiers, documentation, user-facing copy, branch names, and
  Git history in English. Use focused Conventional Commit subjects in
  imperative form (`type(scope): summary`); use the body when non-obvious
  rationale, compatibility trade-offs, or migrations need explaining.

## Work routing

- Code changes: read [Development workflow](CONTRIBUTING.md#development-workflow) and [Local quality gates](CONTRIBUTING.md#local-quality-gates).
- Adding or changing a harness: read [Adding a new harness](CONTRIBUTING.md#adding-a-new-harness).
- Preparing a release: read [Preparing a release](CONTRIBUTING.md#preparing-a-release).
- Security-sensitive work: read [SECURITY.md](SECURITY.md).
- CI or release automation: inspect [.github/workflows/](.github/workflows/).

Keep detailed procedures in their authoritative documents; this file should
contain only stable, cross-cutting principles and routing.
