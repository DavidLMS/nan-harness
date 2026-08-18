#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=uninstall-common.sh
source "$script_dir/uninstall-common.sh"

usage() {
    printf 'Usage: %s [--purge] [--yes] [--dry-run]\n' "$0"
    printf '%s\n' 'Removes Cline CLI packages and launchers without touching Cline data by default.'
    printf '%s\n' '  --purge  Also remove CLINE_DATA_DIR (credentials, sessions, and settings).'
    show_common_options | sed '1d'
    printf '%s\n' 'Official commands: npm uninstall -g cline; npm uninstall -g @cline/cli'
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
cline_home="${CLINE_DATA_DIR:-$home/.cline}"
if [[ "$UNINSTALL_PURGE" == true ]] && ! safe_state_path "$cline_home" "$home"; then
    printf 'Refusing to purge unsafe Cline home: %s\n' "$cline_home" >&2
    exit 2
fi
if ! confirm_uninstall_purge 'Cline' "$cline_home"; then
    exit 0
fi

uninstall_npm_package 'cline'
uninstall_npm_package '@cline/cli'
remove_uninstall_paths "$home/.local/bin/cline"

if [[ "$UNINSTALL_PURGE" == true ]]; then
    remove_uninstall_state_path "$cline_home" "$home"
fi

finish_uninstall 'Cline'
