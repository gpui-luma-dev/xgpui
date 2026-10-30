#!/usr/bin/env python3
"""Prepare reviewable daily releases and finalize verified publication reports."""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import subprocess
import tomllib

from prepare_registry_release import release_plan


def git(root, *args):
    return subprocess.check_output(['git', *args], cwd=root, text=True).strip()


def prepare(root, baseline=None):
    baseline = baseline or git(root, 'describe', '--tags', '--match', 'release-*', '--abbrev=0')
    base = git(root, 'rev-parse', baseline + '^{commit}')
    git(root, 'merge-base', '--is-ancestor', base, 'HEAD')
    plan = release_plan(root)
    changed = set(git(root, 'diff', '--name-only', base, 'HEAD').splitlines())
    selected = {}
    for row in plan['packages']:
        directory = str(Path(row['manifest']).parent) + '/'
        if any(p.startswith(directory) and not p.endswith('/Cargo.lock') for p in changed):
            selected[row['name']] = 'crate files changed'
    # Shared packaging/configuration can affect every published archive.
    if changed & {'scripts/release-packages.json', 'scripts/publish_registry_release.py',
                  'scripts/prepare_registry_release.py', 'scripts/publish_preflight.py', 'upstream.json'}:
        selected.update({r['name']: 'shared release inputs changed' for r in plan['packages']})
    while True:
        added = {r['name']: 'dependency version changes' for r in plan['packages']
                 if r['name'] not in selected and set(r['dependencies']) & selected.keys()}
        if not added:
            break
        selected.update(added)
    rows = []
    for row in plan['packages']:
        if row['name'] not in selected:
            continue
        old = row['version']
        if not re.fullmatch(r'\d+\.\d+\.\d+', old):
            raise ValueError('Review prerelease version manually: ' + old)
        major, minor, patch = map(int, old.split('.'))
        rows.append({'name': row['name'], 'old': old, 'new': f'{major}.{minor}.{patch + 1}',
                     'reason': selected[row['name']]})
    return {'baseline': baseline, 'baseline_commit': base, 'source_commit': git(root, 'rev-parse', 'HEAD'),
            'packages': rows}


def rewrite_manifest(text, versions):
    """Preserve formatting; fail closed on dependency forms needing manual editing."""
    doc = tomllib.loads(text)
    package = doc.get('package', {}).get('name')
    section = ''
    result = []
    for line in text.splitlines(keepends=True):
        match = re.match(r'\s*\[([^\[\]]+)\]', line)
        if match:
            section = match[1]
        if section == 'package' and package in versions:
            line = re.sub(r'^(\s*version\s*=\s*)"[^"]+"', lambda m: m[1] + '"' + versions[package] + '"', line)
        if any(part in section.split('.') for part in ('dependencies', 'dev-dependencies', 'build-dependencies')):
            match = re.match(r'\s*([\w-]+)\s*=\s*(.*)', line)
            if match:
                alias, value = match.groups()
                identity = re.search(r'package\s*=\s*"([^"]+)"', value)
                name = identity[1] if identity else alias
                if name in versions:
                    if value.startswith('"'):
                        line = re.sub(r'(=\s*)"([=~^]?)[^"]+"', lambda m: m[1] + '"' + m[2] + versions[name] + '"', line, count=1)
                    elif 'version' in value:
                        line = re.sub(r'(version\s*=\s*)"([=~^]?)[^"]+"', lambda m: m[1] + '"' + m[2] + versions[name] + '"', line)
                    elif not ('path' in value or 'workspace' in value):
                        raise ValueError('Unsupported dependency: ' + line)
        result.append(line)
    updated = ''.join(result)
    tomllib.loads(updated)
    return updated


def apply(root, plan):
    if git(root, 'status', '--porcelain'):
        raise ValueError('Preparation requires a clean checkout')
    versions = {r['name']: r['new'] for r in plan['packages']}
    config = root / 'scripts/release-packages.json'
    mapping = json.loads(config.read_text())
    mapping.setdefault('versions', {}).update(versions)
    config.write_text(json.dumps(mapping, indent=2) + '\n')
    tracked = git(root, 'ls-files').splitlines()
    for name in tracked:
        path = root / name
        if path.name == 'Cargo.toml':
            path.write_text(rewrite_manifest(path.read_text(), versions))
        elif path.name == 'Cargo.lock':
            text = path.read_text()
            # Only local package records; never change registry package checksums.
            blocks = text.split('[[package]]')
            for i, block in enumerate(blocks[1:], 1):
                header = block.split('[[', 1)[0]
                doc = tomllib.loads(header)
                if doc.get('name') in versions and 'source' not in doc:
                    block = re.sub(r'(?m)^version = "[^"]+"', 'version = "' + versions[doc['name']] + '"', block, count=1)
                for row in plan['packages']:
                    block = block.replace('"' + row['name'] + ' ' + row['old'] + '"', '"' + row['name'] + ' ' + row['new'] + '"')
                blocks[i] = block
            path.write_text('[[package]]'.join(blocks))
    # Canonical generators own captured and generated manifests.
    for command in ('normalize', 'build-manifests'):
        subprocess.run(['cargo', 'run', '--locked', '--manifest-path', 'tools/release-sync/Cargo.toml', '--', command], cwd=root, check=True)


