#!/usr/bin/env python3
"""Run a native Windows compatibility sweep without publishing child output."""
from __future__ import annotations
import argparse, copy, hashlib, importlib.util, json, os, platform, re, shutil, subprocess, sys
import time
import uuid
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
from cell import WindowsJob, finish_stage, protect_private

HARNESSES = ("claude-code", "codex", "opencode", "hermes", "pi", "omp", "prime-agent",
             "deepseek-harness", "openclaw", "cline", "qwen-code", "kimi-code", "aider", "goose", "fx")
PYTHON_VERSION = "3.12"
PHASES = ("metadata", "prerequisites", "install", "version-doctor", "deterministic-contract", "live-tool")
SAFE_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}$")
SHA = re.compile(r"^[0-9a-f]{40}$")
ROOT = Path(__file__).resolve().parents[2]
PROGRESS_ENV = "NAN_HARNESS_CONFORMANCE_PROGRESS"
ASSERTION_ENV = "NAN_HARNESS_CONFORMANCE_ASSERTION"
PROGRESS_SCENARIOS = frozenset(("inventory", "sentinel", "tool-round-trip", "external-prerequisite"))
PROGRESS_STAGES = frozenset(("scenario", "process", "provider-shutdown", "cleanup"))
PROGRESS_STATUSES = frozenset(("started", "passed", "failed"))
PROGRESS_MAX_LINE = 4096
PROGRESS_MAX_RECORDS = 256
PROBE_MARKER_MAX_BYTES = 64 * 1024
ASSERTION_MAX_RECORDS = 32
# A harness install that fails for a transient reason (network, registry, a download) is
# retried once inside the same phase; a deterministic refusal is not.
INSTALL_ATTEMPTS = 2
INSTALL_DETERMINISTIC_REASONS = frozenset((
    "capability-not-implemented", "invalid-frozen-ref", "invalid-version",
    "official-metadata-no-windows-asset",
))
# Closed reasons a failed conformance assertion may report, one per failure domain.
PROBE_ASSERTION_CODES = frozenset((
    "process-failed", "provider-incomplete", "inventory-mismatch", "tool-call-missing",
    "tool-call-mismatch", "tool-traffic-unexpected", "tool-result-mismatch",
    "tool-result-shell-error", "marker-missing",
    "side-effect-missing", "filesystem-unreadable",
))

class BatchBudget:
    """Monotonic collector deadline with a small reserve for finalization."""
    def __init__(self, seconds, clock=time.monotonic):
        self.clock = clock
        self.deadline = clock() + max(0, seconds)

    def remaining(self):
        return max(0.0, self.deadline - self.clock())

    def exhausted(self):
        return self.remaining() <= 0

    def child_timeout(self, configured, cleanup_reserve=20):
        return max(0.0, min(float(configured), self.remaining() - cleanup_reserve))

def diagnostic_batch_budget(elapsed_seconds, job_seconds=120 * 60,
                            finalization_seconds=600, startup_slack_seconds=120,
                            cap_seconds=6000):
    """Derive a safe collector allowance from elapsed workflow setup time."""
    remaining = int(job_seconds - elapsed_seconds - finalization_seconds - startup_slack_seconds)
    return max(1, min(cap_seconds, remaining))

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

def unfinished_harness(harness, reason="not-started"):
    phases = {name: phase("NOT_REQUESTED", reason) for name in PHASES}
    return {"harness": harness, "outcome": "blocked", "phases": phases}

def _report(args, reports, native_test, selected, setup=None, active=None):
    """Build a pure safe snapshot, including the currently active harness."""
    # Checkpoints must never mutate collector state: a later marker can turn a
    # blocked snapshot into passed, and a shallow setdefault here used to cache
    # the first outcome forever.
    completed = copy.deepcopy(list(reports))
    active = copy.deepcopy(active) if active else None
    by_name = {item["harness"]: item for item in completed}
    if active and active.get("harness") and active["harness"] not in by_name:
        current = copy.deepcopy(active.get("phases", {}))
        by_name[active["harness"]] = {"harness": active["harness"], "phases": current}
    snapshots = []
    required = PHASES if args.mode == "live" else PHASES[:-1]
    for name in selected:
        item = copy.deepcopy(by_name.get(name, unfinished_harness(name)))
        phases = item.setdefault("phases", {})
        # Missing phases in a completed record are genuinely not started;
        # missing phases in an active record are unfinished work.
        active_item = active and active.get("harness") == name
        current_phase = active.get("phase") if active_item else None
        for phase_name in PHASES:
            unfinished = active_item and phase_name == current_phase
            phases.setdefault(phase_name, phase("BLOCKED", "unfinished") if unfinished
                              else phase("NOT_REQUESTED", "not-started"))
        statuses = [phases.get(phase_name, {}).get("status") for phase_name in required]
        item["outcome"] = ("passed" if all(status == "PASS" for status in statuses)
                            else "failed" if "FAIL" in statuses else "blocked")
        snapshots.append(item)
    reports = snapshots
    phase_totals = {status: sum(item["phases"].get(name, {}).get("status") == status
                                for item in reports for name in PHASES)
                    for status in ("PASS", "FAIL", "BLOCKED", "NOT_REQUESTED")}
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
            entry["count"] += 1; entry["harnesses"].append(item["harness"])
            if value.get("diagnostic") and value["diagnostic"] not in entry["diagnostics"]:
                entry["diagnostics"].append(value["diagnostic"])
    return {"schemaVersion": 1, "platform": {"os": "windows", "architecture": platform.machine().lower()},
            "mode": args.mode, "model": args.model, "sourceSha": args.source, "harnesses": reports,
            "setup": setup or {},
            "totals": {"selected": len(selected), "passed": sum(x["outcome"] == "passed" for x in reports),
                       "failed": sum(x["outcome"] == "failed" for x in reports),
                       "blocked": sum(x["outcome"] == "blocked" for x in reports), "phases": phase_totals},
            "groupedCauses": causes, "nativePrerequisites": native_test}

