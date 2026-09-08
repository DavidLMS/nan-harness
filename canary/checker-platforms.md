# Desktop checker distribution qualification

Official download sources checked on 2026-09-08. Availability of an installer
does not establish GUI conformance, authentication bypass, safe unpacking, or
support for an architecture inside an installer without architecture metadata.

| Surface | macOS | Windows | Linux |
| --- | --- | --- | --- |
| ChatGPT | Apple Silicon DMG | Microsoft Store product `9PLM9XGG6VKS` | Official preview DEB for x64/ARM64 |
| Claude | Universal DMG | x64/ARM64 setup | No official installer advertised |
| Hermes | Single DMG bootstrap | Single EXE bootstrap | Official source-build route, not a prebuilt download |
| Pen | x64/ARM64 DMG | x64 setup; ARM64 coming soon | x64/ARM64 tarballs |
| Zed | x64/ARM64 DMG | x64/ARM64 setup | x64/ARM64 tarballs |

Sources: [ChatGPT desktop](https://learn.chatgpt.com/docs/app),
[ChatGPT Linux](https://learn.chatgpt.com/docs/linux/linux-app),
[ChatGPT Windows](https://learn.chatgpt.com/docs/windows/windows-app),
[Claude downloads](https://claude.com/download),
[Hermes website](https://hermes-agent.nousresearch.com/),
[Hermes Desktop README](https://github.com/NousResearch/hermes-agent/tree/main/apps/desktop),
[Pen downloads](https://www.pen.dev/downloads),
[Zed installation](https://zed.dev/docs/installation), and
[Zed release assets](https://github.com/zed-industries/zed/releases/latest).

The catalog uses only these publishers, never community Linux wrappers for
Claude or similarly named third-party Hermes applications. It does not execute
downloaded installers. Store and source-build distributions remain distinct
from archives that can be unpacked into a private run root. Hermes bootstrap
architecture must be checked before execution; its public download page does
not expose architecture-specific URLs. The ChatGPT DEB can be extracted
without registering its package repository; global APT installation would
introduce state outside the checker's private run root.

Discovery canonicalizes aliases, refuses ambiguous installations and reports
unreadable or incomplete installations rather than treating them as absent.
Application metadata is read without starting Electron applications. Missing
or non-semver metadata stays unknown; no version is inferred from a download
filename. `--version` is reserved for Zed and the bundled Codex runtime.

## Checker distribution

The independent `Release Desktop checker` workflow is manual-only and accepts
an existing `desktop-check-vX.Y.Z` tag reachable from main. Its five native
artifacts cover macOS Intel/Apple Silicon, Linux x64/ARM64 and Windows x64.
Linux checker binaries use GNU/glibc and require `libxkbcommon`; they are not the
musl binaries distributed by `nanh`.

The `desktop-check` prerelease channel stores a version pointer. Bootstrap
scripts resolve that pointer to an immutable checker tag before downloading
its binary and `SHA256SUMS`. Neither checker publication nor this channel
changes GitHub's recommended `latest` release or nanh's `available` channel.

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://github.com/DavidLMS/nan-harness/releases/download/desktop-check/bootstrap-desktop-check.sh | sh -s -- --yes
```

```powershell
& ([scriptblock]::Create((Invoke-RestMethod 'https://github.com/DavidLMS/nan-harness/releases/download/desktop-check/bootstrap-desktop-check.ps1'))) --yes
```

Pass `--ephemeral` to retain the newly downloaded checker, or set
`NAN_DESKTOP_CHECK_VERSION` to pin a checker version. These bootstraps never
change PATH or overwrite a previously installed checker. Default cleanup runs
after the checker process exits, including on Windows. Publication must happen
before these download commands can be used; repository source alone does not
make a channel available.
