"""Gentle Shell behavioral evidence; only synthetic fixtures leave this module."""
import json
import re
import secrets


def reader_fixture(home, model):
    marker = "GENTLE_READ_" + secrets.token_hex(12)
    (home / "sentinel.txt").write_text(marker + "\n")
    agents = home / ".pi" / "agents"
    agents.mkdir(parents=True, exist_ok=True)
    (agents / "qualification-reader.md").write_text(
        "---\nname: qualification-reader\ndescription: Read the qualification fixture.\n"
        f"model: nan/{model}\ntools: read\n---\n"
        "Read sentinel.txt with the read tool and return its exact contents. Do not change files.\n")
    return marker


def events(output):
    found = []
    for line in output.splitlines():
        try:
            value = json.loads(line)
        except ValueError:
            continue
        if isinstance(value, dict):
            found.append(value)
    return found


def verify_child(agent, output, marker, model):
    completed = [event for event in events(output) if event.get("type") == "tool_execution_end"
                 and event.get("toolName") == "subagent_run" and event.get("isError") is False
                 and event.get("result", {}).get("details", {}).get("gentleAgents", {}).get("agent") == "qualification-reader"
                 and event.get("result", {}).get("details", {}).get("gentleAgents", {}).get("status") == "completed"]
    assert any(marker in json.dumps(event.get("result")) for event in completed), "child-result"
    # Inspect the owned child's actual transcript, rather than accepting a parent's claim.
    for path in (agent / "gentle-agents" / "sessions").glob("*.jsonl"):
        if path.stat().st_size > 8_000_000:
            continue
        records = events(path.read_text(encoding="utf-8"))
        messages = [event.get("message", {}) for event in records]
        correct_model = any(event.get("type") == "model_change" and event.get("provider") == "nan"
                            and event.get("modelId") == model for event in records)
        read_result = any(message.get("role") == "toolResult" and message.get("toolName") == "read"
                          and message.get("isError") is False and marker in json.dumps(message.get("content"))
                          for message in messages)
        if correct_model and read_result:
            return
    raise AssertionError("child-read-evidence")


def child_prompt():
    return ('Call subagent_run exactly once with agent="qualification-reader", mode="task", '
            'task="Read sentinel.txt with read and return its exact contents". '
            'Do not read the file yourself. Return the child result after it finishes.')


def gateway_requests(stderr):
    return sum(int(count) for count in re.findall(r"\b(\d+) requests?\b", stderr))
