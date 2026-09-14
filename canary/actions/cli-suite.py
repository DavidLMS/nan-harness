#!/usr/bin/env python3
"""Run the selected CLI harnesses sequentially in isolated native cells."""

import argparse
from dataclasses import asdict, dataclass
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib
from urllib.request import HTTPRedirectHandler, Request, build_opener
from urllib.parse import urlsplit

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
    diagnostic: dict | None = None

    def as_dict(self):
        result = asdict(self)
        if self.diagnostic is None:
            result.pop("diagnostic")
        return result


_NPM_PACKAGES = {
    "claude-code": "@anthropic-ai/claude-code", "codex": "@openai/codex",
    "opencode": "opencode-ai", "pi": "@earendil-works/pi-coding-agent",
    "deepseek-harness": "@deepseek-ai/dsh", "openclaw": "openclaw",
    "cline": "cline", "qwen-code": "@qwen-code/qwen-code",
}
_PYPI_PACKAGES = {"aider": "aider-chat"}
_WINDOWS_PYPI_PACKAGES = {"kimi-code": "kimi-cli"}
_GITHUB_REPOS = {
    "omp": "can1357/oh-my-pi", "goose": "aaif-goose/goose",
    "hermes": "NousResearch/hermes-agent",
}
# Hermes tags releases by date (v2026.9.11) while `hermes --version` reports the
# pyproject version (0.21.2). Freeze the tag's commit and read the version there.
_COMMIT_PINNED = frozenset({"hermes"})
FX_SOURCE = "https://releases.fx.sh/latest.txt"
_TEXT_SOURCES = {
    "fx": FX_SOURCE,
    # Unix install tooling resolves Kimi through this stable channel. Windows
    # uses the PyPI package because its installer consumes a pip distribution.
    "kimi-code": "https://cdn.kimi.com/kimi-code/latest",
    # Official install.sh resolves this stable channel, not GitHub's latest tag.
    "prime-agent": "https://pub-728493de92a943e2a9b2d17b4719f318.r2.dev/stable",
}
_COMMIT = re.compile(r"[0-9a-f]{40}\Z")
_MANIFEST_METADATA_ERRORS = (OSError, KeyError, TypeError, ValueError, UnicodeError)
_RESOLUTION_CATEGORIES = frozenset({
    "timeout", "dns", "tls", "http", "invalid-json", "missing-tag",
    "invalid-version", "unknown",
})
_GITHUB_API_ORIGIN = ("https", "api.github.com", 443)


class _NoRedirect(HTTPRedirectHandler):
    """Refuse redirects so an API credential cannot cross origins."""

    def redirect_request(self, request, file, code, msg, headers, new_url):
        return None


class _MissingTag(ValueError):
    """The official release document omitted its required tag field."""


class _InvalidVersion(ValueError):
    """The official metadata contained a non-semver version value."""


def _resolution_diagnostic(error):
    """Map resolver failures to closed facts without retaining exception text."""
    if isinstance(error, _MissingTag):
        category = "missing-tag"
    elif isinstance(error, _InvalidVersion):
        category = "invalid-version"
    elif isinstance(error, json.JSONDecodeError):
        category = "invalid-json"
    else:
        category = "unknown"
        try:
            import socket
            import ssl
            from urllib.error import HTTPError, URLError
            if isinstance(error, HTTPError):
                status = error.code
                if isinstance(status, int) and 100 <= status <= 599:
                    return {"category": "http", "httpStatus": status}
                return {"category": "http"}
            if isinstance(error, (socket.timeout, TimeoutError)):
                category = "timeout"
            elif isinstance(error, ssl.SSLError):
                category = "tls"
            elif isinstance(error, socket.gaierror):
                category = "dns"
            elif isinstance(error, URLError):
                reason = error.reason
                if isinstance(reason, (socket.timeout, TimeoutError)):
                    category = "timeout"
                elif isinstance(reason, ssl.SSLError):
                    category = "tls"
                elif isinstance(reason, socket.gaierror):
                    category = "dns"
        except (ImportError, AttributeError, TypeError):
            category = "unknown"
    return {"category": category}


