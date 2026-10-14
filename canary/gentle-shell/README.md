# Gentle Shell qualification

Gentle Shell is a managed launcher variant of Pi, not a new provider protocol.
The initial qualified pairing is gentle-pi 4.1.0 with Pi 1.1.0 and Node 22.23.1
on macOS ARM64. Gentle Shell now has its own daily and release cells on Linux
ARM64, macOS ARM64 and Windows x64. The Pi cell cannot qualify Gentle Shell.
Each cell freezes and records both the gentle-pi package and Pi runtime version,
with the hosted Node runtime; a changed pair must be requalified.
On Windows, the official installer builds its Gentle AI companion from source
and requires Go 1.25.10 or later. Hosted Windows cells provision that toolchain;
the CLI checks it before offering installation and explains how to install it.

The local synthetic probe runs the real launcher and runtime against an
in-process authenticated provider. No real API key is needed or used:

```sh
npm install --prefix /tmp/gentle-qualification gentle-pi@4.1.0 @earendil-works/pi-coding-agent@1.1.0
cargo build --locked -p nan-harness-cli --bin nanh
python3 canary/gentle-shell/check.py \
  --nanh target/debug/nanh \
  --gentle-shell /tmp/gentle-qualification/node_modules/.bin/gentle-shell
```

Use a disposable HOME for package installation too, since upstream postinstall
may provision native companions. The probe creates its own disposable home and
project. It checks native saved credentials, direct native routing, mandatory
gateway routing despite saved URL overrides, provider-file preservation,
direct fallback to nanh credentials and failure rather than account switching
for malformed native credentials. Hosted doctor evidence verifies the actual Pi runtime version separately from
the launcher. An advisory about a newer Pi version is not a contract failure. The scripted provider also drives a
real child agent to read a fresh random fixture through the gateway. Native search
and Engram-shaped memory fixtures verify preservation; forced search uses a local
SearXNG-protocol fixture. This does not certify every Engram release.

## Initial evidence: 2026-10-12

- Pi 1.1.0 passed the native inventory and read/write/edit/bash/grep/find/ls
  conformance round-trip against the scripted provider.
- Real NaN inference passed for the main agent through the gateway.
- A real Gentle Shell subagent used NaN, invoked Pi's read tool on a synthetic
  fixture, and returned its contents. The gateway counted all four parent and
  child requests.
- Engram 3.4.0, running against a separate temporary database, remained usable
  through the managed launcher; the memory search tool retrieved a synthetic
  observation previously saved through Gentle Shell.
- Forced NaN search reached a local SearXNG-protocol fixture and returned its
  result through the gateway. Without a configured backend, it returned the
  existing search-unavailable error rather than starting a backend implicitly.
- Automated JavaScript contracts exercise native provider replacement, locked
  request routing despite model URL/header overrides, preserved native mode,
  inherited child package paths, and automatic/forced search precedence.

These tests do not qualify native review workflows, every third-party extension,
or the user's remote Orca deployment. Compaction overrides are explicitly
unsupported for this launcher. The ordinary release gate still qualifies the
other harnesses and desktop assets independently.

## Hosted contract

`check.py --output <path>` emits only closed check names and a passed status.
`--live --model qwen3.6` additionally requires `NAN_API_KEY` and observes a real
parent tool call, the child's actual NaN model/read transcript, and gateway usage.
The marker is generated fresh and is never supplied to the model in the prompt.
No prompts, responses, credentials or transcripts are published as evidence.

The daily selector checks the launcher capability manifest in the immutable
release commit; old canary binaries without the independent report identity
remain unavailable for this cell. New publication requires all 52 CLI cells
plus the existing twelve Desktop cells. Historical 49-cell receipts remain
valid for recommending their already-published version, not for new publication.
