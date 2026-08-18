#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=uninstall-common.sh
source "$script_dir/uninstall-common.sh"

usage() {
    printf 'Usage: %s [--purge] [--yes] [--dry-run]\n' "$0"
    printf '%s\n' 'Removes OpenCode packages and standalone binaries without touching data by default.'
    printf '%s\n' '  --purge  Also remove XDG OpenCode config, sessions, logs, cache, and state.'
    show_common_options | sed '1d'
    printf '%s\n' 'Official commands: npm uninstall -g opencode-ai; brew uninstall --formula opencode; opencode uninstall'
    printf '%s\n' 'The official opencode uninstall command is interactive; this helper performs the safe local equivalent.'
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
config_home="${XDG_CONFIG_HOME:-$home/.config}"
data_home="${XDG_DATA_HOME:-$home/.local/share}"
cache_home="${XDG_CACHE_HOME:-$home/.cache}"
state_home="${XDG_STATE_HOME:-$home/.local/state}"
opencode_state="$config_home/opencode"
if [[ "$UNINSTALL_PURGE" == true ]]; then
    for path in "$opencode_state" "$data_home/opencode" "$cache_home/opencode" "$state_home/opencode"; do
        if ! safe_state_path "$path" "$home"; then
            printf 'Refusing to purge unsafe OpenCode path: %s\n' "$path" >&2
            exit 2
        fi
    done
fi
if ! confirm_uninstall_purge 'OpenCode' "$opencode_state"; then
    exit 0
fi

uninstall_npm_package 'opencode-ai'
uninstall_brew_formula 'opencode'

# Standalone installers use a private directory; ~/.local/bin itself is shared.
remove_uninstall_paths \
    "$home/.opencode/bin/opencode" \
    "$home/.local/bin/opencode"
for startup_file in "$home/.zshrc" "$home/.bashrc"; do
    remove_uninstall_path_entries "$startup_file" "$home/.opencode/bin"
    remove_uninstall_path_entries "$startup_file" '\$HOME/.opencode/bin'
done

if [[ "$UNINSTALL_PURGE" == true ]]; then
    remove_uninstall_state_path "$opencode_state" "$home"
    remove_uninstall_state_path "$data_home/opencode" "$home"
    remove_uninstall_state_path "$cache_home/opencode" "$home"
    remove_uninstall_state_path "$state_home/opencode" "$home"
fi

finish_uninstall 'OpenCode'
