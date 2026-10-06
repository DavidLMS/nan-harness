#!/bin/sh
# Download one verified checker into an owned private directory; never alter PATH.
set -eu
umask 077

fail() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

retain=false
for argument in "$@"; do
    [ "$argument" != --ephemeral ] || retain=true
done

case "$(uname -s):$(uname -m)" in
    Darwin:arm64 | Darwin:aarch64) target=aarch64-apple-darwin ;;
    Darwin:x86_64 | Darwin:amd64) target=x86_64-apple-darwin ;;
    Linux:arm64 | Linux:aarch64) target=aarch64-unknown-linux-gnu ;;
    Linux:x86_64 | Linux:amd64) target=x86_64-unknown-linux-gnu ;;
    *) fail 'the checker does not publish a binary for this operating system and architecture' ;;
esac
command -v curl >/dev/null 2>&1 || fail 'curl is required to download the checker'

checker_directory=$(mktemp -d "${TMPDIR:-/tmp}/nanh-desktop-check.XXXXXX")
cleanup() {
    if [ "$retain" = true ]; then
        printf 'Retained checker download in %s\n' "$checker_directory" >&2
    else
        rm -rf -- "$checker_directory"
    fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP

download() {
    if ! curl --proto '=https' --proto-redir '=https' --tlsv1.2 \
        --fail --location --silent --show-error \
        --connect-timeout 15 --max-time 300 --max-redirs 8 \
        --retry 2 --retry-delay 1 --retry-max-time 20 \
        --max-filesize "$3" "$1" --output "$2"; then
        fail 'the checker download failed; check your connection and retry. No installed applications were changed'
    fi
}

repository_url=https://github.com/DavidLMS/nan-harness/releases/download
version_file=$checker_directory/release-version.txt
if [ -n "${NAN_DESKTOP_CHECK_VERSION:-}" ]; then
    version=$NAN_DESKTOP_CHECK_VERSION
else
    download "$repository_url/desktop-check/release-version.txt" "$version_file" 128
    version=$(tr -d '\r\n' < "$version_file")
fi
printf '%s\n' "$version" | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' \
    || fail 'the checker channel returned an invalid version'

base_url=$repository_url/desktop-check-v$version
artifact=nanh-desktop-check-$target
candidate=$checker_directory/$artifact
checksums=$checker_directory/SHA256SUMS
download "$base_url/SHA256SUMS" "$checksums" 65536
download "$base_url/$artifact" "$candidate" 268435456
expected=$(awk -v artifact="$artifact" '$2 == artifact { count++; checksum=$1 } END { if (count != 1) exit 1; print checksum }' "$checksums") \
    || fail 'the checksum manifest does not identify exactly one checker binary'
[ "${#expected}" -eq 64 ] || fail 'the checker checksum is invalid'
case "$expected" in *[!0-9A-Fa-f]*) fail 'the checker checksum is invalid' ;; esac

if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$candidate" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$candidate" | awk '{print $1}')
elif command -v openssl >/dev/null 2>&1; then
    actual=$(openssl dgst -sha256 "$candidate" | sed 's/^.*= //')
else
    fail 'sha256sum, shasum or openssl is required to verify the checker'
fi
[ "$(printf '%s' "$actual" | tr 'A-F' 'a-f')" = "$(printf '%s' "$expected" | tr 'A-F' 'a-f')" ] \
    || fail 'the downloaded checker failed SHA-256 verification'
chmod 700 "$candidate"
reported_version=$("$candidate" --version) \
    || fail 'the checker could not start; Linux builds require glibc and libxkbcommon'
[ "$reported_version" = "nanh-desktop-check $version" ] \
    || fail 'the downloaded checker reports an unexpected version'

set +e
if [ -t 1 ] && [ -r /dev/tty ]; then
    "$candidate" "$@" </dev/tty
else
    "$candidate" "$@"
fi
checker_status=$?
set -e
exit "$checker_status"
