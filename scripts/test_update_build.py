"""The legacy entry point delegates to the authoritative generator."""
import unittest
from unittest.mock import patch
import update_build


class BuildEntryPointTests(unittest.TestCase):
    def test_check_is_forwarded_to_rust_generator(self):
        with patch.object(update_build.sys, "argv", ["update_build.py", "--check"]), patch.object(update_build.subprocess, "call", return_value=7) as run:
            self.assertEqual(update_build.main(), 7)
            self.assertEqual(run.call_args.args[0][-2:], ["build-manifests", "--check"])
            self.assertEqual(run.call_args.kwargs["cwd"], update_build.ROOT)
