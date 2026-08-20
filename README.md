# NaN Harness

[![CI](https://github.com/DavidLMS/nan-harness/actions/workflows/ci.yml/badge.svg)](https://github.com/DavidLMS/nan-harness/actions/workflows/ci.yml)

Run AI coding harnesses through NaN.

NaN Harness is a Rust CLI launcher and compatibility layer for AI coding
harnesses. It resolves the NaN provider configuration, checks the installed
harness version, builds a validated launch plan, starts a local protocol bridge
when required, and then hands control to the original harness.

It does not replace the harnesses themselves. It lets you use the tools you
already know with a common NaN model and provider configuration.

## Supported harnesses

| Command | Harness executable | Transport |
| --- | --- | --- |
| `nan claude` | `claude` | Anthropic Messages bridge |
| `nan codex` | `codex` | OpenAI Responses bridge |
| `nan opencode` | `opencode` | OpenAI Chat Completions |
| `nan hermes` | `hermes` | OpenAI Chat Completions |
| `nan pi` | `pi` | OpenAI Chat Completions |
| `nan prime` | `prime-agent` | OpenAI Chat Completions |
| `nan deepseek` | `dsh` | OpenAI Chat Completions |
| `nan openclaw` | `openclaw` | OpenAI Chat Completions |
| `nan cline` | `cline` | OpenAI Chat Completions |
| `nan qwen` | `qwen` | OpenAI Chat Completions |
| `nan kimi` | `kimi` | OpenAI Chat Completions |
| `nan aider` | `aider` | OpenAI Chat Completions |
| `nan goose` | `goose` | OpenAI Chat Completions |
| `nan fx` | `fx` | fx AI Gateway bridge |

The embedded [compatibility manifest](crates/nan-harness-runtime/resources/compatibility.json)
defines the minimum and last verified version for each harness. Use `nan doctor`
to see the status of the executable installed on your machine.

## Installation

### Pre-built release

On macOS or Linux, download and verify the latest release with the installer:

```sh
curl --proto '=https' --tlsv1.2 -fsSL \
  https://raw.githubusercontent.com/DavidLMS/nan-harness/main/install.sh | sh
```

On Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/DavidLMS/nan-harness/main/install.ps1 | iex
```

The installers download the platform binary, verify its SHA-256 checksum and
reported version, and install both `nan` and the `nan-harness` compatibility
alias. Release binaries are currently published for:

- macOS: Apple Silicon and Intel
- Linux: ARM64 and x86_64 (musl)
- Windows: x86_64

Open a new terminal if the installer asks you to update `PATH`, then check the
installation:

```sh
nan --version
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

The resulting binaries are `target/release/nan` and
`target/release/nan-harness`.

## Configuration

Set your NaN API key in the environment before launching a harness:

```sh
export NAN_API_KEY="<your-NaN-api-key>"
```

`NAN_API_KEY` is required for real launches and is never a command-line
argument. Do not commit it or include it in logs, bug reports, or shell
history.

## Usage

Run a harness from the current directory:

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

The output is JSON and contains secret references rather than secret values.
It is safe for reviewing the planned process, environment overlays, transport,
and temporary artifacts.

Check a harness installation and its compatibility status:

```sh
nan doctor claude
nan doctor codex --executable /path/to/codex
```

Newer, unverified versions produce a warning. Versions below the supported
minimum or versions whose output cannot be parsed require an explicit override:

```sh
nan claude --allow-untested
nan codex --allow-unsupported
```

### Persistent provider integrations

Most harnesses use temporary configuration generated for one launch. The
following commands also support reversible persistence:

```sh
nan opencode --persist
nan opencode --unpersist
```

Persistence is available for `opencode`, `pi`, `prime`, `deepseek`, `qwen`, and
`aider`. NaN manages the provider entries and creates backups where necessary;
remove them later with `--unpersist`. Persistent configurations refer to
`NAN_API_KEY` instead of embedding the key in the configuration.

### Maintenance and telemetry

```sh
nan update
nan telemetry on
nan telemetry off
```

Update checks are automatic for interactive release binaries and can be
disabled with `NAN_NO_UPDATE_CHECK=1`. Anonymous telemetry is off by default.
`telemetry on` enables sanitized error reports and a minimal invocation event
containing the NaN version, harness, operation, transport, OS family,
architecture, and target environment. A random installation identifier counts
repeat usage without collecting prompts, output, arguments, paths, models,
credentials, usernames, or hostnames. NaN does not add source IP addresses to
telemetry payloads, although the receiving HTTPS infrastructure can observe
ordinary network metadata. `telemetry off` stops usage events and deletes that
identifier. When telemetry is off, an interactive error can still offer a
one-time anonymous report.

## How it works

Each adapter produces a typed launch plan before a process is started. The
runtime validates that plan, resolves the provider and model configuration,
forwards signals and exit status, and cleans up temporary files after the
harness exits.

When a harness already speaks OpenAI Chat Completions, NaN configures it to
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
| `nan-harness-cli` | The `nan` and `nan-harness` binaries |
| `nan-harness-telemetry` | Consent-aware error diagnostics and minimal usage analytics |
| `nan-harness-test-support` | Shared fixtures and test utilities |

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

When changing an adapter, update its fixtures and compatibility coverage. Keep
credentials, prompts, model output, and tool input/output out of tests and
diagnostic reports unless a test explicitly requires a local synthetic value.

## License

The code in this repository is licensed under the
[Apache License 2.0](LICENSE). See [NOTICE.md](NOTICE.md) for the treatment of
third-party names, marks, and logos.

## Citation

If you use NaN Harness in research or another project, please cite it using
[`CITATION.cff`](CITATION.cff). Its version is checked against the workspace
version during release validation and the matching file is included in release
artifacts. Prepare workspace, lockfile, and citation metadata with
`cargo xtask set-version`, then commit those changes before creating the matching
tag. CI rejects mismatched tags.
