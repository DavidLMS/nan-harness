#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
python3 "$root/canary/actions/publication.py" checkpoint \
  --repository "${GITHUB_REPOSITORY:-$(jq -er '.repository' "$1")}" --receipt "$1"
