#!/usr/bin/env python3
"""Bounded, non-publishing reproduction of the 2026-09-26 OMP usage incident."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib

import daily_compatibility as daily
from release_gate import digest
from selection import PLATFORM_ASSETS

ROOT = Path(__file__).resolve().parents[2]
REPOSITORY = "DavidLMS/nan-harness"
RELEASE = "v0.1.11"
RELEASE_COMMIT = "cb629096eabc6685fa09bdd2ae496979aa47fd21"
RELEASE_DIGEST = "f8a3c3d324cdf7bd697d034aeb3469b5213bfc4502c9c7e8dbfe90b3b9869cdd"
VERSIONS = ("18.3.1", "18.3.2")


class SetupFailure(ValueError):
    def __init__(self, reason):
        self.reason = reason
        super().__init__("comparison setup failed")


def binaries(kind, directory, source_sha):
    pair = PLATFORM_ASSETS["windows"]
    if kind == "release":
        assets = directory / "assets"
        release = daily.release_assets(REPOSITORY, RELEASE, assets)
        if (release["commit"] != RELEASE_COMMIT
                or release["digests"][pair["harness"]] != RELEASE_DIGEST):
            raise ValueError("incident release identity mismatch")
        return assets / pair["harness"], assets / pair["canary"], RELEASE_COMMIT, RELEASE[1:]
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    return (ROOT / "target/release/nan-harness.exe",
            ROOT / "target/release/nan-harness-canary.exe", source_sha, version)


def run(args):
    # Asset tools and installers do not inherit the provider credential.
    key = os.environ.pop("NAN_API_KEY", "")
    if not key:
        raise SetupFailure("credential-missing")
    source_sha = daily.command(["git", "rev-parse", "HEAD"]).decode().strip()
    if (not re.fullmatch(r"[0-9a-f]{40}", source_sha)
            or source_sha != os.environ.get("GITHUB_SHA")):
        raise SetupFailure("checkout-mismatch")
    args.directory.mkdir(parents=True, exist_ok=False)
    try:
        binary, canary, commit, version = binaries(args.kind, args.directory, source_sha)
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        raise SetupFailure("binary-preparation") from error
    source, package = daily.suite._source("omp", "windows")
    frozen = daily.suite.FrozenHarness("omp", args.omp_version, "windows", "x86_64",
                                      source, package, "qwen3.6", "")
    manifest = args.directory / "versions.json"
    daily.write_json(manifest, {"harnesses": [frozen.as_dict()], "unresolved": []})
    args.reports.mkdir(parents=True, exist_ok=False)
    daily.write_json(args.reports / "provenance.json", {
        "workflowSource": source_sha, "binarySource": commit, "binaryKind": args.kind,
        "nanVersion": version, "ompVersion": args.omp_version, "model": "qwen3.6",
        "binarySha256": digest(binary), "canarySha256": digest(canary), "attempts": 1,
    })
    invocation = [sys.executable, str(ROOT / "canary/actions/cli-suite.py"),
                  "--harnesses", "omp", "--mode", "live", "--trigger", "manual",
                  "--tag", "v" + version, "--model", "qwen3.6",
                  "--source-kind", "release" if args.kind == "release" else "branch",
                  "--source-sha", commit, "--nan-version", version,
                  "--system", "windows", "--architecture", "x86_64",
                  "--manifest", str(manifest), "--binary", str(binary), "--canary", str(canary),
                  "--directory", str(args.directory / "private"), "--output", str(args.reports),
                  "--run-id", os.environ["GITHUB_RUN_ID"] + "-" + os.environ["GITHUB_RUN_ATTEMPT"]]
    env = {k: v for k, v in os.environ.items() if k not in ("GH_TOKEN", "GITHUB_TOKEN")}
    env["NAN_API_KEY"] = key  # cli-suite removes it from every stage except live.
    # cell.py owns process deadlines and cleanup; do not time out its parent and
    # leave a live child unaccounted for. The workflow supplies the outer bound.
    return subprocess.run(invocation, env=env, check=False).returncode


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kind", choices=("release", "candidate"), required=True)
    parser.add_argument("--omp-version", choices=VERSIONS, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--reports", type=Path, required=True)
    args = parser.parse_args()
    args.directory = args.directory.resolve()
    args.reports = args.reports.resolve()
    if os.name != "nt":
        parser.error("this comparison requires native Windows")
    try:
        return run(args)
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        # Only locally authored categories are projected, never exception text.
        reason = error.reason if isinstance(error, SetupFailure) else "setup-unclassified"
        print("OMP comparison setup failed: " + reason + ". No private output was published.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
