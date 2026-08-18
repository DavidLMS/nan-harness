#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=uninstall-common.sh
source "$script_dir/uninstall-common.sh"

usage() {
    printf 'Usage: %s [--purge] [--yes] [--dry-run]\n' "$0"
    printf '%s\n' 'Removes OpenClaw packages and standalone binaries.'
    printf '%s\n' '  --purge  Also remove ~/.openclaw (credentials, sessions, and settings).'
    show_common_options | sed '1d'
    printf '%s\n' 'Official commands: npm uninstall -g openclaw; openclaw installer uninstaller (if provided by that release)'
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
openclaw_home="$home/.openclaw"
if [[ "$UNINSTALL_PURGE" == true ]] && ! safe_state_path "$openclaw_home" "$home"; then
    printf 'Refusing to purge unsafe OpenClaw home: %s\n' "$openclaw_home" >&2
    exit 2
fi
if ! confirm_uninstall_purge 'OpenClaw' "$openclaw_home"; then
    exit 0
fi

uninstall_npm_package 'openclaw'
remove_uninstall_paths \
    "$openclaw_home/bin/openclaw" \
    "$home/.local/bin/openclaw"
for startup_file in "$home/.zshrc" "$home/.bashrc"; do
    remove_uninstall_path_entries "$startup_file" "$openclaw_home/bin"
    remove_uninstall_path_entries "$startup_file" '\$HOME/.openclaw/bin'
done

if [[ "$UNINSTALL_PURGE" == true ]]; then
    remove_uninstall_state_path "$openclaw_home" "$home"
fi

finish_uninstall 'OpenClaw'