def _run_bounded(invocation, cwd, env, timeout):
    """Run one private child and kill its complete tree on timeout."""
    try:
        flags = getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
        suspended = os.name == "nt"
        if suspended:
            flags |= getattr(subprocess, "CREATE_SUSPENDED", 0x00000004)
        child = subprocess.Popen(invocation, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
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
                try:
                    child.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    child.kill()
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
            return None, "launch-failed"
    except (OSError, ValueError, subprocess.SubprocessError):
        return None, "launch-failed"

def run_bounded(argv, cwd, env, timeout):
    return _run_bounded(argv, cwd, env, timeout)

def run_bounded_command_line(command_line, cwd, env, timeout):
    """Run a raw Windows command line, bypassing list2cmdline/CRT quoting."""
    return _run_bounded(command_line, cwd, env, timeout)

def assertion_path(cell):
    """Return a fresh private assertion path for one deterministic invocation."""
    return cell / ("conformance-assertion-" + uuid.uuid4().hex + ".jsonl")


def read_assertions(path):
    """Read only the closed assertion contract; an unknown or torn record is unusable."""
    if not path.exists():
        return {"assertionStatus": "absent"}
    try:
        with path.open("rb") as stream:
            raw = stream.read(ASSERTION_MAX_RECORDS * 64 + 1)
        if len(raw) > ASSERTION_MAX_RECORDS * 64:
            return {"assertionStatus": "corrupt"}
        codes = []
        for line in raw.splitlines():
            payload = line.strip()
            if not payload:
                continue
            code = payload.decode("ascii")
            if code not in PROBE_ASSERTION_CODES:
                return {"assertionStatus": "corrupt"}
            if code not in codes:
                codes.append(code)
        if not codes:
            return {"assertionStatus": "corrupt"}
        return {"assertionStatus": "valid", "assertions": codes}
    except (OSError, UnicodeError, ValueError, TypeError):
        return {"assertionStatus": "corrupt"}


def progress_path(cell):
    """Return a fresh private progress path for one deterministic invocation."""
    return cell / ("conformance-progress-" + uuid.uuid4().hex + ".jsonl")

def last_record_per_scenario(records):
    """The last record of each scenario, in the order the scenarios were entered.

    A failing scenario is not always the last one to run, so the global last record alone
    cannot say which stage of which scenario failed.
    """
    entered = []
    latest = {}
    for record in records:
        name = record["scenario"]
        if name not in latest:
            entered.append(name)
        latest[name] = record
    return [latest[name] for name in entered]


def read_progress(path):
    """Read only the closed progress contract, tolerating a torn final line."""
    if not path.exists():
        return {"progressStatus": "absent"}
    try:
        maximum = PROGRESS_MAX_LINE * PROGRESS_MAX_RECORDS
        with path.open("rb") as stream:
            raw = stream.read(maximum + 1)
        if len(raw) > maximum:
            return {"progressStatus": "corrupt"}
        lines = raw.splitlines(keepends=True)
        records = []
        for index, line in enumerate(lines):
            complete = line.endswith((b"\n", b"\r"))
            payload = line.rstrip(b"\r\n")
            if len(payload) > PROGRESS_MAX_LINE:
                return {"progressStatus": "corrupt"}
            if not complete and index == len(lines) - 1:
                break
            if not complete or not payload:
                return {"progressStatus": "corrupt"}
            value = json.loads(payload.decode("utf-8"))
            if (not isinstance(value, dict) or set(value) != {"schema_version", "scenario", "stage", "status", "elapsed_milliseconds"}
                    or value["schema_version"] != 1 or not isinstance(value["schema_version"], int)
                    or isinstance(value["schema_version"], bool) or value["scenario"] not in PROGRESS_SCENARIOS
                    or value["stage"] not in PROGRESS_STAGES or value["status"] not in PROGRESS_STATUSES
                    or not isinstance(value["elapsed_milliseconds"], int) or isinstance(value["elapsed_milliseconds"], bool)
                    or not 0 <= value["elapsed_milliseconds"] <= 86400000):
                return {"progressStatus": "corrupt"}
            records.append(value)
            if len(records) > PROGRESS_MAX_RECORDS:
                return {"progressStatus": "corrupt"}
        if not records:
            return {"progressStatus": "absent" if not raw else "corrupt"}
        return {"progressStatus": "valid", "progress": records[-1],
                "progressScenarios": last_record_per_scenario(records)}
    except (OSError, UnicodeError, ValueError, TypeError, json.JSONDecodeError):
        return {"progressStatus": "corrupt"}

def _self_test_result(code, reason):
    return {"status": "PASS" if code == 0 else "FAIL", "reason": reason}

def _self_test_reason(name, reason):
    """Map private process outcomes to bounded, actionable self-test reasons."""
    if reason == "nonzero":
        return {
            "cmd-node-npm": "version-probe-failed",
            "cmd-argument-roundtrip": "argument-roundtrip-failed",
            "npm-registry": "registry-probe-failed",
            "git-bash": "shell-probe-failed",
        }.get(name, "tool-runtime-failed")
    if reason == "unavailable":
        return "tool-runtime-unavailable"
    return reason

def _git_for_windows_bash(env):
    """Return Git for Windows' bundled bash, never a generic/WSL bash."""
    git = shutil.which("git.exe", path=env.get("PATH"))
    if not git:
        return None
    try:
        candidate = Path(git).parent.parent / "usr" / "bin" / "bash.exe"
    except (OSError, NotImplementedError, ValueError):
        return None
    return str(candidate) if candidate.is_file() else None

def native_prerequisite_self_test(cell, env, timeout, python_version=PYTHON_VERSION, budget=None,
                                  checkpoint=None):
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

    def check(name, argv, required=(), run_env=None, raw_command_line=None):
        def publish(value):
            if checkpoint is not None:
                checkpoint(name, copy.deepcopy(checks), value is not None)
        checks[name] = {"status": "BLOCKED", "reason": "unfinished"}
        publish(None)
        # Resolve only executable names, never report the resolved path.  This
        # distinguishes a PATH/runtime prerequisite from a command failure
        # while keeping machine-specific paths out of the safe report.
        for executable in required:
            if shutil.which(executable, path=env.get("PATH")) is None:
                checks[name] = _self_test_result(None, "executable-missing")
                publish(checks[name])
                return
        try:
            per_check = budget.child_timeout(bounded) if budget is not None else bounded
            if per_check <= 0:
                checks[name] = _self_test_result(None, "deadline-exhausted")
                publish(checks[name])
                return
            runner = run_bounded_command_line if raw_command_line is not None else run_bounded
            code, reason = runner(raw_command_line if raw_command_line is not None else argv,
                                  fixture, env if run_env is None else run_env, per_check)
        except (OSError, ValueError, RuntimeError):
            code, reason = None, "unavailable"
        checks[name] = _self_test_result(code, _self_test_reason(name, reason))
        publish(checks[name])

    check("pwsh-parser", ["pwsh", "-NoProfile", "-NonInteractive", "-Command",
                          "[void][int]7"], ("pwsh",))
    comspec = env.get("ComSpec") or env.get("COMSPEC")
    if not comspec:
        checks["cmd-node-npm"] = {"status": "FAIL", "reason": "comspec-missing"}
        checks["npm-registry"] = {"status": "FAIL", "reason": "comspec-missing"}
        if checkpoint:
            checkpoint("cmd-node-npm", copy.deepcopy(checks), True)
            checkpoint("npm-registry", copy.deepcopy(checks), True)
    else:
        quoted = 'cd /d "' + str(fixture).replace('"', '\\"') + '" && node --version && npm.cmd --version'
        # /s changes /c quote stripping and can corrupt a quoted working
        # directory; the command is already one ArgumentList element.
        check("cmd-node-npm", [comspec, "/d", "/c", quoted], ("node", "npm.cmd"), raw_command_line=f'"{comspec}" /d /c {quoted}')
        registry = 'cd /d "' + str(fixture).replace('"', '\\"') + '" && npm.cmd view npm version --fetch-retries=0 --fetch-timeout=15000 --json'
        check("npm-registry", [comspec, "/d", "/c", registry], ("npm.cmd",), raw_command_line=f'"{comspec}" /d /c {registry}')
    prefix = env.get("NPM_CONFIG_PREFIX", "")
    cache = env.get("NPM_CONFIG_CACHE", "")
    isolated = str(cell) in prefix and str(cell) in cache
    checks["npm-isolation"] = {"status": "PASS" if isolated else "FAIL",
                                "reason": "private-prefix-cache" if isolated else "prefix-cache-outside-cell"}
    if checkpoint:
        checkpoint("npm-isolation", copy.deepcopy(checks), True)

    venv = fixture / "venv"
    check("python-venv", ["py.exe", f"-{python_version}", "-m", "venv", str(venv)])
    check("python-pip", [str(venv / "Scripts/python.exe"), "-m", "pip", "--version"])
    git_bash = _git_for_windows_bash(env)
    if git_bash:
        check("git-bash", [git_bash, "--noprofile", "--norc", "-c", "exit 0"], (git_bash,))
    else:
        checks["git-bash"] = _self_test_result(None, "git-for-windows-missing")
        if checkpoint:
            checkpoint("git-bash", copy.deepcopy(checks), True)
    if comspec:
        cmd_roundtrip = fixture / "cmd-argv-probe.cmd"
        cmd_roundtrip_result = fixture / "cmd-argv-result.txt"
        cmd_roundtrip.write_text('@echo off\r\n> "%~1" echo %~2\r\nexit /b 0\r\n', encoding="ascii")
        roundtrip = 'call "' + str(cmd_roundtrip).replace('"', '\\"') + '" "' + str(cmd_roundtrip_result).replace('"', '\\"') + '" "NAN_CMD_ARG_OK"'
        check("cmd-argument-roundtrip", [comspec, "/d", "/c", roundtrip], raw_command_line=f'"{comspec}" /d /c {roundtrip}')
        if checks["cmd-argument-roundtrip"]["status"] == "PASS":
            try:
                if cmd_roundtrip_result.read_text(encoding="ascii").strip() != "NAN_CMD_ARG_OK":
                    checks["cmd-argument-roundtrip"] = _self_test_result(None, "argument-roundtrip-failed")
            except (OSError, UnicodeError):
                checks["cmd-argument-roundtrip"] = _self_test_result(None, "argument-roundtrip-failed")
            if checkpoint:
                checkpoint("cmd-argument-roundtrip", copy.deepcopy(checks), True)
    else:
        checks["cmd-argument-roundtrip"] = _self_test_result(None, "comspec-missing")
        if checkpoint:
            checkpoint("cmd-argument-roundtrip", copy.deepcopy(checks), True)
    # run_bounded's suspended child + Job Object path is the native containment
    # and private-DACL probe; protect_private above verifies the output ACL path.
    check("job-dacl", ["pwsh", "-NoProfile", "-NonInteractive", "-Command", "exit 0"])
    doctor_marker = cell / "doctor-probe-result.json"
    doctor_producer = cell / "doctor-producer.ps1"
    doctor_producer.write_text(
        "Write-Output '{\"schemaVersion\":8,\"offline\":true,\"harness\":\"fx\",\"level\":\"ok\","
        "\"installed\":true,\"version\":\"1.2.3\",\"warnings\":[],\"safeToShare\":true}'\nexit 0\n",
        encoding="utf-8",
    )
    doctor_env = dict(env); doctor_env["NAN_CANARY_PROBE_RESULT"] = str(doctor_marker)
    probe = ROOT / "canary/guest/probe-harness.ps1"
    check("doctor-json", ["pwsh", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
                           "-File", str(probe), "-Harness", "fx", "-Stage", "version-doctor",
                           "-NanBinary", str(doctor_producer), "-Canary", str(doctor_producer), "-Version", "1.2.3"], ("pwsh",), doctor_env)
    if checks["doctor-json"]["status"] == "PASS":
        try:
            doctor = json.loads(doctor_marker.read_text(encoding="utf-8-sig"))
            if doctor.get("status") != "passed" or doctor.get("exitCode") != 0:
                checks["doctor-json"] = _self_test_result(None, "doctor-json-invalid")
        except (OSError, ValueError, TypeError):
            checks["doctor-json"] = _self_test_result(None, "doctor-json-invalid")
        if checkpoint:
            checkpoint("doctor-json", copy.deepcopy(checks), True)
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
                                    # The official installers of Kimi and OpenClaw place their
                                    # shims inside the private home; both locations join PATH.
                                    str(home / ".kimi-code" / "bin"), str(home / ".local" / "bin"),
                                    str(home / "AppData" / "Roaming" / "npm"),
                                    str(home / ".npm-global"), original_path))
    git_bash = _git_for_windows_bash(env)
    if git_bash:
        # A harness whose tools run through a POSIX shell on Windows is told where the shell
        # Git for Windows ships lives, so it does not have to rediscover it behind an
        # isolated environment. The conformance environment forwards exactly these names to
        # the harness process.
        env["NAN_HARNESS_GIT_BASH"] = git_bash
        env["KIMI_SHELL_PATH"] = git_bash
        env["KIMI_CLI_GIT_BASH_PATH"] = git_bash
    return env

