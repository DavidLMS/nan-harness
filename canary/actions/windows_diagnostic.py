#!/usr/bin/env python3
"""Run a native Windows compatibility sweep without publishing child output."""
from __future__ import annotations
import argparse, hashlib, importlib.util, json, os, platform, re, shutil, subprocess, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
from cell import WindowsJob, finish_stage, protect_private

HARNESSES = ("claude-code", "codex", "opencode", "hermes", "pi", "omp", "prime-agent",
             "deepseek-harness", "openclaw", "cline", "qwen-code", "kimi-code", "aider", "goose", "fx")
PHASES = ("metadata", "prerequisites", "install", "version-doctor", "deterministic-contract", "live-tool")
SAFE_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}$")
SHA = re.compile(r"^[0-9a-f]{40}$")
ROOT = Path(__file__).resolve().parents[2]

def cause_key(phase_name, reason, detail=""):
    token = hashlib.sha256(f"{phase_name}:{reason}:{detail}".encode()).hexdigest()[:12]
    return f"WIN-{phase_name.upper().replace('-', '_')}-{token}"

def causal(harness, phase_name, reason):
    token = hashlib.sha256(f"{harness}:{phase_name}:{reason}".encode()).hexdigest()[:12]
    return f"WIN-{phase_name.upper().replace('-', '_')}-{token}"

def phase(status, reason="", cause=None, group=None, details=None):
    value = {"status": status}
    if reason: value["reason"] = reason
    if cause: value["causalId"] = cause
    if group: value["causeGroup"] = group
    if details: value["causeDetails"] = details
    return value

def run_bounded(argv, cwd, env, timeout):
    """Run one private child and kill its complete tree on timeout."""
    try:
        flags = getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
        suspended = os.name == "nt"
        if suspended:
            flags |= getattr(subprocess, "CREATE_SUSPENDED", 0x00000004)
        child = subprocess.Popen(argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                 creationflags=flags)
        job = None
        try:
            job = WindowsJob(child.pid) if suspended else None
            if job:
                job.resume(child.pid)
            code = child.wait(timeout=timeout)
            if job:
                finish_stage(child, job)
            return code, "exit" if code == 0 else "nonzero"
        except subprocess.TimeoutExpired:
            if job:
                job.close()
                child.wait(timeout=10)
            elif os.name == "nt":
                # Windows children are created suspended until a Job Object is attached;
                # a missing job means this child never ran and can be terminated alone.
                child.kill(); child.wait(timeout=10)
            else:
                child.kill(); child.wait(timeout=10)
            return None, "timeout"
        except (OSError, RuntimeError, ValueError, subprocess.SubprocessError):
            if job:
                job.close()
            elif child.poll() is None:
                child.kill(); child.wait(timeout=10)
            return None, "unavailable"
    except (OSError, ValueError, subprocess.SubprocessError):
        return None, "unavailable"

def _self_test_result(code, reason):
    return {"status": "PASS" if code == 0 else "FAIL", "reason": reason}

def native_prerequisite_self_test(cell, env, timeout):
    """Exercise independent native boundaries before any harness installer runs.

    Child output is always discarded.  Each check is independent so a broken
    registry or Git Bash does not hide unrelated process, venv, or ACL failures.
    """
    if os.name != "nt":
        return {"status": "NOT_RUN", "reason": "windows-only", "checks": {}}
    checks = {}
    fixture = cell / "native self-test space"
    fixture.mkdir(parents=True, exist_ok=True)
    protect_private(fixture)
    bounded = max(1, min(timeout, 30))

    def check(name, argv):
        try:
            code, reason = run_bounded(argv, fixture, env, bounded)
        except (OSError, ValueError, RuntimeError):
            code, reason = None, "unavailable"
        checks[name] = _self_test_result(code, reason)

    check("pwsh-parser", ["pwsh", "-NoProfile", "-NonInteractive", "-Command",
                          "[void][int]7"])
    comspec = env.get("ComSpec") or env.get("COMSPEC")
    if not comspec:
        checks["cmd-node-npm"] = {"status": "FAIL", "reason": "comspec-missing"}
        checks["npm-registry"] = {"status": "FAIL", "reason": "comspec-missing"}
    else:
        quoted = 'cd /d "' + str(fixture).replace('"', '\\"') + '" && node --version && npm --version'
        check("cmd-node-npm", [comspec, "/d", "/s", "/c", quoted])
        registry = 'cd /d "' + str(fixture).replace('"', '\\"') + '" && npm.cmd view npm version --fetch-retries=0 --fetch-timeout=15000 --json'
        check("npm-registry", [comspec, "/d", "/s", "/c", registry])
    prefix = env.get("NPM_CONFIG_PREFIX", "")
    cache = env.get("NPM_CONFIG_CACHE", "")
    isolated = str(cell) in prefix and str(cell) in cache
    checks["npm-isolation"] = {"status": "PASS" if isolated else "FAIL",
                                "reason": "private-prefix-cache" if isolated else "prefix-cache-outside-cell"}

    venv = fixture / "venv"
    check("python-venv", ["py.exe", "-m", "venv", str(venv)])
    check("python-pip", [str(venv / "Scripts/python.exe"), "-m", "pip", "--version"])
    check("git-bash", ["bash.exe", "--noprofile", "--norc", "-c", "exit 0"])
    # run_bounded's suspended child + Job Object path is the native containment
    # and private-DACL probe; protect_private above verifies the output ACL path.
    check("job-dacl", ["pwsh", "-NoProfile", "-NonInteractive", "-Command", "exit 0"])
    failed = [name for name, value in checks.items() if value["status"] == "FAIL"]
    return {"status": "FAIL" if failed else "PASS", "reason": "checks-failed" if failed else "verified",
            "checks": checks}

