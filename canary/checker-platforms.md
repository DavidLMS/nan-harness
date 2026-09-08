# Desktop checker distribution qualification

Official download sources checked on 2026-09-08. Availability of an installer
does not establish GUI conformance, authentication bypass, safe unpacking, or
support for an architecture inside an installer without architecture metadata.

| Surface | macOS | Windows | Linux |
| --- | --- | --- | --- |
| ChatGPT | Apple Silicon DMG | Store-signed MSIX; product `9PLM9XGG6VKS` | Official preview DEB for x64/ARM64 |
| Claude | Universal DMG | x64/ARM64 MSIX or setup | Official beta DEB for x64/ARM64 |
| Hermes | Single DMG bootstrap | Single EXE bootstrap | Official source-build route, not a prebuilt download |
| Pen | x64/ARM64 DMG | x64 setup; ARM64 coming soon | x64/ARM64 tarballs |
| Zed | x64/ARM64 DMG | x64/ARM64 setup | x64/ARM64 tarballs |

Sources: [ChatGPT desktop](https://learn.chatgpt.com/docs/app),
[ChatGPT Linux](https://learn.chatgpt.com/docs/linux/linux-app),
[ChatGPT Windows](https://learn.chatgpt.com/docs/windows/windows-app),
[Claude downloads](https://claude.com/download),
[Claude Linux installation](https://code.claude.com/docs/en/desktop-linux),
[Claude Windows deployment](https://support.claude.com/en/articles/12622703-deploy-claude-desktop-for-windows),
[ChatGPT Windows deployment](https://learn.chatgpt.com/docs/enterprise/windows-deployment),
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

## Credential-free preparation

Hosted Windows jobs install official MSIX, NSIS or Inno Setup packages before
running the checker. `canary/actions/prepare-desktop.ps1` refuses non-hosted
machines and any environment containing `NAN_API_KEY`; it does not enable
developer mode, grant virtualization access or update an existing installation.
Packages remain installed until the disposable runner is destroyed. The private
installation receipt records the downloaded checksum and installed package or
directory. It is not a public compatibility report.

Claude's Linux resolver selects the newest exact version for the native
architecture from Anthropic's package index, verifies its SHA-256, and extracts
the DEB without registering an APT repository or running package scripts.
Hermes Linux jobs build the official source in a fresh runner directory and
record its source commit; neither route substitutes a community wrapper.

The checker `prepare` command saves a private receipt with exact checker, nanh
and application identities. `run --prepared <receipt> --mode deterministic`
never installs or downloads. A separate `--mode live` step requires a nonempty
key and revalidates the prepared binaries. Its public report contains only the
live track, so unexecuted deterministic checks cannot be mistaken for new
evidence. Auto mode preserves the interactive combined-run behavior.
