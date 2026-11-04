"""Check for changed crates that would be missed by a crates.io release.

Read-only: compare with verified release tags since the configured epoch,
including working-tree edits, then check version availability on crates.io.
"""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

from publish_extensions import fetch
from release_changes import change_plan


def run(command, root):
    try:
        result = subprocess.run(command, cwd=root, capture_output=True, text=True, timeout=30)
    except (OSError, subprocess.TimeoutExpired) as error:
        return None, str(error)
    return result.returncode, (result.stdout + result.stderr).strip()


def source_identity(root):
    code, commit = run(['git', 'rev-parse', 'HEAD'], root)
    if code != 0:
        raise ValueError(f'Cannot identify source commit: {commit}')
    result = subprocess.run(
        ['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'],
        cwd=root, capture_output=True, check=True)
    digest = hashlib.sha256()
    for name in sorted(set(result.stdout.split(b'\0')) - {b''}):
        path = root / os.fsdecode(name)
        if path.is_symlink():
            kind, contents = b'symlink', os.fsencode(os.readlink(path))
        elif path.is_file():
            kind, contents = (b'executable' if path.stat().st_mode & 0o111 else b'file'), path.read_bytes()
        elif not path.exists():
            kind, contents = b'deleted', b''
        else:
            raise ValueError(f'Unsupported source input: {path}')
        digest.update(name + b'\0' + kind + b'\0' + hashlib.sha256(contents).digest())
    code, status = run(['git', 'status', '--porcelain', '--untracked-files=all'], root)
    if code != 0:
        raise ValueError(f'Cannot inspect source status: {status}')
    return {'commit': commit, 'dirty': bool(status), 'sha256': digest.hexdigest(),
            'scope': 'tracked and non-ignored untracked file contents, paths and executable modes; excludes ignored inputs'}


def registry_report(root, epoch=None):
    identity = source_identity(root)
    config = json.loads((root / 'scripts/release-check.json').read_text())
    plan = change_plan(root, epoch or config['epoch'], config['repository'])
    rows = plan['packages']
    for row in rows:
        name, version = row['name'], row['version']
        try:
            data = fetch(f'https://crates.io/api/v1/crates/{name}', missing_ok=True)
            if data is None:
                versions, latest = {}, None
            else:
                document = json.loads(data)
                if document['crate']['id'] != name:
                    raise ValueError('Unexpected registry crate identity')
                versions = {v['num']: v for v in document['versions']}
                latest = document['crate']['max_version']
            row['registry_latest'] = latest
            baseline_version = row['baseline']['version']
            if baseline_version is not None and baseline_version not in versions:
                raise ValueError(f'Baseline version {baseline_version} is not on crates.io; release history needs review')
            existing = versions.get(version)
            if existing:
                row['yanked'] = existing['yanked']
            if row['changed'] and existing:
                row.update(status='version-bump-required',
                           action=f'Changed since {row["baseline"]["tag"]}, but {version} is already published. Bump this crate and update dependent pins.')
            elif row['changed']:
                policy = row['publish_policy']
                disabled = policy is False or (isinstance(policy, list) and 'crates-io' not in policy)
                row.update(status='publication-disabled' if disabled else 'ready-for-new-version',
                           action='Enable crates.io publication before uploading.' if disabled else 'Changes have an unpublished version for release preparation.')
                if not disabled and row['pin_updates']:
                    row.update(status='dependency-pin-update-required', action='Update internal dependency pins to the candidate versions before release.')
            elif existing:
                row.update(status='unchanged', action='No crate changes to publish.')
            else:
                raise ValueError('Unchanged baseline version is missing from crates.io')
        except Exception as error:
            row.update(status='check-failed', error=str(error))
    errors = []
    if source_identity(root) != identity:
        errors.append('Source changed during the check; rerun.')
    counts = {status: sum(row['status'] == status for row in rows)
              for status in ('unchanged', 'version-bump-required', 'ready-for-new-version', 'dependency-pin-update-required', 'publication-disabled', 'check-failed')}
    passed = not errors and not any(counts[status] for status in ('version-bump-required', 'dependency-pin-update-required', 'publication-disabled', 'check-failed'))
    return {'schema_version': 2, 'generated_at': datetime.now(timezone.utc).isoformat(),
            'source': identity, 'epoch': plan['epoch'], 'epoch_commit': plan['epoch_commit'],
            'scope': 'Conservative crate directory changes and internal dependency closure since verified release tags; no builds or archive payload comparison',
            'uploads_enabled': False, 'release_ready': False, 'version_preflight_passed': passed,
            'counts': counts, 'errors': errors, 'packages': rows}


def registry_summary(result):
    lines = ['Crate release preflight', f'Epoch: {result["epoch"]}',
             'Read-only check, including local edits. No versions changed and no crates uploaded.', '']
    for row in result['packages']:
        if row['status'] == 'unchanged':
            continue
        lines.append(f"[{row['status'].upper()}] {row['name']} {row['version']}")
        lines.append('  ' + row.get('error', row.get('action', '')))
        if row['changed_files']:
            lines.append('  Files: ' + ', '.join(row['changed_files']))
        if row['dependency_changes']:
            lines.append('  Changed dependencies: ' + ', '.join(row['dependency_changes']))
        for pin in row['pin_updates']:
            lines.append(f"  Pin {pin['dependency']}: {pin['current']} -> {pin['required']}")
        if row.get('yanked'):
            lines.append('  Published version is yanked; its number still cannot be reused.')
    counts = result['counts']
    lines.extend(['', f"Unchanged: {counts['unchanged']}; need bumps: {counts['version-bump-required']}; need pin updates: {counts['dependency-pin-update-required']}; unpublished versions: {counts['ready-for-new-version']}; blocked checks: {counts['publication-disabled'] + counts['check-failed']}."])
    lines.extend('ERROR: ' + error for error in result['errors'])
    if not result['version_preflight_passed']:
        lines.append('FAIL: Crate changes would be missed by publication, or checks need attention.')
    elif not counts['ready-for-new-version']:
        lines.append('PASS: No unpublished crate changes detected. No release needed.')
    else:
        lines.append('PASS: Changed crates have unpublished versions for release preparation.')
    lines.append('Build correctness, payload verification and publication approval remain separate release steps.')
    return '\n'.join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--epoch', help='Override the configured epoch tag.')
    parser.add_argument('--report', type=Path, help='Also write JSON to a file outside the checkout.')
    parser.add_argument('--json', action='store_true', help='Print JSON instead of the human-readable summary.')
    args = parser.parse_args()
    root = args.root.resolve()
    if args.report and args.report.resolve().is_relative_to(root):
        parser.error('--report must be outside the checkout to avoid changing the source fingerprint')
    try:
        result = registry_report(root, args.epoch)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f'Preflight failed: {error}', file=sys.stderr)
        return 1
    encoded = json.dumps(result, indent=2, sort_keys=True) + '\n'
    if args.report:
        args.report.write_text(encoded)
    print(encoded if args.json else registry_summary(result), end='' if args.json else '\n')
    if args.report and not args.json:
        print(f'JSON report: {args.report.resolve()}')
    return 0 if result['version_preflight_passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
