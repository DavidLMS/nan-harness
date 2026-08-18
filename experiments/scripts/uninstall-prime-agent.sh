#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=uninstall-common.sh
source "$script_dir/uninstall-common.sh"

usage() {
    printf 'Usage: %s [--purge] [--yes] [--dry-run]\n' "$0"
    printf '%s\n' 'Removes Prime Agent standalone binaries and installer-managed extensions.'
    printf '%s\n' '  --purge  Also remove PRIME_AGENT_CODING_AGENT_DIR (credentials, sessions, and settings).'
    show_common_options | sed '1d'
    printf '%s\n' 'Official installer: https://app.primeintellect.ai/prime-agent/install.sh'
    printf '%s\n' 'No stable package-manager uninstall command is advertised; only known user-owned paths are removed.'
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
prime_home="${PRIME_AGENT_CODING_AGENT_DIR:-$home/.prime/agent}"
if ! safe_state_path "$prime_home" "$home"; then
    printf 'Refusing unsafe Prime Agent home: %s\n' "$prime_home" >&2
    exit 2
fi
if [[ "$UNINSTALL_PURGE" == true ]] && ! safe_state_path "$prime_home" "$home"; then
    printf 'Refusing to purge unsafe Prime Agent home: %s\n' "$prime_home" >&2
    exit 2
fi
if ! confirm_uninstall_purge 'Prime Agent' "$prime_home"; then
    exit 0
fi

remove_uninstall_paths \
    "$prime_home/bin/prime-agent" \
    "$home/.prime/bin/prime-agent" \
    "$home/.local/bin/prime-agent"
for startup_file in "$home/.zshrc" "$home/.bashrc"; do
    remove_uninstall_path_entries "$startup_file" "$home/.prime/bin"
    remove_uninstall_path_entries "$startup_file" '\$HOME/.prime/bin'
done

if [[ "$UNINSTALL_PURGE" == true ]]; then
    remove_uninstall_state_path "$prime_home" "$home"
fi

finish_uninstall 'Prime Agent'
