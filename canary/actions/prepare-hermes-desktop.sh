#!/usr/bin/env bash
# Prepare Hermes from the exact source revision in a frozen Desktop manifest.
set -euo pipefail
umask 077
[[ "${GITHUB_ACTIONS:-}" == true && "${RUNNER_ENVIRONMENT:-}" == github-hosted ]] || {
  echo 'Hermes preparation requires a disposable GitHub-hosted runner.' >&2
  exit 1
}
[[ -z "${NAN_API_KEY+x}" ]] || { echo 'Remove NAN_API_KEY before preparing Hermes.' >&2; exit 1; }
script_root="$(cd "$(dirname "$0")" && pwd)"
exec python3 "$script_root/desktop_install.py" "$@"
