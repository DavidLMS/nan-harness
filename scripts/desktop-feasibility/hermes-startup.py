"""Closed, bounded startup evidence; raw child stderr never leaves memory."""
import json
import os
from pathlib import Path
import re
import threading

CAPTURE_LIMIT = 65536
PATTERNS = (
    ('sandbox-helper', rb'SUID sandbox helper binary|chrome-sandbox.*(?:4755|configured correctly)'),
    ('namespace-denied', rb'Failed to move to new namespace|Failed to unshare|No usable sandbox|apparmor.*DENIED'),
    ('root-without-sandbox', rb'Running as root without --no-sandbox'),
    ('display-unavailable', rb'Missing X server|cannot open display|Unable to open X display|Failed to connect to.*(?:X11|Wayland)'),
    ('missing-library', rb'error while loading shared libraries'),
    ('gpu-fatal', rb'GPU process isn.t usable|GPU process launch failed|FATAL[^\n]*gpu'),
    ('native-module', rb'NODE_MODULE_VERSION|Cannot find module[^\n]*\.node|invalid ELF header'),
)


def classify(data):
    return next((name for name, pattern in PATTERNS if re.search(pattern, data, re.I)), 'unclassified')


class Capture:
    def __init__(self, stream):
        self.stream = stream
        self.data = bytearray()
        self.total = 0
        self.complete = False
        self.thread = threading.Thread(target=self.drain, daemon=True)

    def drain(self):
        try:
            while True:
                block = self.stream.read(4096)
                if not block:
                    self.complete = True
                    break
                self.total += len(block)
                self.data.extend(block[:max(0, CAPTURE_LIMIT - len(self.data))])
        except (OSError, ValueError):
            pass

    def start(self):
        self.thread.start()

    def save(self, destination, exit_code, join_timeout=2):
        self.thread.join(timeout=join_timeout)
        policy = os.environ.get('FEASIBILITY_HERMES_NAMESPACE_POLICY', 'default')
        if policy not in ('default', 'scoped-apparmor-userns'):
            policy = 'default'
        facts = {'schemaVersion': 1, 'mechanism': 'hermes-startup', 'namespacePolicy': policy,
                 'disableSetuidSandbox': policy == 'scoped-apparmor-userns',
                 'startupCategory': classify(bytes(self.data)) if self.complete and self.total <= CAPTURE_LIMIT else 'unclassified',
                 'stderrPresent': self.total > 0, 'captureTruncated': self.total > CAPTURE_LIMIT,
                 'drainComplete': self.complete, 'launcherExitCode': exit_code,
                 'effectiveUserIsRoot': os.geteuid() == 0 if hasattr(os, 'geteuid') else None}
        for name in ('apparmor_restrict_unprivileged_userns', 'unprivileged_userns_clone'):
            path = Path('/proc/sys/kernel') / name
            try:
                value = path.read_text().strip()
                facts[name] = int(value) if value in ('0', '1') else None
            except OSError:
                facts[name] = None
        executable = os.environ.get('FEASIBILITY_HERMES_EXECUTABLE')
        facts['sandboxHelperPresent'] = None
        facts['sandboxHelperOwnerIsRoot'] = None
        facts['sandboxHelperModeIs4755'] = None
        if executable:
            try:
                stat = (Path(executable).parent / 'chrome-sandbox').stat()
                facts.update(sandboxHelperPresent=True, sandboxHelperOwnerIsRoot=stat.st_uid == 0,
                             sandboxHelperModeIs4755=(stat.st_mode & 0o7777) == 0o4755)
            except FileNotFoundError:
                facts['sandboxHelperPresent'] = False
            except OSError:
                pass
        destination = Path(destination)
        temporary = destination.with_suffix('.tmp')
        with temporary.open('w') as output:
            os.chmod(temporary, 0o600)
            json.dump(facts, output)
            output.write('\n')
        temporary.replace(destination)
