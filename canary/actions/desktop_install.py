#!/usr/bin/env python3
"""Install the exact external Desktop entries from a frozen private manifest."""

import argparse
import contextvars
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
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


DIAGNOSTIC_FAILURES = frozenset(("timeout", "spawn", "nonzero_exit", "missing_artifact", "identity_failure",
                                 "cleanup_uncertain"))
DIAGNOSTIC_STAGES = frozenset(("artifact", "download", "hermes_build", "hermes_verify", "installer",
                               "installer_identity"))
DIAGNOSTIC_OPERATIONS = frozenset(("resolve_artifact", "read_staged_artifact", "verify_digest",
                                   "verify_staged_artifact", "verify_downloaded_artifact",
                                   "fetch_artifact", "git_init", "git_remote_add", "git_fetch", "git_checkout",
                                   "venv_create", "pip_install", "npm_ci",
                                   "npm_pack", "verify_revision", "verify_version", "verify_desktop_package",
                                   "verify_hermes_launcher", "check_existing_msix", "register_msix",
                                   "check_existing_installation", "check_platform", "run_installer",
                                   "verify_installation", "install", "identity"))
DIAGNOSTIC_APPS = frozenset(("chatgpt-desktop", "claude-desktop", "hermes-desktop", "pen-desktop", "zed-desktop"))
_APP_CONTEXT = contextvars.ContextVar("desktop_install_app", default=None)
PIP_FAILURE_HINTS = frozenset(("interpreter_compatibility", "dependency_resolution",
                               "build_prerequisite", "network", "other"))
PIP_DETAIL_FIELDS = frozenset(("pip_failure_hint", "python_major", "python_minor",
                               "pip_major", "pip_minor"))


class InstallerFailure(RuntimeError):
    """A subprocess failed after its safe diagnostic was emitted."""


def _emit_diagnostic(stage, operation, failure, return_code=None, details=None, spawn_facts=None):
    """Emit one closed, bounded installer diagnostic without private payloads."""
    app = _APP_CONTEXT.get()
    if app not in DIAGNOSTIC_APPS or stage not in DIAGNOSTIC_STAGES or operation not in DIAGNOSTIC_OPERATIONS:
        raise ValueError("unknown installer diagnostic identity")
    if failure not in DIAGNOSTIC_FAILURES:
        raise ValueError("unknown installer diagnostic failure")
    if details is not None:
        if type(details) is not dict or not set(details) <= PIP_DETAIL_FIELDS:
            raise ValueError("unknown installer diagnostic details")
        if details and (app != "hermes-desktop" or stage != "hermes_build" or operation != "pip_install"):
            raise ValueError("installer diagnostic details do not match operation")
        if "pip_failure_hint" in details and (type(details["pip_failure_hint"]) is not str
                                               or details["pip_failure_hint"] not in PIP_FAILURE_HINTS):
            raise ValueError("unknown installer diagnostic hint")
        for key in PIP_DETAIL_FIELDS - {"pip_failure_hint"}:
            if key in details and (type(details[key]) is not int or not 0 <= details[key] <= 99):
                raise ValueError("invalid installer diagnostic fact")
    record = {"schema_version": 1, "app": app, "stage": stage, "operation": operation,
              "failure": failure}
    if return_code is not None:
        record["return_code"] = int(return_code)
    if details is not None:
        record.update(details)
    if spawn_facts is not None:
        if failure != "spawn" or type(spawn_facts) is not dict or not set(spawn_facts) <= {"os_error", "win_error", "npm_resolution"}:
            raise ValueError("invalid spawn diagnostic facts")
        for key in ("os_error", "win_error"):
            if key in spawn_facts and (type(spawn_facts[key]) is not int or not -(2**31) <= spawn_facts[key] < 2**32):
                raise ValueError("invalid spawn diagnostic code")
        if "npm_resolution" in spawn_facts:
            if operation not in {"npm_ci", "npm_pack"} or spawn_facts["npm_resolution"] not in {"missing", "cmd", "exe", "other"}:
                raise ValueError("invalid npm resolution category")
        record.update(spawn_facts)
    print("DESKTOP_INSTALL_DIAGNOSTIC: " + json.dumps(record, sort_keys=True, separators=(",", ":")),
          file=sys.stderr)


def _fail(stage, operation, failure, return_code=None, details=None):
    _emit_diagnostic(stage, operation, failure, return_code, details)
    raise InstallerFailure("desktop installation failed")


