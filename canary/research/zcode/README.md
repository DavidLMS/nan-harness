# ZCode feasibility investigation

Investigated on 2026-10-02, on macOS arm64, from nan-harness branch
`feat/zai-code`. These are research probes, not an implemented adapter or
evidence of complete harness parity.

## Conclusion

Integrating the official Z.ai harness is viable. Use direct OpenAI Chat
Completions, its native provider registry, and nan-harness's existing private
artifact and configuration-receipt mechanisms. No additional protocol bridge
is justified by the exercised behavior. The implementation is moderately
involved: ZCode's provider configuration and resumed-session behavior differ
from MiMo Code.

The proposed canonical command is `nanh zcode`, with `zai` and `zai-code`
aliases, and native setup through `nanh config zcode` using the same aliases.
This refers to [zai-org/ZCode](https://github.com/zai-org/ZCode), not similarly
named community CLIs or GLM integrations.

## Exact upstream scope

- Source commit: `29628c9acdb81b703bbd4080c207a0e7ce5e276e`.
- Built agent CLI: `0.16.9`, confirmed with its actual `--version` command.
- Product/distribution version: `3.14.3`. The unified distribution wrapper
  prints this version, while the underlying agent prints `0.16.9`.
- Runtime used for final probes: Node.js `24.14.0`.
- Build: upstream's locked pnpm installation, then
  `pnpm --filter '@zcode/cli...' build`. Installation lifecycle scripts were
  disabled; the native probes exercised the headless CLI.
- The full distribution wrapper, installer, graphical TUI and Web mode were
  not exercised. No minimum supported version has been established.

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
| Tool execution and result continuation | Pass | Actual SDK tool loop against synthetic provider |
| HTTP 401 and cancellation | Pass | Model factory/SDK; not OS signal handling |
| Native Read → Write → Read → Edit → Bash → Agent | Pass | Built official CLI; file effect, tool continuations and child request verified |
| Native provider/model confinement | Pass | Eight requests in the tested sequence, all to the loopback provider and selected model |
| Model catalog replacement and credential rotation | Pass | A fresh native process reads the replacement private configuration |
| Child model inheritance and resumed pin retention | Pass | Actual upstream policy; an old non-NaN pin is retained rather than silently replaced |
| Auxiliary reasoning/output policy | Pass | Lowest advertised reasoning level and bounded output budget |

Every key, prompt, file and response in these probes is synthetic. Native runs
use temporary homes, data directories and Git workspaces, a private provider
file, disabled model telemetry and a loopback ZCode service origin. They do
not read or change the user's installed ZCode configuration. Probe output
contains assertions and counts, not request bodies or credentials.

## Implementation implications

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
4. **Resumption and overrides:** test `--resume`, `--continue`, `/resume`,
   `/model`, workflow-pinned models and explicit child-model selection.
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
6. **Discovery and installation:** account for the two version surfaces before
   choosing a compatibility manifest pin and command. The documented Unix
   installer creates `~/.local/bin/zcode` and installs under
   `~/.zcode/runtime`, with `ZCODE_DIST_HOME`/`ZCODE_DIST_BIN_DIR` overrides.
   The README's distribution base URL is a placeholder; this investigation
   has not established a public, reproducible upstream download channel.
   Do not invent one or treat a source build as a released package.
7. **Full coverage:** integrate a pinned probe and latest canary only after
   the installation/version contract is settled. Cover NaN search MCP,
   native configuration lifecycle, actual TUI model selection, streaming
   fragmentation, images/reasoning where supported, OS signals and cleanup,
   upgrades, legacy/project configuration conflicts, and Linux/Windows.
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
  --source "$ZCODE_SOURCE_ROOT" --node "$(command -v node)"
```

The build helper bundles the real upstream source functions without altering
them; it resolves workspace source exports and uses the checkout's installed
dependencies. The protocol probe contacts only its own loopback server. The
native probe has a 60-second limit per process and deletes its isolated state.
Keep generated bundles and upstream dependencies outside the repository.
