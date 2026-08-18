#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=uninstall-common.sh
source "$script_dir/uninstall-common.sh"

usage() {
    printf 'Usage: %s [--purge] [--yes] [--dry-run]\n' "$0"
    printf '%s\n' 'Removes Claude Code packages, native binaries, and installer artifacts.'
    printf '%s\n' 'The native installer and npm/Homebrew installations are handled separately.'
    printf '%s\n' '  --purge  Also remove ~/.claude and ~/.claude.json (credentials, sessions, and settings).'
    show_common_options | sed '1d'
    printf '%s\n' 'Official commands: npm uninstall -g @anthropic-ai/claude-code; brew uninstall --cask claude-code'
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
claude_state="$home/.claude"
if [[ "$UNINSTALL_PURGE" == true ]] && ! safe_state_path "$claude_state" "$home"; then
    printf 'Refusing to purge unsafe Claude Code home: %s\n' "$claude_state" >&2
    exit 2
fi
if ! confirm_uninstall_purge 'Claude Code' "$claude_state"; then
    exit 0
fi

uninstall_npm_package '@anthropic-ai/claude-code'
uninstall_brew_cask 'claude-code'
uninstall_brew_cask 'claude-code@latest'

# Native installer artifacts and the legacy local npm installation are not user data.
remove_uninstall_paths \
    "$home/.local/bin/claude" \
    "$home/.local/share/claude" \
    "$home/.claude/local"
for startup_file in "$home/.zshrc" "$home/.bashrc"; do
    remove_uninstall_path_entries "$startup_file" "$home/.claude/local"
    remove_uninstall_path_entries "$startup_file" '\$HOME/.claude/local'
done

if [[ "$UNINSTALL_PURGE" == true ]]; then
    remove_uninstall_state_path "$claude_state" "$home"
    remove_uninstall_state_path "$home/.claude.json" "$home"
fi

finish_uninstall 'Claude Code'
