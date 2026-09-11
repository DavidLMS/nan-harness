#!/usr/bin/env bash
# Validate private snapshots before publishing any bytes.
# Usage: stage-artifacts.sh <condition-directory> <new-staging-directory> <checker>
set -euo pipefail
here="${BASH_SOURCE[0]%/*}"
[ "$here" != "${BASH_SOURCE[0]}" ] || here=.
exec python3 -B "$here/stage_artifacts.py" "$@"