def resolver_module():
    spec = importlib.util.spec_from_file_location("windows_cli_suite", Path(__file__).with_name("cli-suite.py"))
    module = importlib.util.module_from_spec(spec); assert spec.loader is not None; spec.loader.exec_module(module)
    return module

def resolve_one(module, harness, model, timeout=20):
    # The per-request socket timeout bounds each urllib operation.  DNS and
    # response reads across multiple metadata requests are not a hard
    # aggregate wall-clock deadline; collect() rechecks its monotonic budget
    # between requests and records deadline-exhausted before the next phase.
    try:
        resolved, unresolved = module.resolve_manifest([harness], "windows", "x86_64", model,
                                                       timeout=max(1, timeout))
    except TypeError as error:
        # Focused tests and downstream adapters may expose the pre-timeout
        # resolver contract; preserve their behavior without weakening the
        # production cli-suite deadline-aware call.
        if "timeout" not in str(error):
            raise
        resolved, unresolved = module.resolve_manifest([harness], "windows", "x86_64", model)
    if unresolved: return None, unresolved[0].diagnostic or {"category": "unknown"}
    return resolved[0], None

def mark_dependents(phases, names, reason, cause):
    for name in names: phases[name] = phase("BLOCKED", reason, cause)

_INSTALLER_REASONS = frozenset({
    "passed", "installer-failed", "official-asset-missing", "official-metadata-probe-failed",
    "official-metadata-no-windows-asset", "capability-not-implemented", "invalid-frozen-ref",
    "invalid-version", "launcher-verify-failed", "launcher-missing",
})
_INSTALLER_DIAGNOSTIC_KEYS = frozenset({
    "subphase", "executable", "exitCode", "win32Error", "httpStatus", "assetReason",
    "npmCode", "pipCategory", "processReason", "processCategory",
})
# Closed failure classes a nested installer child (pwsh, git, uv) may report.
_INSTALLER_PROCESS_CATEGORIES = frozenset({
    "network-dns", "network-timeout", "network-connection", "tls-certificate", "permission",
    "disk-space", "tool-missing", "package-not-found", "installer-refused",
})
_INSTALLER_SUBPHASES = frozenset({
    "metadata", "download", "archive", "asset-selection", "execute", "install",
    "virtualenv", "cleanup", "unknown",
})
_INSTALLER_EXECUTABLES = frozenset({
    "unknown", "npm-node", "npm-cmd", "pwsh", "py-launcher", "python", "uv", "github-api",
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
    "package-not-found", "permission", "tls-certificate", "npm-command-missing", "npm-unknown",
})
_INSTALLER_PIP_CATEGORIES = frozenset({
    "network-dns", "network-connection", "network-timeout", "package-not-found",
    "permission", "tls-certificate", "pip-missing", "pip-unknown",
})
_INSTALLER_PROCESS_REASONS = frozenset({"win32-launch-failed", "exit-nonzero", "native-unavailable"})
_PROBE_DIAGNOSTICS = frozenset({
    "doctor-child-launch", "doctor-exit-nonzero", "doctor-output-invalid", "doctor-schema-invalid",
    "doctor-version-missing", "doctor-version-invalid", "doctor-version-mismatch",
    "conformance-child-launch", "conformance-exit-nonzero",
    "conformance-output-invalid", "conformance-schema-invalid", "conformance-scenario-missing",
    "conformance-scenario-failed", "conformance-inventory-failed", "conformance-check-invalid",
    "conformance-inventory-operational-failed", "doctor-exit-missing", "conformance-exit-missing",
    "live-child-launch", "live-exit-nonzero",
    "live-exit-missing", "live-credential-missing", "live-tool-evidence-missing",
    "live-read-marker-missing", "live-completion-marker-missing", "live-bridge-sentinel",
    "live-usage-invalid", "live-usage-summary-missing", "probe-unexpected-failure",
})
# Marker field names the probe itself can emit. A rejected marker publishes only these
# names, plus a count of anything else, so the shape of a failure is diagnosable without
# ever copying a value out of the marker.
_PROBE_MARKER_FIELDS_STAGES = frozenset(PHASES) | {
    "complete", "harness-run", "read-marker", "completion-marker", "bridge-sentinel",
    "usage-evidence", "usage-summary",
}
_PROBE_MARKER_FIELDS = frozenset({
    "schemaVersion", "stage", "status", "diagnostics", "exitCode", "doctorVersion",
    "doctorExpectedVersion", "doctorReason", "doctorSchemaReason", "discoveryCode",
    "inventoryFailureReasons", "inventoryProcess", "failedScenarios",
})
_SEMVER = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$")
_DOCTOR_REASONS = frozenset({"missing", "invalid", "mismatch", "discovery-error"})
_DOCTOR_SCHEMA_REASONS = frozenset({"unknown-field", "required-field", "field-type", "field-value"})
_DISCOVERY_CODES = frozenset({f"NH-DISCOVERY-{index:03d}" for index in range(1, 8)})
_CONFORMANCE_REASONS = frozenset({
    "process-failed", "marker-missing", "provider-failed", "provider-shutdown-failed",
    "daemon-cleanup-failed",
})
_INVENTORY_PROCESS_STATUSES = frozenset({
    "completed", "nonzero-exit", "launch-error", "environment-error", "timeout", "missing-output",
    "capture-error", "cleanup-error",
})
_INVENTORY_CLEANUP_STAGES = frozenset({"terminate", "wait", "wait-timeout", "capture-timeout"})
_INVENTORY_CLEANUP_STREAMS = frozenset({"stdout", "stderr"})
_LIVE_FAILURE_STAGES = frozenset({
    "live-tool", "harness-run", "read-marker", "completion-marker", "bridge-sentinel",
    "usage-evidence", "usage-summary",
})

