#!/usr/bin/env python3
"""Prepare, run and publish daily checks of exact published nan-harness assets."""

import argparse
import datetime
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from daily_evidence import collect_release, pending, release_tags
from release_gate import _asset_entries, digest
from selection import CLI_HARNESSES, PLATFORM_ASSETS, PLATFORMS, supported_platforms


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("daily_cli_suite", ROOT / "canary/actions/cli-suite.py")
suite = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = suite
SPEC.loader.exec_module(suite)


def command(args, **kwargs):
    # Child failures are intentionally reported as closed facts, never raw output.
    return subprocess.run([str(a) for a in args], check=True, capture_output=True,
                          timeout=300, **kwargs).stdout


def gh(*args):
    return command(["gh", *args])


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, sort_keys=True, indent=2) + "\n")


def download(repository, tag, name, path):
    path.parent.mkdir(parents=True, exist_ok=True)
    gh("release", "download", tag, "--repo", repository, "--pattern", name,
       "--output", path, "--clobber")


def read_feed(repository, name, directory):
    assets = json.loads(gh("release", "view", "compatibility", "--repo", repository,
                          "--json", "assets"))["assets"]
    names = {asset["name"] for asset in assets}
    selected = name
    if name not in names:
        backups = sorted((a for a in assets if a["name"].startswith(name + ".backup.")),
                         key=lambda a: (a["createdAt"], a["name"]), reverse=True)
        if not backups:
            raise ValueError("compatibility feed and backup are unavailable")
        selected = backups[0]["name"]
    path = directory / name
    download(repository, "compatibility", selected, path)
    task = "validate-unified-compatibility-feed" if name == "compatibility-v3.json" else "validate-compatibility-feed"
    command(["cargo", "xtask", task, path], cwd=ROOT)
    return json.loads(path.read_bytes())


def release_identity(repository, tag):
    # Native gh works on Windows too; generic bash there may resolve to WSL.
    # Include all pages because duplicate drafts can shadow a tag-based download.
    pages = json.loads(gh("api", "--paginate", "--slurp", f"repos/{repository}/releases?per_page=100"))
    if sum(release["tag_name"] == tag for page in pages for release in page) != 1:
        raise ValueError("release tag must have exactly one matching release")
    release = json.loads(gh("api", f"repos/{repository}/releases/tags/{tag}"))
    if release["draft"] or release["prerelease"] or release["tag_name"] != tag:
        raise ValueError("daily checks require a public stable release")
    obj = json.loads(gh("api", f"repos/{repository}/git/ref/tags/{tag}"))["object"]
    for _ in range(10):
        if obj["type"] != "tag":
            break
        obj = json.loads(gh("api", f"repos/{repository}/git/tags/{obj['sha']}"))["object"]
    if obj["type"] != "commit" or not re.fullmatch(r"[0-9a-f]{40}", obj["sha"]):
        raise ValueError("release does not resolve to an immutable commit")
    return obj["sha"]


def release_assets(repository, tag, directory):
    commit = release_identity(repository, tag)
    for name in ("SHA256SUMS", *(n for pair in PLATFORM_ASSETS.values() for n in pair.values())):
        download(repository, tag, name, directory / name)
    gh("attestation", "verify", directory / "SHA256SUMS", "--repo", repository,
       "--signer-workflow", repository + "/.github/workflows/release.yml",
       "--source-ref", "refs/tags/" + tag, "--source-digest", commit, "--deny-self-hosted-runners")
    entries, _ = _asset_entries(directory)
    return {"tag": tag, "version": tag[1:], "commit": commit,
            "digests": {entry["name"]: entry["sha256"] for entry in entries}}


def frozen_versions(model):
    result = {}
    for system, platform in PLATFORMS.items():
        names = [h for h in CLI_HARNESSES if system in supported_platforms(h)]
        resolved, _ = suite.resolve_manifest(names, system, platform["architecture"], model)
        result[system] = {item.harness: item.as_dict() for item in resolved}
    return result


