"""Read data-only hosted artifacts and publish independently bound observations."""

import hashlib
import io
import json
from pathlib import Path
import re
import stat
import tempfile
import zipfile

from hosted import report_update
from provenance import bind_report, specification_digest, trusted_run
from selection import native_platform, resolve_model
from state import StateError, canonical

ARTIFACT = re.compile(r"hosted-evidence-(cli|desktop)-(linux|macos|windows)\Z")
TAG = re.compile(r"v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\Z")
LIMIT = 2_000_000


def artifact_bundle(raw):
    """Never extract a path, import a module, or execute an artifact member."""
    if len(raw) > LIMIT:
        raise StateError("hosted artifact exceeds its size limit")
    try:
        with zipfile.ZipFile(io.BytesIO(raw)) as archive:
            entries = archive.infolist()
            if len(entries) != 1:
                raise StateError("hosted artifact requires exactly one data file")
            entry = entries[0]
            mode = entry.external_attr >> 16
            if (entry.filename != "evidence.json" or entry.file_size > LIMIT
                    or entry.flag_bits & 1 or stat.S_ISLNK(mode)
                    or (stat.S_IFMT(mode) not in (0, stat.S_IFREG))):
                raise StateError("invalid hosted artifact member")
            return json.loads(archive.read(entry))
    except (zipfile.BadZipFile, RuntimeError) as error:
        raise StateError("invalid hosted artifact archive") from error


def source_artifacts(store, run_id, command):
    """Bind each archive to GitHub's artifact ID, run, name and immutable digest."""
    response = store.call(f"actions/runs/{run_id}/artifacts?per_page=100")
    if response.get("total_count", 101) > 100:
        raise StateError("too many source artifacts")
    seen = set()
    for item in response.get("artifacts", []):
        name = item.get("name", "")
        match = ARTIFACT.fullmatch(name)
        if not match:
            continue
        identity = item.get("id")
        if (name in seen or type(identity) is not int or identity < 1
                or item.get("expired") is not False
                or item.get("workflow_run", {}).get("id") != run_id
                or type(item.get("size_in_bytes")) is not int
                or not 0 < item["size_in_bytes"] <= LIMIT):
            raise StateError("invalid hosted artifact identity")
        seen.add(name)
        raw = command(["gh", "api", f"repos/{store.repository}/actions/artifacts/{identity}/zip"])
        if item.get("digest") != "sha256:" + hashlib.sha256(raw).hexdigest():
            raise StateError("hosted artifact digest changed")
        bundle = artifact_bundle(raw)
        if (bundle.get("suite"), bundle.get("platform")) != match.groups():
            raise StateError("artifact name does not match evidence identity")
        yield bundle
    if not seen:
        raise StateError("source run has no hosted evidence")


def validate_bundle(bundle, source_commit):
    fields = {"schemaVersion", "suite", "platform", "architecture", "sourceCommit",
              "releaseTag", "releaseCommit", "model", "specSha256", "reports"}
    if (set(bundle) != fields or bundle["schemaVersion"] != 1
            or bundle["sourceCommit"] != source_commit
            or bundle["suite"] not in ("cli", "desktop")
            or bundle["platform"] not in ("linux", "macos", "windows")
            or not TAG.fullmatch(bundle["releaseTag"])
            or not re.fullmatch(r"[0-9a-f]{40}", bundle["releaseCommit"])
            or resolve_model(bundle["model"], configured="") != bundle["model"]
            or not isinstance(bundle["reports"], list)
            or not 1 <= len(bundle["reports"]) <= 30):
        raise StateError("invalid hosted evidence envelope")
    platform = native_platform(bundle["suite"], bundle["platform"])
    if platform["architecture"] != bundle["architecture"]:
        raise StateError("unexpected native architecture")
    return platform


