#!/usr/bin/env python3
"""Tampered decoders cannot enter the native response oracle."""
import importlib.util
import io
from pathlib import Path
import unittest
from unittest.mock import patch
import zipfile

spec = importlib.util.spec_from_file_location('decoder', Path(__file__).with_name('install-zstd-windows.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class Decoder(unittest.TestCase):
    def test_wrong_digest_and_oversized_bytes_fail_before_zip_access(self):
        with patch.object(zipfile, 'ZipFile') as archive:
            for value in [b'tampered native decoder', b'x' * (module.LIMIT + 1)]:
                with self.assertRaises(ValueError):
                    module.extract(value)
            archive.assert_not_called()

    def test_verified_archive_stages_only_the_exact_unique_entry(self):
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, 'w') as archive:
            archive.writestr('../../foreign-path', b'foreign synthetic file')
            archive.writestr(module.ENTRY, b'synthetic decoder')
        with patch.object(module.hashlib, 'sha256') as digest:
            digest.return_value.hexdigest.return_value = module.SHA256
            self.assertEqual(module.extract(stream.getvalue()), b'synthetic decoder')


if __name__ == '__main__':
    unittest.main()
