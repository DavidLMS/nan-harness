#!/usr/bin/env python3
"""Durable immutable publication requests and phase receipts in a data-only branch.

Git reference updates use compare-and-swap (non-force fast-forward), retrying
concurrent enqueues without losing requests. Workflow concurrency is a writer
mutex, not the queue: any later drain sees every committed unfinished request.
"""

import base64
import hashlib
import json
import re
import subprocess
import time

BRANCH = "compatibility-state"
HEX = re.compile(r"[0-9a-f]{64}\Z")


class StateError(Exception):
    pass


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode() + b"\n"


def api(repository, endpoint, payload=None, method=None):
    command = ["gh", "api", f"repos/{repository}/{endpoint}"]
    if payload is not None:
        command += ["--method", method or "POST", "--input", "-"]
    result = subprocess.run(command, input=canonical(payload) if payload is not None else None,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=120)
    if result.returncode:
        raise StateError("GitHub state operation failed")
    return json.loads(result.stdout)


class Store:
    def __init__(self, repository):
        if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
            raise StateError("invalid repository")
        self.repository = repository

    def call(self, endpoint, payload=None, method=None):
        return api(self.repository, endpoint, payload, method)

    def head(self):
        return self.call(f"git/ref/heads/{BRANCH}")["object"]["sha"]

    def tree_sha(self, commit):
        return self.call(f"git/commits/{commit}")["tree"]["sha"]

    def entries(self, head):
        result = self.call(f"git/trees/{self.tree_sha(head)}?recursive=1")
        if result.get("truncated"):
            raise StateError("state tree exceeds API limit")
        return {item["path"]: item["sha"] for item in result["tree"] if item["type"] == "blob"}

    def read_blob(self, sha):
        item = self.call(f"git/blobs/{sha}")
        if item["encoding"] != "base64" or item["size"] > 2_000_000:
            raise StateError("invalid state object")
        return base64.b64decode(item["content"], validate=False)

    def get(self, path):
        entries = self.entries(self.head())
        return self.read_blob(entries[path]) if path in entries else None

    def put(self, path, payload, immutable=False):
        if not re.fullmatch(r"(?:requests|completed|receipts|recommendations)/[0-9a-f]{64}\.json", path):
            raise StateError("invalid state path")
        blob = self.call("git/blobs", {"content": base64.b64encode(payload).decode(), "encoding": "base64"})["sha"]
        for attempt in range(8):
            parent = self.head()
            entries = self.entries(parent)
            if path in entries:
                current = self.read_blob(entries[path])
                if current == payload:
                    return
                if immutable:
                    raise StateError("immutable request changed")
            tree = self.call("git/trees", {"base_tree": self.tree_sha(parent), "tree": [
                {"path": path, "mode": "100644", "type": "blob", "sha": blob}]})["sha"]
            commit = self.call("git/commits", {"message": "chore(compatibility): persist publication state",
                                              "tree": tree, "parents": [parent]})["sha"]
            try:
                self.call(f"git/refs/heads/{BRANCH}", {"sha": commit, "force": False}, "PATCH")
                return
            except StateError:
                if self.head() == parent:
                    raise
                time.sleep(min(attempt + 1, 5))
        raise StateError("state changed too frequently; retry the operation")

    def initialize(self):
        # Explicit operator operation only. A missing or unreadable branch in normal
        # operation must never be interpreted as empty publication history.
        tree = self.call("git/trees", {"tree": [{"path": "README.md", "mode": "100644",
            "type": "blob", "content": "# Compatibility publication state\n\nData only; never execute this branch.\n"}]})["sha"]
        commit = self.call("git/commits", {"message": "chore(compatibility): initialize durable state",
                                          "tree": tree, "parents": []})["sha"]
        self.call("git/refs", {"ref": f"refs/heads/{BRANCH}", "sha": commit})

    def enqueue(self, request):
        payload = canonical(request)
        identity = hashlib.sha256(payload).hexdigest()
        self.put(f"requests/{identity}.json", payload, immutable=True)
        return identity

    def pending(self):
        entries = self.entries(self.head())
        for path in sorted(entries):
            if path.startswith("requests/") and path.endswith(".json"):
                identity = path.removeprefix("requests/").removesuffix(".json")
                if not HEX.fullmatch(identity):
                    raise StateError("invalid request identity")
                if f"completed/{identity}.json" not in entries:
                    raw = self.read_blob(entries[path])
                    if hashlib.sha256(raw).hexdigest() != identity:
                        raise StateError("request digest mismatch")
                    yield identity, json.loads(raw)


def receipt_identity(repository, tag):
    return hashlib.sha256(f"{repository}:{tag}".encode()).hexdigest()
