import io
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

from publish_extensions import CONFIRMATION, PACKAGES, archive_contents, cargo_command, retry_deadline, run


class ExtensionPublicationTests(unittest.TestCase):
    def test_upload_requires_exact_confirmation_and_token_before_any_command(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'repo'
            for token, confirmation in [('', CONFIRMATION), ('test-token', ''), ('test-token', 'publish all')]:
                with patch.dict(os.environ, {'CARGO_REGISTRY_TOKEN': token}), \
                     patch('publish_extensions.subprocess.check_output', side_effect=AssertionError('unexpected command')):
                    with self.assertRaisesRegex(ValueError, 'confirmation'):
                        run(root, Path(directory) / 'out', publish=True, confirmation=confirmation)
            self.assertFalse((Path(directory) / 'out').exists())

    def test_output_inside_checkout_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(ValueError, 'outside'):
                run(root, root / 'out')

    def test_commands_default_to_dry_run_and_upload_locks_dependencies(self):
        self.assertIn('--dry-run', cargo_command('Cargo.toml'))
        self.assertNotIn('--dry-run', cargo_command('Cargo.toml', publish=True))
        self.assertIn('--locked', cargo_command('Cargo.toml', publish=True))
        self.assertEqual(cargo_command('Cargo.toml', package_only=True)[1], 'package')
        with self.assertRaises(ValueError):
            cargo_command('Cargo.toml', publish=True, package_only=True)

    def test_retries_only_explicit_rate_limit_deadline(self):
        deadline = 'Please try again after Thu, 29 Oct 2026 01:23:06 GMT'
        self.assertIsNotNone(retry_deadline('429 Too Many Requests. ' + deadline))
        for output in ['403 ' + deadline, '401 unauthorized', '429 Too Many Requests',
                       '429 try again after invalid GMT', 'command timed out']:
            self.assertIsNone(retry_deadline(output))

    def test_fixed_scope_has_dependency_order(self):
        self.assertEqual(len(PACKAGES), 6)
        for child, parent in [('xgpui-gamut', 'xgpui-color'), ('xgpui-gamut', 'xgpui-shader'),
                              ('xgpui-media', 'xgpui-color'), ('xgpui-media', 'xgpui-shader')]:
            self.assertLess(PACKAGES.index(parent), PACKAGES.index(child))

    def test_archive_comparison_rejects_duplicate_and_traversal_entries(self):
        for names in [('a', 'a'), ('../a',)]:
            stream = io.BytesIO()
            with tarfile.open(fileobj=stream, mode='w:gz') as bundle:
                for name in names:
                    entry = tarfile.TarInfo(name)
                    entry.size = 1
                    bundle.addfile(entry, io.BytesIO(b'a'))
            with self.assertRaises(ValueError):
                archive_contents(stream.getvalue())


if __name__ == '__main__':
    unittest.main()
