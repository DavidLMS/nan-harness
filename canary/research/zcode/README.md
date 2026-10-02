# ZCode feasibility investigation

Investigated on 2026-10-02, on macOS arm64, from nan-harness branch
`feat/zai-code`. This document records the initial research and the subsequent
production integration described below. Historical probe notes retain their
original evidence boundaries; shared live qualification is recorded below.

## Conclusion

Integrating the official Z.ai harness is viable. Use direct OpenAI Chat
Completions, its native provider registry, and nan-harness's existing private
artifact and configuration-receipt mechanisms. No additional protocol bridge
is justified by the exercised behavior. The implementation is moderately
involved: ZCode's provider configuration and resumed-session behavior differ
from MiMo Code.

The implemented canonical command is `nanh zcode`, with `zai` and `zai-code`
aliases, and native setup through `nanh config zcode` using the same aliases.
This refers to [zai-org/ZCode](https://github.com/zai-org/ZCode), not similarly
named community CLIs or GLM integrations.

## Exact upstream scope

- Source commit: `29628c9acdb81b703bbd4080c207a0e7ce5e276e`.
- Built agent CLI: `0.16.9`, confirmed with its actual `--version` command.
- Product/distribution version: `3.14.3`. The installed wrapper's `--version`
  prints this version; `zcode version` prints the agent version `0.16.9`.
  Use the latter when checking the agent's compatibility contract.
- Runtime used for final probes: Node.js `24.14.0`.
- Build: upstream's locked pnpm installation, then
  `pnpm --filter '@zcode/cli...' build`. Installation lifecycle scripts were
  disabled; the native probes exercised the headless CLI.
- The full distribution wrapper and unmodified Unix installer were exercised
  with a locally built package on macOS arm64. This is not an upstream
  released artifact. The rendered terminal UI was also exercised locally.
  Web mode was not exercised. Backward compatibility before agent version
  `0.16.9` has not been established.

The [official source at the investigated commit](https://github.com/zai-org/ZCode/tree/29628c9acdb81b703bbd4080c207a0e7ce5e276e)
is the reference. Older examples that put `provider`, `model.main` and
`model.lite` in `~/.zcode/cli/config.json` describe an earlier configuration
generation and are not the integration contract for this revision.

## Verified behavior

| Probe | Result | Evidence boundary |
| --- | --- | --- |
| Current personal-provider schema and encode/decode round trip | Pass | Real upstream decoder; synthetic fixtures |
| Future schema version and unsupported `apiKeyEnv` field | Rejected | Do not invent an environment-key reference |
| Custom `nan` provider and previously unknown model | Admitted | Real registry resolver, no Z.ai account |
| TUI login gate with/without a selectable model | Pass | Actual metadata function; not a rendered TUI test |
| Explicit built-in provider isolation | Only NaN selectable | Real registry resolver |
| Authenticated direct Chat Completions and selected model | Pass | Actual upstream model factory and loopback HTTP |
| SSE termination and usage | Pass | `[DONE]`, streamed usage and `include_usage` |
| Fragmented tool arguments, execution and result continuation | Pass | Arguments span separate SSE writes; actual SDK tool loop |
| Image serialization | Pass | Actual model factory emits a PNG data URL; no claim about native attachment gates or model vision quality |
| HTTP 401 and cancellation | Pass | Model factory/SDK; not OS signal handling |
| Native Read → Write → Read → Edit → Bash → Agent | Pass | Built official CLI; file effect, tool continuations and child request verified |
| Native provider/model confinement | Pass | Eight requests in the tested sequence, all to the loopback provider and selected model |
| Model catalog replacement and credential rotation | Pass | A fresh native process reads the replacement private configuration |
| Child model inheritance and resumed pin retention | Pass | Actual upstream policy; an old non-NaN pin is retained rather than silently replaced |
| Auxiliary reasoning/output policy | Pass | Lowest advertised reasoning level and bounded output budget |
| Official Unix installer and repeat installation | Pass | Locally built full distribution; default/custom paths; existing CLI settings preserved |
| Installed full wrapper's native tools | Pass | Same real tool sequence and rotation probe through the installed entry point |
| Rendered TUI and `/model` | Pass on macOS arm64, Linux arm64 and Windows x64 | Real terminal, persisted selection, request using new model, streamed answer and Ctrl-C exit |
| Real `--continue` and explicit `--resume` | Pass on all three tested platforms | Subsequent provider requests contain previous session history |
| Resume after selected model removal | Safe failure | Nonzero exit and zero additional provider requests |
| NaN search MCP | Pass on local macOS | Actual `nan-harness __search-mcp`, inventory, execution and result continuation; CLI config unchanged |
| Linux arm64 and Windows x64 native tools | Pass | [Pinned hosted run](https://github.com/DavidLMS/nan-harness/actions/runs/36968640065), including config, transport, child, rotation, session and TUI probes |

The branch-only [feasibility workflow](../../../.github/workflows/zcode-feasibility.yml)
exercises the pinned source on Linux arm64 and Windows x64 (ConPTY). The final
run tested branch commit `0156c472`; both platform jobs passed. macOS results
come from local execution. The workflow does not publish compatibility feeds.

Every key, prompt, file and response in these probes is synthetic. Native runs
use temporary homes, data directories and Git workspaces, a private provider
file, disabled model telemetry and a loopback ZCode service origin. They do
not read or change the user's installed ZCode configuration. Probe output
contains assertions and counts, not request bodies or credentials.

## Implementation implications

### Official distribution recheck

Rechecked on 2026-10-02 before starting command implementation:

- Official `main` still points to the investigated source commit
  `29628c9acdb81b703bbd4080c207a0e7ce5e276e`.
- The [official GitHub release](https://github.com/zai-org/ZCode/releases/tag/v3.14.3)
  exposes `ZCode-3.14.3-mac-arm64.dmg` and
  `ZCode-3.14.3-win-x64.exe` as uploaded assets. Neither is a standalone CLI
  distribution. GitHub-generated source archives remain available.
- The [official installation guide](https://zcode.z.ai/en/docs/install)
  describes installing the desktop application on Windows and launching it
  from the Start menu or desktop. It does not document installing a terminal
  `zcode` command.
- The official README documents building the CLI from source. Its installer
  download origin remains a publisher-supplied URL, not a published upstream
  CLI channel. The supplied distribution installer is a Unix shell script;
  no first-party PowerShell installer or Windows command shim was found.

The confirmed official route for the tested standalone agent is therefore a
source build. The user authorized automatic installation from that official
source, including Windows, rather than bundling a CLI or waiting for a binary
release. nanh installs the pinned revision independently and adds a checked
entrypoint binding to the upstream `run` API's `projectConfigPath`; this enables
private, per-launch MCP configuration while preserving ordinary startup. The
headless app also forwards that public option; upstream currently forwards it
only in its TUI app creation. Both source anchors are checked before building.
This is a nanh-managed source build, not an upstream Windows binary installer.
Do not substitute desktop installers, npm namesakes or third-party repackaging
for the official source.

1. **Provider configuration:** use
   `ZCODE_PERSONAL_PROVIDER_CONFIG_FILE` to point at a schema-version-1 file.
   Its important fields are `providerConfigRules.providerRules`,
   `modelConfigRules.providerModelRules` and `defaultModelSelection`.
   Native defaults resolve below `ZCODE_DATA_BASE_DIR` (or the user home) to
   `.zcode/v2/provider_config.json`. CLI features and MCP still have a separate
   configuration surface. Respect both path systems.
2. **Managed launch:** create private, cleanup-owned provider artifacts through
   the shared runtime. Keep the real key behind a `SecretRef` in the launch
   plan and substitute it only at runtime; do not serialize it in dry-run
   output. ZCode requires an inline key in the materialized provider file.
   Use an explicit built-in configuration as well: merely selecting NaN does
   not remove providers made available by existing account credentials.
   The native probe validates a built-in file without provider/template
   entries, retaining upstream model rules for this experiment. Production
   model facts must instead be rendered from shared NaN capability profiles.
3. **Models:** populate the personal model list from NaN discovery and render
   per-model capabilities, limits and option maps. The upstream registry admits
   unknown model IDs, but that does not validate every inferred capability.
   Preserve conservative generic behavior. Auxiliary calls and new children
   inherit the active model in this revision; the old independent lite-model
   configuration is obsolete.
4. **Resumption and overrides:** `--resume`, `--continue` and rendered `/model`
   have been exercised. Extend adapter coverage to `/resume`, workflow-pinned
   models and explicit child-model selection.
   Restrict the registry so a retained foreign pin fails safely. Do not rewrite
   user session history or silently send a request to another provider.
   Guard or explicitly exclude the distribution wrapper's `--web` mode, which
   starts a separate server workflow.
5. **Native configuration:** implement configure, refresh, status, remove,
   rotation and uninstall with receipts. Preserve unrelated providers, model
   rules, settings and credentials, and restore any owned default selection.
   Establish and test native model switching semantics separately from the
   managed launch's restricted registry. Do not advertise strict native
   provider confinement until it has been demonstrated.
6. **Discovery and installation:** use `zcode version` for the agent version,
   and choose a compatibility manifest pin against an established released
   artifact. The tested Unix
   installer creates `~/.local/bin/zcode` and installs under
   `~/.zcode/runtime`, with `ZCODE_DIST_HOME`/`ZCODE_DIST_BIN_DIR` overrides.
   The README's distribution base URL is a placeholder; this investigation
   has not established a public, reproducible standalone CLI download channel.
   The [official product site](https://zcode.z.ai/en) advertised desktop
   `3.14.4` packages for macOS, Linux and Windows when checked; these are not
   evidence of a standalone CLI package or Windows CLI installer.
   Do not invent one or treat a source build as a released package.
7. **Full coverage:** integrate a pinned probe and latest canary only after
   the installation/version contract is settled. Remaining adapter tests
   include native configuration receipts and rollback, managed artifact
   cleanup, upgrades, legacy/project configuration conflicts, native image
   attachment gates and per-model reasoning option rendering. Research tests
   already cover search MCP, actual TUI selection and Ctrl-C exit, real
   resumption, fragmented tools and image transport serialization.
   These research probes are not substitutes for that complete daily gate.

Relevant official code: [provider schema](https://github.com/zai-org/ZCode/blob/29628c9acdb81b703bbd4080c207a0e7ce5e276e/packages/provider/src/config/provider-data-schema.ts),
[configuration codec](https://github.com/zai-org/ZCode/blob/29628c9acdb81b703bbd4080c207a0e7ce5e276e/packages/provider-node/src/provider-config-file-codec.ts),
[native model factory](https://github.com/zai-org/ZCode/blob/29628c9acdb81b703bbd4080c207a0e7ce5e276e/apps/zcode-cli/packages/adapters/src/model/model-execution.ts),
[child model policy](https://github.com/zai-org/ZCode/blob/29628c9acdb81b703bbd4080c207a0e7ce5e276e/apps/zcode-cli/packages/bootstrap/src/app/workflow-actor-model.ts),
[distribution documentation](https://github.com/zai-org/ZCode/blob/29628c9acdb81b703bbd4080c207a0e7ce5e276e/README.en.md).

## Reproduce

Use a fresh upstream checkout at the exact commit above, outside nan-harness,
with Node.js `24.14.0` and pnpm `10.33.2`. Run from the upstream checkout:

```sh
pnpm --filter '@zcode/cli...' install --ignore-scripts --frozen-lockfile
pnpm --filter '@zcode/cli...' build
node apps/zcode-cli/packages/cli/dist/zcode.cjs --version
```

From nan-harness, set the upstream path and a temporary output directory:

```sh
export ZCODE_SOURCE_ROOT=/absolute/path/to/isolated/ZCode
zcode_probe_output=$(mktemp -d)
node canary/research/zcode/build-probes.mjs "$zcode_probe_output"
node "$zcode_probe_output/config-probe.mjs"
node "$zcode_probe_output/protocol-probe.mjs"
python3 canary/research/zcode/native_probe.py \
  --source "$ZCODE_SOURCE_ROOT" --node "$(node -p 'process.execPath')"
```

The build helper bundles the real upstream source functions without altering
them; it resolves workspace source exports and uses the checkout's installed
dependencies. The protocol probe contacts only its own loopback server. The
native probe has a 60-second limit per process and deletes its isolated state.
Keep generated bundles and upstream dependencies outside the repository.

Additional real runtime probes:

```sh
python3 canary/research/zcode/runtime_probe.py \
  --source "$ZCODE_SOURCE_ROOT" --node "$(node -p 'process.execPath')" --case sessions
python3 canary/research/zcode/runtime_probe.py \
  --source "$ZCODE_SOURCE_ROOT" --node "$(node -p 'process.execPath')" --case tui
cargo build --locked -p nan-harness-cli
python3 canary/research/zcode/runtime_probe.py \
  --source "$ZCODE_SOURCE_ROOT" --node "$(node -p 'process.execPath')" \
  --nan-binary "$PWD/target/debug/nan-harness" --case mcp
```

On Windows the TUI probe requires `pywinpty==3.0.5`; other platforms use
Python's standard PTY transport. Terminal output is retained only in memory.

To reproduce the **locally built** full distribution and installer probe,
run these commands in the upstream checkout. A CLI-only filtered install is
insufficient for the server/Web distribution. The shared package must be
compiled explicitly before packaging at this source revision:

```sh
pnpm install --ignore-scripts --frozen-lockfile
pnpm exec tsc -p packages/shared/tsconfig.json
node scripts/build-zcode.mjs --base-url http://127.0.0.1:18762/
```

Then, from nan-harness:

```sh
python3 canary/research/zcode/installer_probe.py \
  --source "$ZCODE_SOURCE_ROOT" --node "$(node -p 'process.execPath')"
```

The installer probe serves that package on loopback, injects the temporary
distribution URL, tests default/custom directories twice, verifies both
version commands and runs real native tools through the installed wrapper.
It does not install into the user's home. These initial installer results did
not verify automatic download, Windows CLI installation or the nanh lifecycle;
the subsequent production probe below covers those contracts.

## Production integration and remaining matrix dimensions

The `feat/zai-code` implementation provides managed `nanh zcode` launches
(`zai` and `zai-code` aliases), native `nanh config zcode` lifecycle commands,
and an independent source installer pinned to the revision above. The installer
requires Git, Node.js 24.14 or later and pnpm 10.33.2, and publishes a Unix
launcher or a Windows `.cmd` command only after verifying the agent version and
configuration binding. Failed builds remain temporary; reinstalls verify the
owned receipt and preserve unrelated commands and user configuration.

Managed provider files contain a gateway session token, complete shared model
capabilities and only the current NaN catalog. Their empty built-in release has
schema version 1 and revision 0. The bundled-config environment override is
removed so upstream remote built-in refresh cannot reintroduce another provider.
Native configuration retains foreign providers and individual array members;
required empty rule containers do not take ownership of later user additions.

The repeatable production probe is:

```sh
python3 canary/research/zcode/integration_probe.py --binary target/debug/nanh
```

It installs from a fresh home through the actual terminal prompt, then checks
managed Write/Read/Edit/Agent behavior, private search MCP execution, native
configure/status/refresh/key rotation/remove and standalone command execution.
The `ZCode feasibility` workflow runs it on Linux ARM64, macOS ARM64 and Windows
x64. All provider payloads and credentials in these probes are synthetic.
The [hosted production run](https://github.com/DavidLMS/nan-harness/actions/runs/37005961113)
passed on all three platforms, including Windows process-tree cleanup.

The shared compatibility matrix now includes ZCode on all three native platforms.
The pinned conformance shard installs the verified revision and runs the published
inventory/tool/sentinel contract plus these complete source probes. The daily
canary resolves upstream main once, reads the agent version at that exact commit,
and uses the same source identity across platforms. It builds with the declared
Node/pnpm versions and repeats protocol, session, TUI, managed/native and search
checks before the live provider probe. Source commits are rechecked even if the
agent version has not changed. The installation pin remains unchanged.

The shared release publication gate now requires 49 live cells. Already published
receipts retain their historical matrix only on the recommendation path; no old
receipt qualifies a new publication. Daily selection reports a harness absent
from a published binary's embedded registry as unavailable, without passing it
or substituting a source-built nan-harness binary.

Real-account qualification uses the shared isolated tool probe: Write and Read
must succeed, read and completion markers must be present, and NaN usage must be
observed. Raw provider payloads and credentials remain private and are never
included in evidence artifacts.

The [shared live qualification run](https://github.com/DavidLMS/nan-harness/actions/runs/37067059161)
passed on Linux ARM64, macOS ARM64 and Windows x64 on 2026-10-02, using NaN
model `qwen3.6`, agent `0.16.9` at the upstream revision above, and nan-harness
commit `a1d3f133b965671f36b30b914607a438e337b414`. Each cell passed source
installation, the complete source probes, published conformance and the
real-account tool/streaming/usage probe. The final cell completed at
`2026-10-02T21:38:43Z`.
