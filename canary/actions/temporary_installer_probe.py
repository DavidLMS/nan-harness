"""Temporary credential-free installer comparison; removed before integration."""
import importlib.util
import json
import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import cell

spec = importlib.util.spec_from_file_location("suite", Path(__file__).with_name("cli-suite.py"))
suite = importlib.util.module_from_spec(spec)
sys.modules["suite"] = suite
spec.loader.exec_module(suite)
resolved, unresolved = suite.resolve_manifest(["hermes"], "windows", "x86_64", "qwen3.6")
os.environ.pop("GITHUB_TOKEN", None)
os.environ.pop("NAN_API_KEY", None)
if unresolved or len(resolved) != 1:
    raise RuntimeError("official metadata unavailable")
root = Path(os.environ["RUNNER_TEMP"]) / "cli-cell" / "hermes"
cell.ensure_private_directory(root)
env = cell.cell_environment(root)
item = resolved[0]
code = cell.private_command(cell.installer_command("hermes", item.version, item.ref), root,
                            allow_failure=True, environment=env)
result = {"passed": code == 0, "code": "passed" if code == 0 else
          cell.windows_install_failure(root / "installer-result.json", "unknown")}
result["inherited"] = {name: name in env for name in (
    "MSYSTEM", "MSYSTEM_PREFIX", "MSYS", "MSYS2_PATH_TYPE", "MSYS2_ARG_CONV_EXCL",
    "SHELL", "GIT_EXEC_PATH", "GIT_CONFIG_GLOBAL", "GIT_CONFIG_COUNT", "GIT_ASKPASS")}
Path("installer-diagnostic.json").write_text(json.dumps(result))
