#!/usr/bin/env python3
"""Package and smoke-test native release executables (Python 3.12+)."""
import argparse
import json
from pathlib import Path, PurePosixPath
import re
import socket
import subprocess
import tarfile
import tempfile
import time
import tomllib
import urllib.error
import urllib.request
import zipfile

DOCUMENTS = ('README.md', 'LICENSE-MIT', 'LICENSE-APACHE', 'config.example.toml')
TARGETS = ('x86_64-unknown-linux-gnu', 'aarch64-apple-darwin',
           'x86_64-apple-darwin', 'x86_64-pc-windows-msvc')


def build_archive(root, binary, output, target):
    if target not in TARGETS:
        raise ValueError('Unsupported native target')
    version = tomllib.loads((root / 'Cargo.toml').read_text())['package']['version']
    if not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', version):
        raise ValueError('Release version must be stable SemVer')
    sources = [(binary, 'second-brain-rs.exe' if target.endswith('msvc') else 'second-brain-rs')]
    sources += [(root / name, name) for name in DOCUMENTS]
    for source, _ in sources:
        if not source.is_file() or source.is_symlink():
            raise FileNotFoundError(f'Missing regular release file: {source}')
    prefix = f'second-brain-rs-v{version}-{target}'
    output.mkdir(parents=True, exist_ok=True)
    archive = output / (prefix + ('.zip' if target.endswith('msvc') else '.tar.gz'))
    if archive.exists():
        raise FileExistsError(archive)
    if target.endswith('msvc'):
        with zipfile.ZipFile(archive, 'w', compression=zipfile.ZIP_DEFLATED) as handle:
            for source, name in sources:
                handle.write(source, f'{prefix}/{name}')
    else:
        with tarfile.open(archive, 'w:gz') as handle:
            for source, name in sources:
                info = handle.gettarinfo(str(source), arcname=f'{prefix}/{name}')
                info.mode = 0o755 if name == 'second-brain-rs' else 0o644
                info.uid = info.gid = 0
                info.uname = info.gname = ''
                with source.open('rb') as content:
                    handle.addfile(info, content)
    return archive


def unpack_archive(archive, destination):
    """Only accept the exact flat public package layout; never extract links."""
    is_zip = archive.suffix == '.zip'
    executable = 'second-brain-rs.exe' if is_zip else 'second-brain-rs'
    with zipfile.ZipFile(archive) if is_zip else tarfile.open(archive, 'r:gz') as handle:
        entries = handle.infolist() if is_zip else handle.getmembers()
        names = [entry.filename if is_zip else entry.name for entry in entries]
        paths = [PurePosixPath(name) for name in names]
        if (len(names) != len(DOCUMENTS) + 1 or len(set(names)) != len(names)
                or any(len(p.parts) != 2 or p.is_absolute() or '..' in p.parts or '\\' in str(p) for p in paths)
                or len({p.parts[0] for p in paths}) != 1
                or {p.name for p in paths} != {executable, *DOCUMENTS}):
            raise ValueError('Unexpected archive contents')
        if not is_zip and any(not entry.isfile() for entry in entries):
            raise ValueError('Archive may only contain regular files')
        # Write only validated regular bytes, with controlled permissions.
        destination.mkdir(parents=True, exist_ok=True)
        for entry, path in zip(entries, paths):
            content = handle.read(entry) if is_zip else handle.extractfile(entry).read()
            target = destination / path.name
            with target.open('xb') as output:
                output.write(content)
            target.chmod(0o755 if path.name == executable else 0o644)
    return destination / executable


def smoke_archive(archive):
    with tempfile.TemporaryDirectory(prefix='second-brain-smoke-') as temporary:
        root = Path(temporary)
        binary = unpack_archive(archive, root / 'package')
        vault = root / 'vault'
        vault.mkdir()
        # Reserve an available loopback port, then give it to the application.
        with socket.socket() as reservation:
            reservation.bind(('127.0.0.1', 0))
            port = reservation.getsockname()[1]
        config = root / 'smoke.toml'
        config.write_text(
            f'listen = "127.0.0.1:{port}"\npublic_base_url = "http://127.0.0.1:{port}"\n'
            f'vault_path = {json.dumps(str(vault))}\nstate_path = {json.dumps(str(root / "state"))}\n'
            '[auth]\nmode = "development"\naudience = "release-smoke"\n'
            'trusted_issuers = ["https://example.invalid/"]\n'
            'discovery_authorization_server = "https://example.invalid/"\n'
            'jwks_cache_ttl_seconds = 60\n[index]\nwatcher_polling = false\nignored_globs = []\n'
            '[writes]\ncooldown_seconds = 0\n[daily_note]\ncapture_default_pattern = "B"\n'
            '[logging]\nlog_args = false\n', encoding='utf-8')
        with (root / 'process.log').open('w+b') as log:
            process = subprocess.Popen([str(binary), '--config', str(config)], stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 30
                # Do not route the local smoke request through proxy environment settings.
                opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
                while process.poll() is None and time.monotonic() < deadline:
                    try:
                        with opener.open(f'http://127.0.0.1:{port}/healthz', timeout=1) as response:
                            if response.status == 200 and json.load(response) == {'ok': True}:
                                print(f'Packaged executable smoke test passed: {archive.name}')
                                return
                    except (urllib.error.URLError, TimeoutError, ConnectionError):
                        pass
                    time.sleep(0.1)
                log.flush()
                log.seek(0)
                raise RuntimeError('Packaged executable failed health check: ' + log.read().decode(errors='replace'))
            finally:
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=10)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    package = commands.add_parser('package')
    package.add_argument('--target', choices=TARGETS, required=True)
    package.add_argument('--binary', type=Path, required=True)
    package.add_argument('--out', type=Path, default=Path('dist'))
    smoke = commands.add_parser('smoke')
    smoke.add_argument('--archive', type=Path, required=True)
    args = parser.parse_args()
    if args.command == 'package':
        print(build_archive(Path.cwd(), args.binary, args.out, args.target))
    else:
        smoke_archive(args.archive)


if __name__ == '__main__':
    main()
