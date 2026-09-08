#!/usr/bin/env bash
# Build only the official Hermes source on a fresh hosted Linux runner, without keys.
set -euo pipefail
umask 077
[[ "${GITHUB_ACTIONS:-}" == true && "${RUNNER_ENVIRONMENT:-}" == github-hosted && "${RUNNER_OS:-}" == Linux ]] || {
  echo 'Hermes source preparation requires a disposable GitHub-hosted Linux runner.' >&2
  exit 1
}
[[ -z "${NAN_API_KEY+x}" ]] || { echo 'Remove NAN_API_KEY before preparing Hermes.' >&2; exit 1; }
hermes_root="$(mktemp -d "$RUNNER_TEMP/hermes-desktop.XXXXXX")"
source_root="$hermes_root/hermes-agent"
run_without_credentials() {
  env -u NAN_API_KEY -u GH_TOKEN -u GITHUB_TOKEN -u GITHUB_ENV -u GITHUB_OUTPUT \
    -u GITHUB_PATH -u GITHUB_STEP_SUMMARY -u ACTIONS_RUNTIME_TOKEN -u ACTIONS_ID_TOKEN_REQUEST_TOKEN "$@"
}
run_without_credentials git clone --depth 1 https://github.com/NousResearch/hermes-agent.git "$source_root"
run_without_credentials python3 -m venv "$source_root/venv"
run_without_credentials "$source_root/venv/bin/python" -m pip install --disable-pip-version-check -e "$source_root"
(
  cd "$source_root"
  run_without_credentials npm ci --no-audit --no-fund
  cd apps/desktop
  run_without_credentials npm run pack
)
test -d "$source_root/apps/desktop/release"
printf 'HERMES_HOME=%s\nHERMES_DESKTOP_HERMES_ROOT=%s\nHERMES_DESKTOP_HERMES=%s\n' \
  "$hermes_root" "$source_root" "$source_root/venv/bin/hermes" >> "$GITHUB_ENV"
printf 'Prepared official Hermes source commit %s; retained until this disposable runner is destroyed.\n' \
  "$(git -C "$source_root" rev-parse HEAD)"
