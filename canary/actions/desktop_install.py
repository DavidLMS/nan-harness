#!/usr/bin/env python3
"""Install the exact external Desktop entries from a frozen private manifest."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from desktop_suite import read_frozen_manifest
from cell import CleanupError, StageTimeout, ensure_private_directory, private_command, write_json
from selection import DESKTOP_HARNESSES

SHA256 = re.compile(r"sha256:([0-9a-f]{64})\Z")
REVISION = re.compile(r"[0-9a-f]{40}\Z")
MODEL = re.compile(r"[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}\Z")
SYSTEMS = frozenset(("linux", "macos", "windows"))
PRIVATE_ENV_NAMES = (
    "NAN_API_KEY", "GH_TOKEN", "GITHUB_TOKEN", "GITHUB_ENV", "GITHUB_OUTPUT",
    "GITHUB_PATH", "GITHUB_STEP_SUMMARY", "ACTIONS_RUNTIME_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
)


class CleanupUncertain(RuntimeError):
    """A timed-out child may still own files or mounts; stop subsequent apps."""


def _safe_environment():
    environment = dict(os.environ)
    for name in PRIVATE_ENV_NAMES:
        environment.pop(name, None)
    return environment


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _run(argv, *, cwd=None, timeout=600):
    try:
        return private_command([str(value) for value in argv], Path(cwd or "."),
                               timeout=timeout, environment=_safe_environment(),
                               allow_failure=True) == 0
    except (CleanupError, StageTimeout) as error:
        raise CleanupUncertain from error


def _run_output(argv, *, cwd=None, timeout=60):
    """Capture bounded private child output needed for an identity comparison."""
    directory = Path(cwd or ".")
    output = directory / ".desktop-install-command-output"
    cleanup_proven = True
    try:
        passed = private_command([str(value) for value in argv], directory, timeout=timeout,
                                 output=output, environment=_safe_environment(), allow_failure=True) == 0
        if not passed:
            return None
        with output.open("rb") as source:
            raw = source.read(4097)
        if len(raw) > 4096:
            raise RuntimeError("installer identity output exceeds its bound")
        return raw.decode("utf-8", "strict").strip()
    except (CleanupError, StageTimeout) as error:
        cleanup_proven = False
        raise CleanupUncertain from error
    finally:
        if cleanup_proven:
            output.unlink(missing_ok=True)


def _download(url, destination):
    return _run(("curl", "--proto", "=https", "--proto-redir", "=https", "--fail",
                 "--location", "--silent", "--show-error", "--max-time", "300",
                 "--max-filesize", "2147483648", url, "--output", destination), timeout=360)


def _digest_for(release):
    match = SHA256.fullmatch(release.get("digest", ""))
    return match.group(1) if match else None


def staged_artifact(artifacts, release):
    """Return the only permitted staged path, or fail closed."""
    digest = _digest_for(release)
    if not release.get("staged") or not digest:
        return None
    return Path(artifacts) / (release["app"] + "-" + digest)


def materialize_verified(source, destination, expected):
    """Hash the bytes actually copied to a new native-extension installer file."""
    if source.is_symlink() or not source.is_file() or source.stat().st_size > 2_147_483_648:
        raise RuntimeError("staged installer is not a bounded regular file")
    digest = hashlib.sha256()
    total = 0
    with source.open("rb") as incoming, destination.open("xb") as outgoing:
        os.chmod(destination, 0o600)
        for block in iter(lambda: incoming.read(1024 * 1024), b""):
            total += len(block)
            if total > 2_147_483_648:
                raise RuntimeError("staged installer exceeds its bound")
            digest.update(block)
            outgoing.write(block)
    if digest.hexdigest() != expected:
        raise RuntimeError("materialized installer differs from frozen bytes")


def hermes_source_commands(release, root):
    """Build Hermes at the manifest's exact commit, never a branch or date tag."""
    revision = release.get("revision", "")
    if not REVISION.fullmatch(revision) or release.get("url") != "https://github.com/NousResearch/hermes-agent.git":
        raise ValueError("Hermes source identity is invalid")
    source = Path(root) / "hermes-agent"
    return (
        (("git", "init", "--quiet", source), None),
        (("git", "-C", source, "remote", "add", "origin", release["url"]), None),
        (("git", "-C", source, "fetch", "--depth", "1", "origin", revision), None),
        (("git", "-C", source, "checkout", "--detach", revision), None),
        # Use the interpreter running this action: Windows runners commonly
        # expose it as `python`, while Unix runners may expose `python3`.
        ((sys.executable, "-m", "venv", source / "venv"), None),
        ((source / "venv" / ("Scripts" if os.name == "nt" else "bin") / ("python.exe" if os.name == "nt" else "python"), "-m", "pip", "install", "--disable-pip-version-check", "-e", source), None),
        (("npm", "ci", "--no-audit", "--no-fund"), source),
        (("npm", "run", "pack"), source / "apps" / "desktop"),
    )


