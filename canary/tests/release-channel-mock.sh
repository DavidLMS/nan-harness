#!/usr/bin/env bash

# Writes a `gh` stub that simulates the release API surface the channel helpers use, backed by a
# directory tree. Nothing leaves the machine. The stub reproduces the behaviour that made the
# rejected candidate unsafe: `gh release upload --clobber` deletes the existing asset before it
# uploads, so an injected failure leaves the asset gone.
#
# Layout under $GITHUB_ROOT: releases/<tag>/assets/<name>, releases/<tag>/state
# ("draft", "public" or "prerelease"), tags/<tag> (commit sha), latest (recommended tag name).

write_github_mock() {
  local bin_directory="$1"
  mkdir -p "$bin_directory"
  cat >"$bin_directory/gh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$GITHUB_ROOT/log"

release_state() {
  cat "$GITHUB_ROOT/releases/$1/state" 2>/dev/null || printf 'missing\n'
}

release_json() {
  local tag="$1" state names
  state="$(release_state "$tag")"
  names="$(cd "$GITHUB_ROOT/releases/$tag/assets" 2>/dev/null && ls -1 || true)"
  jq -n --arg tag "$tag" --arg state "$state" --arg names "$names" \
    '{tag_name:$tag,tagName:$tag,
      isDraft:($state == "draft"),draft:($state == "draft"),
      isPrerelease:($state == "prerelease"),prerelease:($state == "prerelease"),
      assets:($names | split("\n") | map(select(length > 0) | {name:.}))}'
}

emit_response() {
  printf 'HTTP/2 %s\r\n\r\n' "$1"
  [ "$#" -lt 2 ] || printf '%s\n' "$2"
}

api() {
  local path="$1"
  case "$path" in
    *releases/latest)
      [ "${GH_LATEST_TRANSPORT_FAILURE:-0}" != 1 ] || exit 1
      if [ -n "${GH_LATEST_STATUS:-}" ]; then
        emit_response "$GH_LATEST_STATUS" '{"message":"injected"}'
        exit 0
      fi
      if [ -f "$GITHUB_ROOT/latest" ]; then
        emit_response 200 "$(release_json "$(cat "$GITHUB_ROOT/latest")")"
      else
        emit_response 404 '{"message":"Not Found"}'
      fi
      ;;
    *releases/tags/*)
      local tag="${path##*/}"
      [ "${GH_TAGS_TRANSPORT_FAILURE:-0}" != 1 ] || exit 1
      if [ -n "${GH_TAGS_STATUS:-}" ]; then
        emit_response "$GH_TAGS_STATUS" '{"message":"injected"}'
        exit 0
      fi
      if [ -d "$GITHUB_ROOT/releases/$tag" ]; then
        emit_response 200 "$(release_json "$tag")"
      else
        emit_response 404 '{"message":"Not Found"}'
      fi
      ;;
    *git/ref/tags/*)
      local tag="${path##*/}"
      [ -f "$GITHUB_ROOT/tags/$tag" ] || exit 1
      printf 'commit\t%s\n' "$(cat "$GITHUB_ROOT/tags/$tag")"
      ;;
    *) exit 1 ;;
  esac
}

