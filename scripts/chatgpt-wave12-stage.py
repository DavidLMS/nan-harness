#!/usr/bin/env python3
"""Stage one closed ChatGPT startup diagnostic envelope.

The desktop checker intentionally validates only the public report schema. This
helper validates that report first, validates the private wrapper observation
with the wave12 reducer, then writes a separate diagnostic envelope. The
envelope is never passed to ``validate-report``.
"""

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tempfile


DIGEST = re.compile(r"^[0-9a-f]{64}$")
MAX_REPORT = 8 << 20
MAX_FACTS = 256 << 10
MAX_ENVELOPE = 8 << 20


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


def validate_report(checker, report_path, report_bytes, expected):
    digest = hashlib.sha256(report_bytes).hexdigest()
    if digest != expected:
        refuse("report digest mismatch")
    with tempfile.TemporaryDirectory(prefix=".wave12-report-") as directory:
        snapshot = Path(directory) / "report.json"
        snapshot.write_bytes(report_bytes)
        result = subprocess.run(
            [str(checker), "validate-report", str(snapshot)],
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, timeout=30, check=False)
        if result.returncode != 0 or result.stdout.strip() != digest.encode():
            refuse("checker rejected report")
    return load_json(report_bytes)


def validate_envelope(envelope, reducer):
    if not isinstance(envelope, dict) or set(envelope) != {
            "diagnosticVersion", "kind", "wrapperSha256", "observation", "report"}:
        refuse("envelope keys are not closed")
    if envelope["diagnosticVersion"] != 1 or envelope["kind"] != "chatgpt-startup-wrapper":
        refuse("envelope identity is invalid")
    if not isinstance(envelope["wrapperSha256"], str) or not DIGEST.fullmatch(envelope["wrapperSha256"]):
        refuse("wrapper digest is invalid")
    if reducer.validate_facts(envelope["observation"]) is not None:
        refuse("observation facts are invalid")
    if not isinstance(envelope["report"], dict):
        refuse("report is not an object")


def stage(report, facts, wrapper, reducer, checker, destination):
    report = Path(os.path.abspath(report))
    facts = Path(os.path.abspath(facts))
    wrapper = Path(os.path.abspath(wrapper))
    reducer = Path(os.path.abspath(reducer))
    checker = Path(os.path.abspath(checker))
    destination = Path(os.path.abspath(destination))
    for path in (report, facts, wrapper, reducer, checker):
        no_symlink(path)
    if destination.exists() or not destination.parent.is_dir():
        refuse("destination must be new")
    if not wrapper.is_file() or not reducer.is_file():
        refuse("wrapper or reducer is missing")
    wrapper_digest = hashlib.sha256(regular(wrapper, MAX_REPORT)).hexdigest()
    report_bytes = regular(report, MAX_REPORT)
    facts_bytes = regular(facts, MAX_FACTS)
    report_object = validate_report(checker, report, report_bytes,
                                    hashlib.sha256(report_bytes).hexdigest())
    observation = load_json(facts_bytes)
    if reducer_module(str(reducer)).validate_facts(observation) is not None:
        refuse("observation facts are invalid")
    envelope = {
        "diagnosticVersion": 1,
        "kind": "chatgpt-startup-wrapper",
        "wrapperSha256": wrapper_digest,
        "observation": observation,
        "report": report_object,
    }
    validate_envelope(envelope, reducer_module(str(reducer)))
    payload = (json.dumps(envelope, separators=(",", ":"), sort_keys=True) + "\n").encode()
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
    if len(argv) != 7:
        return 78
    try:
        stage(*argv[1:])
    except (OSError, ValueError, subprocess.SubprocessError):
        return 78
    return 0


if __name__ == "__main__":
    os.umask(0o077)
    sys.exit(main(sys.argv))
