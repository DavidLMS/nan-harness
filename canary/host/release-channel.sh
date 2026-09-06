#!/usr/bin/env bash

# Shared primitives for the release channels of one repository: the available-release feed and
# GitHub's `latest` pointer, which is the maintainer-recommended release.
#
# Locking boundary: the per-repository transaction lock is the shared publication-host lock, so it
# serializes the supported writers that run on the single macOS publication host — the
# compatibility release gate, the feed publisher it calls, and the maintainer recommendation — and
# claims no cross-host atomicity. See canary/host/host-lock.sh and canary/README.md.

source "$(dirname "${BASH_SOURCE[0]}")/host-lock.sh"

channel_repository_key() {
  printf '%s\n' "${1//\//__}"
}

# Acquires the transaction lock for one repository's release channels, re-entering the lock an
# ancestor transaction already holds.
channel_lock_acquire() {
  host_lock_acquire \
    "$1/release-channel-$(channel_repository_key "$2").lock" 'release channel'
}

channel_lock_release() {
  host_lock_release
}

channel_api() {
  local path="$1"
  local body="$2"
  local response="$body.response"
  gh api "$path" --include >"$response" 2>&1 || true
  local status
  status="$(awk '{ gsub(/\r/, "") } toupper($1) ~ /^HTTP/ { code = $2 } END { print code }' "$response")"
  case "$status" in
    ''|*[!0-9]*) return 1 ;;
  esac
  sed -n '/^[[:space:]]*$/,$p' "$response" | sed '1d' >"$body"
  printf '%s\n' "$status"
}

# Prints `present`, `absent`, or fails when the answer is uncertain.
channel_release_presence() {
  local repository="$1"
  local release_tag="$2"
  local body="$3"
  local status
  status="$(channel_api "repos/$repository/releases/tags/$release_tag" "$body")" || {
    printf 'could not read release %s in %s; refusing to act on an uncertain answer\n' \
      "$release_tag" "$repository" >&2
    return 1
  }
  case "$status" in
    200) printf 'present\n' ;;
    404) printf 'absent\n' ;;
    *)
      printf 'reading release %s in %s answered HTTP %s; refusing to act on an uncertain answer\n' \
        "$release_tag" "$repository" "$status" >&2
      return 1
      ;;
  esac
}

# Resolves a remote tag to the commit it names, following annotated tag objects.
channel_remote_tag_commit() {
  local repository="$1"
  local release_tag="$2"
  local object type sha
  object="$(gh api "repos/$repository/git/ref/tags/$release_tag" --jq '[.object.type,.object.sha] | @tsv')" \
    || return 1
  type="${object%%$'\t'*}"
  sha="${object#*$'\t'}"
  while [ "$type" = tag ]; do
    object="$(gh api "repos/$repository/git/tags/$sha" --jq '[.object.type,.object.sha] | @tsv')" \
      || return 1
    type="${object%%$'\t'*}"
    sha="${object#*$'\t'}"
  done
  [ "$type" = commit ] || return 1
  printf '%s\n' "$sha"
}

channel_sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

# True when $1 is strictly newer than $2; both are stable x.y.z versions.
channel_version_is_newer() {
  [ "$1" != "$2" ] \
    && [ "$(printf '%s\n%s\n' "$1" "$2" | sort -t. -k1,1n -k2,2n -k3,3n | head -1)" = "$2" ]
}

# Prints the version whose immutable manifest the feed pointer must carry: the highest version
# among the feed's `update-manifest-<version>.json` assets. Prints nothing for an empty log.
channel_feed_target_version() {
  local assets="$1"
  jq -r '.assets[]?.name | select(test("^update-manifest-[0-9]+\\.[0-9]+\\.[0-9]+\\.json$"))
         | ltrimstr("update-manifest-") | rtrimstr(".json")' "$assets" \
    | sort -t. -k1,1n -k2,2n -k3,3n | tail -1
}

channel_feed_has_asset() {
  jq -e --arg name "$2" 'any(.assets[]?; .name == $name)' "$1" >/dev/null
}

# Prints the release version the feed pointer currently offers. Fails when the pointer cannot be
# read, which callers must treat as uncertainty.
channel_feed_published_version() {
  local repository="$1"
  local feed_tag="$2"
  local document="$3"
  gh release download "$feed_tag" --repo "$repository" \
    --pattern update-manifest.json --output "$document" --clobber >/dev/null 2>&1 || return 1
  jq -er '.version | strings' "$document"
}

# Validates a consumer manifest document against the release it claims to describe: its schema,
# its version, and artifact URLs that can only point into that exact tag's downloads.
channel_manifest_describes_release() {
  local document="$1"
  local expected_version="$2"
  local repository="$3"
  jq -e \
    --arg version "$expected_version" \
    --arg prefix "https://github.com/$repository/releases/download/v$expected_version/" \
    '.schemaVersion == 1 and .version == $version and
     (.notesUrl | type == "string" and startswith("https://")) and
     (.artifacts | type == "array" and length > 0) and
     all(.artifacts[];
       (.target | type == "string" and length > 0) and
       (.sha256 | test("^[0-9a-fA-F]{64}$")) and
       (.url | startswith($prefix)))' \
    "$document" >/dev/null
}

channel_download_asset() {
  gh release download "$2" --repo "$1" --pattern "$3" --output "$4" --clobber >/dev/null 2>&1
}

# Proves the release still carries every asset the attested checksum document names, with the
# digest recorded there. An unchanged checksum document is only a list of expectations: the assets
# it lists can be replaced or deleted underneath it, and only downloading them can tell.
channel_verify_attested_assets() {
  local repository="$1" release_tag="$2" checksum_manifest="$3" work_directory="$4"
  local expected name asset
  while read -r expected name; do
    [ -n "$name" ] || continue
    asset="$work_directory/asset-$name"
    channel_download_asset "$repository" "$release_tag" "$name" "$asset" || {
      printf 'release %s no longer carries the attested asset %s\n' "$release_tag" "$name" >&2
      return 1
    }
    [ "$(channel_sha256_file "$asset")" = "$expected" ] || {
      printf 'the attested asset %s of release %s no longer matches its recorded checksum\n' \
        "$name" "$release_tag" >&2
      return 1
    }
  done <"$checksum_manifest"
}

# Proves the installable artifacts a consumer manifest points at are assets of this exact release
# and hash to the digests the manifest publishes to clients.
channel_verify_manifest_artifacts() {
  local repository="$1" release_tag="$2" manifest="$3" work_directory="$4"
  local prefix="https://github.com/$repository/releases/download/$release_tag/"
  local url expected name asset
  while read -r url expected; do
    case "$url" in
      "$prefix"*) name="${url#"$prefix"}" ;;
      *)
        printf 'the manifest of release %s offers an artifact outside that release: %s\n' \
          "$release_tag" "$url" >&2
        return 1
        ;;
    esac
    asset="$work_directory/artifact-$name"
    channel_download_asset "$repository" "$release_tag" "$name" "$asset" || {
      printf 'release %s no longer carries the installable artifact %s\n' "$release_tag" "$name" >&2
      return 1
    }
    [ "$(channel_sha256_file "$asset")" = "$expected" ] || {
      printf 'the installable artifact %s of release %s no longer matches its published checksum\n' \
        "$name" "$release_tag" >&2
      return 1
    }
  done < <(jq -r '.artifacts[] | [.url, (.sha256 | ascii_downcase)] | @tsv' "$manifest")
}
