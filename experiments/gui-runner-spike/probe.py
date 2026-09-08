"""Bounded GUI probes; all displayed and recorded data is synthetic."""
import importlib.metadata
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parent
OUTPUT = ROOT / "results"


def probe(kind, repetition):
    import xa11y

    xa11y.set_default_timeout(3)
    if kind == "qt":
        command = [sys.executable, str(ROOT / "qt_app.py")]
    else:
        binary = ROOT / "node_modules/electron/dist"
        if sys.platform == "darwin":
            binary /= "Electron.app/Contents/MacOS/Electron"
        else:
            binary /= "electron.exe" if os.name == "nt" else "electron"
        command = [str(binary), str(ROOT / "electron.cjs")]
    record = {"app": kind, "repetition": repetition, "status": "failed", "steps": []}
    started = time.monotonic()
    process = None
    try:
        process = subprocess.Popen(command, cwd=ROOT)
        app = xa11y.App.by_pid(process.pid, timeout=20)
        app.locator("text_field").wait_visible()
        record["textFields"] = [
            {"role": element.role, "name": element.name,
             "description": element.description, "raw": element.raw}
            for element in app.locator("text_field").elements()
        ]
        field = app.locator(
            'text_field[name="Message"], text_field[description="Message"]')
        record["nameOnlyCandidates"] = {
            name: [{"role": element.role, "name": element.name}
                   for element in app.locator(f'[name="{name}"]').elements()]
            for name in ("Message", "Result")
        }
        record["initialTree"] = app.tree(max_depth=12)
        field.wait_visible()
        assert field.count() == 1, "input selector is ambiguous"
        value = "nanh-spike-" + uuid.uuid4().hex
        record["inputMode"] = "accessibility-set-value"
        try:
            field.set_value(value)
        except xa11y.ActionNotSupportedError as error:
            if sys.platform != "linux" or kind != "electron":
                raise
            record["setValueError"] = str(error)
            record["inputMode"] = "accessibility-focus-and-physical-keyboard"
            field.focus()
            field.wait_focused()
            xa11y.input_sim().type_text(value)
        field.wait_until(lambda element: element is not None and element.value == value)
        record["steps"].append("write-and-read")
        app.locator('button[name="Send"]').press()
        deadline = time.monotonic() + 10
        result = app.locator(
            'text_field[name="Result"], text_field[description="Result"]')
        assert result.count() == 1, "result selector is ambiguous"
        while result.element().value != "Received: " + value:
            if time.monotonic() >= deadline:
                raise AssertionError("button did not produce expected result")
            time.sleep(0.1)
        record["steps"].append("press-and-verify-result")
        negative_started = time.monotonic()
        try:
            app.locator('button[name="Definitely absent control"]').press()
        except xa11y.TimeoutError:
            assert time.monotonic() - negative_started < 10, "negative test exceeded budget"
        else:
            raise AssertionError("missing control unexpectedly succeeded")
        record["steps"].append("missing-control-times-out")
        xa11y.screenshot().save_png(OUTPUT / f"{kind}-{repetition}.png")
        record["steps"].append("screenshot")
        record["status"] = "passed"
    except Exception as error:
        record["error"] = {"type": type(error).__name__, "message": str(error)}
    finally:
        if process is not None:
            if os.name == "nt":
                subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                               capture_output=True, timeout=10)
            else:
                process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        record["durationSeconds"] = round(time.monotonic() - started, 2)
        (OUTPUT / f"{kind}-{repetition}.json").write_text(json.dumps(record, indent=2))
    return record["status"] == "passed"


def main():
    OUTPUT.mkdir(exist_ok=True)
    if len(sys.argv) == 3:
        return 0 if probe(sys.argv[1], int(sys.argv[2])) else 1
    records = []
    for kind in ("qt", "electron"):
        for repetition in range(1, 4):
            report = OUTPUT / f"{kind}-{repetition}.json"
            with (OUTPUT / f"{kind}-{repetition}.log").open("w") as log:
                try:
                    subprocess.run([sys.executable, __file__, kind, str(repetition)],
                                   stdout=log, stderr=subprocess.STDOUT, timeout=90, check=False)
                except subprocess.TimeoutExpired:
                    pass
            record = json.loads(report.read_text()) if report.exists() else {
                "app": kind, "repetition": repetition, "status": "failed",
                "error": "probe timed out or exited without a report",
            }
            records.append(record)
            print(json.dumps(record), flush=True)
    summary = {
        "platform": platform.platform(), "architecture": platform.machine(),
        "python": platform.python_version(), "xa11y": importlib.metadata.version("xa11y"),
        "qt": importlib.metadata.version("PySide6"),
        "runnerImage": os.environ.get("ImageVersion"),
        "results": records,
    }
    (OUTPUT / "summary.json").write_text(json.dumps(summary, indent=2))
    return 0 if all(r["status"] == "passed" for r in records) else 1


if __name__ == "__main__":
    sys.exit(main())
