# SearXNG local backend on Windows x64

The Windows x64 CLI supports the same local lifecycle as macOS and Linux:

```powershell
python.exe --version
tar.exe --version
nanh search setup --local
nanh search status --json
nanh claude --force-search
nanh search update
```

Python 3.10 or newer must be installed and available as `python.exe` on PATH.
Setup creates a private virtual environment and installs the pinned SearXNG
dependencies into it. It does not install a system Python distribution.
`tar.exe` is required to extract the verified source archive. Windows ARM64
local installation is not currently a supported target.

## Installation and ownership

The root is `%USERPROFILE%\AppData\Local\nan-harness\searxng`:

```text
.nanh-owned                    root ownership marker
current\source\                pinned SearXNG source
current\python\Scripts\        private Python virtual environment
current\settings.yml           private settings and generated internal secret
current\install.json           version, target, and archive integrity receipt
.staging\                      installation being prepared
.previous\                     rollback state during publication
```

The common installer pins its source in `searxng.rs` and verifies the archive
digest before writing backend state. It creates private directories using the
[Windows DACL contract](../../SECURITY.md#local-private-files). An unmarked
pre-existing root is preserved and rejected. Failed installation commands leave
the previously published installation intact; a retry reconciles only owned
staging and rollback directories.

The direct server command is `current\python\Scripts\python.exe -m
searx.webapp`, with `current\source` as its working directory. Private settings
enable JSON search and bind the service to `127.0.0.1:8888`. The shared supervisor
owns the process through a Windows Job Object, checks readiness, and keeps it
alive while sessions hold search leases. No startup task is registered.

`nanh search disable` removes the saved endpoint and retains the installed
backend. To remove a configured owned backend, use `nanh search remove` instead.
Update and removal reject active search sessions and preserve unrelated files.

The earlier `searxng::windows` recipe is a separate contract-only API. Its
different layout and receipts are not adopted by the CLI.

## Verification

Run deterministic checks from this branch on Windows:

```powershell
cargo test --locked -p nan-harness-runtime --all-features searxng
cargo test --locked -p nan-harness-runtime --all-features search_supervisor
cargo test --locked -p nan-harness-cli --all-features search
```

The explicit live check downloads the pinned source and dependencies into a
temporary directory, starts the server, requests JSON search, and checks process
shutdown and removal. It requires network access and a free port 8888:

```powershell
cargo test --locked -p nan-harness-runtime --test searxng_live -- --ignored
```

Host-independent tests validate Windows plan construction, receipts, settings,
failed-update recovery, and cleanup. They do not establish native Windows
execution. Record the Windows version, Python version, commit, command, and
result when running the live check. Compilation requires Visual Studio Build
Tools and the Windows SDK; a cross-target check without those headers is not
native Windows validation.

On 2026-09-11, the live check passed on macOS ARM64 with Python 3.11. The
Windows cross-target check on that host stopped in `aws-lc-sys` because the
Windows SDK headers were unavailable. Native Windows execution remains pending;
the Windows CI job now runs both the contracts and the explicit live check.
