#!/usr/bin/env bash
# Start a disposable graphical session around an already prepared checker.
set -euo pipefail
export ZED_EXPERIMENTAL_A11Y=1
# Hosted runners use software graphics; Zed documents this opt-in for its GPU
# warning. The application and checker still execute on the native CPU target.
export ZED_ALLOW_EMULATED_GPU=1
if [[ "${RUNNER_OS:-}" == Linux ]]; then
    export GTK_A11Y=always NO_AT_BRIDGE=0
    session_script="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/run-desktop-check-x11.sh"
    exec dbus-run-session -- bash -c '
        set -euo pipefail
        busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true
        exec xvfb-run -a bash "$@"
    ' -- "$session_script" "$@"
fi
exec "$@"
