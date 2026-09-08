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

On an explicitly authorized hosted Windows session, the checker fits its newly
launched foreground window inside the monitor work area if the default bounds
extend beyond it. Launch ownership is checked before the request; the helper
rechecks the window ID, process ID and foreground identity before resizing.
It does not activate another app or change stacking order. The checker then
acquires stable bounds again and retains every capture/input guard. This uses
the documented [SetWindowPos flags](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos).

The helper is limited to 16 megapixels per request, 512 KiB of output and a
15-second process deadline. Low-confidence or ambiguous text is not accepted as
a control. Response verification also requires a cleared composer; provider and
workspace evidence remain independently required for tool checks.

`THIRD_PARTY_NOTICES.txt` is embedded and available through
`nanh-desktop-check licenses`. The synthetic-pixel OCR test checks the shipped
helper/model together, independently of an installed GUI or system OCR package.
