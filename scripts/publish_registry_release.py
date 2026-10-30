"""Map-driven registry planning, dry runs and explicitly authorized publication.

Plan is the default. All candidate archives and registry collisions are checked
before any upload. Cargo runs outside the checkout without its patches/config.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
import tomllib

from prepare_registry_release import release_plan
from publish_extensions import archive_contents, fetch, retry_deadline
from verify_release_packages import inspect_archive


def payload(data, name, version):
    """Compare packaged files and normalized manifest, not Cargo-generated state."""
    prefix = f'{name}-{version}/'
    files = archive_contents(data)
    if not files or any(not key.startswith(prefix) for key in files):
        raise ValueError(f'{name}: unexpected archive root')
    files = {key[len(prefix):]: value for key, value in files.items()}
    manifest = tomllib.loads(files['Cargo.toml'].decode())
    if (manifest['package']['name'], manifest['package']['version']) != (name, version):
        raise ValueError(f'{name}: archive identity mismatch')
    # Publication permissions and generated lock/VCS files do not change the library.
    manifest['package'].pop('publish', None)
    for generated in ('Cargo.toml', 'Cargo.toml.orig', 'Cargo.lock', '.cargo_vcs_info.json'):
        files.pop(generated, None)
    files['Cargo.toml'] = json.dumps(manifest, sort_keys=True).encode()
    return files


def registry_version(name, version):
    data = fetch(f'https://crates.io/api/v1/crates/{name}/{version}', missing_ok=True)
    if data is None:
        return None
    entry = json.loads(data)['version']
    if entry['crate'] != name or entry['num'] != version or entry['yanked']:
        raise ValueError(f'{name}@{version}: unexpected registry identity or yanked version')
    return entry


def compare_published(name, version, archive, entry):
    data = fetch(f'https://static.crates.io/crates/{name}/{name}-{version}.crate')
    if hashlib.sha256(data).hexdigest() != entry['checksum']:
        raise ValueError(f'{name}: registry checksum mismatch')
    local, remote = payload(archive.read_bytes(), name, version), payload(data, name, version)
    changed = sorted(key for key in local.keys() | remote.keys() if local.get(key) != remote.get(key))
    return changed


def isolated_environment(out):
    home = out / 'cargo-home'
    home.mkdir(exist_ok=True)
    cache = Path(os.environ.get('CARGO_HOME', Path.home() / '.cargo')) / 'registry'
    if cache.is_dir() and not (home / 'registry').exists():
        (home / 'registry').symlink_to(cache.resolve(), target_is_directory=True)
    env = {key: value for key, value in os.environ.items()
           if not key.startswith(('CARGO_', 'RUSTUP_')) and key not in {
               'RUSTFLAGS', 'RUSTDOCFLAGS', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'RUSTC', 'RUSTDOC'}}
    env.update(CARGO_HOME=str(home), CARGO_TARGET_DIR=str(out / 'target'),
               CLANG_MODULE_CACHE_PATH=str(out / 'clang-cache'), CARGO_BUILD_JOBS='4',
               CARGO_HTTP_TIMEOUT='30', CARGO_NET_RETRY='1', CARGO_TERM_COLOR='never')
    return env


def execute(args, out, env, row, token=''):
    log = out / f"{row['name']}-{len(row.setdefault('commands', [])) + 1}.log"
    try:
        result = subprocess.run(args, cwd=out, env=env, text=True, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, timeout=1200)
        code, output = result.returncode, result.stdout
    except subprocess.TimeoutExpired as error:
        output = error.stdout or b''
        output = output.decode(errors='replace') if isinstance(output, bytes) else output
        code, output = 124, output + '\nTimed out; verify registry state before retrying.\n'
    if token:
        output = output.replace(token, '[REDACTED]')
    log.write_text(output)
    row['commands'].append({'command': args, 'exit_code': code, 'log': str(log)})
    print(f"{row['name']}: {' '.join(args[:2])} exited {code}; {log}", flush=True)
    return code, output


def package_candidate(row, out, env):
    manifest = out / 'staged' / row['manifest']
    args = ['cargo', 'package', '--no-verify', '--exclude-lockfile', '--manifest-path', str(manifest)]
    code, _ = execute(args, out, env, row)
    if code:
        raise ValueError(f"{row['name']}: archive generation failed; inspect saved log")
    identity = f"{row['name']}-{row['version']}"
    archive = out / 'target/package' / (identity + '.crate')
    _, files = inspect_archive(archive, manifest.parent, out / 'inspected', identity)
    row.update(archive=str(archive), archive_sha256=hashlib.sha256(archive.read_bytes()).hexdigest(), files=len(files))
    payload(archive.read_bytes(), row['name'], row['version'])
    return archive


def verify_candidate(row, out, env):
    manifest = out / 'staged' / row['manifest']
    code, _ = execute(['cargo', 'package', '--registry', 'crates-io', '--manifest-path', str(manifest)], out, env, row)
    if code:
        raise ValueError(f"{row['name']}: registry-backed package/build verification failed; inspect saved log")
    archive = Path(row['archive'])
    if payload(archive.read_bytes(), row['name'], row['version']) != payload(
            (out / 'prepared' / archive.name).read_bytes(), row['name'], row['version']):
        raise ValueError(f"{row['name']}: verified package differs from prepared payload")


def run(root, out, mode='plan', confirmation=''):
    root, out = root.resolve(), out.resolve()
    if out == root or root in out.parents:
        raise ValueError('Output must be outside the checkout')
    if mode not in ('plan', 'dry-run', 'publish'):
        raise ValueError('Unknown release mode')
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
    token = os.environ.get('CARGO_REGISTRY_TOKEN', '')
    if mode == 'publish' and (confirmation != f'publish {commit}' or not token):
        raise ValueError('Publication requires exact confirmation: publish FULL_COMMIT_SHA, and CARGO_REGISTRY_TOKEN')
    if subprocess.check_output(['git', 'status', '--porcelain'], cwd=root, text=True).strip():
        raise ValueError('Release source must be a clean commit')
    plan = release_plan(root)
    out.mkdir(parents=True, exist_ok=False)
    rows = plan['packages']
    report = {'schema_version': 1, 'source_commit': commit, 'mode': mode,
              'uploads_enabled': mode == 'publish', 'release_ready': False,
              'started_at': datetime.now(timezone.utc).isoformat(), 'packages': rows}
    def save():
        tmp = out / 'report.tmp'
        tmp.write_text(json.dumps(report, indent=2) + '\n')
        tmp.replace(out / 'report.json')
    save()
    try:
        for directory in ('gpui/crates', 'extensions/crates'):
            shutil.copytree(root / directory, out / 'staged' / directory,
                            ignore=shutil.ignore_patterns('target', 'Cargo.lock', '.cargo', '.git'))
        (out / 'prepared').mkdir()
        env = isolated_environment(out)
        # Every collision is checked before credentials enter any Cargo child.
        for row in rows:
            archive = package_candidate(row, out, env)
            shutil.copy2(archive, out / 'prepared' / archive.name)
            existing = registry_version(row['name'], row['version'])
            if existing:
                changes = compare_published(row['name'], row['version'], archive, existing)
                row.update(status='version-conflict' if changes else 'already-published', changed_files=changes,
                           registry_checksum=existing['checksum'])
            else:
                row['status'] = 'publish-needed'
            print(f"{row['name']}@{row['version']}: {row['status']}", flush=True)
            save()
        collisions = [r['name'] for r in rows if r['status'] == 'version-conflict']
        if collisions:
            raise ValueError('Bump versions with changed published content: ' + ', '.join(collisions))
        if mode == 'plan':
            report['status'] = 'planned'
            return report
        for row in rows:
            if row['status'] == 'already-published':
                continue
            if row['publish_policy'] is False or row['publish_policy'] == [] or (
                    isinstance(row['publish_policy'], list) and 'crates-io' not in row['publish_policy']):
                raise ValueError(f"{row['name']}: crates.io publication disabled in manifest")
        for row in rows:
            if row['status'] == 'already-published':
                continue
            pending = [dep for dep in row['dependencies'] if next(r for r in rows if r['name'] == dep)['status']
                       not in ('already-published', 'published-verified')]
            if mode == 'dry-run' and pending:
                row.update(status='blocked-unpublished-dependencies', blocked_by=pending)
                save()
                continue
            verify_candidate(row, out, env)
            row['status'] = 'verified-not-uploaded'
            save()
            if mode != 'publish':
                continue
            manifest = out / 'staged' / row['manifest']
            for attempt in range(3):
                entry = registry_version(row['name'], row['version'])
                if entry is None:
                    upload_env = dict(env, CARGO_REGISTRY_TOKEN=token)
                    row['status'] = 'uploading-unverified'
                    save()
                    code, output = execute(['cargo', 'publish', '--locked', '--registry', 'crates-io',
                                            '--manifest-path', str(manifest)], out, upload_env, row, token)
                    for poll in range(13):
                        entry = registry_version(row['name'], row['version'])
                        if entry or code != 0 or poll == 12:
                            break
                        time.sleep(5)
                else:
                    code, output = 0, ''
                if entry:
                    changes = compare_published(row['name'], row['version'], Path(row['archive']), entry)
                    if changes:
                        raise ValueError(f"{row['name']}: uploaded/observed version differs: {changes}")
                    row.update(status='published-verified', registry_checksum=entry['checksum'])
                    save()
                    break
                deadline = retry_deadline(output)
                if deadline is None or attempt == 2:
                    raise ValueError(f"{row['name']}: upload unverified (Cargo exit {code}); inspect before retrying")
                row['status'] = 'waiting-for-registry-quota'
                save()
                while (remaining := deadline - time.time()) > 0:
                    print(f"{row['name']}: quota retry in {remaining:.0f}s", flush=True)
                    time.sleep(min(30, remaining + 1))
            else:
                raise ValueError(f"{row['name']}: retry limit reached")
        report['status'] = 'published-verified' if mode == 'publish' else 'dry-run-complete'
        report['release_ready'] = mode == 'publish' and all(r['status'] in ('already-published', 'published-verified') for r in rows)
        return report
    except Exception as error:
        report.update(status='failed', error=str(error).replace(token, '[REDACTED]') if token else str(error))
        raise
    finally:
        report['finished_at'] = datetime.now(timezone.utc).isoformat()
        save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--mode', choices=('plan', 'dry-run', 'publish'), default='plan')
    parser.add_argument('--confirmation', default='')
    args = parser.parse_args()
    run(Path(__file__).resolve().parents[1], args.out, args.mode, args.confirmation)


if __name__ == '__main__':
    main()
