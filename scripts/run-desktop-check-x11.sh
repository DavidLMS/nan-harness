#!/usr/bin/env bash
# A virtual X server alone does not provide a window manager or focus policy.
set -euo pipefail
env -u NAN_API_KEY openbox --sm-disable >/dev/null 2>&1 &
window_manager_pid=$!
trap 'kill "$window_manager_pid" 2>/dev/null || true; wait "$window_manager_pid" 2>/dev/null || true' EXIT
for ((attempt = 0; attempt < 100; attempt++)); do
    kill -0 "$window_manager_pid" 2>/dev/null || break
    if [[ "$(xprop -root _NET_SUPPORTING_WM_CHECK)" == *"window id # 0x"* ]]; then
        "$@"
        exit "$?"
    fi
    sleep 0.1
done
echo "The disposable X11 window manager did not become ready." >&2
exit 1