def isolated_environment(cell):
    """Construct an allowlisted environment; credentials never enter setup."""
    home, temp, bin_dir = cell / "home", cell / "tmp", cell / "bin"
    for directory in (home, temp, bin_dir, home / "AppData/Roaming", home / "AppData/Local",
                      home / ".cache", home / ".config", home / ".npm-global"):
        directory.mkdir(parents=True, exist_ok=True); protect_private(directory)
    allowed = {"PATH", "PATHEXT", "SystemRoot", "SYSTEMROOT", "ComSpec", "COMSPEC", "WINDIR",
               "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "PSModulePath",
               "PROCESSOR_ARCHITECTURE", "PROCESSOR_IDENTIFIER", "NUMBER_OF_PROCESSORS",
               "USERDOMAIN", "USERNAME"}
    env = {key: value for key, value in os.environ.items() if key in allowed}
    original_path = env.get("PATH", "")
    env.update({"HOME": str(home), "USERPROFILE": str(home), "APPDATA": str(home / "AppData/Roaming"),
                "LOCALAPPDATA": str(home / "AppData/Local"), "TEMP": str(temp), "TMP": str(temp),
                "NAN_HARNESS_CONFIG_DIR": str(home / ".config/nan-harness"),
                "NPM_CONFIG_PREFIX": str(home / ".npm-global"), "NPM_CONFIG_CACHE": str(home / ".cache/npm"),
                "NAN_CANARY_HOSTED": "1", "NAN_CANARY_REDACT_FAILURE_OUTPUT": "1"})
    env["PATH"] = os.pathsep.join((str(bin_dir), str(cell / "hermes" / "bin"),
                                    str(home / ".nan-harness-canary-venv" / "Scripts"),
                                    str(home / ".npm-global"), original_path))
    return env

def resolver_module():
    spec = importlib.util.spec_from_file_location("windows_cli_suite", Path(__file__).with_name("cli-suite.py"))
    module = importlib.util.module_from_spec(spec); assert spec.loader is not None; spec.loader.exec_module(module)
    return module

def resolve_one(module, harness, model):
    resolved, unresolved = module.resolve_manifest([harness], "windows", "x86_64", model)
    if unresolved: return None, unresolved[0].diagnostic or {"category": "unknown"}
    return resolved[0], None

def mark_dependents(phases, names, reason, cause):
    for name in names: phases[name] = phase("BLOCKED", reason, cause)

