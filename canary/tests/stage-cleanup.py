#!/usr/bin/env python3
"""Unknown cleanup cannot promote a passed stage or leak its supervision handle."""

from pathlib import Path
import errno
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "actions"))
import cell
import desktop_suite


class CleanupTests(unittest.TestCase):
    def test_job_cleanup_does_not_depend_on_taskkill_and_wait_is_bounded(self):
        child, job = Mock(pid=12), Mock()
        with patch.object(cell, "terminate_process_tree", side_effect=subprocess.TimeoutExpired("taskkill", 10)) as kill:
            cell.finish_stage(child, job)
        kill.assert_not_called()
        job.close.assert_called_once()
        child.wait.assert_called_once_with(timeout=10)

    def test_failed_job_cleanup_still_reaps_parent_and_aborts(self):
        child, job = Mock(pid=12), Mock()
        job.close.side_effect = RuntimeError("unproven")
        with self.assertRaises(cell.CleanupError):
            cell.finish_stage(child, job)
        child.wait.assert_called_once_with(timeout=10)
        job.close.assert_called_once()

    def test_unreaped_parent_is_not_successful_cleanup(self):
        child = Mock(pid=12)
        child.wait.side_effect = subprocess.TimeoutExpired("child", 10)
        with self.assertRaises(cell.CleanupError):
            cell.finish_stage(child, Mock())

    def test_darwin_exiting_group_requires_observed_absence_without_repeating_kill(self):
        child = Mock(pid=12, returncode=0)
        denied = PermissionError(errno.EPERM, "synthetic")
        with patch.object(cell.sys, "platform", "darwin"), patch.object(cell.time, "sleep"), \
                patch.object(cell, "terminate_process_tree", side_effect=denied) as terminate, \
                patch.object(cell.os, "killpg", create=True, side_effect=[denied, ProcessLookupError(errno.ESRCH, "gone")]) as observe:
            cell.finish_stage(child, None)
        terminate.assert_called_once_with(12)
        self.assertEqual(observe.call_args_list, [((12, 0),), ((12, 0),)])
        child.wait.assert_called_once_with(timeout=10)

    def test_darwin_presence_and_unknown_cleanup_remain_terminal(self):
        denied = PermissionError(errno.EPERM, "synthetic")
        for observation in (denied, None, OSError(errno.EIO, "query failed")):
            child = Mock(pid=12, returncode=0)
            with patch.object(cell.sys, "platform", "darwin"), patch.object(cell.time, "sleep"), \
                    patch.object(cell, "terminate_process_tree", side_effect=denied) as terminate, \
                    patch.object(cell.os, "killpg", create=True, side_effect=observation) as observe:
                with self.assertRaises(cell.CleanupError):
                    cell.finish_stage(child, None)
            terminate.assert_called_once_with(12)
            self.assertLessEqual(observe.call_count, 40)
            self.assertTrue(all(call.args == (12, 0) for call in observe.call_args_list))
            child.wait.assert_called_once_with(timeout=10)

    def test_group_settlement_requires_darwin_eperm_reaped_child_and_remaining_time(self):
        for platform, code, error in [('linux', 0, errno.EPERM), ('darwin', None, errno.EPERM),
                                      ('darwin', 0, errno.EACCES)]:
            with patch.object(cell.sys, "platform", platform), patch.object(cell.os, "killpg", create=True) as observe:
                self.assertFalse(cell.settled_darwin_group(Mock(pid=12, returncode=code), PermissionError(error, "synthetic")))
            observe.assert_not_called()
        with patch.object(cell.sys, "platform", "darwin"), patch.object(cell.time, "monotonic", side_effect=[0, 3]), \
                patch.object(cell.os, "killpg", create=True) as observe:
            self.assertFalse(cell.settled_darwin_group(Mock(pid=12, returncode=0), PermissionError(errno.EPERM, "synthetic")))
        observe.assert_not_called()

    def test_private_log_is_protected_before_use_and_removed_after_failure(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(cell, "protect_private") as protect:
            with self.assertRaises(RuntimeError):
                with cell.private_log(directory) as log:
                    path = Path(log.name)
                    protect.assert_called_once_with(path)
                    self.assertEqual(path.stat().st_size, 0)
                    log.write(b"synthetic-private-payload")
                    raise RuntimeError("synthetic operation failed")
            self.assertFalse(path.exists())

    def test_desktop_propagates_unknown_cleanup_to_abort_status(self):
        with patch.object(desktop_suite, "private_command", side_effect=cell.CleanupError()):
            with self.assertRaises(desktop_suite.StageTimeout):
                desktop_suite.run_stage(["unused-checker"])

    def test_windows_sharing_violation_retries_removal_without_ignoring_other_errors(self):
        path = Mock()
        sharing = PermissionError()
        sharing.winerror = 32
        path.unlink.side_effect = [sharing, None]
        with patch.object(cell.time, "sleep"):
            cell.remove_private_log(path)
        self.assertEqual(path.unlink.call_count, 2)
        path.unlink.side_effect = PermissionError()
        with self.assertRaises(cell.CleanupError):
            cell.remove_private_log(path)


if __name__ == "__main__":
    unittest.main()
