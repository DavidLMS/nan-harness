#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=uninstall-common.sh
source "$script_dir/uninstall-common.sh"

usage() {
    printf 'Usage: %s [--purge] [--yes] [--dry-run]\n' "$0"
    printf '%s\n' 'Removes Hermes Agent packages, managed binaries, and installer artifacts.'
    printf '%s\n' '  --purge  Also remove HERMES_HOME (credentials, sessions, skills, and settings).'
    show_common_options | sed '1d'
    printf '%s\n' 'Official commands: hermes uninstall --yes; hermes uninstall --full --yes'
    printf '%s\n' 'Fallback package commands: pipx uninstall hermes-agent; python3 -m pip uninstall hermes-agent'
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
hermes_home="${HERMES_HOME:-$home/.hermes}"
if ! safe_state_path "$hermes_home" "$home"; then
    printf 'Refusing unsafe Hermes home: %s\n' "$hermes_home" >&2
    exit 2
fi
if [[ "$UNINSTALL_PURGE" == true ]] && ! safe_state_path "$hermes_home" "$home"; then
    printf 'Refusing to purge unsafe Hermes home: %s\n' "$hermes_home" >&2
    exit 2
fi
if ! confirm_uninstall_purge 'Hermes Agent' "$hermes_home"; then
    exit 0
fi

# Hermes provides an official uninstaller for its managed install. It leaves
# config/data alone unless --full is explicitly selected.
if command -v hermes >/dev/null 2>&1; then
    if [[ "$UNINSTALL_PURGE" == true ]]; then
        printf 'Official Hermes uninstall: hermes uninstall --full --yes\n'
        run_uninstall_command hermes uninstall --full --yes
    else
        printf 'Official Hermes uninstall: hermes uninstall --yes\n'
        run_uninstall_command hermes uninstall --yes
    fi
fi
uninstall_pipx_package 'hermes-agent'
uninstall_python_package 'python3' 'hermes-agent'
remove_uninstall_paths "$home/.local/bin/hermes" "$hermes_home/bin/hermes"
for startup_file in "$home/.zshrc" "$home/.bashrc"; do
    remove_uninstall_path_entries "$startup_file" "$hermes_home/bin"
    remove_uninstall_path_entries "$startup_file" '\$HOME/.hermes/bin'
done

if [[ "$UNINSTALL_PURGE" == true ]]; then
    remove_uninstall_state_path "$hermes_home" "$home"
fi

finish_uninstall 'Hermes Agent'
