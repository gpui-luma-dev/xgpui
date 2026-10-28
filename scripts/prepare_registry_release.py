"""Prepare a core-first release plan and optional offline archives; never contact a registry.

No upload mode, credential access, registry lookup, or publication flag changes.
Python 3.11+. Outputs must be outside the checkout.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tomllib

from publish_preflight import audit, dependency_layers
from verify_release_packages import inspect_archive


def release_plan(root):
    inventory = audit(root)
    rows = {row['name']: row for row in inventory['packages']}
    identities = json.loads((root / 'scripts/release-packages.json').read_text())['packages']
    framework = {name for local, name in identities.items() if not local.startswith('xgpui-')}
    extensions = set(rows) - framework
    if rows.get('xgpui') is None or 'xgpui' not in framework:
        raise ValueError('Expected xgpui as the framework core')
    graph = {name: {dep['package'] for dep in row['dependencies']
                    if dep['kind'] != 'dev-dependencies' and dep['package'] in rows}
             for name, row in rows.items()}
    if any(graph[name] & extensions for name in framework):
        raise ValueError('Framework depends on a child extension; review the hierarchy')
    ordered = []
    phases = []
    for phase, names in [('framework', framework), ('extensions', extensions)]:
        layers, blocked = dependency_layers({name: graph[name] & names for name in names})
        if blocked:
            raise ValueError(f'{phase} dependency cycle: {blocked}')
        sequence = [name for layer in layers for name in layer]
        phases.append({'name': phase, 'packages': sequence})
        ordered.extend(sequence)
    packages = []
    for name in ordered:
        row = rows[name]
        for issue in row['issues']:
            if issue.startswith(('release-map-identity-mismatch', 'internal-version-mismatch',
                                 'internal-path-mismatch', 'unresolved-workspace-dependency')):
                raise ValueError(f'{name}: {issue}')
        packages.append({
            'name': name, 'version': row['version'], 'manifest': row['manifest'],
            'role': 'core' if name == 'xgpui' else ('framework' if name in framework else 'extension'),
            'dependencies': sorted(graph[name]), 'publish_policy': row['publish'],
            'review_items': row['issues'],
        })
    return {
        'schema_version': 1, 'registry_access': False, 'uploads_enabled': False,
        'release_ready': False, 'core_package': 'xgpui',
        'phases': phases, 'packages': packages,
        'development_cycles': inventory['including_dev_cycle_blocked_packages'],
        'patches_to_review': inventory['patches_to_review_outside_repository'],
        'approval_required_before': [
            'any crates.io API/index/download request, including availability checks',
            'registry-backed cargo package or cargo publish --dry-run',
            'credentials/ownership setup or enabling extension publication',
            'any upload, first registration, tag or GitHub release',
        ],
        'next_registry_phase': [
            'Present exact names, versions, ownership, scope, commands and network activity for approval.',
            'After approval, check availability/ownership and resolve registry dependencies without checkout patches.',
            'Verify registry-equivalent packaging, including development cycles and Git dependency fallbacks.',
            'Upload in framework-first dependency order; confirm registry visibility before each dependent.',
            'Resume only the same commit after comparing already-published archive contents; stop on mismatch.',
            'Verify downloaded content and version-only consumers before calling a release complete.',
        ],
    }


def offline_archives(root, output, plan):
    # Share only cached registry files. Do not inherit credentials or Cargo config.
    env = {key: value for key, value in os.environ.items()
           if not key.startswith('CARGO_') and key not in {
               'RUSTFLAGS', 'RUSTDOCFLAGS', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER',
               'RUSTC', 'RUSTDOC', 'RUSTUP_TOOLCHAIN'}}
    home = output / 'cargo-home'
    home.mkdir()
    cached_home = Path(os.environ.get('CARGO_HOME', Path.home() / '.cargo'))
    if (cached_home / 'registry').is_dir():
        (home / 'registry').symlink_to(cached_home / 'registry', target_is_directory=True)
    env.update(CARGO_HOME=str(home), CARGO_TARGET_DIR=str(output / 'target'), CARGO_NET_OFFLINE='true')
    for row in plan['packages']:
        source = root / row['manifest']
        stage = output / 'staged' / source.parent.relative_to(root)
        shutil.copytree(source.parent, stage,
                        ignore=shutil.ignore_patterns('target', 'Cargo.lock', '.cargo', '.git'))
    for row in plan['packages']:
        manifest = output / 'staged' / row['manifest']
        command = ['cargo', 'package', '--offline', '--no-verify', '--exclude-lockfile',
                   '--manifest-path', str(manifest)]
        log = output / (row['name'] + '.log')
        with log.open('w') as stream:
            subprocess.run(command, cwd=output, env=env, stdout=stream,
                           stderr=subprocess.STDOUT, check=True)
        package_id = row['name'] + '-' + row['version']
        archive = output / 'target/package' / (package_id + '.crate')
        _, files = inspect_archive(archive, manifest.parent, output / 'unpacked', package_id)
        row['archive'] = {'path': str(archive), 'sha256': hashlib.sha256(archive.read_bytes()).hexdigest(),
                          'files': len(files), 'inspection': 'passed', 'command': command}
    plan['archive_scope'] = 'Offline content inspection only; no registry resolution or Cargo build verification.'


def prepare(root, output, archives=False):
    root = root.resolve()
    output = output.resolve()
    if output == root or root in output.parents:
        raise ValueError('Output must be outside the repository')
    plan = release_plan(root)
    plan['source_commit'] = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
    plan['source_dirty'] = bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=root, text=True).strip())
    output.mkdir(parents=True, exist_ok=False)
    report = output / 'release-plan.json'
    report.write_text(json.dumps(plan, indent=2) + '\n')
    try:
        if archives:
            offline_archives(root, output, plan)
        plan['preparation_status'] = 'passed'
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        plan['preparation_status'] = 'failed'
        plan['error'] = str(error)
        raise
    finally:
        report.write_text(json.dumps(plan, indent=2) + '\n')
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--archives', action='store_true', help='Inspect all archives using offline Cargo only')
    args = parser.parse_args()
    print(prepare(Path(__file__).resolve().parents[1], args.out, args.archives))


if __name__ == '__main__':
    main()
