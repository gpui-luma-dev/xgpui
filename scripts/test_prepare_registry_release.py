import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from prepare_registry_release import prepare, release_plan


class RegistryPreparationTests(unittest.TestCase):
    def inventory(self, reverse_edge=False):
        def row(name, deps):
            return {'name': name, 'version': '1.0.0', 'manifest': name + '/Cargo.toml',
                    'publish': False, 'issues': [],
                    'dependencies': [{'package': dep, 'kind': 'dependencies'} for dep in deps]}
        return {'packages': [row('xgpui', ['xgpui-macros', *(['xgpui-color'] if reverse_edge else [])]),
                             row('xgpui-macros', []), row('xgpui-platform', ['xgpui']),
                             row('xgpui-color', [])],
                'including_dev_cycle_blocked_packages': [], 'patches_to_review_outside_repository': []}

    def fixture(self, root):
        (root / 'scripts').mkdir()
        (root / 'scripts/release-packages.json').write_text(json.dumps({'packages': {
            'gpui': 'xgpui', 'gpui-macros': 'xgpui-macros',
            'gpui-platform': 'xgpui-platform', 'xgpui-color': 'xgpui-color'}}))

    def test_framework_precedes_independent_child_and_requires_explicit_registry_approval(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            with patch('prepare_registry_release.audit', return_value=self.inventory()), \
                 patch('prepare_registry_release.subprocess.run', side_effect=AssertionError('unexpected command')):
                plan = release_plan(root)
            self.assertEqual(plan['phases'][0]['packages'], ['xgpui-macros', 'xgpui', 'xgpui-platform'])
            self.assertEqual(plan['phases'][1]['packages'], ['xgpui-color'])
            self.assertEqual(plan['core_package'], 'xgpui')
            self.assertFalse(plan['registry_access'])
            self.assertFalse(plan['uploads_enabled'])
            self.assertFalse(plan['release_ready'])
            self.assertTrue(all(row['publish_policy'] is False for row in plan['packages']))

    def test_framework_cannot_depend_on_a_child_extension(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            with patch('prepare_registry_release.audit', return_value=self.inventory(True)):
                with self.assertRaisesRegex(ValueError, 'child extension'):
                    release_plan(root)

    def test_output_inside_checkout_rejected_before_commands_or_writes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch('prepare_registry_release.subprocess.check_output', side_effect=AssertionError('unexpected command')):
                with self.assertRaisesRegex(ValueError, 'outside'):
                    prepare(root, root / 'out', archives=True)
            self.assertFalse((root / 'out').exists())


if __name__ == '__main__':
    unittest.main()
