#!/usr/bin/env python3
"""Observe one canonical Linux MiMo live probe with immutable v0.1.27 assets."""
import argparse
import copy
import json
import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import cell
import mimo_conformance_diagnostic as canonical
import mimo_inventory_diagnostic as inventory
import mimo_live_observation as observation

TAG = "v0.1.27"
COMMIT = "7c74af43c6c2917737ad90859c0d2706d674e75f"
MANIFEST = "205da655631c75615c33ac2a07520f7663aa3251c712fa9cdb493ba0bb7bf5b8"
CLI_SHA = "f68de8e51ee49b4cb26ae6e83197c6c04fa9e4ccec3ffd7cfa900ebc148ef0fa"
CANARY_SHA = "fb805d3469f0eb34278076cdb44de6a9778420736f31f1b6bc02340cc83f78fd"


def binding(args):
    value = inventory.read_json(args.provenance)
    if (type(value) is not dict or set(value) != inventory.PROVENANCE_KEYS
            or type(value["schemaVersion"]) is not int or value["schemaVersion"] != 1
            or value["controllerSha"] != args.source_sha
            or value["releaseTag"] != TAG or value["releaseCommit"] != COMMIT
            or value["manifestSha256"] != MANIFEST
            or value["nanHarnessSha256"] != CLI_SHA or value["canarySha256"] != CANARY_SHA
            or any(type(value[key]) is not str or not inventory.SOURCE.fullmatch(value[key])
                   for key in ("controllerSha", "releaseCommit", "assetWorkflowSha"))
            or any(type(value[key]) is not str or not inventory.DIGEST.fullmatch(value[key])
                   for key in ("artifactSha256", "manifestSha256", "nanHarnessSha256", "canarySha256"))
            or any(not inventory.integer(value[key], 1, 2**63-1) for key in ("assetRunId", "artifactId"))
            or inventory.digest(args.binary) != CLI_SHA or inventory.digest(args.canary) != CANARY_SHA):
        raise ValueError("immutable release binding differs")
    return value


def frozen_selection(args):
    inventory.read_json(args.manifest)
    suite = inventory.windows.resolver_module()
    items = suite.read_frozen_manifest(args.manifest, ["mimo-code"], "linux", "aarch64", "qwen3.6")
    if len(items) != 1 or items[0].version != "0.1.16" or items[0].ref or items[0].runtime_version:
        raise ValueError("frozen stock version differs")
    return items[0]


def probe_failure(report, failure):
    stage = failure.stage if type(failure.stage) is str and failure.stage in cell.PROBE_STAGES | {"marker-missing"} else "unavailable"
    status = failure.status if inventory.integer(failure.status, -2147483648, 2147483647) else None
    report.update(originalLiveStage=stage, originalLiveExitCode=status,
                  originalLiveCode=f"live-{stage}-exit-{status}" if stage != "unavailable" and status is not None else "live-probe-unavailable")


