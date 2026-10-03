#!/usr/bin/env python3
"""Run full deterministic qualification only inside a disposable hosted session."""
import argparse
import hashlib
import json
import struct
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
WINDOWS_PROOF = {'FEASIBILITY_WINDOWS_PROOF_PYTHON', 'FEASIBILITY_WINDOWS_PROOF_SCRIPT'}
HERMES_RUNTIME = {'HERMES_DESKTOP_HERMES_ROOT', 'HERMES_DESKTOP_HERMES'}
CLAUDE_WINDOWS_BOOTSTRAP_SHA256 = '97910a0668710a80315da1ab12d215c4b8d58d9f159886493e37848feb92351e'
CLAUDE_BOOTSTRAP_SHA256 = '83126565df48e98691a3845f27bb7ee78d0632aa14b5881adad8c7ac4f0a3adf'


def validate_claude_bundle(executable):
    # Prepared/frozen receipts bind bytes; this additionally rejects arbitrary
    # adopted binaries and ensures the direct official bundle entry point.
    if (executable.is_symlink() or not executable.is_file()
            or executable.name != 'Claude' or executable.parent.name != 'MacOS'
            or executable.parent.parent.name != 'Contents'
            or executable.parent.parent.parent.name != 'Claude.app'
            or executable.resolve() != executable):
        raise ValueError('Claude native bundle is invalid')
    contents = executable.parent.parent
    for relative in ('Info.plist', 'Resources/app.asar'):
        document = contents / relative
        if document.is_symlink() or not document.is_file() or document.resolve() != document:
            raise ValueError('Claude native bundle is invalid')

    asar = contents / 'Resources/app.asar'
    with asar.open('rb') as archive:
        prefix = archive.read(16)
        if len(prefix) != 16:
            raise ValueError('Claude bootstrap is invalid')
        size_payload, header_size, header_payload, json_size = struct.unpack('<4I', prefix)
        if (size_payload != 4 or not 0 < json_size <= 16 * 1024 * 1024
                or header_payload != header_size - 4
                or header_size != 8 + (json_size + 3) // 4 * 4):
            raise ValueError('Claude bootstrap is invalid')
        header = json.loads(archive.read(json_size))
        entry = header['files']['.vite']['files']['build']['files']['index.pre.js']
        size, offset = entry.get('size'), entry.get('offset')
        if (type(size) is not int or not 0 < size <= 16 * 1024 * 1024
                or type(offset) is not str or not offset.isascii() or not offset.isdecimal()
                or len(offset) > 20 or entry.get('unpacked') is True or 'link' in entry):
            raise ValueError('Claude bootstrap is invalid')
        archive.seek(8 + header_size + int(offset))
        bootstrap = archive.read(size)
        if len(bootstrap) != size or hashlib.sha256(bootstrap).hexdigest() != CLAUDE_BOOTSTRAP_SHA256:
            raise ValueError('Claude bootstrap differs from the inspected release')


CODEX_PROJECT_RELEASES = {
    'macos': ('bfda661a7c9ca44dac3168134058dd6007947cde318ade37d570c484329f6d41',
              'b078df75c1cf593b99351622f5bc8184a44f993bb0bd0a4cbefac31f4a746bbd'),
    'windows': ('f7b0266d6c00d4743da01d62bc82488f7ec5560c642501758119cb9885f67c87',
                'b35bf062c01d73da090c60e62186dc180c2a8545cb6fc9575b4403c8fa3db49e'),
    'linux': ('e0174d8d0a5f4141145458c814f3c2d863dd67e942b868785a1f5dac9cba3e16',
              '207c4fbff7e2fcc1b0789448351ac6eed206206d94c5a0835e5f07c7cd73d6e3'),
}


def validate_codex_project_release(release, executable_hash, platform='windows'):
    # Native flags and renderer controls were inspected in each platform's bytes.
    pinned = CODEX_PROJECT_RELEASES.get(platform)
    if (pinned is None or release.get('version') != '26.930.31730'
            or release.get('digest') != 'sha256:' + pinned[0]
            or executable_hash != pinned[1]):
        raise ValueError('Codex project trial requires the inspected official release')


def validate_claude_windows_bundle(executable):
    # The pinned MSIX stores app/claude.exe; Windows names are case-insensitive.
    # Canonical identity and bootstrap bytes remain independent requirements.
    if (not executable.is_absolute() or executable.name.casefold() != 'claude.exe' or executable.parent.name.casefold() != 'app'
            or executable.is_symlink() or not executable.is_file() or executable.resolve() != executable):
        raise ValueError('claude-windows-executable-invalid')
    asar = executable.parent / 'resources/app.asar'
    if asar.is_symlink() or not asar.is_file() or asar.resolve() != asar:
        raise ValueError('claude-windows-bootstrap-invalid')
    try:
        with asar.open('rb') as archive:
            header = archive.read(16)
            if len(header) != 16:
                raise ValueError('claude-windows-bootstrap-invalid')
            _, header_size, _, json_size = struct.unpack('<4I', header)
            if not 0 < json_size <= 1024 * 1024 or header_size < json_size:
                raise ValueError('claude-windows-bootstrap-invalid')
            metadata = json.loads(archive.read(json_size))
            entry = metadata['files']['.vite']['files']['build']['files']['index.pre.js']
            size, offset = entry['size'], int(entry['offset'])
            if type(size) is not int or not 0 < size <= 2 * 1024 * 1024 or offset < 0:
                raise ValueError('claude-windows-bootstrap-invalid')
            archive.seek(8 + header_size + offset)
            data = archive.read(size)
            if len(data) != size or hashlib.sha256(data).hexdigest() != CLAUDE_WINDOWS_BOOTSTRAP_SHA256:
                raise ValueError('claude-windows-bootstrap-mismatch')
    except (KeyError, TypeError, json.JSONDecodeError, struct.error, OverflowError):
        raise ValueError('claude-windows-bootstrap-invalid') from None


def qualification_environment(app, facts, real_nanh, executable, inherited=None):
    source = os.environ if inherited is None else inherited
    if source.get('GITHUB_ACTIONS') != 'true' or source.get('RUNNER_ENVIRONMENT') != 'github-hosted':
        raise ValueError('disposable hosted session required')
    if source.get('NANH_ZED_CURSOR_HIT') is not None and app != 'zed-desktop':
        raise ValueError('Zed cursor hit trial is unavailable')
    if source.get('NANH_CLAUDE_MAC_NATIVE_CHAT') is not None and app != 'claude-desktop':
        raise ValueError('Claude native Chat controller is unavailable')
    if source.get('NANH_CLAUDE_MAC_CHAT_NAVIGATION') is not None and app != 'claude-desktop':
        raise ValueError('Claude native Chat navigation is unavailable')
    layout = source.get('NANH_ZED_LAYOUT_POLICY')
    if layout is not None and (app != 'zed-desktop' or source.get('RUNNER_OS') != 'Linux' or layout != 'zoom-before-send'):
        raise ValueError('Zed layout trial is unavailable')
    retry_method = source.get('NANH_ZED_RETRY_METHOD')
    if retry_method is not None and (app != 'zed-desktop' or source.get('RUNNER_OS') != 'Linux' or retry_method != 'atspi-click'):
        raise ValueError('Zed native Retry trial is unavailable')
    zoom = source.get('NANH_ZED_PANEL_ZOOM')
    if zoom is not None and (app != 'zed-desktop' or source.get('RUNNER_OS') != 'Linux' or zoom != 'observe'):
        raise ValueError('Zed panel zoom diagnostic is unavailable')
    if source.get('NANH_CODEX_PROJECT_POLICY') is not None and app != 'chatgpt-desktop':
        raise ValueError('Codex native project policy is unavailable')
    environment = {key: value for key, value in source.items() if key in SESSION_ENV}
    if source.get('RUNNER_OS') == 'Windows':
        for key in WINDOWS_PROOF:
            value = source.get(key)
            if not value:
                raise ValueError('Windows ownership helper is missing')
            path = Path(value)
            if not path.is_absolute() or not path.is_file() or path.is_symlink():
                raise ValueError('Windows ownership helper is invalid')
            environment[key] = value
    environment.update(NANH_DESKTOP_QUALIFICATION_FACTS=str(facts),
                       FEASIBILITY_FACTS=str(facts), FEASIBILITY_REAL_NANH=str(real_nanh))
    if app == 'zed-desktop':
        environment.update({key: value for key, value in source.items() if key in ZED_HELPERS})
        if layout is not None:
            environment['NANH_ZED_LAYOUT_POLICY'] = layout
        if zoom is not None:
            environment['NANH_ZED_PANEL_ZOOM'] = zoom
        if retry_method is not None:
            environment['NANH_ZED_RETRY_METHOD'] = retry_method
        cursor = source.get('NANH_ZED_CURSOR_HIT')
        if cursor is not None:
            if cursor != '1' or source.get('RUNNER_OS') != 'Linux':
                raise ValueError('Zed cursor hit trial is unavailable')
            environment['NANH_ZED_CURSOR_HIT'] = cursor
        delivery = source.get('NANH_ZED_XRECORD')
        if delivery is not None:
            if delivery != '1' or source.get('RUNNER_OS') != 'Linux':
                raise ValueError('Zed delivery diagnostic is unavailable')
            environment['NANH_ZED_XRECORD'] = delivery
        if (environment.get('FEASIBILITY_ZED_INPUT_DRIVER_MODE') != 'paste'
                or environment.get('FEASIBILITY_ZED_RESPONSE_METHOD') != 'thread-export'
                or not all(environment.get(key) for key in ZED_HELPERS - {'NANH_ZED_ICON_TEMPLATES', 'FEASIBILITY_ZED_INPUT_SCRIPT'})):
            raise ValueError('qualification requires native input and thread export helpers')
    elif app == 'hermes-desktop':
        environment.update({key: value for key, value in source.items() if key in HERMES_RUNTIME})
        policy = source.get('FEASIBILITY_HERMES_NAMESPACE_POLICY', 'default')
        if policy not in {'default', 'scoped-apparmor-userns'}:
            raise ValueError('namespace policy is invalid')
        readiness = source.get('FEASIBILITY_HERMES_READINESS_POLICY')
        if readiness is not None:
            if readiness != 'current-catalog' or source.get('RUNNER_OS') != 'Windows':
                raise ValueError('Hermes readiness policy is unavailable')
            # nANH writes active-profile.json with the owned nan profile and
            # Electron pins that profile before spawning its local backend.
            environment.update(FEASIBILITY_HERMES_READINESS_POLICY=readiness,
                               FEASIBILITY_HERMES_CATALOG_PROFILE='nan')
        environment.update(FEASIBILITY_HERMES_NAMESPACE_POLICY=policy,
                           FEASIBILITY_HERMES_CDP='enabled', FEASIBILITY_HERMES_DOM_INPUT='1',
                           FEASIBILITY_HERMES_EXECUTABLE=str(executable),
                           FEASIBILITY_HERMES_DOM_DRIVER=str(Path(__file__).with_name('observe-hermes.cjs').resolve()))
    elif app in {'chatgpt-desktop', 'claude-desktop', 'pen-desktop'}:
        mode = source.get('NANH_DESKTOP_QUALIFICATION_MODE', 'renderer')
        if mode not in {'renderer', 'startup-baseline'}:
            raise ValueError('renderer mode is invalid')
        native_chat = source.get('NANH_CLAUDE_MAC_NATIVE_CHAT')
        if native_chat is not None:
            if (native_chat != '1' or app != 'claude-desktop' or source.get('RUNNER_OS') != 'macOS'
                    or source.get('NANH_CLAUDE_MAC_PROFILE_POLICY') != 'native-known-folders'):
                raise ValueError('Claude native Chat controller is unavailable')
            # Full native Chat uses the same isolated known-folder acquisition,
            # without admitting Claude's packaged renderer to CDP.
            mode = 'startup-baseline'
            environment['NANH_CLAUDE_MAC_NATIVE_CHAT'] = native_chat

        onboarding = source.get('NANH_CODEX_PUBLIC_ONBOARDING')
        if onboarding is not None:
            if (onboarding != 'engineering' or app != 'chatgpt-desktop'
                    or source.get('RUNNER_OS') not in {'Windows', 'Linux', 'macOS'} or mode != 'renderer'):
                raise ValueError('public onboarding diagnostic is unavailable')
            environment['NANH_CODEX_PUBLIC_ONBOARDING'] = onboarding
        project_policy = source.get('NANH_CODEX_PROJECT_POLICY')
        if project_policy is not None:
            if (project_policy != 'open-project' or app != 'chatgpt-desktop'
                    or source.get('RUNNER_OS') not in {'Windows', 'Linux', 'macOS'} or mode != 'renderer'
                    or onboarding != 'engineering'):
                raise ValueError('Codex native project policy is unavailable')
            environment['NANH_CODEX_PROJECT_POLICY'] = project_policy
        profile_policy = source.get('NANH_CLAUDE_MAC_PROFILE_POLICY')
        if profile_policy is not None:
            if (profile_policy not in {'electron-user-data-dir', 'native-known-folders'} or app != 'claude-desktop'
                    or source.get('RUNNER_OS') != 'macOS' or mode != 'startup-baseline'):
                raise ValueError('Claude native profile policy is unavailable')
            validate_claude_bundle(Path(executable))
            environment['NANH_CLAUDE_MAC_PROFILE_POLICY'] = profile_policy
        navigation = source.get('NANH_CLAUDE_MAC_CHAT_NAVIGATION')
        if navigation is not None:
            if (navigation != '1' or app != 'claude-desktop'
                    or source.get('RUNNER_OS') != 'macOS' or mode != 'startup-baseline'
                    or profile_policy != 'native-known-folders'):
                raise ValueError('Claude native Chat navigation is unavailable')
            environment['NANH_CLAUDE_MAC_CHAT_NAVIGATION'] = navigation
        mcp_fixture = source.get('NANH_CLAUDE_MCP_FIXTURE')
        if mcp_fixture is not None:
            if (mcp_fixture != 'read-only' or app != 'claude-desktop'
                    or source.get('RUNNER_OS') != 'macOS' or mode != 'startup-baseline'
                    or profile_policy != 'native-known-folders' or navigation != '1'):
                raise ValueError('Claude read fixture policy is unavailable')
            script = Path(__file__).with_name('claude-read-fixture.py').resolve(strict=True)
            source_hash = 'ecb56f97d549f3040908f1bb8f0bb32235f9b48d9572ea348098135fe7999fc0'
            if digest(script) != source_hash:
                raise ValueError('Claude read fixture source differs')
            environment.update(NANH_CLAUDE_MCP_FIXTURE=mcp_fixture,
                               NANH_CLAUDE_MCP_PYTHON=str(Path(sys.executable).resolve(strict=True)),
                               NANH_CLAUDE_MCP_SCRIPT=str(script),
                               NANH_CLAUDE_MCP_SOURCE_SHA256=source_hash)
        windows_profile = source.get('NANH_CLAUDE_WINDOWS_PROFILE_POLICY')
        if windows_profile is not None:
            if (windows_profile != 'private-env' or app != 'claude-desktop'
                    or source.get('RUNNER_OS') != 'Windows' or mode != 'startup-baseline'):
                raise ValueError('claude-windows-policy-invalid')
            validate_claude_windows_bundle(Path(executable))
            environment['NANH_CLAUDE_WINDOWS_PROFILE_POLICY'] = windows_profile
        if app == 'chatgpt-desktop':
            environment['FEASIBILITY_CODEX_DOM_DRIVER'] = str(Path(__file__).with_name('codex-dom.cjs').resolve())
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
    if os.environ.get('NANH_CLAUDE_MAC_PROFILE_POLICY') is not None:
        release = manifest['apps'][0]
        if (args.app != 'claude-desktop' or args.platform != 'macos'
                or release.get('version') != '2.19675.0'
                or release.get('digest') != 'sha256:86f1460ca694313223a0b524da4f411bbccf6531c2271698d1ffc29a2131e392'):
            raise ValueError('Claude native profile trial requires the inspected official release')
    if os.environ.get('NANH_CLAUDE_WINDOWS_PROFILE_POLICY') is not None:
        release = manifest['apps'][0]
        if (args.app != 'claude-desktop' or args.platform != 'windows'
                or release.get('version') != '2.19675.0'
                or release.get('digest') != 'sha256:8355c3d28aa08d2e8d841596b41abce0a4fc3805cc9d74f6836d42176ab6b0f9'):
            raise ValueError('claude-windows-release-mismatch')
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
    release = manifest['apps'][0]
    if (args.app == 'claude-desktop' and args.platform == 'linux'
            and release.get('version') == '2.9939.4'
            and release.get('digest') == 'sha256:3cfddb23bf2911e05e27b4ed3856b8e795df94643b2c35b59deb317cf995bca0'
            and environment.get('NANH_DESKTOP_QUALIFICATION_MODE') == 'startup-baseline'):
        environment['NANH_CLAUDE_LINUX_SOURCE_POLICY'] = 'official-2.9939.4'
    if environment.get('NANH_CODEX_PROJECT_POLICY') is not None:
        if args.app != 'chatgpt-desktop' or args.platform not in CODEX_PROJECT_RELEASES:
            raise ValueError('Codex native project trial platform differs')
        validate_codex_project_release(manifest['apps'][0], digest(Path(executable)), args.platform)
        environment['NANH_CODEX_PROJECT_ARTIFACT_SHA256'] = CODEX_PROJECT_RELEASES[args.platform][0]
    expected_os = {'macos': ('macOS', 'darwin'), 'linux': ('Linux', 'linux'), 'windows': ('Windows', 'win32')}[args.platform]
    if environment.get('RUNNER_OS') != expected_os[0] or sys.platform != expected_os[1]:
        raise ValueError('host platform differs')
    command = [str(args.checker), 'run', '--app', args.app, '--model', 'qwen3.6',
               '--yes', '--non-interactive', '--ephemeral', '--mode', 'deterministic',
               '--session', 'github-hosted', '--verification', 'semantic-only',
               '--prepared', str(args.prepared), '--output', str(report)]
    try:
        status = execute_with_diagnostics(command, args.directory, facts, args.platform,
                                          args.source_sha, environment)
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


def execute_with_diagnostics(command, directory, facts, platform, source_sha, environment):
    """Publish closed observations even when executor cleanup or its deadline fails."""
    capture = Capture(platform)
    completed = False
    try:
        status = private_command(command, directory, timeout=1200, allow_failure=True,
                                 environment=environment, diagnostic_callback=capture.observe)
        completed = True
        return status
    finally:
        try:
            write_json(facts / 'native-diagnostics.json', dict(schemaVersion=1, sourceSha=source_sha,
                       platform=platform, events=capture.events, invalidEvents=capture.invalid))
        except Exception:
            # Publication must not replace an already pending executor failure.
            # Without that failure, a publication error remains an error.
            if completed:
                raise


def publish_runner_failure(args, category):
    """Replace only the same-commit pending envelope with a closed failure fact."""
    if (os.environ.get('GITHUB_ACTIONS') != 'true'
            or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted'):
        return
    try:
        directory = args.directory
        if not directory.is_absolute() or any(p.is_symlink() or not p.is_dir()
                or getattr(p, 'is_junction', lambda: False)() for p in (directory, *directory.parents)):
            return
        architecture = 'aarch64' if args.platform == 'macos' else 'x86_64'
        pending = envelope(args.app, args.platform, architecture, args.source_sha)
        output = directory / 'qualification.json'
        if bounded_json(output, 65536) != pending:
            return
        fact = dict(schemaVersion=1, mechanism='qualification-runner-failure',
                    diagnosticsOnly=True, errorCategory=category)
        from desktop_qualification import RUNNER_FAILURES
        if category not in RUNNER_FAILURES:
            return
        pending['semanticObservations'] = [fact]
        write_json(output, pending)
    except (OSError, ValueError, TypeError, RuntimeError):
        # A diagnostic never replaces the original execution failure.
        return


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
            'Windows ownership helper is missing': 'windows-ownership-helper-missing',
            'Windows ownership helper is invalid': 'windows-ownership-helper-invalid',
            'prepared identities differ': 'prepared-identity-mismatch',
            'prepared app is unavailable': 'prepared-app-unavailable',
            'prepared executable identity is missing': 'prepared-executable-missing',
            'prepared executable changed': 'prepared-executable-changed',
            'host platform differs': 'host-platform-mismatch',
            'qualification backend is unavailable': 'backend-unavailable',
            'official frozen application is unavailable': 'frozen-app-unavailable',
            'qualification report is absent': 'report-absent',
            'Codex project trial requires the inspected official release': 'codex-project-release-mismatch',
            **{category: category for category in (
                'claude-windows-executable-invalid', 'claude-windows-bootstrap-invalid',
                'claude-windows-bootstrap-mismatch', 'claude-windows-release-mismatch',
                'claude-windows-policy-invalid')},
        }
        category = categories.get(str(error), 'invalid-preflight')
        publish_runner_failure(args, category)
        raise SystemExit('desktop qualification failed: ' + category) from None
    except (OSError, RuntimeError, subprocess.SubprocessError):
        publish_runner_failure(args, 'execution-failed')
        raise SystemExit('desktop qualification failed; private evidence retained') from None


if __name__ == '__main__':
    raise SystemExit(main())
