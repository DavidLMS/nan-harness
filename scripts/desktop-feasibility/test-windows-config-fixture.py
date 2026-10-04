import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('fixture', Path(__file__).with_name('windows-config-fixture.py'))
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


class ClosedFixtureTests(unittest.TestCase):
    def test_categories_never_export_private_output(self):
        start = ('test ' + fixture.TEST + ' ... ').encode()
        for output, category in ((b'error: could not compile PRIVATE', 'compile-failure'),
                (b'error: could not execute process PRIVATE', 'test-process-unavailable'),
                (start + b'panicked at PRIVATE os error 32', 'fixture-panic-sharing-violation'),
                (start + b'panicked at PRIVATE os error 5', 'fixture-panic-access-denied'),
                (start + b'PRIVATE FAILED', 'fixture-failure'),
                (b'PRIVATE', 'unclassified-failure')):
            observation = fixture.classify(output, False)
            self.assertEqual(observation['category'], category)
            self.assertNotIn('PRIVATE', json.dumps(observation))
        self.assertEqual(fixture.classify(start + b'PRIVATE ok', True)['category'], 'passed')
        self.assertEqual(fixture.classify(b'PRIVATE' * 20000, False)['category'], 'output-overflow')


if __name__ == '__main__':
    unittest.main()
