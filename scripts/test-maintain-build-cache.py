#!/usr/bin/env python3
"""Behavioral contracts for local build-cache maintenance."""
import importlib.util
import os
from pathlib import Path
import tempfile
import sys
import time
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location("maintenance", Path(__file__).with_name("maintain-build-cache.py"))
maintenance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(maintenance)


class CacheMaintenanceTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.target = self.root / "target"
        self.target.mkdir()
        (self.target / "CACHEDIR.TAG").write_text("Signature: 8a477f597d28d172789f06886806bc55\n")
        self.git = patch.object(maintenance.subprocess, "check_output", return_value=b"").start()
        self.run = patch.object(maintenance.subprocess, "run").start()
        self.busy = patch.object(maintenance, "busy", return_value=False).start()
        self.inventory = patch.object(maintenance, "inventory", return_value=(21 * maintenance.GIB, time.time() - 7200)).start()
        self.addCleanup(patch.stopall)

    def test_preview_never_cleans(self):
        maintenance.maintain(self.root)
        self.run.assert_not_called()

    def test_size_limit_cleans_only_this_checkout(self):
        maintenance.maintain(self.root, apply=True)
        command = self.run.call_args.args[0]
        self.assertEqual(command[:3], ["cargo", "clean", "--locked"])
        self.assertEqual(command[-2:], ["--target-dir", str(self.target)])

    def test_stale_small_cache_is_eligible(self):
        self.inventory.return_value = (1024, time.time() - 15 * 86400)
        maintenance.maintain(self.root, apply=True)
        self.run.assert_called_once()

    def test_recent_small_cache_is_retained(self):
        self.inventory.return_value = (1024, time.time() - 7200)
        maintenance.maintain(self.root, apply=True)
        self.run.assert_not_called()

    def test_active_compiler_and_recent_writes_defer_cleanup(self):
        self.busy.return_value = True
        maintenance.maintain(self.root, apply=True)
        self.busy.return_value = False
        self.inventory.return_value = (21 * maintenance.GIB, time.time())
        maintenance.maintain(self.root, apply=True)
        self.run.assert_not_called()

    def test_unavailable_process_inventory_refuses_cleanup(self):
        self.busy.side_effect = OSError("unavailable")
        with self.assertRaises(OSError):
            maintenance.maintain(self.root, apply=True)
        self.run.assert_not_called()

    def test_unmarked_and_tracked_targets_are_preserved(self):
        self.git.return_value = b"target/user-file\n"
        with self.assertRaises(ValueError):
            maintenance.maintain(self.root, apply=True)
        self.git.return_value = b""
        (self.target / "CACHEDIR.TAG").unlink()
        with self.assertRaises(ValueError):
            maintenance.maintain(self.root, apply=True)
        self.run.assert_not_called()

    def test_symlink_target_is_preserved(self):
        other = self.root / "other"
        self.target.rename(other)
        self.target.symlink_to(other, target_is_directory=True)
        with self.assertRaises(ValueError):
            maintenance.maintain(self.root, apply=True)
        self.assertTrue((other / "CACHEDIR.TAG").exists())


class InventoryTests(unittest.TestCase):
    def test_hardlinks_count_once_and_directory_symlinks_are_not_followed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "target"
            target.mkdir()
            artifact = target / "binary"
            artifact.write_bytes(b"x" * 8192)
            os.link(artifact, target / "same-binary")
            outside = root / "evidence"
            outside.mkdir()
            (outside / "record").write_bytes(b"y" * 8192)
            (target / "external").symlink_to(outside, target_is_directory=True)
            size, _ = maintenance.inventory(target)
            self.assertEqual(size, artifact.stat().st_blocks * 512)


if __name__ == "__main__":
    unittest.main()
