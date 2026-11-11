#!/usr/bin/env python3
"""Exercise isolated APT commands without network access or administrator rights."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / '.github/scripts/ubuntu-desktop-packages.sh'


class DesktopPackageTests(unittest.TestCase):
    def execute(self, failure='none', host=True, packages=('xvfb', 'python3-dbus')):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binaries = root / 'bin'
            binaries.mkdir()
            (binaries / 'sudo').write_text('#!/bin/sh\nexec "$@"\n')
            (binaries / 'timeout').write_text('''#!/bin/sh
[ "$1" = '--kill-after=10s' ] && [ "$2" = '300s' ] || exit 91
[ "$FAILURE" != timeout ] || exit 124
shift 2
exec "$@"
''')
            (binaries / 'apt-get').write_text('#!' + sys.executable + '\n' + '''import json,os,sys
from pathlib import Path
args=sys.argv[1:]
options=dict(args[i+1].split('=',1) for i,value in enumerate(args) if value=='-o')
operation='update' if 'update' in args else 'install'
lists=Path(options['Dir::State::lists'])
source=Path(options['Dir::Etc::sourcelist'])
parts=Path(options['Dir::Etc::sourceparts'])
record={'operation':operation,'options':options,'sources':source.read_text(),
        'emptySourceparts':not list(parts.iterdir()),'freshLists':not list(lists.iterdir()),
        'errorOnAny':'--error-on=any' in args,'arguments':args}
with open(os.environ['COMMANDS'],'a') as output: output.write(json.dumps(record)+'\\n')
if operation=='update':
 if os.environ['FAILURE']=='update':
  print('W: synthetic incomplete index update',file=sys.stderr)
  raise SystemExit(100)
 (lists/'verified-index').write_text('complete')
else:
 assert (lists/'verified-index').is_file()
 if os.environ['FAILURE']=='install': raise SystemExit(100)
''')
            for binary in binaries.iterdir():
                binary.chmod(0o700)
            old = root / 'old-lists'
            old.mkdir()
            (old / 'azure-index').write_text('must remain unused')
            commands = root / 'commands.jsonl'
            environment = {**os.environ, 'PATH': str(binaries) + os.pathsep + os.environ['PATH'],
                           'RUNNER_TEMP': str(root), 'COMMANDS': str(commands), 'FAILURE': failure}
            invocation = 'source "$1"; shift; validate_desktop_package_host() { return ' + ('0' if host else '1') + '; }; install_desktop_packages "$@"'
            result = subprocess.run(['bash', '-c', invocation, 'fixture', str(SCRIPT), *packages],
                                    env=environment, capture_output=True, text=True)
            records = [json.loads(line) for line in commands.read_text().splitlines()] if commands.exists() else []
            self.assertFalse(list(root.glob('nan-desktop-apt.*')))
            self.assertEqual((old / 'azure-index').read_text(), 'must remain unused')
            return result, records

    def test_install_uses_only_the_complete_fresh_signed_metadata(self):
        result, records = self.execute()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([r['operation'] for r in records], ['update', 'install'])
        self.assertEqual(records[0]['options'], records[1]['options'])
        self.assertTrue(records[0]['freshLists'] and records[0]['errorOnAny'])
        self.assertTrue(all(r['emptySourceparts'] for r in records))
        self.assertNotIn('azure', records[0]['sources'])
        self.assertIn('https://archive.ubuntu.com/ubuntu noble ', records[0]['sources'])
        self.assertIn('https://security.ubuntu.com/ubuntu noble-security ', records[0]['sources'])
        self.assertEqual(records[0]['sources'].count('signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg'), 4)
        self.assertNotIn('trusted=yes', records[0]['sources'])
        self.assertNotIn('AllowInsecureRepositories', str(records))
        self.assertEqual(records[1]['arguments'][-2:], ['xvfb', 'python3-dbus'])

    def test_incomplete_update_and_timeout_never_run_install(self):
        for failure in ('update', 'timeout'):
            result, records = self.execute(failure)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn('install', [r['operation'] for r in records])

    def test_install_failure_is_not_replayed(self):
        result, records = self.execute('install')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual([r['operation'] for r in records], ['update', 'install'])

    def test_wrong_host_and_option_injection_never_invoke_apt(self):
        for host, packages in ((False, ('xvfb',)), (True, ('--allow-unauthenticated',)), (True, ())):
            result, records = self.execute(host=host, packages=packages)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(records, [])


if __name__ == '__main__':
    unittest.main()
