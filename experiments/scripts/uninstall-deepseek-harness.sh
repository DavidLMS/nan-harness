#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=uninstall-common.sh
source "$script_dir/uninstall-common.sh"

usage() {
    printf 'Usage: %s [--purge] [--yes] [--dry-run]\n' "$0"
    printf '%s\n' 'Removes DeepSeek Harness (dsh) global packages and standalone launchers.'
    printf '%s\n' '  --purge  Also remove DSH_HOME (credentials, settings, and sessions).'
    show_common_options | sed '1d'
    printf '%s\n' 'Official package commands: pip uninstall deepseek-harness-cli; npm uninstall -g @deepseek-ai/dsh'
    printf '%s\n' 'npx runs are ephemeral; this helper does not delete the shared npm cache by default.'
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
dsh_home="${DSH_HOME:-$home/.dsh}"
if [[ "$UNINSTALL_PURGE" == true ]] && ! safe_state_path "$dsh_home" "$home"; then
    printf 'Refusing to purge unsafe DeepSeek Harness home: %s\n' "$dsh_home" >&2
    exit 2
fi
if ! confirm_uninstall_purge 'DeepSeek Harness' "$dsh_home"; then
    exit 0
fi

uninstall_npm_package '@deepseek-ai/dsh'
uninstall_python_package 'python3' 'deepseek-harness-cli'
uninstall_python_package 'py' 'deepseek-harness-cli'
remove_uninstall_paths \
    "$dsh_home/bin/dsh" \
    "$home/.local/bin/dsh"
for startup_file in "$home/.zshrc" "$home/.bashrc"; do
    remove_uninstall_path_entries "$startup_file" "$dsh_home/bin"
    remove_uninstall_path_entries "$startup_file" '\$HOME/.dsh/bin'
done

if [[ "$UNINSTALL_PURGE" == true ]]; then
    remove_uninstall_state_path "$dsh_home" "$home"
fi

finish_uninstall 'DeepSeek Harness'
