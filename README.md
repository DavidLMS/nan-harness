# nan-harness

<p align="center">
  <img src="assets/nan-harness-banner.png" alt="nan-harness" width="100%">
</p>

[![CI](https://github.com/DavidLMS/nan-harness/actions/workflows/ci.yml/badge.svg)](https://github.com/DavidLMS/nan-harness/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/DavidLMS/nan-harness?sort=semver)](https://github.com/DavidLMS/nan-harness/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/DavidLMS/nan-harness/total)](https://github.com/DavidLMS/nan-harness/releases)
[![License](https://img.shields.io/github/license/DavidLMS/nan-harness)](LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.97.1%2B-dea584?logo=rust&logoColor=white)](rust-toolchain.toml)

Run any supported AI coding harness with [NaN](https://nan.builders/).

nan-harness is a Rust CLI and compatibility layer for AI coding harnesses. Run
`nanh <harness>` to check compatibility, discover available NaN models,
prepare the connection, and supervise the process without changing the
harness's persistent provider configuration. Advanced users can configure a
supported harness for direct use with NaN by running `nanh config <harness>`, then
start the harness directly.

It works with the harnesses you already use.

## Supported harnesses

| Recommended command | Harness | Transport | Native setup |
| --- | --- | --- | --- |
| `nanh aider` | [Aider](https://aider.chat/) | OpenAI Chat Completions | Optional |
| `nanh cline` | [Cline](https://cline.bot/) | OpenAI Chat Completions | Optional |
| `nanh goose` | [Goose](https://github.com/block/goose) | OpenAI Chat Completions | Optional |
| `nanh claude` | [Claude Code](https://www.anthropic.com/claude-code) | Anthropic Messages bridge | Not available |
| `nanh codex` | [Codex](https://openai.com/codex/) | OpenAI Responses bridge | Not available |
| `nanh opencode` | [OpenCode](https://opencode.ai/) | OpenAI Chat Completions | Optional |
| `nanh qwen` | [Qwen Code](https://qwenlm.github.io/qwen-code-docs/en/users/overview) | OpenAI Chat Completions | Optional |
| `nanh pi` | [Pi](https://pi.dev/) | OpenAI Chat Completions | Optional |
| `nanh kimi` | [Kimi Code](https://www.kimi.com/code) | OpenAI Chat Completions | Optional |
| `nanh openclaw` | [OpenClaw](https://openclaw.ai/) | OpenAI Chat Completions | Optional |
| `nanh hermes` | [Hermes Agent](https://hermes-agent.nousresearch.com/) | OpenAI Chat Completions | Optional |
| `nanh omp` | [Oh My Pi](https://omp.sh/) | OpenAI Chat Completions | Optional |
| `nanh prime-agent` | [Prime Agent](https://github.com/PrimeIntellect-ai/prime-agent) | OpenAI Chat Completions | Optional |
| `nanh dsh` | [DeepSeek Harness](https://deepseek.com/harness/en/) | OpenAI Chat Completions | Optional |
| `nanh fx` | [fx](https://fx.sh/) | fx AI Gateway bridge | Not available |

Harnesses that use OpenAI Chat Completions use an authenticated local gateway by
default. This enables features such as reporting token usage when a session ends.

### Desktop app integrations

You can use NaN models in these desktop apps with a special configuration:

| Command | App | Available on |
| --- | --- | --- |
| `nanh chatgpt-desktop` | [ChatGPT](https://openai.com/chatgpt/desktop/) | macOS, Windows, and Linux (preview) |
| `nanh claude-desktop` | [Claude](https://claude.ai/download) | macOS, Windows, and Linux beta |
| `nanh hermes-desktop` | [Hermes](https://hermes-agent.nousresearch.com/) | macOS, Windows, and Linux |
| `nanh pen` (`nanh pen-desktop`) | [Pen](https://www.pen.dev/) | macOS, Windows, and Linux |

These integrations are experimental. All four apps have been tested on macOS.
The other platform combinations are covered by automated compatibility tests.

## Installation

### Pre-built release

On macOS or Linux, download and verify the latest release with the installer:

```sh
curl --proto '=https' --tlsv1.2 --fail --location --show-error \
  --connect-timeout 10 --max-time 120 \
  https://github.com/DavidLMS/nan-harness/releases/latest/download/install.sh | sh
```

On Windows PowerShell:

```powershell
irm https://github.com/DavidLMS/nan-harness/releases/latest/download/install.ps1 | iex
```

The installers verify the binary's SHA-256 checksum and version, then install
`nan-harness` and the `nanh` alias. `nanh` is the shorter alias for
`nan-harness`. Release binaries are currently published for:

- macOS: Apple Silicon and Intel
- Linux: ARM64 and x86_64 (musl)
- Windows: x86_64

Open a new terminal if the installer asks you to update `PATH`, then check the
installation:

```sh
nan-harness --version
nanh --help
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
shorter `nanh` alias at `target/release/nanh`.

## Credentials

If no API key is already available, the first interactive operation that needs
one asks for it with hidden input, verifies it against the NaN model catalog, and
saves it. You can also manage the saved credential explicitly:

```sh
nanh auth login
nanh auth status
nanh auth logout
```

nan-harness stores your saved NaN API key in your operating system's credential
store: Keychain on macOS, Credential Manager on Windows, or Secret Service on
Linux. If no store is available, it uses a private file and warns you.

For CI or advanced shell setups, set `NAN_API_KEY`:

```sh
export NAN_API_KEY="<your-NaN-api-key>"
```

This takes precedence over any saved key and is never copied into a harness
configuration.

## Recommended: run your harness through nan-harness

For everyday use, run `nanh <harness>`:

```sh
nanh claude
nanh codex --model qwen3.6
nanh opencode --model deepseek-v4-flash
```

On each launch, nan-harness checks compatibility, discovers available NaN
models, prepares any required bridge, and supervises the harness without
changing its persistent provider configuration.

To pass arguments to the harness itself, place `--` before them:

```sh
nanh codex --model qwen3.6 -- --full-auto
nanh claude -- --resume
```

When supported, nan-harness prints provider-reported input and output token
totals when the session ends. These are local figures, not estimates or
telemetry; incomplete sessions are marked as partial.

For troubleshooting an OpenAI Chat Completions integration, bypass the local
gateway for one launch:

```sh
nanh pi --no-chat-gateway
```

The harness then receives the provider credential directly, and gateway-dependent
features are unavailable for that launch.

## Run desktop apps through nan-harness

Run one of these commands to use NaN models in a desktop app:

```sh
nanh chatgpt-desktop
nanh claude-desktop
nanh hermes-desktop
nanh pen
```

Use `--dry-run` to preview the launch without reading your API key, changing
files, or opening the app. If a launch is interrupted, close the app and run the
same command with `--restore`.

- ChatGPT uses a separate profile. It keeps your login, history, and cache, but
  removes the temporary NaN connection when the app closes. `--debug` may print
  private app data.
- Claude restores your previous configuration after the app closes.
  `--show-auto` may print private request and response data from Auto mode.
- Hermes keeps conversations and local state in a separate `nan` profile.
  `--no-chat-gateway` skips the local gateway, so web search and the usage
  summary are not available. You can pass Hermes arguments after `--`.
- Pen receives a temporary `NaN` provider containing every text model available
  to the current account. The authenticated loopback gateway keeps the real key
  out of Pen, filters non-text models, and reports provider token usage. Pen must
  be fully quit before launch and reloads model changes only after a cold start.

## Web search fallback

Managed launches add NaN web search only when nan-harness does not find another
recognized search provider in the harness, project, or local search settings.
Existing search configuration is preserved.

```sh
nanh claude                         # Use the automatic fallback
nanh claude --no-search             # Disable the NaN fallback
nanh cline --force-search           # Force NaN search
```

`--no-search` affects only the NaN fallback. `--force-search` is available for
every harness except Aider, which keeps its existing search behavior and reports
an error if NaN search is forced.

Native setup follows the same policy. A chosen `--force-search` or `--no-search`
is preserved on later `--refresh` runs unless you pass a new flag. Use
`nanh config --status` to inspect the stored policy.

Aider supports native model configuration but not the NaN web search fallback.

Generate a safe system report when troubleshooting:

```sh
nanh doctor
nanh doctor --json
```

Use `nanh doctor --json` for a stable, safe-to-share report.

The report checks the NaN API, model availability, supported harness
installations, managed native configurations, and telemetry status. It excludes
API keys, paths, prompts, model output, model IDs, and private configuration, so
you can review it before sharing it in a GitHub issue.
The JSON form has a stable schema, omits executable paths, and exits with a
failure status when it contains an actual error. Missing optional harnesses are
informational and do not make the command fail.

Check one harness installation and its compatibility status in detail:

```sh
nanh doctor claude
nanh doctor claude --json
nanh doctor codex --executable /path/to/codex
```

The detailed command includes the local executable path, so review it before
sharing. Its JSON form deliberately excludes that path and reports the last
version confirmed compatible with this nan-harness release plus the latest live
verification evidence. Newer command-line harness versions produce a warning
and continue; newer desktop app versions require `--allow-untested`. Versions
below the supported minimum require `--allow-unsupported`, while command-line
harness versions whose output cannot be parsed require `--allow-untested`:

```sh
nanh claude --allow-untested
nanh codex --allow-unsupported
```

## Advanced: native setup

Use `nanh config <harness>` when another tool or integration needs to start a
supported harness directly instead of through nan-harness. It writes persistent
provider settings, copies the saved credential and a snapshot of the model
catalog. You must maintain those values yourself; the command only configures
the harness:

```sh
nanh config pi
pi
nanh config omp
omp
nanh config pi --status
nanh config pi --refresh
nanh config pi --remove
nanh config --status
nanh config --refresh-all
nanh config --remove-all --yes
```

Claude Code, Codex, and fx need nan-harness running because their NaN connection
depends on a local bridge or gateway. They cannot be prepared for standalone use
with `nanh config`.

### Run Hermes and Pen directly

Configure either app once:

```sh
nanh config hermes-desktop
nanh config pen
```

Use `--status`, `--refresh`, or `--remove` with either command.

After configuring Hermes, open it with:

```sh
hermes desktop
```

Open Pen normally after configuring it. Native setup copies your saved NaN
credential and a snapshot of the model catalog into the app. Direct launches do
not show usage summaries because nan-harness is not running.

Refresh a native configuration after changing your saved key or the NaN model
catalog.

## Maintenance and privacy

```sh
nanh update          # Update nan-harness
nanh auth status     # Show credential status
nanh telemetry on    # Enable anonymous telemetry
nanh telemetry off   # Disable anonymous telemetry
nanh uninstall       # Remove nan-harness and managed data
```

Update checks are automatic for interactive release binaries. Set
`NAN_NO_UPDATE_CHECK=1` to disable them.

Telemetry is off by default. When enabled, it sends sanitized diagnostics and
minimal usage data; it never includes prompts, responses, credentials, or local
paths. An interactive error may still offer a one-time report when telemetry is
off.

`nanh uninstall` asks for confirmation and stops if it would overwrite a
configuration changed after nan-harness created it. Use `nanh uninstall --yes`
only in non-interactive automation.

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
