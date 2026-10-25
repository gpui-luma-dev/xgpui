#!/usr/bin/env python3
"""Compare independent queues with shared 64/128 MiB pools in fresh native processes."""
import argparse
from pathlib import Path
import subprocess
import sys
import platform
import plistlib
import tempfile
import shutil

p = argparse.ArgumentParser()
p.add_argument('--output', type=Path, required=True)
p.add_argument('--binary', type=Path, default=Path('target/release/svg-lab'))
p.add_argument('--corpus', type=Path, default=Path('/tmp/xgpui-svg-corpus'))
p.add_argument('--repeat', type=int, default=1)
a = p.parse_args()
runner = Path(__file__).with_name('gallery_run.py')
bundle_args=[]
bundle_root=None
if platform.system()=='Darwin':
    bundle_root=Path(tempfile.mkdtemp(prefix='xgpui-svg-stress-'))
    bundle=bundle_root/'SVG Scheduler Stress.app'
    contents=bundle/'Contents'
    (contents/'MacOS').mkdir(parents=True)
    (contents/'MacOS'/'svg-lab').symlink_to(a.binary.resolve())
    (contents/'Info.plist').write_bytes(plistlib.dumps({'CFBundleIdentifier':'dev.xgpui.svg-stress.'+bundle_root.name.removeprefix('xgpui-svg-stress-').replace('_','-'),'CFBundleName':'SVG Scheduler Stress','CFBundleExecutable':'svg-lab','CFBundlePackageType':'APPL','NSHighResolutionCapable':True}))
    bundle_args=['--app-bundle',str(bundle)]
for name, extra in [('independent', []), ('shared-64', ['--scheduler-mib', '64']), ('shared-128', ['--scheduler-mib', '128']), ('shared-64-96tiles', ['--scheduler-mib', '64', '--tiles', '96', '--eager-only'])]:
    subprocess.run([sys.executable, str(runner), '--output', str(a.output / name), '--binary', str(a.binary), '--corpus', str(a.corpus), '--budgets', '16', '--repeat', str(a.repeat), *bundle_args, *extra], check=True)

if bundle_root:
    shutil.rmtree(bundle_root)
