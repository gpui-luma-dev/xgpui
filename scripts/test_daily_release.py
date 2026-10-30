import unittest
from unittest.mock import patch
from pathlib import Path

import daily_release as release

SHA = 'a' * 40


class DailyReleaseTests(unittest.TestCase):
    def report(self):
        return dict(mode='publish', status='published-verified', release_ready=True,
                    source_commit=SHA, started_at='2026-10-30T11:00:00+00:00',
                    packages=[dict(name='xgpui', version='1.1.0', status='published-verified')])

    def test_finalization_rejects_incomplete_or_foreign_reports(self):
        for changes in [dict(mode='plan'), dict(status='failed'), dict(release_ready=False),
                        dict(source_commit='main'), dict(packages=[]),
                        dict(packages=[dict(name='x', status='publish-needed')])]:
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                release.release_metadata(self.report() | changes)

    def test_release_identity_is_stable_and_distinct_per_commit(self):
        tag, sha, notes = release.release_metadata(self.report())
        self.assertEqual(tag, 'release-2026-10-30-aaaaaaaaaaaa')
        self.assertEqual(sha, SHA)
        self.assertIn('1.1.0', notes)
        self.assertNotEqual(tag, release.release_metadata(self.report() | dict(source_commit='b' * 40))[0])

    def test_only_internal_manifest_versions_change(self):
        text = '''[package]
name = "xgpui"
version = "1.1.0"
[dependencies]
external = "1.1.0"
gpui = { package = "xgpui", version = "1.1.0", path = "../gpui" }
[target.'cfg(windows)'.dependencies]
xgpui = "1.1.0"
'''
        updated = release.rewrite_manifest(text, {'xgpui': '1.1.1'})
        self.assertIn('external = "1.1.0"', updated)
        self.assertEqual(updated.count('"1.1.1"'), 3)

    def test_dependency_closure_and_docs_exclusion(self):
        rows = [dict(name='a', version='1.0.0', manifest='crates/a/Cargo.toml', dependencies=[]),
                dict(name='b', version='0.2.0', manifest='crates/b/Cargo.toml', dependencies=['a']),
                dict(name='c', version='1.0.0', manifest='crates/c/Cargo.toml', dependencies=[])]
        def fake_git(root, *args):
            if args[0] == 'diff':
                return 'crates/a/src/lib.rs\ndocs/WHATS_NEW.md'
            return SHA
        with patch.object(release, 'git', side_effect=fake_git), patch.object(release, 'release_plan', return_value={'packages': rows}):
            plan = release.prepare(Path('/unused'), 'release-baseline')
        self.assertEqual([(r['name'], r['new']) for r in plan['packages']], [('a', '1.0.1'), ('b', '0.2.1')])
        self.assertEqual(plan['packages'][1]['reason'], 'dependency version changes')

    def test_existing_tag_cannot_be_retargeted(self):
        with patch.object(release, 'git', side_effect=[SHA, '2026-10-30T11:00:00+00:00', 'b' * 40 + '\trefs/tags/release-2026-10-30-aaaaaaaaaaaa']), self.assertRaisesRegex(ValueError, 'different commit'):
            release.finalize(Path('/unused'), self.report(), 'owner/repo')


if __name__ == '__main__':
    unittest.main()
