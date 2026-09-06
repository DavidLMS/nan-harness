#!/usr/bin/env bash

# Runs a real process that holds a publication-host lock, so concurrency tests observe the actual
# primitive rather than a hand-written owner document. The holder blocks on a FIFO with the `read`
# builtin: it has no child process, so killing it really does kill the owner of the lock.

lock_fixture_holder_pid=''
lock_fixture_directory=''

lock_fixture_prepare() {
  lock_fixture_directory="$1"
  mkdir -p "$lock_fixture_directory"
  mkfifo "$lock_fixture_directory/hold"
  cat >"$lock_fixture_directory/holder.sh" <<'HOLDER'
set -euo pipefail
source "$1"
host_lock_acquire "$2" 'publication test' || exit 3
touch "$3"
read -r _ <"$4" || true
host_lock_release
HOLDER
}

# Holds the lock at $2, using the helper at $1, until released or killed.
lock_fixture_hold() {
  rm -f "$lock_fixture_directory/held"
  bash "$lock_fixture_directory/holder.sh" "$1" "$2" \
    "$lock_fixture_directory/held" "$lock_fixture_directory/hold" &
  lock_fixture_holder_pid=$!
  local waited=0
  while [ ! -f "$lock_fixture_directory/held" ]; do
    [ "$waited" -lt 500 ] || {
      printf 'the lock holder never took %s\n' "$2" >&2
      return 1
    }
    waited=$((waited + 1))
    sleep 0.02
  done
}

lock_fixture_release() {
  printf 'go\n' >"$lock_fixture_directory/hold"
  wait "$lock_fixture_holder_pid"
  lock_fixture_holder_pid=''
}

lock_fixture_kill() {
  kill -9 "$lock_fixture_holder_pid"
  wait "$lock_fixture_holder_pid" 2>/dev/null || true
  lock_fixture_holder_pid=''
}
