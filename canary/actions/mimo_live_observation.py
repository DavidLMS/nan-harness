#!/usr/bin/env python3
"""Project bounded MiMo captures to closed facts before ephemeral cleanup."""
import json
import os
from pathlib import Path
import sys

MAX_BYTES = 1048576
MAX_EVENTS = 4096
STAGES = frozenset(("setup", "harness-run", "tool-evidence", "read-marker", "completion-marker",
                    "bridge-sentinel", "usage-evidence", "usage-summary", "complete"))
COUNTS = frozenset(("toolUse", "read", "text", "error", "other"))


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError("duplicate JSON field")
        value[key] = item
    return value


def bounded_read(path):
    if path.is_symlink() or not path.is_file():
        raise OSError("capture unavailable")
    with path.open("rb") as source:
        raw = source.read(MAX_BYTES + 1)
    if len(raw) > MAX_BYTES:
        raise OverflowError("capture exceeds bound")
    return raw


def stream(path, marker):
    result = dict(status="unavailable", events=None, readMarker=None, completionMarker=None, bridgeSentinel=None)
    try:
        raw = bounded_read(path)
        result.update(readMarker=marker in raw, completionMarker=b"NAN_CANARY_OK" in raw,
                      bridgeSentinel=b"NH-BRIDGE-" in raw)
        if not raw.strip():
            result.update(status="empty", events={key: 0 for key in sorted(COUNTS)})
            return result
        lines = raw.splitlines()
        if len(lines) > MAX_EVENTS:
            raise OverflowError("event count exceeds bound")
        counts = {key: 0 for key in sorted(COUNTS)}
        for line in lines:
            if not line.strip():
                continue
            value = json.loads(line, object_pairs_hook=unique_object)
            if type(value) is not dict:
                raise ValueError("event is not an object")
            kind = value.get("type")
            if kind == "tool_use":
                counts["toolUse"] += 1
                part = value.get("part")
                if type(part) is dict and part.get("tool") == "read":
                    counts["read"] += 1
            elif kind in ("text", "error"):
                counts[kind] += 1
            else:
                counts["other"] += 1
        result.update(status="valid", events=counts)
    except OverflowError:
        result.update(status="too-large", events=None)
    except (ValueError, UnicodeError, RecursionError):
        result.update(status="invalid", events=None)
    except OSError:
        pass
    return result


def usage(path):
    if not path.exists():
        return "missing"
    try:
        value = json.loads(bounded_read(path), object_pairs_hook=unique_object)
        if type(value) is not dict or type(value.get("schemaVersion")) is not int or value["schemaVersion"] != 1:
            return "invalid"
        return "observed" if value.get("status") == "observed" else "other"
    except (OSError, ValueError, OverflowError, RecursionError):
        return "invalid"


def validate(value):
    if (type(value) is not dict or set(value) != {"schemaVersion", "stage", "stdout", "stderr", "usage"}
            or type(value["schemaVersion"]) is not int or value["schemaVersion"] != 1
            or type(value["stage"]) is not str or value["stage"] not in STAGES
            or type(value["usage"]) is not str or value["usage"] not in ("observed", "missing", "invalid", "other")):
        raise ValueError("invalid closed observation")
    for name in ("stdout", "stderr"):
        item = value[name]
        if (type(item) is not dict or set(item) != {"status", "events", "readMarker", "completionMarker", "bridgeSentinel"}
                or type(item["status"]) is not str or item["status"] not in ("empty", "valid", "invalid", "too-large", "unavailable")
                or any(item[key] is not None and type(item[key]) is not bool
                       for key in ("readMarker", "completionMarker", "bridgeSentinel"))):
            raise ValueError("invalid closed stream")
        counts = item["events"]
        if item["status"] in ("empty", "valid"):
            if (type(counts) is not dict or set(counts) != COUNTS
                    or any(type(count) is not int or not 0 <= count <= MAX_EVENTS for count in counts.values())
                    or counts["read"] > counts["toolUse"]
                    or sum(counts.values()) - counts["read"] > MAX_EVENTS
                    or item["status"] == "empty" and any(counts.values())
                    or any(type(item[key]) is not bool for key in ("readMarker", "completionMarker", "bridgeSentinel"))):
                raise ValueError("invalid closed event counts")
        elif item["status"] == "unavailable" and any(item[key] is not None for key in ("readMarker", "completionMarker", "bridgeSentinel")):
            raise ValueError("unproven marker predicates")
        elif counts is not None:
            raise ValueError("unproven closed event counts")
    return value


def read_observation(path):
    try:
        raw = bounded_read(path)
        if len(raw) > 4096:
            return None
        return validate(json.loads(raw, object_pairs_hook=unique_object))
    except (OSError, ValueError, OverflowError, RecursionError):
        return None


def main():
    if len(sys.argv) != 7 or sys.argv[5] not in STAGES:
        return 1
    output, error, evidence = (Path(item) for item in sys.argv[1:4])
    marker = sys.argv[4].encode("ascii")
    value = validate(dict(schemaVersion=1, stage=sys.argv[5], stdout=stream(output, marker),
                          stderr=stream(error, marker), usage=usage(evidence)))
    # The controller chooses a fresh destination in its private cell directory.
    descriptor = os.open(sys.argv[6], os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as target:
        json.dump(value, target, sort_keys=True)
        target.write("\n")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, OverflowError, RecursionError):
        raise SystemExit(1) from None
