#!/usr/bin/env bash
# Start a disposable graphical session around an already prepared checker.
set -euo pipefail
export ZED_EXPERIMENTAL_A11Y=1
# Hosted runners use software graphics; Zed documents this opt-in for its GPU
# warning. The application and checker still execute on the native CPU target.
export ZED_ALLOW_EMULATED_GPU=1
screen_policy="${NANH_ZED_SCREEN_POLICY:-}"
if [[ "${NANH_ZED_SCREEN_POLICY+x}" == x ]]; then
    [[ "$screen_policy" == height-1536 && "${GITHUB_ACTIONS:-}" == true \
        && "${RUNNER_ENVIRONMENT:-}" == github-hosted && "${RUNNER_OS:-}" == Linux \
        && "${FEASIBILITY_ZED_MAXIMIZED:-}" == 1 \
        && "${NANH_ZED_PANEL_LAYOUT:-}" == fixed-wide \
        && "${NANH_ZED_LAYOUT_POLICY+x}" != x && "${NANH_ZED_PANEL_ZOOM+x}" != x ]] || exit 1
    # Only this owned qualification entry can request a nondefault screen;
    # source/archive admission remains in the runner before any vendor launch.
    [[ $# -ge 6 && "$1" == python3 && "$2" == scripts/desktop-feasibility/run-qualification.py \
        && "$3" == --app && "$4" == zed-desktop && "$5" == --platform && "$6" == linux ]] || exit 1
fi
if [[ "${RUNNER_OS:-}" == Linux ]]; then
    export GTK_A11Y=always NO_AT_BRIDGE=0
    session_script="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/run-desktop-check-x11.sh"
    exec dbus-run-session -- bash -c '
        set -euo pipefail
        busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true
        if [[ "${NANH_ZED_SCREEN_POLICY:-}" == height-1536 ]]; then
            exec xvfb-run -a --server-args="-screen 0 1280x1536x24" bash "$@"
        fi
        exec xvfb-run -a bash "$@"
    ' -- "$session_script" "$@"
fi
exec "$@"