# Closed vocabularies a rejected marker may still report, so a failure is diagnosable
# without ever copying free text out of the marker.
# `status` is deliberately absent: the result's own status is the phase outcome, and a
# rejected marker never turns it into a pass.
_MARKER_ENUM_FIELDS = {
    "stage": _PROBE_MARKER_FIELDS_STAGES,
    "doctorReason": _DOCTOR_REASONS,
    "doctorSchemaReason": _DOCTOR_SCHEMA_REASONS,
    "discoveryCode": _DISCOVERY_CODES,
}


def _invalid_marker(value):
    """The closed result for a marker that exists but violates its schema."""
    result = {"status": "failed", "markerState": "invalid"}
    if isinstance(value, dict):
        result["markerFields"] = sorted(name for name in value if name in _PROBE_MARKER_FIELDS)
        unexpected = sum(1 for name in value if name not in _PROBE_MARKER_FIELDS)
        if unexpected:
            result["unexpectedFieldCount"] = unexpected
        for name, vocabulary in _MARKER_ENUM_FIELDS.items():
            item = value.get(name)
            if isinstance(item, str) and item in vocabulary:
                result[name] = item
        exit_code = value.get("exitCode")
        if isinstance(exit_code, int) and not isinstance(exit_code, bool) and -1 <= exit_code <= 65535:
            result["exitCode"] = exit_code
        diagnostics = value.get("diagnostics")
        if (isinstance(diagnostics, list) and len(diagnostics) <= 16
                and all(isinstance(item, str) and item in _PROBE_DIAGNOSTICS
                        for item in diagnostics)):
            result["diagnostics"] = list(diagnostics)
    return result


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
        elif key == "processCategory":
            if not isinstance(item, str) or item not in _INSTALLER_PROCESS_CATEGORIES:
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

