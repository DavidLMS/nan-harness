#!/usr/bin/env python3
"""Dispatch one manual workflow and wait for its uniquely identified run."""

import argparse
import json
import subprocess
import sys
import time
import uuid

from publication import TAG
from state import StateError, Store


def dispatch(repository, kind, tag):
    Store(repository)
    if not TAG.fullmatch(tag):
        raise StateError("invalid release tag")
    identity = uuid.uuid4().hex
    workflow = "cli-release-gate.yml" if kind == "gate" else "compatibility-approve.yml"
    fields = ["-f", "tag=" + tag, "-f", "coverage=release"] if kind == "gate" else [
        "-f", "operation=recommend", "-f", "source=" + tag]
    subprocess.run(["gh", "workflow", "run", workflow, "--repo", repository, "--ref", "main",
                    "-f", "request_id=" + identity, *fields], check=True)
    for _ in range(60):
        response = subprocess.run(["gh", "run", "list", "--repo", repository,
                                   "--workflow", workflow, "--event", "workflow_dispatch",
                                   "--limit", "100", "--json", "databaseId,displayTitle"],
                                  check=True, capture_output=True)
        runs = [run for run in json.loads(response.stdout) if run["displayTitle"].endswith(identity)]
        if len(runs) == 1:
            return subprocess.run(["gh", "run", "watch", str(runs[0]["databaseId"]),
                                   "--repo", repository, "--exit-status"], check=False).returncode
        if runs:
            raise StateError("workflow correlation was not unique")
        time.sleep(2)
    raise StateError("workflow was dispatched but did not become visible")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kind", choices=("gate", "recommend"))
    parser.add_argument("--repository", required=True)
    parser.add_argument("--tag", required=True)
    args = parser.parse_args()
    try:
        return dispatch(args.repository, args.kind, args.tag)
    except (StateError, subprocess.SubprocessError, ValueError, KeyError):
        print("Hosted workflow dispatch or wait failed; inspect Actions before retrying.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
