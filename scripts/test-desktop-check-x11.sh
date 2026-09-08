#!/usr/bin/env bash
# The contract owns a new X server; it never changes the caller's desktop.
set -euo pipefail
helper=$(realpath -- "$1")
source_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
fixture_directory=$(mktemp -d)
fixture="$fixture_directory/stale-focus"
trap 'rm -f -- "$fixture"; rmdir -- "$fixture_directory"' EXIT
c++ -std=c++17 "$source_root/fixtures/desktop-stale-focus.cpp" -lX11 -o "$fixture"
# Preserve root properties when the fixture disconnects from this private server.
xvfb-run -a -s '-screen 0 800x600x24 -noreset' bash -c '
    set -euo pipefail
    "$1"
    if timeout 15s "$2" --windows; then
        echo "A stale foreground window must not certify input or capture." >&2
        exit 1
    else
        [[ $? == 5 ]]
    fi
    inventory=$(timeout 15s "$2" --windows-absence)
    [[ "$inventory" == $'"'"'FG 0 0\nDISPLAY 0 0 800 600'"'"' ]]
    if DISPLAY=invalid-display timeout 15s "$2" --windows-absence; then
        echo "An unavailable display must not certify absence." >&2
        exit 1
    else
        [[ $? == 5 ]]
    fi
' -- "$fixture" "$helper"
