#!/usr/bin/env python3
"""Run the selected CLI harnesses sequentially in isolated native cells."""

import argparse
from dataclasses import asdict, dataclass
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib
from urllib.request import Request, urlopen

sys.path.insert(0, str(Path(__file__).resolve().parent))
from selection import CLI_HARNESSES, resolve_model
from cell import SEMVER, source_identity


@dataclass(frozen=True)
class FrozenHarness:
    """The immutable installer identity passed from resolution to a cell.

    ``ref`` is empty except for sources whose release tag is not the product
    version; it then holds the 40-character commit the installer must check out.
    """

    harness: str
    version: str
    system: str
    architecture: str
    source: str
    package: str = ""
    model: str = ""
    ref: str = ""

    def as_dict(self):
        return asdict(self)


@dataclass(frozen=True)
class UnresolvedHarness:
    """A selected harness whose official metadata was unavailable; it has no version."""

    harness: str
    system: str
    architecture: str
    source: str
    package: str = ""
    model: str = ""

    def as_dict(self):
        return asdict(self)


_NPM_PACKAGES = {
    "claude-code": "@anthropic-ai/claude-code", "codex": "@openai/codex",
    "opencode": "opencode-ai", "pi": "@earendil-works/pi-coding-agent",
    "deepseek-harness": "@deepseek-ai/dsh", "openclaw": "openclaw",
    "cline": "cline", "qwen-code": "@qwen-code/qwen-code",
}
_PYPI_PACKAGES = {"aider": "aider-chat"}
_GITHUB_REPOS = {
    "omp": "can1357/oh-my-pi", "goose": "block/goose",
    "hermes": "NousResearch/hermes-agent",
}
# Hermes tags releases by date (v2026.9.11) while `hermes --version` reports the
# pyproject version (0.21.2). Freeze the tag's commit and read the version there.
_COMMIT_PINNED = frozenset({"hermes"})
FX_SOURCE = "https://releases.fx.sh/latest.txt"
_TEXT_SOURCES = {
    "fx": FX_SOURCE,
    "kimi-code": "https://code.kimi.com/kimi-code/latest",
    # Official install.sh resolves this stable channel, not GitHub's latest tag.
    "prime-agent": "https://pub-728493de92a943e2a9b2d17b4719f318.r2.dev/stable",
}
_COMMIT = re.compile(r"[0-9a-f]{40}\Z")
_MANIFEST_METADATA_ERRORS = (OSError, KeyError, TypeError, ValueError, UnicodeError)


def _official_json(url):
    request = Request(url, headers={"Accept": "application/json", "User-Agent": "nan-harness-cli-gate"})
    with urlopen(request, timeout=20) as response:
        raw = response.read(2_000_001)
        if len(raw) > 2_000_000:
            raise ValueError("official version metadata exceeds its size limit")
        return json.loads(raw)


def _official_text(url, limit=256):
    with urlopen(Request(url, headers={"User-Agent": "nan-harness-cli-gate"}), timeout=20) as response:
        raw = response.read(limit + 1)
    if len(raw) > limit:
        raise ValueError("official version marker exceeds its size limit")
    return raw.decode("ascii" if limit <= 256 else "utf-8").strip()


def _version(value):
    if not isinstance(value, str):
        raise ValueError("official metadata requires a version string")
    if value.startswith("v"):
        value = value[1:]
    if not SEMVER.fullmatch(value):
        raise ValueError("official metadata did not contain a semantic version")
    return value


def _source(harness):
    """Closed source and package identity for a known CLI harness."""
    if harness in _NPM_PACKAGES:
        return "npm:" + _NPM_PACKAGES[harness], _NPM_PACKAGES[harness]
    if harness in _PYPI_PACKAGES:
        return "pypi:" + _PYPI_PACKAGES[harness], _PYPI_PACKAGES[harness]
    if harness in _GITHUB_REPOS:
        return "github:" + _GITHUB_REPOS[harness], ""
    if harness in _TEXT_SOURCES:
        return _TEXT_SOURCES[harness], ""
    raise ValueError("unknown CLI harness: " + harness)