def select_cells(plan, release, versions, feed, force):
    for harness in CLI_HARNESSES:
        platforms = supported_platforms(harness)
        frozen = [versions[system].get(harness) for system in platforms]
        status = None
        if any(item is None for item in frozen) or len({(v["version"], v.get("ref", "")) for v in frozen if v}) != 1:
            status = "unresolved"
        elif not pending(feed, release["version"], harness, frozen[0]["version"], force):
            status = "current"
        if status:
            plan["results"].append({"tag": release["tag"], "harness": harness, "status": status})
            continue
        for item in frozen:
            system = item["system"]
            cell_id = f"{release['tag']}-{system}-{harness}"
            plan["cells"].append({"id": cell_id, "tag": release["tag"], "harness": harness,
                                  "system": system, "runner": PLATFORMS[system]["runner"],
                                  "version": item["version"], "frozen": item,
                                  "report": cell_id + ".json"})


def prepare(args):
    work = args.directory
    work.mkdir(parents=True, exist_ok=True)
    available_path = work / "available.json"
    download(args.repository, "available", "update-manifest.json", available_path)
    available = json.loads(available_path.read_bytes())
    recommended = json.loads(gh("api", f"repos/{args.repository}/releases/latest"))
    feed = read_feed(args.repository, "compatibility-v3.json", work)
    plan = {"repository": args.repository, "runId": args.run_id,
            "workflowCommit": args.workflow_commit, "model": "qwen3.6",
            "specSha256": digest(ROOT / "canary/actions/cell.py"),
            "startedAt": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "releases": [], "cells": [], "results": []}
    versions = frozen_versions(plan["model"])
    for tag in release_tags(available, recommended):
        try:
            release = release_assets(args.repository, tag, work / "assets" / tag)
        except (subprocess.SubprocessError, ValueError, KeyError, TypeError, OSError):
            plan["results"].append({"tag": tag, "harness": "all", "status": "assets-unavailable"})
            continue
        plan["releases"].append(release)
        select_cells(plan, release, versions, feed, args.force)
    write_json(work / "plan.json", plan)
    matrix = {"include": [{"id": c["id"], "runner": c["runner"]} for c in plan["cells"]]}
    with open(os.environ["GITHUB_OUTPUT"], "a") as output:
        output.write("matrix=" + json.dumps(matrix) + "\n")
        output.write("has_cells=" + str(bool(plan["cells"])).lower() + "\n")


