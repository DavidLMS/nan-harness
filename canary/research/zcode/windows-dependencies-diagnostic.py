"""Temporary credential-free release installation diagnosis; closed output only."""
import importlib.util
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "canary/actions"))
from cell import cell_environment
spec = importlib.util.spec_from_file_location("source", ROOT / "canary/guest/zcode-source.py")
source = importlib.util.module_from_spec(spec)
spec.loader.exec_module(source)
CODES = ("EPERM", "ENOENT", "ENOSPC", "EACCES", "ENAMETOOLONG", "EINVAL", "ECONNRESET",
         "ETIMEDOUT", "ERR_PNPM_OUTDATED_LOCKFILE", "ERR_PNPM_UNSUPPORTED_PLATFORM",
         "ERR_PNPM_TARBALL_INTEGRITY", "ERR_PNPM_TARBALL_EXTRACT", "ERR_PNPM_FETCH_403",
         "ERR_PNPM_FETCH_404", "ERR_PNPM_FETCH_429", "ERR_PNPM_FETCH_500",
         "ERR_PNPM_FETCH_502", "ERR_PNPM_FETCH_503", "ERR_PNPM_LINKING_FAILED",
         "ERR_PNPM_UNEXPECTED_STORE", "ERR_PNPM_UNSUPPORTED_ENGINE")

def rename_details(raw, cwd):
    text = re.sub(r"\x1b\[[0-9;]*m", "", raw)
    pairs = re.findall(r"rename ['\"]([^'\"]+)['\"]\s*->\s*['\"]([^'\"]+)['\"]", text)
    known = {"node_modules", ".pnpm", ".ignored", ".bin", "pnpm", "node", ".tools", ".nanh-tools",
             "package.json", "pnpm-lock.yaml", "lock.yaml", ".modules.yaml", "store", "v10", "v3",
             "tmp", "temp", "cache", ".cache", "bin", "dist", "npm", "home", "zcode-source",
             "pnpm.cjs", "node.exe", "pnpm.exe", "pnpm.cmd", "pnpm.ps1", "pnpx.cmd", "pnpx.ps1"}
    def shape(value):
        parts = value.replace("\\", "/").split("/")
        return [part.lower() if part.lower() in known else
                "temporary-name" if "tmp" in part.lower() else "version" if re.fullmatch(r"v?\d+\.\d+\.\d+", part)
                else "other" for part in parts[-8:]]
    return [{"sourceShape": shape(a), "destinationShape": shape(b),
             "sourceExists": Path(a).exists(), "destinationExists": Path(b).exists(),
             "sameParent": Path(a).parent == Path(b).parent,
             "parentExists": Path(a).parent.exists()} for a,b in pairs[:4]]

def diagnose(arguments, cwd, environment=None, stage="probe"):
    if stage == "dependencies":
        environment = dict(environment, PNPM_MAX_WORKERS="1")
    with tempfile.TemporaryFile() as output:
        result = subprocess.run([str(a) for a in arguments], cwd=cwd, env=environment,
                                stdout=output, stderr=output, timeout=1200)
        output.seek(0)
        raw = output.read(8 * 1024 * 1024).decode("utf-8", "replace")
        if result.returncode:
            print(json.dumps({"stage": stage, "exitCode": result.returncode,
                              "codes": [code for code in CODES if code in raw],
                              "renameDetails": rename_details(raw, cwd),
                              "symlinkMentioned": "symlink" in raw.lower(),
                              "junctionMentioned": "junction" in raw.lower(),
                              "outOfMemory": "heap out of memory" in raw.lower(),
                              "tooLong": "too long" in raw.lower(),
                              "missingOperations": sorted(set(re.findall(r"ENOENT[^\n]{0,160}?\b(open|stat|lstat|scandir|rename|unlink|readlink|spawn|mkdir|copyfile)\b", raw))),
                              "missingPathKinds": [name for name, pattern in {
                                  "pnpm-store": r"(?:pnpm|[.]pnpm)[/\\].*store|[/\\]store[/\\]",
                                  "virtual-store": r"node_modules[/\\][.]pnpm",
                                  "manifest": r"package[.]json",
                                  "lockfile": r"pnpm-lock[.]yaml",
                                  "patch": r"[.]patch",
                                  "git": r"[/\\][.]git",
                                  "node-executable": r"node[.]exe",
                                  "pnpm-executable": r"pnpm[.]cjs",
                                  "temporary": r"[/\\]tmp[/\\]|[/\\]temp[/\\]",
                              }.items() if any(re.search(pattern, line, re.I) for line in raw.splitlines() if "ENOENT" in line)],
                              "workspaceRootMentioned": any("workspace" in line.lower() for line in raw.splitlines() if "ENOENT" in line)}), flush=True)
            raise source.SourceFailure(stage)
        print(json.dumps({"stage": stage, "status": "passed"}), flush=True)
        return raw.strip()

source.run = diagnose
cell = Path(os.environ["RUNNER_TEMP"]) / "cli-cell/zcode"
cell.mkdir(parents=True)
environment = cell_environment(cell)
for key in list(environment):
    if any(part in key.upper() for part in ("TOKEN", "SECRET", "API_KEY")):
        environment.pop(key)
os.environ.clear()
os.environ.update(environment)
source.install("0.16.9", "aac4755666d09fdcd70272fcf063c077a639015f", cell)