def _pinned_project_version(repo, tag, fetch_json, fetch_document):
    commit = fetch_json("https://api.github.com/repos/" + repo + "/commits/" + tag)["sha"]
    if not isinstance(commit, str) or not _COMMIT.fullmatch(commit):
        raise ValueError("official release tag did not resolve to a commit")
    project = tomllib.loads(fetch_document(
        "https://raw.githubusercontent.com/" + repo + "/" + commit + "/pyproject.toml"))["project"]
    return _version(project["version"]), commit


def _resolve_one(harness, system, architecture, model, fetch_json, fetch_text, fetch_document):
    source, package = _source(harness)
    ref = ""
    if harness in _NPM_PACKAGES:
        version = _version(fetch_json("https://registry.npmjs.org/" + package + "/latest")["version"])
    elif harness in _PYPI_PACKAGES:
        version = _version(fetch_json("https://pypi.org/pypi/" + package + "/json")["info"]["version"])
    elif harness in _GITHUB_REPOS:
        repo = _GITHUB_REPOS[harness]
        tag = fetch_json("https://api.github.com/repos/" + repo + "/releases/latest")["tag_name"]
        if harness in _COMMIT_PINNED:
            if not isinstance(tag, str) or not re.fullmatch(r"v[0-9][0-9A-Za-z.-]{0,63}", tag):
                raise ValueError("official release tag is not a closed identifier")
            version, ref = _pinned_project_version(repo, tag, fetch_json, fetch_document)
        else:
            version = _version(tag)
    else:
        version = _version(fetch_text(source))
    return FrozenHarness(harness, version, system, architecture, source, package, model, ref)


def resolve_manifest(harnesses, system, architecture, model, fetch_json=_official_json,
                     fetch_text=_official_text, fetch_document=None):
    """Resolve each official source independently.

    One unavailable upstream yields an ``UnresolvedHarness`` with no version; the
    others keep their frozen identities. Fetchers are injectable so tests stay offline.
    """
    fetch_document = fetch_document or (lambda url: _official_text(url, 200_000))
    for harness in harnesses:
        _source(harness)
    resolved, unresolved = [], []
    for harness in harnesses:
        try:
            resolved.append(_resolve_one(harness, system, architecture, model,
                                         fetch_json, fetch_text, fetch_document))
        except _MANIFEST_METADATA_ERRORS + (tomllib.TOMLDecodeError,):
            source, package = _source(harness)
            unresolved.append(UnresolvedHarness(harness, system, architecture, source, package, model))
    return resolved, unresolved


def resolve_frozen_versions(harnesses, system, architecture, model, fetch_json=_official_json,
                            fetch_text=_official_text, fetch_document=None):
    """Strict resolution: every selected harness must have an official version."""
    resolved, unresolved = resolve_manifest(harnesses, system, architecture, model,
                                            fetch_json, fetch_text, fetch_document)
    if unresolved:
        raise ValueError("official version metadata is unavailable for " + unresolved[0].harness)
    return resolved


def _load_manifest(path, harnesses, system, architecture, model):
    """Validate the whole manifest: resolved and unresolved entries partition the request."""
    try:
        document = json.loads(Path(path).read_bytes())
        if not isinstance(document, dict) or not set(document) <= {"harnesses", "unresolved"}:
            raise ValueError("frozen manifest has unknown fields")
        resolved = [FrozenHarness(**entry) for entry in document["harnesses"]]
        unresolved = [UnresolvedHarness(**entry) for entry in document.get("unresolved", [])]
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise ValueError("invalid frozen CLI manifest") from error
    requested = list(harnesses)
    resolved_names = [item.harness for item in resolved]
    unresolved_names = [item.harness for item in unresolved]
    if (len(set(requested)) != len(requested)
            or sorted(resolved_names + unresolved_names) != sorted(requested)
            or resolved_names != [name for name in requested if name in resolved_names]
            or unresolved_names != [name for name in requested if name in unresolved_names]):
        raise ValueError("frozen manifest harness selection differs from this run")
    for item in resolved + unresolved:
        if (item.system, item.architecture, item.model) != (system, architecture, model):
            raise ValueError("frozen manifest platform or model differs from this run")
        if (item.source, item.package) != _source(item.harness):
            raise ValueError("frozen manifest has an untrusted installer source")
    for item in resolved:
        if _version(item.version) != item.version:
            raise ValueError("frozen manifest has a noncanonical version")
        if not isinstance(item.ref, str) or not (
                _COMMIT.fullmatch(item.ref) if item.harness in _COMMIT_PINNED else item.ref == ""):
            raise ValueError("frozen manifest has an untrusted installer ref")
    return resolved, unresolved