def _validate_resolution_diagnostic(value):
    """Validate the optional manifest discriminator and discard no safe facts."""
    if value is None:
        return
    if not isinstance(value, dict) or set(value) - {"category", "httpStatus"}:
        raise ValueError("invalid resolver diagnostic")
    category = value.get("category")
    if category not in _RESOLUTION_CATEGORIES:
        raise ValueError("invalid resolver diagnostic category")
    status = value.get("httpStatus")
    if status is not None and (not isinstance(status, int) or isinstance(status, bool)
                               or not 100 <= status <= 599 or category != "http"):
        raise ValueError("invalid resolver diagnostic status")


def _official_json(url, timeout=20):
    parsed = urlsplit(url)
    headers = {"Accept": "application/json", "User-Agent": "nan-harness-cli-gate"}
    port = 443 if parsed.port is None else parsed.port
    if ((parsed.scheme, parsed.hostname, port) == _GITHUB_API_ORIGIN
            and parsed.username is None and parsed.password is None):
        token = os.environ.get("GITHUB_TOKEN", "")
        if token:
            headers["Authorization"] = "Bearer " + token
    request = Request(url, headers=headers)
    opener = build_opener(_NoRedirect)
    with opener.open(request, timeout=timeout) as response:
        raw = response.read(2_000_001)
        if len(raw) > 2_000_000:
            raise ValueError("official version metadata exceeds its size limit")
        return json.loads(raw)


def _official_text(url, limit=256, timeout=20):
    request = Request(url, headers={"User-Agent": "nan-harness-cli-gate"})
    with build_opener(_NoRedirect).open(request, timeout=timeout) as response:
        raw = response.read(limit + 1)
    if len(raw) > limit:
        raise ValueError("official version marker exceeds its size limit")
    return raw.decode("ascii" if limit <= 256 else "utf-8").strip()


def _version(value):
    if not isinstance(value, str):
        raise _InvalidVersion("official metadata requires a version string")
    if value.startswith("v"):
        value = value[1:]
    if not SEMVER.fullmatch(value):
        raise _InvalidVersion("official metadata did not contain a semantic version")
    return value


def _pypi_version(document):
    """Resolve a PyPI JSON document without leaking shape errors as unknown."""
    try:
        value = document["info"]["version"]
    except (KeyError, TypeError):
        raise _InvalidVersion("official PyPI metadata omitted its version") from None
    return _version(value)


def _source(harness, system=""):
    """Closed source and package identity for a known CLI harness."""
    if harness in _NPM_PACKAGES:
        return "npm:" + _NPM_PACKAGES[harness], _NPM_PACKAGES[harness]
    packages = _WINDOWS_PYPI_PACKAGES if system == "windows" else _PYPI_PACKAGES
    if harness in packages:
        return "pypi:" + packages[harness], packages[harness]
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
    source, package = _source(harness, system)
    ref = ""
    if harness in _NPM_PACKAGES:
        version = _version(fetch_json("https://registry.npmjs.org/" + package + "/latest")["version"])
    elif source.startswith("pypi:"):
        version = _pypi_version(fetch_json("https://pypi.org/pypi/" + package + "/json"))
    elif harness in _GITHUB_REPOS:
        repo = _GITHUB_REPOS[harness]
        release = fetch_json("https://api.github.com/repos/" + repo + "/releases/latest")
        try:
            tag = release["tag_name"]
        except (KeyError, TypeError):
            raise _MissingTag("official release metadata omitted its tag") from None
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
                     fetch_text=_official_text, fetch_document=None, timeout=20):
    """Resolve each official source independently.

    One unavailable upstream yields an ``UnresolvedHarness`` with no version; the
    others keep their frozen identities. Fetchers are injectable so tests stay offline.
    """
    if fetch_json is _official_json:
        fetch_json = lambda url: _official_json(url, timeout)
    if fetch_text is _official_text:
        fetch_text = lambda url: _official_text(url, timeout=timeout)
    fetch_document = fetch_document or (lambda url: _official_text(url, 200_000, timeout))
    for harness in harnesses:
        _source(harness, system)
    resolved, unresolved = [], []
    for harness in harnesses:
        try:
            resolved.append(_resolve_one(harness, system, architecture, model,
                                         fetch_json, fetch_text, fetch_document))
        except _MANIFEST_METADATA_ERRORS + (tomllib.TOMLDecodeError,) as error:
            source, package = _source(harness, system)
            diagnostic = _resolution_diagnostic(error)
            unresolved.append(UnresolvedHarness(harness, system, architecture, source, package,
                                                model, diagnostic))
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
        unresolved = []
        for entry in document.get("unresolved", []):
            if not isinstance(entry, dict):
                raise ValueError("invalid unresolved manifest entry")
            diagnostic = entry.get("diagnostic")
            _validate_resolution_diagnostic(diagnostic)
            unresolved.append(UnresolvedHarness(**entry))
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
        if (item.source, item.package) != _source(item.harness, system):
            raise ValueError("frozen manifest has an untrusted installer source")
    for item in resolved:
        if _version(item.version) != item.version:
            raise ValueError("frozen manifest has a noncanonical version")
        if not isinstance(item.ref, str) or not (
                _COMMIT.fullmatch(item.ref) if item.harness in _COMMIT_PINNED else item.ref == ""):
            raise ValueError("frozen manifest has an untrusted installer ref")
    return resolved, unresolved


