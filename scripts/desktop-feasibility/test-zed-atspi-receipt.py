import importlib.util
import json
from pathlib import Path
import tempfile
import sys
import unittest

source = Path(__file__).parents[2] / 'canary/actions/desktop_qualification.py'
sys.path.insert(0, str(Path.cwd() / 'canary/actions'))
spec = importlib.util.spec_from_file_location('q', source)
q = importlib.util.module_from_spec(spec)
spec.loader.exec_module(q)

class ReceiptTests(unittest.TestCase):
    def test_closed_forwarded_and_uncertain_receipts(self):
        value = dict(schemaVersion=1, mechanism='zed-atspi-retry', diagnosticsOnly=True,
                     method='atspi-click', stage='postflight', actionAttempted=True, forwarded=True)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'receipt.json'
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(Path(directory), 'zed-desktop'), [value])
            for change in (dict(secret='private'), dict(method='direct-handler'),
                           dict(actionAttempted=False), dict(stage='preflight'),
                           dict(forwarded='true')):
                path.write_text(json.dumps(dict(value, **change)))
                with self.assertRaises(ValueError):
                    q.semantic_observations(Path(directory), 'zed-desktop')
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                q.semantic_observations(Path(directory), 'claude-desktop')
            value.update(stage='action', forwarded=False)
            path.write_text(json.dumps(value))
            self.assertEqual(q.semantic_observations(Path(directory), 'zed-desktop'), [value])

if __name__ == '__main__':
    unittest.main()
