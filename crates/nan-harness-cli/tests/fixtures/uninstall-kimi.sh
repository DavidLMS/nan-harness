#!/usr/bin/env bash
set -euo pipefail

usage() {
    printf 'Usage: %s [--purge] [--yes]\n' "$0"
    printf '\n'
    printf 'Removes the official Kimi Code executable and its installer PATH entries.\n'
    printf -- '--purge  Also remove Kimi Code configuration, sessions, and credentials.\n'
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

home="${HOME:?HOME must be set}"
case "$home" in
    /|.|..|/tmp|/private/tmp|/var/tmp|/var|/usr|/etc|/opt|/Applications|/Users|/home|*/..|*/../*|*/.|*/./*)
        printf 'Refusing unsafe HOME for uninstall: %s\n' "$home" >&2
        exit 2
        ;;
esac
kimi_home="${KIMI_CODE_HOME:-$home/.kimi-code}"
case "$kimi_home" in
    /|"$home"|.|..|/tmp|/private/tmp|/var/tmp|/var|/usr|/etc|/opt|/Applications|/Users|/home|*/..|*/../*|*/.|*/./*)
        printf 'Refusing to purge unsafe Kimi Code home: %s\n' "$kimi_home" >&2
        exit 2
        ;;
    /etc/*|/usr/*|/var/*|/opt/*|/Applications/*|/Users/*|/home/*)
        if [[ "$kimi_home" != "$home/"* ]]; then
            printf 'Refusing to purge unsafe Kimi Code home: %s\n' "$kimi_home" >&2
            exit 2
        fi
        ;;
esac

temporary_file=""
cleanup() {
    if [[ -n "$temporary_file" ]]; then
        rm -f -- "$temporary_file"
    fi
}
trap cleanup EXIT

if [[ "$purge" == true && "$assume_yes" != true && "$dry_run" != true ]]; then
    printf 'Remove all Kimi Code data from %s? [y/N] ' "$kimi_home"
    read -r answer
    case "$answer" in
        y|Y|yes|YES)
            ;;
        *)
            printf 'Kimi Code purge cancelled.\n'
            exit 0
            ;;
    esac
fi

removed=false
for executable in \
    "$kimi_home/bin/kimi" \
    "$kimi_home/bin/kimi.exe" \
    "$kimi_home/bin/kimi.bak" \
    "$kimi_home/bin/kimi.exe.bak" \
    "$home/.local/bin/kimi" \
    "$home/.local/bin/kimi.exe"; do
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

for startup_file in "$home/.zshrc" "$home/.bashrc"; do
    if [[ ! -f "$startup_file" ]]; then
        continue
    fi
    temporary_file="$(mktemp "${TMPDIR:-/tmp}/nan-kimi-uninstall.XXXXXX")"
    awk 'index($0, ".kimi-code/bin") && index($0, "PATH") { next } { print }' \
        "$startup_file" > "$temporary_file"
    if ! cmp -s "$startup_file" "$temporary_file"; then
        if [[ "$dry_run" == true ]]; then
            printf 'Would remove Kimi Code PATH entry from %s\n' "$startup_file"
            rm -f -- "$temporary_file"
        else
            mv -- "$temporary_file" "$startup_file"
            printf 'Removed Kimi Code PATH entry from %s\n' "$startup_file"
        fi
        removed=true
    else
        rm -f -- "$temporary_file"
    fi
    temporary_file=""
done

if [[ "$purge" == true && -d "$kimi_home" ]]; then
    if [[ "$dry_run" == true ]]; then
        printf 'Would remove Kimi Code data from %s\n' "$kimi_home"
    else
        rm -rf -- "$kimi_home"
        printf 'Removed Kimi Code data from %s\n' "$kimi_home"
    fi
    removed=true
fi

if [[ "$removed" != true ]]; then
    printf 'Kimi Code installation was not found.\n'
else
    printf 'Kimi Code uninstall complete.\n'
fi
