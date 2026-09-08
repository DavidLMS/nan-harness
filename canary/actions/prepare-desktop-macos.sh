#!/usr/bin/env bash
# Configure only the disposable hosted desktop, never a developer's login session.
set -euo pipefail
if [[ "${GITHUB_ACTIONS:-}" != true || "${RUNNER_ENVIRONMENT:-}" != github-hosted || "${RUNNER_OS:-}" != macOS || "$(uname -s)" != Darwin || "${NAN_API_KEY+x}" == x ]]; then
    echo "Desktop session preparation requires a hosted macOS runner without provider credentials." >&2
    exit 1
fi

# The hosted image's Dock owns a full-screen surface above application windows.
# Remove that surface in this disposable login session; retain the checker's
# occlusion checks and do not alter Accessibility or Screen Recording permissions.
service="gui/$(id -u)/com.apple.Dock.agent"
if launchctl print "$service" >/dev/null 2>&1; then
    launchctl bootout "$service"
fi
