#!/usr/bin/env python3
"""Run the selected CLI harnesses sequentially in isolated native cells."""

import argparse
from dataclasses import asdict, dataclass
import json
import os
from pathlib import Path
import subprocess
import sys
from urllib.request import Request, urlopen

sys.path.insert(0, str(Path(__file__).resolve().parent))
from selection import CLI_HARNESSES, resolve_model
from cell import SEMVER, source_identity


@dataclass(frozen=True)
class FrozenHarness:
    """The immutable installer identity passed from resolution to a cell."""

    harness: str
    version: str
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
FX_SOURCE = "https://releases.fx.sh/latest.txt"
_TEXT_SOURCES = {
    "fx": FX_SOURCE,
    "kimi-code": "https://code.kimi.com/kimi-code/latest",
    # Official install.sh resolves this stable channel, not GitHub's latest tag.
    "prime-agent": "https://pub-728493de92a943e2a9b2d17b4719f318.r2.dev/stable",
}


def _official_json(url):
    request = Request(url, headers={"Accept": "application/json", "User-Agent": "nan-harness-cli-gate"})
    with urlopen(request, timeout=20) as response:
        raw = response.read(2_000_001)
        if len(raw) > 2_000_000:
            raise ValueError("official version metadata exceeds its size limit")
        return json.loads(raw)


def _official_text(url):
    with urlopen(Request(url, headers={"User-Agent": "nan-harness-cli-gate"}), timeout=20) as response:
        raw = response.read(257)
    if len(raw) > 256:
        raise ValueError("official version marker exceeds its size limit")
    return raw.decode("ascii").strip()


def _version(value):
    if not isinstance(value, str):
        raise ValueError("official metadata requires a version string")
    if value.startswith("v"):
        value = value[1:]
    if not SEMVER.fullmatch(value):
        raise ValueError("official metadata did not contain a semantic version")
    return value


def resolve_frozen_versions(harnesses, system, architecture, model, fetch_json=_official_json,
                            fetch_text=_official_text):
    """Resolve official latest metadata once and return a serializable frozen manifest.

    ``fetch_json`` is injectable so tests never contact providers.  The resulting
    manifest is the only version input accepted by the installation stage.
    """
    result = []
    for harness in harnesses:
        if harness in _NPM_PACKAGES:
            package = _NPM_PACKAGES[harness]
            metadata = fetch_json("https://registry.npmjs.org/" + package + "/latest")
            version = _version(metadata["version"])
            source = "npm:" + package
        elif harness in _PYPI_PACKAGES:
            package = _PYPI_PACKAGES[harness]
            metadata = fetch_json("https://pypi.org/pypi/" + package + "/json")
            version = _version(metadata["info"]["version"])
            source = "pypi:" + package
        elif harness in _GITHUB_REPOS:
            repo = _GITHUB_REPOS[harness]
            metadata = fetch_json("https://api.github.com/repos/" + repo + "/releases/latest")
            version = _version(metadata["tag_name"])
            package = ""
            source = "github:" + repo
        elif harness in _TEXT_SOURCES:
            source = _TEXT_SOURCES[harness]
            version = _version(fetch_text(source))
            package = ""
        else:
            raise ValueError("unknown CLI harness: " + harness)
        result.append(FrozenHarness(harness, version, system, architecture, source, package, model))
    return result


def read_frozen_manifest(path, harnesses, system, architecture, model):
    try:
        entries = json.loads(Path(path).read_bytes())
        entries = entries["harnesses"]
        if [entry["harness"] for entry in entries] != list(harnesses):
            raise ValueError("frozen manifest harness selection differs from this run")
        frozen = [FrozenHarness(**entry) for entry in entries]
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise ValueError("invalid frozen CLI manifest") from error
    for item in frozen:
        if (item.system, item.architecture, item.model) != (system, architecture, model):
            raise ValueError("frozen manifest platform or model differs from this run")
        _version(item.version)
        sources = {**{name: "npm:" + package for name, package in _NPM_PACKAGES.items()},
                   **{name: "pypi:" + package for name, package in _PYPI_PACKAGES.items()},
                   **{name: "github:" + repo for name, repo in _GITHUB_REPOS.items()}, **_TEXT_SOURCES}
        package = _NPM_PACKAGES.get(item.harness, _PYPI_PACKAGES.get(item.harness, ""))
        if item.source != sources.get(item.harness) or item.package != package or _version(item.version) != item.version:
            raise ValueError("frozen manifest has an untrusted installer source")
    return frozen


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
    frozen = resolve_frozen_versions(harnesses, args.system, args.architecture, model)
    args.output.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    args.output.write_text(json.dumps({"harnesses": [item.as_dict() for item in frozen]}, sort_keys=True))
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
        frozen = read_frozen_manifest(args.manifest, harnesses, args.system, args.architecture, model)
    except ValueError as error:
        parser.error(str(error))

    args.output.mkdir(mode=0o700, parents=True, exist_ok=True)
    failures = []
    base_env = os.environ.copy()
    deterministic_env = dict(base_env)
    deterministic_env.pop("NAN_API_KEY", None)
    for harness, frozen_item in zip(harnesses, frozen):
        cell_directory = args.directory / harness
        report = args.output / f"{args.system}-{args.architecture}-{harness}.json"
        base_command = [sys.executable, str(args.cell_script),
                   "--harness", harness, "--trigger", args.trigger, "--tag", args.tag,
                   "--model", model, "--binary", str(args.binary), "--canary", str(args.canary),
                   "--directory", str(cell_directory), "--output", str(report), "--run-id", args.run_id,
                   "--system", args.system, "--architecture", args.architecture,
                   "--source-kind", args.source_kind, "--source-sha", args.source_sha,
                   "--nan-version", args.nan_version, "--mode", args.mode,
                   "--harness-version", frozen_item.version]
        stages = ["install", "conformance"]
        if args.mode == "live":
            stages.append("live")
        stages.append("report")
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
