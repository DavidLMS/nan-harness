# Desktop compatibility checker

`nanh-desktop-check` is an independent, opt-in test executable. It does not
invoke an upgrade of an existing `nanh` or application, submit reports by
default, or change the recommended nanh release. Native app self-updaters are
a separate risk that must be controlled before personal-machine rollout. See the
[distribution catalog and bootstrap commands](checker-platforms.md).

## Qualification status

The runner, report validation and lifecycle contracts have local automated
tests. The five real applications have **not yet been qualified** on the native
Actions matrix. Do not interpret the synthetic accessibility experiment or a
passing unit suite as application compatibility. Complete disposable-runner
qualification before distributing this checker for personal-machine use.

Native Zed 1.18.1 qualification on 2026-09-08 passed three complete deterministic
scenarios on both macOS architectures in run 34265009005, but subsequent input
verification failures exposed instability. Shorter synthetic prompts restored
three ARM64 passes in [run 34272339383](https://github.com/DavidLMS/nan-harness/actions/runs/34272339383).
Live verification remains untested. Linux now starts in Zed's stateless mode,
which avoids its single-instance socket exceeding the private journal path's
Unix socket limit; interaction and cleanup remain unqualified.

Windows probes default to `isolation-unavailable` before app launch. Native
qualification proved that redirected AppData
folders do not redirect the account's `UserProfile` known folder. Official Zed
can also use the account's credential store. The qualification workflow explicitly
selects `--session github-hosted --yes` on its fresh `windows-2025` VM. This
authorizes use of the disposable VM account, not a personal profile. The option
also rejects missing GitHub-hosted runner metadata and self-hosted runners.
Metadata is an operational guard, not an isolation mechanism or attestation;
the workflow's fresh VM, credential-free preparation and absence of personal
data are the boundary. Never copy this option into a personal session or fake
the runner variables. GitHub documents the
[fresh VM lifecycle](https://docs.github.com/en/actions/how-tos/manage-runners/github-hosted-runners/use-github-hosted-runners).
Ordinary managed `nanh` launches are unchanged.

A later local visual check on the same date completed a conversation and a
native `read_file` round trip in Zed 1.18.1 using a private app copy and profile.
The synthetic loopback provider verified the submitted input and returned tool
content; both the input and final response were independently read on screen.
The corrected launcher succeeded without an external terminal, exited cleanly,
and restored the profile settings. This is operator-driven deterministic
evidence, not a passing automated checker report or a live NaN verification.

On Unix, Zed reloads its login-shell environment when stdout is not a terminal.
Managed launches now supply a private output terminal in that case so the
launch-scoped credential survives. Its output is drained without recording
native logs. Existing interactive terminal behavior is unchanged. See Zed's
[environment model](https://zed.dev/docs/environment) for the native distinction.

For local visual inspection, bind actions to the verified app PID and window,
not merely its bundle ID: restoring focus by bundle ID opened the installed
app instead of the private copy during this check. Do not retry that operation
against a disappeared window. Confirm the visible caret before typing, wait
for the complete text to appear before sending, and verify the response outside
the input field against independent provider evidence. A private data directory
does not isolate global agent skills; a live personal-machine check still needs
that context boundary qualified before any real provider calls.

Zed probes now use its native `--user-data-dir`, disable automatic updates and
telemetry in that private profile, and handle the fresh workspace trust dialog
before opening the agent panel. macOS Zed does not honor `XDG_CONFIG_HOME` for
its normal settings. Other applications' self-update and profile boundaries
remain unqualified; do not launch them against personal installations yet.

The installer can unpack official DMG, gzip tar and supported DEB assets into
private directories. It does not run global setup executables, register Store
packages, install shared runtimes or build Hermes from source. Such missing
applications produce `installation-unavailable`, not a passing check. A release
asset's existence alone does not qualify its installation or GUI behavior.

Hosted preparation now installs official Windows MSIX/setup packages and builds
Hermes for Linux before checker execution, without a provider key. These operations
are refused outside disposable GitHub-hosted runners; see the distribution
catalog for the installation receipt and retention boundary.

macOS requires Accessibility permission for the checker. Linux
requires glibc, `libxkbcommon`, an accessible AT-SPI session and a working native
graphics stack; the hosted workflow prepares a D-Bus/Xvfb session. Wayland input
restrictions, onboarding, absent login and unsupported selectors remain explicit
non-passing outcomes. macOS apps launched through Launch Services may detach
from the owned process group; the checker refuses to send input without process
ownership evidence.

## Run and inspect

After the independent checker release has been published:

```sh
nanh-desktop-check run --app zed-desktop
nanh-desktop-check run --app zed-desktop --nan-harness /absolute/path/to/nanh
nanh-desktop-check run --yes --non-interactive --ephemeral
```

Without `--yes`, the checker displays its inventory and asks before test or
installation operations. Noninteractive runs without `--yes` do not proceed.
Apps already running are left alone. Ambiguous or unreadable installations do
not trigger replacement downloads. An absent nanh is resolved through the
official `available` manifest, with the downloaded checksum and version checked.
An existing nanh that lacks the integration's `--provider-base-url` option is
reported as unsupported for the probe and remains installed unchanged.

Each selected app gets three separate deterministic probes. Set `NAN_API_KEY`
in the launching environment to add one live probe, using `--model` to override
the default `qwen3.6`. The checker does not look up saved provider credentials.
Never place a key in command arguments or in a report. The real key stays in
the probe's local provider boundary; nanh receives a random session token.

Live probes allow at most four generation requests, 2,048 output tokens per
request and 180 seconds per app, including shutdown. Deterministic probes have
a 240-second process deadline. Requests are not retried by the checker. The
report records elapsed probe time, not a billing total. Provider generation
limits are upper bounds, not measured cost estimates.

Passing evidence requires GUI input readback, visible response text, a real
tool result containing the synthetic fixture marker, and completed provider
output. Deterministic probes also require controlled error recovery. A tool
result or an assistant claim by itself does not establish success. Live and
deterministic evidence remain independent.

Schema v2 reports record both the input method and the response verification
method. Accessibility is preferred; incomplete app trees can use xa11y input and
owned-window captures with bundled local OCR. Captures and recognized text are
not written to public reports or artifacts. Focus changes, occlusion, unexpected
windows, geometry changes and unsupported executable architectures fail closed.
Schema v1 reports remain readable without attributing visual capabilities to them.

For hosted or explicitly separated execution, run `prepare` without `NAN_API_KEY`
and keep its private receipt and downloaded checker. Then use `run --prepared
<receipt> --mode deterministic`; a separate `--mode live` invocation requires a
nonempty key and makes no installation changes. Both invocations must select
the same applications as preparation. Each writes its own sanitized report.

The final report path and exact SHA-256 digest are printed. Exit status 0 means
all requested checks passed (or operations were declined); 1 means the report
contains a non-passing requested check or cleanup problem; 2 means the command
could not finish. A missing key is an explicit skipped live probe, not a failure
of a deterministic-only run.

## Cleanup and recovery

Default cleanup removes only new, unchanged resources recorded by this run.
Existing applications, nanh binaries and shared packages are never uninstalled.
`--ephemeral` retains owned installations and the bootstrap's downloaded copy;
it does not skip process shutdown, configuration restoration or key protection.

Private journals live under `XDG_STATE_HOME/nanh-desktop-check` on Unix, falling
back to `~/.local/state/nanh-desktop-check`; Windows uses
`LOCALAPPDATA/nanh-desktop-check`. Reports contain no recovery paths, raw errors,
prompts, model output or credentials. Journals and reports are separate files.

If cleanup fails, retain the private run directory and use:

```sh
nanh-desktop-check cleanup <run-id>
```

Close the tested application first. Recovery checks the selected nanh binary's
digest and retries native restoration before sealing interrupted probe files.
Changed binaries, replaced paths, mounted disk images and interrupted unsealed
installations require inspection; the checker preserves them. It does not claim
automatic recovery after every forced shutdown. Do not upload the private
journal or remove it while native configuration still needs restoration.

## Share evidence deliberately

```sh
nanh-desktop-check validate-report /path/to/report.json
nanh-desktop-check submit /path/to/report.json
```

Submission displays the sanitized report and requires a separate confirmation.
It uses existing GitHub CLI authentication, with a browser fallback, and never
installs GitHub CLI or requests release-write credentials. An issue is only an
inbox: creating or editing one does not execute tests or update compatibility.

The maintainer runs `Approve compatibility evidence`, selecting `desktop-issue`
or `desktop-run`, the source issue/run and the exact reviewed report digest.
For Actions, also select its `desktop-report-<platform>-<app>` artifact. The
workflow freezes the reviewed bytes and rejects edits that change the digest.
Hosted runs contain separate `deterministic.json` and, when requested,
`live.json` reports. The digest selects exactly one of them; approve each track
separately. Legacy artifacts containing `report.json` remain accepted.
Approval trusts the maintainer's dispatch, not an author allowlist. The central
publisher still verifies the tested nanh's official binary and historical
registry before accepting an exact app/runtime/platform/architecture tuple.

`Manual Desktop compatibility checks` uses the published checker in one fresh
runner per selected app, with read-only permissions and no schedule. Live calls
require the explicit workflow option and protected `canary-live` environment.
Reports need a separate approval; Desktop checks never gate a nanh release.
See the [hosted canary runbook](README.md) for state initialization, CLI release
qualification, recommendation and the guarded Tart emergency path.
