#!/usr/bin/env bash
# Use complete signed Ubuntu metadata without inheriting a stale hosted mirror.
set -euo pipefail

validate_desktop_package_host() {
  # This workflow's native Linux target is Ubuntu 24.04 x86_64.
  source /etc/os-release
  [[ "$ID" == ubuntu && "${VERSION_CODENAME:-}" == noble && "$(dpkg --print-architecture)" == amd64 ]]
}

install_desktop_packages() (
  validate_desktop_package_host || { echo 'Expected Ubuntu Noble AMD64.' >&2; exit 1; }
  [[ "$#" -gt 0 ]] || { echo 'At least one package is required.' >&2; exit 1; }
  for package in "$@"; do
    [[ "$package" =~ ^[a-z0-9][a-z0-9.+-]*$ ]] || { echo 'Invalid package name.' >&2; exit 1; }
  done
  # Hosted runner temporary directories can have non-traversable ancestors for _apt.
  apt_directory=$(mktemp -d /tmp/nan-desktop-apt.XXXXXX)
  trap 'sudo rm -rf -- "$apt_directory"' EXIT
  mkdir "$apt_directory/lists" "$apt_directory/sourceparts"
  # Metadata contains no credentials; the _apt sandbox must be able to traverse it.
  chmod 755 "$apt_directory" "$apt_directory/lists" "$apt_directory/sourceparts"
  cat > "$apt_directory/ubuntu.list" <<'SOURCES'
deb [arch=amd64 signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] https://archive.ubuntu.com/ubuntu noble main restricted universe multiverse
deb [arch=amd64 signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] https://archive.ubuntu.com/ubuntu noble-updates main restricted universe multiverse
deb [arch=amd64 signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] https://archive.ubuntu.com/ubuntu noble-backports main restricted universe multiverse
deb [arch=amd64 signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] https://security.ubuntu.com/ubuntu noble-security main restricted universe multiverse
SOURCES
  chmod 644 "$apt_directory/ubuntu.list"
  apt_options=(-o Acquire::Retries=2 -o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30
    -o "Dir::Etc::sourcelist=$apt_directory/ubuntu.list"
    -o "Dir::Etc::sourceparts=$apt_directory/sourceparts"
    -o "Dir::State::lists=$apt_directory/lists"
    -o Dir::Cache::pkgcache= -o Dir::Cache::srcpkgcache=)
  # update otherwise returns success with old indexes after transient fetch errors.
  # Only metadata acquisition is wrapped in timeout; package configuration is not replayed.
  timeout --kill-after=10s 300s sudo apt-get "${apt_options[@]}" update --error-on=any
  sudo apt-get "${apt_options[@]}" install --no-install-recommends -y "$@"
)

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  install_desktop_packages "$@"
fi