_INSTALLER_REASONS = frozenset({
    "passed", "installer-failed", "official-asset-missing", "official-metadata-probe-failed",
    "official-metadata-no-windows-asset", "capability-not-implemented", "invalid-frozen-ref",
    "invalid-version",
})
_INSTALLER_DIAGNOSTIC_KEYS = frozenset({
    "subphase", "executable", "exitCode", "win32Error", "httpStatus", "assetReason",
    "npmCode", "pipCategory", "processReason",
})
_INSTALLER_SUBPHASES = frozenset({
    "metadata", "download", "archive", "asset-selection", "execute", "install",
    "virtualenv", "cleanup", "unknown",
})
_INSTALLER_EXECUTABLES = frozenset({
    "unknown", "npm-cmd", "pwsh", "py-launcher", "python", "uv", "github-api",
    "http-download", "official-metadata", "archive", "archive-extract",
})
_INSTALLER_ASSET_REASONS = frozenset({
    "release-empty", "expected-asset-missing", "expected-executable-missing", "invalid-archive",
    "empty-download", "windows-mapping-missing", "windows-asset-missing", "metadata-request-failed",
    "metadata-inconclusive",
    "invalid-ref", "invalid-version",
})
_INSTALLER_NPM_CODES = frozenset({
    "registry-dns", "registry-connection", "registry-timeout", "registry-unreachable",
    "package-not-found", "permission", "tls-certificate", "npm-unknown",
})
_INSTALLER_PIP_CATEGORIES = frozenset({
    "network-dns", "network-connection", "network-timeout", "package-not-found",
    "permission", "tls-certificate", "pip-missing", "pip-unknown",
})
_INSTALLER_PROCESS_REASONS = frozenset({"win32-launch-failed", "exit-nonzero", "native-unavailable"})
_PROBE_DIAGNOSTICS = frozenset({
    "doctor-child-launch", "doctor-exit-nonzero", "doctor-output-invalid", "doctor-schema-invalid",
    "doctor-version-mismatch", "conformance-child-launch", "conformance-exit-nonzero",
    "conformance-output-invalid", "conformance-schema-invalid", "conformance-scenario-missing",
    "conformance-scenario-failed", "conformance-inventory-failed", "conformance-check-invalid",
    "doctor-exit-missing", "conformance-exit-missing", "live-child-launch", "live-exit-nonzero",
    "live-exit-missing", "live-credential-missing", "live-tool-evidence-missing",
    "live-read-marker-missing", "live-completion-marker-missing", "live-bridge-sentinel",
    "live-usage-invalid", "live-usage-summary-missing",
})
_SEMVER = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$")
_DOCTOR_REASONS = frozenset({"missing", "invalid", "mismatch", "discovery-error"})
_DISCOVERY_CODES = frozenset({f"NH-DISCOVERY-{index:03d}" for index in range(1, 8)})
_CONFORMANCE_REASONS = frozenset({
    "process-failed", "marker-missing", "provider-failed", "provider-shutdown-failed",
    "daemon-cleanup-failed",
})
_LIVE_FAILURE_STAGES = frozenset({
    "live-tool", "harness-run", "read-marker", "completion-marker", "bridge-sentinel",
    "usage-evidence", "usage-summary",
})

def _safe_installer_diagnostic(value):
    if not isinstance(value, dict) or set(value) - _INSTALLER_DIAGNOSTIC_KEYS:
        return None
    diagnostic = {}
    for key, item in value.items():
        if key in {"subphase", "executable", "assetReason"}:
            if not isinstance(item, str) or len(item) > 64:
                return None
            if key == "subphase" and item not in _INSTALLER_SUBPHASES:
                return None
            if key == "executable" and item not in _INSTALLER_EXECUTABLES:
                return None
            if key == "assetReason" and item not in _INSTALLER_ASSET_REASONS:
                return None
            diagnostic[key] = item
        elif key == "npmCode":
            if not isinstance(item, str) or item not in _INSTALLER_NPM_CODES:
                return None
            diagnostic[key] = item
        elif key == "pipCategory":
            if not isinstance(item, str) or item not in _INSTALLER_PIP_CATEGORIES:
                return None
            diagnostic[key] = item
        elif key == "processReason":
            if not isinstance(item, str) or item not in _INSTALLER_PROCESS_REASONS:
                return None
            diagnostic[key] = item
        elif key in {"exitCode", "win32Error", "httpStatus"}:
            if not isinstance(item, int) or isinstance(item, bool) or not (-1 <= item <= 65535):
                return None
            if key == "httpStatus" and not (100 <= item <= 599):
                return None
            diagnostic[key] = item
    return diagnostic

