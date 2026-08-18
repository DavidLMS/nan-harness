#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=uninstall-common.sh
source "$script_dir/uninstall-common.sh"

usage() {
    printf 'Usage: %s [--purge] [--yes] [--dry-run]\n' "$0"
    printf '%s\n' 'Removes Qwen Code packages, standalone launchers, and installer PATH entries.'
    printf '%s\n' '  --purge  Also remove QWEN_HOME (credentials, sessions, extensions, and settings).'
    show_common_options | sed '1d'
    printf '%s\n' 'Official commands: npm uninstall -g @qwen-code/qwen-code; brew uninstall --formula qwen-code'
    printf '%s\n' 'Official standalone command (manual fallback): https://qwen-code-assets.oss-cn-hangzhou.aliyuncs.com/installation/uninstall-qwen-standalone.sh'
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
qwen_home="${QWEN_HOME:-$home/.qwen}"
if [[ "$UNINSTALL_PURGE" == true ]] && ! safe_state_path "$qwen_home" "$home"; then
    printf 'Refusing to purge unsafe Qwen Code home: %s\n' "$qwen_home" >&2
    exit 2
fi
if ! confirm_uninstall_purge 'Qwen Code' "$qwen_home"; then
    exit 0
fi

uninstall_npm_package '@qwen-code/qwen-code'
uninstall_brew_formula 'qwen-code'
remove_uninstall_paths \
    "$qwen_home/bin/qwen" \
    "$home/.local/bin/qwen"
for startup_file in "$home/.zshrc" "$home/.bashrc"; do
    remove_uninstall_path_entries "$startup_file" "$qwen_home/bin"
    remove_uninstall_path_entries "$startup_file" '\$HOME/.qwen/bin'
done

if [[ "$UNINSTALL_PURGE" == true ]]; then
    remove_uninstall_state_path "$qwen_home" "$home"
fi

finish_uninstall 'Qwen Code'