def attested_release(store, bundle, work, command, remote_commit):
    """Authenticate the checksum manifest and load registry bytes, never tag code."""
    tag, commit = bundle["releaseTag"], bundle["releaseCommit"]
    if remote_commit(store, tag) != commit:
        raise StateError("hosted release tag changed")
    manifest = work / "SHA256SUMS"
    command(["gh", "release", "download", tag, "--repo", store.repository,
             "--pattern", "SHA256SUMS", "--output", manifest])
    command(["gh", "attestation", "verify", manifest, "--repo", store.repository,
             "--signer-workflow", store.repository + "/.github/workflows/release.yml",
             "--source-ref", "refs/tags/" + tag, "--source-digest", commit,
             "--deny-self-hosted-runners"])
    if manifest.stat().st_size > 65536:
        raise StateError("release manifest exceeds its size limit")
    digests = {}
    for line in manifest.read_text().splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9._-]+)", line)
        if not match or match[2] in digests:
            raise StateError("invalid release checksum entry")
        digests[match[2]] = match[1]
    command(["git", "fetch", "--no-tags", "origin", f"refs/tags/{tag}"])
    registry = command(["git", "show", commit + ":crates/nan-harness-runtime/resources/compatibility.json"])
    if len(registry) > LIMIT:
        raise StateError("release registry exceeds its size limit")
    return digests, registry


def bundle_updates(store, bundle, source_commit, work, root, validators, command, remote_commit):
    platform = validate_bundle(bundle, source_commit)
    spec = specification_digest(root, source_commit, bundle["suite"])
    if bundle["specSha256"] != spec:
        raise StateError("probe specification changed")
    digests, registry = attested_release(store, bundle, work, command, remote_commit)
    suffix = ".exe" if bundle["platform"] == "windows" else ""
    binary = f"nan-harness-{platform['architecture']}-{platform['target']}{suffix}"
    if binary not in digests:
        raise StateError("release lacks its native binary")
    expected = {"version": bundle["releaseTag"][1:], "binarySha256": digests[binary],
                "platform": bundle["platform"], "architecture": platform["architecture"],
                "model": bundle["model"]}
    updates = []
    for index, report in enumerate(bundle["reports"]):
        raw = canonical(report)
        if len(raw) > 65536:
            raise StateError("hosted report exceeds its size limit")
        path = work / f"report-{index}.json"
        path.write_bytes(raw)
        command([validators[bundle["suite"]], "validate-report", path])
        bind_report(report, bundle["suite"], expected)
        updates.append(raw)
    return updates, registry


def enqueue(args, store, root, command, remote_commit):
    run_id = int(args.run)
    source_commit = trusted_run(store, run_id)
    command(["git", "fetch", "--no-tags", "origin", source_commit])
    bundles = list(source_artifacts(store, run_id, command))
    validators = {"cli": args.validator, "desktop": args.checker}
    for bundle in bundles:
        with tempfile.TemporaryDirectory(prefix="nan-hosted-validate-") as temporary:
            bundle_updates(store, bundle, source_commit, Path(temporary), root,
                           validators, command, remote_commit)
    # Only validated, closed report schemas enter the durable data-only queue.
    request = {"schemaVersion": 1, "kind": "hosted", "sourceRun": run_id,
               "sourceCommit": source_commit, "bundles": bundles}
    if len(canonical(request)) > LIMIT:
        raise StateError("hosted request exceeds its durable size limit")
    return store.enqueue(request)


def publish(args, store, request, work, root, command, remote_commit):
    source_commit = trusted_run(store, request["sourceRun"])
    if source_commit != request["sourceCommit"]:
        raise StateError("queued source identity changed")
    command(["git", "fetch", "--no-tags", "origin", source_commit])
    validators = {"cli": args.validator, "desktop": args.checker}
    for index, bundle in enumerate(request["bundles"]):
        directory = work / str(index)
        directory.mkdir()
        reports, registry = bundle_updates(store, bundle, source_commit, directory,
                                          root, validators, command, remote_commit)
        updates = directory / "updates"
        updates.mkdir()
        count = 0
        for number, raw in enumerate(reports):
            update = report_update(bundle["suite"], raw, bundle["specSha256"], request["sourceRun"])
            if update is not None and update["hostedChecks"]:
                (updates / f"{number}.json").write_bytes(canonical(update))
                count += 1
        if not count:
            continue
        registry_path = directory / "registry.json"
        registry_path.write_bytes(registry)
        command(["bash", root / "canary/actions/publish-hosted.sh", "--updates", updates,
                 "--registry", registry_path, "--version", bundle["releaseTag"][1:],
                 "--repository", store.repository, "--output", directory / "candidate.json", "--publish"])