def hermes_runtime_paths(root, windows=False):
    """Return the platform-native Hermes venv launcher and PATH directory."""
    scripts = Path(root) / "venv" / ("Scripts" if windows else "bin")
    launcher = scripts / ("hermes.exe" if windows else "hermes")
    python = scripts / ("python.exe" if windows else "python")
    return python, launcher, scripts


def _prepare_hermes(release, workspace):
    workspace.mkdir(mode=0o700, parents=True, exist_ok=False)
    for command, cwd in hermes_source_commands(release, workspace):
        if not _run(command, cwd=cwd, timeout=900):
            raise RuntimeError("Hermes source preparation failed")
    source = workspace / "hermes-agent"
    if _run_output(("git", "-C", source, "rev-parse", "HEAD"), timeout=30) != release["revision"]:
        raise RuntimeError("Hermes revision verification failed")
    package = source / "apps" / "desktop" / "package.json"
    try:
        version = json.loads(package.read_bytes()).get("version")
    except (OSError, ValueError, TypeError):
        version = None
    if version != release.get("version"):
        raise RuntimeError("Hermes product version verification failed")
    if not (source / "apps" / "desktop" / "release").is_dir():
        raise RuntimeError("Hermes desktop package was not produced")
    _, launcher, _ = hermes_runtime_paths(source, os.name == "nt")
    if not launcher.is_file():
        raise RuntimeError("Hermes launcher was not produced")
    return source, launcher


def _install_windows(release, package, workspace):
    app = release["app"]
    if release.get("format") == "msix":
        package_names = {"chatgpt-desktop": ("OpenAI.Codex", "OpenAI.ChatGPT-Desktop"),
                         "claude-desktop": ("Claude",)}
        names = package_names.get(app)
        if not names:
            raise ValueError("unknown MSIX identity")
        quoted_names = ",".join("'" + name + "'" for name in names)
        if _run_output(("powershell", "-NoProfile", "-NonInteractive", "-Command",
                        "$ErrorActionPreference='Stop'; $n=@(" + quoted_names + "); if (@(Get-AppxPackage | Where-Object { $n -contains $_.Name }).Count -ne 0) { exit 1 }"),
                       cwd=workspace, timeout=30) is None:
            raise RuntimeError("an existing MSIX installation was left unchanged")
        # Registration verifies the signature; the query binds the installed identity.
        command = ("$ErrorActionPreference='Stop'; Add-AppxPackage -Path '" + str(package).replace("'", "''") + "'; "
                   "$n=@(" + quoted_names + "); $p=@(Get-AppxPackage | Where-Object { $n -contains $_.Name }); "
                   "if ($p.Count -ne 1) { exit 1 }")
        if not _run(("powershell", "-NoProfile", "-NonInteractive", "-Command", command), timeout=600):
            raise RuntimeError("MSIX registration failed")
        return
    targets = {"hermes-desktop": "Hermes", "pen-desktop": "Pen", "zed-desktop": "Zed"}
    target_name = targets.get(app)
    if not target_name:
        raise ValueError("unknown Windows installer identity")
    target = Path(os.environ.get("LOCALAPPDATA", str(workspace))) / "Programs" / target_name
    if target.exists():
        raise RuntimeError("an existing Desktop installation was left unchanged")
    if app == "zed-desktop":
        arguments = (package, "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART",
                     "/NOCLOSEAPPLICATIONS", "/NORESTARTAPPLICATIONS", "/TASKS=", "/DIR=" + str(target))
    else:
        # NSIS consumes the unquoted final /D tail, unlike ordinary argv parsing.
        command = ("$ErrorActionPreference='Stop'; $s=New-Object System.Diagnostics.ProcessStartInfo; "
                   "$s.FileName='" + str(package).replace("'", "''") + "'; "
                   "$s.Arguments='/S /D=" + str(target).replace("'", "''") + "'; "
                   "$s.UseShellExecute=$false; $p=[System.Diagnostics.Process]::Start($s); "
                   "$p.WaitForExit(); exit $p.ExitCode")
        arguments = ("powershell", "-NoProfile", "-NonInteractive", "-Command", command)
    if not _run(arguments, cwd=workspace, timeout=600):
        raise RuntimeError("Desktop installer failed")
    if not target.is_dir():
        raise RuntimeError("Desktop installation directory was not created")


