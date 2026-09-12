#!/usr/bin/env python3
"""Select bounded native suite jobs before reserving their runners."""

import argparse
import json
import os
import re


CLI_HARNESSES = (
    "claude-code", "codex", "opencode", "hermes", "pi", "omp", "prime-agent",
    "deepseek-harness", "openclaw", "cline", "qwen-code", "kimi-code", "aider",
    "goose", "fx",
)
DESKTOP_HARNESSES = (
    "chatgpt-desktop", "claude-desktop", "hermes-desktop", "pen-desktop", "zed-desktop",
)
SYSTEMS = ("linux", "macos", "windows")
DEFAULT_MODEL = "qwen3.6"
MODEL_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}\Z")


def select_names(value, known, label):
    """Accept all or a nonempty, duplicate-free comma-separated selection."""
    names = [part.strip() for part in value.split(",")]
    if names == ["all"]:
        return list(known)
    if (not names or any(not name or name not in known for name in names)
            or len(names) != len(set(names))):
        raise ValueError(f"{label} must be all or distinct known identifiers")
    # Stable catalog order makes equivalent requests share the same identity.
    return [name for name in known if name in names]


def resolve_model(requested="", configured=None):
    """An explicit model wins over the repository default; never substitute one."""
    if configured is None:
        configured = os.environ.get("CANARY_MODEL", "")
    model = requested or configured or DEFAULT_MODEL
    if not isinstance(model, str) or not MODEL_ID.fullmatch(model):
        raise ValueError("model must be a bounded model identifier")
    return model


def native_platform(suite, system):
    if system == "linux":
        if suite == "cli":
            return {"system": system, "runner": "ubuntu-24.04-arm",
                    "architecture": "aarch64", "target": "unknown-linux-musl"}
        return {"system": system, "runner": "ubuntu-24.04",
                "architecture": "x86_64", "target": "unknown-linux-musl"}
    if system == "macos":
        return {"system": system, "runner": "macos-15",
                "architecture": "aarch64", "target": "apple-darwin"}
    return {"system": system, "runner": "windows-2025",
            "architecture": "x86_64", "target": "pc-windows-msvc"}


def select_suite(suite, platforms="all", harnesses="all", mode="deterministic", model=""):
    if suite not in ("cli", "desktop"):
        raise ValueError("suite must be cli or desktop")
    if mode not in ("deterministic", "live"):
        raise ValueError("mode must be deterministic or live")
    catalog = CLI_HARNESSES if suite == "cli" else DESKTOP_HARNESSES
    selected = select_names(harnesses, catalog, "harnesses")
    systems = select_names(platforms, SYSTEMS, "platforms")
    return {"suite": suite, "mode": mode, "model": resolve_model(model),
            "harnesses": selected,
            "platforms": [{**native_platform(suite, system), "harnesses": selected}
                          for system in systems]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite", required=True, choices=("cli", "desktop"))
    parser.add_argument("--platforms", default="all")
    parser.add_argument("--harnesses", default="all")
    parser.add_argument("--mode", default="deterministic")
    parser.add_argument("--model", default="")
    args = parser.parse_args()
    try:
        selected = select_suite(args.suite, args.platforms, args.harnesses, args.mode, args.model)
    except ValueError as error:
        parser.error(str(error))
    print(json.dumps(selected, sort_keys=True))


if __name__ == "__main__":
    main()
