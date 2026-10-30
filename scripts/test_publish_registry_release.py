import io
import json
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import publish_registry_release as release

SHA = 'a' * 40


def archive(name='sample', version='1.0.0', source=b'pub fn sample() {}', lock=b'one', pin='1'):
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode='w:gz') as bundle:
        for path, data in {
            'Cargo.toml': f'[package]\nname="{name}"\nversion="{version}"\n[dependencies]\nother="{pin}"\n'.encode(),
            'Cargo.toml.orig': b'original manifest', 'Cargo.lock': lock,
            '.cargo_vcs_info.json': b'{}', 'src/lib.rs': source,
        }.items():
            entry = tarfile.TarInfo(f'{name}-{version}/{path}')
            entry.size = len(data)
            bundle.addfile(entry, io.BytesIO(data))
    return stream.getvalue()


class RegistryReleaseTests(unittest.TestCase):
    def test_payload_ignores_generated_state_but_retains_code_and_dependency_identity(self):
        self.assertEqual(release.payload(archive(lock=b'old'), 'sample', '1.0.0'),
                         release.payload(archive(lock=b'new'), 'sample', '1.0.0'))
        for blob in (archive(source=b'changed'), archive(pin='2')):
            self.assertNotEqual(release.payload(archive(), 'sample', '1.0.0'),
                                release.payload(blob, 'sample', '1.0.0'))
        with self.assertRaises(ValueError):
            release.payload(archive(), 'other', '1.0.0')

    def test_existing_version_requires_registry_identity_and_no_yank(self):
        for entry in ({'crate': 'other', 'num': '1.0.0', 'yanked': False},
                      {'crate': 'sample', 'num': '1.0.0', 'yanked': True}):
            with patch.object(release, 'fetch', return_value=json.dumps({'version': entry}).encode()):
                with self.assertRaises(ValueError):
                    release.registry_version('sample', '1.0.0')
        with patch.object(release, 'fetch', return_value=None):
            self.assertIsNone(release.registry_version('sample', '1.0.0'))

    def test_registry_checksum_verified_before_payload_comparison(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'sample.crate'
            path.write_bytes(archive())
            with patch.object(release, 'fetch', return_value=archive()):
                with self.assertRaisesRegex(ValueError, 'checksum'):
                    release.compare_published('sample', '1.0.0', path, {'checksum': 'wrong'})

    def test_publish_requires_exact_commit_confirmation_and_token(self):
        for token, confirmation in [('', 'publish ' + SHA), ('secret', ''), ('secret', 'publish other')]:
            with tempfile.TemporaryDirectory() as directory, \
                    patch.dict(os.environ, {'CARGO_REGISTRY_TOKEN': token}), \
                    patch.object(release.subprocess, 'check_output', return_value=SHA), \
                    patch.object(release, 'release_plan', side_effect=AssertionError('plan should not run')):
                with self.assertRaisesRegex(ValueError, 'confirmation'):
                    release.run(Path(directory) / 'repo', Path(directory) / 'out', 'publish', confirmation)

    def exercise(self, mode, existing, changes=None, disabled=False):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root, out = Path(directory.name) / 'repo', Path(directory.name) / 'out'
        for path in ('gpui/crates', 'extensions/crates'):
            (root / path).mkdir(parents=True)
        rows = [dict(name=name, version='1.0.0', manifest=f'gpui/crates/{name}/Cargo.toml',
                     dependencies=[] if name == 'parent' else ['parent'], publish_policy=not disabled)
                for name in ('parent', 'child')]
        verified, uploaded = [], []
        present = set(existing)
        def package(row, out, env):
            target = out / (row['name'] + '-1.0.0.crate')
            target.write_bytes(archive(row['name']))
            row['archive'] = str(target)
            return target
        def registry(name, version):
            return {'checksum': 'checked'} if name in present else None
        def execute(args, out, env, row, token=''):
            self.assertIn('--locked', args)
            self.assertEqual(env['CARGO_REGISTRY_TOKEN'], 'secret')
            uploaded.append(row['name'])
            present.add(row['name'])
            return 0, ''
        with patch.dict(os.environ, {'CARGO_REGISTRY_TOKEN': 'secret'}), \
                patch.object(release.subprocess, 'check_output', side_effect=[SHA, '']), \
                patch.object(release, 'release_plan', return_value={'packages': rows}), \
                patch.object(release, 'isolated_environment', return_value={}), \
                patch.object(release, 'package_candidate', side_effect=package), \
                patch.object(release, 'registry_version', side_effect=registry), \
                patch.object(release, 'compare_published', side_effect=lambda name, *args: (changes or {}).get(name, [])), \
                patch.object(release, 'verify_candidate', side_effect=lambda row, *args: verified.append(row['name'])), \
                patch.object(release, 'execute', side_effect=execute):
            try:
                report = release.run(root, out, mode, 'publish ' + SHA)
                error = None
            except ValueError as exc:
                report, error = json.loads((out / 'report.json').read_text()), str(exc)
        return report, verified, uploaded, error

    def test_collision_blocks_all_uploads_even_when_earlier_candidate_is_missing(self):
        report, verified, uploaded, error = self.exercise('publish', ['child'], {'child': ['src/lib.rs']})
        self.assertIn('Bump versions', error)
        self.assertEqual(report['status'], 'failed')
        self.assertFalse(verified or uploaded)
        self.assertEqual(report['packages'][1]['changed_files'], ['src/lib.rs'])

    def test_already_published_versions_skip_build_and_upload_even_with_publish_disabled(self):
        report, verified, uploaded, error = self.exercise('publish', ['parent', 'child'], disabled=True)
        self.assertIsNone(error)
        self.assertFalse(verified or uploaded)
        self.assertTrue(report['release_ready'])

    def test_missing_versions_upload_dependency_first_and_verify_visibility(self):
        report, verified, uploaded, error = self.exercise('publish', [])
        self.assertIsNone(error)
        self.assertEqual(verified, ['parent', 'child'])
        self.assertEqual(uploaded, ['parent', 'child'])
        self.assertTrue(all(row['status'] == 'published-verified' for row in report['packages']))

    def test_plan_never_builds_or_uploads_and_dry_run_reports_missing_dependencies(self):
        report, verified, uploaded, error = self.exercise('plan', [])
        self.assertIsNone(error)
        self.assertFalse(verified or uploaded)
        self.assertFalse(report['uploads_enabled'])
        report, verified, uploaded, error = self.exercise('dry-run', [])
        self.assertIsNone(error)
        self.assertEqual(verified, ['parent'])
        self.assertFalse(uploaded)
        self.assertEqual(report['packages'][1]['status'], 'blocked-unpublished-dependencies')
        self.assertFalse(report['release_ready'])

    def test_missing_disabled_package_blocks_before_any_upload(self):
        report, verified, uploaded, error = self.exercise('publish', [], disabled=True)
        self.assertIn('disabled', error)
        self.assertFalse(verified or uploaded)

    def test_output_under_checkout_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(ValueError, 'outside'):
                release.run(root, root / 'output')


if __name__ == '__main__':
    unittest.main()
