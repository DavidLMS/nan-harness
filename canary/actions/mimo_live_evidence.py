#!/usr/bin/env python3
"""Verify a completed supported MiMo task without reconstructing console output."""
import os
from pathlib import Path
import sys

from mimo_live_observation import noninteractive_prompt
from mimo_session_observation import collect


EVIDENCE = 'mimo-0.1.16-native-session-empty-stdout'
DETAIL = ('MiMo 0.1.16: live task verified from its exact native session; '
          'stdout was empty. Headless console output remains an upstream limitation.')

VERSIONS = frozenset(('0.1.16', '0.1.17'))
EVIDENCE_BY_VERSION = {version: f'mimo-{version}-native-session-empty-stdout' for version in VERSIONS}
DETAIL_BY_EVIDENCE = {evidence: DETAIL.replace('0.1.16', version)
                      for version, evidence in EVIDENCE_BY_VERSION.items()}


def main():
    if (len(sys.argv) != 3 or os.isatty(0)
            or os.environ.get('NAN_CANARY_MIMO_SESSION_EVIDENCE') not in VERSIONS):
        return 1
    prompt = noninteractive_prompt(os.environ.get('NAN_CANARY_MIMO_EXPECTED_PROMPT', ''))
    value = collect(Path(sys.argv[1]), sys.argv[2], prompt, require_live_task=True)
    return 0 if value['status'] == 'observed' else 1


if __name__ == '__main__':
    raise SystemExit(main())
