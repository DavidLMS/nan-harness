import importlib.util,os,pathlib,sys,tempfile,time,unittest
spec=importlib.util.spec_from_file_location('state',pathlib.Path(__file__).with_name('codex-macos-state.py'));m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
@unittest.skipIf(sys.platform == "win32", "POSIX descriptor-relative transport")
class State(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory();self.root=pathlib.Path(self.tmp.name).resolve();os.chmod(self.root,0o700)
  records=[]
  for suffix in m.SUFFIXES:
   p=self.root/suffix;p.mkdir(mode=0o700,exist_ok=True);mta=p.stat()
   records.append(dict(path=str(p),device=str(mta.st_dev),inode=str(mta.st_ino),uid=mta.st_uid,mode=0o700))
  self.state=self.root/m.SUFFIXES[6]/'.codex-global-state.json';self.state.write_bytes(b'{"synthetic":true}');os.chmod(self.state,0o600)
  self.request=dict(loan=dict(schemaVersion=1,platform='macos',directories=records,stateRootIndex=6,stateBasename='.codex-global-state.json',diagnosticsOnly=True),deadline=time.time()*1000+2000,caller=os.getppid())
 def tearDown(self):self.tmp.cleanup()
 def take(self):return m.snapshot(self.request,platform='darwin')
 def test_descriptor_relative_fixed_file(self):
  v=self.take();self.assertEqual(v['identity']['st_ino'],str(self.state.stat().st_ino))
 def test_wrong_platform(self):
  with self.assertRaises(Exception):m.snapshot(self.request,platform='linux')
 def test_original_deadline(self):
  self.request['deadline']=0
  with self.assertRaises(Exception):self.take()
 def test_wrong_caller(self):
  self.request['caller']=os.getpid()
  with self.assertRaises(Exception):self.take()
 def test_symlink_state(self):
  self.state.rename(self.state.with_suffix('.other'));self.state.symlink_to(self.state.with_suffix('.other'))
  with self.assertRaises(Exception):self.take()
 def test_hardlinked_state(self):
  os.link(self.state,self.state.with_suffix('.other'))
  with self.assertRaises(Exception):self.take()
 def test_replaced_retained_parent(self):
  p=self.root/m.SUFFIXES[6];p.rename(p.with_name('old'));p.mkdir(mode=0o700)
  with self.assertRaises(Exception):self.take()
 def test_nonprivate_state(self):
  os.chmod(self.state,0o644)
  with self.assertRaises(Exception):self.take()
 def test_mode_changed_original_root(self):
  os.chmod(self.root/m.SUFFIXES[4],0o755)
  with self.assertRaises(Exception):self.take()
if __name__=='__main__':unittest.main()
