from pathlib import Path
import subprocess
import shlex
import tempfile
import unittest

import release

class ReleaseTests(unittest.TestCase):
    def test_patch_increment_uses_numeric_order_and_ignores_engine_tags(self):
        self.assertEqual(release.next_tag(['v0.1.9', 'v0.1.10', 'v1.0.0-rc.1',
                                           'whispercpp-v1.9.4-speech2md.1'], '0.1.0'), 'v0.1.11')
        self.assertEqual(release.next_tag(['v2.0.1', 'v1.99.99'], '0.1.0'), 'v2.0.2')
        self.assertEqual(release.next_tag([], '0.1.0'), 'v0.1.0')

    def test_tag_reservation_is_idempotent_and_does_not_move_existing_tags(self):
        with tempfile.TemporaryDirectory() as tmp:
            remote = Path(tmp) / 'remote.git'
            repo = Path(tmp) / 'checkout'
            subprocess.run(['git', 'init', '--bare', str(remote)], check=True, capture_output=True)
            subprocess.run(['git', 'clone', str(remote), str(repo)], check=True, capture_output=True)
            def git(*args):
                return subprocess.check_output(['git', '-C', str(repo), *args], text=True).strip()
            git('config', 'user.name', 'Release test')
            git('config', 'user.email', 'test@example.invalid')
            git('commit', '--allow-empty', '-m', 'first')
            first = git('rev-parse', 'HEAD')
            git('push', 'origin', 'HEAD:main')
            self.assertEqual(release.reserve_tag(repo, first, '0.1.0'), 'v0.1.0')
            self.assertEqual(release.reserve_tag(repo, first, '0.1.0'), 'v0.1.0')
            git('commit', '--allow-empty', '-m', 'second')
            second = git('rev-parse', 'HEAD')
            git('push', 'origin', 'HEAD:main')
            self.assertEqual(release.reserve_tag(repo, second, '0.1.0'), 'v0.1.1')
            self.assertEqual(git('rev-parse', 'v0.1.0'), first)
            self.assertEqual(git('rev-parse', 'v0.1.1'), second)
            self.assertIn(first, git('ls-remote', '--tags', 'origin', 'v0.1.0'))

    def test_tag_collision_with_another_merge_retries_without_retagging(self):
        with tempfile.TemporaryDirectory() as tmp:
            remote = Path(tmp) / 'remote.git'
            repo = Path(tmp) / 'checkout'
            rival = Path(tmp) / 'rival'
            subprocess.run(['git', 'init', '--bare', str(remote)], check=True, capture_output=True)
            subprocess.run(['git', 'clone', str(remote), str(repo)], check=True, capture_output=True)
            def git(*args):
                return subprocess.check_output(['git', '-C', str(repo), *args], text=True).strip()
            git('config', 'user.name', 'Release test')
            git('config', 'user.email', 'test@example.invalid')
            git('commit', '--allow-empty', '-m', 'first')
            first = git('rev-parse', 'HEAD')
            git('push', 'origin', 'HEAD:main')
            subprocess.run(['git', 'clone', '--branch', 'main', str(remote), str(rival)], check=True, capture_output=True)
            git('commit', '--allow-empty', '-m', 'second')
            second = git('rev-parse', 'HEAD')
            git('push', 'origin', 'HEAD:main')
            # Reserve v0.1.0 in a second checkout between the first run's fetch
            # and push. The competing tag must survive, and this run must retry.
            hook = repo / '.git/hooks/pre-push'
            hook.write_text('#!/bin/sh\nrm -- "$0"\n' +
                            'git -C ' + shlex.quote(str(rival)) + ' tag v0.1.0\n' +
                            'git -C ' + shlex.quote(str(rival)) + ' push origin refs/tags/v0.1.0\n')
            hook.chmod(0o755)
            self.assertEqual(release.reserve_tag(repo, second, '0.1.0'), 'v0.1.1')
            self.assertEqual(git('rev-parse', 'v0.1.0'), first)
            self.assertEqual(git('rev-parse', 'v0.1.1'), second)

    def test_manifest_measures_all_four_archives_and_rejects_missing_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            names = ['speech2md-whispercpp-v1.9.4-windows-x86_64.zip',
                     'speech2md-whispercpp-v1.9.4-linux-x86_64.tar.gz',
                     'speech2md-whispercpp-v1.9.4-macos-aarch64.tar.gz',
                     'speech2md-whispercpp-v1.9.4-macos-x86_64.tar.gz']
            for name in names:
                (root / name).write_bytes(b'abc')
            manifest = release.engine_manifest(root, 'HayaoSuzuki/speech2md', 'v0.1.2')
            self.assertEqual(len(manifest['artifacts']), 4)
            for artifact in manifest['artifacts']:
                self.assertEqual(artifact['size'], 3)
                self.assertEqual(artifact['sha256'], 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad')
                self.assertEqual(artifact['url'], 'https://github.com/HayaoSuzuki/speech2md/releases/download/v0.1.2/' + artifact['archive_name'])
            (root / names[-1]).unlink()
            with self.assertRaises(FileNotFoundError):
                release.engine_manifest(root, 'HayaoSuzuki/speech2md', 'v0.1.2')

    def test_version_update_changes_only_cli_and_its_lock_entry(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package = root / 'crates/speech2md-cli/Cargo.toml'
            package.parent.mkdir(parents=True)
            package.write_text('[package]\nname = "speech2md-cli"\nversion = "0.1.0"\n')
            lock = root / 'Cargo.lock'
            lock.write_text('[[package]]\nname = "speech2md-cli"\nversion = "0.1.0"\n\n[[package]]\nname = "other"\nversion = "0.1.0"\n')
            release.set_version(root, '0.1.42')
            self.assertIn('version = "0.1.42"', package.read_text())
            self.assertIn('name = "speech2md-cli"\nversion = "0.1.42"', lock.read_text())
            self.assertIn('name = "other"\nversion = "0.1.0"', lock.read_text())
            with self.assertRaises(ValueError):
                release.set_version(root, 'invalid\nversion')


if __name__ == '__main__':
    unittest.main()
