#!/usr/bin/env python3
"""Stage one closed ChatGPT startup diagnostic envelope.

The runner's instrumented output is a closed four-field diagnostic whose
``observation`` is the ordinary public report.  This helper validates that
embedded observation in a private temporary snapshot, validates the separate
wrapper facts, and then writes diagnostic evidence only.  The diagnostic
envelope is never passed to ``validate-report``.
"""

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile


MAX_DIAGNOSTIC = 8 << 20
MAX_FACTS = 256 << 10
MAX_ENVELOPE = 8 << 20
MAX_BINARY = 128 << 20


def refuse(message):
    raise ValueError(message)


def no_symlink(path):
    # The runner's conventional root may itself be a platform alias (for
    # example /Users on macOS); reject the input object, while allowing such
    # an operating-system parent alias.
    if path.is_symlink():
        refuse("symlink refused")


def regular(path, limit):
    no_symlink(path)
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
        with os.fdopen(descriptor, "rb") as stream:
            if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
                refuse("non-regular input")
            data = stream.read(limit + 1)
    except OSError as error:
        refuse("input unreadable")
    if len(data) > limit:
        refuse("input too large")
    return data


def load_json(data):
    try:
        return json.loads(data.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        refuse("input is not JSON")


def reducer_module(path):
    spec = importlib.util.spec_from_file_location("wave12_reducer", path)
    if spec is None or spec.loader is None:
        refuse("reducer unavailable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def digest_bytes(data):
    return hashlib.sha256(data).hexdigest()


def file_bytes(path, limit):
    return regular(path, limit)


def validate_observation(checker, observation):
    report_bytes = (json.dumps(observation, separators=(",", ":"),
                               sort_keys=True) + "\n").encode()
    digest = digest_bytes(report_bytes)
    with tempfile.TemporaryDirectory(prefix=".wave12-report-") as directory:
        snapshot = Path(directory) / "report.json"
        snapshot.write_bytes(report_bytes)
        os.chmod(snapshot, 0o600)
        result = subprocess.run(
            [str(checker), "validate-report", str(snapshot)],
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, timeout=30, check=False)
        if result.returncode != 0 or result.stdout.strip() != digest.encode():
            refuse("checker rejected observation")


def validate_diagnostic(diagnostic, checker, wrapper_digest):
    if not isinstance(diagnostic, dict) or set(diagnostic) != {
            "diagnosticVersion", "kind", "wrapperSha256", "observation"}:
        refuse("diagnostic keys are not closed")
    if diagnostic["diagnosticVersion"] != 1 or diagnostic["kind"] != "chatgpt-startup-wrapper":
        refuse("diagnostic identity is invalid")
    if diagnostic["wrapperSha256"] != wrapper_digest:
        refuse("diagnostic wrapper digest mismatch")
    if not isinstance(diagnostic["observation"], dict):
        refuse("observation is not an object")
    validate_observation(checker, diagnostic["observation"])


def validate_facts(facts, reducer, identities):
    if reducer.validate_facts(facts) is not None:
        refuse("observation facts are invalid")
    identity = facts.get("identity")
    if not isinstance(identity, dict) or identity != identities:
        refuse("observation identities do not match inputs")


def validate_envelope(envelope, checker, wrapper_digest):
    if not isinstance(envelope, dict) or set(envelope) != {
            "diagnosticVersion", "kind", "wrapperSha256", "observation"}:
        refuse("envelope keys are not closed")
    if envelope["diagnosticVersion"] != 1 or envelope["kind"] != "chatgpt-startup-wrapper":
        refuse("envelope identity is invalid")
    if envelope["wrapperSha256"] != wrapper_digest:
        refuse("wrapper digest is invalid")
    validate_observation(checker, envelope["observation"])


def stage(diagnostic, facts, wrapper, reducer, checker, nanh, destination):
    diagnostic = Path(os.path.abspath(diagnostic))
    facts = Path(os.path.abspath(facts))
    wrapper = Path(os.path.abspath(wrapper))
    reducer = Path(os.path.abspath(reducer))
    checker = Path(os.path.abspath(checker))
    nanh = Path(os.path.abspath(nanh))
    destination = Path(os.path.abspath(destination))
    for path in (diagnostic, facts, wrapper, reducer, checker, nanh):
        no_symlink(path)
    if destination.exists() or not destination.parent.is_dir():
        refuse("destination must be new")
    if not wrapper.is_file() or not reducer.is_file() or not nanh.is_file():
        refuse("wrapper, reducer or nanh is missing")
    wrapper_digest = digest_bytes(file_bytes(wrapper, MAX_BINARY))
    reducer_digest = digest_bytes(file_bytes(reducer, MAX_BINARY))
    nanh_digest = digest_bytes(file_bytes(nanh, MAX_BINARY))
    diagnostic_bytes = regular(diagnostic, MAX_DIAGNOSTIC)
    facts_bytes = regular(facts, MAX_FACTS)
    diagnostic_object = load_json(diagnostic_bytes)
    facts_object = load_json(facts_bytes)
    reducer_object = reducer_module(str(reducer))
    identities = {"realNanhSha256": nanh_digest, "shimSha256": wrapper_digest,
                  "reducerSha256": reducer_digest}
    validate_diagnostic(diagnostic_object, checker, wrapper_digest)
    validate_facts(facts_object, reducer_object, identities)
    validate_envelope(diagnostic_object, checker, wrapper_digest)
    payload = (json.dumps(diagnostic_object, separators=(",", ":"),
                          sort_keys=True) + "\n").encode()
    if len(payload) > MAX_ENVELOPE:
        refuse("envelope too large")
    if not destination.parent.is_dir() or destination.parent.is_symlink():
        refuse("destination directory is missing")
    if stat.S_IMODE(destination.parent.stat().st_mode) != 0o700:
        refuse("destination directory is not private")
    with tempfile.NamedTemporaryFile(mode="wb", dir=destination.parent,
                                     prefix=".envelope-", delete=False) as handle:
        temporary = Path(handle.name)
        os.chmod(temporary, 0o600)
        handle.write(payload)
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, destination)
    os.chmod(destination, 0o600)


def main(argv):
    if len(argv) != 8:
        return 78
    try:
        stage(*argv[1:])
    except (OSError, ValueError, subprocess.SubprocessError):
        return 78
    return 0


if __name__ == "__main__":
    os.umask(0o077)
    sys.exit(main(sys.argv))