def installer_diagnostic(cell):
    """Consume only the installer's closed marker; never retain exception text."""
    marker = cell / "installer-result.json"
    try:
        value = json.loads(marker.read_text(encoding="utf-8-sig"))
        if not isinstance(value, dict) or value.get("status") not in {"passed", "failed"}:
            return "installer-failed", {}
        if value.get("schemaVersion") == 1:
            if set(value) != {"schemaVersion", "status", "reason"} or value["reason"] not in _INSTALLER_REASONS:
                return "installer-failed", {}
            return value["reason"], {}
        if value.get("schemaVersion") != 2 or set(value) != {"schemaVersion", "status", "reason", "diagnostic"}:
            return "installer-failed", {}
        diagnostic = _safe_installer_diagnostic(value["diagnostic"])
        if diagnostic is None or value["reason"] not in _INSTALLER_REASONS:
            return "installer-failed", {}
        return value["reason"], diagnostic
    except (OSError, ValueError, TypeError, KeyError):
        return "installer-failed", {}
    finally:
        marker.unlink(missing_ok=True)

def probe_diagnostic(cell, expected_stage=None):
    """Consume the probe's bounded diagnostics and exit code, never child output."""
    marker = cell / "probe-result.json"
    try:
        value = json.loads(marker.read_text(encoding="utf-8-sig"))
        if not isinstance(value, dict) or value.get("schemaVersion") not in {1, 2}:
            return {"status": "failed"}
        if not isinstance(value.get("stage"), str) or value.get("status") not in {"passed", "failed"}:
            return {"status": "failed"}
        result = {"status": value["status"], "stage": value["stage"]}
        if value["schemaVersion"] == 1:
            if set(value) - {"schemaVersion", "stage", "status", "diagnostic"}:
                return {"status": "failed"}
            return result
        allowed = {"schemaVersion", "stage", "status", "diagnostics", "exitCode",
                   "doctorVersion", "doctorExpectedVersion", "doctorReason", "discoveryCode",
                   "inventoryFailureReasons"}
        if set(value) - allowed:
            return {"status": "failed"}
        for key in ("doctorVersion", "doctorExpectedVersion"):
            if key in value and (not isinstance(value[key], str) or len(value[key]) > 64 or not _SEMVER.fullmatch(value[key])):
                return {"status": "failed"}
        if "doctorReason" in value and (not isinstance(value["doctorReason"], str)
                                         or value["doctorReason"] not in _DOCTOR_REASONS):
            return {"status": "failed"}
        if "discoveryCode" in value and (not isinstance(value["discoveryCode"], str)
                                          or value["discoveryCode"] not in _DISCOVERY_CODES
                                          or value.get("doctorReason") != "discovery-error"):
            return {"status": "failed"}
        if "doctorReason" in value and value["doctorReason"] == "discovery-error" and "discoveryCode" not in value:
            return {"status": "failed"}
        if "inventoryFailureReasons" in value and (expected_stage != "deterministic-contract"
                                                    or not isinstance(value["inventoryFailureReasons"], list)
                                                    or len(value["inventoryFailureReasons"]) > 5
                                                    or len(set(value["inventoryFailureReasons"])) != len(value["inventoryFailureReasons"])
                                                    or any(not isinstance(reason, str) or reason not in _CONFORMANCE_REASONS
                                                           for reason in value["inventoryFailureReasons"])):
            return {"status": "failed"}
        diagnostics = value.get("diagnostics", [])
        if not isinstance(diagnostics, list) or len(diagnostics) > 16 or any(
                not isinstance(item, str) or item not in _PROBE_DIAGNOSTICS for item in diagnostics):
            return {"status": "failed"}
        if value["status"] == "passed":
            if value.get("stage") != "complete" or value.get("exitCode") != 0 or diagnostics:
                return {"status": "failed"}
        elif not diagnostics:
            return {"status": "failed"}
        elif expected_stage == "live-tool":
            if value.get("stage") not in _LIVE_FAILURE_STAGES:
                return {"status": "failed"}
        elif value.get("stage") != expected_stage or "exitCode" not in value:
            return {"status": "failed"}
        result["diagnostics"] = diagnostics
        if "exitCode" in value:
            if not isinstance(value["exitCode"], int) or isinstance(value["exitCode"], bool) or not (-1 <= value["exitCode"] <= 65535):
                return {"status": "failed"}
            result["exitCode"] = value["exitCode"]
        for key in ("doctorVersion", "doctorExpectedVersion", "doctorReason", "discoveryCode", "inventoryFailureReasons"):
            if key in value:
                result[key] = value[key]
        return result
    except (OSError, ValueError, TypeError, KeyError):
        return {"status": "failed"}
    finally:
        marker.unlink(missing_ok=True)

