#!/usr/bin/env bash
# Stage a validated closed diagnostic envelope; no raw wrapper output is kept.
set -euo pipefail
here=${BASH_SOURCE[0]%/*}
[ "$here" != "${BASH_SOURCE[0]}" ] || here=.
exec python3 -B "$here/chatgpt-wave12-stage.py" "$@"
