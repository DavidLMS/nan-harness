"""Native Hermes entry points for the Rust-owned private CLI profile."""
import asyncio
import contextlib
import io
import os
from pathlib import Path
import runpy
import sys
import threading
import time
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parent))

READINESS_EXIT_CODES = {reason: 90 + index for index, reason in enumerate((
    "control-absent", "control-access", "control-timeout", "control-protocol",
    "identity-pid", "identity-home", "identity-supervisor", "runtime-protocol",
    "runtime-profile", "runtime-authority", "runtime-endpoint", "runtime-state",
))}


class ReadinessFailure(RuntimeError):
    def __init__(self, reason):
        self.exit_code = READINESS_EXIT_CODES[reason]
        super().__init__("NH-HERMES-READINESS-" + reason.upper())


def require_absent(home):
    from hermes_cli.gateway_runtime import discover_gateway_endpoint
    if discover_gateway_endpoint(home, timeout=0.2).state != "absent":
        raise RuntimeError("owner_present")


def ready(home, pid):
    from hermes_cli.gateway_runtime import discover_gateway_endpoint
    from hermes_cli.gateway_runtime_discovery import query_identify
    deadline = time.monotonic() + 60
    reason = "control-absent"
    while time.monotonic() < deadline:
        try:
            identity = query_identify(home, timeout=0.2)
        except (OSError, ValueError) as error:
            if isinstance(error, FileNotFoundError):
                reason = "control-absent"
            elif isinstance(error, PermissionError):
                reason = "control-access"
            elif isinstance(error, TimeoutError):
                reason = "control-timeout"
            else:
                reason = "control-protocol"
            time.sleep(0.05)
            continue
        if identity.get("pid") != pid:
            raise ReadinessFailure("identity-pid")
        if Path(identity.get("hermes_home", "")).resolve() != home:
            raise ReadinessFailure("identity-home")
        if identity.get("supervisor") != "external":
            raise ReadinessFailure("identity-supervisor")
        discovery = discover_gateway_endpoint(home, timeout=0.2)
        if discovery.state == "ready" and discovery.endpoint is not None:
            return discovery.endpoint
        reason = {
            "runtime_protocol": "runtime-protocol", "profile_mismatch": "runtime-profile",
            "session_authority_unavailable": "runtime-authority", "invalid_api_origin": "runtime-endpoint",
            "invalid_runtime_identity": "runtime-endpoint", "invalid_supervisor": "runtime-endpoint",
            "control_timeout": "control-timeout", "invalid_control_peer": "control-protocol",
        }.get(discovery.reason_code, "runtime-state")
        time.sleep(0.05)
    raise ReadinessFailure(reason)


def require_owned_session(home, session_id):
    from hermes_state import SessionDB
    from hermes_state_local import local_receipt
    reader = SessionDB(home / "state.db", read_only=True)
    try:
        import json
        receipt = local_receipt(reader, session_id)
        config = json.loads(receipt["policy"]["config_json"])
        if (Path(receipt["profile_id"]).resolve() != home
                or (config.get("model") or {}).get("provider") not in {"nan", "custom:nan"}):
            raise RuntimeError("history_owner")
    finally:
        reader.close()


def native_arguments(arguments):
    model = os.environ["NAN_HARNESS_HERMES_MODEL"]
    if arguments[:4] != ["--provider", "nan", "--model", model]:
        raise RuntimeError("routing_arguments")
    # Routing comes from the owned config. Repeating creation flags on a native
    # --continue/--create-if-missing launch can conflict with its saved policy.
    return arguments[4:]


def require_creation_routing(parsed):
    if any(getattr(parsed, name, None) is not None for name in ("provider", "model", "api_key", "base_url")):
        raise RuntimeError("routing_override")
    if getattr(parsed, "safe_mode", False) or getattr(parsed, "ignore_user_config", False):
        raise RuntimeError("routing_bypass")


