#!/usr/bin/env bash

retry() {
  local maximum_attempts="$1"
  local delay_seconds="$2"
  shift 2
  local attempt=1
  while ! "$@"; do
    if [ "$attempt" -ge "$maximum_attempts" ]; then
      return 1
    fi
    sleep "$delay_seconds"
    attempt=$((attempt + 1))
  done
}
