#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=uninstall-common.sh
source "$script_dir/uninstall-common.sh"

usage() {
    printf 'Usage: %s [--purge] [--yes] [--dry-run]\n' "$0"
    printf '%s\n' 'Removes OpenAI Codex CLI packages and standalone binaries.'
    printf '%s\n' '  --purge  Also remove ~/.codex and ~/.codex.json (credentials, sessions, and settings).'
    show_common_options | sed '1d'
    printf '%s\n' 'Official commands: npm uninstall -g @openai/codex; brew uninstall --cask codex'
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
codex_state="$home/.codex"
if [[ "$UNINSTALL_PURGE" == true ]] && ! safe_state_path "$codex_state" "$home"; then
    printf 'Refusing to purge unsafe Codex home: %s\n' "$codex_state" >&2
    exit 2
fi
if ! confirm_uninstall_purge 'Codex' "$codex_state"; then
    exit 0
fi

uninstall_npm_package '@openai/codex'
uninstall_brew_cask 'codex'

# The Codex install script is standalone; generic ~/.local/bin is intentionally
# left in PATH because other tools commonly use it.
remove_uninstall_paths \
    "$home/.local/bin/codex" \
    "$home/.codex/bin/codex"

if [[ "$UNINSTALL_PURGE" == true ]]; then
    remove_uninstall_state_path "$codex_state" "$home"
    remove_uninstall_state_path "$home/.codex.json" "$home"
fi

finish_uninstall 'Codex'
