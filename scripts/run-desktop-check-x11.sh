#!/usr/bin/env bash
# A virtual X server alone does not provide a window manager or focus policy.
set -euo pipefail
window_manager_arguments=(--sm-disable)
configuration_directory=''
cleanup() {
    if [[ -n "${window_manager_pid:-}" ]]; then
        kill "$window_manager_pid" 2>/dev/null || true
        wait "$window_manager_pid" 2>/dev/null || true
    fi
    if [[ -n "$configuration_directory" ]]; then
        rm -f -- "$configuration_directory/rc.xml"
        rmdir -- "$configuration_directory"
    fi
}
trap cleanup EXIT
if [[ "${FEASIBILITY_ZED_MAXIMIZED:-0}" != 0 ]]; then
    [[ "$FEASIBILITY_ZED_MAXIMIZED" == 1 && "${GITHUB_ACTIONS:-}" == true && "${RUNNER_ENVIRONMENT:-}" == github-hosted && "${RUNNER_OS:-}" == Linux ]] || exit 1
    configuration_directory="$(mktemp -d "${TMPDIR:-/tmp}/nanh-zed-openbox.XXXXXXXX")"
    chmod 700 "$configuration_directory"
    script_directory="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
    python3 "$script_directory/desktop-feasibility/zed-openbox.py" /etc/xdg/openbox/rc.xml "$configuration_directory/rc.xml"
    window_manager_arguments+=(--config-file "$configuration_directory/rc.xml")
fi
env -u NAN_API_KEY openbox "${window_manager_arguments[@]}" >/dev/null 2>&1 &
window_manager_pid=$!

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
