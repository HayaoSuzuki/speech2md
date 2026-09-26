#!/usr/bin/env python3
"""Version and artifact metadata used by the release workflow (Python 3.11+)."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tomllib

PLATFORMS = ('windows-x86_64', 'linux-x86_64', 'macos-aarch64', 'macos-x86_64')
SEMVER = re.compile(r'v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)')
VERSION = re.compile(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-preview\.[0-9]+)?')


def next_tag(tags, base_version):
    versions = [tuple(map(int, match.groups())) for tag in tags if (match := SEMVER.fullmatch(tag))]
    if not versions:
        if not SEMVER.fullmatch('v' + base_version):
            raise ValueError('initial version must be MAJOR.MINOR.PATCH')
        return 'v' + base_version
    major, minor, patch = max(versions)
    return f'v{major}.{minor}.{patch + 1}'


def reserve_tag(root, commit, base_version):
    if not re.fullmatch(r'[0-9a-f]{40}', commit):
        raise ValueError('a full merge commit SHA is required')
    def git(*args):
        return subprocess.check_output(['git', '-C', str(root), *args], text=True).strip()
    for _ in range(5):
        git('fetch', 'origin', '--tags')
        existing = [tag for tag in git('tag', '--points-at', commit).splitlines() if SEMVER.fullmatch(tag)]
        if existing:
            return max(existing, key=lambda tag: tuple(map(int, SEMVER.fullmatch(tag).groups())))
        tag = next_tag(git('tag', '--list').splitlines(), base_version)
        git('tag', tag, commit)
        pushed = subprocess.run(['git', '-C', str(root), 'push', 'origin', f'refs/tags/{tag}'])
        if pushed.returncode == 0:
            return tag
        # Another merge may have reserved this number. Refresh and retry without
        # force-pushing. A successful push with a lost response is also recovered.
        git('tag', '--delete', tag)
    raise RuntimeError('could not reserve a release tag after five attempts')


def engine_manifest(directory, repository, tag):
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository):
        raise ValueError('invalid GitHub repository')
    if not tag.startswith('v') or not VERSION.fullmatch(tag[1:]):
        raise ValueError('invalid release tag')
    artifacts = []
    for platform in PLATFORMS:
        windows = platform == 'windows-x86_64'
        name = f'speech2md-whispercpp-v1.9.4-{platform}.' + ('zip' if windows else 'tar.gz')
        path = Path(directory) / name
        data = path.read_bytes()
        if not data:
            raise ValueError(f'empty engine archive: {name}')
        artifacts.append(dict(version=f'whispercpp-v1.9.4-speech2md.{tag}', platform=platform,
                              url=f'https://github.com/{repository}/releases/download/{tag}/{name}',
                              size=len(data), sha256=hashlib.sha256(data).hexdigest(),
                              archive_name=name, executable_path='bin/whisper-cli' + ('.exe' if windows else '')))
    return {'artifacts': artifacts}


def set_version(root, version):
    if not VERSION.fullmatch(version):
        raise ValueError('invalid release version')
    for relative, pattern in (
        ('crates/speech2md-cli/Cargo.toml', r'(?m)^(version\s*=\s*")[^"]+("\s*)$'),
        ('Cargo.lock', r'(\[\[package\]\]\nname = "speech2md-cli"\nversion = ")[^"]+("\n)'),
    ):
        path = Path(root) / relative
        contents, count = re.subn(pattern, lambda m: m[1] + version + m[2], path.read_text())
        if count != 1:
            raise ValueError(f'expected exactly one CLI version in {relative}')
        path.write_text(contents)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    tag = commands.add_parser('tag')
    tag.add_argument('--commit', required=True)
    manifest = commands.add_parser('manifest')
    manifest.add_argument('--directory', type=Path, required=True)
    manifest.add_argument('--repository', required=True)
    manifest.add_argument('--tag', required=True)
    manifest.add_argument('--output', type=Path, required=True)
    version = commands.add_parser('set-version')
    version.add_argument('version')
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    if args.command == 'tag':
        base = tomllib.loads((root / 'crates/speech2md-cli/Cargo.toml').read_text())['package']['version']
        print(reserve_tag(root, args.commit, base))
    elif args.command == 'manifest':
        result = engine_manifest(args.directory, args.repository, args.tag)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, indent=2) + '\n')
    else:
        set_version(root, args.version)


if __name__ == '__main__':
    main()
