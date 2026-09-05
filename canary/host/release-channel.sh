#!/usr/bin/env bash

# Shared primitives for the release channels of one repository: the available-release feed and
# GitHub's `latest` pointer, which is the maintainer-recommended release.
#
# Locking boundary: this is a local advisory lock. It serializes the supported writers that run on
# the single macOS publication host — the compatibility release gate, the feed publisher it calls,
# and the maintainer recommendation. It provides no cross-host atomicity, so a lock owned by
# another host is refused rather than retired, and a writer outside this host is outside the
# protocol. See canary/README.md.

channel_lock_directory=''
channel_lock_owner=''
channel_lock_token=''
channel_lock_held=false
channel_lock_host="$(hostname 2>/dev/null || printf unknown)"

channel_repository_key() {
  printf '%s\n' "${1//\//__}"
}

channel_lock_mtime() {
  local value
  if value="$(stat -f %m "$1" 2>/dev/null)" && [ -n "$value" ]; then
    printf '%s\n' "$value"
    return 0
  fi
  stat -c %Y "$1" 2>/dev/null
}

channel_lock_retire_stale() {
  local stale="$channel_lock_directory.stale.$channel_lock_token"
  mv "$channel_lock_directory" "$stale" 2>/dev/null || return 1
  rm -f "$stale/owner.json" "$stale/.owner.tmp"
  rmdir "$stale"
}

channel_lock_claim() {
  mkdir "$channel_lock_directory" 2>/dev/null || return 1
  if ! jq -n \
    --argjson pid "$$" \
    --arg host "$channel_lock_host" \
    --arg token "$channel_lock_token" \
    --argjson started_at "$(date +%s)" \
    '{pid:$pid,host:$host,token:$token,startedAt:$started_at}' \
    >"$channel_lock_directory/.owner.tmp" \
    || ! mv "$channel_lock_directory/.owner.tmp" "$channel_lock_owner"; then
    rm -f "$channel_lock_directory/.owner.tmp"
    rmdir "$channel_lock_directory" 2>/dev/null || true
    return 1
  fi
  channel_lock_held=true
}

# Refuses a live or foreign owner and retires one that has outlived the stale window.
channel_lock_resolve_owner() {
  local stale_seconds="$1"
  local owner_pid='' owner_host='' owner_started='' age
  if [ -f "$channel_lock_owner" ]; then
    owner_pid="$(jq -er '.pid | numbers' "$channel_lock_owner" 2>/dev/null || true)"
    owner_host="$(jq -er '.host | strings' "$channel_lock_owner" 2>/dev/null || true)"
    owner_started="$(jq -er '.startedAt | numbers' "$channel_lock_owner" 2>/dev/null || true)"
  fi
  if [ "$owner_host" = "$channel_lock_host" ] && [ -n "$owner_pid" ] \
    && [ "$owner_pid" -gt 0 ] 2>/dev/null && kill -0 "$owner_pid" 2>/dev/null; then
    printf 'another release channel transaction is already running (pid %s)\n' "$owner_pid" >&2
    return 1
  fi
  if [ -n "$owner_host" ] && [ "$owner_host" != "$channel_lock_host" ]; then
    printf 'release channel lock belongs to another host: %s; this protocol only serializes writers on a single publication host\n' \
      "$owner_host" >&2
    return 1
  fi
  if [ -n "$owner_started" ]; then
    age=$(( $(date +%s) - owner_started ))
  else
    age=$(( $(date +%s) - $(channel_lock_mtime "$channel_lock_directory") ))
  fi
  [ "$age" -ge 0 ] || age=0
  if [ "$age" -lt "$stale_seconds" ]; then
    printf 'release channel lock is not stale (age %ss)\n' "$age" >&2
    return 1
  fi
  channel_lock_retire_stale || {
    printf 'release channel lock changed while recovering a stale owner\n' >&2
    return 1
  }
}

# Acquires the per-repository channel lock, re-entering the one an ancestor already holds.
channel_lock_acquire() {
  local state_directory="$1"
  local repository="$2"
  local stale_seconds="${NAN_CANARY_LOCK_STALE_SECONDS:-21600}"
  case "$stale_seconds" in
    ''|*[!0-9]*)
      printf 'NAN_CANARY_LOCK_STALE_SECONDS must be a non-negative integer\n' >&2
      return 2
      ;;
  esac
  channel_lock_directory="$state_directory/release-channel-$(channel_repository_key "$repository").lock"
  channel_lock_owner="$channel_lock_directory/owner.json"
  # An ancestor that already owns this lock exports its directory; re-enter instead of deadlocking.
  if [ "${NAN_CANARY_RELEASE_CHANNEL_LOCK:-}" = "$channel_lock_directory" ]; then
    return 0
  fi
  mkdir -p "$state_directory"
  channel_lock_token="$(date -u +%Y%m%dT%H%M%SZ)-$$-${RANDOM:-0}"
  channel_lock_claim && return 0
  channel_lock_resolve_owner "$stale_seconds" || return 1
  channel_lock_claim
}

channel_lock_release() {
  [ "$channel_lock_held" = true ] && [ -f "$channel_lock_owner" ] || return 0
  if [ "$(jq -er '.token | strings' "$channel_lock_owner" 2>/dev/null || true)" = "$channel_lock_token" ]; then
    rm -f "$channel_lock_owner"
    rmdir "$channel_lock_directory" 2>/dev/null || true
  fi
  channel_lock_held=false
}

# Reads one GitHub API path, writing the response body to $2 and printing the HTTP status.
# Returns 1 when the status could not be established, which callers must treat as uncertainty
# rather than as absence.
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
