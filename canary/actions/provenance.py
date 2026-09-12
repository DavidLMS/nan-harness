"""Bind automated evidence to trusted main code and immutable release assets."""

import hashlib
import re
import subprocess

from state import StateError

COMMIT = re.compile(r"[0-9a-f]{40}\Z")
SHA256 = re.compile(r"[0-9a-f]{64}\Z")
AUTOMATED_WORKFLOW = ".github/workflows/harness-canary.yml"


def trusted_run(store, run_id):
    """Use API metadata, never report claims, to authorize automatic publication."""
    if type(run_id) is not int or run_id < 1:
        raise StateError("invalid source run")
    run = store.call(f"actions/runs/{run_id}")
    commit = run.get("head_sha", "")
    if (run.get("id") != run_id or not COMMIT.fullmatch(commit)
            or run.get("path") != AUTOMATED_WORKFLOW
            or run.get("head_branch") != "main"
            or run.get("head_repository", {}).get("full_name") != store.repository
            or run.get("event") not in ("schedule", "workflow_dispatch")
            or run.get("status") != "completed"
            or run.get("conclusion") not in ("success", "failure")):
        raise StateError("untrusted automatic evidence source")
    # A failed suite can publish closed negative observations, but a cancelled
    # run cannot establish that its cleanup and artifact validation completed.
    main = store.call("git/ref/heads/main")["object"]["sha"]
    if not COMMIT.fullmatch(main):
        raise StateError("invalid main identity")
    comparison = store.call(f"compare/{commit}...{main}")
    if (comparison.get("status") not in ("ahead", "identical")
            or comparison.get("merge_base_commit", {}).get("sha") != commit):
        raise StateError("source code is not on trusted main history")
    return commit


def specification_digest(repository, commit, suite):
    """Hash tracked executable inputs without changing or executing the checkout.

    Include shared runtime and installation code conservatively. Documentation
    edits do not invalidate evidence; changing any executable probe input does.
    Git's tree identifies bytes at the source run, not a mutable working tree.
    """
    if suite not in ("cli", "desktop") or not COMMIT.fullmatch(commit):
        raise StateError("invalid specification identity")
    result = subprocess.run(
        ["git", "-C", str(repository), "ls-tree", "-r", "-z", commit, "--",
         "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo", "crates",
         "canary", "scripts", "tests/conformance", ".github/scripts",
         ".github/workflows"],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30, check=False)
    if result.returncode or len(result.stdout) > 4_000_000:
        raise StateError("could not resolve trusted specification")
    records = []
    for record in result.stdout.split(b"\0"):
        if not record:
            continue
        metadata, path = record.split(b"\t", 1)
        if path.endswith((b".md", b".png", b".svg", b".jpg")):
            continue
        mode, kind, oid = metadata.split(b" ")
        if kind != b"blob" or mode not in (b"100644", b"100755"):
            raise StateError("probe inputs must be tracked regular files")
        records.append(path + b"\0" + mode + b"\0" + oid)
    if not records:
        raise StateError("empty specification")
    return hashlib.sha256(b"hosted-spec-v1\0" + suite.encode() + b"\0"
                          + b"\0".join(sorted(records))).hexdigest()


def bind_report(report, suite, expected):
    """Compare validated report identity with independently verified run inputs."""
    binary = report.get("nanHarness") or {}
    environment = report.get("environment", {}) if suite == "cli" else report
    system = environment.get("operatingSystem") if suite == "cli" else environment.get("platform")
    if (binary.get("version") != expected["version"]
            or binary.get("sha256") != expected["binarySha256"]
            or not SHA256.fullmatch(expected["binarySha256"])
            or system != expected["platform"]
            or environment.get("architecture") != expected["architecture"]):
        raise StateError("report does not describe the verified native release")
    attempted_live = (any(check["name"] == "live-tool" for check in report["checks"])
                      if suite == "cli" else
                      any(app["live"]["status"] != "skipped" for app in report["results"]))
    if attempted_live and report.get("model") != expected["model"]:
        raise StateError("report model does not match the selected run")
