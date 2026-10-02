#!/usr/bin/env python3
"""Fixed native key transport; the checker proves foreground before and after."""
import subprocess
import sys

KEYS = {'new-thread': 'ctrl+alt+n', 'copy-thread': 'ctrl+alt+y',
        'select-all': 'ctrl+a', 'copy': 'ctrl+c', 'paste': 'ctrl+v',
        'right': 'Right', 'submit': 'Return'}


def main():
    if len(sys.argv) != 2 or sys.argv[1] not in KEYS or sys.stdin.buffer.read(1):
        return 2
    try:
        return subprocess.run(['/usr/bin/xdotool', 'key', '--clearmodifiers', KEYS[sys.argv[1]]],
                              timeout=2, stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL, check=False).returncode
    except (OSError, subprocess.TimeoutExpired):
        return 3


if __name__ == '__main__':
    raise SystemExit(main())