def _safe_inventory_process(value):
    if not isinstance(value, dict) or set(value) - {"status", "exitCode", "osErrorCode", "timeoutMilliseconds", "cleanupStage", "cleanupStream"}:
        return None
    status = value.get("status")
    if not isinstance(status, str) or status not in _INVENTORY_PROCESS_STATUSES:
        return None
    for key, bounds in (("exitCode", (-2147483648, 2147483647)), ("osErrorCode", (0, 4294967295)), ("timeoutMilliseconds", (0, 86400000))):
        if key in value and (not isinstance(value[key], int) or isinstance(value[key], bool) or not bounds[0] <= value[key] <= bounds[1]):
            return None
    exit_code = value.get("exitCode")
    os_error_code = value.get("osErrorCode")
    timeout_milliseconds = value.get("timeoutMilliseconds")
    cleanup_stage = value.get("cleanupStage")
    cleanup_stream = value.get("cleanupStream")
    if cleanup_stage is not None and (not isinstance(cleanup_stage, str) or cleanup_stage not in _INVENTORY_CLEANUP_STAGES):
        return None
    if cleanup_stream is not None and (not isinstance(cleanup_stream, str) or cleanup_stream not in _INVENTORY_CLEANUP_STREAMS):
        return None
    if status == "completed" and (exit_code != 0 or os_error_code is not None or timeout_milliseconds is not None or cleanup_stage is not None or cleanup_stream is not None):
        return None
    if status == "nonzero-exit" and (exit_code == 0 or os_error_code is not None or timeout_milliseconds is not None or cleanup_stage is not None or cleanup_stream is not None):
        return None
    if status in {"launch-error", "environment-error"} and (exit_code is not None or timeout_milliseconds is not None or cleanup_stage is not None or cleanup_stream is not None):
        return None
    if status == "timeout" and (exit_code is not None or os_error_code is not None or cleanup_stage is not None or cleanup_stream is not None or not timeout_milliseconds):
        return None
    if status in {"missing-output", "capture-error"} and (cleanup_stage is not None or cleanup_stream is not None or any(
            item is not None for item in (exit_code, os_error_code, timeout_milliseconds))):
        return None
    if status == "cleanup-error" and (exit_code is not None or timeout_milliseconds is not None
                                       or cleanup_stage is None or cleanup_stream is None):
        return None
    return {key: value[key] for key in ("status", "exitCode", "osErrorCode", "timeoutMilliseconds", "cleanupStage", "cleanupStream") if key in value}

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
    result = {"status": "failed", "markerState": "absent"}
    try:
        try:
            if marker.stat().st_size > PROBE_MARKER_MAX_BYTES:
                return {"status": "failed", "markerState": "invalid"}
            with marker.open("rb") as stream:
                raw = stream.read(PROBE_MARKER_MAX_BYTES + 1)
        except FileNotFoundError:
            return result
        if len(raw) > PROBE_MARKER_MAX_BYTES:
            return {"status": "failed", "markerState": "invalid"}
        value = json.loads(raw.decode("utf-8-sig"))
        result["markerState"] = "valid"
        if not isinstance(value, dict) or value.get("schemaVersion") not in {1, 2}:
            result["markerState"] = "invalid"
            return result
        if not isinstance(value.get("stage"), str) or value.get("status") not in {"passed", "failed"}:
            result["markerState"] = "invalid"
            return result
        result = {"status": value["status"], "markerState": "valid", "stage": value["stage"]}
        if value["schemaVersion"] == 1:
            if set(value) - {"schemaVersion", "stage", "status", "diagnostic"}:
                return _invalid_marker(value)
            return result
        allowed = {"schemaVersion", "stage", "status", "diagnostics", "exitCode",
                   "doctorVersion", "doctorExpectedVersion", "doctorReason", "discoveryCode",
                   "doctorSchemaReason", "inventoryFailureReasons", "inventoryProcess",
                   "failedScenarios"}
        if set(value) - allowed:
            return _invalid_marker(value)
        for key in ("doctorVersion", "doctorExpectedVersion"):
            if key in value and (not isinstance(value[key], str) or len(value[key]) > 64 or not _SEMVER.fullmatch(value[key])):
                return _invalid_marker(value)
        if "doctorReason" in value and (not isinstance(value["doctorReason"], str)
                                         or value["doctorReason"] not in _DOCTOR_REASONS):
            return _invalid_marker(value)
        if "doctorSchemaReason" in value and (not isinstance(value["doctorSchemaReason"], str)
                                               or value["doctorSchemaReason"] not in _DOCTOR_SCHEMA_REASONS):
            return _invalid_marker(value)
        if "discoveryCode" in value and (not isinstance(value["discoveryCode"], str)
                                          or value["discoveryCode"] not in _DISCOVERY_CODES
                                          or value.get("doctorReason") != "discovery-error"):
            return _invalid_marker(value)
        if "doctorReason" in value and value["doctorReason"] == "discovery-error" and "discoveryCode" not in value:
            return _invalid_marker(value)
        if "inventoryFailureReasons" in value and (expected_stage != "deterministic-contract"
                                                    or not isinstance(value["inventoryFailureReasons"], list)
                                                    or len(value["inventoryFailureReasons"]) > 5
                                                    or len(set(value["inventoryFailureReasons"])) != len(value["inventoryFailureReasons"])
                                                    or any(not isinstance(reason, str) or reason not in _CONFORMANCE_REASONS
                                                           for reason in value["inventoryFailureReasons"])):
            return _invalid_marker(value)
        failed_scenarios = value.get("failedScenarios")
        if failed_scenarios is not None and (
                expected_stage != "deterministic-contract"
                or not isinstance(failed_scenarios, list) or not failed_scenarios
                or len(failed_scenarios) > len(PROGRESS_SCENARIOS)
                or len(set(failed_scenarios)) != len(failed_scenarios)
                or any(not isinstance(name, str) or name not in PROGRESS_SCENARIOS
                       for name in failed_scenarios)):
            return _invalid_marker(value)
        inventory_process = _safe_inventory_process(value["inventoryProcess"]) if "inventoryProcess" in value else None
        if "inventoryProcess" in value and (expected_stage != "deterministic-contract" or inventory_process is None):
            return _invalid_marker(value)
        diagnostics = value.get("diagnostics", [])
        if not isinstance(diagnostics, list) or len(diagnostics) > 16 or any(
                not isinstance(item, str) or item not in _PROBE_DIAGNOSTICS for item in diagnostics):
            return _invalid_marker(value)
        if value["status"] == "passed":
            if value.get("stage") != "complete" or value.get("exitCode") != 0 or diagnostics:
                return _invalid_marker(value)
        elif not diagnostics:
            return _invalid_marker(value)
        elif expected_stage == "live-tool":
            if value.get("stage") not in _LIVE_FAILURE_STAGES:
                return _invalid_marker(value)
        elif value.get("stage") != expected_stage or "exitCode" not in value:
            return _invalid_marker(value)
        result["diagnostics"] = diagnostics
        if "exitCode" in value:
            if not isinstance(value["exitCode"], int) or isinstance(value["exitCode"], bool) or not (-1 <= value["exitCode"] <= 65535):
                return _invalid_marker(value)
            result["exitCode"] = value["exitCode"]
        for key in ("doctorVersion", "doctorExpectedVersion", "doctorReason", "doctorSchemaReason", "discoveryCode", "inventoryFailureReasons"):
            if key in value:
                result[key] = value[key]
        if failed_scenarios is not None:
            result["failedScenarios"] = sorted(failed_scenarios)
        if inventory_process is not None:
            result["inventoryProcess"] = inventory_process
        return result
    except (OSError, UnicodeError, ValueError, TypeError, KeyError):
        return {"status": "failed", "markerState": "invalid" if marker.exists() else "absent"}
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
    selected = list(harnesses)
    budget = BatchBudget(getattr(args, "budget_seconds", 6000), getattr(args, "clock", time.monotonic))
    active = {"harness": None, "phases": {}}
    setup = {key.removeprefix("NAN_DIAGNOSTIC_SETUP_").lower(): os.environ.get(key, "")
             for key in ("NAN_DIAGNOSTIC_SETUP_CHECKOUT", "NAN_DIAGNOSTIC_SETUP_NODE",
                         "NAN_DIAGNOSTIC_SETUP_PYTHON", "NAN_DIAGNOSTIC_SETUP_RUST",
                         "NAN_DIAGNOSTIC_SETUP_SOURCE", "NAN_DIAGNOSTIC_SETUP_FIXTURES",
                         "NAN_DIAGNOSTIC_SETUP_RUST_FIXTURE", "NAN_DIAGNOSTIC_SETUP_BUILD")}

    def checkpoint():
        write_outputs(_report(args, reports, native_test, selected, setup, active), output)
    native_test = {"status": "NOT_RUN", "reason": "not-started", "checks": {}}
    def native_checkpoint(name, checks, complete):
        native_test["checks"] = checks
        native_test["reason"] = "verified" if complete else "unfinished"
        if complete:
            native_test.pop("currentCheck", None)
        else:
            native_test["currentCheck"] = name
        checkpoint()
    # Persist the initial state before any native process can consume the
    # deadline or be interrupted.  This also makes native self-test absence
    # distinguishable from a completed PASS/FAIL result.
    checkpoint()
    if os.name == "nt":
        native_cell = root / "_native-prerequisite"
        try:
            native_budget = budget.child_timeout(args.timeout)
            if native_budget <= 0:
                native_test = {"status": "BLOCKED", "reason": "deadline-exhausted", "checks": {}}
            else:
                native_test = {"status": "NOT_RUN", "reason": "unfinished", "checks": {}}
            native_cell.mkdir(parents=True, exist_ok=True); protect_private(native_cell)
            if native_test["status"] != "BLOCKED":
                native_env = isolated_environment(native_cell)
                native_test = native_prerequisite_self_test(native_cell, native_env,
                                                            max(1, int(native_budget)), args.python_version,
                                                            budget=budget, checkpoint=native_checkpoint)
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
    if args.mode == "native-diagnostic":
        # Native mode intentionally selects no harnesses, but setup remains a
        # required CI result.  Dropping it here made a failed fixture step
        # look successful to the workflow's final gate.
        report = _report(args, reports, native_test, [], setup, active)
        write_outputs(report, output)
        setup_failed = any(value not in ("", "success") for value in setup.values())
        return report, native_test.get("status") != "PASS" or setup_failed
    checkpoint()
    for harness in selected:
        if budget.exhausted():
            any_failure = True; checkpoint(); break
        phases = {}; cell = root / harness
        active["harness"] = harness; active["phases"] = phases; active["phase"] = "metadata"
        checkpoint()
        try:
            cell.mkdir(parents=True, exist_ok=True); protect_private(cell)
            if resolver is None:
                raise RuntimeError(resolver_error)
            metadata_timeout = budget.child_timeout(20)
            if metadata_timeout <= 0:
                phases["metadata"] = phase("BLOCKED", "deadline-exhausted")
                any_failure = True; reports.append({"harness": harness, "phases": phases})
                active["harness"] = None; active["phases"] = {}; active["phase"] = None; checkpoint(); continue
            item, diagnostic = resolve_one(resolver, harness, args.model, int(metadata_timeout))
            if item is None:
                cid = causal(harness, "metadata", diagnostic.get("category", "unknown"))
                phases["metadata"] = phase("FAIL", "official-metadata-unavailable", cid); any_failure = True
            else: phases["metadata"] = phase("PASS", "official-version-resolved")
        except Exception:
            cid = causal(harness, "metadata", "resolver-error")
            phases["metadata"] = phase("FAIL", "official-metadata-error", cid); item = None; any_failure = True
        active["phase"] = "prerequisites"
        checkpoint()
        try:
            env = isolated_environment(cell)
            missing = [tool for tool in ("pwsh", "node", "npm") if shutil.which(tool, path=env.get("PATH")) is None]
            if missing:
                cause = causal(harness, "prerequisites", "runtime-missing")
                phases["prerequisites"] = phase("FAIL", "required-runtime-missing", cause)
                mark_dependents(phases, PHASES[2:], "prerequisites-failed", cause); any_failure = True
                reports.append({"harness": harness, "phases": phases}); active["harness"] = None; active["phases"] = {}; active["phase"] = None; continue
            phases["prerequisites"] = phase("PASS", "native-runtime-present")
            checkpoint()
        except Exception:
            cause = causal(harness, "prerequisites", "isolation-error")
            phases["prerequisites"] = phase("FAIL", "private-environment-error", cause)
            mark_dependents(phases, PHASES[2:], "prerequisites-failed", cause); any_failure = True
            reports.append({"harness": harness, "phases": phases}); active["harness"] = None; active["phases"] = {}; active["phase"] = None; continue
        if item is None:
            cause = phases["metadata"].get("causalId", causal(harness, "metadata", "unresolved"))
            mark_dependents(phases, PHASES[2:], "metadata-failed", cause)
            reports.append({"harness": harness, "phases": phases}); active["harness"] = None; active["phases"] = {}; active["phase"] = None; continue
        active["phase"] = "install"; checkpoint()
        installer = ROOT / "canary/guest/install-harness.ps1"
        command = ["pwsh", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", str(installer),
                   "-Harness", harness, "-Version", item.version, "-PythonVersion", args.python_version]
        if item.ref: command += ["-Ref", item.ref]
        attempts = 0
        while True:
            child_timeout = budget.child_timeout(args.timeout)
            if child_timeout <= 0:
                phases["install"] = phase("BLOCKED", "deadline-exhausted")
                reports.append({"harness": harness, "phases": phases, "outcome": "blocked"})
                any_failure = True; checkpoint(); active["harness"] = None; active["phases"] = {}; active["phase"] = None; break
            code, reason = run_bounded(command, cell, env, child_timeout)
            if code == 0:
                break
            marker_reason, marker_diagnostic = installer_diagnostic(cell) if (cell / "installer-result.json").exists() else ("installer-failed", {})
            process_reason = reason
            # A marker describes installer internals, but never replaces the
            # typed parent outcome (launch failure, timeout, or nonzero exit).
            typed = {"timeout": "installer-timeout", "launch-failed": "installer-launch-failed",
                     "nonzero": "installer-nonzero"}.get(process_reason)
            attempts += 1
            # One bounded retry after a transient installer failure: the marker names the
            # reason, so a nonzero exit whose marker is transient (a registry or download
            # failure) is retried too. A deterministic refusal is reported unchanged.
            if (marker_reason not in INSTALL_DETERMINISTIC_REASONS
                    and attempts < INSTALL_ATTEMPTS and budget.child_timeout(args.timeout) > 0):
                continue
            failure_reason = typed or marker_reason
            cleanup_ok = cleanup_installer_artifacts(cell)
            if not cleanup_ok and not typed:
                failure_reason = "private-cleanup-failed"
            cause = causal(harness, "install", failure_reason); phases["install"] = phase("FAIL", failure_reason, cause)
            if marker_diagnostic: phases["install"]["diagnostic"] = marker_diagnostic
            phases["install"]["causeDetails"] = {"parentReason": process_reason if process_reason in {"timeout", "launch-failed", "nonzero"} else "launch-failed",
                                                    "installerReason": marker_reason}
            if not cleanup_ok:
                phases["install"]["causeDetails"]["cleanupReason"] = "cleanup-failed"
            mark_dependents(phases, PHASES[3:], "install-failed", cause); any_failure = True
            reports.append({"harness": harness, "phases": phases}); checkpoint(); active["harness"] = None; active["phases"] = {}; active["phase"] = None; break
        # A failed install ended this harness inside the retry loop.
        if phases.get("install", {}).get("status") == "FAIL":
            continue
        installer_diagnostic(cell)
        if not cleanup_installer_artifacts(cell):
            cause = causal(harness, "install", "private-cleanup-failed")
            phases["install"] = phase("FAIL", "installer-private-cleanup-failed", cause)
            phases["install"]["causeDetails"] = {"cleanupReason": "cleanup-failed"}
            mark_dependents(phases, PHASES[3:], "install-failed", cause)
            any_failure = True
            reports.append({"harness": harness, "phases": phases}); checkpoint(); active["harness"] = None; active["phases"] = {}; active["phase"] = None; continue
        phases["install"] = phase("PASS", "native-installer-complete")
        probe = ROOT / "canary/guest/probe-harness.ps1"
        build_ok = args.binary.is_file() and args.canary.is_file()
        common = ["pwsh", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", str(probe),
                  "-Harness", harness, "-NanBinary", str(args.binary), "-Canary", str(args.canary), "-Version", item.version]
        if not build_ok:
            cause = causal(harness, "version-doctor", "build-failed")
            mark_dependents(phases, ("version-doctor", "deterministic-contract", "live-tool"), "build-failed", cause)
            any_failure = True
            reports.append({"harness": harness, "phases": phases}); checkpoint(); active["harness"] = None; active["phases"] = {}; active["phase"] = None; continue
        probe_env = dict(env); probe_env["NAN_CANARY_PROBE_RESULT"] = str(cell / "probe-result.json")
        for name, stage in (("version-doctor", "version-doctor"), ("deterministic-contract", "deterministic-contract")):
            active["phase"] = name; checkpoint()
            child_timeout = budget.child_timeout(args.timeout)
            if child_timeout <= 0:
                phases[name] = phase("BLOCKED", "deadline-exhausted")
                mark_dependents(phases, ("deterministic-contract", "live-tool") if name == "version-doctor" else ("live-tool",), "phase-unfinished")
                any_failure = True; checkpoint(); break
            invocation_env = probe_env
            progress = None
            progress_file = None
            assertion_file = None
            assertion_record = None
            if stage == "deterministic-contract":
                progress_file = progress_path(cell)
                # A new path per invocation prevents stale records from a
                # previous process being mistaken for current hang evidence.
                invocation_env = dict(probe_env)
                invocation_env[PROGRESS_ENV] = str(progress_file)
                # The runner also records the closed code of a failed assertion in a second
                # per-invocation file, so a report can say which expectation failed instead
                # of only that a scenario failed.
                assertion_file = assertion_path(cell)
                invocation_env[ASSERTION_ENV] = str(assertion_file)
            code, reason = run_bounded(common + ["-Stage", stage], cell, invocation_env, child_timeout)
            diagnostic = probe_diagnostic(cell, stage)
            if stage == "deterministic-contract" and code != 0:
                progress = read_progress(progress_file)
                assertion_record = read_assertions(assertion_file)
            marker_failed = diagnostic.get("status") == "failed"
            if code != 0 or marker_failed:
                # The canary treats the tool inventory as maintenance evidence and the hosted gate
                # does not block on an inventory-only failure, so the native collector keeps the same
                # policy: the drift stays in the phase evidence and the functional contracts decide.
                scenarios = diagnostic.get("failedScenarios") or []
                if scenarios == ["inventory"]:
                    phases[name] = phase("PASS", "verified-with-inventory-drift")
                    diagnostic.pop("status", None)
                    if progress:
                        diagnostic["progress"] = progress
                    if assertion_record and assertion_record.get("assertionStatus") != "absent":
                        diagnostic["assertions"] = assertion_record
                    if diagnostic: phases[name]["diagnostic"] = diagnostic
                    checkpoint()
                    continue
                failure_reason = reason if code != 0 else "diagnostic"
                cause = causal(harness, name, failure_reason); phases[name] = phase("FAIL", "probe-" + failure_reason, cause)
                if diagnostic:
                    diagnostic.pop("status", None)
                    if progress:
                        diagnostic["progress"] = progress
                    # An absent record adds nothing; a corrupt or valid one is evidence.
                    if assertion_record and assertion_record.get("assertionStatus") != "absent":
                        diagnostic["assertions"] = assertion_record
                    if diagnostic: phases[name]["diagnostic"] = diagnostic
                mark_dependents(phases, ("deterministic-contract", "live-tool") if name == "version-doctor" else ("live-tool",), name + "-failed", cause)
                any_failure = True; checkpoint(); break
            phases[name] = phase("PASS", "verified")
            checkpoint()
        if "live-tool" not in phases:
            if args.mode != "live": phases["live-tool"] = phase("NOT_REQUESTED", "deterministic-mode")
            elif not os.environ.get("NAN_API_KEY"):
                phases["live-tool"] = phase("BLOCKED", "credential-not-configured", causal(harness, "live-tool", "credential"))
            else:
                live_env = dict(probe_env); live_env["NAN_API_KEY"] = os.environ["NAN_API_KEY"]
                child_timeout = budget.child_timeout(args.timeout)
                if child_timeout <= 0:
                    phases["live-tool"] = phase("BLOCKED", "deadline-exhausted")
                    any_failure = True; reports.append({"harness": harness, "phases": phases}); checkpoint(); active["harness"] = None; active["phases"] = {}; active["phase"] = None; continue
                code, reason = run_bounded(common + ["-Stage", "live-tool", "-Model", args.model], cell, live_env, child_timeout)
                diagnostic = probe_diagnostic(cell, "live-tool")
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
        reports.append({"harness": harness, "phases": phases}); active["harness"] = None; active["phases"] = {}; active["phase"] = None; checkpoint()
    report = _report(args, reports, native_test, selected, setup, active)
    return report, any_failure or report["totals"]["failed"] > 0 or report["totals"]["blocked"] > 0

