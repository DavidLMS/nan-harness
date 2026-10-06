#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import unittest

spec=importlib.util.spec_from_file_location('motion',Path(__file__).with_name('zed-xi2-motion.py'))
motion=importlib.util.module_from_spec(spec)
spec.loader.exec_module(motion)


class PayloadTests(unittest.TestCase):
    def sample(self,**change):
        value=dict(window=41,root=1,local=(10.,20.),screen=(110.,220.),
                   axes=b'\x03',buttons=b'\x00',modifiers=0)
        value.update(change)
        return motion.payload_matches(value,41,1,(100,200),{(110,220),(115,225)})

    def test_exact_owned_xy_motion_matches_retained_point(self):
        self.assertEqual(self.sample(),(True,True,True,{(110,220)}))

    def test_scroll_only_buttons_modifier_or_bad_translation_never_match(self):
        for change in [dict(axes=b'\x04'),dict(axes=b''),dict(buttons=b'\x02'),
                       dict(modifiers=1),dict(local=(9.,20.)),dict(screen=(111.,220.))]:
            with self.subTest(change=change):
                self.assertEqual(self.sample(**change)[3],set())

    def test_foreign_identity_malformed_coordinates_and_overlong_masks_deny(self):
        for change in [dict(window=42),dict(root=2),dict(local=(float('nan'),20.)),
                       dict(screen=(110.,float('inf'))),dict(axes=b'\x03'*33),dict(buttons=b'\x00'*33)]:
            with self.subTest(change=change),self.assertRaises(ValueError):
                self.sample(**change)

    def test_fixedpoint_rounding_tolerance_is_bounded(self):
        self.assertEqual(self.sample(screen=(110.+1/131072,220.))[3],{(110,220)})
        self.assertEqual(self.sample(screen=(110.+1/32768,220.))[3],set())

    def test_closed_receipt_has_no_private_payload_or_input_authority(self):
        value=motion.receipt('complete',9,12,10,9,True,True)
        self.assertFalse(value['inputAuthorized'])
        self.assertTrue(value['observerOnly'])
        self.assertFalse({'window','root','local','screen','axes','buttons'} & set(value))


if __name__=='__main__':
    unittest.main()
