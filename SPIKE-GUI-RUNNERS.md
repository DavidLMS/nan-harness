# Hosted GUI runner spike

Executed on 2026-09-08. This is a temporary experiment, not compatibility
certification for any nan-harness Desktop integration.

## Result

GitHub-hosted runners can execute real graphical accessibility tests. Windows
completed both Qt and Electron scenarios. Linux completed Qt; Electron started
after sandbox preparation but its text action failed. The overall matrix is
therefore **not green**.

| Runner | Qt widgets | Electron | Final job duration |
| --- | --- | --- | --- |
| ubuntu-24.04, x86_64 | 3/3 passed | 0/3 passed; text action unsupported | 49 seconds |
| windows-2025, AMD64 | 3/3 passed | 3/3 passed | 126 seconds |

Durations include dependency installation and artifact handling, exclude queue
time, and are single-run observations rather than performance guarantees.
The preceding Windows job took 79 seconds and also passed all six probes.

Each successful repetition launches a fresh process, discovers it through
xa11y, sets and reads a unique synthetic text value, invokes Send, reads the
expected resulting value, confirms a missing button times out, takes a PNG
screenshot, and terminates the process. Input uses accessibility set_value and
press, not a physical-keyboard or coordinate-click simulation.

## Reproducibility and evidence

- Branch: spike/gui-runners, isolated from the local main branch.
- Base: 19e69e02ceddbdc5e5f6857f5541ac8ebc990ac1.
- Tested commit: 0df7eb901d13ce2247ec5b8955d4253a11d882f5.
- Python 3.12; xa11y 0.13.0; PySide6 6.11.2; Electron 44.2.0.
- Linux image: 20260831.293.1; Xvfb, D-Bus, AT-SPI and Fluxbox are
  provisioned by the pinned xa11y/setup-a11y action.
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

The second correction sets root ownership and mode 4755 on the downloaded
Electron chrome-sandbox helper inside the disposable Linux runner. The spike
does not disable the Electron sandbox.

## Remaining limitation

Linux Electron reports ActionNotSupportedError: "Text value not supported for
this element". Its log contains ATK_IS_EDITABLE_TEXT assertions. The selector
currently matches name alone; the HTML has both label text and a named input.
A plausible explanation is that Linux resolves a non-editable named element.
This is a hypothesis, not a verified diagnosis: no accessibility-tree dump was
collected at that failure.

The next targeted experiment should inspect that tree and use a role-qualified
editable selector before considering another input mechanism. Do not conclude
that Linux runners or Electron automation are impossible from this failure.
The agreed two correction iterations have been used; no third correction was
launched.

## Scope and recommendation

Proceed with investigating a real nan-harness Desktop app on Windows and
resolve the Linux Electron selector/action boundary first. These results justify
using hosted runners as a candidate test environment, not enabling automatic
compatibility publication.

Not tested: Windows 11, Linux Wayland, macOS, any of the five supported Desktop
apps, login/onboarding, physical input, provider traffic, tools, or recovery of
nan-harness configuration. The test apps deliberately expose accessible controls
and Electron enables renderer accessibility.

No NaN credentials, accounts, model calls, release changes, compatibility-feed
writes, main-branch pushes, PRs, or recurring schedules were used. Only the
temporary branch was pushed. Workflow permissions are contents:read.

The experiment is intentionally retained for reproduction and is not merged
into main. Its workflow runs only for changes to its own scripts/workflow on
the spike branch; this report alone does not trigger another run.
