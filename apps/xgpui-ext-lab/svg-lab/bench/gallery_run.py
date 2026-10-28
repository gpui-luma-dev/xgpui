#!/usr/bin/env python3
"""Run native gallery cases serially; requires a graphical desktop session."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import platform
import subprocess

p = argparse.ArgumentParser()
p.add_argument('--binary', type=Path, default=Path('target/release/svg-lab'))
p.add_argument('--app-bundle', type=Path, help='macOS bundle to launch with open -n -W')
p.add_argument('--corpus', type=Path, default=Path('/tmp/xgpui-svg-corpus'))
p.add_argument('--output', type=Path, required=True)
p.add_argument('--scheduler-mib', type=int)
p.add_argument('--scheduler-jobs', type=int, default=2)
p.add_argument('--budgets', type=int, nargs='+', default=[16, 4])
p.add_argument('--tiles', type=int, default=24)
p.add_argument('--eager-only', action='store_true')
p.add_argument('--repeat', type=int, default=1)
p.add_argument('--skip-existing', action='store_true')
a = p.parse_args()
assert 1 <= a.repeat <= 10
a.output.mkdir(parents=True, exist_ok=True)
manifest = json.loads(Path(__file__).with_name('corpus.json').read_text())
art = ['tiger', 'gallardo', 'AJ_Digital_Camera', 'USStates', 'rg1024_green_grapes', 'rg1024_metal_effect', 'dragon', 'ny1']
for item in manifest:
    if item['id'] in art:
        assert hashlib.sha256((a.corpus / (item['id'] + '.svg')).read_bytes()).hexdigest() == item['sha256']
metadata = {'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'platform': platform.platform(), 'machine': platform.machine(), 'binary_sha256': hashlib.sha256(a.binary.read_bytes()).hexdigest(), 'scheduler_mib':a.scheduler_mib, 'scheduler_jobs':a.scheduler_jobs, 'tiles':a.tiles, 'app_bundle':str(a.app_bundle) if a.app_bundle else None, 'corpus': [x for x in manifest if x['id'] in art], 'hardware': subprocess.run(['sysctl', '-n', 'machdep.cpu.brand_string', 'hw.memsize'], capture_output=True, text=True).stdout.strip() if platform.system() == 'Darwin' else platform.processor()}
(a.output / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
for repeat in range(a.repeat):
    for eager in ([True] if a.eager_only else [True, False]):
        for budget in a.budgets:
            name = f'{"eager" if eager else "visible"}-{budget}' + (f'-r{repeat+1}' if a.repeat > 1 else '')
            report = a.output / (name + '.json')
            if a.skip_existing and report.exists():
                continue
            cmd = [str(a.binary.resolve()), '--gallery', str(a.corpus.resolve()), '--gallery-tiles', str(a.tiles), '--budget-mib', str(budget), '--gallery-report', str(report.resolve())]
            if a.scheduler_mib is not None:
                cmd += ['--scheduler-mib', str(a.scheduler_mib), '--scheduler-jobs', str(a.scheduler_jobs)]
            if eager:
                cmd.append('--gallery-eager')
            if a.app_bundle:
                cmd = ['open', '-n', '-W', str(a.app_bundle.resolve()), '--args', *cmd[1:]]
            report.unlink(missing_ok=True)
            print(name, flush=True)
            result = subprocess.run(cmd, capture_output=True, text=True, timeout=100)
            (a.output / (name + '.log')).write_text(result.stdout + result.stderr)
            result.check_returncode()
            data = json.loads(report.read_text())
            assert len(data['frames']) > 100 and data['samples'][-1]['phase'] == 'unmounted', 'Incomplete desktop run'
            if data.get('scheduler'):
                limits=data['scheduler']
                for sample in data['samples']:
                    stats=sample['scheduler']
                    assert stats['peak_managed_bytes'] <= limits['max_bytes'], 'Aggregate reservation overflow'
                    assert stats['peak_active_jobs'] <= limits['max_jobs'], 'Concurrency overflow'
                assert data['samples'][-1]['scheduler']['managed_bytes'] == 0, 'Reservations retained after unmount'
                assert data['samples'][-1]['scheduler']['active_jobs'] == 0, 'Active work after unmount'
