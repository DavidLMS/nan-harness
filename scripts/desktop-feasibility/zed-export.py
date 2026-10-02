#!/usr/bin/env python3
"""Bounded assistant-only oracle for Zed 1.22 SharedThread exports."""
import base64
import json
import pathlib
import subprocess
import sys
import threading

LIMIT = 1024 * 1024


def validate_thread(value, prompt, marker):
    result = {"verified": False, "version": None, "userCount": 0, "assistantTextCount": 0, "resumeCount": 0, "agentCount": 0, "totalAssistantTextCount": 0, "error": "schema"}
    if not isinstance(value, dict) or set(value) - {"title", "messages", "updated_at", "model", "version"} or value.get("version") != "1.0.0":
        return result
    result["version"] = "1.0.0"
    messages = value.get("messages")
    if not isinstance(messages, list) or len(messages) > 128:
        return result
    texts = []
    for message in messages:
        # Pinned Message::Resume is a serde unit variant, exported as a string.
        # Only the latest resumed segment can certify recovery of this User.
        if message == "Resume":
            if result["userCount"] != 1:
                return result
            result["resumeCount"] += 1
            texts.clear()
            continue
        if not isinstance(message, dict) or len(message) != 1:
            return result
        if "User" in message:
            user = message["User"]
            if not isinstance(user, dict) or set(user) != {"id", "content"} or user.get("content") != [{"Text": prompt}]:
                result["error"] = "user-mismatch"
                return result
            result["userCount"] += 1
            if result["userCount"] != 1 or texts:
                result["error"] = "user-mismatch"
                return result
        elif "Agent" in message:
            result["agentCount"] += 1
            agent = message["Agent"]
            if result["userCount"] != 1 or not isinstance(agent, dict) or set(agent) != {"content", "tool_results", "reasoning_details"} or not isinstance(agent.get("content"), list) or len(agent["content"]) > 128:
                return result
            for part in agent["content"]:
                if not isinstance(part, dict) or len(part) != 1 or not set(part) <= {"Text", "Thinking", "RedactedThinking", "ToolUse"}:
                    return result
                if "Text" in part:
                    if not isinstance(part["Text"], str):
                        return result
                    result["totalAssistantTextCount"] += 1
                    texts.append(part["Text"])
                    if len(texts) > 128:
                        return result
        else:
            return result
    result["assistantTextCount"] = len(texts)
    result["verified"] = result["userCount"] == 1 and texts == [marker] and bool(marker) and marker not in prompt
    result["error"] = None if result["verified"] else "assistant-mismatch"
    return result


def decompress(zstd, data):
    process = subprocess.Popen([zstd, "-d", "-q", "-c"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    output = []
    def write():
        try:
            process.stdin.write(data)
        except (BrokenPipeError, OSError):
            pass
        finally:
            process.stdin.close()
    def read():
        output.append(process.stdout.read(LIMIT + 1))
        if len(output[0]) > LIMIT:
            process.kill()
    writer = threading.Thread(target=write)
    reader = threading.Thread(target=read)
    writer.start()
    reader.start()
    try:
        status = process.wait(timeout=3)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()
        status = -1
    finally:
        writer.join()
        reader.join()
        process.stdout.close()
    if status != 0 or not output or len(output[0]) > LIMIT:
        raise ValueError("decompression")
    return output[0]


def main():
    result = {"verified": False, "version": None, "userCount": 0, "assistantTextCount": 0, "resumeCount": 0, "agentCount": 0, "totalAssistantTextCount": 0, "error": "request"}
    try:
        if len(sys.argv) != 3 or sys.argv[1] != "--zstd" or not pathlib.Path(sys.argv[2]).is_absolute():
            raise ValueError("request")
        raw = sys.stdin.buffer.read(128 * 1024 + 1)
        if len(raw) > 128 * 1024:
            raise ValueError("request")
        request = json.loads(raw)
        if set(request) != {"expectedPrompt", "expectedMarker", "clipboard"} or not all(isinstance(v, str) for v in request.values()) or len(request["clipboard"]) > 65536 or len(request["expectedPrompt"]) > 4096 or len(request["expectedMarker"]) > 4096:
            raise ValueError("request")
        result["error"] = "decompression"
        data = base64.b64decode(request["clipboard"], validate=True)
        decoded = decompress(sys.argv[2], data)
        result["error"] = "schema"
        value = json.loads(decoded)
        result = validate_thread(value, request["expectedPrompt"], request["expectedMarker"])
    except (ValueError, OSError, TypeError, RecursionError):
        pass
    print(json.dumps(result, separators=(",", ":")))


if __name__ == "__main__":
    main()
