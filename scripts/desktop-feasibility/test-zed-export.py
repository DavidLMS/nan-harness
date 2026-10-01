import importlib.util
import pathlib
import shutil
import subprocess
import tempfile
import sys
import json
import unittest

spec = importlib.util.spec_from_file_location("zed_export", pathlib.Path(__file__).with_name("zed-export.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def thread(parts):
    return {"version": "1.0.0", "messages": [{"User": {"id": "nonce", "content": [{"Text": "prompt-nonce"}]}}, {"Agent": {"content": parts, "tool_results": {}, "reasoning_details": None}}]}


class ExportTests(unittest.TestCase):
    def test_only_independent_assistant_text_can_pass(self):
        self.assertTrue(module.validate_thread(thread([{"Text": "reply-nonce"}]), "prompt-nonce", "reply-nonce")["verified"])
        for parts in [[{"Thinking": {"text": "reply-nonce"}}], [{"ToolUse": {"content": "reply-nonce"}}], [{"Text": "old reply-nonce"}], [{"Text": "reply-nonce"}, {"Text": "reply-nonce"}]]:
            self.assertFalse(module.validate_thread(thread(parts), "prompt-nonce", "reply-nonce")["verified"])

    def test_owned_decompressor_timeout_budget_and_malformed_transport_are_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            executable = pathlib.Path(directory) / "synthetic-zstd"
            for body in ["import time; time.sleep(30)", "import sys; sys.stdout.buffer.write(b'x' * (1024 * 1024 + 1))"]:
                executable.write_text("#!" + sys.executable + "\n" + body + "\n")
                executable.chmod(0o700)
                with self.assertRaises(ValueError):
                    module.decompress(str(executable), b"synthetic")
            request = {"expectedPrompt": "private-prompt-nonce", "expectedMarker": "private-response-nonce", "clipboard": "invalid-base64!"}
            output = subprocess.run([sys.executable, str(pathlib.Path(__file__).with_name("zed-export.py")), "--zstd", str(executable)], input=json.dumps(request).encode(), stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
            value = json.loads(output.stdout)
            self.assertEqual(value["error"], "decompression")
            self.assertFalse(value["verified"])
            self.assertNotIn(b"private", output.stdout + output.stderr)
            self.assertEqual(set(value), {"verified", "version", "userCount", "assistantTextCount", "error"})

    @unittest.skipUnless(shutil.which("zstd"), "zstd unavailable")
    def test_decompression_is_bounded_and_rejects_truncation(self):
        zstd = shutil.which("zstd")
        compressed = subprocess.run([zstd, "-q", "-c"], input=b"synthetic-json", stdout=subprocess.PIPE, check=True).stdout
        self.assertEqual(module.decompress(zstd, compressed), b"synthetic-json")
        with self.assertRaises(ValueError):
            module.decompress(zstd, compressed[:-2])
        oversized = subprocess.run([zstd, "-q", "-c"], input=b"x" * (module.LIMIT + 1), stdout=subprocess.PIPE, check=True).stdout
        with self.assertRaises(ValueError):
            module.decompress(zstd, oversized)

    def test_duplicate_user_unknown_version_and_structure_fail(self):
        value = thread([{"Text": "reply-nonce"}])
        value["messages"].insert(1, value["messages"][0])
        self.assertFalse(module.validate_thread(value, "prompt-nonce", "reply-nonce")["verified"])
        value = thread([{"Text": "reply-nonce"}])
        value["version"] = "2.0.0"
        self.assertFalse(module.validate_thread(value, "prompt-nonce", "reply-nonce")["verified"])
        self.assertFalse(module.validate_thread([], "prompt-nonce", "reply-nonce")["verified"])
        self.assertFalse(module.validate_thread(thread([{"Text": "reply-nonce"}]), "reply-nonce", "reply-nonce")["verified"])


if __name__ == "__main__":
    unittest.main()
