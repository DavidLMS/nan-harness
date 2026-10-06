#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('session', Path(__file__).with_name('windows-session.py'))
session = importlib.util.module_from_spec(spec)
spec.loader.exec_module(session)


class SessionPrivacy(unittest.TestCase):
    def test_only_typed_closed_desktop_facts_are_publishable(self):
        value = dict(schemaVersion=1, mechanism='windows-session', diagnosticsOnly=True,
                     userInteractive=True, sameConsoleSession=True, foregroundPresent=False,
                     displayCount=1, uiaRootPresent=True, uiaChildCount=0,
                     errorCategory=None, errorCode=None)
        self.assertEqual(session.validate(value), value)
        for changed in ({**value, 'windowNames': 'PRIVATE'}, {**value, 'errorCode': 'PRIVATE'},
                        {**value, 'uiaChildCount': True}, {**value, 'displayCount': 33},
                        {**value, 'errorCategory': 'PRIVATE'}):
            with self.assertRaises(ValueError):
                session.validate(changed)


if __name__ == '__main__':
    unittest.main()
