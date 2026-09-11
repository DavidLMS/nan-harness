"""Publish only closed metadata and checker-validated, digest-bound snapshots."""

import hashlib
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tempfile


RULES = {
    "source_commit": r"unknown|[0-9a-f]{40}",
    "condition": r"baseline|dock-hidden",
    "run_url": r"none|https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/actions/runs/[0-9]+",
    "probe_exit": r"not-run|0|[1-9][0-9]{0,2}",
    "report": r"validated|invalid|absent",
    "occlusion": r"validated|invalid|absent",
    "docks_changed": r"0|1",
    "report_sha256": r"[0-9a-f]{64}",
    "occlusion_sha256": r"[0-9a-f]{64}",
    "prior_state": r"none|absent|true|false|unreadable|invalid",
    "restore_status": r"none|ok|failed",
}
REQUIRED = set(RULES) - {"report_sha256", "occlusion_sha256"}
RESTORABLE = {"absent", "true", "false"}


def no_symlinks(path):
    for component in (path, *path.parents):
        if component.is_symlink():
            raise ValueError("symlink refused")


def read_regular(path, limit):
    no_symlinks(path)
    descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    with os.fdopen(descriptor, "rb") as stream:
        if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
            raise ValueError("non-regular artifact")
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise ValueError("artifact too large")
    return data


def metadata(data):
    fields = {}
    for line in data.decode("ascii").splitlines():
        key, separator, value = line.partition("=")
        if not separator or key in fields or key not in RULES:
            raise ValueError("unexpected metadata field")
        if re.fullmatch(RULES[key], value) is None:
            raise ValueError("invalid metadata value")
        fields[key] = value
    if not REQUIRED <= fields.keys():
        raise ValueError("incomplete metadata")
    if fields["probe_exit"] != "not-run" and int(fields["probe_exit"]) > 255:
        raise ValueError("invalid probe exit")
    if not consistent(fields):
        raise ValueError("contradictory metadata")
    return fields


def consistent(fields):
    """Closed facts must agree: publication never hides restoration uncertainty."""
    changed = fields["docks_changed"] == "1"
    prior = fields["prior_state"]
    if fields["condition"] == "baseline":
        state_ok = prior == "none" and not changed
    else:
        # Only a readable, restorable prior value is ever mutated.
        state_ok = prior != "none" and changed == (prior in RESTORABLE)
    # A mutation always has a restoration verdict; no mutation never has one.
    restore_ok = changed == (fields["restore_status"] != "none")
    probe_ok = fields["probe_exit"] != "not-run" or (
        fields["report"] == "absent" and fields["occlusion"] == "absent")
    return state_ok and restore_ok and probe_ok


def validated_report(source, scratch, checker, name, expected):
    # Validate precisely the bytes being published, not a mutable source.
    payload = read_regular(source / (name + ".json"), 8 << 20)
    digest = hashlib.sha256(payload).hexdigest()
    if expected != digest:
        raise ValueError("evidence digest mismatch")
    recorded = read_regular(source / (name + ".sha256"), 65).decode("ascii")
    if recorded not in (digest, digest + "\n"):
        raise ValueError("recorded digest mismatch")
    snapshot = scratch / (name + ".json")
    snapshot.write_bytes(payload)
    # Validator output may be private on failure; never forward either channel.
    result = subprocess.run(
        [str(checker), "validate-" + name, str(snapshot)],
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        timeout=30, check=False,
    )
    answer = result.stdout
    if result.returncode != 0 or answer not in (digest.encode(), (digest + "\n").encode()):
        raise ValueError("checker refused snapshot")
    (scratch / (name + ".sha256")).write_text(digest + "\n", encoding="ascii")


def stage(source, destination, checker):
    source, destination, checker = map(lambda p: Path(os.path.abspath(p)),
                                       (source, destination, checker))
    for path in (source, destination, checker):
        no_symlinks(path)
    # Never reuse a destination containing an earlier run or symlinks.
    if destination.exists() or not destination.parent.is_dir():
        raise ValueError("staging destination must be new")
    with tempfile.TemporaryDirectory(prefix=".wave12-stage-", dir=destination.parent) as temporary:
        scratch = Path(temporary)
        if source.exists():
            fields = metadata(read_regular(source / "evidence.txt", 4096))
            probe = read_regular(source / "probe-status.txt", 32)
            if probe != ("probe_exit=" + fields["probe_exit"] + "\n").encode():
                raise ValueError("probe status mismatch")
            for name in ("report", "occlusion"):
                if fields[name] == "validated":
                    validated_report(source, scratch, checker, name,
                                     fields.get(name + "_sha256"))
                elif name + "_sha256" in fields:
                    raise ValueError("digest without validated artifact")
            (scratch / "evidence.txt").write_text(
                "".join(key + "=" + fields[key] + "\n" for key in RULES if key in fields),
                encoding="ascii")
            (scratch / "probe-status.txt").write_bytes(probe)
        # No source path is part of the public manifest.
        names = sorted(path.name for path in scratch.iterdir())
        (scratch / "staged.txt").write_text("".join(name + "\n" for name in names), encoding="ascii")
        os.rename(scratch, destination)


def main():
    os.umask(0o077)
    try:
        if len(sys.argv) != 4:
            raise ValueError("usage")
        stage(*sys.argv[1:])
    except (OSError, ValueError, subprocess.SubprocessError):
        print("wave12 artifact staging refused", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