def _hermes_operation(command):
    """Map the fixed Hermes command sequence to stable operation identifiers."""
    executable = str(command[0])
    if executable == "git" and command[1] == "init":
        return "git_init"
    if executable == "git" and command[3] == "remote":
        return "git_remote_add"
    if executable == "git" and command[3] == "fetch":
        return "git_fetch"
    if executable == "git" and command[3] == "checkout":
        return "git_checkout"
    if executable == "npm" and command[1] == "ci":
        return "npm_ci"
    if executable == "npm":
        return "npm_pack"
    if executable == sys.executable and command[1:3] == ("-m", "venv"):
        return "venv_create"
    return "pip_install"


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


_RUNTIME_FACTS_SCRIPT = (
    "import json,sys; import pip; v=pip.__version__.split('.'); "
    "print(json.dumps({'python_major':sys.version_info[0],"
    "'python_minor':sys.version_info[1],'pip_major':int(v[0]),"
    "'pip_minor':int(v[1])},separators=(',',':')))"
)


def _runtime_facts(python, cwd):
    """Read exact venv interpreter facts through a bounded private subprocess."""
    observed = []
    def capture(log):
        try:
            log.flush()
            log.seek(0)
            raw = log.read(4097)
            if len(raw) > 4096:
                return
            facts = json.loads(raw.decode("utf-8", "strict"))
            if (type(facts) is dict and set(facts) == PIP_DETAIL_FIELDS - {"pip_failure_hint"}
                    and all(type(value) is int and 0 <= value <= 99 for value in facts.values())):
                observed.append(facts)
        except (OSError, ValueError, RecursionError):
            pass
    try:
        status = private_command((str(python), "-c", _RUNTIME_FACTS_SCRIPT), Path(cwd), timeout=15,
                                 environment=_safe_environment(), allow_failure=True, diagnostic_callback=capture)
    except (CleanupError, StageTimeout):
        raise CleanupUncertain from None
    except OSError:
        return None
    return observed[0] if status == 0 and observed else None


_PIP_HINT_PATTERNS = {
    "interpreter_compatibility": (re.compile(rb"(?im)^ERROR:.*requires-python"),
                                    re.compile(rb"(?im)^ERROR:.*requires Python")),
    "dependency_resolution": (re.compile(rb"(?im)^ERROR: Cannot install .*conflicting dependencies"),
                               re.compile(rb"(?im)^ERROR: ResolutionImpossible")),
    "build_prerequisite": (re.compile(rb"(?im)^error: subprocess-exited-with-error$"),
                            re.compile(rb"(?im)^ERROR: Failed building wheel")),
    "network": (re.compile(rb"(?im)^(?:WARNING: )?Could not fetch URL"),
                 re.compile(rb"(?im)^(?:ERROR: )?(?:Temporary failure in name resolution|.*ConnectTimeout|.*ProxyError)$")),
}


def _pip_failure_hint(log):
    """Classify only unambiguous, anchored pip signatures; retain no output."""
    try:
        log.flush()
        log.seek(0)
        raw = log.read(64 * 1024 + 1)
        if len(raw) > 64 * 1024:
            return "other"
        raw.decode("utf-8", "strict")
        matches = {hint for hint, patterns in _PIP_HINT_PATTERNS.items()
                   if any(pattern.search(raw) for pattern in patterns)}
        return next(iter(matches)) if len(matches) == 1 else "other"
    except Exception:
        return "other"


def _pip_diagnostic_callback(holder, log):
    """Never let optional hint collection replace executor cleanup failures."""
    try:
        holder[0] = _pip_failure_hint(log)
    except Exception:
        holder[0] = "other"


