# Hosted GUI runner spike

Executed on 2026-09-08. This is a temporary experiment, not compatibility
certification for any nan-harness Desktop integration.

## Result

GitHub-hosted runners can execute real graphical tests on Linux, Windows and
macOS. The final matrix is **green: 18/18 fresh-process probes passed**.
Linux Electron requires simulated keyboard input; its accessibility set_value
operation remains unsupported. A passing GUI scenario is not a claim that all
accessibility actions work identically across platforms.

| Runner | Qt widgets | Electron | Final job duration |
| --- | --- | --- | --- |
| ubuntu-24.04, x86_64 | 3/3 passed | 3/3 passed; keyboard fallback | 57 seconds |
| windows-2025, AMD64 | 3/3 passed | 3/3 passed | 79 seconds |
| macos-15, arm64 | 3/3 passed | 3/3 passed | 61 seconds |

Durations include dependency installation and artifact handling, exclude queue
time, and are single-run observations rather than performance guarantees.

Each successful repetition launches a fresh process, discovers it through
xa11y, sets and reads a unique synthetic text value, invokes Send, reads the
expected resulting value, confirms a missing button times out, takes a PNG
screenshot, and terminates the process. Input uses accessibility set_value
except Linux Electron, which uses accessibility focus plus simulated keyboard
input. All button activation and value verification use accessibility APIs.

## Reproducibility and evidence

- Branch: spike/gui-runners, isolated from the local main branch.
- Base: 19e69e02ceddbdc5e5f6857f5541ac8ebc990ac1.
- Tested commit: 17d5c190c5673a2b7b2db9051b205fd029348a49.
- Python 3.12; xa11y 0.13.0; PySide6 6.11.2; Electron 44.2.0.
- Linux image: 20260831.293.1; Xvfb, D-Bus, AT-SPI and Fluxbox are
  provisioned by the pinned xa11y/setup-a11y action.
- macOS 15.7.9 image: 20260829.0321.1; the same pinned action grants
  Accessibility permission to the setup-python interpreter in the disposable VM.
- Windows Server 2025 image: 20260824.214.3.
- The workflow and fixtures live in .github/workflows/gui-runner-spike.yml
  and experiments/gui-runner-spike/. npm dependencies have a committed lockfile.
- Each job has a 20-minute cap. Each probe subprocess has a 90-second cap.
- Results, synthetic-app logs and desktop screenshots are attached to each run
  with seven-day retention. Windows screenshots also show the disposable
  runner's background console; these are not user desktops.

Runs:

1. [Initial run](https://github.com/DavidLMS/nan-harness/actions/runs/34206590919):
   Qt passed on both systems. Electron's executable had not been downloaded.
2. [Explicit Electron download](https://github.com/DavidLMS/nan-harness/actions/runs/34206789646):
   Windows passed both apps. Linux Electron refused to launch because its
   packaged sandbox helper lacked the required ownership/mode.
3. [Configured Linux sandbox helper](https://github.com/DavidLMS/nan-harness/actions/runs/34207158805):
   Windows passed both apps again. Linux Qt passed; Electron launched but all
   three text actions failed.
4. [Role-qualified selectors and tree diagnostics](https://github.com/DavidLMS/nan-harness/actions/runs/34207744905):
   Linux Electron still rejected set_value on the single correct text field.
   This ruled out the suspected label/input name collision.
5. [macOS and Linux keyboard input](https://github.com/DavidLMS/nan-harness/actions/runs/34207946626):
   macOS Qt passed 3/3; Electron fields had empty names. Linux Electron passed
   2/3 using keyboard input; immediate value verification failed once.
6. [Final three-platform run](https://github.com/DavidLMS/nan-harness/actions/runs/34208169432):
   all 18 probes passed after matching native descriptions and explicitly
   waiting for focus and the expected input value.

The second correction sets root ownership and mode 4755 on the downloaded
Electron chrome-sandbox helper inside the disposable Linux runner. The spike
does not disable the Electron sandbox.

## Diagnosed platform differences

Linux Electron reports ActionNotSupportedError: "Text value not supported for
this element" on the correct text_field. Its log contains ATK_IS_EDITABLE_TEXT
assertions. The fallback is restricted to this app/platform and exception,
records setValueError and inputMode, focuses the field, waits for focus, types
through InputSim, and waits for the exact value. It does not suppress a failed
button action or incorrect result. The underlying EditableText support remains
unfixed; this experiment establishes a working GUI input alternative.

macOS Electron exposes the input labels in AXDescription while AXTitle and
xa11y's normalized name are empty. Recorded native attributes confirm this.
Selectors now accept name or description within the text_field role, and still
require exactly one match. Qt and Windows retain their working name mapping.
The user authorized further Ubuntu debugging and then adding macOS after the
original two-correction experiment.

## Scope and recommendation

Proceed with investigating a real nan-harness Desktop app on each platform.
These results justify
using hosted runners as a candidate test environment, not enabling automatic
compatibility publication.

Not tested: Windows 11, Linux Wayland, Intel macOS, any of the five supported Desktop
apps, login/onboarding, general keyboard/pointer behavior, provider traffic, tools, or recovery of
nan-harness configuration. The test apps deliberately expose accessible controls
and Electron enables renderer accessibility.

No NaN credentials, accounts, model calls, release changes, compatibility-feed
writes, main-branch pushes, PRs, or recurring schedules were used. Only the
temporary branch was pushed. Workflow permissions are contents:read.

The experiment is intentionally retained for reproduction and is not merged
into main. Its workflow runs only for changes to its own scripts/workflow on
the spike branch; this report alone does not trigger another run.
