import importlib.util
import pathlib
import shutil
import subprocess
import tempfile
import sys
import json
import base64
import unittest

spec = importlib.util.spec_from_file_location("zed_export", pathlib.Path(__file__).with_name("zed-export.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def thread(parts):
    return {"version": "1.0.0", "messages": [{"User": {"id": "nonce", "content": [{"Text": "prompt-nonce"}]}}, {"Agent": {"content": parts, "tool_results": {}, "reasoning_details": None}}]}


class ExportTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which("zstd"), "zstd unavailable")
    def test_resumed_export_transport_emits_only_the_closed_verdict(self):
        value = thread([])
        value["messages"].extend(["Resume", thread([{"Text": "reply-nonce"}])["messages"][1]])
        zstd = shutil.which("zstd")
        compressed = subprocess.run([zstd, "-q", "-c"], input=json.dumps(value).encode(), stdout=subprocess.PIPE, check=True).stdout
        request = {"expectedPrompt": "prompt-nonce", "expectedMarker": "reply-nonce", "clipboard": base64.b64encode(compressed).decode()}
        output = subprocess.run([sys.executable, str(pathlib.Path(__file__).with_name("zed-export.py")), "--zstd", zstd], input=json.dumps(request).encode(), stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
        result = json.loads(output.stdout)
        self.assertTrue(result["verified"])
        self.assertEqual(result["assistantTextCount"], 1)
        self.assertEqual(set(result), {"verified", "version", "userCount", "assistantTextCount", "resumeCount", "agentCount", "totalAssistantTextCount", "error"})
        self.assertNotIn(b"nonce", output.stdout + output.stderr)

    def test_latest_empty_segment_keeps_only_closed_history_counts(self):
        value = thread([{"Text": "reply-nonce"}])
        value['messages'].append('Resume')
        result = module.validate_thread(value, 'prompt-nonce', 'reply-nonce')
        self.assertFalse(result['verified'])
        self.assertEqual((result['assistantTextCount'], result['resumeCount'], result['agentCount'], result['totalAssistantTextCount']), (0, 1, 1, 1))
        self.assertNotIn('nonce', json.dumps(result))

    def test_native_resume_certifies_only_the_latest_assistant_segment(self):
        value = thread([])
        value["messages"].extend([
            "Resume",
            {"Agent": {"content": [], "tool_results": {}, "reasoning_details": None}},
            "Resume",
            {"Agent": {"content": [{"Text": "reply-nonce"}], "tool_results": {}, "reasoning_details": None}},
        ])
        result = module.validate_thread(value, "prompt-nonce", "reply-nonce")
        self.assertTrue(result["verified"])
        self.assertEqual(result["userCount"], 1)
        self.assertEqual(result["assistantTextCount"], 1)

    def test_stale_failed_text_thinking_and_tools_cannot_certify_recovery(self):
        for latest in [[], [{"Thinking": {"text": "reply-nonce"}}], [{"ToolUse": {"content": "reply-nonce"}}], [{"Text": "different-nonce"}]]:
            value = thread([{"Text": "reply-nonce"}])
            value["messages"].extend([
                "Resume",
                {"Agent": {"content": latest, "tool_results": {}, "reasoning_details": None}},
            ])
            result = module.validate_thread(value, "prompt-nonce", "reply-nonce")
            self.assertFalse(result["verified"])
            self.assertEqual(result["error"], "assistant-mismatch")

    def test_resume_does_not_relax_user_identity_or_accept_unknown_variants(self):
        for invalid in ["resume", "Continue where you left off", {"Resume": None}, {"Compaction": {"Summary": "reply-nonce"}}]:
            value = thread([{"Text": "reply-nonce"}])
            value["messages"].insert(1, invalid)
            self.assertEqual(module.validate_thread(value, "prompt-nonce", "reply-nonce")["error"], "schema")
        value = thread([{"Text": "reply-nonce"}])
        value["messages"].insert(0, "Resume")
        self.assertFalse(module.validate_thread(value, "prompt-nonce", "reply-nonce")["verified"])
        value = thread([])
        value["messages"].extend(["Resume", value["messages"][0], thread([{"Text": "reply-nonce"}])["messages"][1]])
        result = module.validate_thread(value, "prompt-nonce", "reply-nonce")
        self.assertFalse(result["verified"])
        self.assertEqual(result["error"], "user-mismatch")

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
            self.assertEqual(set(value), {"verified", "version", "userCount", "assistantTextCount", "resumeCount", "agentCount", "totalAssistantTextCount", "error"})

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
