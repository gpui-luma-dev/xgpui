import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from release_check import registry_report, source_identity
from release_changes import change_plan, verified_versions


class ReleaseCheckTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.git('init', '-q')
        self.git('config', 'user.email', 'fixture@example.test')
        self.git('config', 'user.name', 'Fixture')
        (self.root / 'scripts').mkdir()
        (self.root / 'scripts/release-check.json').write_text(json.dumps({'epoch': 'release-epoch', 'repository': 'owner/repo'}))
        self.mapping = {'packages': {'a': 'a', 'b': 'b', 'c': 'c'}, 'version': '1.0.0', 'versions': {}}
        (self.root / 'upstream.json').write_text(json.dumps({'packages': {n: 'crates/' + n for n in 'abc'}}))
        self.save_mapping()
        for name in 'abc':
            directory = self.root / 'crates' / name
            (directory / 'src').mkdir(parents=True)
            (directory / 'src/lib.rs').write_text('pub fn sample() {}\n')
            (directory / 'Cargo.toml').write_text(f'[package]\nname="{name}"\nversion="1.0.0"\n' +
                ('[dependencies]\na = { version="=1.0.0", path="../a" }\n' if name == 'b' else ''))
        self.commit()
        self.git('tag', 'release-epoch')
        self.history = []
        self.registry = {name: ['1.0.0'] for name in 'abc'}

    def git(self, *args):
        return subprocess.check_output(['git', *args], cwd=self.root, text=True).strip()

    def commit(self):
        self.git('add', '.')
        self.git('commit', '-qm', 'fixture')

    def save_mapping(self):
        (self.root / 'scripts/release-packages.json').write_text(json.dumps(self.mapping))

    def edit(self, name='a'):
        (self.root / f'crates/{name}/src/lib.rs').write_text('pub fn changed() {}\n')

    def bump(self, name, version):
        path = self.root / f'crates/{name}/Cargo.toml'
        path.write_text(path.read_text().replace('version="1.0.0"', f'version="{version}"', 1))
        self.mapping['versions'][name] = version
        self.save_mapping()

    def check(self, error=False):
        def fetch(url, missing_ok=False):
            name = url.rsplit('/', 1)[-1]
            if error:
                raise OSError('network unavailable')
            return json.dumps({'crate': {'id': name, 'max_version': self.registry[name][-1]},
                               'versions': [{'num': version, 'yanked': False} for version in self.registry[name]]}).encode()
        with patch('release_changes.release_history', return_value=self.history), \
             patch('release_check.fetch', side_effect=fetch):
            return registry_report(self.root)

    def test_unchanged_and_docs_only_changes_pass_without_a_new_version(self):
        (self.root / 'README.md').write_text('docs only')
        result = self.check()
        self.assertTrue(result['version_preflight_passed'])
        self.assertEqual(result['counts']['unchanged'], 3)
        self.assertFalse(result['uploads_enabled'])

    def test_changed_crate_needs_bump_before_dependency_requirements_change(self):
        self.edit()
        result = self.check()
        self.assertFalse(result['version_preflight_passed'])
        rows = {r['name']: r for r in result['packages']}
        self.assertEqual(rows['a']['status'], 'version-bump-required')
        self.assertEqual(rows['a']['changed_files'], ['crates/a/src/lib.rs'])
        self.assertEqual(rows['b']['status'], 'unchanged')
        self.assertEqual(rows['b']['dependency_changes'], [])
        self.assertEqual(rows['c']['status'], 'unchanged')

    def test_compatible_dependency_does_not_force_a_dependent_release(self):
        path = self.root / 'crates/b/Cargo.toml'
        path.write_text(path.read_text().replace('=1.0.0', '1.0.0'))
        self.commit()
        self.git('tag', '-f', 'release-epoch')
        self.edit()
        self.bump('a', '1.0.1')
        rows = {r['name']: r for r in self.check()['packages']}
        self.assertEqual(rows['b']['status'], 'unchanged')
        self.assertEqual(rows['b']['pin_updates'], [])

    def test_bumps_and_matching_pins_make_changes_eligible(self):
        self.edit()
        for name in 'ab':
            self.bump(name, '1.0.1')
        result = self.check()
        self.assertEqual(result['counts']['dependency-pin-update-required'], 1)
        path = self.root / 'crates/b/Cargo.toml'
        path.write_text(path.read_text().replace('=1.0.0', '=1.0.1'))
        result = self.check()
        self.assertTrue(result['version_preflight_passed'])
        self.assertEqual(result['counts']['ready-for-new-version'], 2)

    def test_a_later_verified_release_stops_reporting_already_shipped_changes(self):
        self.edit('c')
        self.bump('c', '1.0.1')
        self.commit()
        self.git('tag', 'release-next')
        self.history = [{'tag_name': 'release-next', 'draft': False,
                         'body': '| `c` | 1.0.1 | published-verified |'}]
        self.registry['c'].append('1.0.1')
        result = self.check()
        self.assertTrue(result['version_preflight_passed'])
        row = next(r for r in result['packages'] if r['name'] == 'c')
        self.assertEqual(row['baseline']['tag'], 'release-next')
        self.assertEqual(row['status'], 'unchanged')
        # A subsequent change at that published version must still fail.
        (self.root / 'crates/c/src/added.rs').write_text('new file')
        result = self.check()
        self.assertEqual(result['counts']['version-bump-required'], 1)

    def test_draft_or_unverified_tags_do_not_hide_unpublished_changes(self):
        self.edit('c')
        self.commit()
        self.git('tag', 'release-partial')
        for draft, body in [(True, '| c | 1.0.0 | already-published |'), (False, 'Publication started')]:
            self.history = [{'tag_name': 'release-partial', 'draft': draft, 'body': body}]
            self.assertEqual(self.check()['counts']['version-bump-required'], 1)

    def test_deleted_and_renamed_files_are_changes(self):
        self.git('mv', 'crates/c/src/lib.rs', 'crates/c/src/renamed.rs')
        result = self.check()
        row = next(r for r in result['packages'] if r['name'] == 'c')
        self.assertEqual(row['changed_files'], ['crates/c/src/lib.rs', 'crates/c/src/renamed.rs'])

    def test_registry_failure_and_inconsistent_release_map_fail_closed(self):
        self.assertFalse(self.check(error=True)['version_preflight_passed'])
        self.mapping['versions']['a'] = '9.0.0'
        self.save_mapping()
        with self.assertRaisesRegex(ValueError, 'does not match'):
            self.check()

    def test_versioned_dev_dependency_is_included_but_path_only_is_not(self):
        path = self.root / 'crates/c/Cargo.toml'
        path.write_text(path.read_text() + '[dev-dependencies]\na = { path="../a" }\n')
        self.commit()
        self.git('tag', '-f', 'release-epoch')
        self.edit()
        self.assertEqual(self.check()['counts']['version-bump-required'], 2)
        path.write_text(path.read_text().replace('path="../a"', 'path="../a", version="=1.0.0"'))
        result = self.check()
        row = next(r for r in result['packages'] if r['name'] == 'c')
        self.assertIn('a', row['dependency_changes'])

    def test_epoch_must_be_an_existing_ancestor_tag(self):
        with patch('release_changes.release_history', return_value=[]):
            with self.assertRaises(subprocess.CalledProcessError):
                change_plan(self.root, 'missing-tag', 'owner/repo')

    def test_fingerprint_tracks_edits_and_untracked_files(self):
        before = source_identity(self.root)
        self.edit()
        edited = source_identity(self.root)
        self.assertNotEqual(before['sha256'], edited['sha256'])
        self.assertTrue(edited['dirty'])
        (self.root / 'new.rs').write_text('new')
        self.assertNotEqual(edited['sha256'], source_identity(self.root)['sha256'])

    def test_verification_rows_require_an_explicit_success_status(self):
        self.assertEqual(verified_versions('| c | 1.0.1 | publish-needed |'), {})
        self.assertEqual(verified_versions('| `c` | 1.0.1 | already-published |'), {'c': '1.0.1'})


if __name__ == '__main__':
    unittest.main()
