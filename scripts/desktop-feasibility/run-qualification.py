#!/usr/bin/env python3
"""Run full deterministic qualification only inside a disposable hosted session."""
import argparse
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'canary/actions'))
from cell import private_command, ensure_private_directory, write_json
from desktop_diagnostics import Capture
from desktop_qualification import APPS, bounded_json, cell, digest, envelope
from desktop_suite import read_frozen_manifest

# Start from a session allowlist, rather than attempting to enumerate every
# provider, cloud, Actions or user credential an inherited environment can hold.
SESSION_ENV = {'PATH', 'HOME', 'TMPDIR', 'TMP', 'TEMP', 'USER', 'LOGNAME', 'SHELL',
               'USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'SystemRoot', 'SYSTEMROOT', 'COMSPEC', 'PATHEXT',
               'LANG', 'LC_ALL', 'DISPLAY', 'XAUTHORITY', 'DBUS_SESSION_BUS_ADDRESS',
               'XDG_RUNTIME_DIR', 'XDG_STATE_HOME', 'GITHUB_ACTIONS', 'RUNNER_ENVIRONMENT', 'RUNNER_OS'}
ZED_HELPERS = {'FEASIBILITY_ZED_INPUT_DRIVER', 'FEASIBILITY_ZED_INPUT_DRIVER_MODE',
               'FEASIBILITY_ZED_EXPORT_PARSER', 'FEASIBILITY_ZED_ZSTD',
               'FEASIBILITY_ZED_RESPONSE_METHOD', 'NANH_ZED_ICON_TEMPLATES', 'FEASIBILITY_ZED_INPUT_SCRIPT'}
HERMES_RUNTIME = {'HERMES_DESKTOP_HERMES_ROOT', 'HERMES_DESKTOP_HERMES'}


def qualification_environment(app, facts, real_nanh, executable, inherited=None):
    source = os.environ if inherited is None else inherited
    if source.get('GITHUB_ACTIONS') != 'true' or source.get('RUNNER_ENVIRONMENT') != 'github-hosted':
        raise ValueError('disposable hosted session required')
    environment = {key: value for key, value in source.items() if key in SESSION_ENV}
    environment.update(NANH_DESKTOP_QUALIFICATION_FACTS=str(facts),
                       FEASIBILITY_FACTS=str(facts), FEASIBILITY_REAL_NANH=str(real_nanh))
    if app == 'zed-desktop':
        environment.update({key: value for key, value in source.items() if key in ZED_HELPERS})
        if (environment.get('FEASIBILITY_ZED_INPUT_DRIVER_MODE') != 'paste'
                or environment.get('FEASIBILITY_ZED_RESPONSE_METHOD') != 'thread-export'
                or not all(environment.get(key) for key in ZED_HELPERS - {'NANH_ZED_ICON_TEMPLATES', 'FEASIBILITY_ZED_INPUT_SCRIPT'})):
            raise ValueError('qualification requires native input and thread export helpers')
    elif app == 'hermes-desktop':
        environment.update({key: value for key, value in source.items() if key in HERMES_RUNTIME})
        policy = source.get('FEASIBILITY_HERMES_NAMESPACE_POLICY', 'default')
        if policy not in {'default', 'scoped-apparmor-userns'}:
            raise ValueError('namespace policy is invalid')
        environment.update(FEASIBILITY_HERMES_NAMESPACE_POLICY=policy,
                           FEASIBILITY_HERMES_CDP='enabled', FEASIBILITY_HERMES_DOM_INPUT='1',
                           FEASIBILITY_HERMES_EXECUTABLE=str(executable),
                           FEASIBILITY_HERMES_DOM_DRIVER=str(Path(__file__).with_name('observe-hermes.cjs').resolve()))
    elif app in {'chatgpt-desktop', 'claude-desktop', 'pen-desktop'}:
        mode = source.get('NANH_DESKTOP_QUALIFICATION_MODE', 'renderer')
        if mode not in {'renderer', 'startup-baseline'}:
            raise ValueError('renderer mode is invalid')
        policy = source.get('NANH_DESKTOP_QUALIFICATION_NAMESPACE_POLICY', 'default')
        if policy not in {'default', 'scoped-apparmor-userns'}:
            raise ValueError('namespace policy is invalid')
        environment.update(NANH_DESKTOP_QUALIFICATION_MODE=mode,
                           NANH_DESKTOP_QUALIFICATION_NAMESPACE_POLICY=policy,
                           NANH_DESKTOP_RENDERER_APP=app,
                           NANH_DESKTOP_RENDERER_DRIVER=str(Path(__file__).with_name('observe-renderer.cjs').resolve()))
    else:
        raise ValueError('qualification backend is unavailable')
    return environment


