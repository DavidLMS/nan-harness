#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=uninstall-common.sh
source "$script_dir/uninstall-common.sh"

usage() {
    printf 'Usage: %s [--purge] [--yes] [--dry-run]\n' "$0"
    printf '%s\n' 'Removes the Pi coding agent package and standalone launchers.'
    printf '%s\n' '  --purge  Also remove PI_CODING_AGENT_DIR (credentials, sessions, extensions, and settings).'
    show_common_options | sed '1d'
    printf '%s\n' 'Official command: npm uninstall -g @earendil-works/pi-coding-agent'
}

if ! parse_uninstall_options "$@"; then
    usage >&2
    exit 2
fi
if [[ "$UNINSTALL_HELP" == true ]]; then
    usage
    exit 0
fi

home="${HOME:?HOME must be set}"
validate_uninstall_home "$home"
pi_home="${PI_CODING_AGENT_DIR:-$home/.pi/agent}"
if [[ "$UNINSTALL_PURGE" == true ]] && ! safe_state_path "$pi_home" "$home"; then
    printf 'Refusing to purge unsafe Pi home: %s\n' "$pi_home" >&2
    exit 2
fi
if ! confirm_uninstall_purge 'Pi' "$pi_home"; then
    exit 0
fi

uninstall_npm_package '@earendil-works/pi-coding-agent'
remove_uninstall_paths \
    "$home/.local/bin/pi" \
    "$home/.local/bin/pi-coding-agent"

if [[ "$UNINSTALL_PURGE" == true ]]; then
    remove_uninstall_state_path "$pi_home" "$home"
fi

finish_uninstall 'Pi'
