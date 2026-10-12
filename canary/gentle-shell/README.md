# Gentle Shell qualification

Gentle Shell is a managed launcher variant of Pi, not a new provider protocol.
The initial qualified pairing is gentle-pi 4.1.0 with Pi 1.1.0 and Node 22.23.1
on macOS ARM64. The existing hosted Pi cell does not by itself qualify Gentle
Shell. Do not infer full upstream-version or platform coverage from that cell.

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
for malformed native credentials. It also verifies that the parsed Pi version
does not produce a launcher-version warning.

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
