"""Stage only launch configuration; native history belongs to the managed home."""
import os
from pathlib import Path
import shutil


def require_plain(path, kind):
    if path.is_symlink() or (path.exists() and not kind(path)):
        raise RuntimeError("profile_boundary")


def private_write(path, content):
    require_plain(path, Path.is_file)
    if not path.is_file():
        raise RuntimeError("private_file_missing")
    descriptor = os.open(path, os.O_WRONLY | os.O_TRUNC | getattr(os, "O_NOFOLLOW", 0))
    with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
        stream.write(content)


def stage(home, overlay, base_url, model):
    import hermes_yaml as yaml
    from gateway.config import Platform
    from hermes_cli.plugins import discover_plugins
    from gateway.platform_registry import platform_registry

    # This is a CLI owner: never import bots, cron jobs, profile multiplexing,
    # live admissions, or gateway control files from the original profile.
    rendered = yaml.safe_load((overlay / ".nanh-launch.yaml").read_text(encoding="utf-8")) or {}
    # The native home only supplies installed dependencies. Its fallback routes,
    # credentials, automation and plugin search paths are not this profile's.
    config = {key: rendered[key] for key in ("compression", "stt", "tts", "image_gen", "web") if key in rendered}
    enabled = []
    if (home / "plugins" / "web" / "nan_harness").is_dir() and (rendered.get("web") or {}).get("search_backend") == "nan-harness":
        enabled.append("web/nan_harness")
    if (home / "plugins" / "image_gen" / "nan_harness").is_dir():
        enabled.append("image_gen/nan_harness")
    config["plugins"] = {"enabled": enabled, "entries": {"image_gen/nan_harness": {"allow_tool_override": True}}}
    config["gateway"] = {"multiplex_profiles": False}
    config["updates"] = {"check": False}
    private_write(home / "config.yaml", yaml.safe_dump(config))
    discover_plugins()
    platforms = {platform.value for platform in Platform} | set(platform_registry.registered_names())
    config["platforms"] = {name: {"enabled": False} for name in platforms if name != "local"}
    for platform in platforms:
        config.pop(platform, None)
    config["model"] = {"provider": "nan", "default": model}
    config["kanban"] = {"dispatch_in_gateway": False, "notify_in_gateway": False}
    providers = config.setdefault("providers", {})
    providers["nan"] = {
        "name": "NaN", "base_url": base_url, "key_env": "NAN_API_KEY",
        "transport": "openai_chat", "model": model, "default_model": model,
        "models": {model: {}},
    }
    private_write(home / "config.yaml", yaml.safe_dump(config))
    # Rust precreates every generated file with the private DACL/mode. Updating
    # those handles preserves protection before any payload is written.
    return config


def clear_launch_files(home):
    for name in ("config.yaml", "plugins", "installs"):
        path = home / name
        require_plain(path, Path.is_dir if name != "config.yaml" else Path.is_file)
        if path.is_dir():
            shutil.rmtree(path)
        elif path.exists():
            path.unlink()
