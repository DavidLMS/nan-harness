#!/usr/bin/env python3
"""Approve, durably enqueue, and drain publication work from trusted Actions jobs."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

from state import Store, StateError, canonical, receipt_identity
from selection import select_suite, resolve_model

ROOT = Path(__file__).resolve().parents[2]
TAG = re.compile(r"v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?\Z")
PHASES = ("assetsVerified", "suitePassed", "compatibilityFeedPublished",
          "releasePublished", "availableFeedPublished")
HARNESSES = ("claude-code", "codex", "opencode", "hermes", "pi", "omp", "prime-agent",
             "deepseek-harness", "openclaw", "cline", "qwen-code", "kimi-code", "aider", "goose", "fx")
BINARIES = {"linux": "nan-harness-aarch64-unknown-linux-musl", "macos": "nan-harness-aarch64-apple-darwin",
            "windows": "nan-harness-x86_64-pc-windows-msvc.exe"}


def command(args, env=None):
    result = subprocess.run([str(arg) for arg in args], env=env, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, timeout=1800)
    if result.returncode:
        raise StateError("publication operation did not complete")
    return result.stdout


def remote_commit(store, tag):
    if not TAG.fullmatch(tag):
        raise StateError("invalid release tag")
    obj = store.call(f"git/ref/tags/{tag}")["object"]
    for _ in range(8):
        if obj["type"] == "commit" and re.fullmatch(r"[0-9a-f]{40}", obj["sha"]):
            return obj["sha"]
        if obj["type"] != "tag":
            break
        obj = store.call(f"git/tags/{obj['sha']}")["object"]
    raise StateError("could not resolve release commit")


def validate_reports(reports, version, matrix_version=1, model=None):
    if matrix_version not in (1, 2):
        raise StateError("unsupported release matrix contract")
    jobs = select_suite("cli")["platforms"]
    if matrix_version == 1:
        jobs = [job for job in jobs if job["system"] != "windows"]
    elif model is None or resolve_model(model, configured="") != model:
        raise StateError("native release matrix requires its selected model")
    architectures = {job["system"]: job["architecture"] for job in jobs}
    expected = {(job["system"], harness) for job in jobs for harness in job["harnesses"]}
    actual = set()
    for report in reports:
        cell = (report["environment"]["operatingSystem"], report["harness"]["id"])
        if cell in actual or cell not in expected:
            raise StateError("unexpected or duplicate release cell")
        actual.add(cell)
        if (report["outcome"] != "passed" or report["trigger"] != "release"
                or report["tier"] != "release-gate" or report["nanHarness"]["version"] != version
                or report["environment"]["architecture"] != architectures[cell[0]]
                or (model is not None and report.get("model") != model)
                or [(c["name"], c["status"]) for c in report["checks"]] != [
                    ("install-and-diagnose", "passed"), ("deterministic-conformance", "passed"),
                    ("live-tool", "passed")]):
            raise StateError("release cell did not satisfy the gate")
    if actual != expected:
        raise StateError("release gate requires its complete supported native matrix")
    for harness in HARNESSES:
        if len({r["harness"]["version"] for r in reports if r["harness"]["id"] == harness}) != 1:
            raise StateError("platform harness versions do not match")


def enqueue_gate(args, store):
    reports = []
    for path in sorted(args.reports.glob("*.json")):
        if path.stat().st_size > 65536:
            raise StateError("report exceeds its size limit")
        command([args.validator, "validate-report", path])
        reports.append(json.loads(path.read_bytes()))
    matrix_version = 2 if getattr(args, "native_matrix", False) else 1
    model = resolve_model(args.model) if matrix_version == 2 else None
    validate_reports(reports, args.tag[1:], matrix_version, model)
    commit = remote_commit(store, args.tag)
    if commit != args.commit:
        raise StateError("tested release commit changed")
    request = {"schemaVersion": 1, "kind": "gate", "tag": args.tag,
               "commit": commit, "reports": reports,
               "sourceRun": os.environ.get("GITHUB_RUN_ID", "manual")}
    if matrix_version == 2:
        request.update({"matrixVersion": 2, "model": model})
    return store.enqueue(request)


def pending_gate_reports(store, tag, commit, matrix_version=1, model=None):
    """Suite evidence already persisted for this exact release but not yet receipted."""
    for _identity, request in store.pending():
        if (request.get("kind") == "gate" and request.get("tag") == tag and request.get("commit") == commit
                and request.get("matrixVersion", 1) == matrix_version and request.get("model") == model):
            return request["reports"]
    return None


def resume(args, store):
    """Reuse only durable complete suite evidence for unchanged attested assets.

    The evidence is a passed receipt, or a gate request that was persisted before an
    interrupted writer created that receipt. Either way no model call is repeated.
    """
    commit = remote_commit(store, args.tag)
    raw = store.get(f"receipts/{receipt_identity(store.repository, args.tag)}.json")
    receipt = json.loads(raw) if raw is not None else None
    recorded_manifest = None
    matrix_version = 2 if getattr(args, "native_matrix", False) else 1
    model = resolve_model(args.model) if matrix_version == 2 else None
    if receipt is not None and receipt["phases"]["suitePassed"] and "reports" in receipt:
        if receipt["tagCommit"] != commit:
            raise StateError("release commit changed after testing")
        if receipt.get("matrixVersion", 1) != matrix_version or receipt.get("model") != model:
            return False
        reports, recorded_manifest = receipt["reports"], receipt["assetManifestSha256"]
    else:
        reports = pending_gate_reports(store, args.tag, commit, matrix_version, model)
        if reports is None:
            return False
    validate_reports(reports, args.tag[1:], matrix_version, model)
    with tempfile.TemporaryDirectory() as temporary:
        manifest = Path(temporary) / "SHA256SUMS"
        command(["gh", "release", "download", args.tag, "--repo", store.repository,
                 "--pattern", "SHA256SUMS", "--output", manifest])
        command(["gh", "attestation", "verify", manifest, "--repo", store.repository,
                 "--signer-workflow", store.repository + "/.github/workflows/release.yml",
                 "--source-ref", "refs/tags/" + args.tag, "--source-digest", commit,
                 "--deny-self-hosted-runners"])
        attested = manifest.read_bytes()
    if recorded_manifest is not None and hashlib.sha256(attested).hexdigest() != recorded_manifest:
        raise StateError("release assets changed after testing")
    digests = {name: digest for digest, _, name in
               (line.partition("  ") for line in attested.decode().splitlines())}
    for report in reports:
        if report["nanHarness"]["sha256"] != digests.get(BINARIES[report["environment"]["operatingSystem"]]):
            raise StateError("reused evidence does not describe the attested binaries")
    args.reports.mkdir(parents=True, exist_ok=True)
    for report in reports:
        name = report["environment"]["operatingSystem"] + "-" + report["harness"]["id"] + ".json"
        (args.reports / name).write_bytes(canonical(report))
    return True


def reviewed_desktop_bytes(path, digest):
    """Choose one bounded report by exact approval digest, including legacy artifacts."""
    candidates = [path] if not path.is_dir() else [
        path / name for name in ("report.json", "deterministic.json", "live.json")
        if (path / name).exists()]
    matches = []
    for candidate in candidates:
        if candidate.is_symlink() or not candidate.is_file():
            raise StateError("Desktop report must be a regular file")
        with candidate.open("rb") as source:
            raw = source.read(49153)
        if len(raw) > 49152:
            raise StateError("Desktop report exceeds its size limit")
        if hashlib.sha256(raw).hexdigest() == digest:
            matches.append(raw)
    if len(matches) != 1:
        raise StateError("approval must identify exactly one unchanged Desktop report")
    return matches[0]


def enqueue_desktop(args, store):
    if not re.fullmatch(r"[0-9a-f]{64}", args.digest or ""):
        raise StateError("an exact reviewed report digest is required")
    if args.issue:
        if not args.issue.isdecimal():
            raise StateError("invalid issue number")
        issue = store.call(f"issues/{args.issue}")
        if "pull_request" in issue:
            raise StateError("expected an issue, not a pull request")
        blocks = re.findall(r"```nanh-desktop-report\n([^`]+)\n```", issue.get("body") or "")
        if len(blocks) != 1:
            raise StateError("issue must contain one checker report block")
        raw = blocks[0].encode()
        source = {"issue": int(args.issue)}
    else:
        if not args.report or not args.run or not args.run.isdecimal():
            raise StateError("a report and source Actions run are required")
        run = store.call(f"actions/runs/{args.run}")
        if (run["path"] != ".github/workflows/desktop-check.yml"
                or run["event"] != "workflow_dispatch" or run["status"] != "completed"
                or run["head_repository"]["full_name"] != store.repository):
            raise StateError("unrecognized Desktop report source")
        raw = reviewed_desktop_bytes(args.report, args.digest)
        source = {"run": int(args.run)}
    if len(raw) > 49152 or hashlib.sha256(raw).hexdigest() != args.digest:
        raise StateError("the reviewed report changed or exceeds its size limit")
    with tempfile.TemporaryDirectory() as temporary:
        report_path = Path(temporary) / "report.json"
        report_path.write_bytes(raw)
        command([args.checker, "validate-report", report_path])
    return store.enqueue({"schemaVersion": 1, "kind": "desktop", "digest": args.digest,
                          "report": json.loads(raw), "source": source})


def checkpoint(args, store):
    receipt = json.loads(args.receipt.read_bytes())
    if receipt["repository"] != store.repository or not TAG.fullmatch(receipt["tag"]):
        raise StateError("receipt identity mismatch")
    phases = receipt["phases"]
    if (receipt.get("schemaVersion") != 2 or set(phases) != set(PHASES)
            or any(type(phases[phase]) is not bool for phase in PHASES)
            or any(phases[PHASES[index]] and not phases[PHASES[index - 1]] for index in range(1, len(PHASES)))
            or not re.fullmatch(r"[0-9a-f]{40}", receipt["tagCommit"])):
        raise StateError("receipt phases or commit are invalid")
    if phases["assetsVerified"] and not re.fullmatch(r"[0-9a-f]{64}", receipt["assetManifestSha256"] or ""):
        raise StateError("verified receipt requires its asset manifest digest")
    identity = receipt_identity(store.repository, receipt["tag"])
    prior = store.get(f"receipts/{identity}.json")
    if prior is not None:
        previous = json.loads(prior)
        if (previous["tagCommit"] != receipt["tagCommit"]
                or (previous["assetManifestSha256"] is not None
                    and previous["assetManifestSha256"] != receipt["assetManifestSha256"])
                or any(previous["phases"][phase] and not phases[phase] for phase in PHASES)):
            raise StateError("durable receipt cannot change identity or regress")
    if receipt["phases"]["suitePassed"] and "reports" not in receipt:
        output = Path(receipt["outputDirectory"])
        validator = output / "run/nan-harness-canary-aarch64-apple-darwin"
        reports = []
        for system in ("linux", "macos"):
            for harness in HARNESSES:
                path = output / "reports" / f"{system}-{harness}.json"
                command([validator, "validate-report", path])
                reports.append(json.loads(path.read_bytes()))
        validate_reports(reports, receipt["tag"][1:])
        receipt["reports"] = reports
    receipt.pop("outputDirectory", None)
    store.put(f"receipts/{identity}.json", canonical(receipt))


def restore_receipt(store, tag, directory, output):
    raw = store.get(f"receipts/{receipt_identity(store.repository, tag)}.json")
    if raw is None:
        return None
    receipt = json.loads(raw)
    if receipt["repository"] != store.repository or receipt["tag"] != tag:
        raise StateError("durable receipt identity does not match")
    receipt["outputDirectory"] = str(output)
    directory.mkdir(parents=True, exist_ok=True)
    (directory / f"{tag}.json").write_bytes(canonical(receipt))
    return receipt


def checkpoint_recommendation(args, store):
    receipt = json.loads(args.receipt.read_bytes())
    if (receipt.get("schemaVersion") != 1 or receipt["repository"] != store.repository
            or not TAG.fullmatch(receipt["tag"]) or receipt["version"] != receipt["tag"][1:]
            or not re.fullmatch(r"[0-9a-f]{40}", receipt["tagCommit"])
            or set(receipt) != {"schemaVersion", "repository", "tag", "version", "tagCommit", "recommendedAt"}):
        raise StateError("invalid recommendation receipt")
    path = f"recommendations/{receipt_identity(store.repository, receipt['tag'])}.json"
    previous = store.get(path)
    if previous is not None:
        if json.loads(previous)["tagCommit"] != receipt["tagCommit"]:
            raise StateError("recommendation identity changed")
        return
    store.put(path, canonical(receipt), immutable=True)


def gate(store, request, work):
    tag, commit = request["tag"], request["commit"]
    matrix_version, model = request.get("matrixVersion", 1), request.get("model")
    validate_reports(request["reports"], tag[1:], matrix_version, model)
    if remote_commit(store, tag) != commit:
        raise StateError("release tag no longer matches the tested commit")
    # Only official release code, never a checkout reference supplied in a report.
    command(["git", "fetch", "--no-tags", "origin", f"refs/tags/{tag}"])
    code = work / "code"
    command(["git", "worktree", "add", "--detach", code, commit])
    try:
        state = work / "state"
        assets = state / "assets" / tag
        assets.mkdir(parents=True)
        downloads = ["gh", "release", "download", tag, "--repo", store.repository,
                     "--pattern", "nan-harness-aarch64-*", "--pattern", "nan-harness-canary-aarch64-*"]
        if matrix_version == 2:
            downloads.extend(("--pattern", BINARIES["windows"]))
        command([*downloads, "--dir", assets])
        command(["bash", code / "canary/host/verify-release-assets.sh", "--release-tag", tag,
                 "--assets-dir", assets, "--repository", store.repository,
                 "--expected-commit", commit])
        manifest = hashlib.sha256((assets / "SHA256SUMS").read_bytes()).hexdigest()
        output = state / "runs" / "hosted"
        reports_directory = output / "reports"
        reports_directory.mkdir(parents=True)
        validator = assets / "nan-harness-canary-aarch64-unknown-linux-musl"
        validator.chmod(0o700)
        for report in request["reports"]:
            system, harness = report["environment"]["operatingSystem"], report["harness"]["id"]
            if report["nanHarness"]["sha256"] != hashlib.sha256((assets / BINARIES[system]).read_bytes()).hexdigest():
                raise StateError("report does not describe the attested binary")
            path = reports_directory / f"{system}-{harness}.json"
            path.write_bytes(canonical(report))
            command([validator, "validate-report", path])
        receipts = state / "receipts" / store.repository.replace("/", "__")
        receipt = restore_receipt(store, tag, receipts, output)
        if receipt and (receipt["tagCommit"] != commit or receipt["assetManifestSha256"] != manifest):
            raise StateError("durable receipt refers to different release assets")
        if receipt and (receipt.get("matrixVersion", 1) != matrix_version or receipt.get("model") != model):
            raise StateError("durable gate receipt belongs to another matrix or model")
        if not receipt:
            receipt = {"schemaVersion": 2, "repository": store.repository, "tag": tag,
                       "tagCommit": commit, "assetManifestSha256": manifest,
                       "outputDirectory": str(output), "availableFeedVersion": None,
                       "phases": {phase: index < 2 for index, phase in enumerate(PHASES)},
                       "reports": request["reports"]}
            if matrix_version == 2:
                receipt.update({"matrixVersion": 2, "model": model})
            receipts.mkdir(parents=True, exist_ok=True)
            (receipts / f"{tag}.json").write_bytes(canonical(receipt))
            durable = dict(receipt)
            durable.pop("outputDirectory")
            store.put(f"receipts/{receipt_identity(store.repository, tag)}.json", canonical(durable))
        env = os.environ.copy()
        env.update({"NAN_CANARY_STATE_DIR": str(state), "NAN_CANARY_TAG_WORKTREE": "1",
                    "NAN_CANARY_TAG_COMMIT": commit, "NAN_CANARY_REPORT_VALIDATOR": str(validator),
                    "NAN_CANARY_RECEIPT_CHECKPOINT_COMMAND": str(ROOT / "canary/actions/checkpoint.sh"),
                    "NAN_CANARY_WRITER": "actions"})
        command(["bash", code / "canary/host/run-release-gate.sh", "--tag", tag,
                 "--repo", store.repository, "--force"], env=env)
    finally:
        command(["git", "worktree", "remove", "--force", code])


def recommend(store, request, work):
    state = work / "state"
    receipts = state / "receipts" / store.repository.replace("/", "__")
    if restore_receipt(store, request["tag"], receipts, state / "runs") is None:
        raise StateError("recommendation requires a complete durable gate receipt")
    env = os.environ.copy()
    env.update({"NAN_CANARY_STATE_DIR": str(state), "NAN_CANARY_WRITER": "actions"})
    env["NAN_CANARY_RECOMMENDATION_CHECKPOINT_COMMAND"] = str(ROOT / "canary/actions/checkpoint-recommendation.sh")
    command(["bash", ROOT / "canary/host/recommend-release.sh", "--tag", request["tag"],
             "--repository", store.repository], env=env)


def desktop(store, request, work, checker):
    report = work / "report.json"
    report.write_bytes(canonical(request["report"]))
    command([checker, "validate-report", report])
    if request["report"].get("nanHarness") is None:
        return False
    updates = work / "updates"
    updates.mkdir()
    result = command([checker, "feed-updates", report])
    if not json.loads(result)["desktopChecks"]:
        return False
    (updates / "desktop.json").write_bytes(result)
    command(["bash", ROOT / "canary/actions/publish-desktop.sh", "--report", report,
             "--updates", updates, "--repository", store.repository])
    return True


def acknowledge_issue(store, issue, identity, published):
    if type(issue) is not int or issue < 1:
        raise StateError("invalid source issue")
    marker = "Publication receipt: `" + identity + "`."
    acknowledged = False
    for page in range(1, 11):
        comments = store.call(f"issues/{issue}/comments?per_page=100&page={page}")
        if any(marker in (comment.get("body") or "") for comment in comments):
            acknowledged = True
            break
        if len(comments) < 100:
            break
    else:
        raise StateError("issue acknowledgement exceeds its pagination limit")
    if not acknowledged:
        body = ("Approved compatibility evidence was published." if published else
                "Reviewed report contains no publishable positive evidence; compatibility is unchanged.")
        store.call(f"issues/{issue}/comments", {"body": body + "\n\n" + marker})
    if published:
        store.call(f"issues/{issue}", {"state": "closed"}, "PATCH")


def drain(args, store):
    failures = 0
    for identity, request in store.pending():
        try:
            with tempfile.TemporaryDirectory(prefix="nan-publication-") as temporary:
                work = Path(temporary)
                if request.get("schemaVersion") != 1:
                    raise StateError("unsupported publication request")
                if request["kind"] == "gate":
                    gate(store, request, work)
                elif request["kind"] == "recommend":
                    recommend(store, request, work)
                elif request["kind"] == "desktop":
                    published = desktop(store, request, work, args.checker)
                    issue = request["source"].get("issue")
                    if issue is not None:
                        acknowledge_issue(store, issue, identity, published)
                elif request["kind"] == "hosted":
                    from hosted_publication import publish
                    publish(args, store, request, work, ROOT, command, remote_commit)
                else:
                    raise StateError("unknown publication kind")
            store.put(f"completed/{identity}.json", canonical({"request": identity}), immutable=True)
        except (StateError, OSError, KeyError, ValueError, subprocess.SubprocessError):
            failures += 1
            print(f"Publication {identity} remains pending; no private output was logged.", file=sys.stderr)
    if failures:
        raise StateError("some requests remain pending")


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=("initialize", "gate", "desktop", "recommend", "checkpoint",
                                              "checkpoint-recommendation", "drain", "resume", "hosted"))
    parser.add_argument("--repository", required=True)
    parser.add_argument("--tag")
    parser.add_argument("--commit")
    parser.add_argument("--reports", type=Path)
    parser.add_argument("--validator", type=Path, default=ROOT / "target/release/nan-harness-canary")
    parser.add_argument("--checker", type=Path, default=ROOT / "target/release/nanh-desktop-check")
    parser.add_argument("--receipt", type=Path)
    parser.add_argument("--issue")
    parser.add_argument("--run")
    parser.add_argument("--report", type=Path)
    parser.add_argument("--digest")
    parser.add_argument("--native-matrix", action="store_true")
    parser.add_argument("--model", default="")
    args = parser.parse_args()
    try:
        store = Store(args.repository)
        if args.operation == "initialize":
            store.initialize()
        elif args.operation == "gate":
            print(enqueue_gate(args, store))
        elif args.operation == "desktop":
            print(enqueue_desktop(args, store))
        elif args.operation == "hosted":
            from hosted_publication import enqueue
            print(enqueue(args, store, ROOT, command, remote_commit))
        elif args.operation == "recommend":
            remote_commit(store, args.tag)
            print(store.enqueue({"schemaVersion": 1, "kind": "recommend", "tag": args.tag}))
        elif args.operation == "checkpoint":
            checkpoint(args, store)
        elif args.operation == "checkpoint-recommendation":
            checkpoint_recommendation(args, store)
        elif args.operation == "resume":
            print("true" if resume(args, store) else "false")
        else:
            drain(args, store)
    except (StateError, OSError, KeyError, TypeError, ValueError, subprocess.SubprocessError):
        print("Publication failed closed; retry after resolving the state or validation failure.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
