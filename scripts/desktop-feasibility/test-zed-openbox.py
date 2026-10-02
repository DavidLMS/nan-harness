#!/usr/bin/env python3
"""Verify first-map XML policy and shell launch boundaries without a display."""
import importlib.util
import os
from pathlib import Path
import subprocess
import stat
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('zed_openbox', Path(__file__).with_name('zed-openbox.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
STOCK = b'<openbox_config xmlns="http://openbox.org/3.4/rc"><focus><focusNew>yes</focusNew></focus><keyboard><keybind key="C-a"><action name="Example"/></keybind></keyboard><mouse><context name="Client"/></mouse><applications><application class="Other"><maximized>no</maximized></application></applications></openbox_config>'


class PolicyTests(unittest.TestCase):
    def test_preserves_bindings_and_appends_only_exact_normal_zed_rule(self):
        original = ET.fromstring(STOCK)
        result = ET.fromstring(module.transform(STOCK))
        namespace = {'o': module.NAMESPACE}
        for section in ('keyboard', 'mouse', 'focus'):
            self.assertEqual(ET.tostring(original.find('o:' + section, namespace)),
                             ET.tostring(result.find('o:' + section, namespace)))
        rules = result.findall('o:applications/o:application', namespace)
        self.assertEqual(rules[0].attrib, {'class': 'Other'})
        self.assertEqual(rules[-1].attrib, {'name': 'dev.zed.Zed', 'class': 'dev.zed.Zed', 'type': 'normal'})
        self.assertEqual(rules[-1].find('o:maximized', namespace).text, 'yes')
        self.assertEqual(len(rules), 2)

    def test_rejects_malformed_or_incomplete_stock_policy(self):
        for payload in (b'<invalid/>', b'<', STOCK.replace(b'<mouse><context name="Client"/></mouse>', b''),
                        STOCK.replace(b'</applications>', b'</applications><applications/>')):
            with self.assertRaises((ValueError, ET.ParseError)):
                module.transform(payload)

    def test_generated_configuration_is_private_and_cannot_overwrite(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory).resolve()
            source, destination = directory / 'stock.xml', directory / 'private.xml'
            source.write_bytes(STOCK)
            environment = dict(os.environ, GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Linux')
            command = [sys.executable, str(Path(__file__).with_name('zed-openbox.py')), str(source), str(destination)]
            result = subprocess.run(command, env=environment, capture_output=True, timeout=3)
            self.assertEqual(result.returncode, 0)
            self.assertEqual(stat.S_IMODE(destination.stat().st_mode), 0o600)
            original = destination.read_bytes()
            result = subprocess.run(command, env=environment, capture_output=True, timeout=3)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(destination.read_bytes(), original)

    def test_nonhosted_optin_cannot_start_window_manager(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / 'started'
            binary = Path(directory) / 'openbox'
            binary.write_text('#!/bin/sh\ntouch "$WM_STARTED"\n')
            binary.chmod(0o700)
            environment = dict(os.environ, PATH=directory + ':' + os.environ['PATH'],
                               WM_STARTED=str(marker), FEASIBILITY_ZED_MAXIMIZED='1',
                               GITHUB_ACTIONS='false', RUNNER_ENVIRONMENT='github-hosted')
            result = subprocess.run(['bash', str(ROOT / 'scripts/run-desktop-check-x11.sh'), '/usr/bin/true'],
                                    env=environment, capture_output=True, timeout=3)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(marker.exists())

    def test_hosted_optin_passes_private_policy_then_cleans_it(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory).resolve()
            (directory / 'stock.xml').write_bytes(STOCK)
            python = directory / 'python3'
            python.write_text('#!/bin/sh\nexec "$REAL_PYTHON" "$1" "$STOCK_XML" "$3"\n')
            python.chmod(0o700)
            binary = directory / 'openbox'
            binary.write_text('#!/bin/sh\nprintf "%s\n" "$@" > "$WM_ARGS"\nexec sleep 30\n')
            binary.chmod(0o700)
            probe = directory / 'xprop'
            probe.write_text('#!/bin/sh\nif [ -f "$WM_ARGS" ]; then echo "window id # 0x1"; fi\n')
            probe.chmod(0o700)
            environment = dict(os.environ, PATH=str(directory) + ':' + os.environ['PATH'],
                               REAL_PYTHON=sys.executable, STOCK_XML=str(directory / 'stock.xml'),
                               WM_ARGS=str(directory / 'args'), FEASIBILITY_ZED_MAXIMIZED='1',
                               GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Linux')
            self.explicit_fixture_interpreters(directory, environment)
            result = subprocess.run(['bash', str(ROOT / 'scripts/run-desktop-check-x11.sh'), '/usr/bin/true'],
                                    env=environment, capture_output=True, timeout=3)
            self.assertEqual(result.returncode, 0, result.stderr)
            arguments = (directory / 'args').read_text().splitlines()
            self.assertEqual(arguments[:2], ['--sm-disable', '--config-file'])
            self.assertEqual(len(arguments), 3)
            self.assertFalse(Path(arguments[2]).exists())
            self.assertFalse(Path(arguments[2]).parent.exists())

    def test_default_off_keeps_stock_window_manager_arguments(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            binary = directory / 'openbox'
            binary.write_text('#!/bin/sh\nprintf "%s\\n" "$@" > "$WM_ARGS"\nexec sleep 30\n')
            binary.chmod(0o700)
            probe = directory / 'xprop'
            probe.write_text('#!/bin/sh\nif [ -f "$WM_ARGS" ]; then echo "window id # 0x1"; fi\n')
            probe.chmod(0o700)
            environment = dict(os.environ, PATH=str(directory) + ':' + os.environ['PATH'],
                               WM_ARGS=str(directory / 'args'), FEASIBILITY_ZED_MAXIMIZED='0')
            self.explicit_fixture_interpreters(directory, environment)
            result = subprocess.run(['bash', str(ROOT / 'scripts/run-desktop-check-x11.sh'), '/usr/bin/true'],
                                    env=environment, capture_output=True, timeout=3)
            self.assertEqual(result.returncode, 0)
            self.assertEqual((directory / 'args').read_text(), '--sm-disable\n')

    @staticmethod
    def explicit_fixture_interpreters(directory, environment):
        # Exercise the real wrapper and env credential removal without asking
        # the OS to execute temporary shebang fixtures directly.
        startup = directory / 'shell-environment'
        startup.write_text(
            'env() { [[ "$1" == -u && "$2" == NAN_API_KEY && "$3" == openbox ]] || return 93; command env "$1" "$2" /bin/sh "$SYNTHETIC_OPENBOX/openbox" "${@:4}"; }\n'
            'xprop() { /bin/sh "$SYNTHETIC_OPENBOX/xprop" "$@"; }\n'
            'python3() { /bin/sh "$SYNTHETIC_OPENBOX/python3" "$@"; }\n'
        )
        environment.update(BASH_ENV=str(startup), SYNTHETIC_OPENBOX=str(directory))


if __name__ == '__main__':
    unittest.main()
