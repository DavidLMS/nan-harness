# Zed desktop integration

> **Experimental:** Zed 1.18.0 is live-verified on macOS. Linux and Windows
> remain contract-only, and newer Zed versions require `--allow-untested`.

Zed support is an experimental, launch-only desktop integration. It is kept
outside the stable harness registry, pinned conformance suite, canaries, and
release compatibility feed.

Run it with:

```sh
nanh zed --model qwen3.6 /path/to/workspace
```

The `zed-desktop` command is an alias. Zed must be completely closed before a
managed launch. nan-harness creates a temporary `language_models.openai_compatible.nan`
provider, selects it through `agent.default_model`, and restores the previous
JSONC settings after Zed is confirmed closed. If recovery cannot finish, close
Zed and run:

```sh
nanh zed --restore
```

The provider credential stays in nan-harness. Zed receives only a launch-scoped
`NAN_API_KEY` accepted by the authenticated loopback Chat Completions gateway.
The provider catalog comes from the current account at launch time; nan-harness
does not add edit predictions, search, MCP servers, agent profiles, or persistent
Zed preferences.

Use `nanh zed --dry-run` for an inert, redacted plan and `nanh doctor zed --json`
for local compatibility evidence. Zed installation and updates remain explicit
operator actions; if Zed is missing, the command points to the official download
or accepts `--executable`. The current [spike evidence](zed-desktop-spike.md)
records GO for the experimental macOS surface only.