def cleanup_installer_artifacts(cell):
    """Remove private installer logs even when the parent kills a timed-out child."""
    clean = True
    try:
        if (cell / "installer-tmp").exists():
            shutil.rmtree(cell / "installer-tmp", ignore_errors=False)
    except OSError:
        clean = False
    try:
        (cell / "installer-result.json").unlink(missing_ok=True)
    except OSError:
        clean = False
    return clean

def collect(args, harnesses, output):
    output.mkdir(parents=True, exist_ok=True); protect_private(output)
    root = output / "cells"; root.mkdir(exist_ok=True); protect_private(root)
    try:
        resolver = resolver_module()
        resolver_error = None
    except Exception:
        resolver = None
        resolver_error = "resolver-error"
    reports = []; any_failure = False
    native_test = {"status": "NOT_RUN", "reason": "not-started", "checks": {}}
    if os.name == "nt":
        native_cell = root / "_native-prerequisite"
        try:
            native_cell.mkdir(parents=True, exist_ok=True); protect_private(native_cell)
            native_env = isolated_environment(native_cell)
            native_test = native_prerequisite_self_test(native_cell, native_env, args.timeout)
        except (OSError, RuntimeError, ValueError) as error:
            # Keep metadata/install work independent when preflight setup itself
            # cannot run; the closed reason is retained in the safe report.
            native_test = {"status": "FAIL", "reason": "self-test-unavailable", "checks": {
                "preflight-setup": {"status": "FAIL", "reason": "setup-error"}}}
        # A self-test is evidence, not a blanket gate: independent harness
        # stages still run whenever their own runtime boundary is available.
        try:
            shutil.rmtree(native_cell / "native self-test space", ignore_errors=True)
        except OSError:
            pass
    for harness in harnesses:
        phases = {}; cell = root / harness
        try:
            cell.mkdir(parents=True, exist_ok=True); protect_private(cell)
            if resolver is None:
                raise RuntimeError(resolver_error)
            item, diagnostic = resolve_one(resolver, harness, args.model)
            if item is None:
                cid = causal(harness, "metadata", diagnostic.get("category", "unknown"))
                phases["metadata"] = phase("FAIL", "official-metadata-unavailable", cid); any_failure = True
            else: phases["metadata"] = phase("PASS", "official-version-resolved")
        except Exception:
            cid = causal(harness, "metadata", "resolver-error")
            phases["metadata"] = phase("FAIL", "official-metadata-error", cid); item = None; any_failure = True
        try:
            env = isolated_environment(cell)
            missing = [tool for tool in ("pwsh", "node", "npm") if shutil.which(tool, path=env.get("PATH")) is None]
            if missing:
                cause = causal(harness, "prerequisites", "runtime-missing")
                phases["prerequisites"] = phase("FAIL", "required-runtime-missing", cause)
                mark_dependents(phases, PHASES[2:], "prerequisites-failed", cause); any_failure = True
                reports.append({"harness": harness, "phases": phases}); continue
            phases["prerequisites"] = phase("PASS", "native-runtime-present")
        except Exception:
            cause = causal(harness, "prerequisites", "isolation-error")
            phases["prerequisites"] = phase("FAIL", "private-environment-error", cause)
            mark_dependents(phases, PHASES[2:], "prerequisites-failed", cause); any_failure = True
            reports.append({"harness": harness, "phases": phases}); continue
        if item is None:
            cause = phases["metadata"].get("causalId", causal(harness, "metadata", "unresolved"))
            mark_dependents(phases, PHASES[2:], "metadata-failed", cause)
            reports.append({"harness": harness, "phases": phases}); continue
        installer = ROOT / "canary/guest/install-harness.ps1"
        command = ["pwsh", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", str(installer),
                   "-Harness", harness, "-Version", item.version]
        if item.ref: command += ["-Ref", item.ref]
        code, reason = run_bounded(command, cell, env, args.timeout)
        if code != 0:
            marker_reason, marker_diagnostic = installer_diagnostic(cell) if (cell / "installer-result.json").exists() else ("installer-failed", {})
            reason = marker_reason
            if not cleanup_installer_artifacts(cell):
                reason = "private-cleanup-failed"
            cause = causal(harness, "install", reason); phases["install"] = phase("FAIL", "installer-" + reason, cause)
            if marker_diagnostic: phases["install"]["diagnostic"] = marker_diagnostic
            mark_dependents(phases, PHASES[3:], "install-failed", cause); any_failure = True
            reports.append({"harness": harness, "phases": phases}); continue
        installer_diagnostic(cell)
        if not cleanup_installer_artifacts(cell):
            cause = causal(harness, "install", "private-cleanup-failed")
            phases["install"] = phase("FAIL", "installer-private-cleanup-failed", cause)
            mark_dependents(phases, PHASES[3:], "install-failed", cause)
            any_failure = True
            reports.append({"harness": harness, "phases": phases})
            continue
        phases["install"] = phase("PASS", "native-installer-complete")
        probe = ROOT / "canary/guest/probe-harness.ps1"
        build_ok = args.binary.is_file() and args.canary.is_file()
        common = ["pwsh", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", str(probe),
                  "-Harness", harness, "-NanBinary", str(args.binary), "-Canary", str(args.canary), "-Version", item.version]
        if not build_ok:
            cause = causal(harness, "version-doctor", "build-failed")
            mark_dependents(phases, ("version-doctor", "deterministic-contract", "live-tool"), "build-failed", cause)
            any_failure = True
            reports.append({"harness": harness, "phases": phases})
            continue
        probe_env = dict(env); probe_env["NAN_CANARY_PROBE_RESULT"] = str(cell / "probe-result.json")
        for name, stage in (("version-doctor", "version-doctor"), ("deterministic-contract", "deterministic-contract")):
            code, reason = run_bounded(common + ["-Stage", stage], cell, probe_env, args.timeout)
            diagnostic = probe_diagnostic(cell, stage) if (cell / "probe-result.json").exists() else {"status": "failed"}
            marker_failed = diagnostic.get("status") == "failed"
            if code != 0 or marker_failed:
                failure_reason = reason if code != 0 else "diagnostic"
                cause = causal(harness, name, failure_reason); phases[name] = phase("FAIL", "probe-" + failure_reason, cause)
                if diagnostic:
                    diagnostic.pop("status", None)
                    if diagnostic: phases[name]["diagnostic"] = diagnostic
                mark_dependents(phases, ("deterministic-contract", "live-tool") if name == "version-doctor" else ("live-tool",), name + "-failed", cause)
                any_failure = True; break
            phases[name] = phase("PASS", "verified")
        if "live-tool" not in phases:
            if args.mode != "live": phases["live-tool"] = phase("NOT_REQUESTED", "deterministic-mode")
            elif not os.environ.get("NAN_API_KEY"):
                phases["live-tool"] = phase("BLOCKED", "credential-not-configured", causal(harness, "live-tool", "credential"))
            else:
                live_env = dict(probe_env); live_env["NAN_API_KEY"] = os.environ["NAN_API_KEY"]
                code, reason = run_bounded(common + ["-Stage", "live-tool", "-Model", args.model], cell, live_env, args.timeout)
                diagnostic = probe_diagnostic(cell, "live-tool") if (cell / "probe-result.json").exists() else {"status": "failed"}
                marker_failed = diagnostic.get("status") == "failed"
                if code == 0 and not marker_failed:
                    phases["live-tool"] = phase("PASS", "verified")
                else:
                    failure_reason = reason if code != 0 else "diagnostic"
                    cause = causal(harness, "live-tool", failure_reason)
                    phases["live-tool"] = phase("FAIL", "probe-" + failure_reason, cause)
                    diagnostic.pop("status", None)
                    if diagnostic: phases["live-tool"]["diagnostic"] = diagnostic
                    any_failure = True
        reports.append({"harness": harness, "phases": phases})
    for report in reports:
        required = PHASES if args.mode == "live" else PHASES[:-1]
        statuses = [report["phases"][name]["status"] for name in required]
        report["outcome"] = "passed" if all(status == "PASS" for status in statuses) else ("failed" if "FAIL" in statuses else "blocked")
    phase_totals = {status: sum(item["phases"].get(name, {}).get("status") == status for item in reports for name in PHASES) for status in ("PASS", "FAIL", "BLOCKED", "NOT_REQUESTED")}
    causes = {}
    for item in reports:
        for phase_name, value in item["phases"].items():
            if not value.get("causalId"):
                continue
            detail = json.dumps(value.get("diagnostic", {}), sort_keys=True, separators=(",", ":"))
            group = cause_key(phase_name, value.get("reason", ""), detail)
            value["causeGroup"] = group
            entry = causes.setdefault(group, {"count": 0, "phase": phase_name,
                                               "reason": value.get("reason", ""),
                                               "harnesses": [], "diagnostics": []})
            entry["count"] += 1
            entry["harnesses"].append(item["harness"])
            if value.get("diagnostic") and value["diagnostic"] not in entry["diagnostics"]:
                entry["diagnostics"].append(value["diagnostic"])
    report = {"schemaVersion": 1, "platform": {"os": "windows", "architecture": platform.machine().lower()}, "mode": args.mode,
              "model": args.model, "sourceSha": args.source, "harnesses": reports,
              "setup": {key.removeprefix("NAN_DIAGNOSTIC_SETUP_").lower(): os.environ.get(key, "")
                        for key in ("NAN_DIAGNOSTIC_SETUP_CHECKOUT", "NAN_DIAGNOSTIC_SETUP_NODE",
                                    "NAN_DIAGNOSTIC_SETUP_PYTHON", "NAN_DIAGNOSTIC_SETUP_RUST",
                                    "NAN_DIAGNOSTIC_SETUP_SOURCE", "NAN_DIAGNOSTIC_SETUP_BUILD")},
              "totals": {"selected": len(reports), "passed": sum(x["outcome"] == "passed" for x in reports),
                         "failed": sum(x["outcome"] == "failed" for x in reports), "blocked": sum(x["outcome"] == "blocked" for x in reports), "phases": phase_totals},
              "groupedCauses": causes, "nativePrerequisites": native_test}
    return report, any_failure or report["totals"]["failed"] > 0 or (args.mode == "live" and report["totals"]["blocked"] > 0)

