# Offline visual helper

The checker first uses xa11y's accessibility tree. When an application does not
expose the required controls or response text, xa11y supplies input and an owned
window capture; the bundled helper recognizes English text locally with
Tesseract 5.5.2. No image, recognized text or prompt is sent to an OCR service or
saved as a diagnostic artifact.

`build.py` downloads digest-pinned Tesseract, Leptonica and `tessdata_fast` English
inputs. It builds static OCR libraries and embeds the resulting native helper and
model into the Rust executable. The first build requires network access; verified
downloads are reused inside Cargo's output directory. Build failures do not fall
back to a system OCR installation or an unpinned model.

Build on the target operating system and architecture, using Python 3, CMake and
a C++17 toolchain. Linux additionally needs X11 and xa11y's libxkbcommon development
packages. macOS uses the Apple SDK; Windows uses MSVC. Cross-compilation is
deliberately rejected. `NAN_DESKTOP_CMAKE` and `NAN_DESKTOP_BUILD_PYTHON` select
local build tools when they are not on PATH. Neither is a runtime dependency of
the published checker.

Runtime extraction uses a private temporary directory. The helper receives only
bounded RGBA pixels through stdin and returns bounded TSV through stdout. Its
environment excludes provider credentials. Window metadata contains process IDs,
window IDs, bounds and process names, not window titles. Capture requires an
owned foreground window, stable geometry and scale, one containing display and
no overlapping window above it. Headless sessions and Wayland without a usable
X11 window inventory fail closed. macOS may require both accessibility and screen
recording permissions; the checker does not grant them.

The hosted Claude macOS native-folder preflight uses a separate
`--claude-known-folders` mode. It compares Foundation's user home and Application
Support directories with the original managed launch HOME and emits only
`true\n` or `false\n`. It does not initialize AppKit, inspect applications,
capture pixels or run OCR. The caller clears the helper environment, supplies
only HOME, rejects any other output and retains the five-second query deadline.

On X11 the helper reads focus, stacking, attributes and ownership inside one
[server grab](https://www.x.org/releases/X11R7.7/doc/xproto/x11protocol.html),
so windows destroyed by other clients cannot make a snapshot partial. The grab is
bounded by a fixed query budget and a two-second timer that exits the helper;
the protocol releases a grab when its connection closes. If that timer or its
signal handler cannot be armed, the helper reports an unavailable inventory
without grabbing. Output and process-name
reads happen after release. Any X error inside the grab still discards the whole
snapshot with a closed exit category.

An X11 window manager may retain a destroyed `_NET_ACTIVE_WINDOW` ID. Inside the
grab that hint is verified; when its window no longer exists, the helper reports
the server's input focus, which reverts when its window stops being viewable,
attributed to its top-level window. Focus on no owned window still rejects
input/capture. Absence verification uses complete window enumeration without
querying focus and returns only windows, not a guard-capable snapshot.
`scripts/test-desktop-check-x11.sh` verifies stale focus, focus fallback,
concurrent window churn and unavailable-display rejection inside a fresh Xvfb
server.

On an explicitly authorized hosted Windows session, the checker fits its newly
launched foreground window inside the monitor work area if the default bounds
extend beyond it. Launch ownership is checked before the request; the helper
rechecks the window ID, process ID and foreground identity before resizing.
It does not activate another app or change stacking order. The checker then
acquires stable bounds again and retains every capture/input guard. This uses
the documented [SetWindowPos flags](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos).

Hosted Claude startup may first request foreground activation once when the
unique launch-owned window is behind the runner. The helper rechecks its HWND,
PID, top-level ownership, visibility, enabled state and absence of an active
popup before calling
[SetForegroundWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow).
Windows may deny the request; denial stops acquisition. A successful request
still requires a fresh stable snapshot and the existing focus, containment and
occlusion guards. Later window identity changes or focus losses cannot trigger
another activation attempt.
Closed failure stages distinguish window eligibility, an OS activation denial,
and a failed postcondition without exporting window identities.

The disposable hosted Windows Claude qualification runner temporarily sets the
foreground lock timeout to zero before launching its three sessions. This uses
`SystemParametersInfoW` with flags zero, without persisting a user setting. It
reads back preparation and restores and verifies the original value in a
`finally` block, including when qualification fails. A closed
`windows-foreground-session` receipt prevents acceptance if preparation or
restoration is incomplete. This runner-only setup does not change the native
checker's single activation attempt or its ownership, focus and occlusion
checks. Local and self-hosted desktops cannot enter this preparation path.

Before a subsequent Windows Claude prompt, an exact single composer with no
Send or Start button produces `composer-send-pending`. This is a read-only
observation before any input; the supervisor may repeat it within the original
deadline. Ambiguous controls and uncertain input delivery remain terminal.

Windows clipboard modes use `CF_UNICODETEXT` directly through User32, with a
private message-only owner window and one `OpenClipboard` attempt. Writes accept
at most 1 KiB of valid UTF-8 without embedded NULs; reads accept at most 64 KiB
of UTF-8 after a bounded UTF-16 scan. Private pipe and conversion buffers are
wiped. A successful write transfers its movable allocation to Windows; the
checker verifies readback and clears the clipboard before ending the probe.
These modes do not capture pixels or initialize OCR. A separately selected
disposable Windows contract checks Unicode roundtrip, invalid-input rejection
without replacing existing data, and verified clear before launching Zed.

The helper is limited to 16 megapixels per request, 512 KiB of output and a
15-second process deadline. Low-confidence or ambiguous text is not accepted as
a control. Response verification also requires a cleared composer; provider and
workspace evidence remain independently required for tool checks.

The fresh Zed profile uses 18-pixel agent responses and 16-pixel message text
through its native [agent font settings](https://zed.dev/docs/visual-customization#agent-panel).
This changes only the disposable profile, not the user's typography or the OCR
confidence, exact-match, entropy and visible-transcript requirements.

`THIRD_PARTY_NOTICES.txt` is embedded and available through
`nanh-desktop-check licenses`. The synthetic-pixel OCR test checks the shipped
helper/model together, independently of an installed GUI or system OCR package.

Windows Claude recovery accepts the exact public button labels `Retry` and
`Try again`. In the official 2.19675.0 MSIX, the renderer action builder in
`app/resources/ion-dist/assets/v1/c3e34355f-BCwspPRT.js`
(SHA-256 `99571dee5d72e9985b0fec05f14aefc98b68bc379569b9739ca4f62f86a4d513`)
uses `Try again` for its ordinary `onRetry` action. These renderer resources are
outside `app.asar`. The adapter requires one matching button in the independently
proved failed-turn scope and rechecks its exact name and retained identity before
Invoke. Both labels appearing together are ambiguous; model-switching and
purchase-retry controls are excluded. Retry diagnostic counts include both exact
labels, while action authority continues to require the Button role.

The official conversation renderer places user and assistant content in separate
message rows (`cd5a31703-DiwdunLT.js`, SHA-256
`87e6b710a540352fcd4f9a1f0f6a8f9f9b6377ca676fd99c3e4d8bc87653dceb`).
For this layout, recovery requires a unique exact user heading and prompt text
in one branch, followed by the unique failure marker and Retry in another branch
of the same non-boundary Group. Later user headings, unrelated reply branches,
duplicate controls and crossed Document/Pane/Window boundaries are rejected.
The failed user heading joins the control, failure anchor and shared ancestor
in the retained identity checks immediately before Invoke. Earlier conversation
headings do not make the final failed turn ambiguous by themselves.