def write_outputs(report, output):
    protect_private(output); path = output / "report.json"
    text = json.dumps(report, indent=2, sort_keys=True) + "\n"
    tmp = output / ".report.json.tmp"
    tmp.write_text(text, encoding="utf-8"); protect_private(tmp); os.replace(tmp, path); protect_private(path)
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
    summary = output / "summary.md"; summary_tmp = output / ".summary.md.tmp"
    summary_tmp.write_text("\n".join(lines) + "\n", encoding="utf-8"); protect_private(summary_tmp)
    os.replace(summary_tmp, summary); protect_private(summary)

def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument("--mode", choices=("deterministic", "live", "native-diagnostic"), default="deterministic")
    parser.add_argument("--harnesses", default="all"); parser.add_argument("--model", default="qwen3.6")
    parser.add_argument("--source-sha", "--source", dest="source", default=""); parser.add_argument("--python-version", default=PYTHON_VERSION); parser.add_argument("--binary", type=Path, required=True); parser.add_argument("--canary", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=Path("windows-canary-report")); parser.add_argument("--timeout", type=int, default=900)
    parser.add_argument("--budget-seconds", type=int, default=6000); args = parser.parse_args(argv)
    if not SAFE_ID.fullmatch(args.model) or not re.fullmatch(r"^[0-9]+\.[0-9]+$", args.python_version) or args.timeout < 1 or args.timeout > 3600 or args.budget_seconds < 1 or (args.source and not SHA.fullmatch(args.source)): parser.error("bounded identity, Python version, timeout, and budget are required")
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
