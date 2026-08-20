#!/bin/sh
set -eu

fail() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

repository=${NAN_INSTALL_REPOSITORY:-DavidLMS/nan-harness}
owner=${repository%%/*}
name=${repository#*/}
if [ -z "$owner" ] || [ -z "$name" ] || [ "$name" != "${name#*/}" ]; then
    fail "NAN_INSTALL_REPOSITORY must use the owner/name format"
fi
case "$repository" in
    *[!A-Za-z0-9._/-]*) fail "NAN_INSTALL_REPOSITORY contains unsupported characters" ;;
esac

system=$(uname -s)
machine=$(uname -m)
case "$system:$machine" in
    Darwin:arm64 | Darwin:aarch64) target=aarch64-apple-darwin ;;
    Darwin:x86_64 | Darwin:amd64) target=x86_64-apple-darwin ;;
    Linux:arm64 | Linux:aarch64) target=aarch64-unknown-linux-musl ;;
    Linux:x86_64 | Linux:amd64) target=x86_64-unknown-linux-musl ;;
    *) fail "NaN does not publish a binary for $system $machine" ;;
esac

command -v curl >/dev/null 2>&1 || fail "curl is required to install NaN"
base_url=${NAN_INSTALL_BASE_URL:-https://github.com/$repository/releases/latest/download}
case "$base_url" in
    https://*) curl_protocol='=https' ;;
    http://127.0.0.1:* | http://localhost:* | http://\[::1\]:*) curl_protocol='=http' ;;
    *) fail "NAN_INSTALL_BASE_URL must use HTTPS" ;;
esac

temporary_directory=$(mktemp -d "${TMPDIR:-/tmp}/nan-install.XXXXXX")
trap 'rm -rf "$temporary_directory"' EXIT HUP INT TERM
artifact=nan-$target
candidate=$temporary_directory/$artifact
checksum_file=$temporary_directory/$artifact.sha256
version_file=$temporary_directory/release-version.txt

download() {
    curl --proto "$curl_protocol" --tlsv1.2 --fail --location --silent --show-error \
        "$1" --output "$2"
}

download "$base_url/$artifact" "$candidate"
download "$base_url/$artifact.sha256" "$checksum_file"
download "$base_url/release-version.txt" "$version_file"

expected_checksum=$(sed -n '1{s/[[:space:]].*//;p;q;}' "$checksum_file")
if [ "${#expected_checksum}" -ne 64 ]; then
    fail "the release checksum is invalid"
fi
case "$expected_checksum" in
    *[!0-9A-Fa-f]*) fail "the release checksum is invalid" ;;
esac

if command -v sha256sum >/dev/null 2>&1; then
    actual_checksum=$(sha256sum "$candidate" | sed -n '1{s/[[:space:]].*//;p;q;}')
elif command -v shasum >/dev/null 2>&1; then
    actual_checksum=$(shasum -a 256 "$candidate" | sed -n '1{s/[[:space:]].*//;p;q;}')
elif command -v openssl >/dev/null 2>&1; then
    actual_checksum=$(openssl dgst -sha256 "$candidate" | sed 's/^.*= //')
else
    fail "sha256sum, shasum, or openssl is required to verify NaN"
fi
if [ "$(printf '%s' "$actual_checksum" | tr 'A-F' 'a-f')" != "$(printf '%s' "$expected_checksum" | tr 'A-F' 'a-f')" ]; then
    fail "the downloaded binary failed SHA-256 verification"
fi

release_version=$(tr -d '\r\n' < "$version_file")
case "$release_version" in
    '' | *[!0-9A-Za-z.+-]*) fail "the release version is invalid" ;;
esac
chmod 700 "$candidate"
if ! candidate_version=$("$candidate" --version 2>&1); then
    fail "the downloaded binary did not pass its version check"
fi
if [ "$candidate_version" != "nan $release_version" ]; then
    fail "the downloaded binary does not report version $release_version"
fi

install_directory=${NAN_INSTALL_DIR:-"$HOME/.local/bin"}
mkdir -p "$install_directory"
destination=$install_directory/nan
if [ -d "$destination" ]; then
    fail "$destination is a directory"
fi
staged_binary=$(mktemp "$install_directory/.nan.XXXXXX")
cat "$candidate" > "$staged_binary"
chmod 755 "$staged_binary"
mv -f "$staged_binary" "$destination"

alias_path=$install_directory/nan-harness
if [ -e "$alias_path" ] && [ ! -L "$alias_path" ]; then
    fail "$alias_path exists and is not a symbolic link"
fi
staged_alias=$install_directory/.nan-harness.$$
rm -f "$staged_alias"
ln -s nan "$staged_alias"
mv -f "$staged_alias" "$alias_path"

printf 'NaN %s installed successfully in %s.\n' "$release_version" "$install_directory"
case ":${PATH:-}:" in
    *":$install_directory:"*) ;;
    *)
        printf 'Add %s to PATH, then open a new terminal before running nan.\n' \
            "$install_directory" >&2
        ;;
esac
