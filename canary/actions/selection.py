#!/usr/bin/env python3
"""Validate and expand the hosted CLI matrix before runner use."""

import argparse
import json
import os
import re


CLI_HARNESSES = (
    "claude-code", "codex", "opencode", "hermes", "pi", "omp", "prime-agent",
    "deepseek-harness", "openclaw", "cline", "qwen-code", "kimi-code", "aider",
    "goose", "fx",
)
# One entry per hosted platform: the runner label that provides it, the architecture
# a cell must prove on that runner, and the Rust target whose release asset carries it.
PLATFORMS = {
    "linux": {"runner": "ubuntu-24.04-arm", "architecture": "aarch64",
              "target": "aarch64-unknown-linux-musl"},
    "macos": {"runner": "macos-14", "architecture": "aarch64",
              "target": "aarch64-apple-darwin"},
    "windows": {"runner": "windows-2025", "architecture": "x86_64",
                "target": "x86_64-pc-windows-msvc"},
}
SYSTEMS = tuple(PLATFORMS)
# Explicit maintainer policy until official native Windows distributions exist.
# This is availability, not qualification: other Windows harnesses still need live evidence.
WINDOWS_UNAVAILABLE = frozenset(("prime-agent", "fx"))
WINDOWS_SKIP_REASON = "official-windows-distribution-unavailable"
DEFAULT_MODEL = "qwen3.6"
MODEL_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}\Z")

# Platforms whose evidence a harness must supply before its compatibility feed may
# advance. The published feed keeps one platform-independent record per harness, so
# a harness is qualified only when every platform listed here passes. Windows joins
# a harness's list once its native cell passes deterministic and live qualification.
_BASE_PLATFORMS = ("linux", "macos")
HARNESS_PLATFORMS = {harness: _BASE_PLATFORMS for harness in CLI_HARNESSES}


def supported_platforms(harness):
    """Platforms a harness must satisfy before the feed may advance it."""
    if harness not in HARNESS_PLATFORMS:
        raise ValueError("unknown CLI harness: " + harness)
    return HARNESS_PLATFORMS[harness]


def platform(architecture_system):
    """The hosted platform table entry for a canonical system name."""
    if architecture_system not in PLATFORMS:
        raise ValueError("unknown hosted platform: " + str(architecture_system))
    return PLATFORMS[architecture_system]


def identity(system, architecture):
    """Validate a platform/architecture pair and return its canonical table entry."""
    entry = platform(system)
    if architecture != entry["architecture"]:
        raise ValueError(
            "architecture must be " + entry["architecture"] + " on " + system)
    return entry


# Release assets one cell needs: the harness binary it qualifies plus the canary binary
# that produces the evidence. A ``None`` canary records that the platform publishes no
# canary asset yet, so qualifying a harness there fails closed until it exists.
PLATFORM_ASSETS = {
    "linux": {"harness": "nan-harness-aarch64-unknown-linux-musl",
              "canary": "nan-harness-canary-aarch64-unknown-linux-musl"},
    "macos": {"harness": "nan-harness-aarch64-apple-darwin",
              "canary": "nan-harness-canary-aarch64-apple-darwin"},
    "windows": {"harness": "nan-harness-x86_64-pc-windows-msvc.exe",
                "canary": "nan-harness-canary-x86_64-pc-windows-msvc.exe"},
}


def qualified_platforms():
    """Platforms the compatibility feed requires, from every harness support list."""
    return tuple(sorted({system for harness in CLI_HARNESSES
                         for system in supported_platforms(harness)}))


def required_assets():
    """Every release asset the qualified platforms need before qualification runs."""
    assets = []
    for system in qualified_platforms():
        entry = PLATFORM_ASSETS[system]
        assets.append(entry["harness"])
        if entry["canary"] is not None:
            assets.append(entry["canary"])
    return tuple(assets)


def qualified_identities():
    """Canonical ``platform/harness`` identities the feed requires, one per cell."""
    return {f"{system}/{harness}" for harness in CLI_HARNESSES
            for system in supported_platforms(harness)}


def _names(value, known, label):
    values = [part.strip() for part in value.split(",")]
    if values == ["all"]:
        return list(known)
    if (not values or any(not value or value not in known for value in values)
            or len(values) != len(set(values))):
        raise ValueError(f"{label} must be all or distinct known identifiers")
    return [name for name in known if name in values]


def resolve_model(requested="", configured=None):
    """Resolve a bounded model identifier without accepting shell syntax."""
    if configured is None:
        configured = os.environ.get("CANARY_MODEL", "")
    model = requested or configured or DEFAULT_MODEL
    if not isinstance(model, str) or not MODEL_ID.fullmatch(model):
        raise ValueError("model must be a bounded model identifier")
    return model


def select_cli(platforms="all", harnesses="all", mode="deterministic", model=""):
    """Return one independent cell per selected platform and harness.

    An explicit dispatch may select any harness on any hosted platform, including a
    harness that is not yet qualified there: the cell reports what it finds and the
    feed keeps requiring only `supported_platforms`.
    """
    if mode not in ("deterministic", "live"):
        raise ValueError("mode must be deterministic or live")
    systems = _names(platforms.replace("both", "linux,macos") if platforms == "both" else platforms,
                     SYSTEMS, "platforms")
    selected = _names(harnesses, CLI_HARNESSES, "harnesses")
    cells = []
    skipped = []
    for system in systems:
        entry = PLATFORMS[system]
        for harness in selected:
            if system == "windows" and harness in WINDOWS_UNAVAILABLE:
                skipped.append({"system": system, "harness": harness,
                                "status": "skipped", "reason": WINDOWS_SKIP_REASON})
                continue
            cells.append({"system": system, "runner": entry["runner"],
                          "architecture": entry["architecture"], "target": entry["target"],
                          "harness": harness, "mode": mode})
    return {"mode": mode, "model": resolve_model(model), "cells": cells, "skipped": skipped}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platforms", default="all")
    parser.add_argument("--harnesses", default="all")
    parser.add_argument("--mode", default="deterministic")
    parser.add_argument("--model", default="")
    args = parser.parse_args()
    try:
        result = select_cli(args.platforms, args.harnesses, args.mode, args.model)
    except ValueError as error:
        parser.error(str(error))
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
