#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 3 ] || { [ "$3" != absent ] && [ "$3" != unique ]; }; then
  printf 'usage: %s <repository> <tag> <absent|unique>\n' "$0" >&2
  exit 2
fi

# GitHub allows duplicate drafts for a tag; tag-based downloads can select an
# older draft even after the tag moves. Include every page and fail on API errors.
count="$(gh api --paginate --slurp "repos/$1/releases?per_page=100" |
  jq --arg tag "$2" '[.[][] | select(.tag_name == $tag)] | length')"
expected=0
if [ "$3" = unique ]; then expected=1; fi
if [ "$count" -ne "$expected" ]; then
  printf 'Release tag must have exactly %s matching releases; found %s. Resolve existing drafts before continuing.\n' \
    "$expected" "$count" >&2
  exit 1
fi
