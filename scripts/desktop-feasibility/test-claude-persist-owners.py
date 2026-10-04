import importlib.util
import json
from pathlib import Path
import unittest
import tempfile
from unittest.mock import patch

spec=importlib.util.spec_from_file_location('owners',Path(__file__).with_name('claude-persist-owners.py'))
owners=importlib.util.module_from_spec(spec);spec.loader.exec_module(owners)

class OwnersTests(unittest.TestCase):
    def test_identity_counts_are_complete_and_creation_bound(self):
        self.assertEqual(owners.classify_owners([(1,11),(2,22)],(1,11),lambda p:{1:11,2:22}[p]),
                         dict(ownerCount=2,currentProcessCount=1,otherProcessCount=1))
        self.assertEqual(owners.classify_owners([],(1,11),lambda p:None)['ownerCount'],0)
        for values,live in [([(1,11)],lambda p:12), ([(2,22)],lambda p:None),
                            ([(1,11),(1,11)],lambda p:11), ([(0,1)],lambda p:1),
                            ([(i+1,i+1) for i in range(65)],lambda p:p)]:
            with self.assertRaises(ValueError):owners.classify_owners(values,(1,11),live)

    def test_request_is_closed_and_deadline_is_not_renewed(self):
        value=dict(workspace='C:\\owned',temporary='C:\\owned\\.nan-file',destination='C:\\owned\\config',cliPid=1,deadlineMs=1200)
        with patch.object(owners,'current_milliseconds',return_value=1000):
            self.assertEqual(owners.parse_request(json.dumps(value)),value)
            for invalid in ({**value,'deadlineMs':1000},{**value,'deadlineMs':46001},
                            {**value,'cliPid':True},{**value,'PRIVATE':'secret'}, {**value,'temporary':'bad\0path'}):
                with self.assertRaises(ValueError):owners.parse_request(json.dumps(invalid))
            with self.assertRaises(ValueError):owners.parse_request('x'*4097)
            with self.assertRaises(ValueError):owners.parse_request(json.dumps(value)[:-1]+',"cliPid":2}')

    def test_resource_scope_rejects_links_and_retains_file_identity(self):
        with tempfile.TemporaryDirectory() as tmp:
            workspace=Path(tmp).resolve();parent=workspace/'profile/home/AppData/Roaming/Claude'
            parent.mkdir(parents=True);temporary=parent/'.nan-owned';temporary.write_text('fixture')
            destination=parent/'claude_desktop_config.json'
            value=dict(workspace=str(workspace),temporary=str(temporary),destination=str(destination))
            before=owners.owned_files(value)
            self.assertFalse(before[2])
            replacement=parent/'.replacement';replacement.write_text('different')
            replacement.replace(temporary)
            self.assertNotEqual(owners.owned_files(value)[3],before[3])
            temporary.unlink();temporary.symlink_to(destination)
            with self.assertRaises(ValueError):owners.owned_files(value)
            with self.assertRaises(ValueError):owners.owned_files({**value,'destination':str(workspace/'foreign')})

    def test_api_surface_never_requests_application_shutdown(self):
        source=Path(owners.__file__).read_text()
        for forbidden in ('RmShutdown','RmRestart','TerminateProcess','PROCESS_TERMINATE'):
            self.assertNotIn(forbidden,source)

if __name__=='__main__':unittest.main()
