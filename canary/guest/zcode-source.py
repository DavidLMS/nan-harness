"""Build an immutable official ZCode CLI in a disposable compatibility cell."""

import argparse
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
CLI = "apps/zcode-cli/packages/cli/dist/zcode.cjs"
PIN = "29628c9acdb81b703bbd4080c207a0e7ce5e276e"
SEMVER = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+\Z")
COMMIT = re.compile(r"[0-9a-f]{40}\Z")


def run(arguments, cwd, environment=None):
    return subprocess.run([str(a) for a in arguments], cwd=cwd, env=environment,
                          check=True, timeout=1200, capture_output=True, text=True).stdout.strip()


def bind(source):
    directory = source / "apps/zcode-cli/packages/cli/src"
    replacements = {
        "main.ts": [("void main();", 'if (process.argv[2] === "--nanh-source-info") {\n'
                     '  console.log("nanh-zcode-config-v1");\n} else {\n  void main();\n}'),
                    ("const exitCode = await run(context, {", "const exitCode = await run(context, {\n"
                     "      projectConfigPath: process.env.NAN_HARNESS_ZCODE_PROJECT_CONFIG_FILE,")],
        "prompt-command.ts": [("      env: appEnv,", "      env: appEnv,\n"
                               "      projectConfigPath: deps.projectConfigPath,")],
    }
    for name, changes in replacements.items():
        path = directory / name
        content = path.read_text()
        for old, new in changes:
            if content.count(old) != 1:
                raise ValueError("ZCode source configuration contract changed")
            content = content.replace(old, new)
        path.write_text(content)


def npm_command():
    if os.name != "nt":
        return ["npm"]
    shim = shutil.which("npm.cmd")
    if not shim:
        raise ValueError("npm is unavailable")
    return ["node", str(Path(shim).parent / "node_modules/npm/bin/npm-cli.js")]


def install(version, ref, destination):
    if not SEMVER.fullmatch(version) or not COMMIT.fullmatch(ref):
        raise ValueError("ZCode requires a frozen agent version and source commit")
    source = destination / "zcode-source"
    # Cells own this directory; never reuse or replace a caller's source checkout.
    source.mkdir(parents=True, exist_ok=False)
    run(["git", "init", "-q"], source)
    run(["git", "remote", "add", "origin", "https://github.com/zai-org/ZCode.git"], source)
    run(["git", "fetch", "--depth", "1", "origin", ref], source)
    run(["git", "checkout", "--detach", "FETCH_HEAD"], source)
    if run(["git", "rev-parse", "HEAD"], source) != ref:
        raise ValueError("ZCode source identity changed")
    project = json.loads((source / "apps/zcode-cli/package.json").read_bytes())
    node_version = project["engines"]["node"]
    manager = project["packageManager"]
    if (project["version"] != version or not SEMVER.fullmatch(node_version)
            or not re.fullmatch(r"pnpm@[0-9]+\.[0-9]+\.[0-9]+", manager)):
        raise ValueError("ZCode source runtime or version contract changed")
    tools = source / ".nanh-tools"
    npm = npm_command()
    # Node's npm package downloads its platform runtime in its install script.
    # All tools and caches are cell-owned; no global runtime is changed.
    run([*npm, "install", "--prefix", tools, "--no-audit", "--no-fund", "node@" + node_version], source)
    run([*npm, "install", "--prefix", tools, "--ignore-scripts", "--no-audit", "--no-fund", manager], source)
    node = tools / "node_modules/node/bin" / ("node.exe" if os.name == "nt" else "node")
    pnpm = tools / "node_modules/pnpm/bin/pnpm.cjs"
    environment = dict(os.environ, PATH=str(node.parent) + os.pathsep + os.environ.get("PATH", ""))
    if run([node, "--version"], source, environment) != "v" + node_version:
        raise ValueError("ZCode source runtime could not be verified")
    run([node, pnpm, "--filter", "@zcode/cli...", "install", "--ignore-scripts", "--frozen-lockfile"], source, environment)
    bind(source)
    run([node, pnpm, "--filter", "@zcode/cli...", "build"], source, environment)
    for argument, expected in (("version", version), ("--nanh-source-info", "nanh-zcode-config-v1")):
        if expected not in run([node, source / CLI, argument], source, environment):
            raise ValueError("ZCode built command failed verification")
    bin_directory = Path(os.environ.get("HOME", "")) / ".local/bin"
    bin_directory.mkdir(parents=True, exist_ok=True)
    launcher = bin_directory / ("zcode.cmd" if os.name == "nt" else "zcode")
    if launcher.exists():
        raise ValueError("ZCode installer refuses to replace an existing command")
    command = (f'@echo off\r\n"{node}" "{source / CLI}" %*\r\n' if os.name == "nt" else
               f'#!/bin/sh\nexec {shlex.quote(str(node))} {shlex.quote(str(source / CLI))} "$@"\n')
    launcher.write_text(command)
    launcher.chmod(0o755)
    if os.name == "nt":
        run([sys.executable, "-m", "pip", "install", "--only-binary=:all:", "--target",
             source / ".python-tools", "pywinpty==3.0.5"], source)
    (destination / "zcode-source.json").write_text(json.dumps({"ref": ref, "version": version,
                                                            "source": str(source), "node": str(node)}))


def check(destination, binary):
    receipt = json.loads((destination / "zcode-source.json").read_bytes())
    source, node = Path(receipt["source"]), Path(receipt["node"])
    probes = ROOT / "canary/research/zcode"
    environment = dict(os.environ, ZCODE_SOURCE_ROOT=str(source),
                       PATH=str(node.parent) + os.pathsep + os.environ.get("PATH", ""))
    environment.pop("NAN_API_KEY", None)
    environment["PYTHONPATH"] = str(source / ".python-tools")
    output = destination / "zcode-contracts"
    output.mkdir(exist_ok=False)
    run([node, probes / "build-probes.mjs", output], ROOT, environment)
    for script in ("config-probe.mjs", "protocol-probe.mjs"):
        run([node, output / script], ROOT, environment)
    run([sys.executable, probes / "native_probe.py", "--source", source, "--node", node], ROOT, environment)
    for case in ("sessions", "tui"):
        run([sys.executable, probes / "runtime_probe.py", "--source", source, "--node", node,
             "--case", case], ROOT, environment)
    run([sys.executable, probes / "integration_probe.py", "--binary", binary, "--source", source], ROOT, environment)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stage", choices=("install", "check"))
    parser.add_argument("--version")
    parser.add_argument("--ref", default=PIN)
    parser.add_argument("--directory", type=Path, default=Path.cwd())
    parser.add_argument("--binary", type=Path)
    args = parser.parse_args()
    os.environ.pop("NAN_API_KEY", None)
    try:
        if args.stage == "install" and args.version == "latest":
            import importlib.util
            sys.path.insert(0, str(ROOT / "canary/actions"))
            spec = importlib.util.spec_from_file_location("zcode_latest_suite", ROOT / "canary/actions/cli-suite.py")
            suite = importlib.util.module_from_spec(spec)
            sys.modules[spec.name] = suite
            spec.loader.exec_module(suite)
            frozen = suite.resolve_frozen_versions(["zcode"], "linux", "aarch64", "qwen3.6")[0]
            args.version, args.ref = frozen.version, frozen.ref
        if args.stage == "install":
            install(args.version, args.ref, args.directory.resolve())
        else:
            check(args.directory.resolve(), args.binary.resolve())
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        print("ZCode source compatibility stage failed; private child output is withheld.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
