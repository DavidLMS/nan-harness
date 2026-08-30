#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 2 ] || [ "$#" -gt 4 ]; then
  printf 'usage: %s <owner/repository> <commit-sha> [workflow] [branch]\n' "$0" >&2
  exit 2
fi

repository="$1"
commit_sha="$2"
workflow="${3:-ci.yml}"
branch="${4:-main}"
timeout_seconds="${NAN_RELEASE_CI_WAIT_SECONDS-300}"
poll_seconds="${NAN_RELEASE_CI_POLL_SECONDS-15}"
case "$timeout_seconds" in
  ''|*[!0-9]*) printf 'CI wait duration must be a non-negative integer\n' >&2; exit 2 ;;
esac
case "$poll_seconds" in
  ''|*[!0-9]*|0) printf 'CI poll duration must be a positive integer\n' >&2; exit 2 ;;
esac

deadline="$(( $(date +%s) + timeout_seconds ))"
while true; do
  response="$(gh api --method GET \
    "repos/$repository/actions/workflows/$workflow/runs" \
    -f "head_sha=$commit_sha" \
    -f event=push \
    -f per_page=100)" || {
      printf 'could not query CI runs for %s\n' "$commit_sha" >&2
      exit 1
    }
  run="$(jq --compact-output \
    --arg sha "$commit_sha" \
    --arg branch "$branch" \
    '[.workflow_runs[] | select(.head_sha == $sha and .head_branch == $branch and .event == "push")] | sort_by(.id) | last // empty' \
    <<<"$response")" || {
      printf 'GitHub returned malformed CI run data\n' >&2
      exit 1
    }

  if [ -n "$run" ]; then
    status="$(jq --exit-status --raw-output '.status' <<<"$run")"
    conclusion="$(jq --exit-status --raw-output '.conclusion // ""' <<<"$run")"
    url="$(jq --exit-status --raw-output '.html_url // ""' <<<"$run")"
    if [ "$status" = completed ]; then
      if [ "$conclusion" = success ]; then
        printf 'Exact-SHA CI is green: %s\n' "$url"
        exit 0
      fi
      printf 'exact-SHA CI completed with %s: %s\n' "${conclusion:-no conclusion}" "$url" >&2
      exit 1
    fi
  fi

  if [ "$(date +%s)" -ge "$deadline" ]; then
    printf 'no successful completed CI run for %s on %s within %s seconds\n' \
      "$commit_sha" "$branch" "$timeout_seconds" >&2
    exit 1
  fi
  sleep "$poll_seconds"
done
