#!/usr/bin/env python3
"""Synthetic renderer counter parsing and publication boundaries."""
import json
from pathlib import Path
import sys
import re
import unittest

from zed_render_counts import SYMBOLS, probes, seeds, split_counts
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'canary/actions'))
from desktop_qualification import validate_overlay_render_counts
from zed_retry_trace import program


def encoded(values):
    return (json.dumps(dict(type='map', data={'@renders': values})) + '\n').encode()


class RenderCountTests(unittest.TestCase):
    def test_separates_lifetime_counts_from_activation_counts(self):
        values = {f'{index},{slot}': 1 for index in range(7) for slot in range(4)}
        values.update({'4,0': 4, '4,1': 2, '4,2': 3})
        ordinary = b'{"type":"map","data":{"@input":9}}\n'
        remaining, result = split_counts(encoded(values) + ordinary)
        self.assertEqual(remaining.strip(), ordinary.strip())
        self.assertEqual(result['totals']['zedPrompt'], 3)
        self.assertEqual([v['zedPrompt'] for v in result['windows']], [1, 2, 0])
        validate_overlay_render_counts(result, 3)
        for change in ({'4,0': 0}, {'4,0': True}, {'4,0': 65538}, {'4,0': 2}, {'PRIVATE': 1}):
            with self.assertRaises(ValueError):
                split_counts(encoded({**values, **change}))
        for invalid in (encoded(values) + encoded(values), encoded({}), b'PRIVATE'):
            with self.assertRaises(ValueError):
                split_counts(invalid)

    def test_closed_reducer_rejects_native_text_partial_counts_and_invalid_windows(self):
        counts = dict.fromkeys(SYMBOLS, 0)
        good = dict(totals=counts, windows=[counts] * 3)
        validate_overlay_render_counts(good, 3)
        for invalid in ({**good, 'text': 'PRIVATE'}, {**good, 'windows': []},
                        {**good, 'totals': {**counts, 'zedPrompt': True}},
                        {**good, 'windows': [{**counts, 'zedPrompt': 1}] * 3}):
            with self.assertRaises(ValueError):
                validate_overlay_render_counts(invalid, 3)

    def test_probes_count_entries_without_native_arguments_or_stacks(self):
        source = seeds() + probes('/owned/zed-editor')
        self.assertEqual(source.count('uprobe:'), 7)
        for forbidden in ('arg0', 'arg1', 'uptr', 'ustack', 'printf', 'str('):
            self.assertNotIn(forbidden, source)

    def test_complete_initializer_reserves_capacity_under_kernel_map_limit(self):
        source = program('/owned/zed-editor', Path('/owned/markers'), True)
        maps = set(re.findall(r'@[A-Za-z][A-Za-z0-9_]*', source.splitlines()[0]))
        # Kernel MAX_USED_MAPS is 64; leave room for bpftrace's internal maps.
        self.assertLessEqual(len(maps), 60)


if __name__ == '__main__':
    unittest.main()