async def refresh(home, pid, arguments):
    from hermes_cli.gateway_client import connect_gateway
    from hermes_cli import main as native
    from hermes_cli.gateway_chat import _resume, _resume_latest, continue_title, wants_latest
    from hermes_cli.gateway_client import GatewayClientError

    # Use the native grammar and lookup rules. Non-chat commands retain their
    # normal parser and behavior in the frontend; no user text leaves this child.
    with contextlib.redirect_stderr(io.StringIO()), contextlib.redirect_stdout(io.StringIO()):
        try:
            parsed, unknown = native._light_chat_parser().parse_known_args(
                native._coalesce_session_name_args(native_arguments(arguments)))
        except SystemExit:
            return
    if unknown or parsed.command not in {None, "chat"}:
        return
    require_creation_routing(parsed)
    native._set_chat_arg_defaults(parsed)
    title = continue_title(parsed)
    name = getattr(parsed, "resume", None) or title
    if not name and not wants_latest(parsed):
        return
    endpoint = ready(home, pid)
    async with connect_gateway(endpoint=endpoint) as client:
        try:
            if wants_latest(parsed):
                snapshot = await _resume_latest(client, parsed)
            else:
                try:
                    snapshot = await _resume(client, parsed, session_id=name)
                except GatewayClientError as error:
                    if str(error) != "not_found":
                        raise
                    snapshot = await _resume(client, parsed, title=name)
        except GatewayClientError as error:
            if str(error) == "not_found":
                return
            raise
        require_owned_session(home, snapshot["session_id"])
        result = await client.rpc(
            "session.mutate", session_id=snapshot["session_id"],
            request_id=uuid.uuid4().hex,
            expected_revision=snapshot["revision"],
            expected_generation=snapshot["execution_generation"],
            operation="model",
            payload={"provider": "nan", "model": os.environ["NAN_HARNESS_HERMES_MODEL"]},
        )
        if (result.get("status") in {"confirmation_required", "deferred"}
                or result.get("model") != os.environ["NAN_HARNESS_HERMES_MODEL"]
                or type(result.get("execution_generation")) is not int):
            raise RuntimeError("history_route")


def owner():
    def watch_host():
        # Rust is the only writer. EOF also covers a crashed host. Native marker
        # shutdown is cooperative on Windows, unlike TerminateProcess/SIGTERM.
        while sys.stdin.buffer.read(1):
            pass
        from gateway.status import write_planned_stop_marker
        if not write_planned_stop_marker(os.getpid()):
            os._exit(125)
    threading.Thread(target=watch_host, daemon=True).start()
    sys.argv = ["hermes", "--profile", "default", "gateway", "run", "--quiet",
                "--external-supervisor", "--no-supervise"]
    runpy.run_module("hermes_cli.main", run_name="__main__", alter_sys=True)


def main(arguments):
    action, *rest = arguments
    home = Path(os.environ["NAN_HARNESS_HERMES_MANAGED_HOME"]).resolve()
    overlay = Path(os.environ["NAN_HARNESS_HERMES_OVERLAY"]).resolve()
    os.environ["HERMES_HOME"] = str(home)
    os.environ["HERMES_PROFILE"] = "default"
    os.environ["GATEWAY_MULTIPLEX_PROFILES"] = "false"
    os.environ.pop("HERMES_TUI_GATEWAY_URL", None)
    if os.name != "nt":
        os.umask(0o077)
    if action == "absent":
        require_absent(home)
    elif action == "stage":
        from nanh_profile import stage
        from hermes_cli.gateway_chat import env_launch_flags
        flags = env_launch_flags()
        if flags.get("safe_mode") or flags.get("ignore_user_config"):
            raise RuntimeError("routing_bypass")
        require_absent(home)
        stage(home, overlay, os.environ["NAN_HARNESS_HERMES_BASE_URL"],
              os.environ["NAN_HARNESS_HERMES_MODEL"])
    elif action == "owner":
        owner()
    elif action == "ready":
        ready(home, int(rest[0]))
    elif action == "refresh":
        asyncio.run(refresh(home, int(rest[0]), rest[1:]))
    elif action == "cleanup":
        from nanh_profile import clear_launch_files
        require_absent(home)
        clear_launch_files(home)
    elif action == "frontend":
        sys.argv = ["hermes", "--profile", "default", *native_arguments(rest)]
        runpy.run_module("hermes_cli.main", run_name="__main__", alter_sys=True)
    else:
        raise RuntimeError("unknown_action")


if __name__ == "__main__":
    try:
        main(sys.argv[1:])
    except ReadinessFailure as error:
        raise SystemExit(error.exit_code) from None
    except Exception as error:
        # Fixed phase only. Native exceptions can contain private configuration.
        category = "IMPORT" if isinstance(error, ImportError) else "IO" if isinstance(error, OSError) else "CONTRACT" if isinstance(error, RuntimeError) else "INTERNAL"
        print("NH-HERMES-SUPERVISOR-" + category, file=sys.stderr)
        raise SystemExit(125) from None
