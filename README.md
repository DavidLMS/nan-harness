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
| `nanh zed` (`nanh zed-desktop`) | [Zed](https://zed.dev/) | macOS, Windows, and Linux |
| `nanh chatgpt-desktop` | [ChatGPT](https://openai.com/chatgpt/desktop/) | macOS, Windows, and Linux (preview) |
| `nanh claude-desktop` | [Claude](https://claude.ai/download) | macOS, Windows, and Linux beta |
| `nanh hermes-desktop` | [Hermes](https://hermes-agent.nousresearch.com/) | macOS, Windows, and Linux |
| `nanh pen` (`nanh pen-desktop`) | [Pen](https://www.pen.dev/) | macOS, Windows, and Linux |

These integrations are experimental. Zed, ChatGPT, Claude, Hermes, and Pen have
been tested on macOS; their other platform combinations are covered by automated
compatibility tests.

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

### Session budgets and context targets

You can set a launch-wide admission budget and, for supported harnesses, a
native compaction target:

```sh
nanh codex --session-max-tokens 10000000 --context 150000
```

`--session-max-tokens` counts provider-reported input plus output tokens across
all inference requests in that launch, including requests made by auxiliary
models and compacting turns. It is enforced by the local coordinator when a
request is admitted. Requests already in flight can finish and may put the
observed total above the limit; an unverified response blocks later requests
instead of being treated as zero. The budget is not reset by compaction or
model changes, is private to the launch, and requires the chat gateway.

Once the limit is reached, the next inference request completes with a local
`nan-harness: session token budget reached.` notice showing the observed usage
and limit. Your conversation remains available, but subsequent inference
requests in that launch stay blocked. Start a new nan-harness launch with a
higher budget and resume the conversation to continue. The notice does not
consume tokens or count as an inference, and budget stops stay in local
diagnostics without generating error telemetry.

Requests requiring structured output, a mandatory tool call, or a permission
decision receive a non-retryable HTTP 400 before streaming instead of a text
notice. nan-harness never fabricates a tool result or permission decision.
Independent provider and accounting failures remain reportable.

`--context` is an approximate native compaction target, calculated from the
starting model's effective context window. It is supported by Claude Code,
Codex, OpenCode, Hermes, Pi, Prime Agent, OMP, Qwen Code, Kimi Code, Aider,
Goose, Hermes Desktop and Zed. It is not supported by DeepSeek Harness,
OpenClaw, Cline, fx, ChatGPT Desktop, Claude Desktop or Pen Desktop; those
commands reject the option. The native harness may compact earlier to preserve
its own safety margin, and changing models can change the effective threshold.

Use `--dry-run` to inspect the requested budget and native context setting
without starting a process or consuming provider tokens. `--context` can be
used without the gateway; `--session-max-tokens` cannot be combined with
`--no-chat-gateway`.

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
nanh zed
nanh chatgpt-desktop
nanh claude-desktop
nanh hermes-desktop
nanh pen
```

Use `--dry-run` to preview the launch without reading your API key, changing
files, or opening the app. If a launch is interrupted, close the app and run the
same command with `--restore`.

- Zed receives a temporary NaN provider and restores its previous configuration
  after the app closes.
- ChatGPT uses a separate profile. It keeps login, history, cache and native
  preferences in that profile, and removes the temporary NaN connection when
  the app closes. `--debug` may print private app data.
- Claude restores your previous configuration after the app closes.
  `--show-auto` may print private request and response data from Auto mode.
- Hermes keeps conversations and local state in a separate `nan` profile.
  `--no-chat-gateway` skips the local gateway, so web search and the usage
  summary are not available. You can pass Hermes arguments after `--`.
- Pen receives a temporary `NaN` provider containing every text model available
  to the current account. The authenticated loopback gateway keeps the real key
  out of Pen, filters non-text models, and reports provider token usage. Pen must
  be fully quit before launch and reloads model changes only after a cold start.

On the first managed ChatGPT launch, complete any initial setup in the app.
Interactive launches wait until it connects, exits or you press Ctrl+C. Scripts
have a 15-second startup timeout. Use `--startup-timeout <SECONDS>` to set an
explicit limit of 1–86400 seconds in either mode.

## Web search

NaN web search is an optional SearXNG-backed feature. The backend is not
started or contacted until you explicitly configure one with
`nanh search setup`. The saved configuration contains only a validated
endpoint and mode; it does not contain a SearXNG credential. NaN's provider
credential remains in nan-harness, and a remote endpoint should be a SearXNG
instance you trust.

Choose one backend:

- `--local` installs and supervises a private loopback SearXNG instance on
  supported macOS and Linux targets.
- `--docker` creates and manages an owned SearXNG container through Docker.
- `--url https://...` uses an HTTPS SearXNG endpoint managed elsewhere.

Setup verifies the selected endpoint before saving it. A remote URL is contacted
by that verification; `status` may re-probe a remote URL but never starts a
managed backend.

The lifecycle commands are explicit. `status --json` inspects state without
starting a backend, `disable` removes the saved endpoint while retaining a
managed backend, `update` updates a managed local or Docker backend, and
`remove` removes an owned local or Docker backend and the saved endpoint.

```sh
nanh search status --json
nanh search setup --local
nanh search setup --docker
nanh search setup --url https://search.example.test
nanh search disable
nanh search update
nanh search remove
```

Managed launches add NaN web search only when nan-harness does not find another
recognized search provider in the harness, project, or local search settings.
Existing search configuration is preserved. If the NaN fallback is selected
before a SearXNG backend is configured, a search request reports setup guidance;
the launch does not silently install or start SearXNG.

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

See the [SearXNG manual test runbook](docs/viability/searxng-manual-test.md) for
isolated configuration, backend lifecycle checks, and Windows executable notes.
The integrated feature is documented on the `DavidLMS/searxng-search` branch;
`main` is not changed by this work.

Generate a safe system report when troubleshooting:

```sh
nanh doctor
nanh doctor --json
```

Use `nanh doctor --json` for a stable, shareable report.

Use `nanh doctor --offline` or `nanh doctor claude --offline` (also with
`--json`) to check locally without nan-harness network activity or credential-store
resolution. Offline doctor skips provider/model discovery, compatibility refresh,
update checks, telemetry uploads, and analytics. Skipped checks are informational;
actual local errors keep their usual behavior. Compatibility evidence comes from
the local cache or embedded registry and is not freshly verified. No model cache
is used. Bounded harness executable version probes still run; any activity of
those external executables is outside the offline guarantee.

Version and capability probes use a 30-second deadline and a 1 MiB combined
stdout/stderr limit per command, including transient executable-busy retries.
Timeout or overflow cancels collection and terminates the owned process group
or Windows job, with up to one additional second for direct-child cleanup.
These limits do not apply to harness sessions.

By default, the report checks the NaN API, model availability, supported harness
installations, managed native configurations, and telemetry status. It includes
available model IDs and capabilities. It excludes API keys, paths, prompts,
model output, and private configuration. This local diagnostic is separate from
telemetry, so review it before sharing it in a GitHub issue.
The JSON form has a stable schema, omits executable paths, and exits with a
failure status when it contains an actual error. Missing optional harnesses are
informational and do not make the command fail.
Managed configurations report `active`, `missing`, `changed`, `invalid`, or
`unreadable`. Missing or changed documents produce warnings; invalid or unreadable
documents produce errors without exposing their contents. JSON schema version 8
retains the `active` field as a compatibility projection of the configuration state.

Check one harness installation and its compatibility status in detail:

```sh
nanh doctor claude
nanh doctor claude --json
nanh doctor codex --executable /path/to/codex
nanh doctor zed --json
```

The detailed command includes the local executable path, so review it before
sharing. Its JSON form deliberately excludes that path and reports the last
version confirmed compatible with this nan-harness release plus the latest live
verification evidence. Newer command-line and desktop harness versions produce a warning and
continue without requiring an override. Versions
below the supported minimum require `--allow-unsupported`, while command-line
harness versions whose output cannot be parsed require `--allow-untested`:

```sh
nanh claude --allow-untested
nanh codex --allow-unsupported
```

Release builds refresh compatibility evidence for CLI and Desktop harnesses
without replacing nan-harness. Evidence is specific to the installed nan-harness
release; Desktop records also identify the platform and any bundled runtime.
If the feed is unavailable, launches use valid cached evidence or the embedded
registry. `NAN_NO_COMPATIBILITY_CHECK=1` disables automatic refresh.

Older Desktop clients need a one-time upgrade to support this feed. New evidence
can confirm compatibility with a newer harness, but protocol changes that require
an adapter fix still need a nan-harness release. See the
[compatibility feed reference](canary/compatibility-feed.md).

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

Automatic checks and new installations offer the recommended release, the one
the maintainer has announced. `nanh update` asks for the newest published
release instead, so an explicit update can pick up a validated release before
it is recommended. Neither path ever downgrades an installation, and installing
a version explicitly does not subscribe it to unannounced releases.

Telemetry is off by default. When enabled, it sends sanitized diagnostics and
minimal usage data; it never includes prompts, model output, credentials, or
local paths. A model-specific diagnostic may include the NaN model ID. An
interactive error may still offer a one-time report when telemetry is off.

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
