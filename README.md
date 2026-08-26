# nan-harness

<p align="center">
  <img src="assets/nan-harness-banner.png" alt="nan-harness" width="100%">
</p>

[![CI](https://github.com/DavidLMS/nan-harness/actions/workflows/ci.yml/badge.svg)](https://github.com/DavidLMS/nan-harness/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/DavidLMS/nan-harness?sort=semver)](https://github.com/DavidLMS/nan-harness/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/DavidLMS/nan-harness/total)](https://github.com/DavidLMS/nan-harness/releases)
[![License](https://img.shields.io/github/license/DavidLMS/nan-harness)](LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.97.1%2B-dea584?logo=rust&logoColor=white)](rust-toolchain.toml)

Run any supported AI coding harness with the [NaN provider](https://nan.builders/).

nan-harness is a Rust CLI and compatibility layer for AI coding harnesses. For
everyday use, run `nan <harness>`. nan-harness checks compatibility, discovers
your current NaN models, prepares the connection, and supervises the process
without changing the harness's persistent provider configuration. Advanced
users can optionally write NaN into a supported harness's native configuration
with `nan config <harness>` and then start that harness directly. The full
command is `nan-harness`; `nan` is its shorter command alias, which is used in
the examples below.

It does not replace the harnesses themselves. It lets you use the tools you
already know with a consistent NaN connection.

## Supported harnesses

| Recommended command | Harness executable | Transport | Native setup |
| --- | --- | --- | --- |
| `nan aider` | `aider` | OpenAI Chat Completions | Optional |
| `nan cline` | `cline` | OpenAI Chat Completions | Optional |
| `nan goose` | `goose` | OpenAI Chat Completions | Optional |
| `nan claude` | `claude` | Anthropic Messages bridge | Not available |
| `nan codex` | `codex` | OpenAI Responses bridge | Not available |
| `nan opencode` | `opencode` | OpenAI Chat Completions | Optional |
| `nan qwen` | `qwen` | OpenAI Chat Completions | Optional |
| `nan pi` | `pi` | OpenAI Chat Completions | Optional |
| `nan kimi` | `kimi` | OpenAI Chat Completions | Optional |
| `nan openclaw` | `openclaw` | OpenAI Chat Completions | Optional |
| `nan hermes` | `hermes` | OpenAI Chat Completions | Optional |
| `nan prime-agent` | `prime-agent` | OpenAI Chat Completions | Optional |
| `nan dsh` | `dsh` | OpenAI Chat Completions | Optional |
| `nan fx` | `fx` | fx AI Gateway bridge | Not available |

The embedded [compatibility manifest](crates/nan-harness-runtime/resources/compatibility.json)
defines the minimum and bundled last compatible version for each harness. Release
builds refresh successful daily canary results at most once every 24 hours; the
remote feed can advance release-scoped compatibility evidence but cannot change
minimums, transports, runtime requirements, or policy. nan-harness checks
harness-specific runtime requirements before installation or launch and provides
actionable instructions when something is missing. Use `nan doctor` to see the
status and release-scoped evidence for the executable installed on your machine.

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

## Credentials

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
on Unix; on Windows, a protected DACL granting full control only to the current
process user and `SYSTEM`) and prints a warning. If private-file hardening
fails, file-backed persistence aborts.
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
it in logs, bug reports, or shell history.

## Recommended: launch with nan-harness

For everyday use, run `nan <harness>`. This recommended workflow works with
every supported harness. nan-harness checks compatibility, discovers your
current NaN models, uses your current credential source, prepares any required
bridge, and supervises the harness process without leaving persistent provider
configuration to maintain:

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

Generate a safe whole-system report when troubleshooting:

```sh
nan doctor
nan doctor --json
```

Use `nan doctor --json` for a stable, safe-to-share report.

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
sharing. Its JSON form deliberately excludes that path and reports the last
version confirmed compatible with this nan-harness release plus the latest live
verification evidence. Newer versions produce a warning. Versions below the
supported minimum or versions whose output cannot be parsed require an explicit
override:

```sh
nan claude --allow-untested
nan codex --allow-unsupported
```

When telemetry is enabled, nan-harness can also send sanitized reports for CLI
and bridge failures. A harness started directly from native configuration runs
outside that observability layer.

## Advanced: native setup

Use `nan config <harness>` only when another tool or integration needs to start
a supported harness with its usual command instead of launching it through
nan-harness. Native setup writes persistent provider settings, copies the saved
credential and a snapshot of the model catalog, and leaves those values for you
to maintain. `nan config` only configures the harness; it never launches it:

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
replaced atomically with owner-only modes (`0600` files and `0700` private
directories) on Unix or a protected DACL granting full control only to the
current process user and `SYSTEM` on Windows. If private-file hardening fails,
file-backed persistence aborts. Removal restores previous defaults only while
the managed values remain unchanged.

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
A random installation identifier correlates repeated diagnostics from the same
installation and counts repeat usage while telemetry is enabled. Usage events
never include models. Error reports can include the normalized NaN model ID only
when it is needed to classify a model-specific failure, together with typed
details such as the reasoning policy, bridge endpoint, harness version, HTTP
status, process stage, or operating-system error kind.

The identifier remains stored locally when telemetry is off so separate reports
explicitly approved with `y` can be recognized as coming from the same anonymous
installation. Its presence does not enable usage events or automatic error
delivery. Reports never contain prompts, responses, arguments, paths, source
code, tool input/output, credentials, usernames, or hostnames.
nan-harness does not add source IP addresses to telemetry payloads, although the
receiving HTTPS infrastructure can observe ordinary network metadata.
`telemetry off` stops usage events and automatic error delivery. When telemetry
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

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the contributor workflow, focused
development loop, local quality gate, harness requirements, and release
preparation.

## License

The code in this repository is licensed under the
[Apache License 2.0](LICENSE). See [NOTICE.md](NOTICE.md) for the treatment of
third-party names, marks, and logos.

## Citation

If you use nan-harness in research or another project, please cite it using
[`CITATION.cff`](CITATION.cff).
