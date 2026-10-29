"""Registry dry runs and explicitly approved, resumable extension publication.

Only the six fixed extension names at 0.2.0 are supported. Never publishes by
default. Cargo runs outside the checkout without repository patches. Rate-limit
retries occur only after the registry's stated deadline and an absent-version
check. Credentials never enter reports, commands, or saved logs.
"""

import argparse
from datetime import datetime, timezone
from email.utils import parsedate_to_datetime
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import time
import tomllib
import urllib.error
import urllib.request

from verify_release_packages import inspect_archive

PACKAGES = ('xgpui-color', 'xgpui-shader', 'xgpui-svg', 'xgpui-parley', 'xgpui-gamut', 'xgpui-media')
ROOTS = PACKAGES[:4]
VERSION = '0.2.0'
CONFIRMATION = 'publish six extensions 0.2.0 only'
USER_AGENT = 'xgpui-release (https://github.com/gpui-luma-dev/xgpui)'


def archive_contents(data):
    with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as bundle:
        contents = {}
        for entry in bundle:
            if not (entry.isfile() or entry.isdir()) or '..' in Path(entry.name).parts:
                raise ValueError('Unexpected archive entry')
            if entry.isfile():
                if entry.name in contents:
                    raise ValueError('Duplicate archive entry')
                contents[entry.name] = bundle.extractfile(entry).read()
        return contents


def retry_deadline(output):
    plain = re.sub(r'\x1b\[[0-9;]*m', '', output)
    if not re.search(r'\b429\b', plain):
        return None
    match = re.search(r'try again after ([^\n]+? GMT)', plain)
    try:
        return parsedate_to_datetime(match[1]).timestamp() if match else None
    except (ValueError, TypeError):
        return None


def cargo_command(manifest, publish=False, package_only=False):
    if package_only and publish:
        raise ValueError('Conflicting command modes')
    if package_only:
        return ['cargo', 'package', '--registry', 'crates-io', '--manifest-path', str(manifest)]
    return ['cargo', 'publish', *(['--locked'] if publish else ['--dry-run']),
            '--registry', 'crates-io', '--manifest-path', str(manifest)]


