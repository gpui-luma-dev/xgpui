"""Run the preserved GPU command inventory with separate build/execution timings."""
import argparse
import json
import os
from pathlib import Path
import shlex
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
PLANS = json.loads(Path(__file__).with_name('commands.json').read_text())
SCOPES = ('full', 'color-gamut', 'producer-cache', 'paint-parity')


def phases(command):
    args = shlex.split(command)
    if args[1] == 'check':
        return [('build', args)]
    if args[1] == 'test':
        build = args[:args.index('--')] if '--' in args else args[:]
        return [('build', build + ['--no-run']), ('test', args)]
    build = args[:args.index('--')]
    build[1] = 'build'
    return [('build', build), ('capture', args)]


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--backend', choices=PLANS, required=True)
    parser.add_argument('--scope', choices=SCOPES, default='full')
    parser.add_argument('--dry-run', action='store_true')
    args = parser.parse_args()
    selected = [s for s in PLANS[args.backend] if args.scope == 'full' or s['scope'] == args.scope]
    if not selected:
        parser.error(f'{args.scope} is unavailable for {args.backend}; use Windows native for color/gamut')
    if args.dry_run:
        print(json.dumps([dict(step=s['name'], phases=phases(s['command'])) for s in selected], indent=2))
        return 0
    if not os.environ.get('CARGO_TARGET_DIR'):
        parser.error('set an absolute CARGO_TARGET_DIR to share builds across manifests')
    output = ROOT / 'paint-results'
    output.mkdir(exist_ok=True)
    report = dict(backend=args.backend, scope=args.scope,
                  sha=os.environ.get('GITHUB_SHA', subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()),
                  cache_mode=os.environ.get('CACHE_MODE', 'local'),
                  cache_match=os.environ.get('CACHE_MATCH', ''),
                  setup_seconds=time.time() - float(os.environ.get('SETUP_STARTED', time.time())),
                  rustc=subprocess.check_output(['rustc', '-Vv'], text=True),
                  target_dir=os.environ['CARGO_TARGET_DIR'], results=[], acceptance=False)
    status = 0
    try:
        for index, step in enumerate(selected):
            for phase, command in phases(step['command']):
                print(f"::group::{step['name']} [{phase}]", flush=True)
                print(shlex.join(command), flush=True)
                started = time.monotonic()
                log = output / f'{index:02}-{phase}.log'
                with log.open('w', encoding='utf-8') as stream:
                    process = subprocess.Popen(command, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding='utf-8', errors='replace')
                    for line in process.stdout:
                        print(line, end='', flush=True)
                        stream.write(line)
                    status = process.wait()
                report['results'].append(dict(name=step['name'], phase=phase, command=command,
                                              seconds=time.monotonic()-started, exit_code=status))
                print('::endgroup::', flush=True)
                if status:
                    return status
        report['acceptance'] = args.scope == 'full'
        return 0
    finally:
        (output / 'timings.json').write_text(json.dumps(report, indent=2)+'\n')
        totals = {phase: sum(r['seconds'] for r in report['results'] if r['phase'] == phase) for phase in ('build', 'test', 'capture')}
        summary = (f"## {args.backend} / {args.scope}\n\nCommit: `{report['sha']}`\n\n"
                   f"Full backend acceptance: **{report['acceptance']}**. Focused runs are diagnostic only.\n\n"
                   f"Cache: `{report['cache_mode']}`; restored key: `{report['cache_match'] or 'miss'}`\n\n"
                   f"Setup: {report['setup_seconds']:.1f}s; build: {totals['build']:.1f}s; "
                   f"test: {totals['test']:.1f}s; capture: {totals['capture']:.1f}s.\n")
        (output / 'summary.md').write_text(summary)
        if os.environ.get('GITHUB_STEP_SUMMARY'):
            with open(os.environ['GITHUB_STEP_SUMMARY'], 'a') as stream:
                stream.write(summary)


if __name__ == '__main__':
    raise SystemExit(main())
