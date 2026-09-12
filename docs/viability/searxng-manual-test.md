# SearXNG search manual test runbook

This runbook exercises the public SearXNG configuration and launch-policy
surfaces. Setup requires a working backend and verifies it before saving state.
Docker, local installation, and remote search checks are operator-run checks,
not evidence that this documentation change contacted a live service.

## Scope and prerequisites

Use a built `nanh` binary and run the checks with no active nan-harness search
sessions. For a remote check, use an HTTPS SearXNG endpoint that you control or
trust. Do not put credentials, prompts, result payloads, or private paths in
the test output or a report.

The `NAN_HARNESS_CONFIG_DIR` override isolates `search.json`. Local and Docker
backend data also lives below the user home, so set a disposable home when
testing those backends. A config override by itself does not isolate backend
data.

On macOS or Linux:

```sh
TEST_ROOT="$(mktemp -d)"
export HOME="$TEST_ROOT/home"
export NAN_HARNESS_CONFIG_DIR="$TEST_ROOT/config"
export NAN_NO_UPDATE_CHECK=1
export NAN_NO_COMPATIBILITY_CHECK=1
mkdir -p "$HOME" "$NAN_HARNESS_CONFIG_DIR"
trap 'rm -rf "$TEST_ROOT"' EXIT
```

On Windows PowerShell:

```powershell
$TestRoot = Join-Path $env:TEMP ("nanh-search-" + [guid]::NewGuid())
$env:USERPROFILE = Join-Path $TestRoot "profile"
$env:APPDATA = Join-Path $TestRoot "appdata"
$env:LOCALAPPDATA = Join-Path $TestRoot "localappdata"
$env:NAN_HARNESS_CONFIG_DIR = Join-Path $TestRoot "config"
$env:NAN_NO_UPDATE_CHECK = "1"
$env:NAN_NO_COMPATIBILITY_CHECK = "1"
New-Item -ItemType Directory -Force -Path $env:USERPROFILE | Out-Null
New-Item -ItemType Directory -Force -Path $env:APPDATA | Out-Null
New-Item -ItemType Directory -Force -Path $env:LOCALAPPDATA | Out-Null
New-Item -ItemType Directory -Force -Path $env:NAN_HARNESS_CONFIG_DIR | Out-Null
```

Remove `$TestRoot` after the run if the PowerShell session did not exit. The
temporary environment prevents the checks from reading or overwriting the
normal nan-harness configuration.

## Remote lifecycle check

These commands do not require a NaN API key. Replace the example URL with a
reachable SearXNG endpoint:

```sh
nanh search status --json
nanh search setup --url https://search.example.test
nanh search status --json
nanh search disable
nanh search status --json
```

The first status reports an unconfigured backend. Setup probes the HTTPS
endpoint and persists it only when the probe succeeds; it therefore requires a
reachable endpoint for this step. The second status reports the remote mode,
`disable` removes the saved endpoint, and the final status is unconfigured
again. To test removal after an explicit backend setup, run `nanh search
remove`; it is destructive for an owned local or Docker backend.

## Backend and launch-policy checks

Run only the backend checks that match the environment. They can download or
start software and are not part of the deterministic documentation evidence.
Choose one backend path per isolated run; setup replaces the saved endpoint but
does not clean up an earlier owned backend.

```sh
# Private local SearXNG (supported macOS/Linux/Windows x64 targets)
nanh search setup --local
nanh search status --json
nanh search update

# Owned Docker SearXNG (requires a Docker daemon)
nanh search setup --docker
nanh search status --json
nanh search update

# Externally managed SearXNG (HTTPS is required)
nanh search setup --url https://search.example.test

# Choose one cleanup action after the checks: disable retains an owned backend.
nanh search disable
# Instead of disable, use this to remove the configured owned backend:
# nanh search remove
```

Check launch policy with inert plans:

```sh
nanh claude --dry-run --no-search
nanh cline --dry-run --force-search
nanh config pi --status
```

Automatic policy preserves a recognized existing search provider. `--no-search`
disables only the NaN fallback for that launch; `--force-search` selects NaN
search when another provider is present. Aider does not support the NaN
fallback. Native `nanh config` setup follows the same policy for harnesses that
support native configuration.

## Windows executable notes

The Windows x64 CLI supports `nanh search setup --local`. Install Python 3.11
or newer with `python.exe` on PATH; Windows must also provide `tar.exe`.
See the [Windows local backend](searxng-windows.md) for commands and prerequisites.

When inspecting that installation, the owned executable is the virtual
environment's `python.exe` and the mapped command is `python.exe -m
searx.webapp` from the `current/source` working directory:

```powershell
$Root = Join-Path $env:USERPROFILE "AppData\Local\nan-harness\searxng"
$Source = Join-Path $Root "current\source"
$Python = Join-Path $Root "current\python\Scripts\python.exe"
$Settings = Join-Path $Root "current\settings.yml"
$env:SEARXNG_SETTINGS_PATH = $Settings
Push-Location $Source
& $Python -m searx.webapp
Pop-Location
```

The recipe binds to loopback `127.0.0.1:8888`, uses no shell wrapper, and does
not register startup tasks. Do not claim that this standalone process was
started unless the check was actually run on Windows.

## Evidence and limitations

Record the exact command, platform, binary version, and result for every
operator-run backend check. Deterministic CLI tests and Markdown/link checks
are sufficient for the documentation change. This runbook does not claim a
live Docker daemon, local SearXNG installation, remote endpoint, or Windows
execution unless a separately recorded operator run provides that evidence.
