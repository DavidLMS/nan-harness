# Desktop compatibility checker

`nanh-desktop-check` is an independent, opt-in test executable. It does not
invoke an upgrade of an existing `nanh` or application, submit reports by
default, or change the recommended nanh release. Native app self-updaters are
a separate risk that must be controlled before personal-machine rollout. See the
[distribution catalog and bootstrap commands](checker-platforms.md).

## Qualification status

Eleven of twelve active cells have historical full acceptance. Zed Linux now
passes all three instrumented diagnostic sessions; uninstrumented acceptance
and the final same-commit matrix remain unqualified. See the
[current semantic qualification table](#semantic-deterministic-qualification).

### Historical joint campaign

The following subsection records an earlier six-cell investigation, not current
acceptance status.

Pen is deferred. The active acceptance matrix contains twelve cells: Zed,
Codex, Claude and Hermes on Linux x64, macOS ARM64 and Windows x64. The six
historical passes below are evidence from different commits, not a final
same-commit qualification.

Use `desktop-automation-feasibility.yml` with `app=all`,
`platform=open-cells`, `experiment=deterministic-full`, and `native_only=true`
for the six unresolved cells: Zed Linux, all three Codex platforms, and Claude
Linux/Windows. Jobs continue independently after failures and retain closed
qualification JSON. Each cell runs three fresh sessions. After corrections,
`platform=all` provides the final twelve-cell campaign on one commit. Its
aggregation job publishes `deterministic-qualification-matrix`, excludes Pen
explicitly, and fails if any active cell is missing, duplicated, from another
commit, or unqualified. The six-cell diagnostic campaign does not claim this
final matrix result.

The current candidate uses scoped CDP DOM actions for Codex, including the
home composer. Native window focus is not an admission requirement for this
explicit channel; original process, profile, listener, document identity,
visibility, and DOM actionability remain required. Provider response, file
read, controlled failure, UI recovery, and cleanup remain acceptance criteria.
Zed Linux's candidate Retry action uses the enabled accessible control and
exact pointer hit; cursor artwork is advisory. Claude Linux's candidate performs one ordinary accessible Retry action after
matching the failed turn; its empty-input witness still rejects unexplained drift.
None of these candidate changes establishes a new native pass by itself.

The latest completed joint campaign, [37228562433](https://github.com/DavidLMS/nan-harness/actions/runs/37228562433),
ran commit `ff59502c0326f3b08c103766f4eafd16da30b65f`. All six jobs compiled and
completed, but none completed full acceptance. Application/global cleanup pass
except for Claude Windows, whose later probes are withheld after cleanup fails.

| Cell | Latest observed boundary | Next required evidence |
| --- | --- | --- |
| Zed Linux | Response/tool pass; no new provider generation after Retry; file log observation is missing in all three sessions | Read closed events from the existing private PTY, where Zed actually writes its logs, to distinguish callback, Resume and provider boundaries |
| Codex Linux | Two sessions pass first response but exec-command exits nonzero; one session fails before input | Classify the file-read failure and stabilize initial admission without weakening ownership |
| Codex macOS | First response and real tool pass in all three sessions; Retry completes and provider recovery response is verified, but DOM response verification times out | Establish the recovered assistant turn's public DOM association |
| Codex Windows | Owned endpoint/main document retained; one observer reaches folder-trust and two stop in source-dialog | Identify the exact incomplete onboarding operation within the original budget |
| Claude Linux | First response passes in all three sessions; two verify second input, then fail tree-identity/children; third reaches deadline | Validate reduced redundant history scans while keeping full checks around readback and before Send |
| Claude Windows | First and second input/readback/response copying pass; Read returns a plain unclassified error; cleanup rejects target-image-file-id | Classify the Read failure and validate cleanup of original owned descendant handles, including distinct helper executables |

The pending grouped candidate captures only closed Zed PTY event IDs, retains
Windows cleanup targets by proved ancestry and original handles, and reduces
Claude Linux history scans during clipboard readback and passive Send inspection.
The grouped candidate also associates Codex assistant blocks by their public
semantic search key inside the same conversation: pinned source retains this
key when one turn is split into separate virtual rows. Physical row identity
alone excluded the recovered response. Exact user text and response nonce
remain required. Incremental Windows onboarding receipts retain catalog and
folder-trust phases before a timeout can hide the incomplete operation.

Tool failures now carry closed advisory lexical hints (never raw error text).
A completed, bounded tool turn with verified UI/provider response continues
into the independent recovery scenario even when the file result fails. The
original tool failure is retained; recovery cannot qualify a missing file read.

These local changes are not native qualification. A classifier limit reached
before Retry must remain a limit, never become an apparently empty successful
observation. No additional campaign is needed merely to retrieve the existing
closed qualification artifacts.

The Codex Send and Mac optional Skip corrections now have native evidence for
those transitions. They do not establish recovery. Windows renderer checkpoints
also retain the exact incomplete phase when the observer is interrupted.

Zed's final failed-thread exports contain one User and no Resume or Agent
entries. The Retry control disappearing in two probes supports a UI transition,
not proof that the native retry callback completed. Pointer delivery and the
passive control count cannot independently qualify recovery.

The next candidate groups these changes before another hosted campaign:

- Codex's mock capacity error retains HTTP 503 and adds the exact nested code
  `server_is_overloaded`. The real bridge preserves that typed error; the
  generic fixture previously became a different upstream error. Other apps
  retain their existing failure envelopes. Recovery still requires a new
  provider result and the exact response in the UI.
- Codex's passive source-dialog sample is bracketed by fresh ownership and
  original-document checks, reducing ownership queries from eight to two.
  No input occurs in that sample, and ownership/document changes still reject.
- Codex's file-tool observation distinguishes the bounded exec envelope's
  launch failure, completed exit, running session and unknown/ambiguous shape.
  No output, session identifier or exit code is published. An exit-zero result
  alone cannot replace the required fixture marker.
- Claude Linux validates each unique D-Bus connection owner before and after
  each fresh tree traversal, rather than once per node. Well-known aliases
  remain checked per node and no ownership result survives a traversal. The
  neutral 101-node fixture reduces owner queries from 101 to two and rejects
  foreign or lost owners, following the [D-Bus unique-name contract](https://dbus.freedesktop.org/doc/dbus-specification.html#message-bus-names).
  Transport exceptions at the original cutoff are classified as deadline
  failures without retrying the operation.
- Claude Windows uses UI Automation ControlView for Chat traversal, preserving
  exact semantic controls, row ancestry, uniqueness and the existing bounds.
  Cleanup compares each target to the original pinned executable using full
  volume/file identity and retained metadata. This permits alternate names for
  that same file, not different files or foreign processes. The failed run
  establishes a path mismatch, not its cause; hosted verification is pending.
- Hosted Linux Zed enables the official agent debug filter and observes only
  the bounded log interval surrounding the single Retry. Named counters cover
  session lookup, native Resume, ordinary Send and turn lifecycle events from
  the pinned source. Private message suffixes are discarded; rotated, missing
  or incomplete logs cannot be mistaken for zero observed events. These
  untrusted diagnostics cannot authorize input or replace provider/UI evidence.

The hidden launch-wrapper option is restricted to deterministic ChatGPT startup
diagnostics. Only application launch is instrumented; help, version and restore
still use the tested nanh directly. Wrapped runs save a private diagnostic
envelope containing the wrapper digest and observed result, not a public
compatibility report. `validate-report`, `submit` and `feed-updates` reject that
envelope. Do not extract its observation and publish it as an ordinary run.
Closed startup facts remain separate from the envelope; neither contains raw
application output.

The runner, report validation and lifecycle contracts have local automated
tests. Six cells have passed three complete deterministic native probes:
Zed 1.22.0 on macOS ARM64 in [run 36976970895](https://github.com/DavidLMS/nan-harness/actions/runs/36976970895),
Zed 1.22.0 on Windows x64 in [run 36996850316](https://github.com/DavidLMS/nan-harness/actions/runs/36996850316),
and Hermes 0.17.6 on Linux x64 and macOS ARM64 in
[run 36978761448](https://github.com/DavidLMS/nan-harness/actions/runs/36978761448),
plus Hermes 0.17.6 on Windows x64 in
[run 37031874268](https://github.com/DavidLMS/nan-harness/actions/runs/37031874268),
and Claude 2.19675.0 on macOS ARM64 in
[run 37188000425](https://github.com/DavidLMS/nan-harness/actions/runs/37188000425)
(commit `acb62475ff0f630700a20511fff98270d9e97129`).
All verify response, real file-tool use, controlled provider failure, UI Retry
recovery and application, global and clipboard cleanup without OCR. Hermes
observes Chromium 144.0.7559.236; Electron's separate version and Zed's runtime
version remain unobserved. Claude's bundled runtime version is also unobserved. Exact commits and artifact hashes are recorded in
each closed result. Six of the twelve active cells have historical qualification evidence; Pen is excluded from the current campaign.
Codex and Claude also distribute official Linux beta packages; their Linux cells
remain unqualified, rather than unsupported. Pen is deferred from the active
workflow at the user's direction. The active scope contains twelve cells for
Zed, Hermes, Codex and Claude, with six qualified and six open. The full
fifteen-cell inventory remains available, and scoped aggregation explicitly
records Pen as excluded rather than accepted. No test account is authorized.
Personal-machine isolation and live-provider behavior still require separate
evidence.

An earlier Linux Claude trial, [37208339146](https://github.com/DavidLMS/nan-harness/actions/runs/37208339146)
(`d158cdb2`, official `2.9939.4`), sends and independently copies the first
response in all three fresh sessions. The next input is rejected: its 17
characters match neither a retained prompt nor an actual placeholder attribute.
Only three of five owned nodes resolve through Hypertext links. Application and
global cleanup pass. [37210099000](https://github.com/DavidLMS/nan-harness/actions/runs/37210099000)
(`fb7eb4ea`) additionally observes the sole paragraph's exact source tag and
whole-document-empty class pair in all three sessions; two unmapped Text
leaves remain retained in the witness. `340bdcfe` introduces a separate
source-proved empty-model capability: all text/link/attachment records and
prior copied prompt/response history must remain unchanged through final paste
proof. It never clears text before paste and still requires exact prompt
readback before Send. In
[37211315141](https://github.com/DavidLMS/nan-harness/actions/runs/37211315141),
all three sessions reject the next helper transport after the first copied
response; both cleanup checks pass. `5fff78e4` distinguishes bounded transport
spawn, I/O, wait, status, size, decode and deadline failures without exposing
helper output. Its initial Linux trials stopped at a preinstall lint;
`1624e242` corrects that lint before fresh native trials.

The Windows source-sharing and configuration lifecycle fixtures all pass in
[37207929495](https://github.com/DavidLMS/nan-harness/actions/runs/37207929495)
(`5796b2b5`), before any vendor installation. The complete Claude Windows trial
[37208094730](https://github.com/DavidLMS/nan-harness/actions/runs/37208094730)
(`cdfe879c`) acquires and fits the first window, then rejects profile isolation.
Its cleanup also fails: the first descendant-holder preflight reaches its
cutoff; the second stops all five retained descendants but final accessibility
absence is not established. [37209405410](https://github.com/DavidLMS/nan-harness/actions/runs/37209405410)
(`f1b20acc`) passes both cleanup checks and locates the isolation rejection at
the first configuration file's privacy check. The repairing reader changed its
DACL before rejecting; this does not establish the original descriptor shape.
`6b346820` adds a nonmutating protected-or-exact-inherited classifier under
the retained original private root. Its synthetic Windows fixtures pass in
[37213103824](https://github.com/DavidLMS/nan-harness/actions/runs/37213103824),
but application installation fails before native acquisition; this supplies
no new application acceptance. General credential policy stays
protected. `fb7eb4ea` also prioritizes original descendant retention within
the same existing ten-second stop budget and reports closed preflight progress.
Directory leases permit child renames and deny directory deletion; configuration
file leases still deny writes. No rejected cleanup counts as acceptance.

[Codex macOS 37207737165](https://github.com/DavidLMS/nan-harness/actions/runs/37207737165)
(`cf0abf98`) completes ordinary onboarding in one session whose main window
was already focused. It reaches the initial home composer, whose managed
profile/context proof is not yet integrated on macOS. Another session rejects
source-point capture; the third rejects an auxiliary-page birth. Both cleanup
checks pass. The bounded owned-window movement trial has not established a
successful move; it grants no input authority. `ddbe5dea` integrates original
eight-root macOS profile custody and a fixed, descriptor-relative read-only
state transport. Four Rust custody cases, nine synthetic filesystem cases
and the real helper transport on synthetic files pass locally. Separate macOS
renderer source pins and context correlation remain necessary; profile custody
does not authorize Send.

Linux Codex [37208341472](https://github.com/DavidLMS/nan-harness/actions/runs/37208341472)
(`d158cdb2`) reaches the home composer and attempts ordinary selection of the
sole source-verified local project in two sessions. The retained guard rejects
the transition before reopening the menu. Application and global cleanup pass;
selection/context correlation and Send remain unproved. [37210102136](https://github.com/DavidLMS/nan-harness/actions/runs/37210102136)
(`fb7eb4ea`) locates the rejection immediately after the item click while
checking original popup closure. `c0ceddd3` passively awaits that one closure
within the original cutoff, rechecking the original CDP document and retained
editor before and after every sample; full source proof is mandatory before
reopening. In
[37211386151](https://github.com/DavidLMS/nan-harness/actions/runs/37211386151),
all three sessions still reject at original popup closure and pass cleanup.
`02911969` admits only the frozen source-declared projectless-to-selected
trigger replacement after one consumed item action. Original document, home
and editor must survive; one new-trigger reopen and the actual selected menu
check remain mandatory. Synthetic cases pass; native acceptance is pending.
Windows Codex's original-root custody and separate runtime source pins remain
under development; Linux proof cannot be reused as Windows acceptance.

[Zed Linux 37208343606](https://github.com/DavidLMS/nan-harness/actions/runs/37208343606)
(`d158cdb2`) verifies response and real file-tool use but still blocks Retry.
In all three sessions the passive XI2 observer matches all nine retained
hover points, with neutral buttons and matching root translation. Complete
owned transient-dialog censuses find zero dialogs before and after hover.
The fixed public Adwaita cursor theme still produces Arrow rather than Hand.
These observations prove delivery to the observer, not GPUI consumption or a
rendered hit; they do not authorize a click. Both cleanup checks pass.
Six cells remain open; the final same-commit production matrix and final
repository gate are pending.

Native Zed 1.18.1 qualification on 2026-09-08 passed three complete deterministic
scenarios on both macOS architectures in run 34265009005, but subsequent input
verification failures exposed instability. Shorter synthetic prompts restored
three ARM64 passes in [run 34272339383](https://github.com/DavidLMS/nan-harness/actions/runs/34272339383).
Live verification remains untested. Linux now starts in Zed's stateless mode,
which avoids its single-instance socket exceeding the private journal path's
Unix socket limit. Reply, file-tool and cleanup checks now pass, while explicit
Retry recovery still prevents full qualification.

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

The manual `desktop-check-qualification.yml` workflow calls the reusable
`desktop-automation-feasibility.yml` workflow from the same commit. It selects
all twelve active cells: Zed, Codex, Claude and Hermes on Linux x64, macOS ARM64
and Windows x64. Pen is excluded. This keeps installation, native preparation,
evidence reduction and cleanup on the same implementation used by diagnostic
campaigns. The final invocation runs the repository gate and aggregates all
twelve reports against its source SHA. Any missing or unqualified cell prevents
aggregate acceptance.

Until the qualification entry point is registered on the default branch, use
`Desktop semantic automation feasibility` on the integration branch with
`app=all`, `platform=all`, `experiment=deterministic-full`, `native_only=false`
and `quality_only=false`. For targeted iteration use `platform=open-cells` and
`native_only=true`; this selects Zed Linux and Claude Windows and enables the
Zed diagnostic tracer. Instrumented results cannot qualify the final matrix.
For the remaining Zed-only investigation, also set `app=zed-desktop`; this
avoids repeating the qualified Claude cell. Use `quality_only=true` when only
the repository gate is needed.

As of 2026-10-05, eleven cells have completed three full sessions on historical
commits. These results do not establish a complete matrix on the current SHA.
Each full session requires a verified response, real tool execution, explicit
UI error recovery and successful cleanup. Startup inventories, action
acknowledgements and partial sessions cannot satisfy that gate.

| Application | Linux x64 | macOS ARM64 | Windows x64 |
| --- | --- | --- | --- |
| Zed | Three full diagnostic sessions pass; uninstrumented acceptance pending | Historical full pass | Historical full pass |
| Codex | Historical full pass | Historical full pass | Historical full pass |
| Claude | Historical full pass | Historical full pass | Historical full pass |
| Hermes | Historical full pass | Historical full pass | Historical full pass |
| Pen | Deferred | Deferred | Deferred |

The historical passes are recorded in campaigns
[37242619508](https://github.com/DavidLMS/nan-harness/actions/runs/37242619508),
[37245073630](https://github.com/DavidLMS/nan-harness/actions/runs/37245073630),
[37248461596](https://github.com/DavidLMS/nan-harness/actions/runs/37248461596),
[37251043590](https://github.com/DavidLMS/nan-harness/actions/runs/37251043590) and
[37262040806](https://github.com/DavidLMS/nan-harness/actions/runs/37262040806).
Claude Windows completed all three full sessions in
[37275786700](https://github.com/DavidLMS/nan-harness/actions/runs/37275786700)
on `3fa4575802c451e04e7967413fc9dd5cd3d22356`, with application/global cleanup
and foreground restoration passing. The native readiness observer waits for
the exact failed user row without accepting an ambiguous control.

Zed Linux completed three full instrumented sessions in
[37288941016](https://github.com/DavidLMS/nan-harness/actions/runs/37288941016)
on `d28de4f5dcc19b4fc01f6ec3fc5e5a2a6b7fb979`. The qualification environment
had dropped the hosted session's explicit `ZED_ALLOW_EMULATED_GPU=1` opt-in.
Zed consequently rendered its full-window Unsupported GPU prompt over Retry.
Preserving that exact opt-in for hosted Linux Zed removes the prompt: the
closed diagnostic reports zero prompt renders, no blocking hitboxes, and one
Retry handler entry per session. Response, real tool execution, explicit Retry
recovery and both cleanup scopes pass in all three sessions. Instrumentation
intentionally keeps this report unqualified; the uninstrumented twelve-cell
campaign and final repository gate are still required.

Earlier Claude Windows campaign
[37269411810](https://github.com/DavidLMS/nan-harness/actions/runs/37269411810)
produced a report but its 70 observations exceeded the reducer's former limit.
The corrected Claude limit is 96 individually validated records; this capacity
correction does not itself establish application success.

### Historical investigation notes

The following observations describe earlier campaigns and hypotheses. The
status table above distinguishes historical acceptance from unresolved cells;
only a complete same-commit aggregate establishes final matrix acceptance.

Codex Linux completed onboarding in all three sessions in
[37190228367](https://github.com/DavidLMS/nan-harness/actions/runs/37190228367).
The later source-count run [37191551146](https://github.com/DavidLMS/nan-harness/actions/runs/37191551146)
observes Codex as the current mode in two sessions; another rejects an ambiguous
initial target. [37194306290](https://github.com/DavidLMS/nan-harness/actions/runs/37194306290)
then identifies exactly one local home composer in all three sessions through
its source-defined editable ancestry. The Work-home page wrapper is absent;
that wrapper cannot be a required marker for every local home composer.
The result does not authorize input or qualify the initial-send adapter.
Current macOS evidence is
[37192174385](https://github.com/DavidLMS/nan-harness/actions/runs/37192174385),
and Windows [37191553330](https://github.com/DavidLMS/nan-harness/actions/runs/37191553330).
Current Claude evidence is Linux [37195180348](https://github.com/DavidLMS/nan-harness/actions/runs/37195180348),
macOS [37188000425](https://github.com/DavidLMS/nan-harness/actions/runs/37188000425),
and Windows [37194302818](https://github.com/DavidLMS/nan-harness/actions/runs/37194302818).
Both cleanup scopes pass and invalid closed diagnostic events are zero in these runs.
Only the macOS Claude result adds a qualified cell; a passing exact-commit
production matrix and the final repository gate remain required.

The neutral Linux XI2 fixture in [37190616585](https://github.com/DavidLMS/nan-harness/actions/runs/37190616585)
creates only its own synthetic window and installs no vendor application. Warp
and XTEST each deliver three owned motion events with the X/Y valuator mask
required by GPUI. This refutes the proposed missing-motion-mask explanation for
Zed Retry; it does not qualify Zed or change its pointer transport.

The later owned-window observation in [37194304527](https://github.com/DavidLMS/nan-harness/actions/runs/37194304527)
records motion headers during the existing Retry hover, with no normal Enter
header during that interval. The cursor remains an arrow. This proves server
delivery, not GPUI consumption; zero Enter does not establish whether the
application had already received an Enter before observation began.
Codex macOS [37193191000](https://github.com/DavidLMS/nan-harness/actions/runs/37193191000)
places the owned window completely inside the usable screen area while the
elevated overlap also intersects that area. Fitting the window into that area
is therefore unsupported as a remedy. A bounded, read-only process-category
diagnostic is under evaluation; input still requires a clear native stack.

A Windows-only configuration persistence trial uses one safe Rust rename
invocation with retained file identity and read-only private-DACL checks before
and after it. Its preinstallation filesystem fixtures must pass before any
application trial. The default persistence path remains unchanged. This tests
an alternative to tempfile's attribute-setting path. The later closed failure
record proves that this alternative reaches the rename invocation and still
receives a sharing violation; it does not identify the conflicting handle.
The same synthetic production-writer lifecycle fixture passes before official
installation and fails afterwards in [37194971798](https://github.com/DavidLMS/nan-harness/actions/runs/37194971798).
This establishes an environment-dependent difference without launching the
application in that fixture. The exact failure category must be established
before treating it as a reproduction of the actual launch's sharing violation.

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

The hosted Codex macOS onboarding trial activates one retained native main
window instead of calling renderer `bringToFront`. Admission requires a unique
visible standard main window of the exact installed executable, fresh process
ancestry and creation identity, unchanged native geometry, an unobstructed
window stack, and two matching accessibility main-window observations. The
activation and exact retained-window raise are consumed once; uncertainty is
terminal. Two fresh native and renderer document-focus proofs are required
before onboarding. Native and renderer association relies on these independent
uniqueness proofs; it does not claim a direct renderer-target/window-ID mapping.
The private native cutoff is clipped to the original startup deadline.

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
account-free routes. Pen's [official authentication documentation](https://docs.pen.dev/getting-started/authentication)
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

The four native trials on commit `a4cc7954` remain unqualified:

- Linux Zed [37008709193](https://github.com/DavidLMS/nan-harness/actions/runs/37008709193)
  passes the first four scenario steps in all three probes, with cleanup passing.
  Retry is enabled and contained, both maximization flags are set, and the pointer
  is over the owned application client. No independent recovery is observed.
  These measurements exclude the previously suspected decoration/state causes;
  they do not establish event delivery or GPUI handler execution.
- Windows Codex [37008710412](https://github.com/DavidLMS/nan-harness/actions/runs/37008710412)
  positively proves the conversational role scope and Engineering control, then
  rejects fresh action proof before any click. All cleanup passes. Its combined
  deadline/ownership reason is split in the next diagnostic, including a deadline
  check after synchronous ownership queries. The 25-second budget is unchanged.
- macOS Claude [37008711197](https://github.com/DavidLMS/nan-harness/actions/runs/37008711197)
  validates all configuration contracts in three probes, with cleanup passing.
  Foundation path alignment is false twice and unmeasured once. Configuration
  consumption and model discovery remain unmeasured; no eligible editor is found.
- Windows Hermes [37008711106](https://github.com/DavidLMS/nan-harness/actions/runs/37008711106)
  exits before the observer attaches in all three probes, with cleanup passing.
  The checker incorrectly forwarded the ChatGPT-only `--startup-timeout` option
  to Hermes. Removing that unsupported argument restores Hermes' existing CLI
  interface; this run supplies no evidence about the catalog readiness trial.

The next macOS Claude startup trial pins official version 2.19675.0 and its
inspected archive digest. It launches the canonical bundle executable directly
with Electron's native `--user-data-dir` argument targeting the owned private
configuration pair. Electron 44.4.3 supports this path override and the inspected
Claude bootstrap derives its third-party directory from that path. Claude does
not document this argument as a vendor deployment contract; native evidence is
still required. The trial rejects other versions, noncanonical bundles, unsigned
vendor overrides and other platforms/modes. Foundation path alignment continues
to describe the original native directory, not the effective Electron override.

The clean Linux repository gate passes for `a4cc7954` in
[37009642401](https://github.com/DavidLMS/nan-harness/actions/runs/37009642401).
The local gate fails at a synthetic five-second shell fixture. A controlled
comparison reproduces the direct temporary executable timeout while explicitly
invoking `/bin/sh` completes promptly. The fixture now uses that interpreter;
its original deadline and API-key removal assertion remain intact, and all twenty
focused desktop-suite tests pass. The next final tree still requires the full
local gate; repository checks do not qualify a native cell.

The complete local repository gate passes on `d7d619a3`. Its macOS Claude trial
[37011848470](https://github.com/DavidLMS/nan-harness/actions/runs/37011848470)
exits before an application process is observed in all three probes, with cleanup
passing. Source inspection identifies a preparation conflict: the new trial
requires private Claude directories, while ordinary external configuration
writes create missing parents with general directory defaults. The checker now
precreates both roots privately only for this hosted opt-in. The strict profile
guard and ordinary CLI behavior remain intact. This failed run does not test
whether Electron's argument is accepted by the native application.

Windows Codex on the same commit in
[37011840857](https://github.com/DavidLMS/nan-harness/actions/runs/37011840857)
explicitly reports an exhausted onboarding session deadline before any click.
The first observed document has no inputs; the second has no renderer page.
The second probe also fails restoration with a nonzero exit, stopping the third
probe. This is a separate cleanup blocker, not a successful onboarding result.

The next Codex Windows diagnostic reserves separate bounded stages: at most
35 seconds for renderer startup and at most 25 seconds for the public work-role
interaction, within a 60-second observation clock and a 75-second supervised
driver limit. Other apps, platforms and policies retain their existing limits.
Every action still requires fresh ownership and exact scoped control proof.
A failure-only restore receipt classifies the actual typed CLI error without
reading stderr; it cannot override the failed cleanup verdict.

Hermes Windows [37011833500](https://github.com/DavidLMS/nan-harness/actions/runs/37011833500)
on `d7d619a3` reaches an owned renderer in all three probes, but the current-catalog
trial fails before opening the model menu. Each receipt reports
`composer-unavailable`, no provider requests and passing cleanup. The next
receipt separates missing composer/editor, the source-defined fallback picker,
model-label variants and ownership/deadline failure. It retains the same action
guards and cold-start deadline.

The Linux Zed feasibility cell now optionally records already-delivered XI2
button headers for the exact owned X client through XRecord. Fresh XRes identity,
foreground, geometry and pointer preflight remain required. Native calls run in
a separately supervised worker within the original input helper deadline;
missing libraries/extensions and uncertain identity yield advisory unknowns.
It selects no input events, takes no grab and generates no additional click.
Only bounded press/release counts and an ordered-pair boolean leave memory.
Delivered events still do not prove GPUI consumption or qualify recovery.

The private-root correction in `962198f2` allows macOS Claude's native process to
start in all three probes in
[37013251894](https://github.com/DavidLMS/nan-harness/actions/runs/37013251894).
Each remains blocked because no eligible visible window appears during native
acquisition; all cleanup passes. The owned configuration contract remains valid,
but consumption and model discovery are still unmeasured. A running process
does not establish that the account-free editor is available.

The complete local repository gate passes on `abca075c`. Native trials on that
commit still leave four of fifteen cells qualified. Linux Zed in
[37014555336](https://github.com/DavidLMS/nan-harness/actions/runs/37014555336)
passes the first four scenario steps in all three probes, but manual Retry fails.
Its pointer preflight passes; XRecord supplies no delivery evidence because the
input subprocess environment omitted its opt-in flag. The next trial preserves
that flag across the credential-clearing boundary, tests the actual subprocess
boundary with a synthetic fixture, and installs the required XRes library.

Windows Hermes in
[37014563397](https://github.com/DavidLMS/nan-harness/actions/runs/37014563397)
has an editor in every probe, but its exact URL guard blocks before any input.
The pinned bootstrap loads the renderer without a hash, and the ordinary fresh
session route subsequently changes it to `#/`. The next trial permits only that
initial transition on the same owned file document, with no query change, then
requires stable composer observations and freezes the exact URL for all actions.
Provider requests remain absent and all cleanup passes in the failed run.

Windows Codex in
[37014570767](https://github.com/DavidLMS/nan-harness/actions/runs/37014570767)
selects Engineering through the public control and verifies its checked state in
two probes. Continue remains blocked in all three; all cleanup passes. The next
receipt separates native ownership failure from page replacement, page count,
URL changes and failed queries. A source-verified native `--open-project`
argument targets only the existing owned fixture directory in an exact-version,
exact-artifact hosted Windows trial. This is project-entry feasibility, not a
complete conversation or recovery adapter.

The macOS Claude readiness receipt needs a parser correction: its complete
closed protocol includes an optional inventory line that the readiness-only
parser previously rejected. The null readiness fields in the preceding trial
therefore do not establish a native readiness state. The next trial changes the
parser without guessing a different launch or graphics policy. Complete typed
inventory input is accepted; unknown fields and extra raw lines remain rejected.
Pen's three cells remain externally blocked by mandatory vendor activation;
the user has requested an official route without test accounts.

The synthetic Zed export round-trip now uses real `zstd` compression and
decompression, preserving its bounded transport and private-output assertions.
On this Mac, direct execution of a temporary Python script repeatedly times out
while explicitly invoking its interpreter completes promptly. The unrelated
synthetic Openbox launch fixtures encounter the same temporary-script boundary
on macOS; their authoritative Linux workflow check remains required.

Linux Zed on `edb2d933` in
[37018798133](https://github.com/DavidLMS/nan-harness/actions/runs/37018798133)
again passes response and file-tool checks in all three probes, then fails Retry;
all cleanup passes. The input observer is now enabled, but both emitted delivery
receipts report `unavailable`, not measured zero events. The next receipt names
only a closed preparation boundary, including insufficient remaining budget,
without changing the input action or treating an unknown as success.

The parser-corrected macOS Claude trial on the same commit in
[37018822593](https://github.com/DavidLMS/nan-harness/actions/runs/37018822593)
reports `finishedLaunching=true`, `hidden=false` and `active=false` in all three
probes. The exact matching native process exists, but its window inventory is
absent; all cleanup passes. Configuration consumption remains unmeasured. This
does not demonstrate an editor or a requirement for an Anthropic account.

The local gate on `edb2d933` stops at the synthetic X11 session fixture's direct
temporary shell execution. That fixture now uses explicit interpreters while
still exercising the real wrapper and real `env` credential removal. It verifies
window-manager readiness, preservation of the checker's failure status and
process cleanup. Both the session and Openbox fixtures pass with their original
deadlines; ordinary desktop launch code is unchanged. The final tree still
requires a complete repository gate.

The clean Linux gate for `edb2d933` passes in
[37018868617](https://github.com/DavidLMS/nan-harness/actions/runs/37018868617).
Windows Hermes in
[37018804947](https://github.com/DavidLMS/nan-harness/actions/runs/37018804947)
finds one complete composer, editor and expected model pill in every probe, but
fails a document query before clicking; all cleanup passes. The frame comparison
incorrectly treated CDP's fragment-free `Frame.url` as the complete URL. The next
trial reconstructs it using the protocol's separate `urlFragment`, still checking
the original document, loader and exact allowed route. Synthetic tests cover this
actual protocol shape and reject missing, foreign or invalid fragments.

Windows Codex in
[37018815332](https://github.com/DavidLMS/nan-harness/actions/runs/37018815332)
launches with the exact native project policy in all three probes. Its public role
form is present with one dialog and no editor; the old sampler rejects every
visible dialog before any click. The next sampler can admit only one enclosing
dialog containing that already-proved role form. Foreign dialogs, alerts, menus,
ambiguous forms and covered controls remain blocked. The native result does not
yet establish that the project or coding composer opened; all cleanup passes.

Codex runtime `0.159.0-alpha.12.1` requires canonical
`server_is_overloaded` classification for its coding capacity Retry. The inspected
[HTTP classifier](https://github.com/openai/codex/blob/180d8caaac22c656bfc6329f2f573ee1430cbe20/codex-rs/codex-api/src/api_bridge.rs#L99)
accepts HTTP 503 with that exact error code; its Responses stream also accepts
the canonical code. The bridge previously replaced both with generic failures.
The bridge now preserves only this explicitly typed overload, emits a fixed safe
message, and retains other failures and retry policies. HTTP/SSE and negative
contract tests pass. This enables a future recovery adapter; it does not qualify
Codex's inventory trial or prove a manual click before the UI's automatic timer.

The next Claude macOS diagnostic snapshots only regular-file presence for
`Local State` and `Default/Preferences` in the two exact owned profile roots
before and after launch. Fresh artifacts can establish private application
storage use, but never gateway configuration consumption or editor acceptance.
Unsafe metadata invalidates that advisory observation; file contents and paths
are not emitted. The inspected bootstrap deliberately changes its user-data
directory to the private `Claude-3p` sibling after parsing third-party mode.

On `c5707f50`, Linux Zed in
[37021920391](https://github.com/DavidLMS/nan-harness/actions/runs/37021920391)
passes launch, input, response and file-tool checks in all three probes, then
fails Retry; all cleanup passes. The observer reports `unavailable` at the
`library` boundary in every probe. Its XRes SONAME incorrectly used `libXres`
instead of `libXRes`; the next trial corrects that spelling and verifies the
three observer libraries and required symbols before creating a display.
This is not measured evidence of missing input events.

Windows Hermes in
[37021928975](https://github.com/DavidLMS/nan-harness/actions/runs/37021928975)
now passes the document guard and observes a complete composer in all three
probes. It stops at `menu-unavailable` before model refresh or provider requests;
all cleanup passes. The next investigation concerns the ordinary model menu.

Windows Codex in
[37021938863](https://github.com/DavidLMS/nan-harness/actions/runs/37021938863)
reports `control-not-actionable` before any role click. Restoration then reports
the typed `APP-RUNNING` process failure, so cleanup fails and the two remaining
probes are not run. This run cannot count as acceptance or safe restoration.

macOS Claude in
[37021947961](https://github.com/DavidLMS/nan-harness/actions/runs/37021947961)
again has a matching finished process without windows. None of the four measured
storage files exists before or after any probe; all cleanup passes. This leaves
private storage use and configuration consumption unproved. The next official
account-free hypothesis uses native Application Support folders on a disposable
hosted macOS runner, with explicit fresh-directory ownership and cleanup guards.

The clean Linux quality run
[37022007039](https://github.com/DavidLMS/nan-harness/actions/runs/37022007039)
fails the synthetic session fixture's process-cleanup assertion: its Bash `env`
wrapper leaves an intermediate shell. The wrapper now uses `exec env`, preserving
the real credential-removal command, original deadline and cleanup assertion.
The corresponding local session and Openbox tests pass. The local full gate on
`c5707f50` was interrupted before completion while synthetic checks were stalled;
it is not a passing gate. The final tree still requires the complete gate.

Linux Zed on `70efffba` in
[37024793430](https://github.com/DavidLMS/nan-harness/actions/runs/37024793430)
loads all observer libraries and reports one ordered press/release pair delivered
to the exact owned X11 client in every probe. All first four acceptance steps and
cleanup pass, but Retry still produces no verified recovery. Server delivery does
not prove GPUI handler activation; the remaining investigation is inside the
source-defined UI hit test and recovery action.

The next Windows Codex cleanup check supplements window enumeration with a
bounded, read-only process snapshot. It requires a strictly parsed exact image
match and the inspector's own row, never treats localized or malformed output as
absence, and never kills an enumerated app process. Only the existing owned job
controls application termination. Synthetic parsing and deadline tests pass;
native Windows verification remains pending.

The Claude macOS `native-known-folders` startup trial follows the official
[single-machine third-party setup](https://claude.com/docs/third-party/claude-desktop/installation#single-machine-setup)
without an Anthropic account. It is restricted to the inspected frozen release
and a disposable GitHub-hosted session. It verifies Foundation's native folder
alignment independently, requires both app folders and matching processes to be
absent, preserves native HOME, and supplies no signed environment token or
Electron profile override. Private nANH receipts remain in the probe workspace.
Explicit cleanup runs after every prepared outcome, requires process absence,
matching directory identities and restored configuration, and propagates any
failure. Partial creation can roll back only its own empty directories. Synthetic
ownership, policy and path tests and focused Clippy pass; native acceptance is
still pending. Existing storage-presence diagnostics never establish gateway
configuration consumption or a working conversation.

The first native-folder Claude trial on `479f34ac` in
[37026402040](https://github.com/DavidLMS/nan-harness/actions/runs/37026402040)
proves native path alignment and fresh `Claude-3p/Local State` creation. One probe
acquires a window, then returns `window-changed`; another blocks before launch
and the third is not run. The window receipt has 21 observations, two present,
19 absent and one stable pair, with no measured identity or bounds changes.
Application and global cleanup pass. Configuration consumption remains unknown;
this is progress in native acquisition, not conversation acceptance.

The full local gate on that commit fails the existing Claude policy fixture,
which still supplies a plain text placeholder where the new frozen bootstrap
check requires an ASAR container. The fixture now has the real padded ASAR
structure and a separately pinned synthetic digest. A new contract checks padding,
truncation, invalid header lengths and digest mismatch while retaining the exact
production pin. All 47 reducer and runner tests pass; the complete gate must be
rerun after the correction. The preceding Linux quality gate on `70efffba` passes
in [37024794192](https://github.com/DavidLMS/nan-harness/actions/runs/37024794192).

Windows Codex on `a7e1f524` in
[37025953559](https://github.com/DavidLMS/nan-harness/actions/runs/37025953559)
passes application and global cleanup in all three probes after the process
absence check. Two probes acquire the renderer and stop at the public role
control's actionability guard; another does not acquire a renderer. The next
closed field distinguishes foreign overlays, disabled pointer paths, hidden
controls and unstable or intercepted points without admitting new actions.

Hermes Windows on `aa84ca13` in
[37025045302](https://github.com/DavidLMS/nan-harness/actions/runs/37025045302)
reports the source-defined first-run onboarding overlay intercepting every model
pill point in all three probes. All cleanup passes. The next trial uses its
ordinary unique "I'll choose a provider later" control once, proves the overlay
absent and the original composer/document unchanged, then opens the model menu.
Ambiguous, disabled, replaced or uncertain dismissal blocks before refresh;
no account or provider setting is changed by that source-defined control.

The next Claude trial names a closed native-root preflight boundary and captures
existing process/readiness/window observations immediately after an acquired
window is lost, before cleanup. The preceding launcher's zero exit may follow
cleanup and does not establish spontaneous application termination. The third
probe was skipped by the checker's deliberate `WindowChanged` stop policy.

The next Linux Zed panel experiment correlates immutable Maximize/Minimize icon
candidates with two fresh owned accessible-button snapshots. Its exact source
assets and alpha masks are pinned; native calibration is not established.
The receipt contains bounded counts and an explicitly unavailable toggle state
when the platform adapter omits it. It never activates zoom or substitutes icon
correlation for acceptance. The existing single Retry action and provider oracle
remain unchanged. Source inspection rules out an unguarded keyboard shortcut:
this Retry is not configured as keyboard-focusable, and the editor/panel does not
export the accessibility identity needed to prove panel keyboard focus.

The complete local gate on `8c869ab0` passes, as does the clean Linux quality
run [37028862949](https://github.com/DavidLMS/nan-harness/actions/runs/37028862949).
Its native trials retain four qualified cells; the following observations do
not add conversation acceptance:

- Linux Zed in [37028839806](https://github.com/DavidLMS/nan-harness/actions/runs/37028839806)
  again passes its first four steps and cleanup in all three probes, but Retry
  fails. Each observer reports one ordered press/release pair. Panel diagnostics
  find two stable Maximize icons, no Minimize icon and no correlated accessible
  button; toggle state is unavailable and no zoom action is attempted.
- macOS Claude in [37028844846](https://github.com/DavidLMS/nan-harness/actions/runs/37028844846)
  first fails the bounded Foundation query. The next probe proves native folder
  alignment, creates `Claude-3p/Local State`, acquires a window and stops at
  `focus-changed`; the third is not run. Cleanup passes. The 20 window observations
  contain two present and 18 absent snapshots, without establishing their order.
  A compiled closed Foundation query will replace cold Swift compilation, and
  post-acquisition diagnostics will cover both focus and window changes.
- Windows Codex in [37028846253](https://github.com/DavidLMS/nan-harness/actions/runs/37028846253)
  acquires the renderer in all three probes. Its role form is blocked by a
  foreign overlay before any role action. No editor or login control is observed;
  cleanup passes. The overlay's identity and account-free dismissal remain
  unproved, so its presence does not authorize an unknown modal action.
- Windows Hermes in [37028845305](https://github.com/DavidLMS/nan-harness/actions/runs/37028845305)
  dismisses onboarding and verifies a freshly requested catalog in two probes.
  Both fail the model-row check; the third fails a startup document query.
  Cleanup passes. The frozen source places reasoning metadata inside the model
  label span, so the next check compares only the exact model name and waits
  read-only for React's row update within the existing deadline. It never repeats
  refresh or selects a model to manufacture readiness.

The next hosted Linux Zed layout trial uses a stronger source-backed boundary
than anonymous accessibility focus metadata. The frozen global NewThread handler
focuses AgentPanel's current conversation editor, and the first native Copy must
replace a fresh clipboard sentinel with the exact private unsent prompt. Only
then may one public Shift-Escape toggle zoom. A fresh sentinel and a second Copy
must prove that same unsent prompt unchanged before Send; there is no repaste,
refocus or action replay. Failed transport or readback blocks Send, and later
turns and recovery never toggle again. Icon measurements remain advisory. Native
acceptance still requires the original complete provider, tool, Retry and cleanup
oracle. Focused Rust, synthetic X11 transport and runner-policy tests pass; the
native result is pending.

The next Windows Codex diagnostic fingerprints the foreign overlay without
acting on it. It holds the original document, role scope and actual dialog
objects across two fresh owned observations and compares the frozen final
onboarding heading, form, single Continue and legal-link layout privately.
Only a closed classification leaves the browser. Lookalikes, changed objects,
ambiguous overlays and ownership loss remain blocked. Public source inspection
shows that this final dialog's handler can update onboarding state and attempt
an announcement metadata request, whose failure is ignored; it does not create
an account or sign in. That source finding does not establish the measured
overlay's identity or authorize an unknown action. Synthetic callback and
behavioral tests and all 49 reducer contracts pass; native classification is
pending.

Hermes Windows on `82e6c52c` qualifies in
[37031874268](https://github.com/DavidLMS/nan-harness/actions/runs/37031874268):
all three probes pass all five acceptance steps and application/global cleanup.
Each uses the ordinary onboarding dismissal, one explicit catalog refresh, exact
model-name readback excluding nested reasoning metadata, and a verified unchanged
composer. Chromium is observed as `144.0.7559.236`; Electron remains unobserved.
This raises the matrix to five qualified cells, covering Hermes on all three
platforms. No further Hermes rerun is required by an unrelated change.

Claude macOS on `7349d8cd` in
[37032550333](https://github.com/DavidLMS/nan-harness/actions/runs/37032550333)
passes the compiled Foundation preflight, creates fresh third-party storage and
acquires a window. It stops at `focus-changed`, skips the remaining probes and
passes cleanup. The new pre-cleanup observation sees a matching active, visible,
finished process and an eligible window. The next diagnostic retains the exact
rejected startup snapshot's existing closed guard category; it does not activate,
reacquire or loosen the window guard.

The complete local macOS gate on `2a3cdfb7` passes after an initial sandboxed
attempt is blocked by the synthetic Tart test's `ps` invocation. The clean Linux
quality run [37033466249](https://github.com/DavidLMS/nan-harness/actions/runs/37033466249)
and the Zed/Codex native trials
[37033428517](https://github.com/DavidLMS/nan-harness/actions/runs/37033428517) and
[37033433279](https://github.com/DavidLMS/nan-harness/actions/runs/37033433279)
fail before application launch: the new helper method references `Path`, whose
import was restricted to macOS. The import is now shared. Those two native trials
provide no layout or overlay evidence and must be repeated after this correction.

The complete local macOS gate on `d72649af` passes. Linux Zed in
[37035051703](https://github.com/DavidLMS/nan-harness/actions/runs/37035051703)
reaches all three native probes: the first four steps and cleanup pass, while
Retry still fails. Each panel observation now has one stable Maximize icon and
no Minimize icon or correlated button; this change does not certify zoom.
The recovery export has no Resume message and the provider oracle sees no
verified recovery. Ordinary transport acknowledgement still cannot substitute
for handler activation.

Windows Codex in
[37035047811](https://github.com/DavidLMS/nan-harness/actions/runs/37035047811)
still fails before launch: the newly shared `Path` import makes the Windows-only
fully qualified accessor trigger the existing unnecessary-qualification deny
lint. The accessor now uses the shared type directly; no lint is weakened.
macOS Claude in
[37035049762](https://github.com/DavidLMS/nan-harness/actions/runs/37035049762)
again passes preflight and cleanup but stops at focus. Its composer diagnostic
remains empty because the earlier capture was wired to the separate Hermes
startup experiment. The actual semantic inventory path now passes its existing
diagnostic vector to both startup guards, preserving their exact single-snapshot
verdict and closed category.

The clean Linux quality run on `d72649af`
[37035049178](https://github.com/DavidLMS/nan-harness/actions/runs/37035049178)
passes. Native trials on `72cf852e` compile on both platforms. macOS Claude
[37037626734](https://github.com/DavidLMS/nan-harness/actions/runs/37037626734)
now identifies the startup rejection as `same-process-window`: the process is
active, visible and finished, but a different owned window precedes the acquired
window. Cleanup passes; this is not evidence of an authentication requirement.
Windows Codex
[37037629093](https://github.com/DavidLMS/nan-harness/actions/runs/37037629093)
acquires the first window with no editable or login controls; the next two probes
reject a different foreground process during acquisition. All cleanup passes.
The startup baseline does not exercise the instrumented foreign-overlay classifier.

The next Zed measurement includes AT-SPI ToggleButton (role 62), which xa11y
maps to Switch. The pinned AccessKit adapter exports its ON state as Pressed,
not Checked. A read-only pre-retry sampler observes the held objects' state and
screen/window geometry with bounded closed counters; it neither normalizes
activation coordinates nor certifies zoom from the earlier icon-count change.
The next Claude trials add private Windows configuration/storage receipts and a
qualification-only bridge counter for successful authenticated model-catalog
requests. A positive count establishes that discovery route; zero remains
unknown and does not establish configuration consumption or mandatory login.

The complete local macOS gate on `0bc487ec` passes. Claude macOS
[37040050548](https://github.com/DavidLMS/nan-harness/actions/runs/37040050548)
records two successful authenticated local-catalog requests with a complete
observation, without an Anthropic account. The remaining startup rejection is
again `same-process-window`; cleanup passes. This establishes the discovery and
authentication route, not full coding qualification. Windows Claude
[37040060071](https://github.com/DavidLMS/nan-harness/actions/runs/37040060071)
stops in runner preflight before any checker probe. Closed preflight categories
now distinguish policy, release identity, executable identity, ASAR structure and
bootstrap digest; no pin is relaxed without evidence. Linux Zed
[37040042341](https://github.com/DavidLMS/nan-harness/actions/runs/37040042341)
fails compilation because its new diagnostic array mixes signed origins and
unsigned extents. Checked conversions now reject zero/overflow advisory geometry;
a synthetic test compiles this conversion on macOS too. No native geometry was
measured by that failed build.

Windows Codex's actual renderer experiment on `72cf852e`
[37039212469](https://github.com/DavidLMS/nan-harness/actions/runs/37039212469)
acquires all three owned renderer sessions. Each has the public role form and
one dialog, but the read-only foreign-overlay fingerprint rejects its guard.
No role or dialog action occurs; application/global cleanup passes. The overlay
remains unidentified.

The current official Linux Pen download matches the inspected `1.2.15` archive
by provider checksum and length, with SHA-256
`62be02efa74085ac97987d4025effce797bb596468279c2dd99a71022d192ba0`.
Its compiled renderer still sets `skipActivation=false`. The current
[official authentication guide](https://docs.pen.dev/getting-started/authentication)
separates application sign-in from the optional custom-provider API key; no
supported account-free desktop route has been established. This remains an
external blocker under the user's no-account constraint.

The next Codex classifier fixes an asynchronous handle-lifetime bug: returning
the diagnostic promise allowed the enclosing `finally` to dispose its control
before classification completed. Awaiting that promise retains the original
control throughout the guarded observation. Synthetic handles now enforce
disposal and reproduce the failure. A closed `foreignOverlayProof` enum records
the specific rejection; unidentified dialogs still receive no input.
The next macOS Claude diagnostic reduces the exact rejected guard snapshot to
bounded same-process window counts: eligibility, intersection and normal versus
other layer, plus foreground ownership booleans. It issues no additional native
query and never waives the guard. Focused Rust/Node tests, checker Clippy and all
52 reducer contracts pass; native results remain pending.

The clean Linux gate and all semantic experiment contracts on `e7520d98` pass
in [37041726135](https://github.com/DavidLMS/nan-harness/actions/runs/37041726135).
The corresponding local gate was interrupted by a machine restart and has no
retained completion evidence. Claude macOS
[37041715707](https://github.com/DavidLMS/nan-harness/actions/runs/37041715707)
again records two authenticated catalog requests and passes cleanup. Its exact
rejected snapshot has one same-process window ahead: below the eligibility
threshold, not intersecting the held window, and on a nonzero layer. This does
not prove which window holds keyboard focus or authorize ignoring it.
Codex Windows
[37041705326](https://github.com/DavidLMS/nan-harness/actions/runs/37041705326)
now completes the overlay fingerprint in all three sessions and classifies the
dialog as `other`. It is not the inspected final-onboarding dialog. No dialog
action occurs; all cleanup passes.

Linux Zed on `70f16bf7`
[37041142386](https://github.com/DavidLMS/nan-harness/actions/runs/37041142386)
passes the first four acceptance steps and cleanup in all three probes, but
Retry still fails. Each fresh sampler observes 33 stable owned controls whose
SCREEN and WINDOW extents agree despite a nonzero owned client origin. The
existing Retry transport already translates this missing offset; the result
does not justify applying it twice. Geometry, clipping and native hit testing
remain under investigation.
Windows Claude on the same commit
[37041156791](https://github.com/DavidLMS/nan-harness/actions/runs/37041156791)
identifies its preflight rejection as `claude-windows-executable-invalid`. The
exact pinned MSIX contains `app/claude.exe`, while the validator compared the
basename to `Claude.exe` case-sensitively. Windows name comparisons now respect
case-insensitive filesystem semantics; canonical identity, symlink rejection,
release and bootstrap pins remain unchanged. The synthetic lowercase archive
fixture passes and noncanonical/symlink paths remain rejected.

Windows Claude on `f6bbaa80`
[37061001823](https://github.com/DavidLMS/nan-harness/actions/runs/37061001823)
passes preflight and acquires a window. Its owned third-party configuration
matches the gateway, authentication, model discovery and chat policy. The
startup inventory contains no editable controls, and cleanup fails after the
first probe; the remaining probes correctly do not run. No native diagnostic
event survives in the reduced artifact. The next reduction preserves the
existing closed invalid-event counter to distinguish rejected diagnostics from
an absent event without publishing process logs.

The next Codex Windows observation separates enclosing modal surfaces from
separate dialogs and records which inspected final-onboarding fingerprint
check failed. Two fresh observations must agree under the existing guard;
unidentified surfaces still receive no input. Linux Zed now compares the
exact held Retry control's coordinate spaces and independently verifies the
client origin through X11. These observations do not alter input coordinates,
Retry selection or acceptance. Five cells remain qualified; ten remain open.

The macOS Claude focus trial reads public accessibility focus only for the
already acquired owned process. Focused window, main window and the focused
control's window must agree before and after the retained window inventory;
finite exact bounds must identify one owned normal-layer window. The exported
receipt contains only a status and a nullable match boolean. The existing
same-process-window guard remains enforced, including when focus matches the
held window. This trial does not activate or dismiss a window.

The complete local gate on `38494449` passes. Its hosted Claude macOS trial
[37062973171](https://github.com/DavidLMS/nan-harness/actions/runs/37062973171)
again records two authenticated catalog requests and passes cleanup, but detects changed window
bounds immediately after acquisition. The next scoped startup trial requires
two seconds and at least three consecutive unchanged observations before the
single initial binding, within the original 45-second deadline. Candidate
absence breaks continuity; no post-acquisition rebinding is introduced.
Windows Claude
[37062977467](https://github.com/DavidLMS/nan-harness/actions/runs/37062977467)
again fails cleanup and has no diagnostic bundle. A synthetic event identifies
a capture defect: a valid `launchExit: "unknown"` raised an unbound-variable
exception before bundle publication. The decoder now accepts that closed
variant and preserves cleanup facts. This does not establish which payload
caused the historical missing bundle.

Linux Zed
[37062969515](https://github.com/DavidLMS/nan-harness/actions/runs/37062969515)
passes the first four steps and cleanup in all three probes. The exact Retry
control has the missing-origin relation, and all three independent client
origin checks disagree with xdotool. Retry still produces no provider recovery.
The installed transport version and its reparented-client translation need
verification before correcting coordinates; adding the existing offset twice
is still unsupported.

Codex Windows
[37062965739](https://github.com/DavidLMS/nan-harness/actions/runs/37062965739)
passes cleanup in all three sessions. Two classify the blocker as a separate
dialog whose heading does not match the inspected final-onboarding fingerprint;
the third loses the role-form proof during classification. No unknown modal
receives input. Source fingerprints for other shipped startup dialogs remain
under investigation.

Claude macOS on `45447e23`
[37064335977](https://github.com/DavidLMS/nan-harness/actions/runs/37064335977)
observes seven consecutive present candidates with six stable pairs, then
rejects the initial readiness guard for another window of the same process.
It passes cleanup and again records two authenticated catalog requests. Focus diagnostics previously
ran only after binding, so this earlier rejection has no focus receipt. The
next diagnostic observes the already selected, process-owned candidate before
binding while preserving the original rejection and issuing no input.

The Ubuntu Noble source for `xdotool` and `libxdo3`
`1:3.20160805.1-5build1` retains the reparented-client translation bug: it
translates the parent's client offset instead of the client origin. The next
Linux Retry trial requires both installed package versions to match, two
unchanged independent X11 geometry snapshots, equal dimensions, and an origin
discrepancy exactly equal to the measured parent offset. Only then may the
existing single click use the directly translated origin. Unknown discrepancies
fail before the click; no pixel scale or extra activation is guessed.

Windows Claude on `45447e23`
[37064339922](https://github.com/DavidLMS/nan-harness/actions/runs/37064339922)
still fails cleanup and has no native diagnostic bundle after the unknown-exit
decoder fix. The executor publishes the bundle only after its owned-process
cleanup returns; an executor cleanup exception can therefore discard already
captured closed events. The next trial publishes those events on failure too,
while preserving the executor failure and blocking further probes. No process
output is added to the artifact.

The local full gate on `a3ec421f` passes. The clean Linux gate
[37065467921](https://github.com/DavidLMS/nan-harness/actions/runs/37065467921)
identifies unused macOS focus members in the non-macOS library build. Focus
types, storage, parser records and observation methods are now compiled only
for macOS or synthetic tests; no lint exception is introduced.

The same commit's Linux Zed trial
[37065452851](https://github.com/DavidLMS/nan-harness/actions/runs/37065452851)
verifies the exact installed Noble packages and parent-offset discrepancy in
all three probes. Each uses the corrected origin and delivers one complete
pointer pair, but Retry still fails. Response and tool steps and cleanup pass.
Two separate diagnostics still compare incompatible origins: the AT-SPI
sampler uses old xdotool geometry, and the icon correlation compares screenshot
root coordinates against unshifted accessible bounds. Zero correlation cannot
therefore certify a missing panel control or its zoom state.

Claude macOS
[37065460048](https://github.com/DavidLMS/nan-harness/actions/runs/37065460048)
now records the initial focus query as `query-error` while preserving its
same-process-window rejection; model discovery and cleanup pass. Windows
Claude
[37065464254](https://github.com/DavidLMS/nan-harness/actions/runs/37065464254)
finally retains its closed cleanup diagnostic: `restore` fails with
`nonzero-exit`, after an `action-unsupported` startup inventory. The next
Windows investigation must classify that restore failure, rather than alter
process ownership or treat cleanup as successful.


The hosted Linux quality gate on `9682cb8a`
[37068168269](https://github.com/DavidLMS/nan-harness/actions/runs/37068168269)
passes. The local gate reaches 277 passing checker tests and one synthetic
executable-readiness timeout; that exact failed test passes on a focused rerun.
The next final tree still requires its complete local gate.

Codex Windows on that revision
[37068171470](https://github.com/DavidLMS/nan-harness/actions/runs/37068171470)
preserves `page-count` as the role-proof rejection in all three cleanly restored
sessions. The final renderer inventory has one page, so it cannot identify the
transient rejected inventory. The next trial records bounded protocol counts
of that snapshot without retaining URLs or selecting among renderer targets.

The next Claude Windows trial records a closed restoration stage and typed
failure category from the CLI itself. It distinguishes session lock, process
inspection and receipt restoration without exposing error messages or paths;
the ordinary restoration result is unchanged. macOS focus diagnostics similarly
identify the precise public AX query stage and error while keeping the initial
same-process-window rejection. Authenticated model discovery counters count
catalog requests, not returned models or editor readiness.

Zed Linux diagnostic correlation now uses two independent client-origin reads
and translates SCREEN/WINDOW bounds exactly once before comparing screenshot
root coordinates. A bounded private handoff carries only indexed canonical
rectangles and raw toggle states; public receipts remain closed counts/enums.
This corrects the measurement rather than asserting a zoom state transition.
No extra shortcut or input is added. Qualification remains 5 of 15 cells.


On `a4f95f1a`, all four scoped native trials retain valid closed diagnostics;
none adds a qualified cell. macOS Claude
[37069925452](https://github.com/DavidLMS/nan-harness/actions/runs/37069925452)
reports `focused-element/no-value` before input-window lookup. Focused and main
window queries succeed, and cleanup passes. This supports a separate official
Chromium renderer-accessibility flag trial, not an AXWindow self-parent fallback.
The flag remains hosted-only, startup-baseline-only, and scoped to Claude under
the macOS native-known-folders policy; ownership/focus guards are unchanged.
See Chromium's [assistive technology detection documentation](https://www.chromium.org/developers/design-documents/accessibility/#how-chrome-detects-the-presence-of-assistive-technology).

Windows Claude
[37069922557](https://github.com/DavidLMS/nan-harness/actions/runs/37069922557)
fails `absence-after-stop/accessibility-enumeration/already-running`, before the
CLI restoration command can run. There is consequently no restoration receipt,
rather than a lost diagnostic. Job termination behaves like per-process
termination, which can return before exit completes; the next bounded Windows
trial must prove absence before restoration, without adding another kill or
accepting a late/failed query. See Microsoft's
[TerminateJobObject](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-terminatejobobject)
and [TerminateProcess](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-terminateprocess) contracts.

Linux Zed
[37069928393](https://github.com/DavidLMS/nan-harness/actions/runs/37069928393)
now uniquely correlates one stable Maximize glyph with canonical button bounds
in each probe. Its state remains unavailable, so the measurement does not prove
AgentPanel zoom. All first four steps and cleanup pass; Retry still produces no
verified recovery. The pinned fullscreen-control source exports ToggleButton62
with PRESSED20, so ordinary-button containment cannot certify its toggle state.

Windows Codex
[37069931417](https://github.com/DavidLMS/nan-harness/actions/runs/37069931417)
rejects snapshots with two app-protocol pages, one the held page, in all three
cleanly restored probes. The exact frozen MSIX's app.asar creates prewarmed
auxiliary pages. The next read-only diagnostic classifies fixed public routes
and document visibility without retaining URLs or selecting an auxiliary target.
Document visibility alone cannot establish a harmless window.

The Linux quality gate
[37069934214](https://github.com/DavidLMS/nan-harness/actions/runs/37069934214)
rejects the now-unused `Toggled` import on non-test Linux. That import is scoped
to its consumers. The obsolete local gate was stopped during canary fixtures
rather than continuing to certify a known platform failure; the corrected final
tree requires a new complete gate. Qualification remains 5 of 15 cells.


Both the complete local gate and hosted Linux quality gate on `7b119970`
[37072140449](https://github.com/DavidLMS/nan-harness/actions/runs/37072140449)
pass. Native qualification remains separate. The macOS accessibility-flag trial
[37072133814](https://github.com/DavidLMS/nan-harness/actions/runs/37072133814)
reports `focused-window/cannot-complete` under the existing 100-ms AX timeout;
cleanup and authenticated catalog requests pass, but editor readiness is unproved.
The next read-only observation independently checks focused/main window identity
without relying on a focused UI element and cannot authorize input or waive a guard.

Windows Claude
[37072131094](https://github.com/DavidLMS/nan-harness/actions/runs/37072131094)
fails after stop at process enumeration with `desktop-unavailable`. Windows Codex
[37072136816](https://github.com/DavidLMS/nan-harness/actions/runs/37072136816)
has one cleanly restored probe, then the same enumeration failure. The shared
whole-chain budget is therefore restricted to Claude, with a five-second bound;
other applications recover their original absence path. Failure-only typed
inspector stages distinguish deadline, environment, spawn, exit, read, schema
and size failures; the present error is not assumed to be a timeout.

Codex's first probe identifies one source-bound avatar-overlay page and one
unknown route, both with visible documents. Its second probe instead sees a
separate modal with no recognized public heading. No auxiliary target is chosen
and no unknown dialog receives input. Qualification remains 5 of 15 cells.


After the workstation restart, the unchanged native trials on `5bc847a6`
retain the same frozen inputs. macOS Claude
[37074772135](https://github.com/DavidLMS/nan-harness/actions/runs/37074772135)
now proves both full focus and the independent window-only identity against the
held window. Its remaining acquisition failure is the same-process stacking
rule: one non-normal-layer window precedes it without intersecting its bounds.
Cleanup passes and diagnostic validation rejects no events. A focused correction
must retain exact foreground identity, complete display containment and rejection
of every intersecting window, and continue rejecting another normal-layer window
from the same process. An accessibility write is unnecessary for this observation.
Neither catalog activity nor native focus alone qualifies the coding backend.

The Windows trials
[37074769314](https://github.com/DavidLMS/nan-harness/actions/runs/37074769314)
and [37074775121](https://github.com/DavidLMS/nan-harness/actions/runs/37074775121)
stop during compilation, before application launch: the failure-receipt writer
incorrectly assumes an I/O error converts into a serde JSON error. `9fa9311f`
handles serialization and flushing separately. The obsolete local gate was stopped
before changing its source tree; a new final gate is required after the pending
macOS and Linux diagnostic changes freeze. These runs establish no new native
qualification result. The matrix remains five qualified cells, eight open native
cells, and two platforms without an official native distribution.


The next frozen tree uses a separate hosted-Claude focus verdict for the measured
auxiliary-panel case. It requires both exact AX proofs, rejects same-process
normal-layer windows and every intersecting window, and preserves the original
guard and foreground-recovery behavior elsewhere. Initial acquisition checks the
original deadline and revalidates process ownership after its fresh proof.
Synthetic tests cover missing/stale proofs, overlap, identity duplication, changed
bounds, lost ownership and late results. This is an acquisition correction, not
acceptance of a missing composer or coding backend.

The existing Linux Zed pre-Retry observation now reports matched push-button and
toggle-button counts, nested containing controls and a closed matched-role value.
Only raw ToggleButton62 can establish toggle state; ordinary Button43 remains
unavailable. These are observations at the existing measurement point, not a
before/after ShiftEscape transition proof. No extra input or capture is introduced.


The complete local gate on `039d4585` passes. Windows Codex
[37075370575](https://github.com/DavidLMS/nan-harness/actions/runs/37075370575)
again restores all three probes; role/renderer ownership still blocks coding
acceptance. Windows Claude
[37075368043](https://github.com/DavidLMS/nan-harness/actions/runs/37075368043)
acquires its window but still fails the AX absence check after stop, before an
independent process inspection. A once-only hosted diagnostic now inspects exact
Claude executable presence at the first AX rejection within the same five-second
budget. Its closed observation cannot override the failed cleanup verdict.

macOS Claude
[37075778336](https://github.com/DavidLMS/nan-harness/actions/runs/37075778336)
passes initial focus acquisition, then the first startup guard rejects changed
window bounds. Cleanup passes. AX attachment follows the initial geometry binding
and can take ten seconds; its causing the resize is not established. Acquisition
now retains the original 45-second deadline through attachment and final stability
checks. Before any input, the same original CG identity/PID/name must pass both
focus proofs, ownership, display and occlusion checks for three observations over
two seconds. Only then is final initial geometry bound. Later guards remain strict.

Linux Zed
[37075781409](https://github.com/DavidLMS/nan-harness/actions/runs/37075781409)
reports one ordinary push button, no toggle and no nested matching controls in
all three pre-Retry observations. Pinned ThreadView source also uses Maximize in
an ordinary message-editor ExpandMessageEditor control; that glyph is not proof
of panel zoom. Reply/tool acceptance and all cleanup still pass; explicit Retry
recovery remains unverified. No new native cell is qualified.


Both the complete local gate and hosted Linux quality gate
[37077307516](https://github.com/DavidLMS/nan-harness/actions/runs/37077307516)
pass on `5042debe`. macOS Claude
[37076922575](https://github.com/DavidLMS/nan-harness/actions/runs/37076922575)
rejects final acquisition on focus validation. The initial exact AX proofs pass,
but the existing private new-file writer retains only that initial receipt; it
does not explain the final rejection. The next trial preserves one separate
`final-stability` receipt from the rejected snapshot, with unchanged guards.

Windows Claude
[37076925536](https://github.com/DavidLMS/nan-harness/actions/runs/37076925536)
passes AX absence and fails process inspection with the newly measured `deadline`
stage. There is no first-AX-rejection receipt in this run because AX absence
succeeds. Native absence previously re-created its bundled helper and opened
nested five/15-second deadlines. The corrected Claude post-stop path borrows the
already-owned helper, keeps one absolute five-second budget and performs a single
complete native inventory. If no Gui exists, helper preparation happens once
inside that same budget. AX/window/exact-process absence still all have to pass.
Ordinary native transport retry and timeout behavior remains unchanged.

Zed's existing pre-click XQueryPointer result now retains only a closed modifier
category and nullable button-held flag. Configurable Mod1..Mod5 bits are not
misidentified as Alt or Super; unknown bits remain unknown. This requires no extra
query or action and cannot certify GPUI callback consumption or Retry recovery.
The matrix still has five qualified cells; new diagnostics and acquisition fixes
remain subject to native runner acceptance.


Native trials on `36f5999c` supply two useful negative observations. macOS Claude
[37078536365](https://github.com/DavidLMS/nan-harness/actions/runs/37078536365)
proves initial focus but its final receipt records an incomplete AX `main-window`
query during confirmation. That is not proof of a changed focused window. Initial
acquisition now treats only `CannotComplete` as pending while the exact native
candidate, foreground, ownership, display and clear-stack checks remain safe.
Pending discards earlier stability, keeps the original deadline and authorizes
no input. Other proof failures and all later guards remain immediate failures.

Windows Claude
[37078533986](https://github.com/DavidLMS/nan-harness/actions/runs/37078533986)
still exhausts process inspection after retained-helper reuse. The next frozen
implementation enumerates the read-only Toolhelp process snapshot directly,
without invoking tasklist. It requires complete `ERROR_NO_MORE_FILES` termination,
the checker's own entry, exact executable names, and no late result. Only its
snapshot handle is opened/closed; application processes are never opened or killed.
The same two/five-second process/whole-chain deadlines remain in effect. Legacy
failure receipts remain decodable; new API failures use closed snapshot/first/next
stages. The actual Windows API body has been typechecked without executing it.

Linux Zed
[37078539019](https://github.com/DavidLMS/nan-harness/actions/runs/37078539019)
observes no held modifiers or mouse buttons in all three Retry dispatches. Server
delivery, first reply/tool acceptance and cleanup pass; Retry recovery does not.
This eliminates held-input-state as the cause in these probes. No cell is promoted.


Windows trials on `0b9ccbe9` fail before application launch:
[Claude 37079744074](https://github.com/DavidLMS/nan-harness/actions/runs/37079744074)
and [Codex 37079749513](https://github.com/DavidLMS/nan-harness/actions/runs/37079749513).
The checker denies unsafe Rust, so the direct Toolhelp calls cannot compile there.
The corrected implementation places this read-only enumeration in the existing
C++ native boundary, returns only closed presence/failure facts, and retains the
original absolute deadline and checker-entry completeness requirement. No lint
exception is introduced. Retained Claude helpers are reused for this query too.

macOS Claude
[37079746739](https://github.com/DavidLMS/nan-harness/actions/runs/37079746739)
passes cleanup, records two authenticated model-catalog requests, and proves both
initial and final focus. It still rejects final acquisition with `window-changed`.
That receipt does not distinguish candidate ambiguity, identity or display
failure. The next receipt adds a closed final candidate state without changing
selection or guards. Startup inventory remains unqualified.

Frozen Windows Claude 2.19675.0 bootstrap resolves third-party storage using
`LOCALAPPDATA/Claude-3p`, already matching the private qualification profile.
Real known-folder copying is not supported by this observation. Config-file
presence and catalog requests remain distinct from consumed configuration or a
usable composer. No account-free Pen composer route has been established.


The checker now captures Windows Claude storage metadata before its own launch
and records it after stopping the owned process tree, before restoration. This
avoids depending on a CLI finalizer that JobObject termination can interrupt.
The diagnostic requires the hosted startup trial, exact private child environment,
fixed roots and no symlink substitution; it reads only file presence, not payloads.
It cannot certify configuration consumption or qualify a startup-only backend.

macOS Claude
[37080447988](https://github.com/DavidLMS/nan-harness/actions/runs/37080447988)
now identifies the final acquisition failure as `off-display`; both independent
focus proofs pass and cleanup passes. The next distinct trial fits only the
already-owned, exactly focused initial window to the public visible work area,
then requires the original strict guards and stability before any input.


Windows Claude
[37080445695](https://github.com/DavidLMS/nan-harness/actions/runs/37080445695)
compiles the corrected native boundary and acquires its window. Cleanup still
fails: the independent complete process snapshot reports Claude present after
owned stop while AX absence is also rejected. This is positive presence evidence,
not another process-inspection timeout; it does not establish whether the process
belongs to the stopped job. Exact ownership and prelaunch presence are the next
closed diagnostics. No global process kill is authorized by a matching name.

The next Linux Zed trial compares guarded frames against the existing pre-submit
baseline and correlates new stable exact-source Copy/Close icon candidates with
the unchanged native Retry rectangle. It emits only closed counts and relations.
Masks remain uncalibrated against GPUI, so even a unique matching cluster is
advisory; this neither certifies the hitbox/callback nor changes the Retry click.


The complete hosted Linux quality gate
[37080540050](https://github.com/DavidLMS/nan-harness/actions/runs/37080540050)
passes on `bc9cccea`. Windows Codex
[37080450261](https://github.com/DavidLMS/nan-harness/actions/runs/37080450261)
passes all three cleanup repetitions with the corrected process helper. Its
public role onboarding remains blocked by an additional source-classified
avatar renderer; no coding acceptance is claimed.

Windows Claude
[37080901135](https://github.com/DavidLMS/nan-harness/actions/runs/37080901135)
still leaves positive process presence after stop. The new checker storage
receipt is missing. Its child-environment binding compared raw Windows paths
with extended canonical paths; it now compares directory identities instead.
This source correction needs a new hosted observation. Initial absence now also
checks exact Claude.exe presence, with a first-query baseline receipt; previous
Claude prelaunch guards checked only AX and native windows. No process is killed
because of a matching executable name.

Linux Zed
[37081092966](https://github.com/DavidLMS/nan-harness/actions/runs/37081092966)
reports one Close match and no Copy match in each repetition, while reply/tool
acceptance and cleanup still pass and Retry recovery still fails. The proposed
Copy/Close pair is not a valid locator for every terminal error: pinned source
renders ProviderRejection with Retry enabled and Copy disabled. Therefore missing
Copy cannot establish missing rendered Retry or a wrong click target. The extra
visual hook is removed; closed historical receipts remain decodable. No new
native cell is qualified.

The next macOS Claude trial fits the unique, independently focused owned initial
window through public AX size/position attributes, at most once across both
acquisition phases. The helper rechecks original CGID/PID, AX object identity,
clear stack and foreground before mutation and verifies full display containment
afterward. Pure geometry/precondition/deadline fixtures pass. Actual hosted fit
and the original post-fit stability guards remain required for acceptance.

The complete local and hosted Linux quality gates pass on `3c44cf18`; hosted
[37081875534](https://github.com/DavidLMS/nan-harness/actions/runs/37081875534)
completed successfully. macOS Claude
[37081870838](https://github.com/DavidLMS/nan-harness/actions/runs/37081870838)
acquires the window in two of three repetitions after the one-time fit. Those
two inventories report one generic editable and no login controls; this does
not identify a prompt composer. All three cleanup checks pass and all three
trials observe authenticated model discovery. The remaining initial acquisition
failure has no more specific fit-stage evidence.

Windows Claude
[37081873118](https://github.com/DavidLMS/nan-harness/actions/runs/37081873118)
proves complete process absence before launch and positive Claude process
presence after owned stop. Cleanup fails on the first repetition, preventing
later trials. This excludes a pre-existing process at the initial check but
does not prove job escape, broker activation or environment loss. The private
storage receipt is still missing. Component-wise joins replace slash-containing
joins after Windows extended-path canonicalization; this correction requires a
new hosted observation.

The next startup trials add two advisory diagnostics. On macOS, exact labels
from the frozen official 2.19675.0 renderer identify classic prompt and Send
controls separately from generic editables. Only bounded counts and reference
source hashes are exported; these do not independently verify loaded renderer
bytes or authorize input. On Windows, fixed known-folder metadata is observed
before launch and after owned stop, alongside isolated-profile metadata. A
failed query stays unavailable rather than being classified as fresh. No file
contents, paths, credentials or app logs are exported, and no native known
folder is created, copied or removed. These observations do not qualify a cell.

The official [Claude third-party deployment documentation](https://claude.com/docs/third-party/claude-desktop)
describes account-free local device identity; the official
[extension documentation](https://claude.com/docs/third-party/claude-desktop/extensions)
supports managed MCP servers, including local stdio tools. The current trial
has not yet established a guarded composer, a response-scoped assertion or an
installed fixture tool. Those proofs remain necessary before coding acceptance.

Local `cargo check-all` and the hosted Linux gate
[37083450089](https://github.com/DavidLMS/nan-harness/actions/runs/37083450089)
pass on `6d66721b`. The next macOS Claude trial
[37083447503](https://github.com/DavidLMS/nan-harness/actions/runs/37083447503)
again acquires two of three initial windows, with all cleanup checks passing.
Both acquired inventories identify exactly one classic source-labelled editable,
no Send message control and one Start task control. This supports investigating
Cowork-to-Chat navigation; it does not authorize submitting a Cowork task.
The next passive inventory scopes Chat/Cowork counts under the exact Mode group
from the frozen renderer, rather than matching a global Chat button.

Windows Claude
[37083448793](https://github.com/DavidLMS/nan-harness/actions/runs/37083448793)
acquires a window with no generic editable, then fails cleanup at the bounded
process-enumeration deadline. Unlike earlier runs it provides no positive
post-stop presence receipt. Its native known-folder observations are valid and
fresh before launch; all four fixed file-presence flags remain false afterward.
The isolated-profile receipt remains missing, so neither environment inheritance
nor storage location is established. Closed capture/read/decode stage receipts
will identify why the isolated-profile observation is unavailable.

Source investigation rejects two proposed shortcuts. The pinned x11rb cursor
loader does not publish cursor names through XFixes, so a rendered hand cursor
cannot be inferred from a missing server name. The Codex avatar's initial
nonfocusable setting is mutable during QuickChat; its exact route alone cannot
justify a two-page exception. Public dismissal retains that renderer for ten
minutes, and forced target closure can recreate it. No such exception or closure
has been added.

The next Linux Zed diagnostic compares the held Retry's published WINDOW bounds
against its same-application ancestor bounds before and after the existing
mouse movement. Only a closed containment/status/count result leaves memory;
references and rectangles remain transient. This can falsify published ancestor
containment but cannot prove GPUI's rendered content-mask hitbox or callback.
The original single click, owned-window guards and deadline remain in force.

macOS Claude
[37084353789](https://github.com/DavidLMS/nan-harness/actions/runs/37084353789)
stops before inventory on the first trial: initial AX focus queries report
CannotComplete while a nonintersecting, non-normal-layer same-process panel is
above the sole eligible window. Cleanup passes. The initial acquisition phase
now applies the already-defined read-only pending-focus predicate, rechecks
ownership and resets continuity within the original 45-second deadline. It
never treats an incomplete query as readiness or retries a fit. The one-time
native fit also publishes its closed failure stage; only timely empty helper
output still proves successful fit.

Windows Claude
[37084355297](https://github.com/DavidLMS/nan-harness/actions/runs/37084355297)
does not reach application qualification: the closed desktop-session preflight
step fails. No application version or native probe exists, so this is not
compatibility evidence and does not diagnose the preflight's cause. The next
trial records first/last results and exact bounded counts of existing post-stop
process queries. It adds no query and changes no cleanup verdict or deadline.

Linux Zed
[37084356150](https://github.com/DavidLMS/nan-harness/actions/runs/37084356150)
passes reply/tool acceptance and cleanup in all three trials; Retry recovery
still fails. All three ancestor observations are unavailable with zero checked
bounds. This cannot establish clipping. Pinned AccessKit confirms GetApplication
is a method and Parent is a property; changing those APIs blindly is unwarranted.
A closed query-stage observation will distinguish the failed existing read.


The next macOS startup trial permits one ordinary public Chat navigation only
when fresh source-scoped Mode/Chat counts, nonempty stable AX identity, owned
foreground and unchanged native window all agree. A separate absolute five-second
navigation phase remains inside the worker watchdog; it is never reset. Failed
or uncertain AX completion is observed without a second press. Only classic
composer plus Send message and absence of Start task proves the navigation
postcondition. No conversation input, response assertion or qualification is
added. xa11y cache handles are freshly allocated per query and cannot establish
AX identity; the guard uses stable identifier, PID, bounds and the exact source
scope instead.


### Wave 8 restart continuation

Revision `9f2d494e` retains five qualified cells out of thirteen supported cells.
Hosted Linux quality [37085892041](https://github.com/DavidLMS/nan-harness/actions/runs/37085892041)
passed. The local final gate found a native fit diagnostic regression: rejected
requests must retain a nonzero helper exit code. The correction preserves that
contract while the bounded fit transport accepts only the closed rejection
protocol for its diagnostic. A separate synthetic fixture timed out in the full
run and passed in the focused run; the final gate must be repeated after fixes.

Claude macOS [37085888754](https://github.com/DavidLMS/nan-harness/actions/runs/37085888754)
stopped before composer/navigation with a same-process-window rejection; cleanup
passed. The first AX receipt does not describe the later fresh decision. The next
trial records that terminal decision from the same snapshot without another query
or a guard exception.

Claude Windows [37085889780](https://github.com/DavidLMS/nan-harness/actions/runs/37085889780)
failed private storage preparation and cleanup. Forty existing post-stop process
queries reported presence at both ends. This does not establish membership in the
owned job. The private-root creation failed before launch. The next correction
creates only the final owned directory under existing canonical AppData parents,
avoiding recursive traversal of the Windows verbatim drive prefix. The frozen
MSIX explicitly excludes LocalAppData\Claude-3p from write virtualization;
blanket package redirection is therefore insufficient to explain missing native
third-party storage.

Zed Linux [37085890943](https://github.com/DavidLMS/nan-harness/actions/runs/37085890943)
again passed reply/tool and cleanup in all three trials but failed Retry recovery.
Each ancestor diagnostic stopped at ancestor-bounds, with no measured ancestor
bounds. No coordinate correction or clipping conclusion follows from that result.

The next Windows Codex observation correlates the already held sole main target
with a later source-scoped auxiliary target, using bounded passive samples. It
keeps the multiple-page rejection and performs no auxiliary dismissal or input.
Neither diagnostic invents full acceptance for inventory-only backends.


### Active scope correction and startup handoff

The official Linux Codex package is documented at
[OpenAI Linux desktop](https://learn.chatgpt.com/docs/linux/linux-app), and
[Claude installation](https://support.claude.com/en/articles/10065433-install-claude-desktop)
documents Ubuntu/Debian x64 and ARM64 beta packages. The catalog already resolves
these official APT sources. Earlier statements that the two Linux cells lacked
official distributions were incorrect; distribution availability is separate
from native qualification. Pen is explicitly excluded from the active matrix
and aggregate, preserving a full inventory for later work.

On `82e7758e`,
[Claude Windows 37087363582](https://github.com/DavidLMS/nan-harness/actions/runs/37087363582)
confirms private-root preparation and the third-party Local State file appearing
inside the owned profile. Cleanup remains failed, with first/last presence across
37 existing process queries.
[Claude macOS 37087361588](https://github.com/DavidLMS/nan-harness/actions/runs/37087361588)
proves initial focus but rejects bounds changing after final acquisition and
launch-diagnostic capture, before the first conversation guard. The next trial
moves the existing final initial binding after those diagnostics, immediately
before conversation readiness. It preserves the original 45-second deadline,
original window identity, ownership, both focus proofs and stability rules; no
post-input rebinding is added.

[Codex Windows 37087365578](https://github.com/DavidLMS/nan-harness/actions/runs/37087365578)
produces only a not-run envelope in both attempts. The runner now retains a closed
failure category in that same unqualified envelope, without exception text or
raw application logs. [Quality 37087916144](https://github.com/DavidLMS/nan-harness/actions/runs/37087916144)
and the local full gate with `RUST_TEST_THREADS=1` pass on `82e7758e`; parallel
local tests expose intermittent two-second synthetic-script readiness timeouts.

### Current startup and Linux panel follow-up

The next candidate prepares every owned Codex state ancestor with the private
filesystem contract before the ordinary launcher creates its managed profile.
Run [37114901374](https://github.com/DavidLMS/nan-harness/actions/runs/37114901374)
reported `root-component` rejection in all Linux and macOS sessions. Windows
reached the renderer but could not bind its initial main document. Initial
binding now waits within the existing deadline for the sole owned blank target
to commit the exact official primary route; it still rejects a second target,
unknown route, changed identity or lost ownership before granting input.

Claude macOS records the independent window-only accessibility query error as
well as the full focus query. Only explicitly incomplete queries may remain
pending during the original initial acquisition budget. Runtime focus guards
are unchanged. Claude Windows records closed facts about the existing owned
wrapper termination and release; these facts do not establish that job members
are absent and do not waive the independent process-absence requirement.

The Linux Zed panel trial uses a fixed private binding for the public
`workspace::ToggleZoom` action. It sends that action once and requires the
source-backed selected Minimize control before submitting the first prompt.
An acknowledged keyboard transport alone is insufficient. Existing exact
prompt readback, ownership, accessibility hit testing and cursor checks remain
mandatory for the later single Retry action. These changes are candidates for
hosted verification, not additional qualified acceptance cells. Pen remains
excluded from the active qualification matrix.

The complete local `cargo check-all` gate passed on `33e0cc2a` with serial
Rust tests and incremental compilation disabled, including all 333 desktop
checker tests. Its Linux hosted quality run rejected an unused diagnostic
accessor in test builds; the subsequent fix now exercises that accessor in the independent
window-query contract test.

Run [37116677735](https://github.com/DavidLMS/nan-harness/actions/runs/37116677735)
acquired Claude macOS's editor in all three fresh sessions, observed no login
controls, completed authenticated model discovery, and passed cleanup. Chat
navigation still stopped before input: the native transport and caller both
added a line terminator, so the strict helper rejected the request. Commit
`929c2ec5` removes the duplicate terminator and adds a synthetic transport
regression test that rejects the formerly malformed request.

Run [37116675663](https://github.com/DavidLMS/nan-harness/actions/runs/37116675663)
confirms that the isolated Codex ancestors no longer prevent Linux and macOS
startup. Renderer binding remains unproved on all three platforms. The next
closed initial-document receipt reports only route categories and identity-field
presence, without URLs, target identifiers, frame identifiers or application
content. This diagnostic never grants an input capability.

Run [37116681120](https://github.com/DavidLMS/nan-harness/actions/runs/37116681120)
refused Linux Zed's first Send because the selected zoom control remained
unproved. Zero matching icon rectangles also explain its per-button containment
rejection counts; those counts alone do not prove a coordinate conversion bug.
Run [37116679406](https://github.com/DavidLMS/nan-harness/actions/runs/37116679406)
issued Claude Windows's owned wrapper termination and released the wrapper,
but independent process absence still failed. Neither wrapper release nor a
future process-ancestry diagnostic can substitute for that cleanup verdict.
These measurements add no qualified cells.


### Source identity and bounded native follow-up

Hosted quality run [37118762408](https://github.com/DavidLMS/nan-harness/actions/runs/37118762408)
passed on `12528df4`. Native results on that source add no qualified cells:

- Claude macOS run [37118758818](https://github.com/DavidLMS/nan-harness/actions/runs/37118758818)
  completed one guarded Chat press, but the last measured counts remained one
  classic editor, one Start task control and zero Send message controls. Two
  other sessions failed stable-window acquisition; cleanup passed throughout.
- Claude Windows run [37118760617](https://github.com/DavidLMS/nan-harness/actions/runs/37118760617)
  observed five surviving processes with retained creation-time descendant
  identities, zero unlinked matches and no surviving launcher. This proves
  historical ancestry, not Job membership. Independent cleanup still failed.
- Zed Linux run [37118756754](https://github.com/DavidLMS/nan-harness/actions/runs/37118756754)
  exhausted the original zoom-proof budget before any Send. The candidate now
  reuses already retained canonical candidates while rechecking ownership,
  selected state, geometry and native hit identity at each hover; the deadline
  and unique-tooltip requirement are unchanged.

Codex run [37118189156](https://github.com/DavidLMS/nan-harness/actions/runs/37118189156)
proved initial target/frame/loader presence on the exact primary route in all
nine probes. Later main binding failed. The pinned onboarding source renders
its role form inside an ordinary div, independently of visible foreign dialogs.
The candidate identifies exactly one source role root; separate modal guards
still block every role/Continue click. Synthetic fixtures prove this separation
and reject duplicate source roots.

Claude's pinned macOS renderer uses Start task when the composer is on a new
agent route; that label alone does not identify Cowork. Its mode pill instead
sets `aria-current="page"` when active. Chromium's
[native mapping](https://chromium.googlesource.com/chromium/src/+/HEAD/ui/accessibility/platform/ax_platform_node_cocoa.mm)
exposes the token as `AXARIACurrent`. A candidate checks that exact token twice
on the retained, uniquely scoped Chat control after the existing window,
geometry and hit proofs. A `current-chat` receipt means no press was attempted;
it neither asserts a navigation transition nor qualifies conversation behavior.
Unknown/unsupported values retain the existing guarded one-press path. Pure
CoreFoundation tests reject booleans and other tokens; no native application is
launched locally.


Run [37119592690](https://github.com/DavidLMS/nan-harness/actions/runs/37119592690)
on `4ad1ef60` proves the source Chat pill is already current in both acquired
macOS sessions. The exact native current-page token, unique scoped control,
window ownership and hit proof succeed; no redundant Chat press occurs. Each
session observes one classic editor, Start task and no login controls. The
third session fails stable-window acquisition. All cleanup passes. This remains
startup evidence, not conversation acceptance.

Run [37119370868](https://github.com/DavidLMS/nan-harness/actions/runs/37119370868)
on `c4ce9449` still leaves Codex unqualified on all platforms. Windows observes
the 11-radio role form and later a second page; macOS/Linux observe no role
form. Initial document capture succeeds everywhere. A closed confirmation
receipt now distinguishes missing initial binding, changed document, unfocused
document, unmatched source scope, rejected guard and deadline using only flags
and source-control counts from existing queries. No focus action or identity
rebind is inferred from this diagnostic.

Run [37119372629](https://github.com/DavidLMS/nan-harness/actions/runs/37119372629)
on `c4ce9449` still exhausts Zed Linux's zoom-proof budget before Send. The next
receipt records the last fixed tooltip phase and bounded remaining milliseconds
at entry/exit, without changing the deadline or authorizing input. The official
Linux Claude 2.9939.4 source-label inventory is separately enabled only after
the runner verifies its frozen release identity. Linux source hashes cannot be
mixed with the inspected macOS hashes.


The integrated native candidate adds a macOS Claude Chat controller. Its private
request includes a fixed owned window, checker-parent identity and absolute
monotonic cutoff; native actions reject parent loss or late execution. Input
requires current Chat, one source-labelled editor, exact prompt readback and a
single enabled source send control. Response Copy requires the fresh settled
assistant heading and a unique control in its native scope. Retry requires the
same failed prompt and controlled failure in one bounded scope; missing native
relationships block input. Clipboard and independent provider observations are
both required. The `native-assistant-clipboard` response method is distinct from
Zed's thread export. Startup-only observations remain unqualified; the full
backend requires all five steps in three fresh sessions and successful cleanup.

Claude Windows retains verified native process handles before the ordinary
owned-wrapper stop. A separate bounded holder locks the original executable,
verifies its launch-time SHA256 and historical creation-time ancestry, and can
terminate only those retained same-image handles after ordinary shutdown. It
cannot acquire targets after the trigger or reopen a bare PID. Its action cutoff
uses the original absence deadline, with parent-liveness and identity checks
immediately before termination. An uncertain holder receipt never proves cleanup;
the independent application/process absence check remains authoritative.

Integrated focused checks passed: all-target locked checker Clippy, all 349
checker library tests, 87 qualification contracts, both Codex behavioral suites
and pure native selector/process fixtures. Actual macOS conversation behavior
and Windows descendant termination still require disposable hosted evidence.
The final repository-wide gate will run on the final converged source.


### Hosted continuation, 2026-10-03

The full local repository gate passed on `8c514f5c` with serial Rust tests and
incremental compilation disabled. Later changes have focused verification;
they still require the final gate on the converged tree.

Run [37123153883](https://github.com/DavidLMS/nan-harness/actions/runs/37123153883)
verified Windows Claude cleanup against five retained owned descendants, with
independent application and global absence checks passing. Startup still
blocked on changed window geometry. The Windows initial-readiness policy was
subsequently found to reject Python's ordinary drive path because Rust
canonicalization adds a verbatim prefix. The corrected policy validates every
original ancestor against reparse points before retaining its canonical path.
A hosted synthetic directory test precedes the next Claude measurement.

Run [37125059406](https://github.com/DavidLMS/nan-harness/actions/runs/37125059406)
completed all three Zed Linux candidate hovers under one thirty-second absolute
zoom-proof budget. None exposed the exact source tooltip, so input submission
remained blocked. The original ten-second allocation could not reach its first
hover while preserving the required helper reserve. The longer allocation
changes timing only; unique control proof and all ownership guards remain
mandatory.

Run [37125317940](https://github.com/DavidLMS/nan-harness/actions/runs/37125317940)
reached the macOS Claude conversation controller in two fresh sessions. Both
rejected input readback before Send. This is progress past acquisition, not
conversation qualification. The controller's next receipt distinguishes an
unavailable or nonempty initial value from clipboard and native value
mismatches without publishing text.

Run [37124585800](https://github.com/DavidLMS/nan-harness/actions/runs/37124585800)
confirmed Codex Windows' original main renderer and the source-known inert
auxiliary renderer. Role selection remained blocked before any click. The
next diagnostic distinguishes the exact rejected main/auxiliary proof and
elapsed deadlines. Passive startup classifications remain unknown; they do
not establish a login requirement. No additional cell is qualified by these
observations. Pen remains deferred.

Pinned GPUI source inspection subsequently established that tooltip titles are
raw `SharedString` elements without accessibility IDs. The absence of the
Disable Full Screen text is therefore inconclusive. Tooltip observations no
longer authorize zoom; the retained ON control and unique source icon remain
required. Existing WINDOW-to-client normalization already handles a missing
AccessKit screen origin. The failed association now needs source-raster
calibration, rather than another coordinate offset or weaker match threshold.

Run [37125656759](https://github.com/DavidLMS/nan-harness/actions/runs/37125656759)
stopped at the synthetic Windows directory contracts before application
installation. Normal and verbatim directory spellings were rejected, while the
junction rejection test passed. This runner result does not measure Claude
startup and must not be interpreted as an application failure.


### Owned session corrections, 2026-10-03

The complete local gate and hosted Linux quality-only run
[37126269012](https://github.com/DavidLMS/nan-harness/actions/runs/37126269012)
passed on `fcf16619`. Later implementation changes require the final gate on
the next converged tree.

The synthetic Windows qualification-directory contracts passed in
[37126171093](https://github.com/DavidLMS/nan-harness/actions/runs/37126171093).
Actual startup then rejected an off-display window. That failure also revealed
that discarding the acquired GUI before final readiness removed the native
transport needed for owned cleanup. The acquired transport now remains held
through cleanup even when readiness refuses input; off-display rejection is
unchanged.

macOS Claude in
[37125969670](https://github.com/DavidLMS/nan-harness/actions/runs/37125969670)
reached the conversation controller in three sessions, but all three stopped
at a nonempty initial accessibility value before any input. The owned-profile
controller now borrows the actual native-root owner, verifies both newly
created private directories retain their original identities, and permits an
ordinary Select All and Paste only after proving the retained editor is the
application's focused accessibility element. Legacy input still rejects a
nonempty editor. Exact clipboard and native-value readback remains mandatory
before the single ordinary Send action. Synthetic ownership-loss, selector,
request-framing and privacy contracts pass; actual conversation acceptance
remains pending hosted evidence.

Codex Windows in
[37125658347](https://github.com/DavidLMS/nan-harness/actions/runs/37125658347)
rejected its renderer page set before role selection in all three sessions.
The new diagnostic retains only a closed rejection category, bounded page
counts and whether the original main page remained present. It records no
route, title, identity or raw exception. It does not authorize a replacement
main page or relax auxiliary renderer admission.

Zed Linux in
[37126267101](https://github.com/DavidLMS/nan-harness/actions/runs/37126267101)
observed a best minimize-icon correlation of 980 milli in all three sessions,
below the unchanged 985 threshold. Exact pinned GPUI source identifies resvg
rasterization, doubled-resolution linear sampling and brightness-dependent
Linux shader correction. Selected zoom also uses the theme's accent color.
Source-generated reference correction is in progress; measurements remain
unqualified. Five active cells remain qualified, seven remain open, and Pen
remains deferred.


Follow-up run
[37127849670](https://github.com/DavidLMS/nan-harness/actions/runs/37127849670)
on `e07b14b1` verifies the cleanup correction against five retained Windows
Claude descendants: all five targeted processes exited, none was rejected,
and independent application/global absence checks passed. Initial readiness
still refuses the off-display window. The subsequent one-shot fit retains
its use state across both acquisition phases and requires two equal geometry
samples, the original HWND/PID/name, owned ancestry, foreground and a clear
window stack; it never permits a second fit or input before strict reacquisition.

Run [37127849665](https://github.com/DavidLMS/nan-harness/actions/runs/37127849665)
with Claude `2.19675.0` verifies one input and one Send in a fresh macOS session.
Response acquisition then stopped at a pre-action tree read. Another session
reported an uncertain action, which is never replayed. All application and
global cleanup checks passed. This is progress through input, not successful
response/retry qualification.


Run [37128104248](https://github.com/DavidLMS/nan-harness/actions/runs/37128104248)
on `8ebb5977` verifies input, response and the read-tool scenario in all three
Linux Zed sessions. Correct source rasterization raises the retained Minimize
reference correlation to 999 milli, above the unchanged 985 requirement.
Recovery remains blocked before its single Retry action: all nine queried AX
hit points match the retained control, but the native cursor is an arrow.
Pinned source defines this HTTP400 recovery control as an enabled filled text
button with `PointingHand`, so dropping the hand check is not justified. The
next helper verifies actual pointer position and owned client child before
sampling, and requires two hand samples at the same held point within the
original four-second deadline. It retains no pointer coordinates or pixels.

Run [37127849568](https://github.com/DavidLMS/nan-harness/actions/runs/37127849568)
confirms the Windows Codex page-set failure in all three sessions: one page
becomes two during a proof while the original main remains present, before any
role click. The guard permits one incomplete pre-input proof to be discarded
for this exact transition only. A full fresh proof must retain the original
main document and prove the source-known inert auxiliary twice. The first
attempted role click permanently seals this permission, including uncertain
failures. No page replacement, extra target or later-action retry is admitted.

Codex macOS
[37128182583](https://github.com/DavidLMS/nan-harness/actions/runs/37128182583)
and Linux
[37128182781](https://github.com/DavidLMS/nan-harness/actions/runs/37128182781)
remain blocked at a dialog lacking the expected role scope. Closed startup and
managed-sign-in classifications remain unknown. A new passive source-dialog
classifier checks the exact published workspace-load failure title and its
Try Again button, without granting action permission. macOS also reports a
native ownership rejection; its original renderer identity remains unchanged
where the query completes. Neither observation establishes a login requirement.

macOS Claude's passive tree acquisition now distinguishes query failures from
terminal identity, focus, type, duplicate and limit failures. Only a transient
query failure before Copy or retry-readiness permits a fresh passive read under
the original polling deadline. Input, Send, Retry and uncertain Copy actions
are never repeated by this policy. Synthetic receipts, pure native selectors,
framing and privacy checks pass; hosted qualification remains pending.

### Source-bound acquisition follow-up

Revision `5fb92e1b` passed the complete local gate and the hosted Linux gate in
[37129498896](https://github.com/DavidLMS/nan-harness/actions/runs/37129498896).
Native measurements remain separate from those deterministic quality checks:

- Zed Linux in [37129240885](https://github.com/DavidLMS/nan-harness/actions/runs/37129240885)
  again verifies input, response and the read-tool scenario in all three
  sessions. Actual pointer position and owned client-child checks now match,
  but Retry still has an arrow cursor; no Retry click is attempted. Published
  AX ancestor bounds are now sampled on a completed rejected cursor scan too.
  These bounds are advisory layout, not a measurement of GPUI's paint mask.
- Claude macOS in [37129240909](https://github.com/DavidLMS/nan-harness/actions/runs/37129240909)
  verifies one input and one Send, then stops before a response-action receipt.
  Another session loses focus. Revision `b9ea38fc` removes an adjacent duplicate
  guard while retaining the immediate pre-action and post-action checks, and
  records closed action phases and native transport failure categories.
- Claude Windows in [37128325832](https://github.com/DavidLMS/nan-harness/actions/runs/37128325832)
  still fails display containment, with five retained descendants cleaned up.
  Revision `b9ea38fc` moves the sole fit to source-editor-ready finalization.
  Passive attachment cannot authorize input; the original window still needs
  strict display, focus and stack reacquisition afterward. An uncertain fit
  consumes the attempt. The Windows runner tests this policy before installing
  the application.
- Codex Windows in [37128779405](https://github.com/DavidLMS/nan-harness/actions/runs/37128779405)
  passes the initial one-to-two page settlement, then rejects an unknown
  separate dialog before its first role click. Codex macOS in
  [37129512258](https://github.com/DavidLMS/nan-harness/actions/runs/37129512258)
  and Linux in [37129512273](https://github.com/DavidLMS/nan-harness/actions/runs/37129512273)
  also see one unknown dialog. Both exact workspace-failure title and Retry
  counts are zero; the published workspace-failure hypothesis is ruled out.
  These observations do not establish that an account is required.

The macOS listener proof now explicitly requests `lsof`'s `f` descriptor field
alongside PID and endpoint. The parser already requires that field;
[lsof's field-output contract](https://lsof.readthedocs.io/en/stable/manpage/)
does not guarantee it implicitly. A disposable synthetic child listener tests
actual owned ancestry and rejection of foreign ancestry without launching an
application or retaining process identities. At revision `e7332918`, the proof
still required one listener record. The bounded multi-owned-listener follow-up
and its hosted result are recorded below.

There are still five qualified active cells and seven open active cells. Pen
remains deferred. Acquisition and diagnostic improvements are not additional
acceptance results.

### Response scope and owned acquisition follow-up (2026-10-03)

The [30c Codex macOS trial](https://github.com/DavidLMS/nan-harness/actions/runs/37133477016)
identifies two complete loopback-listener records in two processes. Revision
`1fc6d7e5` permits at most four complete exact-endpoint records only when every
listener process freshly descends from the original launch owner. All native
queries share one bounded deadline, clipped to the caller's existing deadline;
no process ownership proof is cached across actions. Malformed, foreign,
non-loopback and excessive listener sets remain rejected. Synthetic native
listener and deterministic parser/deadline fixtures pass; hosted acceptance
still requires the full independent conversation and cleanup checks.

The [30c Claude Windows trial](https://github.com/DavidLMS/nan-harness/actions/runs/37133479080)
identifies one foreign overlapping window at passive attachment, with the
original off-display foreground window still uniquely owned and no same-process
window ahead. Revision `4fd39d86` permits passive source-editor inspection and
the sole owned no-activate/no-z-order fit in this state. Fresh strict display,
foreground and occlusion checks remain mandatory before input. The two pure
Windows acquisition tests pass. This is a startup experiment; Windows and Linux
still require platform-specific native conversation controllers before full
qualification is possible.

The [f223 Claude macOS trial](https://github.com/DavidLMS/nan-harness/actions/runs/37134065950)
verifies input and Send in two sessions. Both independently observe a provider
generation and completed fixture response without provider failure. The native
assistant anchor remains absent, so neither session verifies Copy or Retry.
Chromium's native heading Value is a numeric heading level; its name can be
Title or Description. Revision `8c4de894` selects the exact source assistant
prefix from those attributes on the same heading, rejects conflicting names,
and retains marker uniqueness and row-bound control checks. Revision `03562514`
reports a remaining action budget exhausted by the teardown reserve as Timeout,
without extending the budget or replaying input. Pure native selection,
transport and deadline fixtures pass.

The [default-viewport Zed Linux trial](https://github.com/DavidLMS/nan-harness/actions/runs/37132548916)
verifies input, response and the read-tool scenario in all three sessions,
then rejects Retry before any click. Ancestor bounds are invalid geometry in
all three; owned client-child and actual pointer checks still match.
The [larger-viewport experiment](https://github.com/DavidLMS/nan-harness/actions/runs/37134063867)
also fails to make Retry actionable and introduces two tool input mismatches.
Revision `fabe93a5` restores the default viewport. A resize is not a demonstrated
fix, and the cursor/actionability requirement is unchanged.

The immutable `3b71ef65` integration passed both the complete local gate and
[hosted Linux quality gate](https://github.com/DavidLMS/nan-harness/actions/runs/37132550745).
The immutable `f223df0d` tree also passed the complete local gate. These quality
results do not promote native cells: five active cells remain qualified, seven
remain open, and Pen is deferred.

The complete local gate also passed on immutable `03562514`. Its
[Codex macOS trial](https://github.com/DavidLMS/nan-harness/actions/runs/37135593304)
proves endpoint ownership in all three sessions, but rejects an unknown separate
dialog before the first role click. The multi-listener correction fixes this
acquisition boundary; it does not establish account or conversation readiness.
Revision `7b5aa2ab` adds a passive, source-defined imported-setup title/button
classifier. It cannot dismiss a dialog or authorize another action.

The [03562514 Claude Windows trial](https://github.com/DavidLMS/nan-harness/actions/runs/37135595388)
acquires the original window, observes one source editor, completes the sole fit
and reaches strict startup readiness in its first session. The second session
loses foreground before acquisition and fails cleanup; the third is not run.
The retained-descendant cleanup was skipped when no Gui existed. Revision
`aa6c3ceb` retains handles through the standalone native transport in that case,
under the same hosted policy, original launcher ancestry, executable digest
and cleanup deadline. It does not authorize terminating a name-matched process.
The bounded holder protocol tests pass; hosted verification remains necessary.

Claude's frozen response-summary implementation limits the accessible heading
summary to 160 characters. The previous semantic word marker is at least 178
characters. Revision `0aeb6f65` uses a 32-digit hexadecimal marker in semantic
scenarios, preserving all 128 random bits and exact full-response clipboard
verification. Pure fixtures confirm round-trip entropy and preservation by the
actual frozen summary function. Its [macOS trial](https://github.com/DavidLMS/nan-harness/actions/runs/37136424228)
still observes a completed provider response without a native response anchor;
this correction is necessary for that summary contract but is not a sufficient
fix. Revision `029ef1f2` distinguishes no native heading, no assistant-prefix
heading and no marker in an assistant heading, using only the already-collected
tree and closed failure labels. It adds no input or accessibility queries.

The [12d6a0f6 Zed Linux trial](https://github.com/DavidLMS/nan-harness/actions/runs/37136660987)
again verifies input, response and the tool scenario in all three sessions.
The bounded passive X11 inventory reports zero mapped or unmapped owned transient
Dialog windows in each session. The blocked-parent Dialog hypothesis is therefore
not supported by this trial; Retry still rejects before any click. Native
cleanup passes and the strict privacy reducer reports zero invalid events.

The [aa6c3ceb Windows trial](https://github.com/DavidLMS/nan-harness/actions/runs/37140104786)
confirms cleanup after both acquired and failed-window sessions: each retains
and closes five verified descendants, with global cleanup passed and zero invalid
native diagnostic events. One session reaches owned-window fit and one source
editor; a later foreground change still blocks acquisition. No conversation is
qualified by these startup results.

Revision `ac4acf9e` adds a passive Windows UIA collector after strict initial
readiness. It receives the retained acquisition deadline, capped at three seconds,
and rechecks exact native HWND/PID/rectangle, foreground, display and clear stack
before and after a complete bounded tree. Closed counts and failure stages can
expose native control projection; they cannot authorize Send, Copy or acceptance.
The normal hosted build compiles its Windows SDK body before vendor installation,
and a separate pre-install test checks the strict receipt parser. The complete
privacy reducer passes 97 deterministic contracts locally; native projection
remains to be observed on the disposable runner.

The `02547346` follow-up compiled the actual Windows UIA implementation and
passed the pre-installation Rust receipt tests in
[run 37140686691](https://github.com/DavidLMS/nan-harness/actions/runs/37140686691).
The acquired window produced a transport failure rather than UIA counts; both
acquired and failed-acquisition sessions again cleaned up five verified owned
descendants, with zero rejected diagnostic events. macOS
[Claude run 37140688993](https://github.com/DavidLMS/nan-harness/actions/runs/37140688993)
failed before the response-heading boundary could be measured: one input
transport timed out, another sent a verified input but stopped before the next
guard, and the third lost focus. The diagnostic provider snapshot recorded a
generation without completed fixture response; it does not establish the final
provider or renderer state. Cleanup passed. In
[Codex run 37140690985](https://github.com/DavidLMS/nan-harness/actions/runs/37140690985),
all three endpoints were owned, but the separate dialog remained unknown and
no role or Continue action was dispatched. All three results remain unqualified.

The `00ab3098` Linux feasibility trial replaces the pointer Retry attempt with
one AT-SPI `DoAction(0)` on the retained unique button. It requires two matching
ownership, state, bounds and single-click-action proofs, consumes the attempt
before dispatch, and rejects uncertain completion without fallback or replay.
A true forwarding receipt and unchanged post-action ownership permit subsequent
observation only; qualification still requires a new provider recovery and an
exact export of the same failed turn. The production qualification workflow
continues to use its existing route pending full native evidence.

The Windows UIA transport failure exposed a concrete request framing bug:
the UIA encoder appended a newline and the shared process writer appended
another. The native receiver rejected the extra byte before any UIA query.
Commit `cb776b6d` makes the shared writer own the delimiter. A synthetic test
uses the actual Rust process transport to distinguish one frame from the old
double frame; the shared C++ framing fixture rejects trailing bytes, missing
delimiters and oversized input during the pre-installation native build.

A public-source audit also identifies the separate Computer History consent
screen in Codex `26.930.31730` (`onboarding-page-5330b67eb049.js`, SHA-256
`05667867f6c8b4525a8009610dc942881bdcbff3a737a3dd3411e28956a88137`).
The follow-up observer measures exact public title, source form and control
counts within the retained dialog, using the existing two identical reads and
ownership guards. The classifier remains passive, never accepts permission,
and does not establish that this was the dialog in the preceding run.
Project-import title/control counts are likewise source-bound and diagnostic.

Claude macOS Chat pre- and post-action guards now share the original action
deadline with the native transport. Previously those focus queries could use
an independent fifteen-second cap. The original window verdict remains
unchanged, and expiration cannot reset the budget or spawn a second guard
helper. A synthetic native-process regression covers that exhausted-deadline
case. This corrects the timing boundary; it does not establish the cause of
the preceding `before-guard` failure.

Commit `8ad3a361` passed the complete local `cargo check-all` gate. Its native
[Codex run 37141943730](https://github.com/DavidLMS/nan-harness/actions/runs/37141943730)
remains unqualified: two sessions classified the same unknown separate dialog
with all eight Computer History/project-import source counts zero; the third
failed the role-legend proof. No role, Continue or permission action occurred.
[Claude macOS run 37141943800](https://github.com/DavidLMS/nan-harness/actions/runs/37141943800)
failed acquisition in one session and lost focus in another before verified
input. The native Chat receipt stopped at its post-action guard; no response or
retry was certified. Both runs passed cleanup with zero invalid diagnostic events.

The one-frame UIA fix passed its actual Windows native build in
[run 37141736476](https://github.com/DavidLMS/nan-harness/actions/runs/37141736476).
The observer now reaches UIA and rejects an element process-identity check,
rather than request framing. The current receipt does not distinguish root
from descendant or a query failure from a mismatched PID. Ownership remains
strict; Chromium's native accessibility implementation runs browser-side, so
accepting arbitrary renderer processes would lack evidence. Cleanup passes.

The AT-SPI Retry experiment in
[run 37141494677](https://github.com/DavidLMS/nan-harness/actions/runs/37141494677)
passes input, response and file-tool verification in all three Zed sessions.
Each receives a true native forwarding receipt with unchanged post-action
ownership. None produces independent provider recovery or assistant export;
the exported failed turn still contains one User and no assistant or Resume.
The cell therefore remains unqualified. Pinned source has no same-turn keyboard
Retry command, and historical-message Regenerate rewinds/resubmits. The public
ScrollOutputToBottom command affects the conversation list, but its editor
focus is not exported by the pinned accessibility tree; an unknown keyboard
context cannot authorize that proposed experiment.

Commit `1d709748` passed the complete local gate. Its
[Claude macOS run 37151572757](https://github.com/DavidLMS/nan-harness/actions/runs/37151572757)
verified input and native assistant-copy response in two sessions. File-tool
verification still failed: the provider observed a completed tool turn and the
fixture response, but not the file's independent marker. This is not evidence
that the selected Read tool successfully accessed the checker fixture.
[Codex macOS run 37151579859](https://github.com/DavidLMS/nan-harness/actions/runs/37151579859)
again rejected the unknown separate dialog; passive actionability measurements
were unavailable. Its
[Linux run 37151586884](https://github.com/DavidLMS/nan-harness/actions/runs/37151586884)
attached to the owned renderer in three sessions but found no role form or
editor and one unknown dialog. All three runs remain unqualified and passed
cleanup with zero invalid diagnostic events.

[Claude Windows run 37151374891](https://github.com/DavidLMS/nan-harness/actions/runs/37151374891)
proved the root process identity and failed at a descendant process mismatch.
The passive observer now distinguishes zero/default and invalid negative
process properties from a different positive identity, without admitting any
of them or publishing process IDs. Microsoft's
[ProcessId property documentation](https://learn.microsoft.com/en-us/dotnet/api/system.windows.automation.automationelement.processidproperty?view=windowsdesktop-10.0)
defines zero as the default when the property is not reported. Synthetic native
and strict wire-decoder tests preserve rejection and null counts for each case.
Actual runner evidence is required before attributing the mismatch to that
specific default.

The follow-up
[Windows run 37153026660](https://github.com/DavidLMS/nan-harness/actions/runs/37153026660)
still reports `descendant-process-mismatch`, proving that the first rejected
property is a different positive identity, not zero/default. A bounded passive
ancestry observer now distinguishes a verified owned descendant, a complete
stable foreign chain, and unavailable correlation. All three remain rejected
with null counts. Root live-handle/creation proof, two bounded process snapshots,
link creation ordering and the original deadline are retained; no renderer
identity is admitted by this experiment.

[Claude Linux run 37153349217](https://github.com/DavidLMS/nan-harness/actions/runs/37153349217)
finds a unique visible Mode group in three sessions, but all eight exact-source
button/radio counts are zero. This rules out the tested compact-radio/attention
label selector variants as a sufficient fix; it does not prove those controls
are unavailable through every native interface.
[Claude macOS run 37153349373](https://github.com/DavidLMS/nan-harness/actions/runs/37153349373)
again verifies input and response in two sessions, then fails at the following
input's collapsed native focus boundary before the tool-result diagnostic can
be written. No tool error or namespace conclusion is established by that run.
Native input receipts now distinguish the original guard, focus-setting,
clipboard and key-dispatch failures without adding queries or replaying input.

The passive Codex measurement in
[macOS run 37153177800](https://github.com/DavidLMS/nan-harness/actions/runs/37153177800)
and all three sessions of
[Windows run 37153358989](https://github.com/DavidLMS/nan-harness/actions/runs/37153358989)
reports `geometry-outside`; the unknown separate dialog still blocks action.
Passive samples now remain within the intersection of integer client dimensions
and fractional DOMRect dimensions. This avoids rejecting a rounded client edge
alone; viewport containment and all action guards remain unchanged. CSSOM View
[defines clientWidth/clientHeight as integer properties](https://drafts.csswg.org/cssom-view/#extensions-to-the-element-interface).
Actual evidence is still required to determine whether fractional rounding
explains these receipts. All four cited application runs remain unqualified,
pass cleanup and emit zero invalid diagnostic events.

The intermediate local gate failed at five CLI fixture publication waits and
one discovery sentinel. The process metadata fixtures unnecessarily executed
newly created scripts even though assertions only inspect metadata or a missing
sibling; exit-status cases already use `/bin/sh`. That extra fixture launch has
been removed without changing the assertions. All seven focused process tests
and the independent discovery sentinel now pass. The converged final gate must
still pass before this iteration is considered verified.


### Retained native identities and passive dialog titles

Commit `045d1562` passed the complete local gate and hosted
[quality run 37154433018](https://github.com/DavidLMS/nan-harness/actions/runs/37154433018).
Actual application evidence remains separate:

- [Claude macOS 37154355791](https://github.com/DavidLMS/nan-harness/actions/runs/37154355791)
  verifies input and response in two sessions, then rejects the next input at
  `input-focused-identity`. A passive wait now requires the exact retained
  composer before every existing key, within the original request deadline.
  Different owned focus may settle; target mutation or ownership loss rejects.
- [Claude Windows 37154068497](https://github.com/DavidLMS/nan-harness/actions/runs/37154068497)
  identifies the first rejected descendant as owned. Passive collection now
  retains verified child process handles and creation identities through the
  complete inventory, alongside the original live root and window guards.
  Foreign, unreported, changed or unavailable identities still discard counts.
- Claude Linux now measures sixteen role/label counts relative to the retained
  Mode group, with stable provider, PID, accessibility identity and bounds
  before and after. Counts remain advisory and unavailable queries stay null.
- Zed Linux now records the retained Retry target after its sole native action.
  Defunct, changed or unchanged targets do not certify recovery; independent
  provider recovery and resumed assistant export remain required.

[Codex macOS 37154355809](https://github.com/DavidLMS/nan-harness/actions/runs/37154355809)
measures an active unknown dialog: three of nine passive points belong to the
dialog and none to the intended control. It cannot be ignored as a stale
overlay. [Codex Linux 37154355548](https://github.com/DavidLMS/nan-harness/actions/runs/37154355548)
recognizes none of the four known startup dialogs and also fails cleanup; no
specific cleanup cause is established by its closed diagnostics.

A new diagnostic-only catalog contains sixty source-verified static DialogTitle
message IDs from official Codex `26.930.31730` on macOS and Linux. The manifest
records per-callsite hashes and the helper requires exact platform artifact
pins. Both source wrappers render Radix DialogTitle as an `h2` referenced by
the dialog. The reader retains that exact title and dialog across two matching
reads under original document/frame/loader/endpoint guards. Visually hidden
source titles can identify an active visible dialog. Duplicate IDs, changed
references and ambiguous labels reject or remain ambiguous. Only fixed public
catalog IDs and bounded counts leave the page; no title text or DOM IDs do.
Recognition authorizes no dismissal, input or qualification. A separate Windows
manifest now pins the same sixty IDs to the verified official MSIX, with its
own artifact, wrapper and catalog hashes; macOS/Linux manifest bytes are unchanged.

Focused verification passes: 378 serial checker tests, checker Clippy with
warnings denied, synthetic native identity fixtures, nine Zed helper tests,
105 strict qualification tests and the static-title Node suite. The new native
behavior still requires hosted application evidence; no additional cell is
qualified by these changes.


The parent now emits a separate closed `parent-journal-seal` receipt when
sealing changes a probe result to `cleanup-failed`. It preserves the original
reason and distinguishes only `io`, `locked`, `invalid` and `conflict`; paths,
messages and OS error payloads are excluded. It does not change sealing or
cleanup policy. The macOS native build also runs the pure retained-composer
fixture before installing any application, with assertions enabled in release
builds.

Hosted experiments dispatched for the retained identity changes:
[Claude macOS 37155936892](https://github.com/DavidLMS/nan-harness/actions/runs/37155936892),
[Claude Windows 37155937963](https://github.com/DavidLMS/nan-harness/actions/runs/37155937963),
[Claude Linux 37155939393](https://github.com/DavidLMS/nan-harness/actions/runs/37155939393),
and [Zed Linux 37155940550](https://github.com/DavidLMS/nan-harness/actions/runs/37155940550).
The first static-title catalog is exercised by
[Codex macOS 37156026123](https://github.com/DavidLMS/nan-harness/actions/runs/37156026123)
and [Codex Linux 37156026993](https://github.com/DavidLMS/nan-harness/actions/runs/37156026993),
with [quality run 37156028197](https://github.com/DavidLMS/nan-harness/actions/runs/37156028197).
Dispatch is not evidence of completion or qualification.


The retained UIA experiment
[Claude Windows 37155937963](https://github.com/DavidLMS/nan-harness/actions/runs/37155937963)
now collects a complete 141-node guarded tree with one classic editor and one
Start task control. This is passive inventory evidence, not input or response
qualification. Start task is also used by a new Chat; it does not identify
Cowork. Current-mode proof is required before an input adapter.
[Claude Linux 37155939393](https://github.com/DavidLMS/nan-harness/actions/runs/37155939393)
finds exactly two visible buttons relative to the retained Mode group, with
zero tested source-label matches.
[Zed Linux 37155940550](https://github.com/DavidLMS/nan-harness/actions/runs/37155940550)
verifies response and tool in all three sessions, but the retained Retry target
is unchanged after forwarding. Independent recovery is still absent.

[Claude macOS 37155936892](https://github.com/DavidLMS/nan-harness/actions/runs/37155936892)
is blocked by readiness/runtime guards. One session submits input and observes
the fixture response, then rejects a tree query as `same-process-window`; no
new exact-focus failure is established by this run. Initial focus receipts
cannot distinguish the rejected runtime predicate. The runtime guard now
records that exact rejected snapshot separately, without another query or
refreshing held geometry.

The first Codex static-title runs remain unqualified: macOS rejects the
catalog guard in all sessions, while one Linux session observes one referenced
title with no known static match. The macOS passive reader now uses the
original captured sole main document before role binding. Input/action guards
are unchanged; a source-scope predicate for controls hidden behind the modal
is no longer a prerequisite for reading its title. All six application runs
above pass cleanup and emit zero invalid diagnostic events.
[Quality run 37156028197](https://github.com/DavidLMS/nan-harness/actions/runs/37156028197)
passes the complete hosted gate and experiment contracts at `d936c222`.

Follow-ups dispatched at `45838176`:
[Codex Windows 37156447227](https://github.com/DavidLMS/nan-harness/actions/runs/37156447227),
[Codex Linux 37156518819](https://github.com/DavidLMS/nan-harness/actions/runs/37156518819),
and [Claude macOS 37156658157](https://github.com/DavidLMS/nan-harness/actions/runs/37156658157).
Their results remain pending. GitHub API requests temporarily returned HTTP
503; named closed qualification artifacts were retrieved after recovery.


The source catalog now includes three additional hoisted static DialogTitle
descriptors, verified separately in all three pinned official artifacts:
Global search, Import unverified extensions?, and Import from your browser.
Each has an explicit descriptor/import data-flow proof into DialogTitle; this
does not admit arbitrary messages whose IDs end in title. The catalog has
63 IDs. An optional `sourceTitleEmpty` flag distinguishes an empty referenced
label from an unmatched nonempty label, without exporting runtime text.

[Claude macOS 37156658157](https://github.com/DavidLMS/nan-harness/actions/runs/37156658157)
reaches two verified/submitted/copied turns in one session and reports a
completed Read result with a tool-error marker and unknown closed category.
Tool verification still fails; no concrete file/path/namespace cause is proved.
[Codex Linux 37156518819](https://github.com/DavidLMS/nan-harness/actions/runs/37156518819)
refuses the frozen project-release policy before qualification. Its
`codex-project-release-mismatch` receipt establishes a policy mismatch, not a
particular new application version or native compatibility failure. Updated
source provenance is required before changing any artifact pin.


Windows UIA inventory now includes optional advisory current-mode proof. It
recognizes the source Mode group and visible enabled Chat/Cowork buttons,
reads bounded `AriaProperties` for exact `current=page`, and compares retained
group/button identities twice. The initial projection reuses the complete
inventory walk; the second projection is limited to retained Mode subtrees.
Fresh bounded parent chains must reach the exact retained root before and
after the second projection. Unavailable, detached or changed proof discards
all mode counts. This adds no input or Invoke action and changes no acceptance
condition. Pure property, attachment and ownership fixtures execute with
assertions enabled on release builds across platforms. Focused Rust transport
tests, 107 strict qualification tests, native pure contracts and checker Clippy
pass; hosted Windows compilation and actual current-mode evidence remain
required.


### Exact-source follow-up: Codex release drift and native guards

The official macOS and Linux Codex artifacts now report `26.930.41038`;
Windows remains at the previously inspected `26.930.31730` artifact. Separate
platform catalogs bind the new artifact, executable and DialogTitle wrapper
hashes. Linux admits 63 inspected static titles and macOS admits 24. A
release mismatch still blocks input; these source updates do not establish
native qualification. Static-title rejection now publishes an optional closed
stage after two consistent measurements, without runtime text or DOM identity.

[Claude Windows 37157851348](https://github.com/DavidLMS/nan-harness/actions/runs/37157851348)
builds and observes a complete 141-node owned UIA tree, one classic editor and
one Start task control. The retained Mode group is present, but the exact
Chat/Cowork button counts are zero; current-mode status is `missing`. No mode
or input action is authorized by that observation.
[Claude macOS 37157852142](https://github.com/DavidLMS/nan-harness/actions/runs/37157852142)
again reaches two submitted/copied responses in one session, then reports a
completed Read tool error with unknown category. Both runs pass cleanup and
emit zero invalid diagnostic events. Neither cell is qualified.

A preceding macOS runtime receipt proves native foreground and full AX focus
while the window-only AX query returns `cannot-complete`. Runtime guards now
passively settle only that existing incomplete-read predicate under the
original deadline and retained identity/geometry. Pending reads never
authorize input; hard rejection, transport failure and deadline expiry remain
terminal. Nine focused runtime tests, 109 strict qualification tests, static
catalog contracts and CLI/checker Clippy pass on the updated tree.


[Updated-source Codex run 37158752056](https://github.com/DavidLMS/nan-harness/actions/runs/37158752056)
passes installation and exact release admission on macOS/Linux `26.930.41038`,
but neither cell qualifies. Linux consistently observes a nonempty referenced
DialogTitle with no match in the inspected 63-title catalog. macOS rejects the
passive scope or changed identity. Windows remains unqualified and its title
reader reports deadline/query rejection after role binding. The Windows passive
reader now uses the original captured sole main document before role binding,
with the same ownership, document identity, two-read and original deadline
guards as macOS/Linux; action guards remain unchanged. All three cells pass
cleanup with zero invalid diagnostic events.

[Claude macOS 37158753060](https://github.com/DavidLMS/nan-harness/actions/runs/37158753060)
again copies two verified turns in one session, then receives an unknown Read
tool error. A separate session exhausts its native-turn deadline before a
verified submission. Public static inspection of the exact SDK `0.3.286` binary
establishes its single `Error calling tool (Read): ` wrapper. The classifier
now removes that wrapper only for selected Read inside one complete tool-error
envelope; marker verification is unchanged. Optional inventory counts only
the source-proven `mcp__nanh-read-fixture__read_file` offer under the existing
private hosted fixture policy. It does not select or invoke that tool.

Windows startup trials now explicitly set the supported managed
`coworkTabEnabled=false` configuration in the owned temporary profile. The
exact Windows `2.19675.0` bootstrap/schema supports that field in third-party
deployment. This does not prove classic Chat is selected: retained runtime
mode/editor observations remain required, and no input authority is added.


The complete Linux `26.930.41038` archive contains additional DialogTitle
callsites outside dialog/onboarding-named chunks. The Linux catalog now has
168 fixed IDs, including 105 additional direct literal Intl children whose
imports resolve to the pinned `eC`/H2 wrapper. Each added row records its
callsite hash and wrapper proof; dynamic placeholders and arbitrary headings
remain excluded. Mac/Windows catalogs are unchanged. This is broader source
coverage, not an identification of the currently observed Linux dialog.

The Mac native-turn cap is now 15 seconds, chosen before each operation; copy
and passive retry queries still clip it to their caller deadline. Native
transport, protocol and helper preserve the original absolute monotonic cutoff
and teardown margin. A prior `deadline` receipt does not establish which inner
stage exhausted the budget. Pure native contracts, all 389 checker unit tests
(serial), 110 strict qualification tests, Windows policy tests and CLI/checker
Clippy pass. An earlier complete gate failed in synthetic process-fixture
timing/exit assertions while edits and focused builds were concurrent; the
serial checker rerun passes every assertion. A clean complete gate on the
committed tree remains required.

The clean complete gate on `3e369e31` passes locally; hosted quality-only
[37159909557](https://github.com/DavidLMS/nan-harness/actions/runs/37159909557)
also passes. The preceding parallel checker run exposed synthetic transport
timing failures; the unchanged serial suite passes. Native measurements remain
separate from these quality checks.

[Claude Windows 37159781018](https://github.com/DavidLMS/nan-harness/actions/runs/37159781018)
observes a complete 107-node owned tree, one classic editor, one Start task
control and a unique current Chat button with Cowork absent. This establishes
the supported Chat-only configuration. Current-mode classification now accepts
that source-supported shape while retaining historical conservative `missing`
receipts. Optional passive capability facts report Value/Invoke support,
read-only/empty state, password and keyboard-focus properties after two equal
retained-control reads and attachment/ownership guards. They authorize no input.

[Claude macOS 37159782030](https://github.com/DavidLMS/nan-harness/actions/runs/37159782030)
verifies submitted/copied responses in two sessions, then fails Read; a third
session exhausts its native deadline. The owned MCP offer diagnostic was absent
because it incorrectly bound the fixture to the checker worker's current
directory. It now binds the canonical private fixture parent, with file
identity, permissions, size and script checks preserved. Read error-envelope
diagnostics and native deadline-phase labels are closed enums; neither changes
the marker oracle or budgets. No session is newly qualified.

[Codex Linux 37159907401](https://github.com/DavidLMS/nan-harness/actions/runs/37159907401)
still observes an unmatched nonempty DialogTitle against the 168-title catalog.
[Codex Windows 37159908338](https://github.com/DavidLMS/nan-harness/actions/runs/37159908338)
now measures an unmatched title before role binding in one session; other
sessions reject query/deadline. All these native runs pass cleanup and emit zero
invalid diagnostic events. Exact public bundle inspection expands the catalogs
to Linux 186, macOS 187 and Windows 192 source-bound fixed titles. Source coverage
does not establish the identity or actionability of an observed dialog.

The next Linux Claude startup trial explicitly disables Cowork through the
exact official `2.9939.4` managed configuration. It requires the admitted release
and artifact, GitHub-hosted Linux startup mode, and canonical private checker
profile roots; defaults remain unchanged. This trial adds no GUI actions.

The clean complete local gate on `fd94e1a5` passes, including 393 checker tests.
[Claude macOS 37161281615](https://github.com/DavidLMS/nan-harness/actions/runs/37161281615)
proves one exact owned MCP offer in the provider inventory. Built-in Read still
returns a completed error (`plain-other` envelope); one other native session
expires in the input phase. The next trial preferentially selects only the
unique source-known fixture offer with its exact owned-path schema, under the
existing hosted fixture policy. Duplicate offers in a toolset, schema drift and
latest-toolset absence reject selection. Other harnesses retain their existing
selector. The same independent tool-result marker remains mandatory.

Native macOS input now waits passively for the retained composer's exact pasted
value before issuing readback keys. It keeps the original deadline, focus,
identity and window guards and never repeats paste or an uncertain action.
Closed paste/readback deadline phases distinguish the two boundaries.

[Claude Linux 37161283473](https://github.com/DavidLMS/nan-harness/actions/runs/37161283473)
exits in the child CLI before a window appears. Source review confirms the
Chat-only guard requires private `profile/nanh` before the CLI applies its
configuration, but the checker did not prepare that root. The admitted trial
now precreates it privately; existing insecure or redirected state is rejected.
[Claude Windows 37161282371](https://github.com/DavidLMS/nan-harness/actions/runs/37161282371)
proves current Chat, writable ValuePattern, keyboard focus, non-password editor
and Start task InvokePattern. Initial value is nonempty, so a future controller
must use the explicitly owned replacement contract. A second startup rejects
foreground change. Cleanup passes for both Claude runs; no cell qualifies.

[Codex 37161284248](https://github.com/DavidLMS/nan-harness/actions/runs/37161284248)
still rejects macOS passive scope and measures unknown nonempty Linux/Windows
titles. Linux's second probe also fails parent journal sealing with an I/O
error. Optional closed scope, I/O-kind and seal-operation facts now distinguish
these boundaries without changing acceptance or cleanup. The Linux catalog
also includes one fixed title from the separately verified public Radix Title
wrapper (187 total); no observed dialog identity is inferred.

The next Zed Linux recovery trial uses official private startup settings:
right dock, fixed 960px width, flexible sizing off and content-width cap off.
The exact pinned source places the Retry callout outside the conversation
scroller and clips its horizontal content. A wider panel is a layout trial,
not proof of the preceding click failure. It adds no zoom activation or Retry
replay and preserves the existing response/tool/Resume oracle.

The source-bound Claude macOS recovery selector uses the modern error card's
ordinary **Try again** button. In the frozen 2.19675.0 renderer,
`cd5a31703-DiwdunLT.js` (`87e6b710a540352fcd4f9a1f0f6a8f9f9b6377ca676fd99c3e4d8bc87653dceb`)
passes `onRetryLastTurn` from `Uqe` through `$q` to the `Bw` action builder,
without a retry-label override. That builder is export `a` / function `on`
in `c3e34355f-BCwspPRT.js`
(`99571dee5d72e9985b0fec05f14aefc98b68bc379569b9739ca4f62f86a4d513`),
and defaults to the fixed English message `FazwRldA7z`, **Try again**.
The selector does not admit a generic **Retry** as an alternative. The
existing failed-prompt, failure-marker, unique-control, native ownership and
single-attempt requirements remain in force. Advisory row-shape retry counts
now count this exact source control; absent row anchors remain unresolved.
A classic composer label alone does not prove which transcript/error renderer
is active, and this correction does not establish successful recovery.

The October 4 macOS candidate also supports a separate causal recovery mode:
it requires two prior UI-verified turns with one unique instruction context per
turn and the same routed model, an exact match to the latest verified context, a
unique third user prompt, one consumed request-specific provider failure,
a clean Chat before Send, and two same-call proofs of the current error group
and retained **Try again** control. Legacy marker-based admission remains
unchanged. The current hosted result refuses to arm that capability; closed
context/history/stream diagnostics are advisory and cannot qualify recovery.
The main qualification workflow selects the same isolated Codex and Claude
conversation paths as the scoped feasibility runs. Final full-matrix acceptance
on a converged commit remains pending.


### Claude managed fixture failure request authority

The hosted macOS managed `read-only` MCP fixture may bind the controlled failure to the exact bounded OpenAI tool array from the second independently UI- and file-tool-verified main request. This alternative is enabled only after `FixtureRead`, the real tool result and copied fixture response pass, under the existing native-known-folders disposable profile policy. Default callers retain exact latest instruction-context matching.

The admitted failure still requires streaming, the unchanged routed model and full exact three-user history with the private failure nonce only in the final user message. The tool array must contain one valid `mcp__nanh-read-fixture__read_file` definition and remain byte-equivalent after JSON serialization; missing, duplicate or changed tool definitions and ambiguous second-turn advertisements reject. The third instruction context must remain valid and bounded. Tool hashes, prompts and instructions stay private; a matching hash alone grants no authority. Failure injection remains consumed once per request epoch.

Frozen Claude 2.19675.0 source proof: native title generation `JNr/YNr/ZNr` uses one template user and no tools; fallback `ePr` explicitly supplies `--tools ""`. Compaction uses one wrapped transcript user and only `summarize_conversation`. Connector auto-review can advertise tools, but creates a separate temporary conversation with one generated user prompt; it does not submit the exact three main user messages. These are fixed source exclusions, not title-keyword heuristics or instruction/date normalization.

Public source hashes:

__.vite__build__index.chunk-BZdcw7TE.js SHA256 0e794998a1ad175f651818913cba690de0724d92d7a946efc58ae0f08c7df226
__.vite__build__index.chunk-oULTE0fJ.js SHA256 244dd4f5df73f3673422b8f0a06e6e74bec6064cc8832811da830fb929263160
shared-9-mvoGrP5v.js SHA256 0e9e6ba098b3f72b8c2883dea74cbc67835b26862d2e89e5e6ef202e2458bcc9
cd9350303-DEt9YaG0.js SHA256 622887bb47c413ea73e0b606678fe9760baeb4287c79d604b2ca5f971492a16d
cf07a5c93-YV5wvunt.js SHA256 940d4b603ef691b327eaf32ff4f7e62145fd3fdbedd496afae95268cce91fa59
c121d00d7-DNoZTY8y.js SHA256 1d5fc2aa282f32f7866bcde710c320194b76e8cdfb7394d47859ffbc8257f8ef

### Grouped twelve-cell qualification candidate

[Campaign 37230585560](https://github.com/DavidLMS/nan-harness/actions/runs/37230585560)
tested the six unresolved cells at `9546169c3f738e2c5fe405abc2d125ec90003f4b`.
All native builds succeeded; none of the six cells qualified. Earlier passes
for Hermes on all three platforms, Zed macOS/Windows and Claude macOS do not
establish a complete matrix on this commit.

The next candidate groups the following corrections and closed observations:

| Cell | Observed boundary | Candidate change |
| --- | --- | --- |
| Zed Linux | Response and real file tool pass; recovery does not | Match actual module-only log tags and calibrate passive events against a completed prior turn before interpreting Retry counts |
| Codex Linux | Real command fails with sandbox hint; recovery response is not correlated | Install scoped distribution bubblewrap/AppArmor prerequisites and verify namespaces before launch; correlate the new empty Retry turn |
| Codex macOS | Recovery reports ownership loss or uncertain action | Retain the original ownership checks and bind the new Retry turn to the retained failed user and conversation |
| Codex Windows | Source dialog/folder trust ownership guards exhaust acquisition | Prove launcher and listener ancestry in one fresh native transaction with identity and listener rechecks |
| Claude Linux | Second-turn tree identity/owner query fails | Record the fixed D-Bus error category and operation without exporting native messages or references |
| Claude Windows | Real Read fails; failure control scope is absent | Record separate total and prompt-scoped Retry/details counts from the already collected owned tree |

Codex capacity Retry starts a new empty turn in the pinned renderer. Recovery
therefore requires a new assistant source unit after the retained failed user,
the same document and conversation, no intervening user, and the exact recovery
nonce. Ordinary response verification and provider verification remain required;
diagnostic observations never grant action authority or qualify a failed tool.

Run the grouped candidate with `app=all`, `platform=all`,
`experiment=deterministic-full`, and `native_only=false`. This excludes Pen,
keeps independent native jobs running after other failures, runs the integration
gate, and aggregates all twelve cells against one source commit. Native success
and the final integration gate remain unproven until that campaign completes.

### Twelve-cell campaign 37232180513

[The complete campaign](https://github.com/DavidLMS/nan-harness/actions/runs/37232180513)
at `3a0e7281e36e1d5926da41bbc3a28daa05236ff3` produced all twelve
qualification reports. The integration-quality job passed. Six native cells
qualified, each in three independent sessions; the aggregate correctly remains
incomplete. App and global cleanup passed in every cell.

| Application | Linux x64 | macOS ARM64 | Windows x64 |
| --- | --- | --- | --- |
| Zed | Recovery fails | Passed | Initial clipboard acquisition / recovery selector fails |
| Codex | Passed | Passed | Onboarding acquisition fails |
| Claude | Second-turn accessibility queries fail | Passed | File Read fails; diagnostic transport rejects receipt |
| Hermes | Passed | Two sessions pass; one recovery fails | Passed |

The Codex Linux sandbox prerequisite and new-empty-turn Retry correlation now
pass real tool and recovery acceptance. The same Retry correlation passes on
macOS. Windows reaches folder trust, completing that action in one session,
but never qualifies the initial conversation.

Zed Linux records a completed prior turn in all three passive log baselines,
then zero session/turn events after Retry. This calibrates the observer and
narrows the unresolved boundary to activation or the path before native resume;
it does not establish which of those failed. Claude Linux reports D-Bus
`NoReply` from identity, owner and children queries after the first response.

Claude Windows exposes a checker integration defect: the new optional
`failure-scope` line is accepted by the receipt parser but rejected earlier by
the single-line process transport. Its real Read tool also remains unsuccessful.
Hermes macOS reports a detached retained Retry button after the owned onboarding
dismissal in its failed session. Neither failure is accepted as a pass or hidden
by a rerun. Correct and test the grouped candidate before another campaign.

The next Hermes candidate permits one passive replacement of a detached Retry
handle only after a completed owned onboarding dismissal. It retains the same
document and original settling deadline, revalidates the unique failed user and
error-scoped control, and requires fresh stable hit samples before its single
Retry click. It rejects a changed document, changed failed turn or another
detachment. This candidate has synthetic coverage; native confirmation is pending.

Claude Linux's next-turn candidate removes two redundant full-history walks
during passive preparation. The two complete snapshots bracketing editor
admission remain, as do local custody checks and complete history verification
immediately before focus, paste and Send. A regression test changes history
after admission and requires rejection before any focus or input. Native timing
and second-turn completion remain unproven.

Codex Windows now receives an absolute observer cutoff derived from the
supervisor's original readiness clock, with five seconds reserved for the closed
receipt and process exit. Node startup no longer starts an independent 120-second
allowance that can outlive its parent. Passive role-binding samples save the
existing scope/focus/count observations before another ownership check. The
clock mismatch is established in source; its contribution to the campaign's
onboarding failures still requires native evidence.

The Windows Zed recovery receipt has a second-query ambiguity: readiness can
find one named Retry, while dispatch later finds zero and leaves the readiness
substage unchanged. Dispatch now performs the existing `retry-revalidate`
custody guard before this query. A zero count at that stage means loss between
readiness and dispatch, not failure of initial discovery. This diagnostic
correction does not establish a successful Retry action.

The Windows named-control candidate now retains the original readiness cutoff
and passively waits for the same unique visible semantic control before its one
click. Changed owner, geometry, role, name, description or optional automation
ID rejects the action. GPUI's ordinary element ID does not become a UIA
AutomationId, so an absent AutomationId remains valid, as in the existing
selector contract. Synthetic coverage includes that source-defined absence,
temporary disappearance, replacement, duplicates, lost ownership and expiry.
Other platforms and the tooltip discovery path keep their existing behavior.

### Claude Windows managed read fixture candidate

Static inspection of the official 2.19675.0 MSIX, whose SHA-256 exactly matches
campaign 37232180513, establishes a supported alternative to its failing built-in
Read tool. Desktop's `managedMcpServers` array accepts an HTTP entry with
`name: "nanh-read-fixture"`, `transport: "http"`, a loopback `url`, and
`toolPolicy: {"read_file": "allow"}`. With OAuth and helper fields omitted,
successful anonymous MCP initialization does not require an account. This is
source evidence, not a native qualification result. Do not forge
`trustedDelivery` or bypass a consent boundary.

Implement the bounded HTTP fixture in the checker lifecycle: start it after
creating the real read target and before launching Claude, pass its endpoint
only to the owned launcher, and stop it on every scenario exit. Retain private
Windows directory and file handles, reject reparse points, prevent write/delete
sharing on the file, and read its actual bytes on each accepted tool call. The
fixture must never receive the expected response nonce or mark any acceptance
oracle as satisfied. The existing provider and UI result checks remain mandatory.

The [MCP Streamable HTTP contract](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports)
permits JSON POST responses, empty 202 notification responses and 405 for an
unused GET stream. Bound request size, count and lifetime; validate Origin and
loopback authority; support the vendor's preliminary anonymous initialization
as well as its subsequent actual connection. Test the protocol and file custody
before enabling this candidate in the next grouped native campaign.

The candidate now implements that HTTP tool inside the checker worker. Its
random loopback endpoint is generated before launch, passed only to the owned
CLI, and checked against the retained applied configuration before UI input.
The runner rejects caller-supplied endpoints and mixed platform fixture flags.
Windows file handles deny writes/deletion; each tool call verifies the retained
private target and reads its bytes. An unavailable server rejects the scenario
instead of falling back to a different tool. Both provider and UI oracles still
have to verify the real result, followed by the existing failure/recovery test.

The server supports the vendor's probe and active initialization connections,
bounded JSON requests, 202 notifications and an optional GET returning 405. It
validates HTTP authority, Origin and protocol version, and has a 64-request and
worker-lifetime limit. Normal teardown explicitly drains the server after app
cleanup and releases its file handles; early failure cancels the in-process
listener. Local HTTP, file-custody and CLI-entry tests pass. The existing Windows
job additionally runs the Windows file-sharing and production configuration
write/restore fixtures before installation. Native MCP discovery, actual tool
execution and recovery on Windows remain pending the grouped campaign.

The grouped Linux Zed candidate also records `transientDialogsBeforeDispatch`
after the final hover proof and before its single click. This uses the existing
closed census schema, includes unmapped owned dialogs, preserves the earlier
and final observations, and rechecks the same owner and original cutoff. A
positive count supports the source-defined input-blocking hypothesis; zero does
not expose or rule out stale GPUI internal state. The census grants no input
authority. Synthetic driver, transient-census and report-reducer checks pass.

### Count-only Linux Zed recovery diagnostic

The `open-cells` feasibility campaign enables `NANH_ZED_RETRY_ENTRY_TRACE=1`
for Zed Linux only. It attaches uprobes to `ThreadView::retry_generation`,
`NativeAgentSessionRetry::run` and `Window::dispatch_event` in the exact inspected official 1.22.0 GUI
binary. The prepared CLI locates its sibling `libexec/zed-editor`; its SHA256
must also match. The entry/return counters read no application memory, stacks
or user data. Return probes classify only the verified boolean return registers.
The public receipt contains only bounded entry counts, lifecycle stage and
completion/cleanup status. Counts cover the three probe sessions together.
`inputDispatchEntries` provides an independent control for ordinary GUI event
activity; zero Retry entries without observed input activity cannot establish
where recovery stopped. Older receipts lacking this field remain readable.

The current diagnostic also records three activation intervals independently.
The owned pointer helper opens empty runner-owned start/end marker files around
its sole click and post-click observations. Fixed `openat` tracepoint filters
compare only those marker paths; paths are never exported. Each interval counts
GUI input, Retry, native retry and `ThreadView::clear_thread_error` entries. The
last counter can reveal error dismissal during an activation, but a zero count
cannot exclude compiler inlining. Missing or unmatched markers invalidate the
interval capture. A matched interval still measures a time window, not a causal
association between every counted event and the click. No tracing result changes
input authority or qualifies a cell.

The optional `NANH_ZED_HIT_GEOMETRY=1` diagnostic reads a bounded set of numeric
fields from the pinned GPUI window during the first dispatch in each activation
interval: viewport size, prior pointer position and at most 1,024 rendered
hitbox rectangles, content masks and behavior flags. It reads no text, object
identifiers, stacks or credentials. Numeric maps remain in the runner's private
tracer pipe; only closed counts and classifications enter the qualification
artifact. Raw numeric maps and addresses are never uploaded. The exact official
binary hash is mandatory because these field offsets are version-specific.

The owned pointer helper records its independently checked target in a private
marker directory before activation. The reducer uses the physical/logical
viewport ratio, requires a complete frame, and compares the accessible rectangle
with the rendered hitboxes. It distinguishes absent/ambiguous bounds, clipping
and blocking hitboxes ahead in paint order. A geometric match does not prove
element identity; these observations never authorize input. The pointer sample
precedes dispatch and is not claimed as the incoming event's coordinates.
Invalid, oversized or incomplete geometry remains unavailable.

The runner uses Ubuntu 24.04's bpftrace 0.20.2. Its source-defined
`__BPFTRACE_NOTIFY_PROBES_ATTACHED` notification establishes attachment readiness;
`BEGIN` runs too early and this version has no systemd readiness notification.
The helper requires that exact tool version and matches only the exact marker
on its private child pipe; other diagnostic text is discarded. Early exit and
incomplete output remain unavailable, never zero. Capture starts outside the
pointer helper's deadline and ends after the checker; failed cleanup fails the
driver. A bounded timer expires the tracer if its parent disappears.
See the [pinned bpftrace implementation](https://github.com/bpftrace/bpftrace/blob/v0.20.2/src/bpftrace.cpp).

Even otherwise successful probes with this receipt remain `unqualified`, with
reason `instrumented-diagnostic`. Run `platform=all` without the trace opt-in
for the final twelve-cell qualification. The final matrix excludes Pen.

### Same-commit campaign corrections (2026-10-05)

Campaign [37290009354](https://github.com/DavidLMS/nan-harness/actions/runs/37290009354)
on `4fbe72cb09f9b972f8f4186aaad081050da190b4` accepted nine cells, including
all Zed and Claude platforms, Hermes Linux/macOS and Codex Linux. Codex macOS
and Hermes Windows each completed two full sessions; their first sessions
did not qualify. The `not-started` summary alone does not establish whether
the application launched. Both cleanup scopes passed. Closed prelaunch
categories now distinguish process inspection, capability probing and profile
preparation without changing their admission or timing policies.

Codex Windows failed before installation because the official mutable MSIX
changed. The inspected replacement has package version `26.930.41038`, MSIX
identity version `26.930.4958.0`, and archive SHA256
`e03019134d729c6416173b0712aa5c51d079966253f77077f4bf105d29d8fce7`.
The executable and passive title catalog are pinned to those inspected bytes.
All 202 catalog entries retain verified source provenance, including the six
indirect descriptor, branding and finite plural bindings. Ordinary onboarding
Skip, retained home composer and semantic conversation markers remain required.
Native acceptance of this replacement remains pending.

Hermes Windows subsequently passed all three full sessions in
[37339322507](https://github.com/DavidLMS/nan-harness/actions/runs/37339322507).
Codex macOS passed all three in
[37357588636](https://github.com/DavidLMS/nan-harness/actions/runs/37357588636)
on `74a03d40d441587e8d76858be56230d894a55b81`, including real-tool verification,
UI recovery and both cleanup scopes. Its inventory had completed but the CDP
client's disconnect stalled. The observer now bounds that disconnect to two
seconds and exits its own process; the parent retains application/profile
custody and rechecks the completed inventory before starting a separate turn
controller. All three sessions exercised that bounded exit. An incomplete or
failed inventory cannot qualify through the disconnect fallback.

Codex Windows remains unqualified. In
[37359120776](https://github.com/DavidLMS/nan-harness/actions/runs/37359120776),
the parent-side diagnostic records `application-exited` at `process-custody`
in all three sessions, before attaching the observer. A later custody check
had obscured that initial failure with `isolation-unavailable`. Profile
preparation and both cleanup scopes passed. The checker now preserves the
original observer failure and captures the existing closed launcher diagnostic
before cleanup, as it already does for native acquisition failures. Eleven cells now
have full evidence across different commits; this is not a successful final
twelve-cell campaign.

Use `app=all`, `platform=final-corrections`, `experiment=deterministic-full`
and `native_only=true` to run only Codex macOS/Windows and Hermes Windows.
This diagnostic selection does not run the twelve-cell aggregate. After it
passes, the complete `platform=all` campaign must still pass on one commit.
The first local final gate hit five synthetic process timeouts; all 93 native
contracts passed when run serially. Use `RUST_TEST_THREADS=1` for the next full
local gate, preserving production deadlines. The hosted full Cargo gate passed;
its subsequent automation contracts exposed a stale diagnostic expectation,
which is corrected separately.

### Windows Codex release refresh after the startup corrections

Run `37408890481` at `bca2ec8a` stopped at official metadata freezing:
the mutable Windows download had changed before any qualification session.
The replacement was inspected statically: package version `26.930.51102`,
MSIX identity version `26.930.6422.0`, artifact SHA-256
`12070c9dd6cca622d043abdaf2225406abe6de19e8061024d93b93255478603e`,
and executable SHA-256
`669f7e6f49e4c3ac7fc02f830741df8353aca28a17c57069d399e55bedebcabb`.
Windows admission, installer policy, onboarding policy and the passive title
catalog now use this release together; Linux and macOS retain `26.930.41038`.
The cross-language policy contract checks the platform-specific versions.
The 202 title entries retain their source hashes, including refreshed wrapper
imports and the derived update, folder-consent and browser-import titles.
This refresh grants no new UI actions and is not runtime acceptance evidence.
The startup settlement and fixture-read corrections still require a complete
Windows run before the final twelve-cell, same-commit campaign.

Run `37410841843` at `27dc0ab0` admitted Windows Codex `26.930.51102`.
All three sessions verified response and explicit recovery; both cleanup scopes
passed. One session verified the fixture read, while two returned a running
unified-exec session before producing the file content. The deterministic
provider now follows that exact returned session with at most three empty
`write_stdin` calls. It requires an exit-zero envelope before completing the
script, preserves the existing turn deadline, and refuses missing tools,
changed sessions and exhausted polling. No session identifiers enter reports.
This continuation is opt-in for the desktop fixture-read scenario.

The second attempt of `37412936566` at `b5058dbd` completed two full sessions
and both cleanup scopes. The first session stopped during initial document
readiness, with an expired endpoint proof and no UI action. Windows cold
startup now shares the original 120-second qualification cutoff instead of
shorter 35-second startup and 10-second document clocks. This does not extend
the parent cutoff or permit input without fresh ownership and document proof.
The full local `cargo check-all` passed at `b5058dbd` before this timing change.

The qualification baseline freezes the inspected official Claude releases on
all three platforms: Linux `2.9939.4`, macOS and Windows `2.19675.0`. Immutable
vendor URLs and SHA-256 checks bind those downloads to the existing source
admission. The ordinary latest-version resolver still follows upstream releases;
a newer release requires inspection before joining this deterministic baseline.
Campaign `37415300226` exposed the mismatch between that admission and mutable
latest downloads. The previous official packages remain available and match
all three admitted artifact hashes.

The same campaign observed a macOS Codex ownership failure during document
readiness, while the following two sessions completed. Its separate 10-second
document cutoff could exhaust the proof budget before the total deadline. Every
Codex onboarding trial now shares its original total startup deadline across
document readiness and onboarding (60 seconds on Unix, 120 on Windows). No
ownership proof or acceptance step is skipped, and actions never renew the clock.

Hermes Windows in that campaign completed sessions two and three; session one
stopped before clicking its unique visible, enabled onboarding choice, with
ownership still verified. Its separate five-second readiness cutoff also included
native custody queries. Onboarding now shares the existing overall readiness
cutoff, retaining the same unique-control, stable-point and custody proofs.

Campaign `37416555321` observed one macOS Zed Retry control in the accessibility
count, followed by no matching element during immediate capture. No Retry was
dispatched in that session; the other two sessions completed, and cleanup passed.
macOS now uses the same bounded capture and retained-control revalidation already
used on Windows. Only absence may settle within the original deadline; ambiguity,
changed identity, failed custody and uncertain dispatch still fail immediately.

Campaign `37418781483` reproduced the same pre-dispatch Retry capture gap on
Linux Zed (two sessions complete, one absent candidate after a unique count).
The named Retry path now retains and revalidates its control on all platforms,
using the existing deadline and rejecting replacement or ambiguous candidates.

Campaign `37418781483` also measured a macOS Codex startup exhausting its
60-second clock during coding readiness: the retained guard alone took 38 seconds,
including 16 seconds of identity queries and 13 seconds of native proofs. The
macOS trial now allocates 90 seconds once, with 15 seconds of parent teardown
headroom. Incomplete binding/composer readiness remains a failed inventory and
cannot be published as complete before the semantic driver reads its binding.

The same run observed Hermes Windows first-run coverage disappearing before its
choice could be queried. Readiness now handles that no-action transition by
revalidating the original frame, editor, composer root and actionable model pill.
It neither clicks a missing choice nor accepts a replaced or covered composer.

Campaign `37421726262` completed setup on the revised Codex macOS and Hermes
Windows paths, but did not qualify the full matrix. Codex Retry admission now
waits read-only for the unique enabled control within its existing deadline,
reproving the exact failed turn and custody each time. Hermes checks the final
retained Retry identity after its already constrained onboarding-remount proof;
it no longer rejects that remount before the proof can run. Neither change
replays an attempted Retry. Codex restore diagnostics distinguish closed I/O
categories without changing restoration behavior or exposing paths/messages.

### Follow-up qualification after `ef2f0c48`

Runs `37430942957` and `37430946272` passed all three sessions and cleanup
for Codex Linux/macOS and Hermes Linux/macOS. Hermes Windows passed its third
session; its earlier startup attempts reported a frame-query rejection and an
unstable Refresh models button. The latter now waits for three stable samples
of the same retained, owned control before dispatch, within the original budget.
An uncertain click is never repeated. Zed Retry diagnostics now report only the
failed identity invariant, preserving strict action admission.

Codex Windows failed before installation because its moving official MSIX URL
changed. The replacement was statically inspected as application `26.930.61225`
(MSIX identity `26.930.7945.0`), artifact SHA-256
`0fcd11295dfd239ef8b6a2cb088a4ead18316b80a87e0c0e1abbad9d830edef3`,
executable SHA-256
`746fc8e491616076dad7ae3348bc73748d27ee8471dc589844390d08f4fba25d`.
The passive Windows dialog catalog retains only literals verified in this
package's renderer sources; the removed browser-import title is not admitted.
The deterministic workflow now caches the verified public MSIX before any GUI
work. Restored bytes must match the committed digest; symlinks and corrupt
cache entries fail closed. No credentials, profiles or GUI output enter that
cache. Cache eviction still requires the exact official package to be available.
These targeted results do not qualify the remaining cells or a full matrix.

Run `37433125564` passed Zed, Claude and Hermes Windows (three sessions and
both cleanups each); run `37433129018` passed Zed macOS. Codex Windows passed
sessions one and three with complete cleanup, but session two reported an
uncertain folder-trust click followed by the owned onboarding form. Playwright
1.61.1 waits for post-click navigation and hit-interceptor evaluation by default
([pinned implementation](https://github.com/microsoft/playwright/blob/v1.61.1/packages/playwright-core/src/server/dom.ts)).
Folder trust now opts out of that implicit post-click wait and independently
requires the retained dialog, form and button to detach within the existing
deadline, with directory and window custody re-proved. A remaining dialog, lost
custody or uncertain dispatch fails; input is never replayed. Synthetic tests
cover the successful transition, a remaining dialog and post-click custody loss.

### Windows readiness follow-up after the ten-cell pass

Run `37437734044` at `be3ca350` passed ten of twelve cells and the complete
integration-quality job. Codex and Zed passed all platforms. Claude Windows
passed two sessions, then rejected an ambiguous heading snapshot before Retry;
Hermes Windows rejected its first document query, then passed both subsequent
sessions. All cleanup checks passed. These failures keep the aggregate unqualified.

Claude's read-only Retry readiness loop may now reobserve heading ambiguity only
when the closed counts identify exactly one server error, failed user heading,
failed prompt, prompt group, Retry button and Retry label. The native action
continues to require its complete unambiguous scope; duplicated controls, lost
ownership and uncertain input remain terminal. The original deadline is unchanged.
Hermes now retains frame and loader identity while its initial page/frame reads
settle across the known root-fragment transition. Reloads and foreign routes
remain rejected. Closed query-phase diagnostics distinguish future ownership,
page-set and frame-query failures without exporting identifiers or URLs.

Run `37440542326` at `2aa29f7c` passed integration quality, Zed Windows and
Hermes Windows, including three sessions and both cleanups. Claude Windows
still rejected a unique-count heading snapshot before Invoke in one session;
its other two sessions passed. The failure-details and Retry request paths now
wait only on typed pre-Invoke scope/query receipts within their original
deadlines. An uncertain Invoke or transport result remains terminal. The closed
`nativeAction` diagnostic distinguishes these request paths without UI content.

Codex Windows passed two sessions; the first reported an uncertain Retry click
while the provider had already produced the recovery response. This does not
prove a successful UI recovery and remains a failed session. Its ordinary click
now opts out of Playwright's implicit post-click wait, as folder trust already
does. The existing owned-turn response loop must independently verify the
recovery; lost custody or uncertain dispatch fails without repeating input.
Synthetic tests cover post-click custody loss and a single uncertain dispatch.
