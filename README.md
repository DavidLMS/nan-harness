# nan-harness

[![CI](https://github.com/DavidLMS/nan-harness/actions/workflows/ci.yml/badge.svg)](https://github.com/DavidLMS/nan-harness/actions/workflows/ci.yml)

Use AI coding harnesses with the NaN provider.

nan-harness is a Rust CLI and compatibility layer for AI coding harnesses. It
supports two independent workflows: it can launch and supervise a harness with
the NaN provider, or it can write NaN into a supported harness's native provider
configuration so that harness can be started directly. The full command is
`nan-harness`; `nan` is its shorter command alias, which is used in the examples
below.

It does not replace the harnesses themselves. It lets you use the tools you
already know with a common NaN model and provider configuration.

## Supported harnesses

| Managed launch | Harness executable | Transport | Native setup |
| --- | --- | --- | --- |
| `nan claude` | `claude` | Anthropic Messages bridge | Needs nan-harness |
| `nan codex` | `codex` | OpenAI Responses bridge | Needs nan-harness |
| `nan opencode` | `opencode` | OpenAI Chat Completions | Available |
| `nan hermes` | `hermes` | OpenAI Chat Completions | Available |
| `nan pi` | `pi` | OpenAI Chat Completions | Available |
| `nan prime-agent` | `prime-agent` | OpenAI Chat Completions | Available |
| `nan dsh` | `dsh` | OpenAI Chat Completions | Available |
| `nan openclaw` | `openclaw` | OpenAI Chat Completions | Available |
| `nan cline` | `cline` | OpenAI Chat Completions | Available |
| `nan qwen` | `qwen` | OpenAI Chat Completions | Available |
| `nan kimi` | `kimi` | OpenAI Chat Completions | Available |
| `nan aider` | `aider` | OpenAI Chat Completions | Available |
| `nan goose` | `goose` | OpenAI Chat Completions | Available |
| `nan fx` | `fx` | fx AI Gateway bridge | Needs nan-harness |

The embedded [compatibility manifest](crates/nan-harness-runtime/resources/compatibility.json)
defines the minimum and bundled last verified version for each harness. Release
builds refresh successful daily canary results at most once every 24 hours; the
remote feed can advance verified versions but cannot change minimums, transports,
or policy. nan-harness checks harness-specific runtime requirements before
installation or launch and provides actionable instructions when something is
missing. Use `nan doctor` to see the status of the executable installed on your
machine.

## Installation

### Pre-built release

On macOS or Linux, download and verify the latest release with the installer:

```sh
curl --proto '=https' --tlsv1.2 --fail --location --show-error \
  --connect-timeout 10 --max-time 120 \
  https://github.com/DavidLMS/nan-harness/releases/latest/download/install.sh | sh
```

The installer shows download progress and stops with an error if GitHub cannot
be reached within the timeout.

On Windows PowerShell:

```powershell
irm https://github.com/DavidLMS/nan-harness/releases/latest/download/install.ps1 | iex
```

The installers download the platform binary, verify its SHA-256 checksum and
reported version, and install `nan-harness` plus its shorter `nan` command
alias. Release binaries are currently published for:

- macOS: Apple Silicon and Intel
- Linux: ARM64 and x86_64 (musl)
- Windows: x86_64

Open a new terminal if the installer asks you to update `PATH`, then check the
installation:

```sh
nan-harness --version
nan --help
```

### Build from source

The repository pins the required Rust toolchain in
[`rust-toolchain.toml`](rust-toolchain.toml). With Rustup installed:

```sh
git clone https://github.com/DavidLMS/nan-harness.git
cd nan-harness
cargo build --locked --release -p nan-harness-cli
```

The resulting binaries are the canonical `target/release/nan-harness` and its
shorter `nan` alias at `target/release/nan`.

## Configuration

The first interactive operation that needs a credential asks for the API key
with hidden input, verifies it against the NaN model catalog, and saves it. You
can also manage the saved credential explicitly:

```sh
nan auth login
nan auth status
nan auth logout --yes
```

nan-harness prefers the operating system credential store: Keychain on macOS,
Credential Manager on Windows, and Secret Service on Linux. If that store is
unavailable, nan-harness falls back to a private application file (mode `0600`
on Unix) and prints a warning.
Set `NAN_HARNESS_CREDENTIAL_BACKEND=keyring` to refuse that fallback, or `file`
for a deliberate file-backed setup such as a headless machine.

For ordinary launches, a non-empty `NAN_API_KEY` takes precedence over the key
saved by nan-harness. Successful launch-time verification is cached for one
hour to keep startup fast. `nan auth` and `nan config` always perform a fresh
provider check. HTTP 401 and 403 responses are described as a rejected key;
network failures and provider errors do not trigger credential replacement.

`NAN_API_KEY` remains supported for CI and advanced shell configuration:

```sh
export NAN_API_KEY="<your-NaN-api-key>"
```

The environment variable is never copied into a harness configuration. API
keys are never accepted as command-line arguments. Do not commit one or include
it in logs, bug reports, or shell history. Dry runs do not need a credential.

## Managed launch

Use `nan <harness>` when you want nan-harness to check compatibility, discover
your available NaN models, prepare any required bridge, and supervise the
harness process:

```sh
nan claude
nan codex --model qwen3.6
nan opencode --model deepseek-v4-flash
```

Arguments intended for the underlying harness can be passed after `--`:

```sh
nan codex --model qwen3.6 -- --full-auto
nan claude -- --resume
```

Before executing anything, inspect the normalized launch plan with `--dry-run`:

```sh
nan claude --model qwen3.6 --dry-run
```

The output is JSON and keeps launcher-managed secrets as references rather than
values. User-supplied arguments and local paths remain visible, so inspect the
output before sharing it.

Generate a safe whole-system report when troubleshooting:

```sh
nan doctor
nan doctor --json
```

The aggregate report checks the NaN API and available model count, all supported
harness installations, managed native configurations, and telemetry status. It never
prints API keys, local paths, prompts, model output, model IDs, or private
configuration, so it is suitable for a GitHub issue after you review it.
The JSON form has a stable schema, omits executable paths, and exits with a
failure status when it contains an actual error. Missing optional harnesses are
informational and do not make the command fail.

Check one harness installation and its compatibility status in detail:

```sh
nan doctor claude
nan doctor claude --json
nan doctor codex --executable /path/to/codex
```

The detailed command includes the local executable path, so review it before
sharing. Its JSON form deliberately excludes that path. Newer, unverified
versions produce a warning. Versions below the supported minimum or versions
whose output cannot be parsed require an explicit override:

```sh
nan claude --allow-untested
nan codex --allow-unsupported
```

## Native setup

Use `nan config <harness>` when you want to add NaN to a supported harness's
own provider configuration and then start that harness with its usual command.
`nan config` only configures the harness; it never launches it:

```sh
nan config pi
pi
nan config pi --status
nan config pi --refresh
nan config pi --remove
nan config --status
nan config --refresh-all
nan config --remove-all --yes
```

Native setup is supported for `opencode`, `hermes`, `pi`,
`prime-agent`, `dsh`, `openclaw`, `cline`, `qwen`, `kimi`, `aider`, and
`goose`. Claude Code, Codex, and fx need nan-harness running because their NaN
connection depends on a local bridge or gateway. They cannot be prepared for
standalone use with `nan config`.

The first configuration shows every file it will manage and asks for
confirmation. It copies only the API key explicitly saved by nan-harness, never
the current `NAN_API_KEY` environment variable. The saved key is copied because
the harness must authenticate when you later run its own executable directly.
Receipts contain only hashes and non-secret previous defaults. Files are
replaced atomically with owner-only permissions, and removal restores previous
defaults only while the managed values remain unchanged.

Model catalogs are snapshots. Run `nan config <harness> --refresh` after the
NaN catalog changes or after replacing the saved key. `nan config --status` and
`nan auth status` identify configurations whose copied key is stale. If you
rotate a key with `nan auth login`, nan-harness offers to refresh every managed
configuration. `nan auth logout` recommends removing them first; non-interactive
logout with managed configurations requires either
`--remove-configs --yes` or `--keep-configs --yes`.

### Maintenance and telemetry

```sh
nan update
nan auth status
nan telemetry on
nan telemetry off
nan uninstall
```

Update checks are automatic for interactive release binaries and can be
disabled with `NAN_NO_UPDATE_CHECK=1`. Anonymous telemetry is off by default.
`telemetry on` enables sanitized error reports and a minimal invocation event
containing the nan-harness version, harness, operation, transport, OS family,
architecture, and target environment. Normal harness launch commands use the
`nan-harness-<name>` event name so Umami Boards can show a harness breakdown
without custom filtering. Dry runs, configuration changes, diagnostics, and
non-harness commands use `nan-operation-<operation>`. All events also carry a
coarse dashboard tag.
A random installation identifier counts repeat usage without collecting
prompts, output, arguments, paths, models, credentials, usernames, or hostnames.
nan-harness does not add source IP addresses to telemetry payloads, although the
receiving HTTPS infrastructure can observe ordinary network metadata.
`telemetry off` stops usage events and deletes that identifier. When telemetry
is off, an interactive error can still offer a one-time anonymous report.

`nan uninstall` asks for confirmation, removes every native harness
configuration and saved API key recorded by nan-harness, deletes application data, and
removes both command names. It refuses to overwrite harness configuration
changed after nan-harness configured it; resolve that conflict and run the command again. Use
`nan uninstall --yes` only for non-interactive automation.

## How it works

Each adapter produces a typed launch plan before a process is started. The
runtime validates that plan, resolves the provider and model configuration,
forwards signals and exit status, and cleans up temporary files after the
harness exits.

When a harness already speaks OpenAI Chat Completions, nan-harness configures it to
connect directly to the provider. Claude Code and Codex use authenticated
loopback bridges that translate their native protocols to the NaN API. The
bridge keeps the real provider credential in the launcher and gives the child
process a short-lived local session token.

The workspace is split into focused crates:

| Crate | Responsibility |
| --- | --- |
| `nan-harness-core` | Domain contracts, launch plans, models, secrets, and compatibility types |
| `nan-harness-adapters` | Harness-specific launch adapters |
| `nan-harness-bridge` | Anthropic, Responses, and fx protocol bridges |
| `nan-harness-runtime` | Configuration, discovery, process supervision, temporary files, and updates |
| `nan-harness-cli` | The canonical `nan-harness` binary and its short `nan` alias |
| `nan-harness-diagnostics` | Typed user-facing warnings, setup guidance, and errors |
| `nan-harness-telemetry` | Consent-aware error diagnostics and minimal usage analytics |
| `nan-harness-test-support` | Shared fixtures and test utilities |
| `nan-harness-canary` | Private clean-VM compatibility runner and safe evidence aggregator |

## Development

Run the same quality gates used by CI:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps
```

Dependency policy checks require [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny):

```sh
cargo deny check
```

Most tests are deterministic and do not need a live API key. The ignored
conformance and live tests require the relevant external harness executable and
are intended for compatibility verification rather than ordinary pull-request
feedback.

The private Mac mini canary architecture, safety boundaries, and operations are
documented in [`canary/README.md`](canary/README.md).

When changing an adapter, update its fixtures and compatibility coverage. Keep
credentials, prompts, model output, and tool input/output out of tests and
diagnostic reports unless a test explicitly requires a local synthetic value.

## License

The code in this repository is licensed under the
[Apache License 2.0](LICENSE). See [NOTICE.md](NOTICE.md) for the treatment of
third-party names, marks, and logos.

## Citation

If you use nan-harness in research or another project, please cite it using
[`CITATION.cff`](CITATION.cff). Its version is checked against the workspace
version during release validation and the matching file is included in release
artifacts. Record user-visible changes under `[Unreleased]` in
[`CHANGELOG.md`](CHANGELOG.md). `cargo xtask set-version <VERSION>` promotes
those notes into a dated release and synchronizes the workspace, lockfile, and
citation metadata. Commit the result before creating the matching tag. CI
rejects missing changelog entries and mismatched release metadata.
