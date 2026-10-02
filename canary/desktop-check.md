# Desktop compatibility checker

`nanh-desktop-check` is an independent, opt-in test executable. It does not
invoke an upgrade of an existing `nanh` or application, submit reports by
default, or change the recommended nanh release. Native app self-updaters are
a separate risk that must be controlled before personal-machine rollout. See the
[distribution catalog and bootstrap commands](checker-platforms.md).

## Qualification status

The hidden launch-wrapper option is restricted to deterministic ChatGPT startup
diagnostics. Only application launch is instrumented; help, version and restore
still use the tested nanh directly. Wrapped runs save a private diagnostic
envelope containing the wrapper digest and observed result, not a public
compatibility report. `validate-report`, `submit` and `feed-updates` reject that
envelope. Do not extract its observation and publish it as an ordinary run.
Closed startup facts remain separate from the envelope; neither contains raw
application output.

The runner, report validation and lifecycle contracts have local automated
tests. Four cells have passed three complete deterministic native probes:
Zed 1.22.0 on macOS ARM64 in [run 36976970895](https://github.com/DavidLMS/nan-harness/actions/runs/36976970895),
Zed 1.22.0 on Windows x64 in [run 36996850316](https://github.com/DavidLMS/nan-harness/actions/runs/36996850316),
and Hermes 0.17.6 on Linux x64 and macOS ARM64 in
[run 36978761448](https://github.com/DavidLMS/nan-harness/actions/runs/36978761448).
All verify response, real file-tool use, controlled provider failure, UI Retry
recovery and application, global and clipboard cleanup without OCR. Hermes
observes Chromium 144.0.7559.236; Electron's separate version and Zed's runtime
version remain unobserved. Exact commits and artifact hashes are recorded in
each closed result. Eleven cells in the five-application, three-platform matrix
remain unqualified. The three Pen cells are blocked by vendor activation: no
official account-free route is established, and no test account is authorized.
Personal-machine isolation and live-provider behavior still require separate
evidence.

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

Each probe disables the detached nan-harness coordinator inside its private
environment. The checker owns the forwarding budget, and a persistent coordinator
would outlive the app and retain locks in the disposable profile. This does not
stop or reconfigure an existing user coordinator or change normal harness launches.

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

## Semantic deterministic qualification

The manual `desktop-check-qualification.yml` workflow evaluates the initial
Linux x64, macOS ARM64 and Windows x64 matrix. A pending backend remains an
explicit unqualified cell and prevents aggregate acceptance. Current qualified semantic
adapters cover Zed on macOS ARM64 and Windows x64, and Hermes on Linux x64 and
macOS ARM64; additional application and platform adapters require their own
native evidence.

Until the qualification workflow is present on the default branch, use the
registered `Desktop semantic automation feasibility` workflow on the integration
branch with `experiment=deterministic-full`. Select `native_only=true` while
iterating. For the final tree, select `app=all`, `native_only=false` and
`quality_only=false` to run both native adapters and the repository gate on the
same commit. Select `quality_only=true` when only the repository gate is needed. The older
`native-copy-dom` experiment remains feasibility evidence and cannot satisfy
the full acceptance gate. Implemented adapters are accepted only when their
full native result meets the gate; implementation and feasibility alone do not
qualify a cell.

Four cells are qualified, leaving eleven remaining cells. The following
observations describe completed runs, rather than additional qualification:

| Application | Linux x64 | macOS ARM64 | Windows x64 |
| --- | --- | --- | --- |
| Zed | Response and file-tool steps pass; pointer and accessibility Retry acknowledgements do not produce provider recovery. | Three complete probes pass the expanded retry policy. | Three complete probes pass with direct native clipboard transport and all cleanup checks. |
| Hermes | Three complete probes pass with renderer process ownership. | Three complete probes pass with private native userData and bounded cold-start waits. | One complete probe passes all five stages and cleanup. Two others fail attachment or initial response; three-pass acceptance remains unmet. |
| ChatGPT / Codex | Owned document loads without a composer; the latest completed startup diagnostic passes cleanup. | Renderer observation remains intermittent; cleanup passes. | Three owned documents load with twelve visible inputs and no composer; cleanup passes. |
| Claude | Three uninstrumented windows acquired. Explicit native accessibility returns zero visible editors or login buttons. Official account-free gateway remains unqualified. | Instrumented child exits with code 1; uninstrumented startup has no eligible window. | Stable owned window acquired; no editable controls observed and cleanup fails. |
| Pen | Software GLES removes GPU startup failure; three fresh sessions show sign-in and no editor. No official account-free route found for the frozen release. | Two probes expose composer/Send; another exposes sign-in. Conversation adapter remains unimplemented. | Owned renderer loads sign-in and a dialog; conversation adapter remains unimplemented. |

The completed evidence is available in runs
[36975359718](https://github.com/DavidLMS/nan-harness/actions/runs/36975359718)
(Zed Linux), [36975364554](https://github.com/DavidLMS/nan-harness/actions/runs/36975364554)
(Zed macOS), [36975369406](https://github.com/DavidLMS/nan-harness/actions/runs/36975369406)
(Hermes macOS), [36973164802](https://github.com/DavidLMS/nan-harness/actions/runs/36973164802)
(Zed Windows), [36972670961](https://github.com/DavidLMS/nan-harness/actions/runs/36972670961)
(Hermes), [36974215083](https://github.com/DavidLMS/nan-harness/actions/runs/36974215083)
(ChatGPT), [36972931753](https://github.com/DavidLMS/nan-harness/actions/runs/36972931753)
and [36971025105](https://github.com/DavidLMS/nan-harness/actions/runs/36971025105)
(Claude instrumented/startup baseline), and
[36974221876](https://github.com/DavidLMS/nan-harness/actions/runs/36974221876) (Pen).
Each closed artifact records its tested commit, platform, app/runtime version
when observed, preparation identity, probe steps and cleanup verdict. Renderer
inventories and startup baselines cannot satisfy full acceptance.

Run [36976970895](https://github.com/DavidLMS/nan-harness/actions/runs/36976970895)
reconfirms Zed macOS with three complete probes on commit `3e0bafdf`. Its Linux
probes still reject recovery despite acknowledged accessibility activation.
The Hermes jobs in run
[36976968063](https://github.com/DavidLMS/nan-harness/actions/runs/36976968063)
reach reduction but reject their evidence; their placeholder `not-run` artifacts
cannot establish either success or an application failure. Complete renderer
probes can exceed the former 32-record budget once policy, provider, backend,
frontend and Windows ownership diagnostics are retained. The reducer now bounds
64 records, each still subject to its closed schema and size limit, and exposes
only static rejection categories. Fresh run
[36978761448](https://github.com/DavidLMS/nan-harness/actions/runs/36978761448)
on commit `e54f1b92` qualifies Hermes Linux x64 and macOS ARM64 with three complete
probes each, 42 closed semantic records, and application/global/clipboard cleanup.

Run [36981298109](https://github.com/DavidLMS/nan-harness/actions/runs/36981298109)
rejects the Codex startup-switch hypothesis: supplying
`codex-browser-background-networking-disabled` also loses Linux ownership and
fails cleanup. The switch has been removed. Static inspection identifies an
application relaunch when the switch disagrees with in-app browser availability;
a fixed assumption is insufficient. A supported relaunch supervisor requires
separate implementation and native evidence.

Static inspection of the official Claude Linux 2.9939.4 archive identifies an
explicit startup rejection of `remote-debugging-port` and `remote-debugging-pipe`
in `.vite/build/index.pre.js`. Its exception requires a short-lived, signed
developer authorization token bound to the userData path. Ordinary packaged
startup also removes an unapproved `CLAUDE_USER_DATA_DIR` override. CDP therefore
remains unavailable for this distribution without upstream authorization;
normal native accessibility is the remaining route to evaluate on disposable
runners. Do not modify the distribution or fabricate this authorization.
The inspected archive digest is
`3cfddb23bf2911e05e27b4ed3856b8e795df94643b2c35b59deb317cf995bca0`, matching
the frozen native Linux evidence. Run
[36977329493](https://github.com/DavidLMS/nan-harness/actions/runs/36977329493)
reconfirms all nine remaining inventory/startup outcomes on commit `3e0bafdf`.
None completes a conversation scenario.

The next Linux Zed trial uses the same fixed X11 helper as keyboard input for
its single primary Retry activation. It validates bounded native coordinates,
rechecks the exact foreground window and PID before and after moving, then uses
an ordinary click with modifiers cleared. The checker still proves control
uniqueness, owned bounds and native guards; no uncertain activation receives a
second attempt. This transport is described in
[xdotool's upstream manual](https://github.com/jordansissel/xdotool/blob/main/xdotool.pod).

`--verification semantic-only` requires a disposable GitHub-hosted deterministic
session and a supported adapter. It never falls back to OCR. Renderer input
requires a live owned launch root, fresh loopback-listener ancestry and one owned
page. It does not depend on OS foreground because input is dispatched to that
page. Global keyboard, pointer and clipboard operations retain native window
and foreground guards. Hermes binds Electron's userData to the same private
directory that nANH uses for its managed active-profile file.

New scenarios disclose `semantic-failure-policy`: Zed injects HTTP 400 to observe
explicit UI Retry independently of automatic 503 backoff; Hermes injects HTTP
503 with its disclosed automatic-recovery policy. Both still require independent
provider failure evidence, a fresh recovery nonce, rendered recovery output and
cleanup. This policy change does not retroactively qualify earlier results.

Zed uses native
clipboard input readback and its native thread export; Hermes uses an owned
renderer DOM connection, keyboard activation of Send, and one ordinary pointer
activation of Retry. Retry samples nine interior points on the actual button and
requires a stable owned hit after rechecking the failed turn. It never forces a
click through an overlay or falls back to another activation after uncertainty.
Coordinates remain private. If the fresh profile shows the fixed provider
onboarding cover, Hermes selects its normal “I'll choose a provider later”
button once, verifies the cover disappeared, and revalidates the same failed
turn before Retry. It never calls the onboarding store or changes provider
credentials. The closed report records this UI preparation. Hermes binds
error, Retry and assistant response to the expected user's renderer turn pair. A passing probe must
verify the assistant response, a real file-tool round trip, an observed provider
failure, and recovery through exactly one UI Retry in the failed turn. Each
response also needs independent completed-provider evidence. Zed treats the
exact AX completion timeout as an ambiguous receipt and observes recovery
without repeating the press; a fresh resumed export and provider response must
still pass. Three passing probes and successful app/global cleanup are required per accepted cell.

Hermes's UI Retry trial sets the supported `agent.auto_recovery_cycles: 0` in
its fresh owned profile before the first turn. Ordinary API retries retain the
frozen default of three attempts. This exposes a terminal controlled failure
within the bounded trial, instead of waiting through five automatic recovery
cycles. The closed result records this explicit UI Retry policy and hashes of
the private configuration before and after the change. It does not qualify the
default automatic recovery schedule.

Qualification artifacts contain closed results and exact binary, application,
manifest and report hashes. Private prompts, clipboard exports, raw native logs
and connection details are excluded. These branch-only deterministic results do
not publish release recommendations or certify live provider behavior.

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

Pen Linux qualification requests the fixed `--use-gl=angle` and
`--use-angle=swiftshader` software GLES driver documented by
[Chromium](https://github.com/chromium/chromium/blob/main/docs/gpu/swiftshader.md).
This is confined to owned hosted qualification; it does not disable the sandbox.
Its native outcome remains pending. Zed's X11 pointer helper now emits only a
closed transport stage to distinguish foreground/process rejection, failed
movement and failed activation. Neither diagnostic can qualify a probe.

Run [36981736432](https://github.com/DavidLMS/nan-harness/actions/runs/36981736432)
confirms that software GLES removes Pen's Linux GPU startup failure. Three
owned documents now expose sign-in and no composer. This is startup progress,
not conversation qualification. Run
[36981732926](https://github.com/DavidLMS/nan-harness/actions/runs/36981732926)
identifies Zed Linux's pointer failure as foreground identity mismatch. The
helper now proves the active client's bounded X11 ancestor chain against the
owned top-level frame, and checks the active client's PID before and after
movement; acceptance still requires provider recovery and native Resume evidence.

Run [36981294763](https://github.com/DavidLMS/nan-harness/actions/runs/36981294763)
confirms Hermes Windows private retry-policy replacement now succeeds. One probe
also reaches response and file-tool verification, but cold attachment, response
and failure-observation timeouts and cleanup still prevent acceptance. Windows
renderer ownership now has a qualification-only Win32 transport using fresh
Toolhelp process metadata, creation times, session IDs and the unique IPv4
loopback listener, including rejection of concurrent IPv6 listeners. It rechecks
the listener after ancestry proof and retains no PID map across actions. Native
metadata contracts run before GUI launch. This aims to remove repeated CIM
startup from the bounded action deadline; native qualification remains pending.

The same hosted Win32 proof is used during native Windows window acquisition.
Repeated PowerShell/CIM ancestry startup otherwise consumes the 45-second
stability deadline before two post-fit snapshots can agree. Native observation
helpers also use `CREATE_NO_WINDOW`, and the renderer proof uses `windowsHide`,
so metadata queries do not create foreground consoles. These changes preserve
window identity, geometry, foreground, ancestry and cleanup acceptance checks.

Run [36982949036](https://github.com/DavidLMS/nan-harness/actions/runs/36982949036)
passes the X11 client/frame ownership check. Two transports fail while waiting
for movement and one dispatch still fails recovery. The helper replaces
`mousemove --sync`, which waits for movement even when already at the target,
with one movement and an exact bounded pointer-position readback. It never
replays uncertain clicks; recovery remains unqualified until fresh full evidence.

Fresh-account credentials are unavailable; qualification must seek supported
account-free routes. Pen's [official authentication documentation](https://docs.pencil.dev/getting-started/authentication)
describes pen.dev sign-in separately from its optional custom-provider key;
software rendering does not establish an account-free editor. The frozen UI's
“Skip for now” belongs to profile completion, rather than bypassing sign-in.
Claude's existing `deploymentMode: 3p` gateway profile remains the account-free
route under test. Startup baselines now retain only native accessibility presence
and visible editable/Retry/login counts, never labels, field values or trees,
to assess this route without forbidden CDP switches. These remain diagnostics.

The frozen Zed 1.22.0 source exports window-local AccessKit bounds but has no
callers for its X11 root-window origin update. The Linux Retry transport therefore
compares fresh AT-SPI screen and window extents with the owned X11 client geometry.
It accepts only the exact translated bounds or an exact missing-origin match,
rechecks the accessible object's process, button role and Retry name, and dispatches
one ordinary click. Arbitrary coordinate offsets remain rejected. Native recovery
qualification for this correction is pending.

Claude startup-baseline run [36984546491](https://github.com/DavidLMS/nan-harness/actions/runs/36984546491)
acquires the uninstrumented Linux window in three probes, but all native control
queries fail. macOS still has no eligible visible window. Linux baseline launches
now request Chromium's native renderer accessibility explicitly; CDP remains off.
The vendor's frozen debugging-switch denylist does not forbid this accessibility
switch. This is a diagnostic trial, not conversation qualification.

Codex's frozen official bootstrap supports `CODEX_ELECTRON_USER_DATA_PATH`.
Each checker probe now binds it to a fresh private directory, in addition to its
private `CODEX_HOME`, so Electron singleton and onboarding state cannot escape
the probe profile. Read-only renderer inventories retain bounded counts for
known import and project-creation controls, without retaining labels or taking
navigation actions. These observations cannot qualify a conversation backend.

Run [36987032897](https://github.com/DavidLMS/nan-harness/actions/runs/36987032897)
still fails Zed Windows acquisition after two eligible snapshots; cleanup passes.
The full qualification environment had discarded both fixed Win32 ownership
helper paths, leaving the legacy PowerShell lookup active. The environment now
requires and preserves those absolute regular helper paths on Windows, while
continuing to remove credentials and unrelated opt-ins. Native evidence for the
corrected environment is pending. Two observations before expiry do not prove
window instability: fitting consumes the first and the second establishes a
post-fit baseline.

Run [36987296715](https://github.com/DavidLMS/nan-harness/actions/runs/36987296715)
confirms the explicit native accessibility switch makes Claude Linux control
queries readable in three sessions; each returns zero visible editable, Retry
and login controls. Run [36987564606](https://github.com/DavidLMS/nan-harness/actions/runs/36987564606)
confirms private Codex Electron state cleans up on all three platforms. Its Linux
landing document has no editor, New chat or known import/project-creation controls;
macOS loses endpoint ownership after attachment and Windows attachment still fails.
These remain unqualified startup observations.

The user requires account-free qualification. Claude's
[official single-machine setup](https://claude.com/docs/third-party/claude-desktop/installation)
explicitly supports configuring third-party inference without an Anthropic account;
a startup failure must not be classified as an account requirement. Pen's frozen
Linux distribution compiles activation as mandatory, and its optional profile
completion skip does not bypass sign-in. No supported account-free switch was
found. The three Pen cells remain externally constrained under this requirement;
the checker does not alter the vendor distribution or fabricate authenticated state.

A hosted-only Codex supervisor now honors the frozen app's own
`CODEX_ELECTRON_DEV_RELAUNCH_MARKER_PATH` request. It restarts the same owned
executable at most once after a clean exit and a fresh strictly validated private
marker, preserving the bridge, profiles, session token, stderr privacy and original
startup deadline. It changes only the requested browser networking switch; it
never adopts an unrelated successor or bypasses Node permissions or authentication.
Closed `codex-owned-relaunch` stages measure whether this mechanism is requested.
Run [36990309336](https://github.com/DavidLMS/nan-harness/actions/runs/36990309336)
records only `armed` on all three platforms, with cleanup passing. This does not
establish a relaunch: the CLI can also stop an unauthenticated startup at its
noninteractive deadline. Closed terminal receipts now distinguish `child-exited`,
`startup-timeout`, `bridge-stopped` and `cancelled`. Hosted Codex probes explicitly
request the supported `--startup-timeout 120`, exceeding the observer's bounded
cold-start budget without resetting the watch or changing normal CLI defaults.
The separate app-driven Node-permission relaunch remains outside this mechanism.
The renderer also classifies the frozen app's exact public CLI connection error
as `cli-connection-failed`; unknown screen text remains unmeasured and is never
retained. This diagnostic does not establish an account requirement or recovery.

Run [36990110901](https://github.com/DavidLMS/nan-harness/actions/runs/36990110901)
confirms the canonical Win32 proof paths restore stable Zed Windows window
acquisition in three probes. Clipboard writes fail before submission; application
and global cleanup pass, but clipboard cleanup fails. The next transport uses an
absolute validated PowerShell executable and hides its console. Failure-only
`zed-clipboard-transport` diagnostics retain operation, closed failure stage and
elapsed bucket, never clipboard content or subprocess errors. The three-second
deadline and failure verdict were unchanged in that trial. Run
[36992541877](https://github.com/DavidLMS/nan-harness/actions/runs/36992541877)
then records `wait-timeout` at or above three seconds for all three writes and
clears. The next Windows-only trial allows fifteen seconds for one process;
macOS and Linux retain three seconds. No operation is replayed.
Run [36994849458](https://github.com/DavidLMS/nan-harness/actions/runs/36994849458)
still exhausts fifteen seconds for every write and clear. The next Windows
transport replaces PowerShell with the bundled native helper's direct User32
clipboard protocol. It retains bounded private UTF-8 pipes, a hidden owner window,
one attempt, readback and cleanup requirements. The hosted Unicode/invalid-input/
clear contract must pass before the Zed application is installed or launched.

Run [36990312437](https://github.com/DavidLMS/nan-harness/actions/runs/36990312437)
confirms Hermes Windows cleanup passes and one probe completes all five steps.
The other probes fail before attachment or after submitting the first prompt with
zero provider generations. Owned renderer readiness alone does not prove backend
readiness; input must not be replayed after submission.

Run [36992549952](https://github.com/DavidLMS/nan-harness/actions/runs/36992549952)
with the explicit Codex startup deadline acquires owned loaded documents in all
three Windows probes. Each has twelve visible inputs and no composer; cleanup
passes. macOS remains intermittent, while Linux stops on failed cleanup. The
frozen official onboarding chunk provides exact role-radio, legend and optional
suggestions-checkbox selectors; closed `onboardingCounts` measure their presence
without retaining labels or choosing a role. A matching total input count alone
does not authorize a setup action or establish authentication requirements.
Run [36994861637](https://github.com/DavidLMS/nan-harness/actions/runs/36994861637)
positively observes eleven role radios and the exact public legend in one Windows
session, identifying the conversational work-preferences screen. Other sessions
do not yet establish the same screen. Linux run
[36994873060](https://github.com/DavidLMS/nan-harness/actions/runs/36994873060)
observes none of these controls and cleanup passes. No role or skip action has
been dispatched; account-free startup remains unqualified.

Claude Windows startup run
[36992611042](https://github.com/DavidLMS/nan-harness/actions/runs/36992611042)
now acquires a stable owned window, but observes zero editable, Retry or login
controls and fails cleanup. Further diagnosis must establish native configuration
path alignment and accessible web-content availability before a conversation test.

Run [36996850316](https://github.com/DavidLMS/nan-harness/actions/runs/36996850316)
qualifies Zed Windows on source `25fa3d733d52204dac9b33b007bc1166fbb42afe`:
all three probes complete all five steps, with one exported Resume and independent
provider recovery per probe. Application, global and clipboard cleanup pass.
The native Unicode/invalid-input/clear contract also passes before app installation.

The next Zed Linux trial enables a first-map Openbox maximization rule only in
that hosted workflow cell. It copies the runner's stock configuration, preserving
focus and input bindings, and appends an exact normal-window rule matching both
`dev.zed.Zed` instance and class. The private configuration is removed with the
owned window manager. No already-bound window is resized, no geometry guard is
relaxed and no input is replayed. This remains a trial until three complete
native probes and cleanup pass.

Zed Linux first-map trial
[36999327703](https://github.com/DavidLMS/nan-harness/actions/runs/36999327703)
on source `3bbcaebc20177a9039fe8d66f6ccb818fc7bfdca` still completes only the first
four steps in each probe. Each ordinary Retry click is dispatched, but no
independent provider recovery or exported Resume is observed. All cleanup passes.
Maximization alone has not resolved the failure; an AT-SPI acknowledgement does
not prove the application handler ran, because GPUI maps it to center-point input.

The next Codex Windows feasibility trial explicitly opts into the frozen app's
public conversational work-preferences screen. It verifies the exact native
Engineering radio and associated label within the unique source-defined scope,
clicks the label once, checks the selected state and then clicks the scope's
unique enabled Continue once. Each action requires fresh process/listener
ownership, the same sole renderer target and stable native hit testing. It stops
after the role scope disappears, retaining only closed setup receipts; unfamiliar
screens, intercepted controls and uncertain actions remain blocked. It does not
set hidden onboarding state, use an account or establish conversation acceptance.
The canonical qualification workflow does not enable this diagnostic.

Codex Windows run
[37001532086](https://github.com/DavidLMS/nan-harness/actions/runs/37001532086)
on source `5b08ae425dc81499168e17c027a1464ca012d9bb` positively observes the
conversational legend and eleven visible role radios, but its scoped setup proof
does not match. No setup action is attempted. Cleanup fails at restoration,
stopping the other two probes. The next receipt identifies the exact failed role
predicate; only the source-confirmed pending disabled control receives bounded
read-only waiting. Ambiguity remains blocked.

Closed restoration failure receipts now distinguish command creation, process
I/O, the existing thirty-second deadline and a nonzero exit. They preserve the
original scenario reason and the cleanup failure verdict; no application output,
process identity or configuration value is retained.

The final repository gate passes locally and on the clean Linux runner in
[run 37003910826](https://github.com/DavidLMS/nan-harness/actions/runs/37003910826)
for commit `0e56c41c`. These are repository checks, not additional native cell
qualification. Codex Windows diagnostics on that same commit in
[run 37003904340](https://github.com/DavidLMS/nan-harness/actions/runs/37003904340)
record an owned role-selection screen with one role legend and eleven radios,
but reject the all-fieldset uniqueness check before any click. All three
application and global cleanups pass; the earlier restoration failure does not
recur and its underlying cause remains unestablished.

The frozen official conversational-onboarding source contains a second,
legitimate fieldset for the optional personalized-suggestions checkbox. The
next diagnostic counts only visible fieldsets containing the exact role legend
and named radio group, preserving independent global legend, associated label,
owner and scoped Continue checks. It still performs at most one ordinary role
selection and one Continue click; passing this diagnostic would not qualify the
conversation backend.

The next Linux Zed Retry diagnostic observes the active client's maximization
flags, the accessible button's enabled/sensitive/showing/visible/defunct state,
its window-coordinate containment, and the pointer's relation to the owned
client immediately before the existing single click. Measurements use closed
booleans, nulls and enums only; no coordinates, window identities, accessible
paths or application text are retained. These observations do not prove input
delivery or recovery. The unchanged provider and native export oracles remain
the acceptance boundary. The [AT-SPI component contract](https://gnome.pages.gitlab.gnome.org/at-spi2-core/libatspi/method.Component.get_accessible_at_point.html)
distinguishes accessible containment from actual activation.

On commit `cd1be14d`, Linux Zed in
[run 37006717428](https://github.com/DavidLMS/nan-harness/actions/runs/37006717428)
passes response and file-tool verification in all three probes, but Retry still
produces no independent recovery. Maximization, sensitive/showing/visible
state, non-defunct state and accessible containment are observed; the pointer
is reported over the owned Openbox frame. The `enabled` observation from that
commit is invalid: the decoder used AT-SPI state bit 7 (editable) instead of
bit 8 (enabled). The correction has a distinct editable-only regression case.
The next measurement queries the frame's child to distinguish the application
client from decoration. No recovery verdict depends on these diagnostic flags.

Codex Windows in
[run 37006719662](https://github.com/DavidLMS/nan-harness/actions/runs/37006719662)
passes application/global cleanup in all three probes on `cd1be14d`, but
rejects the onboarding session before any role proof or click. An added closed
`sessionProofFailure` distinguishes exhausted or invalid deadlines from missing
guards and unsupported platform/host/policy. The existing total 25-second
observer deadline includes listener checks, attachment and document readiness;
no timeout has been extended based on the ambiguous result.

Hermes Windows has an experimental `current-catalog` readiness policy. It
verifies the fresh managed `nan` profile selection, then uses the source-defined
model menu and at most one ordinary Refresh models action to observe a matching
fresh `model.options` response on the same renderer socket. Cached rows alone
do not establish readiness. This receipt does not independently establish the
backend process identity and cannot qualify a cell: response, file-tool,
failure/Retry and cleanup oracles still apply. Its longer cold-start budget is
Windows-only and remains bounded by the unchanged total worker deadline. Linux
and macOS retain their qualified readiness behavior.

Claude's hosted-only configuration observation reads the four owned disposable
configuration documents after gateway application. It checks their closed
profile/provider/authentication/features contract and optionally compares the
macOS Foundation Application Support directory with the managed HOME-derived
path. It never reads the independently resolved native directory. Presence
and path alignment are separate from consumption: `configurationConsumed` and
`modelDiscoverySeen` remain unknown until independently observed. No vendor
authentication state, signed debugging token or application bootstrap is changed.
