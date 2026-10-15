# OpenCode generations

`nanh opencode` supports both generations. It detects the version reported by the
executable, rather than inferring it from the command name. With both generations
installed, an interactive first launch asks which installation to use and saves
that choice. A valid saved choice is reused. With neither installed, it offers
V2. Installation still requires the normal consent.

`nanh opencode --major 1` selects V1. `nanh opencode --major 2` and
`nanh opencode2` select V2 without changing the saved default or asking which
generation to install. An explicit `--executable` must match the requested major.
Noninteractive ambiguity requires an explicit selection. Managed installations
use separate npm prefixes; they do not replace commands already on PATH.

V2 launches use a private standalone service. Native model catalog and command
arguments differ from V1 and are rendered separately. Persistent configuration
uses the shared compatibility schema and an owned private key file. Refresh can
reload a verified local V2 service without starting one or modifying its database.

## Qualification evidence

V1 retains its existing compatibility manifest evidence. V2 has a separate
`.github/workflows/opencode-v2-conformance.yml`, pinned to the official
`@opencode/cli@2.0.27` package, with Linux, macOS and Windows jobs, required by CI and release draft creation.
The live release gate additionally requires three independent V2 tool probes
against the attested release binaries. Their evidence is separate from the
52 logical-harness cells, whose OpenCode entry remains V1. A workflow
being present does not establish that its hosted jobs have passed.

Local macOS qualification against the real 2.0.27 executable and synthetic,
authenticated HTTP providers passed these scenarios:

- Alias launch with native write/read tool calls and observable file contents.
- Explicit-major managed launch with the selected model and authentication.
- Persistent file credentials, key rotation, maximum reasoning and removal.
- Concurrent private launches with distinct providers, natural process cleanup,
  and preservation of a preexisting isolated shared service.
- Native V2 catalog variants transmitting the requested reasoning effort.

These tests do not establish live NaN provider compatibility, MCP roundtrips,
interactive TUI signal behavior, or hosted Linux/Windows results. They use no
user credentials or user service instances.

Run the native qualification with an explicit V2 executable:

```sh
NAN_OPENCODE_V2_EXECUTABLE=/path/to/opencode2 cargo test --locked \
  -p nan-harness-cli --test conformance_opencode_v2 \
  --test conformance_opencode_v2_lifecycle -- --ignored --test-threads=1
```
