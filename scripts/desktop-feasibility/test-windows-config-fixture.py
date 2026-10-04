import importlib.util
import json
import io
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
            self.assertEqual({k:v for k,v in result['compileDiagnostics'].items() if k!='structured'},dict(expected,noCodeSignals=[]))
            self.assertFalse(result['fixtureStarted'])
            self.assertNotIn('PRIVATE',json.dumps(result))
            self.assertNotIn('E9999',json.dumps(result))
        self.assertNotIn('compileDiagnostics',fixture.classify(b'PRIVATE',False))
        self.assertNotIn('compileDiagnostics',fixture.classify(b'error[E0308]: PRIVATE'+suffix,True))
        self.assertNotIn('compileDiagnostics',fixture.classify(b'PRIVATE'*20000,False))



    def test_no_code_signatures_are_closed_and_anchored(self):
        cases = (
            (b'error: failed to run custom build command for `PRIVATE`', 'build-script'),
            (b'error: environment variable `PRIVATE` not defined at compile time', 'environment-variable'),
            (b"error: couldn't read PRIVATE: Access is denied. (os error 5)", 'read-file'),
            (b'error: failed to write PRIVATE', 'write-file'),
            (b'error: could not remove PRIVATE', 'remove-file'),
            (b'error: could not execute process PRIVATE', 'spawn-process'),
            (b'error: linker `PRIVATE` not found', 'linker-unavailable'),
            (b'error: could not open output file PRIVATE', 'output-file'),
            (b'error: failed to build archive at PRIVATE', 'archive-file'),
            (b"error: couldn't create a temp dir: PRIVATE", 'temporary-directory'),
            (b'error: failed to emit PRIVATE', 'emit-output'),
        )
        for output, signal in cases:
            result = fixture.compile_diagnostics(output)
            self.assertEqual(result['noCodeSignals'], [signal])
            self.assertEqual(result['category'], 'no-code')
            self.assertNotIn('PRIVATE', json.dumps(result))
            self.assertEqual(fixture.compile_diagnostics(b'PRIVATE '+output)['noCodeSignals'], [])
        self.assertEqual(fixture.compile_diagnostics(b'error: PRIVATE unknown error')['noCodeSignals'], [])
        result = fixture.compile_diagnostics(b'error: failed to read PRIVATE metadata')
        self.assertEqual(result['noCodeSignals'], ['metadata-file', 'read-file'])


    def test_structured_compiler_messages_ignore_rendered_paths_and_unknown_codes(self):
        def record(message,level='error',code=None,children=None):
            return dict(reason='compiler-message',package_id='PRIVATE-PACKAGE',
                target={'src_path':'PRIVATE-PATH'},message=dict(message=message,level=level,
                code=code,children=children or [],spans=[{'file_name':'PRIVATE-SECRET'}],
                rendered='error[E0425]: PRIVATE rendered only'))
        def encoded(*records):
            return b'\n'.join(json.dumps(r).encode() for r in records)+b'\n'
        data=encoded(record("couldn't read PRIVATE: os error 5"),
            record('PRIVATE coded',code={'code':'E0308','explanation':'PRIVATE'}),
            record('PRIVATE unknown',code={'code':'E9999'}),
            {'reason':'build-finished','success':False})
        result=fixture.classify(data,False)
        self.assertEqual(result['category'],'compile-failure')
        details=result['compileDiagnostics']
        self.assertEqual(details['rustcCodes'],['E0308'])
        self.assertTrue(details['otherRustcCode'])
        self.assertEqual(details['noCodeSignals'],['read-file'])
        self.assertEqual(details['structured']['levels']['error'],3)
        self.assertEqual(details['structured']['buildFinished'],'failed')
        self.assertFalse(details['structured']['malformed'])
        for private in ['PRIVATE','E9999','E0425','os error 5','couldn']:
            self.assertNotIn(private,json.dumps(result))
        warning=encoded(record('PRIVATE warning',level='warning',code={'code':'unused_variables'}),
            {'reason':'build-finished','success':True})
        details=fixture.compile_diagnostics(warning)
        self.assertFalse(details['structured']['malformed'])
        self.assertFalse(details['structured']['otherErrorMessage'])
        self.assertEqual(details['structured']['levels']['warning'],1)
        self.assertEqual(fixture.classify(warning,False)['category'],'unclassified-failure')
        self.assertEqual(fixture.classify(data,True)['category'],'fixture-not-run')

    def test_structured_unknown_error_children_and_malformed_records_are_bounded(self):
        child=dict(message='PRIVATE note',level='note',code=None,children=[],rendered='PRIVATE')
        record=dict(reason='compiler-message',message=dict(message='PRIVATE unknown',
            level='error',code=None,children=[child],rendered='PRIVATE'))
        data=json.dumps(record).encode()+b'\n'
        result=fixture.classify(data,False)
        details=result['compileDiagnostics']['structured']
        self.assertTrue(details['otherErrorMessage'])
        self.assertEqual(details['diagnosticCount'],2)
        self.assertEqual(details['levels']['note'],1)
        for malformed in [b'{PRIVATE\n',json.dumps({'reason':'compiler-message','message':None}).encode(),
            json.dumps({'reason':'build-finished','success':'PRIVATE'}).encode(),
            json.dumps({'reason':'compiler-message','message':{'level':{},'message':'PRIVATE'}}).encode(),
            json.dumps({'reason':'compiler-message','message':{'level':'error','message':'\ud800'}}).encode()]:
            facts=fixture.compile_diagnostics(malformed)['structured']
            self.assertTrue(facts['malformed'])
            self.assertNotIn('PRIVATE',json.dumps(facts))
        details=fixture.compile_diagnostics(data*200)['structured']
        self.assertEqual(details['diagnosticCount'],256)
        self.assertTrue(details['malformed'])
        self.assertEqual(fixture.classify(b'PRIVATE'*20000,False)['category'],'output-overflow')

    def test_private_reader_filters_artifacts_before_bounded_diagnostics(self):
        artifact=json.dumps(dict(reason='compiler-artifact',filenames=['PRIVATE']*30)).encode()+b'\n'
        error=json.dumps(dict(reason='compiler-message',message=dict(message='PRIVATE error',
            level='error',code={'code':'E0308'},children=[],rendered='PRIVATE'))).encode()+b'\n'
        result=fixture.classify(fixture.read_private_output(io.BytesIO(artifact*2000+error)),False)
        self.assertEqual(result['category'],'compile-failure')
        self.assertEqual(result['compileDiagnostics']['rustcCodes'],['E0308'])
        self.assertNotIn('PRIVATE',json.dumps(result))
        for output in [b'PRIVATE'*22000,b'PRIVATE\n'*20000]:
            self.assertEqual(fixture.classify(fixture.read_private_output(io.BytesIO(output)),False)['category'],
                'output-overflow')

if __name__ == '__main__':
    unittest.main()
