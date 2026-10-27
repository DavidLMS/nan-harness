#!/usr/bin/env python3
"""Stage an attested release pair without replacing the trusted diagnostic controller."""
import argparse
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tempfile
import zipfile

from desktop_suite import digest, validate_release_assets
from selection import PLATFORM_ASSETS, required_assets

COMMIT = re.compile(r"[0-9a-f]{40}\Z")
SHA256 = re.compile(r"[0-9a-f]{64}\Z")
TAG = re.compile(r"v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\Z")
REPOSITORY = "DavidLMS/nan-harness"
MAX_ARCHIVE = 1024 * 1024 * 1024
# release-gate.yml also stages the Linux x64 CLI for its native Desktop jobs.
PREPARE_MEMBERS = frozenset((*required_assets(), "nan-harness-x86_64-unknown-linux-musl", "SHA256SUMS"))


def execute(argv, output=None):
    result = subprocess.run(argv, stdout=output if output is not None else subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL, check=False, timeout=180)
    if result.returncode:
        raise ValueError("release evidence command failed")


def api(endpoint):
    with tempfile.TemporaryFile() as output:
        execute(["gh", "api", endpoint], output)
        output.seek(0)
        raw = output.read(2 * 1024 * 1024 + 1)
    if len(raw) > 2 * 1024 * 1024:
        raise ValueError("release metadata exceeds bound")
    return json.loads(raw)


def validate_run(run, run_id):
    if (type(run) is not dict or type(run.get("id")) is not int or run.get("id") != run_id
            or run.get("head_branch") != "main" or run.get("event") != "workflow_dispatch"
            or run.get("path") != ".github/workflows/release-gate.yml"
            or type(run.get("head_sha")) is not str or not COMMIT.fullmatch(run["head_sha"])
            or type(run.get("repository")) is not dict or type(run.get("head_repository")) is not dict
            or run["repository"].get("full_name") != REPOSITORY
            or run["head_repository"].get("full_name") != REPOSITORY):
        raise ValueError("canonical asset run identity differs")
    return run["head_sha"]


def select_artifact(value, run_id, head):
    artifacts = value.get("artifacts") if type(value) is dict else None
    if type(artifacts) is not list or type(value.get("total_count")) is not int or value.get("total_count") != len(artifacts) or len(artifacts) > 100:
        raise ValueError("canonical artifact list is incomplete")
    selected = [item for item in artifacts if type(item) is dict and item.get("name") == "release-gate-assets"]
    if len(selected) != 1:
        raise ValueError("canonical release artifact is not unique")
    artifact = selected[0]
    binding = artifact.get("workflow_run", {})
    if (artifact.get("expired") is not False or type(artifact.get("id")) is not int or artifact["id"] < 1
            or type(artifact.get("size_in_bytes")) is not int or not 0 < artifact["size_in_bytes"] <= MAX_ARCHIVE
            or type(artifact.get("digest")) is not str or not re.fullmatch(r"sha256:[0-9a-f]{64}", artifact["digest"])
            or type(binding) is not dict or type(binding.get("id")) is not int
            or binding.get("id") != run_id or binding.get("head_sha") != head or binding.get("head_branch") != "main"):
        raise ValueError("canonical artifact identity differs")
    return artifact


def extract_pair(archive, directory):
    wanted = {"SHA256SUMS", PLATFORM_ASSETS["windows"]["harness"], PLATFORM_ASSETS["windows"]["canary"]}
    known = PREPARE_MEMBERS
    with zipfile.ZipFile(archive) as source:
        entries = source.infolist()
        names = [item.filename for item in entries]
        if (len(names) != len(set(names)) or set(names) != known
                or any(item.filename not in known or item.flag_bits & 1 or item.is_dir()
                       or stat.S_ISLNK(item.external_attr >> 16) or not 0 < item.file_size <= MAX_ARCHIVE
                       or item.compress_type not in {zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED}
                       or item.filename == "SHA256SUMS" and item.file_size > 65536
                       for item in entries) or sum(item.file_size for item in entries) > MAX_ARCHIVE):
            raise ValueError("release archive shape differs")
        for name in wanted:
            with source.open(name) as reader, (directory / name).open("xb") as output:
                while block := reader.read(1024 * 1024):
                    output.write(block)


