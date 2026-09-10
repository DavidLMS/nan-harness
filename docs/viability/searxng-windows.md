# SearXNG Windows x64 standalone recipe

This is a contract-only recipe for a per-user SearXNG instance on Windows 10
or Windows 11. It does not add a CLI command, register a logon task, or start
SearXNG automatically.

## Layout and ownership

The default root is `%LOCALAPPDATA%\nan-harness\searxng`:

```text
searxng-src\                 pinned upstream source
searx-pyenv\Scripts\        Python virtual environment and python.exe
config\settings.yml         local SearXNG settings
state\                       local mutable state
logs\                        supervisor-owned logs
install-receipt.json          private version/integrity record
.nanh-owned                   private root ownership marker
.install.lock                 install and recovery lock
.staging\                    marker-owned retry staging area
```

The runtime creates and hardens the root only when it can publish and verify
the exact ownership marker; an existing unmarked root is preserved and
rejected. It then hardens owned paths using the private filesystem contract in
[`SECURITY.md`](../../SECURITY.md). Interrupted cleanup removes
`.staging` only when its exact ownership marker is present; foreign or
obstructed state is preserved.

## Pinned source record

The Windows x64 contract records both the immutable source commit and archive
digest because SearXNG is a rolling release:

```text
Version:    2026.9.8
Commit:     3fdc6d753
Archive:    https://github.com/searxng/searxng/archive/3fdc6d753.tar.gz
SHA-256:    99656d7b2b72b97c716b0d43f83536183dc8cec94aeb6dbda604c5d4f0b672e0
Target:     x86_64-pc-windows-msvc
```

The installer must verify the archive before extraction and persist the same
record in `install-receipt.json`. A receipt from another target, revision, or
digest is not accepted.

## Explicit install and start

The equivalent manual command is direct Python module invocation:

```powershell
$Root = Join-Path $env:LOCALAPPDATA "nan-harness\searxng"
$Source = Join-Path $Root "searxng-src"
$Python = Join-Path $Root "searx-pyenv\Scripts\python.exe"
$Settings = Join-Path $Root "config\settings.yml"

# Download and verify the pinned archive before extracting it into $Source.
py -3.11 -m venv (Join-Path $Root "searx-pyenv")
& $Python -m pip install --upgrade pip setuptools wheel
& $Python -m pip install --no-build-isolation -e $Source

# Start only after an explicit user/supervisor request.
$env:SEARXNG_SETTINGS_PATH = $Settings
& $Python -m searx.webapp
```

The mapped command is exactly `python.exe -m searx.webapp` with
`searxng-src` as its working directory and a loopback `127.0.0.1:8888`
binding. No shell or PowerShell wrapper is required. Managed runtime children
use a Windows Job Object so descendant processes remain owned by the
supervisor during cancellation and cleanup.

There is deliberately no startup registration step. Search-provider
selection, model routing, bridge behavior, and CLI behavior remain in their
existing components and are not changed by this Windows slice.

## Validation note

The Windows target requires an MSVC toolchain and SDK headers. On a host
without those headers, `cargo check --target x86_64-pc-windows-msvc` reaches
the native dependency build and stops with missing `windows.h` or `stdlib.h`;
run the target gate on Windows with the Visual Studio Build Tools installed.

Reference: [SearXNG's installation guide](https://docs.searxng.org/admin/installation-searxng.html).
