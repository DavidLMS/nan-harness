#!/usr/bin/env bash

# One exclusive lock per named resource, held by the kernel through flock(2) on a descriptor this
# shell keeps open.
#
# The kernel owns the exclusion, so there is no owner document to consult before deciding, no
# staleness window, and no reclamation step: a recovery path that renames or deletes whatever
# currently occupies the lock cannot exist, and therefore cannot retire a replacement live owner.
# A holder that dies releases the lock automatically once the last process holding the inherited
# descriptor is gone, which includes the short-lived `gh` and `jq` children it spawned. Recovery is
# therefore never early, only — very briefly — late.
#
# Boundary: flock is local and advisory. It serializes the writers that run on the single macOS
# publication host and claims nothing at all across hosts, so a lock whose note records another
# host is refused and never reclaimed. See canary/README.md.
#
# Requires perl, a macOS base tool the canary preflight checks for. It is used only to call
# flock(2) and fstat(2) on descriptor 9, which bash cannot do on its own.

host_lock_descriptor_path=''
host_lock_label=''
host_lock_state=free
host_lock_host="$(hostname 2>/dev/null || printf unknown)"

# Takes the exclusive lock on descriptor 9 without blocking. Re-locking a descriptor that already
# holds it succeeds, which is what makes an inherited lock re-entrant.
host_lock_flock() {
  perl -e 'use Fcntl qw(:flock);
           open(my $handle, ">&=", 9) or exit 2;
           flock($handle, LOCK_EX | LOCK_NB) or exit 1;'
}

# True when descriptor 9 is already open on this exact lock file, which only an ancestor
# transaction that opened it can arrange.
host_lock_descriptor_is() {
  perl -e 'open(my $handle, ">&=", 9) or exit 1;
           my @open = stat($handle) or exit 1;
           my @named = stat($ARGV[0]) or exit 1;
           exit($open[0] == $named[0] && $open[1] == $named[1] ? 0 : 1);' "$1"
}

host_lock_note_field() {
  jq -er --arg field "$1" '.[$field] | strings // (numbers | tostring)' \
    "$host_lock_descriptor_path" 2>/dev/null || true
}

host_lock_write_note() {
  jq -n --argjson pid "$$" --arg host "$host_lock_host" \
    --argjson started_at "$(date +%s)" \
    '{pid:$pid,host:$host,startedAt:$started_at}' >"$host_lock_descriptor_path"
}

# Refuses a lock whose note records another host: this protocol serializes one publication host
# and must not pretend a local flock reaches another machine.
host_lock_reject_foreign_host() {
  local owner_host
  owner_host="$(host_lock_note_field host)"
  [ -n "$owner_host" ] && [ "$owner_host" != "$host_lock_host" ] || return 1
  printf '%s lock belongs to another host: %s; this protocol only serializes writers on a single publication host\n' \
    "$host_lock_label" "$owner_host" >&2
  return 0
}

# Acquires the lock at $1, described in messages as $2. Re-enters the lock an ancestor already
# holds, proving ownership from the inherited descriptor rather than from any environment marker.
host_lock_acquire() {
  # One descriptor means one lock per process: taking a second one would close the first and
  # silently release it.
  [ "$host_lock_state" = free ] || [ "$host_lock_descriptor_path" = "$1" ] || {
    printf 'this process already holds the %s lock and cannot take a second one\n' \
      "$host_lock_label" >&2
    return 1
  }
  host_lock_descriptor_path="$1"
  host_lock_label="$2"
  mkdir -p "$(dirname "$host_lock_descriptor_path")"
  # The previous protocol used a lock directory. One left behind by a crash under that protocol
  # cannot be opened, so say what to do instead of failing with a redirection error.
  [ ! -d "$host_lock_descriptor_path" ] || {
    printf '%s is blocked by a lock directory from the previous protocol; remove %s once no publication is running\n' \
      "$host_lock_label" "$host_lock_descriptor_path" >&2
    return 1
  }
  local inherited=false
  if host_lock_descriptor_is "$host_lock_descriptor_path"; then
    inherited=true
  else
    exec 9>>"$host_lock_descriptor_path"
  fi
  if ! host_lock_flock; then
    printf 'another %s transaction is already running (pid %s)\n' \
      "$host_lock_label" "$(host_lock_note_field pid)" >&2
    [ "$inherited" = true ] || exec 9>&-
    return 1
  fi
  if host_lock_reject_foreign_host; then
    [ "$inherited" = true ] || exec 9>&-
    return 1
  fi
  if [ "$inherited" = true ]; then
    host_lock_state=inherited
    return 0
  fi
  host_lock_state=held
  host_lock_write_note
}

# Releases only a lock this process opened. An inherited lock belongs to the ancestor transaction.
host_lock_release() {
  [ "$host_lock_state" = held ] || return 0
  : >"$host_lock_descriptor_path"
  exec 9>&-
  host_lock_state=free
}
