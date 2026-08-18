#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=uninstall-common.sh
source "$script_dir/uninstall-common.sh"

usage() {
    printf 'Usage: %s [--purge] [--yes] [--dry-run]\n' "$0"
    printf '%s\n' 'Removes the Goose CLI release binary and optional Homebrew formula.'
    printf '%s\n' '  --purge  Also remove Goose application data and secrets.'
    show_common_options | sed '1d'
    printf '%s\n' 'Official commands: brew uninstall --formula goose; release installer binary is ~/.local/bin/goose'
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
if [[ "$(uname -s 2>/dev/null || true)" == Darwin* ]]; then
    goose_data="$home/Library/Application Support/Block/goose"
else
    goose_data="$home/.local/share/goose"
fi
if [[ "$UNINSTALL_PURGE" == true ]]; then
    for path in "$goose_data" "$home/.config/goose"; do
        if ! safe_state_path "$path" "$home"; then
            printf 'Refusing to purge unsafe Goose path: %s\n' "$path" >&2
            exit 2
        fi
    done
fi
if ! confirm_uninstall_purge 'Goose' "$goose_data"; then
    exit 0
fi

uninstall_brew_formula 'goose'
remove_uninstall_paths "$home/.local/bin/goose" "$home/.local/bin/goose-cli"

if [[ "$UNINSTALL_PURGE" == true ]]; then
    remove_uninstall_state_path "$goose_data" "$home"
    remove_uninstall_state_path "$home/.config/goose" "$home"
fi

finish_uninstall 'Goose'