release_download() {
  local tag="$1"; shift
  local pattern='' output='' directory=''
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --pattern) pattern="$2"; shift 2 ;;
      --output) output="$2"; shift 2 ;;
      --dir) directory="$2"; shift 2 ;;
      *) shift ;;
    esac
  done
  local source="$GITHUB_ROOT/releases/$tag/assets"
  if [ -n "$directory" ]; then
    cp "$source"/* "$directory/"
    return 0
  fi
  [ -f "$source/$pattern" ] || exit 1
  cp "$source/$pattern" "$output"
}

release_upload() {
  local tag="$1"; shift
  local files=()
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --repo) shift 2 ;;
      --clobber) shift ;;
      *) files+=("$1"); shift ;;
    esac
  done
  local directory="$GITHUB_ROOT/releases/$tag/assets"
  mkdir -p "$directory"
  local file name
  for file in "${files[@]}"; do
    name="$(basename "$file")"
    # --clobber deletes the existing asset before uploading the replacement.
    rm -f "$directory/$name"
    if [ "${GH_UPLOAD_KILL:-}" = "$name" ]; then
      exit 137
    fi
    if [ "${GH_UPLOAD_FAIL:-}" = "$name" ]; then
      exit 1
    fi
    cp "$file" "$directory/$name"
  done
}

release_create() {
  local tag="$1"; shift
  local files=() state=public
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --repo|--title|--notes) shift 2 ;;
      --prerelease) state=prerelease; shift ;;
      --draft) state=draft; shift ;;
      *) files+=("$1"); shift ;;
    esac
  done
  mkdir -p "$GITHUB_ROOT/releases/$tag/assets"
  printf '%s\n' "$state" >"$GITHUB_ROOT/releases/$tag/state"
  local file
  for file in "${files[@]}"; do
    cp "$file" "$GITHUB_ROOT/releases/$tag/assets/$(basename "$file")"
  done
}

release_edit() {
  local tag="$1"; shift
  [ "${GH_EDIT_STATUS:-0}" -eq 0 ] || exit "$GH_EDIT_STATUS"
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --draft=false) printf 'public\n' >"$GITHUB_ROOT/releases/$tag/state"; shift ;;
      --latest) printf '%s\n' "$tag" >"$GITHUB_ROOT/latest"; shift ;;
      --latest=false) shift ;;
      *) shift ;;
    esac
  done
}

case "${1:-}" in
  api) api "$2" ;;
  attestation)
    [ "${GH_ATTESTATION_FAILURE:-0}" != 1 ] || exit 1
    ;;
  release)
    action="$2"
    shift 2
    case "$action" in
      download) release_download "$@" ;;
      upload) release_upload "$@" ;;
      create) release_create "$@" ;;
      edit) release_edit "$@" ;;
      view)
        tag="$1"
        [ -d "$GITHUB_ROOT/releases/$tag" ] || exit 1
        release_json "$tag"
        ;;
      *) exit 1 ;;
    esac
    ;;
  *) exit 1 ;;
esac
STUB
  chmod 755 "$bin_directory/gh"
}

asset_digest() {
  shasum -a 256 "$1" | awk '{print $1}'
}

# Creates a published release with installable binaries, the update manifest clients read, and an
# attested checksum document covering all of them.
publish_github_release() {
  local root="$1" repository="$2" version="$3"
  local tag="v$version"
  local assets="$root/releases/$tag/assets"
  mkdir -p "$assets" "$root/tags"
  printf 'public\n' >"$root/releases/$tag/state"
  printf '%s\n' "$(printf 'nan-harness %s' "$tag" | shasum -a 1 | awk '{print $1}')" \
    >"$root/tags/$tag"
  local target
  for target in aarch64-apple-darwin aarch64-unknown-linux-musl; do
    printf 'nan-harness %s for %s\n' "$version" "$target" >"$assets/nan-harness-$target"
  done
  cat >"$assets/update-manifest.json" <<EOF
{
  "schemaVersion": 1,
  "version": "$version",
  "notesUrl": "https://github.com/$repository/releases/tag/$tag",
  "artifacts": [
    {
      "target": "aarch64-apple-darwin",
      "url": "https://github.com/$repository/releases/download/$tag/nan-harness-aarch64-apple-darwin",
      "sha256": "$(asset_digest "$assets/nan-harness-aarch64-apple-darwin")"
    },
    {
      "target": "aarch64-unknown-linux-musl",
      "url": "https://github.com/$repository/releases/download/$tag/nan-harness-aarch64-unknown-linux-musl",
      "sha256": "$(asset_digest "$assets/nan-harness-aarch64-unknown-linux-musl")"
    }
  ]
}
EOF
  local asset
  : >"$assets/SHA256SUMS"
  for asset in nan-harness-aarch64-apple-darwin nan-harness-aarch64-unknown-linux-musl \
    update-manifest.json; do
    printf '%s  %s\n' "$(asset_digest "$assets/$asset")" "$asset" >>"$assets/SHA256SUMS"
  done
}

# Copies a published release's attested metadata into a local assets directory, the way the gate
# hands it to the feed publisher.
stage_release_assets() {
  local root="$1" tag="$2" assets_directory="$3"
  mkdir -p "$assets_directory"
  cp "$root/releases/$tag/assets/SHA256SUMS" "$assets_directory/SHA256SUMS"
}

feed_version() {
  jq -r '.version' "$1/releases/available/assets/update-manifest.json"
}