def write_outputs(report, output):
    protect_private(output); path = output / "report.json"
    path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"); protect_private(path)
    lines = ["# Native Windows CLI diagnostic", "", f"Mode: `{report['mode']}`  Model: `{report['model']}`", "", "| Harness | Outcome |", "| --- | --- |"]
    lines.extend(f"| {item['harness']} | {item['outcome']} |" for item in report["harnesses"])
    lines += ["", "Totals: " + json.dumps(report["totals"], sort_keys=True), "", "## Native prerequisite self-test"]
    native = report.get("nativePrerequisites", {})
    lines.append(f"- Status: `{native.get('status', 'unknown')}` ({native.get('reason', 'unknown')})")
    for name, result in sorted(native.get("checks", {}).items()):
        lines.append(f"- `{name}`: {result.get('status', 'unknown')} ({result.get('reason', 'unknown')})")
    lines.append("")
    lines.append("## Grouped causes")
    lines.extend(f"- `{cause}`: {entry['count']} phase(s), {entry['phase']} / {entry['reason']} "
                 f"({', '.join(entry['harnesses'])})" for cause, entry in sorted(report["groupedCauses"].items()))
    summary = output / "summary.md"; summary.write_text("\n".join(lines) + "\n", encoding="utf-8"); protect_private(summary)

