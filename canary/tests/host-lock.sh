#!/usr/bin/env bash
set -euo pipefail

# Covers the shared publication-host lock with real concurrent processes: two live writers, an
# owner that is killed outright, a nested transaction that re-enters the lock it already holds,
# and the boundaries the protocol refuses to cross. Nothing here mocks the lock itself.

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repository_root/canary/tests/host-lock-fixture.sh"
helper="$repository_root/canary/host/host-lock.sh"

temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
lock="$temporary_directory/state/publication.lock"
lock_fixture_prepare "$temporary_directory/fixture"

# Acquires, reports, and releases immediately. With NESTED_LOCK set it acquires twice, which is
# how the release gate invokes the feed publisher inside its own transaction.
cat >"$temporary_directory/writer.sh" <<'WRITER'
set -euo pipefail
source "$1"
host_lock_acquire "$2" 'publication test' || exit 3
if [ -n "${NESTED_LOCK:-}" ]; then
  nested="$NESTED_LOCK"
  NESTED_LOCK='' bash "$nested" "$1" "$2" || exit 4
fi
printf 'acquired\n'
host_lock_release
WRITER

write_attempt() {
  set +e
  env "$@" bash "$temporary_directory/writer.sh" "$helper" "$lock" \
    >"$temporary_directory/out" 2>&1
  attempt_status=$?
  set -e
}

# A single writer acquires and releases, and leaves no note behind.
write_attempt
[ "$attempt_status" -eq 0 ]
grep -Fqx acquired "$temporary_directory/out"
[ ! -s "$lock" ]

# Two live processes: the second is refused for as long as the first is alive, and the refusal
# names the live owner rather than reclaiming its lock.
lock_fixture_hold "$helper" "$lock"
write_attempt
[ "$attempt_status" -eq 3 ]
grep -Fq "already running (pid $lock_fixture_holder_pid)" "$temporary_directory/out"
kill -0 "$lock_fixture_holder_pid"
lock_fixture_release

# The lock is free again the moment the owner releases it.
write_attempt
[ "$attempt_status" -eq 0 ]

# An owner killed outright releases the lock without any staleness window, and the note it left
# behind is not treated as an obstacle. This kills the process that owns the lock, not a child.
lock_fixture_hold "$helper" "$lock"
lock_fixture_kill
[ -s "$lock" ]
write_attempt
[ "$attempt_status" -eq 0 ]

# A nested transaction re-enters the lock its caller holds, and the caller keeps it: the inner
# release must not hand the lock to anyone else.
write_attempt "NESTED_LOCK=$temporary_directory/writer.sh"
[ "$attempt_status" -eq 0 ]

# An environment marker naming the lock proves nothing. Only the inherited descriptor does, and an
# unrelated process does not have one while a live owner holds the lock.
lock_fixture_hold "$helper" "$lock"
write_attempt "NAN_CANARY_RELEASE_CHANNEL_LOCK=$lock" "HOST_LOCK_PATH=$lock"
[ "$attempt_status" -eq 3 ]
lock_fixture_release

# A note recording another host is refused rather than reclaimed.
printf '{"pid":1,"host":"other-publication-host","startedAt":0}\n' >"$lock"
write_attempt
[ "$attempt_status" -eq 3 ]
grep -Fq 'belongs to another host: other-publication-host' "$temporary_directory/out"

# A lock directory left behind by the previous protocol is reported, not silently worked around.
rm -f "$lock"
mkdir -p "$lock"
write_attempt
[ "$attempt_status" -eq 3 ]
grep -Fq 'lock directory from the previous protocol' "$temporary_directory/out"
rmdir "$lock"

# Two processes racing for a lock whose owner has died: exactly one of them enters.
lock_fixture_hold "$helper" "$lock"
lock_fixture_kill
mkfifo "$temporary_directory/race"
cat >"$temporary_directory/racer.sh" <<'RACER'
set -euo pipefail
source "$1"
read -r _ <"$3" || true
host_lock_acquire "$2" 'publication test' || exit 3
touch "$4"
sleep 0.5
host_lock_release
RACER
for racer in first second; do
  bash "$temporary_directory/racer.sh" "$helper" "$lock" "$temporary_directory/race" \
    "$temporary_directory/won-$racer" >/dev/null 2>&1 &
done
sleep 0.3
printf 'go\ngo\n' >"$temporary_directory/race"
wait
winners="$(ls "$temporary_directory" | grep -c '^won-' || true)"
[ "$winners" -eq 1 ] || {
  printf 'exactly one racer must enter the transaction, not %s\n' "$winners" >&2
  exit 1
}

printf 'canary/tests/host-lock.sh passed\n'
