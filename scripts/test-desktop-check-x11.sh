#!/usr/bin/env bash
# The contract owns a new X server; it never changes the caller's desktop.
set -euo pipefail
helper=$(realpath -- "$1")
source_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
fixture_directory=$(mktemp -d)
fixture="$fixture_directory/stale-focus"
arming="$fixture_directory/grab-arming"
trap 'rm -f -- "$fixture" "$arming"; rmdir -- "$fixture_directory"' EXIT
c++ -std=c++17 "$source_root/fixtures/desktop-stale-focus.cpp" -lX11 -o "$fixture"
# Signal or timer arming failures must reject the inventory before any server grab.
c++ -std=c++17 -I "$source_root/../crates/nan-harness-desktop-check/native" \
    "$source_root/fixtures/desktop-x11-grab-arming.cpp" -o "$arming"
"$arming"
# Preserve root properties when the fixture disconnects from this private server.
xvfb-run -a -s '-screen 0 800x600x24 -noreset' bash -c '
    set -euo pipefail
    nl=$(printf "\nx")
    nl=${nl%x}
    "$1"
    # A destroyed active-window hint falls back to exact server focus. Here that is
    # PointerRoot, so the complete snapshot names no owner and cannot certify input.
    inventory=$(timeout 15s "$2" --windows)
    [[ "$inventory" == "FG 0 1${nl}DISPLAY 0 0 800 600" ]]
    inventory=$(timeout 15s "$2" --windows-absence)
    [[ "$inventory" == "FG 0 0${nl}DISPLAY 0 0 800 600" ]]
    if DISPLAY=invalid-display timeout 15s "$2" --windows-absence; then
        echo "An unavailable display must not certify absence." >&2
        exit 1
    else
        [[ $? == 5 ]]
    fi
    # Server focus inside a retained window attributes its root child, not the stale hint.
    "$1" focused
    inventory=$(timeout 15s "$2" --windows)
    [[ "${inventory%%"$nl"*}" =~ ^FG\ 4242\ ([0-9]+)$ ]]
    [[ "$inventory" == *"${nl}WIN ${BASH_REMATCH[1]} 4242 10.000 20.000 300.000 200.000 "* ]]
    # Windows destroyed around the helper never produce a partial or changed snapshot.
    timeout 60s "$1" churn &
    churn_pid=$!
    for _ in $(seq 50); do
        inventory=$(timeout 15s "$2" --windows)
        [[ "$inventory" == "FG 4242 "* ]]
    done
    kill "$churn_pid"
    wait "$churn_pid" || true
' -- "$fixture" "$helper"
