#!/usr/bin/env python3
"""Run only the synthetic writer fixture and publish fixed diagnostic categories."""
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

TEST = 'commands::claude_desktop::session::configuration_persist_tests::production_std_rename_lifecycle_under_precreation_leases_restores_documents'


def classify(output, succeeded):
    started = any(line.startswith(('test ' + TEST + ' ...').encode()) for line in output.splitlines())
    if len(output) > 131072:
        category = 'output-overflow'
    elif succeeded:
        category = 'passed'
    elif b'could not compile' in output:
        category = 'compile-failure'
    elif b'could not execute process' in output:
        category = 'test-process-unavailable'
    elif started and b'os error 32' in output:
        category = 'fixture-panic-sharing-violation'
    elif started and b'os error 5' in output:
        category = 'fixture-panic-access-denied'
    elif started:
        category = 'fixture-failure'
    else:
        category = 'unclassified-failure'
    return dict(fixtureStarted=started, category=category)


def main():
    expected = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Windows',
                    NANH_CONFIGURATION_POSTINSTALL_OBSERVATION='1')
    if any(os.environ.get(key) != value for key, value in expected.items()) or os.name != 'nt':
        raise ValueError('hosted Windows synthetic fixture required')
    source = os.environ['GITHUB_SHA']
    if re.fullmatch('[0-9a-fA-F]{40}', source) is None:
        raise ValueError('fixture commit required')
    environment = {key: value for key, value in os.environ.items()
                   if key not in {'NAN_API_KEY', 'GH_TOKEN', 'GITHUB_TOKEN'}}
    command = ['cargo', 'test', '--locked', '-p', 'nan-harness-cli', '--features',
               'desktop-qualification', '--lib', TEST, '--', '--exact']
    succeeded = False
    with tempfile.TemporaryFile() as private:
        try:
            result = subprocess.run(command, env=environment, stdout=private, stderr=subprocess.STDOUT,
                                    stdin=subprocess.DEVNULL, timeout=180, check=False)
            succeeded = result.returncode == 0
            private.seek(0)
            observation = classify(private.read(131073), succeeded)
        except subprocess.TimeoutExpired:
            observation = dict(fixtureStarted=False, category='driver-deadline')
        except OSError:
            observation = dict(fixtureStarted=False, category='driver-spawn-failure')
    facts = dict(schemaVersion=1, mechanism='windows-configuration-fixture-driver', diagnosticsOnly=True,
                 sourceSha=source, phase='after-installation', **observation)
    with (Path(os.environ['RUNNER_TEMP']) / 'configuration-fixture-driver.json').open('x') as output:
        json.dump(facts, output)
    return 0 if succeeded else 1


if __name__ == '__main__':
    raise SystemExit(main())
