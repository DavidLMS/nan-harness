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

def causal(harness, phase_name, reason):
    token = hashlib.sha256(f"{harness}:{phase_name}:{reason}".encode()).hexdigest()[:12]
    return f"WIN-{phase_name.upper().replace('-', '_')}-{token}"

def phase(status, reason="", cause=None):
    value = {"status": status}
    if reason: value["reason"] = reason
    if cause: value["causalId"] = cause
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

def installer_reason(cell):
    """Consume only the installer's closed marker; never retain exception text."""
    marker = cell / "installer-result.json"
    try:
        value = json.loads(marker.read_text(encoding="utf-8"))
        allowed = {"passed", "installer-failed", "official-asset-missing",
                   "official-metadata-probe-failed", "capability-not-implemented",
                   "invalid-frozen-ref", "invalid-version"}
        if (set(value) != {"schemaVersion", "status", "reason"} or value["schemaVersion"] != 1
                or value["status"] not in {"passed", "failed"} or value["reason"] not in allowed):
            return "installer-failed"
        return value["reason"]
    except (OSError, ValueError, TypeError, KeyError):
        return "installer-failed"
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
            reason = installer_reason(cell) if (cell / "installer-result.json").exists() else reason
            if not cleanup_installer_artifacts(cell):
                reason = "private-cleanup-failed"
            cause = causal(harness, "install", reason); phases["install"] = phase("FAIL", "installer-" + reason, cause)
            mark_dependents(phases, PHASES[3:], "install-failed", cause); any_failure = True
            reports.append({"harness": harness, "phases": phases}); continue
        installer_reason(cell)
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
        for name, stage in (("version-doctor", "version-doctor"), ("deterministic-contract", "deterministic-contract")):
            code, reason = run_bounded(common + ["-Stage", stage], cell, env, args.timeout)
            if code != 0:
                cause = causal(harness, name, reason); phases[name] = phase("FAIL", "probe-" + reason, cause)
                mark_dependents(phases, ("deterministic-contract", "live-tool") if name == "version-doctor" else ("live-tool",), name + "-failed", cause)
                any_failure = True; break
            phases[name] = phase("PASS", "verified")
        if "live-tool" not in phases:
            if args.mode != "live": phases["live-tool"] = phase("NOT_REQUESTED", "deterministic-mode")
            elif not os.environ.get("NAN_API_KEY"):
                phases["live-tool"] = phase("BLOCKED", "credential-not-configured", causal(harness, "live-tool", "credential"))
            else:
                live_env = dict(env); live_env["NAN_API_KEY"] = os.environ["NAN_API_KEY"]
                code, reason = run_bounded(common + ["-Stage", "live-tool", "-Model", args.model], cell, live_env, args.timeout)
                phases["live-tool"] = phase("PASS", "verified") if code == 0 else phase("FAIL", "probe-" + reason, causal(harness, "live-tool", reason))
                any_failure |= code != 0
        reports.append({"harness": harness, "phases": phases})
    for report in reports:
        required = PHASES if args.mode == "live" else PHASES[:-1]
        statuses = [report["phases"][name]["status"] for name in required]
        report["outcome"] = "passed" if all(status == "PASS" for status in statuses) else ("failed" if "FAIL" in statuses else "blocked")
    phase_totals = {status: sum(item["phases"].get(name, {}).get("status") == status for item in reports for name in PHASES) for status in ("PASS", "FAIL", "BLOCKED", "NOT_REQUESTED")}
    causes = {}
    for item in reports:
        for value in item["phases"].values():
            if value.get("causalId"): causes[value["causalId"]] = causes.get(value["causalId"], 0) + 1
    report = {"schemaVersion": 1, "platform": {"os": "windows", "architecture": platform.machine().lower()}, "mode": args.mode,
              "model": args.model, "sourceSha": args.source, "harnesses": reports,
              "setup": {key.removeprefix("NAN_DIAGNOSTIC_SETUP_").lower(): os.environ.get(key, "")
                        for key in ("NAN_DIAGNOSTIC_SETUP_CHECKOUT", "NAN_DIAGNOSTIC_SETUP_NODE",
                                    "NAN_DIAGNOSTIC_SETUP_PYTHON", "NAN_DIAGNOSTIC_SETUP_RUST",
                                    "NAN_DIAGNOSTIC_SETUP_SOURCE", "NAN_DIAGNOSTIC_SETUP_BUILD")},
              "totals": {"selected": len(reports), "passed": sum(x["outcome"] == "passed" for x in reports),
                         "failed": sum(x["outcome"] == "failed" for x in reports), "blocked": sum(x["outcome"] == "blocked" for x in reports), "phases": phase_totals},
              "groupedCauses": causes}
    return report, any_failure or report["totals"]["failed"] > 0 or (args.mode == "live" and report["totals"]["blocked"] > 0)

def write_outputs(report, output):
    protect_private(output); path = output / "report.json"
    path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"); protect_private(path)
    lines = ["# Native Windows CLI diagnostic", "", f"Mode: `{report['mode']}`  Model: `{report['model']}`", "", "| Harness | Outcome |", "| --- | --- |"]
    lines.extend(f"| {item['harness']} | {item['outcome']} |" for item in report["harnesses"])
    lines += ["", "Totals: " + json.dumps(report["totals"], sort_keys=True), "", "## Grouped causes"]
    lines.extend(f"- `{cause}`: {count} phase(s)" for cause, count in sorted(report["groupedCauses"].items()))
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