def read_frozen_manifest(path, harnesses, system, architecture, model):
    """Resolved entries, in request order, from a manifest that covers ``harnesses`` exactly."""
    return _load_manifest(path, harnesses, system, architecture, model)[0]


def read_unresolved_manifest(path, harnesses, system, architecture, model):
    """Unresolved entries from the same validated manifest; they carry no version."""
    return _load_manifest(path, harnesses, system, architecture, model)[1]


def resolve_main(argv):
    parser = argparse.ArgumentParser(description="Resolve official CLI versions")
    parser.add_argument("--harnesses", required=True)
    parser.add_argument("--system", required=True)
    parser.add_argument("--architecture", required=True)
    parser.add_argument("--model", default="")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    harnesses = [value.strip() for value in args.harnesses.split(",") if value.strip()]
    model = resolve_model(args.model)
    resolved, unresolved = resolve_manifest(harnesses, args.system, args.architecture, model)
    args.output.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    args.output.write_text(json.dumps({"harnesses": [item.as_dict() for item in resolved],
                                       "unresolved": [item.as_dict() for item in unresolved]},
                                      sort_keys=True))
    os.chmod(args.output, 0o600)
    return 0


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "resolve":
        return resolve_main(sys.argv[2:])
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
    parser.add_argument("--system", required=True)
    parser.add_argument("--architecture", required=True)
    parser.add_argument("--source-kind", choices=("branch", "release"), required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--nan-version", required=True)
    parser.add_argument("--cell-script", type=Path,
                        default=Path(__file__).resolve().parent / "cell.py")
    parser.add_argument("--manifest", type=Path, required=True)
    args = parser.parse_args()
    try:
        model = resolve_model(args.model)
        source_identity(args.source_sha)
        harnesses = [item.strip() for item in args.harnesses.split(",")]
        if not harnesses or len(harnesses) != len(set(harnesses)) or any(item not in CLI_HARNESSES for item in harnesses):
            raise ValueError("harnesses must be distinct known CLI identifiers")
        frozen, unresolved = _load_manifest(args.manifest, harnesses, args.system, args.architecture, model)
    except ValueError as error:
        parser.error(str(error))
    frozen_by_name = {item.harness: item for item in frozen}
    unresolved_names = {item.harness for item in unresolved}

    args.output.mkdir(mode=0o700, parents=True, exist_ok=True)
    failures = []
    base_env = os.environ.copy()
    deterministic_env = dict(base_env)
    deterministic_env.pop("NAN_API_KEY", None)
    for harness in harnesses:
        cell_directory = args.directory / harness
        report = args.output / f"{args.system}-{args.architecture}-{harness}.json"
        base_command = [sys.executable, str(args.cell_script),
                   "--harness", harness, "--trigger", args.trigger, "--tag", args.tag,
                   "--model", model, "--binary", str(args.binary), "--canary", str(args.canary),
                   "--directory", str(cell_directory), "--output", str(report), "--run-id", args.run_id,
                   "--system", args.system, "--architecture", args.architecture,
                   "--source-kind", args.source_kind, "--source-sha", args.source_sha,
                   "--nan-version", args.nan_version, "--mode", args.mode]
        if harness in unresolved_names:
            # A closed infrastructure report keeps this observation independent
            # of the resolved cells without claiming any harness version.
            stages = ["unresolved"]
        else:
            frozen_item = frozen_by_name[harness]
            base_command += ["--harness-version", frozen_item.version]
            if frozen_item.ref:
                base_command += ["--harness-ref", frozen_item.ref]
            stages = ["install", "conformance"] + (["live"] if args.mode == "live" else []) + ["report"]
        for stage in stages:
            command = base_command[:]
            command.insert(2, stage)
            stage_env = base_env if stage == "live" else deterministic_env
            completed = subprocess.run(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                       env=stage_env, check=False)
            if completed.returncode == 3:
                print("Native suite aborted because process cleanup is unproven.", file=sys.stderr)
                return 3
            if completed.returncode:
                failures.append(harness)
                break
    if failures:
        print("One or more hosted CLI cells failed; sanitized reports were retained.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