def load_plan(args):
    plan = json.loads((args.directory / "plan.json").read_bytes())
    if (plan["repository"] != args.repository or plan["runId"] != args.run_id
            or plan["workflowCommit"] != args.workflow_commit
            or plan["specSha256"] != digest(ROOT / "canary/actions/cell.py")
            or command(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip() != args.workflow_commit):
        raise ValueError("daily plan does not belong to this workflow execution")
    return plan


def run_cell(args):
    plan = load_plan(args)
    cell = next(c for c in plan["cells"] if c["id"] == args.cell)
    release = next(r for r in plan["releases"] if r["tag"] == cell["tag"])
    assets = args.directory / "assets" / release["tag"]
    for name in PLATFORM_ASSETS[cell["system"]].values():
        if digest(assets / name) != release["digests"][name]:
            raise ValueError("daily cell asset changed in transit")
        (assets / name).chmod(0o755)
    work = args.directory / "cell"
    manifest = work / "versions.json"
    write_json(manifest, {"harnesses": [cell["frozen"]], "unresolved": []})
    pair = PLATFORM_ASSETS[cell["system"]]
    invocation = [sys.executable, str(ROOT / "canary/actions/cli-suite.py"),
                  "--harnesses", cell["harness"], "--mode", "live", "--trigger", "daily",
                  "--tag", release["tag"], "--model", plan["model"], "--source-kind", "release",
                  "--source-sha", release["commit"], "--nan-version", release["version"],
                  "--system", cell["system"], "--architecture", cell["frozen"]["architecture"],
                  "--manifest", str(manifest), "--binary", str(assets / pair["harness"]),
                  "--canary", str(assets / pair["canary"]), "--directory", str(work / "private"),
                  "--output", str(work / "reports"), "--run-id", plan["runId"]]
    if not os.environ.get("NAN_API_KEY"):
        raise ValueError("daily live credential is unavailable")
    completed = subprocess.run(invocation, check=False)
    source = work / "reports" / f"{cell['system']}-{cell['frozen']['architecture']}-{cell['harness']}.json"
    if source.is_file():
        args.reports.mkdir(parents=True, exist_ok=True)
        (args.reports / cell["report"]).write_bytes(source.read_bytes())
    return completed.returncode


def publish_release(args, release, updates):
    directory = args.directory / "updates" / release["tag"]
    directory.mkdir(parents=True, exist_ok=False)
    for update in updates:
        write_json(directory / (update["id"] + ".json"), update)
    invocation = ["bash", ROOT / "canary/host/publish-compatibility.sh",
                  "--trigger", "daily", "--nan-harness-version", release["version"],
                  "--release-tag", release["tag"], "--reports", args.reports,
                  "--output-dir", args.directory / "publication" / release["tag"],
                  "--state-dir", args.directory / "state", "--report-validator", args.validator,
                  "--repository", args.repository, "--verified-updates", directory]
    if args.publish:
        invocation.append("--publish-feed")
    # Publication includes bounded retries and Rust compilation on a cold runner.
    subprocess.run([str(a) for a in invocation], check=True, timeout=1200)


def aggregate(args):
    plan = load_plan(args)
    results = list(plan["results"])
    args.reports.mkdir(parents=True, exist_ok=True)
    for release in plan["releases"]:
        try:
            if release_identity(args.repository, release["tag"]) != release["commit"]:
                raise ValueError("release identity changed since selection")
            updates, statuses = collect_release(
                plan, release, args.reports,
                lambda path: command([args.validator, "validate-report", path]))
            if updates:
                publish_release(args, release, updates)
            for status in statuses:
                if status["status"] == "verified" and args.publish:
                    status["status"] = "published"
            results.extend(statuses)
        except (subprocess.SubprocessError, ValueError, KeyError, TypeError, OSError, StopIteration):
            results.append({"tag": release["tag"], "harness": "all", "status": "validation-or-publication-failed"})
    write_json(args.directory / "summary.json", results)
    with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as output:
        output.write("## Daily CLI compatibility\n\n| nanh | Harness | Result |\n| --- | --- | --- |\n")
        for row in results:
            output.write(f"| {row['tag']} | {row['harness']} | {row['status']} |\n")
        output.write("\nWindows Prime Agent and FX are unavailable and never count as passes.\n")
    return int(any(row["status"] not in ("current", "verified", "published") for row in results))


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stage", choices=("prepare", "cell", "aggregate"))
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--reports", type=Path)
    parser.add_argument("--validator", type=Path)
    parser.add_argument("--cell")
    parser.add_argument("--force", action="store_true")
    parser.add_argument("--publish", action="store_true")
    parser.add_argument("--repository", default=os.environ.get("GITHUB_REPOSITORY"))
    parser.add_argument("--workflow-commit", default=os.environ.get("GITHUB_SHA"))
    parser.add_argument("--run-id", default=os.environ.get("DAILY_RUN_ID"))
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repository or ""):
        parser.error("repository is required")
    if not re.fullmatch(r"[0-9a-f]{40}", args.workflow_commit or "") or not args.run_id:
        parser.error("immutable workflow identity is required")
    try:
        return {"prepare": prepare, "cell": run_cell, "aggregate": aggregate}[args.stage](args) or 0
    except (subprocess.SubprocessError, ValueError, KeyError, TypeError, OSError, StopIteration):
        print("Daily compatibility stage failed; no unverified evidence is accepted.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
