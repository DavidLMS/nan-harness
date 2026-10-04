import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

WRAPPER = Path(__file__).resolve().parents[1] / 'run-desktop-check-session.sh'

class ScreenPolicyTests(unittest.TestCase):
    def invoke(self, changes=None, app='zed-desktop'):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, body in {
                'dbus-run-session': 'shift; exec "$@"',
                'busctl': 'exit 0',
                "xvfb-run": "python3 -c 'import json,sys; print(json.dumps(sys.argv[1:]))' \"$@\"",
            }.items():
                script = root / name
                script.write_text('#!/bin/bash\n' + body + '\n')
                script.chmod(0o700)
            env = {**os.environ, 'PATH': str(root) + ':' + os.environ['PATH'],
                   'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted',
                   'RUNNER_OS': 'Linux', 'FEASIBILITY_ZED_MAXIMIZED': '1',
                   'NANH_ZED_PANEL_LAYOUT': 'fixed-wide',
                   'NANH_ZED_SCREEN_POLICY': 'height-1536'}
            for key in ['NANH_ZED_LAYOUT_POLICY', 'NANH_ZED_PANEL_ZOOM']:
                env.pop(key, None)
            for key, value in (changes or {}).items():
                if value is None:
                    env.pop(key, None)
                else:
                    env[key] = value
            return subprocess.run(['bash', str(WRAPPER), 'python3',
                'scripts/desktop-feasibility/run-qualification.py', '--app', app,
                '--platform', 'linux'], env=env, capture_output=True, text=True,
                timeout=3)

    def test_scoped_screen_and_other_app_default(self):
        result = self.invoke()
        self.assertEqual(result.returncode, 0)
        self.assertEqual(json.loads(result.stdout)[:3],
                         ['-a', '--server-args=-screen 0 1280x1536x24', 'bash'])
        result = self.invoke({'NANH_ZED_SCREEN_POLICY': None}, app='chatgpt-desktop')
        self.assertEqual(result.returncode, 0)
        self.assertEqual(json.loads(result.stdout)[:2], ['-a', 'bash'])
        self.assertFalse(any('server-args' in arg for arg in json.loads(result.stdout)))

    def test_invalid_scope_never_starts_session(self):
        for change in [{'NANH_ZED_SCREEN_POLICY': ''},
                       {'NANH_ZED_SCREEN_POLICY': 'height-2048'},
                       {'RUNNER_OS': 'Windows'}, {'GITHUB_ACTIONS': 'false'},
                       {'RUNNER_ENVIRONMENT': 'self-hosted'},
                       {'NANH_ZED_PANEL_LAYOUT': 'fixed-wide-compact'},
                       {'FEASIBILITY_ZED_MAXIMIZED': '0'},
                       {'NANH_ZED_LAYOUT_POLICY': 'zoom-before-send'},
                       {'NANH_ZED_PANEL_ZOOM': 'observe'}]:
            with self.subTest(change=change):
                result = self.invoke(change)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, '')
        self.assertNotEqual(self.invoke(app='claude-desktop').returncode, 0)

if __name__ == '__main__':
    unittest.main()