def _run(argv, *, cwd=None, timeout=600, stage="installer", operation="install", pip_facts=None):
    pip_hint = ["other"]
    callback = (lambda log: _pip_diagnostic_callback(pip_hint, log)) if operation == "pip_install" else None
    try:
        return_code = private_command([str(value) for value in argv], Path(cwd or "."),
                                      timeout=timeout, environment=_safe_environment(),
                                      allow_failure=True, diagnostic_callback=callback)
        if return_code != 0:
            details = None
            if operation == "pip_install":
                details = {**(pip_facts or {}), "pip_failure_hint": pip_hint[0]}
            _fail(stage, operation, "nonzero_exit", return_code, details)
        return True
    except StageTimeout as error:
        _emit_diagnostic(stage, operation, "timeout")
        raise CleanupUncertain from error
    except CleanupError as error:
        _emit_diagnostic(stage, operation, "cleanup_uncertain")
        raise CleanupUncertain from error
    except OSError as error:
        facts = {}
        for field, attribute in (("os_error", "errno"), ("win_error", "winerror")):
            code = getattr(error, attribute, None)
            if type(code) is int and -(2**31) <= code < 2**32:
                facts[field] = code
        if operation in {"npm_ci", "npm_pack"}:
            resolved = shutil.which("npm")
            suffix = Path(resolved).suffix.lower() if resolved else None
            facts["npm_resolution"] = "missing" if resolved is None else {".cmd": "cmd", ".exe": "exe"}.get(suffix, "other")
        _emit_diagnostic(stage, operation, "spawn", spawn_facts=facts)
        raise InstallerFailure("desktop installation failed") from error


def _run_output(argv, *, cwd=None, timeout=60, stage="installer", operation="identity"):
    """Capture bounded private child output needed for an identity comparison."""
    directory = Path(cwd or ".")
    output = directory / ".desktop-install-command-output"
    cleanup_proven = True
    try:
        return_code = private_command([str(value) for value in argv], directory, timeout=timeout,
                                      output=output, environment=_safe_environment(), allow_failure=True)
        passed = return_code == 0
        if not passed:
            _emit_diagnostic(stage, operation, "identity_failure", return_code)
            return None
        with output.open("rb") as source:
            raw = source.read(4097)
        if len(raw) > 4096:
            _fail(stage, operation, "identity_failure")
        try:
            return raw.decode("utf-8", "strict").strip()
        except UnicodeDecodeError:
            _fail(stage, operation, "identity_failure")
    except StageTimeout as error:
        cleanup_proven = False
        _emit_diagnostic(stage, operation, "timeout")
        raise CleanupUncertain from error
    except CleanupError as error:
        cleanup_proven = False
        _emit_diagnostic(stage, operation, "cleanup_uncertain")
        raise CleanupUncertain from error
    except OSError as error:
        _emit_diagnostic(stage, operation, "spawn")
        raise InstallerFailure("desktop installation failed") from error
    finally:
        if cleanup_proven:
            output.unlink(missing_ok=True)


def _download(url, destination):
    return _run(("curl", "--proto", "=https", "--proto-redir", "=https", "--fail",
                 "--location", "--silent", "--show-error", "--max-time", "300",
                 "--max-filesize", "2147483648", url, "--output", destination), timeout=360,
                stage="download", operation="fetch_artifact")


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
        _fail("artifact", "read_staged_artifact", "missing_artifact")
    digest = hashlib.sha256()
    total = 0
    with source.open("rb") as incoming, destination.open("xb") as outgoing:
        os.chmod(destination, 0o600)
        for block in iter(lambda: incoming.read(1024 * 1024), b""):
            total += len(block)
            if total > 2_147_483_648:
                _fail("artifact", "read_staged_artifact", "missing_artifact")
            digest.update(block)
            outgoing.write(block)
    if digest.hexdigest() != expected:
        _fail("artifact", "verify_digest", "identity_failure")


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
        operation = _hermes_operation(command)
        facts = None
        if operation == "pip_install":
            facts = _runtime_facts(command[0], workspace / "hermes-agent")
        if not _run(command, cwd=cwd, timeout=900, stage="hermes_build", operation=operation,
                    pip_facts=facts):
            raise RuntimeError("Hermes source preparation failed")
    source = workspace / "hermes-agent"
    if _run_output(("git", "-C", source, "rev-parse", "HEAD"), timeout=30,
                   stage="hermes_verify", operation="verify_revision") != release["revision"]:
        _fail("hermes_verify", "verify_revision", "identity_failure")
    package = source / "apps" / "desktop" / "package.json"
    try:
        version = json.loads(package.read_bytes()).get("version")
    except (OSError, ValueError, TypeError):
        version = None
    if version != release.get("version"):
        _fail("hermes_verify", "verify_version", "identity_failure")
    if not (source / "apps" / "desktop" / "release").is_dir():
        _fail("artifact", "verify_desktop_package", "missing_artifact")
    _, launcher, _ = hermes_runtime_paths(source, os.name == "nt")
    if not launcher.is_file():
        _fail("artifact", "verify_hermes_launcher", "missing_artifact")
    return source, launcher


