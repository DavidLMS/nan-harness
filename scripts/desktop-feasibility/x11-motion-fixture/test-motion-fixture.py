import importlib.util
from pathlib import Path
import unittest
spec=importlib.util.spec_from_file_location('fixture',Path(__file__).with_name('motion-fixture.py'))
f=importlib.util.module_from_spec(spec);spec.loader.exec_module(f)
class Tests(unittest.TestCase):
 def test_exact_gpui_axes_predicate(self):
  for mask in (b'\x01',b'\x02',b'\x03',b'\x83'):
   self.assertTrue(f.motion_axes(mask))
  for mask in (b'',b'\x00',b'\x04',b'\xfc',b'\x00\x03'):
   self.assertFalse(f.motion_axes(mask))
 def test_malformed_mask(self):
  for mask in (b'\0'*33,[],True,None):
   with self.assertRaises(ValueError):f.motion_axes(mask)
 def test_closed_initial_counts(self):
  self.assertEqual(f.empty_counts(),dict(motionCount=0,motionAxesCount=0,motionWithoutAxesCount=0,normalEnterCount=0,pointerSampleCount=0,pointerOwnedCount=0))
unittest.main()
