#!/usr/bin/env bash
set -euo pipefail

usage() {
    printf 'Usage: %s [--purge] [--yes] [--dry-run]\n' "$0"
    printf '\n'
    printf 'Removes the uv-installed Mistral Vibe executables.\n'
    printf -- '--purge  Also remove Vibe configuration, sessions, and credentials.\n'
    printf -- '--yes    Skip the confirmation required by --purge.\n'
    printf -- '--dry-run Show actions without changing the filesystem.\n'
}

purge=false
assume_yes=false
dry_run=false
for argument in "$@"; do
    case "$argument" in
        --purge)
            purge=true
            ;;
        --yes)
            assume_yes=true
            ;;
        --dry-run)
            dry_run=true
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            printf 'Unknown argument: %s\n' "$argument" >&2
            usage >&2
            exit 2
            ;;
    esac
done

normalize_path() {
    local value="$1"
    while [[ "$value" != "/" && "$value" == */ ]]; do
        value="${value%/}"
    done
    printf '%s' "$value"
}

home="$(normalize_path "${HOME:?HOME must be set}")"
case "$home" in
    ''|/|.|..|/tmp|/private/tmp|/var/tmp|/var|/usr|/etc|/opt|/Applications|/Users|/home|*/..|*/../*|*/.|*/./*)
        printf 'Refusing unsafe HOME for uninstall: %s\n' "$home" >&2
        exit 2
        ;;
esac
tool_bin="$(normalize_path "${UV_TOOL_BIN_DIR:-$home/.local/bin}")"
case "$tool_bin" in
    ''|/|"$home"|.|..|/tmp|/private/tmp|/var/tmp|/var|/usr|/etc|/opt|/Applications|/Users|/home|*/..|*/../*|*/.|*/./*)
        printf 'Refusing unsafe uv tool directory: %s\n' "$tool_bin" >&2
        exit 2
        ;;
esac
vibe_home="$(normalize_path "${VIBE_HOME:-$home/.vibe}")"
case "$vibe_home" in
    ''|/|"$home"|.|..|/tmp|/private/tmp|/var/tmp|/var|/usr|/etc|/opt|/Applications|/Users|/home|*/..|*/../*|*/.|*/./*)
        printf 'Refusing to purge unsafe Vibe home: %s\n' "$vibe_home" >&2
        exit 2
        ;;
esac
if [[ "$vibe_home" != "$home/"* ]]; then
    printf 'Refusing to purge Vibe data outside HOME: %s\n' "$vibe_home" >&2
    exit 2
fi

if [[ "$purge" == true && "$assume_yes" != true && "$dry_run" != true ]]; then
    printf 'Remove all Vibe data from %s? [y/N] ' "$vibe_home"
    read -r answer
    case "$answer" in
        y|Y|yes|YES)
            ;;
        *)
            printf 'Vibe purge cancelled.\n'
            exit 0
            ;;
    esac
fi

removed=false
for executable in \
    "$tool_bin/vibe" \
    "$tool_bin/vibe.exe" \
    "$tool_bin/vibe-acp" \
    "$tool_bin/vibe-acp.exe" \
    "$tool_bin/vibe-app-server" \
    "$tool_bin/vibe-app-server.exe"; do
    if [[ -e "$executable" || -L "$executable" ]]; then
        if [[ "$dry_run" == true ]]; then
            printf 'Would remove %s\n' "$executable"
        else
            rm -f -- "$executable"
            printf 'Removed %s\n' "$executable"
        fi
        removed=true
    fi
done

if [[ "$purge" == true && -d "$vibe_home" ]]; then
    if [[ "$dry_run" == true ]]; then
        printf 'Would remove Vibe data from %s\n' "$vibe_home"
    else
        rm -rf -- "$vibe_home"
        printf 'Removed Vibe data from %s\n' "$vibe_home"
    fi
    removed=true
fi

if [[ "$removed" != true ]]; then
    printf 'Mistral Vibe installation was not found.\n'
else
    printf 'Mistral Vibe uninstall complete.\n'
fi
