#!/usr/bin/env python3
"""Run the selected CLI harnesses sequentially in isolated native cells."""

import argparse
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from selection import CLI_HARNESSES, resolve_model


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--harnesses", required=True)
    parser.add_argument("--mode", choices=("deterministic", "live"), default="deterministic")
    parser.add_argument("--trigger", choices=("release", "weekly", "daily", "manual"), required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--model", default="")
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--canary", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--run-id", required=True)
    args = parser.parse_args()
    try:
        model = resolve_model(args.model)
        harnesses = [item.strip() for item in args.harnesses.split(",")]
        if not harnesses or len(harnesses) != len(set(harnesses)) or any(item not in CLI_HARNESSES for item in harnesses):
            raise ValueError("harnesses must be distinct known CLI identifiers")
    except ValueError as error:
        parser.error(str(error))

    args.directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    args.output.mkdir(mode=0o700, parents=True, exist_ok=True)
    failures = []
    base_env = os.environ.copy()
    deterministic_env = dict(base_env)
    deterministic_env.pop("NAN_API_KEY", None)
    for harness in harnesses:
        cell_directory = args.directory / harness
        report = args.output / f"{harness}.json"
        command = [sys.executable, str(Path(__file__).resolve().parent / "cell.py"), "install",
                   "--harness", harness, "--trigger", args.trigger, "--tag", args.tag,
                   "--model", model, "--binary", str(args.binary), "--canary", str(args.canary),
                   "--directory", str(cell_directory), "--output", str(report), "--run-id", args.run_id]
        stages = ["install", "conformance"]
        if args.mode == "live":
            stages.append("live")
        stages.append("report")
        for stage in stages:
            command[command.index("install")] = stage
            stage_env = base_env if stage == "live" else deterministic_env
            completed = subprocess.run(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                       env=stage_env, check=False)
            if completed.returncode:
                failures.append(harness)
                break
    if failures:
        print("One or more hosted CLI cells failed; sanitized reports were retained.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
