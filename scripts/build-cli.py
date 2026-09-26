#!/usr/bin/env python3
"""Build, package, and smoke-test the CLI on its target OS."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import tarfile
import tempfile
import zipfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    targets = {
        ('Windows', 'amd64'): ('windows-x86_64', 'x86_64-pc-windows-msvc'),
        ('Linux', 'x86_64'): ('linux-x86_64', 'x86_64-unknown-linux-gnu'),
        ('Darwin', 'arm64'): ('macos-aarch64', 'aarch64-apple-darwin'),
        ('Darwin', 'x86_64'): ('macos-x86_64', 'x86_64-apple-darwin'),
    }
    host = (platform.system(), platform.machine().lower())
    if host not in targets:
        parser.error(f'unsupported build platform: {host}')
    name, target = targets[host]
    root = Path(__file__).resolve().parent.parent
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--no-deps', '--format-version', '1', '--locked'], cwd=root))
    version = next(p['version'] for p in metadata['packages'] if p['name'] == 'yasumaro-cli')
    subprocess.run(['cargo', 'build', '--release', '-p', 'yasumaro-cli', '--locked', '--target', target], cwd=root, check=True)
    windows = host[0] == 'Windows'
    executable = 'yasumaro.exe' if windows else 'yasumaro'
    binary = Path(metadata['target_directory']) / target / 'release' / executable
    files = [(binary, f'bin/{executable}', 0o755)]
    files += [(root / relative, relative, 0o644) for relative in
              ('LICENSE', 'README.md', 'engines/README.md', 'engines/manifest.json')]
    archive = output / (f'yasumaro-v{version}-{name}.' + ('zip' if windows else 'tar.gz'))
    if windows:
        with zipfile.ZipFile(archive, 'w', compression=zipfile.ZIP_DEFLATED) as package:
            for source, relative, mode in files:
                info = zipfile.ZipInfo(relative, date_time=(1980, 1, 1, 0, 0, 0))
                info.external_attr = (0o100000 | mode) << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                package.writestr(info, source.read_bytes())
    else:
        with archive.open('wb') as raw:
            with gzip.GzipFile(filename='', fileobj=raw, mode='wb', mtime=0) as gz:
                with tarfile.open(fileobj=gz, mode='w', format=tarfile.GNU_FORMAT) as package:
                    for source, relative, mode in files:
                        info = package.gettarinfo(source, arcname=relative)
                        info.uid = info.gid = 0
                        info.uname = info.gname = 'root'
                        info.mtime = 0
                        info.mode = mode
                        with source.open('rb') as data:
                            package.addfile(info, data)
    # Run the extracted executable so packaging mistakes fail before upload.
    with tempfile.TemporaryDirectory(prefix='yasumaro-cli-verify-') as tmp:
        if windows:
            with zipfile.ZipFile(archive) as package:
                package.extractall(tmp)
        else:
            with tarfile.open(archive) as package:
                package.extractall(tmp, filter='data')
        exe = str(Path(tmp) / 'bin' / executable)
        reported = subprocess.check_output([exe, '--version'], text=True).strip()
        if reported != f'yasumaro {version}':
            raise RuntimeError(f'unexpected CLI version: {reported}')
        print(reported)
        subprocess.run([exe, '--help'], check=True, stdout=subprocess.DEVNULL)
        subprocess.run([exe, 'doctor'], check=True)
        listing = subprocess.check_output([exe, 'engine', 'list'], text=True)
        manifest = json.loads((root / 'engines/manifest.json').read_text())
        for spec in manifest['artifacts']:
            if f'{spec["platform"]} {spec["version"]}:' not in listing:
                raise RuntimeError('packaged CLI has an outdated engine manifest')
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    Path(str(archive) + '.sha256').write_text(f'{digest}  {archive.name}\n')
    print(archive)
    print(f'sha256: {digest}')


if __name__ == '__main__':
    main()
