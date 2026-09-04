# Contributing to nan-harness

Thank you for contributing. nan-harness is a launcher and compatibility layer
for existing AI coding harnesses. Contributions should preserve that boundary:
the original harness remains responsible for the user experience, while
nan-harness owns provider routing, compatibility, process supervision, and
protocol translation.

## Start with an issue

For a new feature, a new harness, a transport change, or a behavior change,
open an issue before implementing it or opening a pull request. The issue lets
us agree on scope, the user problem, compatibility expectations, and the
smallest useful design before code creates an accidental contract.

An issue proposal should include:

- the problem being solved and who benefits;
- the harness, executable, versions, operating systems, and installation
  method involved;
- the proposed transport and why the existing direct or bridge transport is
  insufficient;
- model discovery and entitlement assumptions;
- whether the intended workflow is a managed launch, native setup, or both;
- the tests and compatibility evidence that should be added; and
- documentation, security, release, and maintenance implications.

Use the appropriate [issue template](https://github.com/DavidLMS/nan-harness/issues/new/choose):
[harness or compatibility proposal](https://github.com/DavidLMS/nan-harness/issues/new?template=harness-proposal.yml),
[bug report](https://github.com/DavidLMS/nan-harness/issues/new?template=bug-report.yml),
or [feature suggestion](https://github.com/DavidLMS/nan-harness/issues/new?template=feature-request.yml).
The [template sources](.github/ISSUE_TEMPLATE/) are versioned with the
repository and can be copied when a form does not fit the proposal exactly.

Small documentation fixes, typo fixes, and narrowly scoped bug fixes may go
directly to a pull request. When in doubt, open the issue first.

## Development workflow

1. Open an issue and wait for the scope to be agreed.
2. Create a focused branch from the current default branch.
3. Implement the smallest coherent change. Keep unrelated refactors out of
   the branch.
4. Add deterministic tests before relying on live harnesses or API calls.
5. Run the local quality gates below.
6. Open a pull request that links the issue, explains the design, and includes
   the completed checklist.
7. Address review feedback with follow-up commits; preserve useful review
   history until the change is ready to merge.

Do not include API keys, prompts, model output, tool input/output, local
credentials, or private configuration files in issues, fixtures, logs, or pull
requests.

Every new reportable error variant must map exhaustively to a closed telemetry
diagnostic reason and safe typed details. Never derive telemetry by parsing an
error's display text or add raw messages, paths, URLs, arguments, or provider
payloads to an error report.

## Local quality gates

Use a focused loop while iterating on one crate or behavior:

```sh
cargo check --locked -p <crate> --all-features
cargo test --locked -p <crate> --all-features
cargo test --locked -p <crate> --all-features <filter>
```

A bare `cargo test` uses the workspace's `default-members`, which currently
means only `nan-harness-cli`. It is useful for CLI iteration but is not a
workspace-wide check. Use `--workspace --all-features` when you need to run
the deterministic suite across every member.

Before opening a pull request, run the repository gate:

```sh
cargo check-all
```

This is the authoritative local gate. It invokes the repository's `xtask`
check task; keep its implementation in `xtask` and its CI counterpart in the
[CI workflow](.github/workflows/ci.yml), rather than copying their command
lists into this guide. Cargo commands in quality gates must use the committed
`Cargo.lock` without updating it. The gate requires `cargo-deny`. Live and
ignored tests may require a particular external harness and `NAN_API_KEY`;
they are compatibility checks, not a substitute for deterministic
pull-request tests.
Run the complete gate once against the final tree. During implementation, use
the focused commands above and do not repeat an already successful command when
the commit, toolchain, dependency graph, features, and relevant environment are
unchanged.

## Preparing a release

Use this checklist when a release is ready. The [release workflow](.github/workflows/release.yml)
automates validation, builds, metadata, and draft creation; the
[compatibility canary runbook](canary/README.md) describes the gate that
verifies the draft before publication.

- [ ] User-visible changes are recorded under `[Unreleased]` in `CHANGELOG.md`.
- [ ] Choose the next semantic version and run `cargo xtask set-version <VERSION>`.
- [ ] Review the generated changes to the workspace manifests, `Cargo.lock`,
      `CITATION.cff`, and `CHANGELOG.md`.
- [ ] Run `cargo check-all` and resolve every failure.
- [ ] Run `cargo xtask release-check v<VERSION>` after the version change.
- [ ] Complete any plan-required live checks before tagging. When the
      compatibility canary already owns an equivalent release criterion, use
      its result instead of repeating the same live matrix locally.
- [ ] Commit the synchronized release metadata before creating the matching
      `v<VERSION>` tag.
- [ ] Push the release commit to `main` and wait for the exact commit's CI run
      to succeed. From this point until tagging, do not change code or release
      metadata.
- [ ] Push the tag to GitHub and confirm that the release workflow creates a
      draft with the expected assets and notes.
- [ ] Keep the draft unpublished until the compatibility gate completes
      successfully.
- [ ] After promotion, confirm that the release is public, is not a prerelease,
      is marked as latest, and contains the expected assets, checksums, and
      attestations.

The tag workflow reuses the successful `main` CI result for the exact release
commit and fails closed if that result is missing or unsuccessful. Re-running
the release workflow is safe when CI had not finished yet. Documentation-only
changes are excluded from Rust CI by the workflow path filter.

## Adding a new harness

A new harness is complete only when it has a safe launch contract, model
behavior, compatibility evidence, and a repeatable test path. Use this
checklist in the issue and PR.

### Proposal and compatibility scope

- [ ] The issue describes the harness, executable name, official installation
      path, supported platforms, and the user workflow to support.
- [ ] The issue identifies a pinned version to verify and a minimum version to
      support. “It starts on my machine” is not compatibility evidence.
- [ ] The issue records the harness's native provider protocol and selects one
      of the existing transports: direct Chat Completions, Anthropic Messages
      bridge, OpenAI Responses bridge, or fx Gateway bridge.
- [ ] The issue explains whether the first version supports managed launches,
      native setup through `nanh config`, or both.
- [ ] The issue identifies the expected model picker, reasoning controls,
      context/output limits, tools, streaming, images, search, and other
      harness-specific capabilities.

### Core registration and discovery

- [ ] `HarnessKind` has the canonical name, aliases, display name, and actual
      executable name.
- [ ] The CLI exposes a subcommand with the shared model, executable,
      provider-routing, compatibility, dry-run, and pass-through argument
      behavior.
- [ ] `nanh doctor <harness>` can discover the executable, run its version
      command, and report the minimum and last compatible versions.
- [ ] The compatibility entry is added to
      `crates/nan-harness-runtime/resources/compatibility.json` with its
      command, transport, minimum version, compatibility evidence, and policy.
- [ ] Executables installed in supported user locations are handled where the
      harness's official installer requires it.
- [ ] Discovery failures, unsupported versions, and unparseable versions have
      actionable messages and do not silently run an unsafe configuration.

### Adapter and launch plan

- [ ] The adapter lives in `crates/nan-harness-adapters` and returns a typed,
      validated `LaunchPlan`; it does not spawn processes directly.
- [ ] The plan preserves user arguments that are unrelated to provider
      routing, model selection, and local configuration.
- [ ] User arguments cannot override the NaN provider, bridge address, session
      token, or selected model accidentally.
- [ ] `--dry-run` produces a useful normalized plan without requiring an API
      key, contacting the provider, or starting the harness.
- [ ] Signals, terminal behavior, exit codes, temporary files, and cleanup are
      handled by the shared runtime rather than ad hoc adapter code.
- [ ] The direct-vs-bridge choice is justified by the harness protocol. Do not
      add a bridge merely to duplicate a native direct integration.

### Model discovery and capabilities

- [ ] The live model response for the user's NaN account remains the source of
      truth for availability and entitlements.
- [ ] The adapter does not hardcode a provider allowlist or assume that every
      known model is available to every account.
- [ ] Known models use the shared capability profiles where appropriate:
      context window, maximum output, image input, reasoning policy, and tool
      support.
- [ ] Unknown but valid provider models degrade to the shared conservative
      generic profile instead of being rejected without a clear reason.
- [ ] The model picker and model aliases are populated from live discovery or
      explicit runtime placeholders, not frozen in a launch plan.
- [ ] Explicit model selection, unavailable models, default selection, and
      model fallback behavior have tests.
- [ ] Reasoning controls are model-aware: unsupported controls are rejected or
      omitted, and defaults are not serialized as explicit user choices.

### Protocol and security behavior

- [ ] Every request and response shape used by the harness is documented in
      fixtures or focused tests, including streaming and tool calls where the
      harness supports them.
- [ ] A bridge translates errors, cancellation, authentication, streaming
      termination, usage, and tool-call lifecycle correctly.
- [ ] Local bridges bind only to loopback and authenticate child requests with
      a launch-scoped token.
- [ ] The real `NAN_API_KEY` is never placed in command arguments, launch-plan
      JSON, logs, temporary artifacts, or telemetry.
- [ ] Temporary files use the shared ownership and cleanup rules.
- [ ] User configuration is not overwritten implicitly. Any `nanh config`
      support must be explicit, reversible, receipt-backed, secret-safe, and
      limited to values owned by nan-harness.
- [ ] Error paths are redacted and do not expose prompts, output, source code,
      paths, tool data, or credentials.

### Test coverage

- [ ] Adapter unit tests cover deterministic plan construction and launch-plan
      validation.
- [ ] Tests cover pass-through arguments and rejection of arguments that could
      bypass nan-harness routing.
- [ ] Tests cover `doctor`, version policy, executable overrides, and the
      command's `--dry-run` path.
- [ ] Tests cover live model discovery, model catalogs, capability rendering,
      and selected-model behavior using a scripted or synthetic provider.
- [ ] Direct integrations have deterministic tests in
      `crates/nan-harness-adapters/tests/direct` suite or a focused companion
      test.
- [ ] Bridges have request/response/streaming contract tests in
      `crates/nan-harness-bridge/tests` and fixtures for tool lifecycle and
      failure cases.
- [ ] Native configurations have configure, refresh, status, remove, key
      rotation, and uninstall tests that preserve user-owned settings.
- [ ] Live or ignored conformance tests are isolated, time-bounded, and
      explicit about their required executable and environment variables.
- [ ] No pull-request test requires a real account or an unredacted secret.

### Compatibility matrix and roadmap evidence

The repository has several levels of compatibility evidence. A new harness
should advance through them rather than claiming full support immediately:

1. **Deterministic contract:** adapter, discovery, model, plan, and failure
   tests pass without a live harness.
2. **Pinned conformance:** the exact version in the compatibility manifest is
   installed and its relevant tool/model workflows pass.
3. **Latest-version canary:** a scheduled job checks the latest upstream
   version and reports regressions without silently changing the minimum
   supported version.
4. **Lifecycle coverage:** native configuration, cancellation, upgrades, extra tools,
   and platform-specific installation paths are covered where applicable.

For a new harness PR:

- [ ] The pinned version was actually exercised and recorded in the
      compatibility manifest.
- [ ] The minimum supported version is justified by a reproducible failure or
      compatibility boundary.
- [ ] The deterministic compatibility matrix has a test entry for the new
      harness.
- [ ] A live/ignored conformance path exists when the harness has meaningful
      external behavior that fixtures cannot cover.
- [ ] A scheduled canary is proposed or added when the harness is likely to
      change independently of provider releases.
- [ ] The issue identifies any missing matrix dimension: operating system,
      harness version, model, transport, tool, streaming mode, native configuration,
      or authentication state.
- [ ] The PR states which evidence level is complete and which roadmap level
      remains.

The current CI runs workspace quality gates, pinned conformance for all
supported harnesses, and a latest-version deterministic matrix. The private Mac
mini canary adds clean Linux and macOS installation plus live `qwen3.6` tool
probes. Release assets remain in a GitHub draft until all 15 harnesses pass that
gate. New harnesses must add a versioned
`tests/conformance/<harness>/manifest.toml`, deterministic coverage, a clean-VM
installer path, and a live tool probe instead of a one-off workflow.

### Documentation and release readiness

- [ ] The supported-harness table and usage examples in `README.md` are
      updated.
- [ ] CLI help text describes the new command and its aliases accurately.
- [ ] Any third-party name, mark, or logo not already covered is reviewed for
      `NOTICE.md` and trademark scope.
- [ ] Release notes can explain the transport, supported version range, and
      known limitations.
- [ ] User-visible changes are recorded under `[Unreleased]` in `CHANGELOG.md`;
      internal refactors, tests, and maintenance-only changes are omitted.
- [ ] If the change is shipping in a release, the
      [release preparation checklist](#preparing-a-release) is complete.

## Pull request checklist

- [ ] The PR links an approved issue, or explains why it is a small fix that
      did not need prior design discussion.
- [ ] The description explains the user-visible behavior and the chosen
      transport.
- [ ] Deterministic tests cover the new behavior and failure modes.
- [ ] Live tests, if any, are ignored, isolated, and documented.
- [ ] Secrets and private user data are absent from the diff and fixtures.
- [ ] README, compatibility metadata, tests, and notices are updated.
- [ ] `cargo check-all` passes, together with any relevant platform or
      compatibility checks reported by CI.
- [ ] The PR contains no unrelated formatting or refactoring churn.

## Commit messages

Use Conventional Commit messages, consistent with the repository history:

```text
feat(adapter): add <harness> launch adapter
test(conformance): cover <harness> tool workflow
fix(discovery): handle <harness> version output
docs(contributing): clarify harness support requirements
build(release): synchronize version metadata
```

Keep the subject short and imperative. Use the body to explain compatibility
trade-offs or migration details when the subject is not enough.

## License and notices

By contributing, you agree that your contribution is provided under the
repository's [Apache License 2.0](LICENSE). Review [NOTICE.md](NOTICE.md) for
the project's treatment of third-party names, marks, and logos.
