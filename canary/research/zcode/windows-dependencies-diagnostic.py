"""Temporary credential-free release installation diagnosis; closed output only."""
import importlib.util
import json
import os
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

def diagnose(arguments, cwd, environment=None, stage="probe"):
    with tempfile.TemporaryFile() as output:
        result = subprocess.run([str(a) for a in arguments], cwd=cwd, env=environment,
                                stdout=output, stderr=output, timeout=1200)
        output.seek(0)
        raw = output.read(8 * 1024 * 1024).decode("utf-8", "replace")
        if result.returncode:
            print(json.dumps({"stage": stage, "exitCode": result.returncode,
                              "codes": [code for code in CODES if code in raw],
                              "symlinkMentioned": "symlink" in raw.lower(),
                              "junctionMentioned": "junction" in raw.lower(),
                              "outOfMemory": "heap out of memory" in raw.lower(),
                              "tooLong": "too long" in raw.lower()}), flush=True)
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