def run(args):
    envelope(args.app, args.platform, 'aarch64' if args.platform == 'macos' else 'x86_64', args.source_sha)
    architecture = 'aarch64' if args.platform == 'macos' else 'x86_64'
    selected = cell(args.app, args.platform, architecture)
    if selected['backend'] == 'pending':
        raise ValueError('qualification backend is unavailable')
    frozen_hash = digest(args.frozen)
    manifest = read_frozen_manifest(args.frozen, [args.app], args.platform, architecture, 'qwen3.6')
    if manifest['apps'][0]['status'] != 'frozen':
        raise ValueError('official frozen application is unavailable')
    prepared = bounded_json(args.prepared)
    launcher = args.real_nanh if args.app == 'zed-desktop' else (
        Path(os.environ['FEASIBILITY_HERMES_LAUNCHER']) if args.platform == 'windows'
        else Path(__file__).with_name('nanh-shim.py'))
    if (prepared.get('schemaVersion') != 2 or prepared.get('platform') != args.platform
            or prepared.get('architecture') != architecture
            or (prepared.get('checker') or {}).get('sha256') != digest(args.checker)
            or (prepared.get('nanh') or {}).get('sha256') != digest(launcher)
            or prepared.get('frozen') != {'sha256': frozen_hash, 'model': 'qwen3.6'}):
        raise ValueError('prepared identities differ')
    apps = prepared.get('apps', [])
    if len(apps) != 1 or apps[0].get('app') != args.app or apps[0].get('blocked') is not None:
        raise ValueError('prepared app is unavailable')
    executable = (apps[0].get('executable') or {}).get('path')
    if not isinstance(executable, str) or not Path(executable).is_absolute():
        raise ValueError('prepared executable identity is missing')
    if digest(executable) != apps[0]['executable'].get('sha256'):
        raise ValueError('prepared executable changed')
    ensure_private_directory(args.directory, reusable=True)
    facts = args.directory.resolve() / 'qualification-facts'
    ensure_private_directory(facts)
    report = args.directory.resolve() / 'report.json'
    if report.exists() or report.is_symlink():
        raise ValueError('report destination already exists')
    environment = qualification_environment(args.app, facts, args.real_nanh, executable)
    expected_os = {'macos': ('macOS', 'darwin'), 'linux': ('Linux', 'linux'), 'windows': ('Windows', 'win32')}[args.platform]
    if environment.get('RUNNER_OS') != expected_os[0] or sys.platform != expected_os[1]:
        raise ValueError('host platform differs')
    command = [str(args.checker), 'run', '--app', args.app, '--model', 'qwen3.6',
               '--yes', '--non-interactive', '--ephemeral', '--mode', 'deterministic',
               '--session', 'github-hosted', '--verification', 'semantic-only',
               '--prepared', str(args.prepared), '--output', str(report)]
    capture = Capture(args.platform)
    try:
        status = private_command(command, args.directory, timeout=1200, allow_failure=True,
                                 environment=environment, diagnostic_callback=capture.observe)
        write_json(facts / "native-diagnostics.json", dict(schemaVersion=1, sourceSha=args.source_sha,
                   platform=args.platform, events=capture.events, invalidEvents=capture.invalid))
        if report.exists():
            report_hash = digest(report)
            subprocess.run([str(args.checker), 'validate-report', str(report)], env=environment,
                           timeout=30, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
            if digest(report) != report_hash:
                raise ValueError('report changed during validation')
            checked = bounded_json(report, 65536)
            if (checked.get('schemaVersion') != 3 or checked.get('platform') != args.platform
                    or checked.get('architecture') != architecture
                    or (checked.get('nanHarness') or {}).get('sha256') != digest(launcher)
                    or len(checked.get('results', [])) != 1
                    or checked['results'][0].get('app') != args.app):
                raise ValueError('validated report identity differs')
        else:
            raise ValueError('qualification report is absent')
        return 0 if status == 0 else 1
    finally:
        if digest(args.frozen) != frozen_hash:
            raise ValueError('frozen manifest changed during qualification')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', choices=APPS, required=True)
    parser.add_argument('--platform', choices=['linux', 'macos', 'windows'], required=True)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--release-tag')
    for name in ('checker', 'real-nanh', 'prepared', 'frozen', 'directory'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    for name in ('checker', 'real_nanh', 'prepared', 'frozen', 'directory'):
        setattr(args, name, getattr(args, name).absolute())
    try:
        return run(args)
    except ValueError as error:
        categories = {
            'prepared identities differ': 'prepared-identity-mismatch',
            'prepared app is unavailable': 'prepared-app-unavailable',
            'prepared executable identity is missing': 'prepared-executable-missing',
            'prepared executable changed': 'prepared-executable-changed',
            'host platform differs': 'host-platform-mismatch',
            'qualification backend is unavailable': 'backend-unavailable',
            'official frozen application is unavailable': 'frozen-app-unavailable',
            'qualification report is absent': 'report-absent',
        }
        category = categories.get(str(error), 'invalid-preflight')
        raise SystemExit('desktop qualification failed: ' + category) from None
    except (OSError, RuntimeError, subprocess.SubprocessError):
        raise SystemExit('desktop qualification failed; private evidence retained') from None


if __name__ == '__main__':
    raise SystemExit(main())
