"""Contract checks for validation scopes and orchestration; no GPU required."""
import importlib.util
from pathlib import Path
import shlex
import subprocess
import unittest
from unittest.mock import patch
import tempfile
import json
import os

spec = importlib.util.spec_from_file_location('paint_runner', Path(__file__).with_name('run.py'))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class RunnerTests(unittest.TestCase):
    def test_full_inventory_matches_original_commands(self):
        # Snapshot of the pre-split acceptance inventory, by Cargo arguments.
        expected = {
            'linux-vulkan': ['coverage_cache', 'wgpu_custom', 'wgpu_extension_paints', 'wgpu_compositor_uniform_frames', 'wgpu_low_alpha_saturated_edges'],
            'windows-wgpu': ['coverage_cache', 'wgpu_custom', 'wgpu_extension_paints', 'wgpu_compositor_uniform_frames', 'wgpu_low_alpha_saturated_edges'],
            'windows-native': ['check', 'coverage_cache', 'dx11_custom', 'custom_draw::wide_output', 'directx_renderer::wide_sdr', 'directx_renderer::color_presentation', 'xgpui-color/Cargo.toml', 'xgpui-gamut/Cargo.toml', 'instanced', 'wgpu,portable-shaders', '--png'],
        }
        for backend, markers in expected.items():
            steps = runner.PLANS[backend]
            self.assertEqual(len(steps), len(markers) + 1)
            self.assertIn('brush::tests', steps[-1]['command'])
            for step, marker in zip(steps, markers):
                self.assertIn(marker, step['command'])
                self.assertIn('--locked', shlex.split(step['command']))
                self.assertIn(step['scope'], runner.SCOPES[1:])
                self.assertTrue((runner.ROOT / shlex.split(step['command'])[shlex.split(step['command']).index('--manifest-path')+1]).with_name('Cargo.lock').is_file())

    def test_build_preserves_test_selection_and_features(self):
        for steps in runner.PLANS.values():
            for step in steps:
                phases = runner.phases(step['command'])
                command = shlex.split(step['command'])
                if command[1] == 'test':
                    self.assertEqual(phases[0][1][:-1], command[:command.index('--')] if '--' in command else command)
                    self.assertEqual(phases[0][1][-1], '--no-run')
                    self.assertEqual(phases[1], ('test', command))

    def test_results_distinguish_focused_full_and_failure(self):
        class Process:
            stdout = ['test output\n']
            def wait(self):
                return process_status
        for scope, process_status, acceptance in [('full', 0, True), ('producer-cache', 0, False), ('full', 7, False)]:
            with tempfile.TemporaryDirectory() as tmp:
                with patch.object(runner, 'ROOT', Path(tmp)), patch.object(runner.subprocess, 'Popen', return_value=Process()), patch.object(runner.subprocess, 'check_output', return_value='test-sha'), patch.dict(os.environ, {'CARGO_TARGET_DIR': str(Path(tmp)/'target'), 'GITHUB_STEP_SUMMARY': str(Path(tmp)/'summary')}), patch('sys.argv', ['run.py', '--backend', 'windows-native', '--scope', scope]):
                    self.assertEqual(runner.main(), process_status)
                    report = json.loads((Path(tmp)/'paint-results/timings.json').read_text())
                    self.assertEqual(report['acceptance'], acceptance)
                    self.assertEqual(report['scope'], scope)
                    self.assertEqual(report['results'][-1]['exit_code'], process_status)
                    self.assertTrue(list((Path(tmp)/'paint-results').glob('*.log')))

    def test_unavailable_scope_rejected(self):
        with patch('sys.argv', ['run.py', '--backend', 'linux-vulkan', '--scope', 'color-gamut', '--dry-run']):
            with self.assertRaises(SystemExit) as result:
                runner.main()
            self.assertEqual(result.exception.code, 2)


if __name__ == '__main__':
    unittest.main()
