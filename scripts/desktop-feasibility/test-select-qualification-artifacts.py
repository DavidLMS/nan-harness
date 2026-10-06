#!/usr/bin/env python3
"""Reruns must select fresh evidence, never an older successful outcome."""
import copy
import hashlib
import io
import tempfile
import zipfile
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('selection', Path(__file__).with_name('select-qualification-artifacts.py'))
selection = importlib.util.module_from_spec(spec)
spec.loader.exec_module(selection)
SHA = 'a' * 40


def fixture():
    artifacts = [dict(id=index + 1, name=f"deterministic-qualification-{cell['app']}-{cell['platform']}",
                      created_at='2026-10-06T10:00:00Z', expired=False,
                      workflow_run=dict(id=20, head_sha=SHA))
                 for index, cell in enumerate(selection.matrix(['pen-desktop'])['include'])]
    return [{'artifacts': artifacts[:6]}, {'artifacts': artifacts[6:]}]


class ArtifactSelectionTests(unittest.TestCase):
    def test_paginated_unordered_reruns_select_latest_even_if_failed(self):
        pages = fixture()
        old = pages[0]['artifacts'][0]
        old['outcome'] = 'passed'
        new = {**old, 'id': 101, 'created_at': '2026-10-06T11:00:00Z', 'outcome': 'failed'}
        pages[1]['artifacts'].insert(0, new)
        pages[0]['artifacts'].insert(0, {'name': 'deterministic-qualification-matrix'})
        chosen = selection.select(list(reversed(pages)), 20, SHA)
        self.assertEqual(len(chosen), 12)
        self.assertIn(101, chosen)
        self.assertNotIn(old['id'], chosen)

    def test_no_fallback_to_expired_missing_foreign_or_ambiguous_evidence(self):
        for mutation in ('expired', 'missing', 'foreign-run', 'foreign-sha', 'duplicate', 'tie', 'date', 'id'):
            with self.subTest(mutation=mutation):
                pages = fixture()
                row = pages[0]['artifacts'][0]
                if mutation == 'expired':
                    pages[1]['artifacts'].append({**row, 'id': 101, 'created_at': '2026-10-06T11:00:00Z', 'expired': True})
                elif mutation == 'missing': pages[0]['artifacts'].pop(0)
                elif mutation == 'foreign-run': row['workflow_run']['id'] = 21
                elif mutation == 'foreign-sha': row['workflow_run']['head_sha'] = 'b' * 40
                elif mutation == 'duplicate': pages[1]['artifacts'].append(copy.deepcopy(row))
                elif mutation == 'tie': pages[1]['artifacts'].append({**row, 'id': 101})
                elif mutation == 'date': row['created_at'] = 'PRIVATE'
                elif mutation == 'id': row['id'] = True
                with self.assertRaises(ValueError): selection.select(pages, 20, SHA)


class ExactDownloadTests(unittest.TestCase):
    def archive(self, entries):
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, 'w') as bundle:
            for name, payload in entries:
                bundle.writestr(name, payload)
        return stream.getvalue()

    def prepared(self, archive):
        pages = fixture()
        for page in pages:
            for item in page['artifacts']:
                item.update(digest='sha256:' + hashlib.sha256(archive).hexdigest(), size_in_bytes=len(archive))
        return pages

    def test_exact_ids_download_all_attempts_without_a_second_latest_filter(self):
        archive = self.archive([('qualification.json', b'{"closed":true}')])
        pages = self.prepared(archive)
        new = {**pages[0]['artifacts'][0], 'id': 101, 'created_at': '2026-10-06T11:00:00Z'}
        pages[1]['artifacts'].append(new)
        fetched = []
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / 'cells'
            selection.download(pages, 20, SHA, target, lambda identifier: fetched.append(identifier) or archive)
            self.assertEqual(set(fetched), set(selection.select(pages, 20, SHA)))
            self.assertEqual(len(list(target.glob('*/qualification.json'))), 12)
            self.assertIn(101, fetched)
            self.assertNotIn(1, fetched)

    def test_rejects_changed_digest_size_and_unclosed_archive_members(self):
        cases = [self.archive([('../qualification.json', b'{}')]),
                 self.archive([('qualification.json', b'{}'), ('raw-log.txt', b'PRIVATE')])]
        for archive in cases:
            with tempfile.TemporaryDirectory() as tmp, self.assertRaises(ValueError):
                selection.download(self.prepared(archive), 20, SHA, Path(tmp) / 'cells', lambda _: archive)
        archive = self.archive([('qualification.json', b'{}')])
        for field, value in [('digest', 'sha256:' + '0' * 64), ('size_in_bytes', len(archive) + 1),
                             ('digest', None), ('size_in_bytes', True)]:
            pages = self.prepared(archive)
            for page in pages:
                for item in page['artifacts']: item[field] = value
            with tempfile.TemporaryDirectory() as tmp, self.assertRaises(ValueError):
                selection.download(pages, 20, SHA, Path(tmp) / 'cells', lambda _: archive)


if __name__ == '__main__':
    unittest.main()
