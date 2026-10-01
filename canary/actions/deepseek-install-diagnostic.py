#!/usr/bin/env python3
"""Compare the original and corrected DeepSeek installs without publishing raw logs."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from cell import cell_environment, ensure_private_directory, private_command


VERSION = "0.1.5-rc.2"
MISSING = "@deepseek-ai/dsh-client-ui-sidebar-documentpreview@^0.1.5-rc.3"
ALLOW_SCRIPTS = "@deepseek-ai/dsh-subprocess-local,koffi,node-pty,@google/genai,protobufjs"
REPOSITORY = Path(__file__).resolve().parents[2]


def inspect_failure(log, status):
    log.seek(0)
    text = log.read(1024 * 1024).decode("utf-8", "replace")
    return {
        "exitCode": status,
        "missingRc3Dependency": "npm error code ETARGET" in text and MISSING in text,
    }


def run(root):
    node = Path(shutil.which("node")).resolve()
    npm = (node.parent / "node_modules/npm/bin/npm-cli.js" if os.name == "nt"
           else node.parent.parent / "lib/node_modules/npm/bin/npm-cli.js")
    if not npm.is_file():
        raise RuntimeError("npm runtime unavailable")
    if subprocess.check_output([node, "-p", "process.versions.node"], text=True).strip() != "24.20.0":
        raise RuntimeError("unexpected Node runtime")
    report = {"node": "24.20.0", "version": VERSION}
    for variant in ("original", "corrected"):
        directory = root / variant
        ensure_private_directory(directory)
        env = cell_environment(directory)
        # The diagnostic workflow has no credentials. Also discard accidental inherited tokens.
        for key in ("NAN_API_KEY", "GITHUB_TOKEN", "GH_TOKEN", "NODE_AUTH_TOKEN", "NPM_TOKEN"):
            env.pop(key, None)
        if variant == "original":
            command = [str(node), str(npm), "install", "--global"]
            command += (["--no-fund", "--no-audit"] if os.name == "nt"
                        else ["--allow-scripts=" + ALLOW_SCRIPTS])
            command += ["@deepseek-ai/dsh@" + VERSION]
        elif os.name == "nt":
            command = ["pwsh", "-NoLogo", "-NoProfile", "-File",
                       str(REPOSITORY / "canary/guest/install-harness.ps1"),
                       "-Harness", "deepseek-harness", "-Version", VERSION]
        else:
            command = ["bash", str(REPOSITORY / "canary/guest/install-harness.sh"),
                       "deepseek-harness", VERSION]
        observations = []
        status = private_command(command, directory, environment=env, allow_failure=True,
                                 diagnostic_callback=lambda log, code: observations.append(inspect_failure(log, code)))
        report[variant] = observations[0]
        if variant == "corrected" and status == 0:
            prefix = Path(env["NPM_CONFIG_PREFIX"])
            modules = prefix / ("node_modules" if os.name == "nt" else "lib/node_modules")
            cli = modules / "@deepseek-ai/dsh/lib/bin.js"
            versions = []

            def inspect_version(log, code):
                log.seek(0)
                versions.append(code == 0 and log.read(4096).decode("utf-8", "replace").strip() == VERSION)

            private_command([str(node), str(cli), "--version"], directory, environment=env,
                            allow_failure=True, diagnostic_callback=inspect_version)
            report[variant]["versionVerified"] = versions == [True]
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    with tempfile.TemporaryDirectory(prefix="deepseek-install-") as temporary:
        root = Path(temporary)
        ensure_private_directory(root, reusable=True)
        report = run(root)
    # This is a closed projection; no raw npm text, paths, arguments or environment escapes.
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report))
    return 0 if report["corrected"].get("versionVerified") else 1


if __name__ == "__main__":
    raise SystemExit(main())