def install_entry(release, platform, artifacts, workspace):
    if release.get("installer") != "external":
        return "skipped"
    if release.get("app") not in DESKTOP_HARNESSES or release.get("status") != "frozen":
        raise ValueError("external entry is not frozen")
    package = staged_artifact(artifacts, release)
    if package is None:
        digest = _digest_for(release)
        if release.get("staged") or (release.get("format") != "source" and not digest):
            raise ValueError("frozen artifact is missing an immutable digest")
        if release.get("format") == "source":
            if release["app"] != "hermes-desktop":
                raise ValueError("only Hermes may use a source entry")
            _prepare_hermes(release, workspace / "hermes")
            return "installed"
        suffix = ".msix" if release.get("format") == "msix" else ".exe"
        package = workspace / (release["app"] + "-" + digest + suffix)
        if not _download(release["url"], package) or sha256(package) != digest:
            raise RuntimeError("downloaded artifact did not match its frozen digest")
    else:
        if not package.is_file() or sha256(package) != _digest_for(release):
            raise RuntimeError("staged artifact did not match its frozen digest")
        suffix = ".msix" if release.get("format") == "msix" else ".exe"
        materialized = workspace / (release["app"] + "-" + _digest_for(release) + suffix)
        materialize_verified(package, materialized, _digest_for(release))
        package = materialized
    if platform != "windows":
        raise ValueError("external package is unsupported on this platform")
    _install_windows(release, package, workspace)
    return "installed"


def install(manifest, artifacts, platform, architecture, model, harnesses):
    if platform not in SYSTEMS or not MODEL.fullmatch(model):
        raise ValueError("invalid platform or model")
    apps = [part.strip() for part in harnesses.split(",")]
    if not apps or len(apps) != len(set(apps)) or any(app not in DESKTOP_HARNESSES for app in apps):
        raise ValueError("harnesses must be distinct known Desktop applications")
    frozen = read_frozen_manifest(manifest, apps, platform, architecture, model)
    entries = frozen.get("apps", frozen) if isinstance(frozen, dict) else frozen
    by_app = {entry.get("app"): entry for entry in entries}
    root = Path(artifacts)
    ensure_private_directory(root, reusable=True)
    installation_root = root / "desktop-install"
    ensure_private_directory(installation_root, reusable=True)
    failures = []
    workspace = installation_root / platform
    ensure_private_directory(workspace, reusable=True)
    for app in apps:
        entry = by_app.get(app)
        try:
            if not entry or entry.get("status") != "frozen":
                continue
            install_entry(entry, platform, root, workspace)
        except CleanupUncertain:
            raise
        except (OSError, RuntimeError, ValueError):
            failures.append(app)
    receipt = {"status": "prepared", "apps": {app: ("failed" if app in failures else "prepared") for app in apps}}
    receipt_path = installation_root / "installation.json"
    write_json(receipt_path, receipt)
    if "hermes-desktop" not in failures:
        _export_hermes_environment(by_app, workspace)
    return True


def _export_hermes_environment(entries, workspace):
    for app, entry in entries.items():
        if app != "hermes-desktop" or entry.get("status") != "frozen" or entry.get("installer") != "external":
            continue
        root = workspace / "hermes" / "hermes-agent"
        _, launcher, scripts = hermes_runtime_paths(root, os.name == "nt")
        env_file = os.environ.get("GITHUB_ENV")
        path_file = os.environ.get("GITHUB_PATH")
        if not env_file or not path_file or not root.is_dir() or not launcher.is_file():
            return
        with Path(env_file).open("a") as target:
            target.write(f"HERMES_HOME={workspace / 'hermes'}\n")
            target.write(f"HERMES_DESKTOP_HERMES_ROOT={root}\n")
            target.write(f"HERMES_DESKTOP_HERMES={launcher}\n")
        with Path(path_file).open("a") as target:
            target.write(f"{scripts}\n")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--architecture", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--harnesses", required=True)
    args = parser.parse_args(argv)
    try:
        expected_os = {"linux": "Linux", "macos": "macOS", "windows": "Windows"}.get(args.platform)
        actual = {"linux": "Linux", "darwin": "macOS", "win32": "Windows"}.get(sys.platform)
        if (os.environ.get("GITHUB_ACTIONS") != "true"
                or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted"
                or os.environ.get("RUNNER_OS") != expected_os or actual != expected_os):
            raise ValueError("Desktop installation requires its disposable native hosted runner")
        return 0 if install(args.manifest, args.artifacts, args.platform, args.architecture,
                            args.model, args.harnesses) else 1
    except (CleanupUncertain, OSError, ValueError, RuntimeError, json.JSONDecodeError):
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