def collect(args):
    source = args.source_sha if type(args.source_sha) is str and inventory.SOURCE.fullmatch(args.source_sha) else None
    report = dict(schemaVersion=1, diagnosticsOnly=True, qualification=False, observerKind="canonical-live-controller",
                  observerSourceSha=source, releaseProvenance=None, stockVersion="0.1.16", model="qwen3.6",
                  platform={"system": "linux", "architecture": "aarch64"}, installStatus="not-run",
                  originalLiveStatus="not-run", originalLiveStage=None, originalLiveCode=None,
                  originalLiveExitCode=None, observationStatus="not-run", observation=None,
                  status="failed", reason="source-invalid")
    if source is None or os.environ.get("GITHUB_SHA") != source or os.environ.get("GITHUB_REF") != "refs/heads/main":
        return report
    phase = "provenance-invalid"
    github_token = os.environ.pop("GITHUB_TOKEN", None)
    try:
        original = binding(args)
        report["releaseProvenance"] = original
        phase = "platform-invalid"
        if cell.detected_identity() != ("linux", "aarch64"):
            raise ValueError("native platform differs")
        phase = "manifest-invalid"
        item = frozen_selection(args)
        phase = "layout-invalid"
        command = canonical.cell_arguments(args, item)
        command.mode, command.trigger = "live", "release"
        if not os.environ.get("NAN_API_KEY"):
            phase = "credential-missing"
            raise ValueError("live credential unavailable")
        state = {"harness": {"id": "mimo-code", "version": "unknown"}, "environment": {"runtimes": []}}
        phase = "install-failed"
        report["installStatus"] = "failed"
        try:
            with canonical.deterministic_environment():
                cell.install(command, state)
            report["installStatus"] = "passed"
        finally:
            previous, phase = phase, "binding-changed"
            if binding(args) != original:
                raise ValueError("release binding changed")
            phase = previous
        callbacks = 0

        def observe(value):
            nonlocal callbacks
            callbacks += 1
            try:
                if callbacks != 1:
                    raise ValueError("repeated live observation")
                report["observation"] = None if value is None else copy.deepcopy(observation.validate(value))
                report["observationStatus"] = "missing" if value is None else "valid"
            except Exception:
                # Observation failures never interrupt the original cell's acceptance path.
                report.update(observation=None, observationStatus="invalid")

        phase = "live-unavailable"
        report["originalLiveStatus"] = "failed"
        try:
            try:
                completed = cell.live(command, state, observe=observe)
                if completed != 1:
                    raise ValueError("live probe did not complete once")
                report.update(originalLiveStatus="passed", originalLiveStage="complete", originalLiveExitCode=0)
            except cell.ProbeFailure as failure:
                phase = "live-probe-failed"
                probe_failure(report, failure)
                raise
            except (cell.CleanupError, cell.ProbeCleanupError):
                phase = "cleanup-unproven"
                report.update(originalLiveStage="cleanup", originalLiveCode="live-cleanup-unproven")
                raise
            except cell.StageTimeout:
                phase = "live-timeout"
                report["originalLiveCode"] = "live-timeout"
                raise
            except cell.CompatibilityMismatch:
                phase = "live-mismatch"
                report.update(originalLiveStage="usage-summary", originalLiveCode="live:usage-summary")
                raise
        finally:
            previous, phase = phase, "binding-changed"
            if binding(args) != original:
                raise ValueError("release binding changed")
            phase = previous
        if callbacks != 1 or report["observationStatus"] != "valid":
            phase = "observation-invalid" if report["observationStatus"] == "invalid" else "observation-unavailable"
            raise ValueError("live observation unavailable")
        report.update(status="observed", reason=None)
    except Exception:
        report["reason"] = phase
        if phase == "binding-changed":
            live_status = "not-run" if report["originalLiveStatus"] == "not-run" else "unproven"
            report.update(observation=None, observationStatus="not-run", originalLiveStatus=live_status,
                          originalLiveStage=None, originalLiveCode=None, originalLiveExitCode=None)
        elif phase != "cleanup-unproven" and report["originalLiveStatus"] == "failed" and report["observationStatus"] in ("missing", "invalid"):
            report["reason"] = "observation-invalid" if report["observationStatus"] == "invalid" else "observation-unavailable"
    finally:
        if github_token is not None:
            os.environ["GITHUB_TOKEN"] = github_token
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("binary", "canary", "provenance", "manifest", "directory", "output"):
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--source-sha", required=True)
    args = parser.parse_args()
    created = False
    try:
        if not args.output.is_absolute() or args.output.exists() or args.output.is_symlink() or not args.output.parent.is_dir():
            raise ValueError("diagnostic destination unavailable")
        report = collect(args)
        with args.output.open("x", encoding="utf-8") as output:
            created = True
            cell.protect_private(args.output)
            json.dump(report, output, sort_keys=True)
            output.write("\n")
    except Exception:
        if created:
            try:
                args.output.unlink(missing_ok=True)
            except OSError:
                pass
        print("MiMo live observation could not be recorded safely.")
        return 1
    return 0 if report["status"] == "observed" and report["originalLiveStatus"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
