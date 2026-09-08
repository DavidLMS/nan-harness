#!/usr/bin/env bash
# Start a disposable graphical session around an already prepared checker.
set -euo pipefail
export ZED_EXPERIMENTAL_A11Y=1
if [[ "${RUNNER_OS:-}" == Linux ]]; then
    export GTK_A11Y=always NO_AT_BRIDGE=0
    exec dbus-run-session -- bash -c '
        set -euo pipefail
        busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true
        exec xvfb-run -a "$@"
    ' -- "$@"
fi
exec "$@"