def stage(args, fetch=api, command=execute):
    if (args.repository != REPOSITORY or not TAG.fullmatch(args.tag)
            or not COMMIT.fullmatch(args.commit) or not COMMIT.fullmatch(args.controller)
            or not SHA256.fullmatch(args.manifest_sha256) or type(args.run_id) is not int or not 1 <= args.run_id <= 2**63 - 1
            or args.directory.exists() or args.directory.is_symlink()):
        raise ValueError("release staging identity is invalid")
    prefix = f"repos/{REPOSITORY}"
    head = validate_run(fetch(f"{prefix}/actions/runs/{args.run_id}"), args.run_id)
    artifact = select_artifact(fetch(f"{prefix}/actions/runs/{args.run_id}/artifacts?per_page=100"), args.run_id, head)
    reference = fetch(f"{prefix}/git/ref/tags/{args.tag}")
    tag = reference.get("object") if type(reference) is dict else None
    for _ in range(4):
        if type(tag) is not dict:
            raise ValueError("release tag cannot be resolved")
        if tag.get("type") == "commit":
            break
        if tag.get("type") != "tag" or type(tag.get("sha")) is not str or not COMMIT.fullmatch(tag["sha"]):
            raise ValueError("release tag cannot be resolved")
        reference = fetch(f"{prefix}/git/tags/{tag['sha']}")
        tag = reference.get("object") if type(reference) is dict else None
    if type(tag) is not dict or tag.get("type") != "commit" or tag.get("sha") != args.commit:
        raise ValueError("release tag commit differs")
    args.directory.mkdir(mode=0o700, parents=True)
    with tempfile.TemporaryDirectory(dir=args.directory.parent) as temporary:
        archive = Path(temporary) / "release.zip"
        with archive.open("xb") as output:
            command(["gh", "api", f"{prefix}/actions/artifacts/{artifact['id']}/zip"], output)
        if archive.stat().st_size != artifact["size_in_bytes"] or digest(archive) != artifact["digest"][7:]:
            raise ValueError("release archive digest differs")
        extract_pair(archive, args.directory)
    manifest = args.directory / "SHA256SUMS"
    if digest(manifest) != args.manifest_sha256:
        raise ValueError("release checksum anchor differs")
    cli = args.directory / PLATFORM_ASSETS["windows"]["harness"]
    canary = args.directory / PLATFORM_ASSETS["windows"]["canary"]
    verify = validate_release_assets(manifest, {cli.name: cli, canary.name: canary}, args.tag, args.commit)
    command([*verify, "--repo", REPOSITORY, "--signer-workflow", REPOSITORY + "/.github/workflows/release.yml",
             "--deny-self-hosted-runners"])
    provenance = dict(schemaVersion=1, controllerSha=args.controller, releaseTag=args.tag,
        releaseCommit=args.commit, assetRunId=args.run_id, assetWorkflowSha=head, artifactId=artifact["id"],
        artifactSha256=artifact["digest"][7:], manifestSha256=digest(manifest),
        nanHarnessSha256=digest(cli), canarySha256=digest(canary))
    return cli, canary, provenance


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--controller", required=True)
    parser.add_argument("--run-id", type=int, required=True)
    parser.add_argument("--manifest-sha256", required=True)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--provenance", type=Path, required=True)
    args = parser.parse_args()
    try:
        cli, canary, provenance = stage(args)
        args.provenance.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        with args.provenance.open("x") as output:
            json.dump(provenance, output, sort_keys=True)
            output.write("\n")
        with Path(os.environ["GITHUB_ENV"]).open("a") as output:
            output.write(f"NAN_DIAGNOSTIC_RELEASE_BINARY={cli.resolve()}\nNAN_DIAGNOSTIC_RELEASE_CANARY={canary.resolve()}\nNAN_DIAGNOSTIC_RELEASE_VERIFIED=1\n")
    except (OSError, ValueError, KeyError, TypeError, zipfile.BadZipFile, subprocess.SubprocessError):
        print("Attested release staging failed; no release binary was started.")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
