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
  # Both transports serve Ubuntu-signed indexes/packages. Never combine partial
  # metadata from one transport with another, or replay package configuration.
  for transport in canonical kernel; do
    : > "$apt_directory/ubuntu.list"
    for suite in noble noble-updates noble-backports noble-security; do
      if [[ "$transport" == kernel ]]; then
        mirror=https://mirrors.edge.kernel.org/ubuntu
      elif [[ "$suite" == noble-security ]]; then
        mirror=https://security.ubuntu.com/ubuntu
      else
        mirror=https://archive.ubuntu.com/ubuntu
      fi
      printf 'deb [arch=amd64 signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] %s %s main restricted universe multiverse\n' "$mirror" "$suite" >> "$apt_directory/ubuntu.list"
    done
    chmod 644 "$apt_directory/ubuntu.list"
    apt_options=(-o Acquire::Retries=2 -o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30
      -o "Dir::Etc::sourcelist=$apt_directory/ubuntu.list"
      -o "Dir::Etc::sourceparts=$apt_directory/sourceparts"
      -o "Dir::State::lists=$apt_directory/lists"
      -o Dir::Cache::pkgcache= -o Dir::Cache::srcpkgcache=)
    # update otherwise returns success with old indexes after transient fetch errors.
    if timeout --kill-after=10s 300s sudo apt-get "${apt_options[@]}" update --error-on=any; then
      sudo apt-get "${apt_options[@]}" install --no-install-recommends -y "$@"
      exit "$?"
    fi
    echo "Complete signed Ubuntu metadata unavailable via $transport." >&2
    sudo rm -rf -- "$apt_directory/lists"
    mkdir "$apt_directory/lists"
    chmod 755 "$apt_directory/lists"
  done
  exit 1
)

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  install_desktop_packages "$@"
fi
