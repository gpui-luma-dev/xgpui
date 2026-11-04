"""Detect unpublished crate changes using an epoch and verified release tags.

Directory diffs conservatively include tests, examples and crate documentation.
They are not a replacement for comparing normalized Cargo archive payloads.
"""

import json
import os
from pathlib import Path
import subprocess
import tomllib

from publish_extensions import fetch
from publish_preflight import dependencies


def git(root, *args):
    return subprocess.check_output(['git', *args], cwd=root, text=True, stderr=subprocess.PIPE).strip()


def ancestor(root, older, newer):
    result = subprocess.run(['git', 'merge-base', '--is-ancestor', older, newer], cwd=root,
                            capture_output=True, text=True)
    if result.returncode not in (0, 1):
        raise ValueError(result.stderr.strip())
    return result.returncode == 0


def release_history(repository):
    """Only explicit per-crate verification rows count; draft tags never count."""
    releases = []
    page = 1
    while True:
        batch = json.loads(fetch(f'https://api.github.com/repos/{repository}/releases?per_page=100&page={page}'))
        if not isinstance(batch, list):
            raise ValueError('Unexpected GitHub releases response')
        releases.extend(r for r in batch if not r['draft'])
        if len(batch) < 100:
            return releases
        page += 1


def verified_versions(body):
    versions = {}
    for line in (body or '').splitlines():
        cells = [cell.strip().strip('`') for cell in line.strip().strip('|').split('|')]
        if len(cells) == 3 and cells[2] in ('already-published', 'published-verified'):
            versions[cells[0]] = cells[1]
    return versions


def tag_sources(root, epoch, releases):
    # Require a real tag, not an arbitrary moving branch or revision expression.
    base = git(root, 'rev-parse', '--verify', f'refs/tags/{epoch}^{{commit}}')
    if not ancestor(root, base, 'HEAD'):
        raise ValueError('Epoch tag must be an ancestor of HEAD')
    ordered = {sha: index for index, sha in enumerate(git(root, 'rev-list', '--topo-order', 'HEAD').splitlines())}
    candidates = []
    for release in releases:
        if release.get('draft'):
            continue
        tag = release['tag_name']
        if tag == epoch or not tag.startswith('release-'):
            continue
        versions = verified_versions(release.get('body'))
        if not versions:
            continue
        try:
            sha = git(root, 'rev-parse', '--verify', f'refs/tags/{tag}^{{commit}}')
        except subprocess.CalledProcessError:
            raise ValueError(f'Missing release tag {tag}; run git fetch origin --tags and retry') from None
        if sha == base or sha not in ordered or not ancestor(root, base, sha):
            continue
        candidates.append({'tag': tag, 'commit': sha, 'versions': versions})
    candidates.sort(key=lambda source: ordered[source['commit']])
    return base, candidates


def manifests_at(root, revision):
    mapping = json.loads(git(root, 'show', revision + ':scripts/release-packages.json'))
    upstream = json.loads(git(root, 'show', revision + ':upstream.json'))
    return {name: upstream['packages'].get(local, f'extensions/crates/{local}') + '/Cargo.toml'
            for local, name in mapping['packages'].items()}


def changed_paths(root, commit):
    # Comparing the commit with the working tree includes staged and unstaged
    # edits, deletions and both paths of a rename; add non-ignored new files.
    paths = set()
    for args in (['diff', '--no-renames', '--name-only', '-z', commit, '--'],
                 ['ls-files', '--others', '--exclude-standard', '-z']):
        result = subprocess.check_output(['git', *args], cwd=root)
        paths.update(os.fsdecode(path) for path in result.split(b'\0') if path)
    return paths


def crate_changes(root, manifest, document, changed):
    directory = Path(manifest).parent
    prefix = directory.as_posix() + '/'
    selected = {path for path in changed if path.startswith(prefix) and Path(path).name != 'Cargo.lock'}
    # Cargo can package a readme/license or use a build script outside the crate.
    for key in ('readme', 'license-file', 'build'):
        value = document['package'].get(key)
        if isinstance(value, str):
            path = (root / directory / value).resolve()
            if not path.is_relative_to(root):
                raise ValueError(f'{manifest}: {key} is outside the repository')
            relative = path.relative_to(root).as_posix()
            if relative in changed:
                selected.add(relative)
    return sorted(selected)


def change_plan(root, epoch, repository):
    releases = release_history(repository)
    base, sources = tag_sources(root, epoch, releases)
    epoch_manifests = manifests_at(root, base)
    current = json.loads((root / 'scripts/release-packages.json').read_text())
    upstream = json.loads((root / 'upstream.json').read_text())
    diffs = {}
    rows = []
    for local, name in current['packages'].items():
        manifest = upstream['packages'].get(local, f'extensions/crates/{local}') + '/Cargo.toml'
        document = tomllib.loads((root / manifest).read_text())
        package = document['package']
        version = current.get('versions', {}).get(name, current['version'])
        if (package['name'], package['version']) != (name, version):
            raise ValueError(f'{manifest}: name/version does not match release-packages.json')
        baseline = {'tag': epoch, 'commit': base, 'version': None, 'manifest': None}
        if name in epoch_manifests:
            old = tomllib.loads(git(root, 'show', base + ':' + epoch_manifests[name]))
            baseline['version'] = old['package']['version']
            baseline['manifest'] = epoch_manifests[name]
        for source in sources:
            if name not in source['versions']:
                continue
            historical = manifests_at(root, source['commit'])
            if name not in historical:
                raise ValueError(f"{source['tag']}: verified crate {name} missing from release map")
            old = tomllib.loads(git(root, 'show', source['commit'] + ':' + historical[name]))
            if old['package']['version'] != source['versions'][name]:
                raise ValueError(f"{source['tag']}: verification version for {name} disagrees with tagged manifest")
            baseline = {'tag': source['tag'], 'commit': source['commit'], 'version': source['versions'][name], 'manifest': historical[name]}
            break
        if baseline['commit'] not in diffs:
            diffs[baseline['commit']] = changed_paths(root, baseline['commit'])
        files = crate_changes(root, manifest, document, diffs[baseline['commit']])
        rows.append({'name': name, 'version': version, 'manifest': manifest,
                     'baseline': baseline, 'changed_files': files,
                     'reasons': (['crate files changed'] if files else []) + (['new crate'] if baseline['version'] is None else []),
                     'dependency_changes': [], 'document': document})
    names = {row['name'] for row in rows}
    # Target-specific, optional and versioned development edges matter too.
    graph = {row['name']: {dep['package'] for dep in dependencies(row['document'])
                          if dep['package'] in names and dep.get('version')}
             for row in rows}
    selected = {row['name'] for row in rows if row['reasons']}
    while True:
        added = {name for name, deps in graph.items() if name not in selected and deps & selected}
        if not added:
            break
        selected.update(added)
    versions = {row['name']: row['version'] for row in rows}
    for row in rows:
        row['dependency_changes'] = sorted(graph[row['name']] & selected)
        if row['dependency_changes']:
            row['reasons'].append('internal dependency changes require version/pin review')
        row['changed'] = row['name'] in selected
        row['pin_updates'] = [
            {'dependency': dep['package'], 'current': dep['version'], 'required': '=' + versions[dep['package']]}
            for dep in dependencies(row['document'])
            if dep['package'] in selected and dep.get('version')
            and dep['version'] != '=' + versions[dep['package']]]
        row['publish_policy'] = row.pop('document')['package'].get('publish', True)
    return {'epoch': epoch, 'epoch_commit': base, 'packages': rows}
