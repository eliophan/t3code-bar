#!/usr/bin/env python3
"""Exercise updater metadata against a disposable Tauri signing key."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import plistlib
import shutil
import struct
import tarfile
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('package-macos.py')
CLI = os.environ.get('TAURI_CLI', str(SCRIPT.parents[1] / 'desktop/node_modules/.bin/tauri'))
spec = importlib.util.spec_from_file_location('package_macos', SCRIPT)
packaging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packaging)
cask_spec = importlib.util.spec_from_file_location('homebrew_cask', SCRIPT.with_name('homebrew-cask.py'))
homebrew = importlib.util.module_from_spec(cask_spec)
cask_spec.loader.exec_module(homebrew)


def app_archive(path, version='2.0.0-alpha.1', cpu=0x0100000c):
    plist = plistlib.dumps({'CFBundleShortVersionString': version, 'CFBundleIdentifier': 'com.t3code.bar.tauri', 'CFBundleExecutable': 'bar'})
    binary = b'\xcf\xfa\xed\xfe' + struct.pack('<I', cpu) + bytes(24)
    with tarfile.open(path, 'w:gz') as bundle:
        for name, data in [('Contents/Info.plist', plist), ('Contents/MacOS/bar', binary)]:
            member = tarfile.TarInfo(packaging.APP + '/' + name)
            member.size = len(data)
            bundle.addfile(member, io.BytesIO(data))


class ArchivePayloadTest(unittest.TestCase):
    def test_payload_binds_version_and_architecture(self):
        with tempfile.TemporaryDirectory() as temp:
            archive = Path(temp) / 'app.tar.gz'
            app_archive(archive)
            packaging.verify_bundle_archive(archive, '2.0.0-alpha.1', 'darwin-aarch64')
            with self.assertRaisesRegex(ValueError, 'version mismatch'):
                packaging.verify_bundle_archive(archive, '99.0.0', 'darwin-aarch64')
            with self.assertRaisesRegex(ValueError, 'target mismatch'):
                packaging.verify_bundle_archive(archive, '2.0.0-alpha.1', 'darwin-x86_64')

    def test_checksum_sidecar_matches_archive_and_rejects_modified_bytes(self):
        with tempfile.TemporaryDirectory() as temp:
            archive = Path(temp) / 'T3-Code-Bar-demo.app.tar.gz'
            app_archive(archive)
            sidecar = packaging.write_checksum(archive)
            checked = subprocess.run(['shasum', '-a', '256', '-c', sidecar.name], cwd=temp, capture_output=True)
            self.assertEqual(checked.returncode, 0, checked.stderr)
            self.assertIn(b'OK', checked.stdout)
            archive.write_bytes(b'tampered')
            checked = subprocess.run(['shasum', '-a', '256', '-c', sidecar.name], cwd=temp, capture_output=True)
            self.assertNotEqual(checked.returncode, 0)

    def test_unknown_cpu_payload_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            archive = Path(temp) / 'app.tar.gz'
            app_archive(archive, cpu=0)
            with self.assertRaisesRegex(ValueError, 'target mismatch'):
                packaging.verify_bundle_archive(archive, '2.0.0-alpha.1', 'darwin-aarch64')


class PackageMetadataTest(unittest.TestCase):
    def setUp(self):
        if not shutil.which('minisign'):
            self.skipTest('minisign prerequisite is not installed; signature verification was not run')
        self.tmp = tempfile.TemporaryDirectory(prefix='bar-package-test-')
        self.addCleanup(self.tmp.cleanup)
        self.directory = Path(self.tmp.name)
        self.key = self.directory / 'synthetic.key'
        subprocess.run([CLI, 'signer', 'generate', '--ci', '--password', '', '--write-keys', str(self.key)], check=True, capture_output=True)
        self.public_key = Path(str(self.key) + '.pub').read_text().strip()
        self.env = dict(os.environ, BAR_UPDATER_PUBLIC_KEY=self.public_key, TAURI_SIGNING_PRIVATE_KEY_PATH=str(self.key), TAURI_SIGNING_PRIVATE_KEY_PASSWORD='')
        self.version = '2.0.0-alpha.1'
        self.target = 'darwin-aarch64'
        self.name = f'T3-Code-Bar-{self.version}-{self.target}.app.tar.gz'
        self.archive = self.directory / self.name
        app_archive(self.archive)
        subprocess.run([CLI, 'signer', 'sign', '--app-version', self.version, str(self.archive)], env=self.env, check=True, capture_output=True)
        self.sig = self.directory / (self.name + '.sig')
        self.manifest = self.directory / 'manifest.json'
        self.manifest.write_text(json.dumps({'version': self.version, 'tag_name': 'v' + self.version, 'platforms': {self.target: {'url': 'https://example.org/' + self.name, 'signature': self.sig.read_text().strip()}}}))
        sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
        self.metadata = self.directory / 'metadata.json'
        self.metadata.write_text(json.dumps({'version': self.version, 'target': self.target, 'archive': self.name, 'archive_sha256': sha(self.archive), 'signature_sha256': sha(self.sig), 'manifest_sha256': sha(self.manifest)}))

    def verify(self):
        return subprocess.run(['python3', str(SCRIPT), 'verify', '--output', str(self.directory)], env=self.env, capture_output=True)

    def test_accepts_signed_metadata_and_rejects_tampering(self):
        result = self.verify()
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.archive.write_bytes(b'changed')
        self.assertNotEqual(self.verify().returncode, 0)
        metadata = json.loads(self.metadata.read_text())
        metadata['archive_sha256'] = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        self.metadata.write_text(json.dumps(metadata))
        self.assertNotEqual(self.verify().returncode, 0, 'updated hash must not bypass signature verification')

    def test_rejects_wrong_target_and_swift_zip(self):
        manifest = json.loads(self.manifest.read_text())
        manifest['platforms'][self.target]['url'] = 'https://example.org/swift.zip'
        self.manifest.write_text(json.dumps(manifest))
        metadata = json.loads(self.metadata.read_text())
        metadata['manifest_sha256'] = hashlib.sha256(self.manifest.read_bytes()).hexdigest()
        self.metadata.write_text(json.dumps(metadata))
        self.assertNotEqual(self.verify().returncode, 0)
        manifest['platforms'][self.target]['url'] = 'https://example.org/' + self.name
        self.manifest.write_text(json.dumps(manifest))
        metadata['manifest_sha256'] = hashlib.sha256(self.manifest.read_bytes()).hexdigest()
        metadata['target'] = 'darwin-x86_64'
        self.metadata.write_text(json.dumps(metadata))
        self.assertNotEqual(self.verify().returncode, 0)

    def test_rejects_another_public_key(self):
        other = self.directory / 'other-synthetic.key'
        subprocess.run([CLI, 'signer', 'generate', '--ci', '--password', '', '--write-keys', str(other)], check=True, capture_output=True)
        self.env['BAR_UPDATER_PUBLIC_KEY'] = Path(str(other) + '.pub').read_text().strip()
        self.assertNotEqual(self.verify().returncode, 0)

    def test_rejects_coordinated_metadata_rebinding(self):
        metadata = json.loads(self.metadata.read_text())
        version, target = '99.0.0', 'darwin-x86_64'
        name = f'T3-Code-Bar-{version}-{target}.app.tar.gz'
        archive = self.directory / name
        signature = self.directory / (name + '.sig')
        self.archive.rename(archive)
        self.sig.rename(signature)
        self.manifest.write_text(json.dumps({'version': version, 'tag_name': 'v' + version, 'platforms': {target: {'url': 'https://example.org/' + name, 'signature': signature.read_text().strip()}}}))
        metadata.update(version=version, target=target, archive=name, manifest_sha256=hashlib.sha256(self.manifest.read_bytes()).hexdigest())
        self.metadata.write_text(json.dumps(metadata))
        self.assertNotEqual(self.verify().returncode, 0, 'a valid signature cannot authenticate rewritten version/target metadata')

    def intel_package(self, version=None):
        version = version or self.version
        directory = self.directory / 'intel'
        directory.mkdir()
        target = 'darwin-x86_64'
        name = f'T3-Code-Bar-{version}-{target}.app.tar.gz'
        archive = directory / name
        app_archive(archive, version=version, cpu=0x01000007)
        subprocess.run([CLI, 'signer', 'sign', '--app-version', version, str(archive)], env=self.env, check=True, capture_output=True)
        sig = directory / (name + '.sig')
        manifest = directory / 'manifest.json'
        manifest.write_text(json.dumps({'version': version, 'tag_name': 'v' + version, 'platforms': {
            target: {'url': 'https://example.org/' + name, 'signature': sig.read_text().strip()}}}))
        (directory / 'metadata.json').write_text(json.dumps({
            'version': version, 'target': target, 'archive': name,
            'archive_sha256': packaging.digest(archive), 'signature_sha256': packaging.digest(sig),
            'manifest_sha256': packaging.digest(manifest)}))
        return directory

    def test_merge_preserves_both_verified_platforms(self):
        intel = self.intel_package()
        output = self.directory / 'merged'
        packaging.merge_packages([self.directory, intel], output, self.public_key)
        manifest = json.loads((output / 'manifest.json').read_text())
        self.assertEqual(set(manifest['platforms']), {'darwin-aarch64', 'darwin-x86_64'})
        for package in (self.directory, intel):
            source = json.loads((package / 'manifest.json').read_text())
            for target, entry in source['platforms'].items():
                self.assertEqual(manifest['platforms'][target], entry)
        self.assertFalse(json.loads((output / 'metadata.json').read_text())['published'])
        with self.assertRaisesRegex(ValueError, 'already exists'):
            packaging.merge_packages([self.directory, intel], output, self.public_key)

    def test_merge_rejects_missing_duplicate_and_mixed_versions(self):
        output = self.directory / 'merged'
        with self.assertRaisesRegex(ValueError, 'both macOS architecture'):
            packaging.merge_packages([self.directory], output, self.public_key)
        with self.assertRaisesRegex(ValueError, 'duplicate package target'):
            packaging.merge_packages([self.directory, self.directory], output, self.public_key)
        intel = self.intel_package('2.0.0-alpha.3')
        with self.assertRaisesRegex(ValueError, 'versions differ'):
            packaging.merge_packages([self.directory, intel], output, self.public_key)
        self.assertFalse(output.exists())

    def test_merge_rejects_tampered_architecture(self):
        intel = self.intel_package()
        archive = next(intel.glob('*.app.tar.gz'))
        archive.write_bytes(b'tampered')
        output = self.directory / 'merged'
        with self.assertRaisesRegex(ValueError, 'hash mismatch'):
            packaging.merge_packages([self.directory, intel], output, self.public_key)
        self.assertFalse(output.exists())


    def github_asset(self, directory):
        metadata_path = directory / 'metadata.json'
        metadata = json.loads(metadata_path.read_text())
        manifest_path = directory / 'manifest.json'
        manifest = json.loads(manifest_path.read_text())
        manifest['platforms'][metadata['target']]['url'] = f'https://github.com/eliophan/t3code-bar/releases/download/v{metadata["version"]}/{metadata["archive"]}'
        manifest_path.write_text(json.dumps(manifest))
        metadata['manifest_sha256'] = hashlib.sha256(manifest_path.read_bytes()).hexdigest()
        metadata_path.write_text(json.dumps(metadata))

    def test_homebrew_arm_only_and_both_architectures(self):
        self.github_asset(self.directory)
        cask = homebrew.generate([self.directory], self.public_key)
        self.assertIn('depends_on arch: :arm64', cask)
        self.assertIn('app "T3 Code Bar Tauri.app"', cask)
        intel = self.intel_package()
        self.github_asset(intel)
        cask = homebrew.generate([self.directory, intel], self.public_key)
        self.assertIn('sha256 arm:', cask)
        self.assertIn('intel:', cask)
        self.assertNotIn('depends_on arch:', cask)
        ruby = self.directory / 't3-code-bar.rb'
        ruby.write_text(cask)
        subprocess.run(['ruby', '-c', str(ruby)], check=True, capture_output=True)

    def test_homebrew_rejects_wrong_release_url(self):
        with self.assertRaisesRegex(ValueError, 'GitHub Release asset'):
            homebrew.generate([self.directory], self.public_key)

    def test_homebrew_rejects_tampered_archive(self):
        self.github_asset(self.directory)
        self.archive.write_bytes(b'tampered')
        with self.assertRaisesRegex(ValueError, 'hash mismatch'):
            homebrew.generate([self.directory], self.public_key)


if __name__ == '__main__':
    unittest.main()