def preparation_notes(plan):
    lines = ['Daily release preparation', '', f"Baseline: `{plan['baseline']}` (`{plan['baseline_commit']}`).", '',
             '**Review required:** patch bumps are proposals. Review public API changes and choose minor/major versions where needed before merging.', '',
             '| Crate | Old | Proposed | Reason |', '| --- | --- | --- | --- |']
    lines += [f"| {r['name']} | {r['old']} | {r['new']} | {r['reason']} |" for r in plan['packages']]
    return '\n'.join(lines) + '\n'


def release_metadata(report, source_date=None):
    if report.get('mode') != 'publish' or report.get('status') != 'published-verified' or report.get('release_ready') is not True:
        raise ValueError('Finalization requires a successful verified publication report')
    sha = report['source_commit']
    if not re.fullmatch('[0-9a-f]{40}', sha):
        raise ValueError('Invalid source commit')
    rows = report['packages']
    if not rows or any(r['status'] not in ('already-published', 'published-verified') for r in rows):
        raise ValueError('Not all packages verified')
    date = datetime.fromisoformat(source_date or report['started_at']).astimezone(timezone.utc).strftime('%Y-%m-%d')
    # Include the immutable commit so multiple releases per day remain distinct.
    tag = f'release-{date}-{sha[:12]}'
    notes = f'Verified crates.io release from `{sha}`.\n\n| Crate | Version | Result |\n| --- | --- | --- |\n'
    notes += '\n'.join(f"| `{r['name']}` | {r['version']} | {r['status']} |" for r in rows) + '\n'
    return tag, sha, notes


def finalize(root, report, repo):
    _, sha, _ = release_metadata(report)
    if git(root, 'rev-parse', 'HEAD') != sha:
        raise ValueError('Report does not match checked-out commit')
    source_date = git(root, 'show', '-s', '--format=%cI', sha)
    tag, sha, notes = release_metadata(report, source_date)
    # Look up before creating: existing tags must identify exactly this source.
    remote = git(root, 'ls-remote', 'origin', 'refs/tags/' + tag, 'refs/tags/' + tag + '^{}')
    if remote:
        refs = dict(line.split()[::-1] for line in remote.splitlines())
        if refs.get('refs/tags/' + tag + '^{}', refs.get('refs/tags/' + tag)) != sha:
            raise ValueError('Existing release tag targets a different commit')
    else:
        subprocess.run(['gh', 'api', '--method', 'POST', f'repos/{repo}/git/refs', '-f', 'ref=refs/tags/' + tag, '-f', 'sha=' + sha], check=True)
    path = root / '.git' / 'daily-release-notes.md'
    path.write_text(notes)
    existing = subprocess.run(['gh', 'release', 'view', tag, '--repo', repo], capture_output=True)
    command = ['gh', 'release', 'edit' if existing.returncode == 0 else 'create', tag, '--repo', repo,
               '--title', 'XGPUI ' + tag.removeprefix('release-'), '--notes-file', str(path)]
    if existing.returncode != 0:
        command.append('--verify-tag')
    subprocess.run(command, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['prepare', 'finalize'])
    parser.add_argument('--baseline')
    parser.add_argument('--apply', action='store_true')
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--notes', type=Path)
    parser.add_argument('--repo')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    if args.command == 'prepare':
        plan = prepare(root, args.baseline)
        if args.apply:
            apply(root, plan)
        args.report.write_text(json.dumps(plan, indent=2) + '\n')
        if args.notes:
            args.notes.write_text(preparation_notes(plan))
    else:
        if not args.repo:
            parser.error('--repo required for finalization')
        finalize(root, json.loads(args.report.read_text()), args.repo)


if __name__ == '__main__':
    main()