def _annotate_resolution_report(path, harness, diagnostic):
    """Add only the closed resolver code to the already validated cell report."""
    _validate_resolution_diagnostic(diagnostic)
    if diagnostic is None:
        return True
    temporary = None
    try:
        state = json.loads(path.read_bytes())
        failure = state.get("failure")
        identity = state.get("harness")
        environment = state.get("environment")
        if (not isinstance(failure, dict) or not isinstance(identity, dict)
                or not isinstance(environment, dict)):
            raise ValueError("resolver report is incomplete")
        required = (
            (state, "outcome"), (state, "tier"), (state, "scenario"),
            (identity, "id"), (identity, "version"),
            (environment, "operatingSystem"), (environment, "architecture"),
            (failure, "class"), (failure, "phase"), (failure, "fingerprint"),
        )
        if any(not isinstance(container.get(key), str) or not container[key].strip()
               for container, key in required):
            raise ValueError("resolver report is incomplete")
        if (state["outcome"] != "infrastructure-failure"
                or failure["class"] != "infrastructure"
                or failure["phase"] != "resolve-official-version"):
            raise ValueError("resolver report identity differs from manifest")
        code = "resolve-" + diagnostic["category"]
        if diagnostic.get("httpStatus") is not None:
            code += "-" + str(diagnostic["httpStatus"])
        if identity["id"] != harness:
            raise ValueError("resolver report identity differs from manifest")
        fingerprint_source = "|".join((
            identity["id"], identity["version"], environment["operatingSystem"],
            environment["architecture"], state["tier"], state["scenario"],
            "Infrastructure", failure["phase"], code,
        ))
        fingerprint = hashlib.sha256(fingerprint_source.encode()).hexdigest()
        failure["code"] = code
        failure["fingerprint"] = fingerprint
        temporary = path.with_name("." + path.name + ".resolver")
        temporary.write_text(json.dumps(state, sort_keys=True) + "\n")
        os.chmod(temporary, 0o600)
        os.replace(temporary, path)
        return True
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError):
        if temporary is not None:
            try:
                temporary.unlink(missing_ok=True)
            except OSError:
                pass
        print("Resolver diagnostic could not be recorded; generic failure was retained.",
              file=sys.stderr)
        return False


def read_frozen_manifest(path, harnesses, system, architecture, model):
    """Resolved entries, in request order, from a manifest that covers ``harnesses`` exactly."""
    return _load_manifest(path, harnesses, system, architecture, model)[0]


def read_unresolved_manifest(path, harnesses, system, architecture, model):
    """Unresolved entries from the same validated manifest; they carry no version."""
    return _load_manifest(path, harnesses, system, architecture, model)[1]


def resolve_main(argv):
    parser = argparse.ArgumentParser(description="Resolve official CLI versions")
    parser.add_argument("--harnesses", required=True)
    parser.add_argument("--system", required=True, choices=("linux", "macos"))
    parser.add_argument("--architecture", required=True, choices=("aarch64",))
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
    deterministic_env.pop("GITHUB_TOKEN", None)
    base_env.pop("GITHUB_TOKEN", None)
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
                if harness in unresolved_names:
                    _annotate_resolution_report(report, harness,
                                                next(item.diagnostic for item in unresolved
                                                     if item.harness == harness))
                failures.append(harness)
                break
    if failures:
        print("One or more hosted CLI cells failed; sanitized reports were retained.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