def _install_windows(release, package, workspace):
    app = release["app"]
    if release.get("format") == "msix":
        package_names = {"chatgpt-desktop": ("OpenAI.Codex", "OpenAI.ChatGPT-Desktop"),
                         "claude-desktop": ("Claude",)}
        names = package_names.get(app)
        if not names:
            _emit_diagnostic("installer_identity", "check_existing_msix", "identity_failure")
            raise ValueError("unknown MSIX identity")
        quoted_names = ",".join("'" + name + "'" for name in names)
        if _run_output(("powershell", "-NoProfile", "-NonInteractive", "-Command",
                        "$ErrorActionPreference='Stop'; $n=@(" + quoted_names + "); if (@(Get-AppxPackage | Where-Object { $n -contains $_.Name }).Count -ne 0) { exit 1 }"),
                       cwd=workspace, timeout=30, stage="installer_identity", operation="check_existing_msix") is None:
            raise RuntimeError("an existing MSIX installation was left unchanged")
        # Registration verifies the signature; the query binds the installed identity.
        command = ("$ErrorActionPreference='Stop'; Add-AppxPackage -Path '" + str(package).replace("'", "''") + "'; "
                   "$n=@(" + quoted_names + "); $p=@(Get-AppxPackage | Where-Object { $n -contains $_.Name }); "
                   "if ($p.Count -ne 1) { exit 1 }")
        if not _run(("powershell", "-NoProfile", "-NonInteractive", "-Command", command), timeout=600,
                    stage="installer", operation="register_msix"):
            raise RuntimeError("MSIX registration failed")
        return
    targets = {"hermes-desktop": "Hermes", "pen-desktop": "Pen", "zed-desktop": "Zed"}
    target_name = targets.get(app)
    if not target_name:
        _emit_diagnostic("installer_identity", "check_existing_installation", "identity_failure")
        raise ValueError("unknown Windows installer identity")
    target = Path(os.environ.get("LOCALAPPDATA", str(workspace))) / "Programs" / target_name
    if target.exists():
        _fail("installer_identity", "check_existing_installation", "identity_failure")
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
    if not _run(arguments, cwd=workspace, timeout=600, stage="installer", operation="run_installer"):
        raise RuntimeError("Desktop installer failed")
    if not target.is_dir():
        _fail("installer", "verify_installation", "missing_artifact")


def _install_entry(release, platform, artifacts, workspace):
    if release.get("installer") != "external":
        return "skipped"
    if release.get("app") not in DESKTOP_HARNESSES or release.get("status") != "frozen":
        raise ValueError("external entry is not frozen")
    package = staged_artifact(artifacts, release)
    if package is None:
        digest = _digest_for(release)
        if release.get("staged") or (release.get("format") != "source" and not digest):
            _fail("artifact", "resolve_artifact", "missing_artifact")
        if release.get("format") == "source":
            if release["app"] != "hermes-desktop":
                raise ValueError("only Hermes may use a source entry")
            _prepare_hermes(release, workspace / "hermes")
            return "installed"
        suffix = ".msix" if release.get("format") == "msix" else ".exe"
        package = workspace / (release["app"] + "-" + digest + suffix)
        if not _download(release["url"], package) or sha256(package) != digest:
            _fail("artifact", "verify_downloaded_artifact", "identity_failure")
    else:
        if not package.is_file() or sha256(package) != _digest_for(release):
            _fail("artifact", "verify_staged_artifact", "missing_artifact" if not package.is_file() else "identity_failure")
        suffix = ".msix" if release.get("format") == "msix" else ".exe"
        materialized = workspace / (release["app"] + "-" + _digest_for(release) + suffix)
        materialize_verified(package, materialized, _digest_for(release))
        package = materialized
    if platform != "windows":
        _emit_diagnostic("installer_identity", "check_platform", "identity_failure")
        raise ValueError("external package is unsupported on this platform")
    _install_windows(release, package, workspace)
    return "installed"


def install_entry(release, platform, artifacts, workspace):
    """Install one entry with diagnostics scoped to this valid manifest app."""
    app = release.get("app")
    if app not in DIAGNOSTIC_APPS:
        return _install_entry(release, platform, artifacts, workspace)
    token = _APP_CONTEXT.set(app)
    try:
        return _install_entry(release, platform, artifacts, workspace)
    finally:
        _APP_CONTEXT.reset(token)


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
