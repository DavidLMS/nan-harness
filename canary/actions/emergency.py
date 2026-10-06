#!/usr/bin/env python3
"""Fail closed unless all hosted publication entrypoints are disabled and idle."""

import argparse
import json
import os
from pathlib import Path
import sys

from state import Store, StateError
from publication import remote_commit, restore_receipt, validate_reports

WRITERS = ("release.yml", "cli-release-gate.yml", "compatibility-approve.yml",
           "hosted-evidence-ingest.yml", "compatibility-publisher.yml")


def assert_idle(store):
    for name in WRITERS:
        workflow = store.call(f"actions/workflows/{name}")
        if workflow["state"] != "disabled_manually":
            raise StateError("hosted publication must be disabled explicitly before emergency mode")
        for status in ("in_progress", "queued", "waiting", "pending", "requested"):
            runs = store.call(f"actions/workflows/{name}/runs?status={status}&per_page=1")
            if runs["total_count"]:
                raise StateError("hosted publication still has active work")


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--tag")
    parser.add_argument("--state-dir", type=Path)
    args = parser.parse_args()
    try:
        store = Store(args.repository)
        assert_idle(store)
        if args.tag:
            if args.state_dir is None:
                raise StateError("emergency recovery requires a private state directory")
            commit = remote_commit(store, args.tag)
            output = args.state_dir / "runs" / ("recovered-" + args.tag)
            receipts = args.state_dir / "receipts" / args.repository.replace("/", "__")
            receipt = restore_receipt(store, args.tag, receipts, output)
            if receipt:
                if receipt["tagCommit"] != commit:
                    raise StateError("release changed after durable receipt creation")
                if receipt["phases"]["suitePassed"]:
                    validate_reports(receipt["reports"], args.tag[1:])
                    reports = output / "reports"
                    reports.mkdir(parents=True, mode=0o700, exist_ok=True)
                    for report in receipt["reports"]:
                        name = report["environment"]["operatingSystem"] + "-" + report["harness"]["id"] + ".json"
                        (reports / name).write_text(json.dumps(report))
    except (StateError, KeyError, ValueError, OSError):
        print("Emergency publication refused: disable and drain hosted writers first.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
