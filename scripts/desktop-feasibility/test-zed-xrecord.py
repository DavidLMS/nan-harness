import struct, sys, unittest
from pathlib import Path
import runpy
Counts = runpy.run_path(str(Path(__file__).with_name('zed-xrecord.py')))['Counts']

def event(kind=4, window=90, stamp=100, device=2):
    return struct.pack(('<' if sys.byteorder == 'little' else '>') + 'BBHIHHIIIII', 35, 131, 0, 12, kind, device, stamp, 1, 1, window, 0)

class Tests(unittest.TestCase):

    def test_owned_ordered_delivery_and_private_reduction(self):
        c = Counts(90, 131, 123)
        c.accept(0, False, 123, event())
        c.accept(0, False, 123, event(5, stamp=110))
        self.assertEqual(c.closed(), {'pressCount': 1, 'releaseCount': 1, 'orderedPair': True})

    def test_foreign_window_cannot_certify_pair(self):
        c = Counts(90, 131, 123)
        c.accept(0, False, 123, event(window=91))
        c.accept(0, False, 123, event(5, stamp=110))
        self.assertFalse(c.closed()['orderedPair'])

    def test_reversed_duplicate_or_identity_uncertainty_fails(self):
        for sequence in [(event(5), event()), (event(), event(), event(5)), (event(), event(5, device=3))]:
            c = Counts(90, 131, 123)
            for data in sequence:
                c.accept(0, False, 123, data)
            self.assertFalse(c.closed()['orderedPair'])
        for swap, base, data in [(True, 123, event()), (False, 124, event()), (False, 123, b'private')]:
            c = Counts(90, 131, 123)
            c.accept(0, swap, base, data)
            c.accept(0, False, 123, event())
            c.accept(0, False, 123, event(5, stamp=110))
            self.assertFalse(c.closed()['orderedPair'])

    def test_byte_budget_never_approves(self):
        c = Counts(90, 131, 123)
        for _ in range(129):
            c.accept(0, False, 123, event(window=91))
        c.accept(0, False, 123, event())
        c.accept(0, False, 123, event(5, stamp=110))
        self.assertFalse(c.closed()['orderedPair'])
if __name__ == '__main__':
    unittest.main()
