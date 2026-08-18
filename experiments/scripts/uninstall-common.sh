#!/usr/bin/env bash

# Shared, deliberately conservative helpers for the harness uninstall scripts.
# Source this file from a sibling script; it is not intended to be run directly.

UNINSTALL_PURGE=false
UNINSTALL_YES=false
UNINSTALL_DRY_RUN=false
UNINSTALL_HELP=false
UNINSTALL_CHANGED=false
UNINSTALL_FAILED=false

parse_uninstall_options() {
    while (($# > 0)); do
        case "$1" in
            --purge)
                UNINSTALL_PURGE=true
                ;;
            --yes)
                UNINSTALL_YES=true
                ;;
            --dry-run)
                UNINSTALL_DRY_RUN=true
                ;;
            --)
                shift
                if (($# > 0)); then
                    printf 'Unexpected argument: %s\n' "$1" >&2
                    return 2
                fi
                return 0
                ;;
            -h|--help)
                UNINSTALL_HELP=true
                return 0
                ;;
            *)
                printf 'Unknown argument: %s\n' "$1" >&2
                return 2
                ;;
        esac
        shift
    done
}

show_common_options() {
    printf '%s\n' '  --purge    Also remove configuration, credentials, sessions, and other state.'
    printf '%s\n' '  --yes      Skip the confirmation required by --purge.'
    printf '%s\n' '  --dry-run  Show actions without changing the filesystem or running a manager.'
}

