#!/usr/bin/env python3
"""Validate and expand the hosted, ARM64 CLI matrix before runner use."""

import argparse
import json
import os
import re


CLI_HARNESSES = (
    "claude-code", "codex", "opencode", "hermes", "pi", "omp", "prime-agent",
    "deepseek-harness", "openclaw", "cline", "qwen-code", "kimi-code", "aider",
    "goose", "fx",
)
SYSTEMS = ("linux", "macos")
DEFAULT_MODEL = "qwen3.6"
MODEL_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}\Z")


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
    """Return one independent ARM64 cell per selected platform and harness."""
    if mode not in ("deterministic", "live"):
        raise ValueError("mode must be deterministic or live")
    systems = _names(platforms.replace("both", "linux,macos") if platforms == "both" else platforms,
                     SYSTEMS, "platforms")
    selected = _names(harnesses, CLI_HARNESSES, "harnesses")
    cells = []
    for system in systems:
        runner = "ubuntu-24.04-arm" if system == "linux" else "macos-14"
        target = "unknown-linux-musl" if system == "linux" else "apple-darwin"
        for harness in selected:
            cells.append({"system": system, "runner": runner, "architecture": "aarch64",
                          "target": target, "harness": harness, "mode": mode})
    return {"mode": mode, "model": resolve_model(model), "cells": cells}


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