def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument("--mode", choices=("deterministic", "live"), default="deterministic")
    parser.add_argument("--harnesses", default="all"); parser.add_argument("--model", default="qwen3.6")
    parser.add_argument("--source-sha", "--source", dest="source", default=""); parser.add_argument("--binary", type=Path, required=True); parser.add_argument("--canary", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=Path("windows-canary-report")); parser.add_argument("--timeout", type=int, default=900); args = parser.parse_args(argv)
    if not SAFE_ID.fullmatch(args.model) or args.timeout < 1 or args.timeout > 3600 or (args.source and not SHA.fullmatch(args.source)): parser.error("bounded identity and timeout are required")
    harnesses = list(HARNESSES) if args.harnesses == "all" else [x.strip() for x in args.harnesses.split(",")]
    if not harnesses or len(harnesses) != len(set(harnesses)) or any(x not in HARNESSES for x in harnesses): parser.error("harnesses must be all or distinct known identifiers")
    args.binary = args.binary.resolve(); args.canary = args.canary.resolve()
    report, failed = collect(args, harnesses, args.output.resolve())
    def identity(path):
        try: return {"sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
        except OSError: return {"sha256": None, "status": "missing"}
    report["nanHarness"] = identity(args.binary); report["canary"] = identity(args.canary)
    write_outputs(report, args.output.resolve()); return 1 if failed else 0

if __name__ == "__main__": raise SystemExit(main())
