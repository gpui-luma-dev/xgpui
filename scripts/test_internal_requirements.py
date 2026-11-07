import unittest

from internal_requirements import matches


class InternalRequirementTests(unittest.TestCase):
    def test_stable_compatibility_boundaries(self):
        for requirement, version, expected in [
            ('3.0.0', '3.1.0', True), ('3.0.0', '4.0.0', False),
            ('3.1.0', '3.0.0', False), ('^3.0.0', '3.2.0', True),
            ('0.4.0', '0.4.9', True), ('0.4.0', '0.5.0', False),
            ('0.0.3', '0.0.4', False), ('~2.2.0', '2.3.0', False),
            ('=2.2.0', '2.2.0', True), ('=2.2.0', '2.2.1', False),
        ]:
            with self.subTest(requirement=requirement, version=version):
                self.assertEqual(matches(requirement, version), expected)

    def test_unfamiliar_requirements_need_review(self):
        for requirement in ('*', '>=2, <4', '2', '2.2.0-alpha', '==2.2.0'):
            with self.subTest(requirement=requirement), self.assertRaises(ValueError):
                matches(requirement, '2.2.0')
