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

    def test_compiler_details_are_fixed_codes_or_closed_stage_without_payload(self):
        suffix = b'\nerror: could not compile PRIVATE-COMPONENT due to errors\n'
        for output, expected in (
                (b'error[E0308]: PRIVATE-VALUE\nerror[E0425]: PRIVATE-PATH\n',
                 dict(category='rustc-code',rustcCodes=['E0308','E0425'],otherRustcCode=False,linkStage=False)),
                (b'error[E9999]: PRIVATE\nerror[E0308]: PRIVATE\nerror[E0308]: PRIVATE\n',
                 dict(category='rustc-code',rustcCodes=['E0308'],otherRustcCode=True,linkStage=False)),
                (b'\x1b[31merror[E0599]\x1b[0m: PRIVATE\n',
                 dict(category='rustc-code',rustcCodes=['E0599'],otherRustcCode=False,linkStage=False)),
                (b'error: linking with "PRIVATE-LINKER-PATH" failed: exit code: 1\n',
                 dict(category='link-stage',rustcCodes=[],otherRustcCode=False,linkStage=True)),
                (b'LINK : fatal error LNK1104: PRIVATE-FILE\n',
                 dict(category='link-stage',rustcCodes=[],otherRustcCode=False,linkStage=True)),
                (b'PRIVATE-PATH error[E0425]: quoted output\nerror[PRIVATE]: no public code\n',
                 dict(category='no-code',rustcCodes=[],otherRustcCode=False,linkStage=False))):
            result=fixture.classify(output+suffix,False)
            self.assertEqual(result['compileDiagnostics'],expected)
            self.assertFalse(result['fixtureStarted'])
            self.assertNotIn('PRIVATE',json.dumps(result))
            self.assertNotIn('E9999',json.dumps(result))
        self.assertNotIn('compileDiagnostics',fixture.classify(b'PRIVATE',False))
        self.assertNotIn('compileDiagnostics',fixture.classify(b'error[E0308]: PRIVATE'+suffix,True))
        self.assertNotIn('compileDiagnostics',fixture.classify(b'PRIVATE'*20000,False))



if __name__ == '__main__':
    unittest.main()