validate_uninstall_home() {
    local home="$1"
    case "$home" in
        ''|/|.|..|/tmp|/private/tmp|/var/tmp|/var|/usr|/etc|/opt|/Applications|/Users|/home|*/..|*/../*|*/.|*/./*)
            printf 'Refusing unsafe HOME for uninstall: %s\n' "$home" >&2
            return 1
            ;;
    esac
    case "$home" in
        /etc/*|/usr/*|/var/*|/opt/*|/Applications/*|/Users/*|/home/*)
            return 0
            ;;
    esac
    return 0
}

confirm_uninstall_purge() {
    local label="$1"
    local state_path="$2"

    [[ "$UNINSTALL_PURGE" == true ]] || return 0
    [[ "$UNINSTALL_DRY_RUN" == true ]] && return 0
    [[ "$UNINSTALL_YES" == true ]] && return 0

    printf 'Remove all %s data from %s? [y/N] ' "$label" "$state_path"
    local answer=''
    if ! read -r answer; then
        printf '%s purge cancelled (no confirmation received).\n' "$label"
        return 1
    fi
    case "$answer" in
        y|Y|yes|YES|Yes)
            ;;
        *)
            printf '%s purge cancelled.\n' "$label"
            return 1
            ;;
    esac
}

safe_state_path() {
    local path="$1"
    local home="$2"

    [[ -n "$path" ]] || return 1
    case "$path" in
        /|.|..|"$home"|"$home/"|/tmp|/private/tmp|/var/tmp|/var|/usr|/etc|/opt|/Applications|/Users|/home)
            return 1
            ;;
        */..|*/../*|*/.|*/./*)
            return 1
            ;;
        /etc/*|/usr/*|/var/*|/opt/*|/Applications/*|/Users/*|/home/*)
            [[ "$path" == "$home/"* ]] || return 1
            ;;
    esac
}

remove_uninstall_state_path() {
    local path="$1"
    local home="$2"

    if ! safe_state_path "$path" "$home"; then
        printf 'Refusing to purge unsafe state path: %s\n' "$path" >&2
        UNINSTALL_FAILED=true
        return 0
    fi
    remove_uninstall_path "$path"
}

remove_uninstall_path() {
    local path="$1"

    if [[ ! -e "$path" && ! -L "$path" ]]; then
        return 0
    fi
    if [[ "$UNINSTALL_DRY_RUN" == true ]]; then
        printf 'Would remove %s\n' "$path"
        return 0
    fi
    if [[ -d "$path" && ! -L "$path" ]]; then
        rm -rf -- "$path"
    else
        rm -f -- "$path"
    fi
    printf 'Removed %s\n' "$path"
    UNINSTALL_CHANGED=true
}

remove_uninstall_paths() {
    local path
    for path in "$@"; do
        remove_uninstall_path "$path"
    done
}

run_uninstall_command() {
    if [[ "$UNINSTALL_DRY_RUN" == true ]]; then
        printf 'Would run:'
        printf ' %q' "$@"
        printf '\n'
        return 0
    fi
    if "$@"; then
        UNINSTALL_CHANGED=true
        return 0
    fi
    printf 'Warning: command failed: %s\n' "$*" >&2
    UNINSTALL_FAILED=true
    return 0
}

npm_package_installed() {
    local package="$1"
    command -v npm >/dev/null 2>&1 || return 1
    npm ls --global --depth=0 "$package" >/dev/null 2>&1
}

uninstall_npm_package() {
    local package="$1"
    if npm_package_installed "$package"; then
        printf 'Official npm uninstall: npm uninstall -g %s\n' "$package"
        run_uninstall_command npm uninstall --global "$package"
    fi
}

brew_formula_installed() {
    local formula="$1"
    command -v brew >/dev/null 2>&1 || return 1
    brew list --formula --versions "$formula" >/dev/null 2>&1
}

uninstall_brew_formula() {
    local formula="$1"
    if brew_formula_installed "$formula"; then
        printf 'Official Homebrew uninstall: brew uninstall --formula %s\n' "$formula"
        run_uninstall_command brew uninstall --formula "$formula"
    fi
}

brew_cask_installed() {
    local cask="$1"
    command -v brew >/dev/null 2>&1 || return 1
    brew list --cask --versions "$cask" >/dev/null 2>&1
}

uninstall_brew_cask() {
    local cask="$1"
    if brew_cask_installed "$cask"; then
        printf 'Official Homebrew uninstall: brew uninstall --cask %s\n' "$cask"
        run_uninstall_command brew uninstall --cask "$cask"
    fi
}

pipx_package_installed() {
    local package="$1"
    command -v pipx >/dev/null 2>&1 || return 1
    pipx list --short 2>/dev/null | awk -v package="$package" '$1 == package { found=1 } END { exit !found }'
}

uninstall_pipx_package() {
    local package="$1"
    if pipx_package_installed "$package"; then
        printf 'Official pipx uninstall: pipx uninstall %s\n' "$package"
        run_uninstall_command pipx uninstall "$package"
    fi
}

python_package_installed() {
    local python_command="$1"
    local package="$2"
    command -v "$python_command" >/dev/null 2>&1 || return 1
    "$python_command" -m pip show "$package" >/dev/null 2>&1
}

uninstall_python_package() {
    local python_command="$1"
    local package="$2"
    if python_package_installed "$python_command" "$package"; then
        printf 'Official pip uninstall: %s -m pip uninstall %s\n' "$python_command" "$package"
        run_uninstall_command "$python_command" -m pip uninstall --yes "$package"
    fi
}

remove_uninstall_path_entries() {
    local startup_file="$1"
    local token="$2"
    local temporary_file=''

    [[ -f "$startup_file" ]] || return 0
    if [[ "$UNINSTALL_DRY_RUN" == true ]]; then
        if awk -v token="$token" 'index($0, token) && index($0, "PATH") { found=1 } END { exit !found }' "$startup_file"; then
            printf 'Would remove PATH entry containing %s from %s\n' "$token" "$startup_file"
        fi
        return 0
    fi

    temporary_file="$(mktemp "${TMPDIR:-/tmp}/nan-harness-uninstall.XXXXXX")"
    awk -v token="$token" 'index($0, token) && index($0, "PATH") { next } { print }' \
        "$startup_file" >"$temporary_file"
    if ! cmp -s "$startup_file" "$temporary_file"; then
        mv -- "$temporary_file" "$startup_file"
        printf 'Removed PATH entry containing %s from %s\n' "$token" "$startup_file"
        UNINSTALL_CHANGED=true
    else
        rm -f -- "$temporary_file"
    fi
}

finish_uninstall() {
    local label="$1"
    if [[ "$UNINSTALL_FAILED" == true ]]; then
        printf '%s uninstall completed with warnings.\n' "$label" >&2
        return 1
    fi
    if [[ "$UNINSTALL_CHANGED" != true && "$UNINSTALL_DRY_RUN" != true ]]; then
        printf '%s installation was not found.\n' "$label"
    else
        printf '%s uninstall complete.\n' "$label"
    fi
}
