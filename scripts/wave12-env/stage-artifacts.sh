#!/usr/bin/env bash
# Validate private snapshots before publishing any bytes.
# Usage: stage-artifacts.sh <condition-directory> <new-staging-directory> <checker>
set -euo pipefail
exec python3 -B "$(dirname -- "${BASH_SOURCE[0]}")/stage_artifacts.py" "$@"