def fetch(url, missing_ok=False):
    request = urllib.request.Request(url, headers={'User-Agent': USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.read()
    except urllib.error.HTTPError as error:
        if error.code == 404 and missing_ok:
            return None
        raise


def registry_version(name):
    data = fetch(f'https://crates.io/api/v1/crates/{name}/{VERSION}', missing_ok=True)
    if data is None:
        return None
    version = json.loads(data)['version']
    if version['crate'] != name or version['num'] != VERSION or version['yanked']:
        raise ValueError(f'{name}: unexpected registry version or yanked release')
    return version


def verify_published(name, archive, version):
    data = fetch(f'https://static.crates.io/crates/{name}/{name}-{VERSION}.crate')
    if hashlib.sha256(data).hexdigest() != version['checksum']:
        raise ValueError(f'{name}: downloaded checksum does not match registry')
    if archive_contents(data) != archive_contents(archive.read_bytes()):
        raise ValueError(f'{name}: published content differs; stop and inspect original release artifacts')
    return version['checksum']


def run(root, out, publish=False, confirmation='', resume=False):
    root, out = root.resolve(), out.resolve()
    if out == root or root in out.parents:
        raise ValueError('Output must be outside the checkout')
    token = os.environ.get('CARGO_REGISTRY_TOKEN', '')
    if publish and (confirmation != CONFIRMATION or not token):
        raise ValueError('Explicit six-extension confirmation and CARGO_REGISTRY_TOKEN are required')
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
    if subprocess.check_output(['git', 'status', '--porcelain'], cwd=root, text=True).strip():
        raise ValueError('Release source must be a clean commit')
    for name in PACKAGES:
        package = tomllib.loads((root / 'extensions/crates' / name / 'Cargo.toml').read_text())['package']
        if (package['name'], package['version'], package.get('publish')) != (name, VERSION, ['crates-io']):
            raise ValueError(f'{name}: expected exact version and explicit crates.io publication policy')
    report_file = out / 'report.json'
    if resume:
        report = json.loads(report_file.read_text())
        if report['source_commit'] != commit or list(report['packages']) != list(PACKAGES):
            raise ValueError('Resume must use the original source commit and package scope')
        report['publish_authorized'] = publish
    else:
        out.mkdir(parents=True, exist_ok=False)
        report = {'source_commit': commit, 'version': VERSION, 'publish_authorized': publish,
                  'started_at': datetime.now(timezone.utc).isoformat(),
                  'packages': {name: {'status': 'pending'} for name in PACKAGES}}
        for directory in ('gpui/crates', 'extensions/crates'):
            shutil.copytree(root / directory, out / 'staged' / directory,
                            ignore=shutil.ignore_patterns('target', 'Cargo.lock', '.cargo', '.git'))
    def save():
        temporary = report_file.with_suffix('.tmp')
        temporary.write_text(json.dumps(report, indent=2) + '\n')
        temporary.replace(report_file)
    save()
    home = out / 'cargo-home'
    home.mkdir(exist_ok=resume)
    cached_registry = Path(os.environ.get('CARGO_HOME', Path.home() / '.cargo')) / 'registry'
    if cached_registry.is_dir() and not (home / 'registry').exists():
        (home / 'registry').symlink_to(cached_registry, target_is_directory=True)
    env = {key: value for key, value in os.environ.items()
           if not key.startswith('CARGO_') and key not in (
               'RUSTFLAGS', 'RUSTDOCFLAGS', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'RUSTC', 'RUSTDOC')}
    env.update(CARGO_HOME=str(home), CARGO_TARGET_DIR=str(out / 'target'),
               CLANG_MODULE_CACHE_PATH=str(out / 'clang-cache'), CARGO_BUILD_JOBS='4',
               CARGO_HTTP_TIMEOUT='30', CARGO_NET_RETRY='1', CARGO_TERM_COLOR='never')
    def command(name, upload=False, package_only=False):
        row = report['packages'][name]
        manifest = out / 'staged/extensions/crates' / name / 'Cargo.toml'
        args = cargo_command(manifest, publish=upload, package_only=package_only)
        child_env = env.copy()
        if upload:
            child_env['CARGO_REGISTRY_TOKEN'] = token
        attempt = len(row.setdefault('commands', [])) + 1
        log = out / f'{name}-{attempt}.log'
        print(f'{name}: {"upload" if upload else "package verification"}', flush=True)
        try:
            result = subprocess.run(args, cwd=out, env=child_env, text=True,
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=1200)
            code, output = result.returncode, result.stdout
        except subprocess.TimeoutExpired as error:
            output = error.stdout or b''
            if isinstance(output, bytes):
                output = output.decode(errors='replace')
            code, output = 124, output + '\nCargo command timed out; registry state must be checked.\n'
        if token:
            output = output.replace(token, '[REDACTED]')
        log.write_text(output)
        print(output, end='', flush=True)
        row['commands'].append({'command': args, 'exit_code': code, 'log': str(log)})
        save()
        return code, output
    def dry_run(name):
        row = report['packages'][name]
        if row.get('archive') and Path(row['archive']).is_file():
            archive = Path(row['archive'])
            if hashlib.sha256(archive.read_bytes()).hexdigest() != row['archive_sha256']:
                raise ValueError(f'{name}: saved archive changed')
            return archive
        existing = registry_version(name)
        code, _ = command(name, package_only=existing is not None)
        if code:
            raise ValueError(f'{name}: package verification failed; inspect its saved log')
        filename = f'{name}-{VERSION}.crate'
        candidates = [out / 'target/package/tmp-registry' / filename, out / 'target/package' / filename]
        archive = next((p for p in candidates if p.is_file()), None)
        if archive is None:
            raise ValueError(f'{name}: verified archive not found')
        staged = out / 'staged/extensions/crates' / name
        _, files = inspect_archive(archive, staged, out / 'inspected', f'{name}-{VERSION}')
        row.update(status='dry-run-passed', archive=str(archive), files=len(files),
                   archive_sha256=hashlib.sha256(archive.read_bytes()).hexdigest())
        save()
        return archive
    try:
        # The four independent roots must all verify before the first upload.
        for name in ROOTS:
            dry_run(name)
        for name in PACKAGES:
            row = report['packages'][name]
            archive = dry_run(name)
            version = registry_version(name)
            if version is not None:
                row.update(status='published-verified', registry_checksum=verify_published(name, archive, version))
                save()
                continue
            if not publish:
                continue
            for attempt in range(3):
                # A version may have appeared after a prior upload timed out.
                version = registry_version(name)
                if version is not None:
                    row.update(status='published-verified', registry_checksum=verify_published(name, archive, version))
                    save()
                    break
                row['status'] = 'uploading; outcome not yet verified'
                save()
                code, output = command(name, upload=True)
                version = registry_version(name)
                if version is None and code == 0:
                    for _ in range(12):
                        time.sleep(5)
                        version = registry_version(name)
                        if version is not None:
                            break
                if version is not None:
                    row.update(status='published-verified', registry_checksum=verify_published(name, archive, version))
                    save()
                    break
                deadline = retry_deadline(output)
                if deadline is None or attempt == 2:
                    raise ValueError(f'{name}: upload not verified (Cargo exit {code}); inspect log before retrying')
                row.update(status='waiting for registry quota', retry_after=datetime.fromtimestamp(deadline, timezone.utc).isoformat())
                save()
                while (remaining := deadline - time.time()) > 0:
                    print(f'{name}: registry retry in {remaining:.0f} seconds', flush=True)
                    time.sleep(min(30, remaining + 1))
            else:
                raise ValueError(f'{name}: upload retry limit reached')
        report['status'] = 'published-verified' if publish else 'dry-runs-only'
        report['finished_at'] = datetime.now(timezone.utc).isoformat()
        save()
    except Exception as error:
        message = str(error).replace(token, '[REDACTED]') if token else str(error)
        report.update(status='failed', error=message)
        save()
        raise
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--publish', action='store_true')
    parser.add_argument('--confirmation', default='')
    parser.add_argument('--resume', action='store_true')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    run(root, args.out, args.publish, args.confirmation, args.resume)


if __name__ == '__main__':
    main()
