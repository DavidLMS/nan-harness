#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=uninstall-common.sh
source "$script_dir/uninstall-common.sh"

usage() {
    printf 'Usage: %s [--purge] [--yes] [--dry-run]\n' "$0"
    printf '%s\n' 'Removes Aider packages and its user launcher.'
    printf '%s\n' '  --purge  Also remove Aider home/repo-independent settings and model metadata.'
    show_common_options | sed '1d'
    printf '%s\n' 'Official commands: pipx uninstall aider-chat; python3 -m pip uninstall aider-chat'
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
state_anchor="$home/.aider.conf.yml"
if ! confirm_uninstall_purge 'Aider' "$state_anchor"; then
    exit 0
fi

uninstall_pipx_package 'aider-chat'
uninstall_python_package 'python3' 'aider-chat'
remove_uninstall_paths "$home/.local/bin/aider" "$home/.local/bin/aider-install"

if [[ "$UNINSTALL_PURGE" == true ]]; then
    # Only home-level files are managed here; project-local .aider files may
    # contain user work and are intentionally never traversed or deleted.
    remove_uninstall_state_path "$home/.aider.conf.yml" "$home"
    remove_uninstall_state_path "$home/.aider.model.settings.yml" "$home"
    remove_uninstall_state_path "$home/.aider.model.metadata.json" "$home"
fi

finish_uninstall 'Aider'
