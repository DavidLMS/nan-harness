#!/usr/bin/env bash

# Sourced before a publication entrypoint mutates remote state. GitHub Actions
# owns the normal writer; Tart is an explicit operational switchover, not a
# parallel fallback. A missing GitHub response never means a writer is idle.
require_publication_writer() {
  case "${NAN_CANARY_WRITER:-}" in
    actions)
      [ "${GITHUB_ACTIONS:-}" = true ] || {
        printf 'the Actions publication context is missing\n' >&2
        return 1
      }
      ;;
    tart-emergency)
      python3 "$repository_root/canary/actions/emergency.py" --repository "$release_repository"
      ;;
    *)
      printf 'publication requires the central Actions writer or explicit Tart emergency mode\n' >&2
      return 1
      ;;
  esac
}
