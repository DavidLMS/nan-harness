import importlib.util
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
import tempfile
import unittest
spec=importlib.util.spec_from_file_location('runner',Path(__file__).with_name('run-qualification.py'))
q=importlib.util.module_from_spec(spec);spec.loader.exec_module(q)
class ReachedExecution(Exception):pass
class Tests(unittest.TestCase):
 def test_run_admits_only_pinned_zed_after_prepared_identity_validation(self):
  with tempfile.TemporaryDirectory() as tmp:
   root=Path(tmp);args=SimpleNamespace(app='zed-desktop',platform='linux',source_sha='a'*40,frozen=root/'frozen',prepared=root/'prepared',checker=root/'checker',directory=root/'output',real_nanh=root/'nanh')
   exe=root/'zed';sha='443670f58a31e7410d0cec0577dd4ebbea6af27e53252c4e6f43d1715d798ba4';release={'status':'frozen','version':'1.22.0','digest':'sha256:5ce3991b34a8fad0a23625f5821cda601c7150a6cc69683c097b8d1b083abc50'}
   prepared={'schemaVersion':2,'platform':'linux','architecture':'x86_64','checker':{'sha256':'fixed'},'nanh':{'sha256':'fixed'},'frozen':{'sha256':'fixed','model':'qwen3.6'},'apps':[{'app':'zed-desktop','executable':{'path':str(exe),'sha256':sha}}]}
   env={'RUNNER_OS':'Linux'}
   def execute(*values):
    self.assertEqual(values[-1]['NANH_ZED_PANEL_LAYOUT'],'fixed-wide-compact');self.assertEqual(values[-1]['NANH_ZED_PANEL_SOURCE_POLICY'],'official-1.22.0');raise ReachedExecution()
   with patch.dict(q.os.environ,{'NANH_ZED_PANEL_LAYOUT':'fixed-wide-compact'},clear=True),patch.object(q.sys,'platform','linux'),patch.object(q,'envelope'),patch.object(q,'cell',return_value={'backend':'native'}),patch.object(q,'read_frozen_manifest',return_value={'apps':[release]}),patch.object(q,'bounded_json',return_value=prepared),patch.object(q,'digest',side_effect=lambda p:sha if Path(p)==exe else 'fixed'),patch.object(q,'ensure_private_directory'),patch.object(q,'qualification_environment',return_value=env),patch.object(q,'execute_with_diagnostics',side_effect=execute) as dispatch:
    with self.assertRaises(ReachedExecution):q.run(args)
    self.assertEqual(dispatch.call_count,1)
    release['version']='1.23.0'
    with self.assertRaisesRegex(ValueError,'source differs'):q.run(args)
    self.assertEqual(dispatch.call_count,1)
 def test_settings_trial_conflicts_and_defaults_are_closed(self):
  source={'GITHUB_ACTIONS':'true','RUNNER_ENVIRONMENT':'github-hosted','RUNNER_OS':'Linux','NANH_ZED_PANEL_LAYOUT':'fixed-wide-compact'}
  source.update({key:'/synthetic/helper' for key in q.ZED_HELPERS})
  source.update(FEASIBILITY_ZED_INPUT_DRIVER_MODE='paste',FEASIBILITY_ZED_RESPONSE_METHOD='thread-export')
  result=q.qualification_environment('zed-desktop',Path('/facts'),Path('/nanh'),Path('/zed'),source)
  self.assertNotIn('NANH_ZED_PANEL_LAYOUT',result) # only release-admitted run forwards it
  for change in [{'RUNNER_OS':'Windows'},{'NANH_ZED_PANEL_LAYOUT':'arbitrary'},{'NANH_ZED_LAYOUT_POLICY':'zoom-before-send'},{'NANH_ZED_PANEL_ZOOM':'observe'}]:
   with self.subTest(change=change),self.assertRaises(ValueError):q.qualification_environment('zed-desktop',Path('/facts'),Path('/nanh'),Path('/zed'),{**source,**change})
  with self.assertRaises(ValueError):q.qualification_environment('claude-desktop',Path('/facts'),Path('/nanh'),Path('/zed'),source)
 def test_claude_release_guards_run_before_prepared_lookup(self):
  for platform,flag in [('macos','NANH_CLAUDE_MAC_PROFILE_POLICY'),('windows','NANH_CLAUDE_WINDOWS_PROFILE_POLICY')]:
   args=SimpleNamespace(app='claude-desktop',platform=platform,source_sha='a'*40,frozen=Path('/synthetic'))
   with patch.dict(q.os.environ,{flag:'synthetic'},clear=True),patch.object(q,'envelope'),patch.object(q,'cell',return_value={'backend':'native'}),patch.object(q,'digest',return_value='fixed'),patch.object(q,'read_frozen_manifest',return_value={'apps':[{'status':'frozen','version':'wrong','digest':'wrong'}]}),patch.object(q,'bounded_json') as prepared:
    with self.assertRaises(ValueError):q.run(args)
    prepared.assert_not_called()
unittest.main()
