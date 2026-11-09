#!/usr/bin/env python3
"""Exercise the Linux installer offline with isolated release assets and HOME."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import shutil
import tempfile
import unittest

INSTALLER = Path(__file__).with_name("install-linux.sh")
VERSION = "2.0.0-alpha.1"


@unittest.skipUnless(sys.platform.startswith("linux"), "Linux installer acceptance requires Linux")
class LinuxInstallerTest(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.home = self.root / "home with spaces"
        self.home.mkdir()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.env = dict(os.environ, HOME=str(self.home), XDG_DATA_HOME=str(self.home / ".local/share"),
                        PATH=f"{self.bin}:{os.environ['PATH']}", FIXTURES=str(self.root))
        self.name = f"T3-Code-Bar-{VERSION}-linux-x86_64.AppImage"
        self.payload = b"synthetic AppImage; must never be executed\n"
        (self.root / "app").write_bytes(self.payload)
        self.release()
        self.checksum()
        self.mock("uname", '#!/bin/sh\ncase "$1" in -s) echo Linux;; -m) echo x86_64;; esac\n')
        self.mock("curl", '''#!/usr/bin/env python3
import os
from pathlib import Path
import sys
args = sys.argv[1:]
url = args[args.index('--output') - 1]
source = 'release.json' if 'api.github.com' in url else ('checksum' if url.endswith('.sha256') else 'app')
Path(args[args.index('--output') + 1]).write_bytes((Path(os.environ['FIXTURES']) / source).read_bytes())
''')

    def mock(self, name, text):
        path = self.bin / name
        path.write_text(text)
        path.chmod(0o755)

    def release(self, names=None):
        names = names if names is not None else [self.name, self.name + ".sha256"]
        base = f"https://github.com/eliophan/t3code-bar/releases/download/v{VERSION}/"
        release = dict(tag_name=f"v{VERSION}", draft=False, assets=[
            dict(name=name, state="uploaded", browser_download_url=base + name.replace(" ", "%20"))
            for name in names])
        (self.root / "release.json").write_text(json.dumps(release))

    def checksum(self, digest=None):
        digest = digest or hashlib.sha256(self.payload).hexdigest()
        (self.root / "checksum").write_text(f"{digest}  {self.name}\n")

    def run_installer(self, *args):
        return subprocess.run(["bash", str(INSTALLER), *args], env=self.env, text=True, capture_output=True)

    def assert_not_installed(self):
        self.assertFalse((self.home / ".local/bin/t3-code-bar").exists())
        self.assertFalse((self.home / ".local/share/applications/t3-code-bar.desktop").exists())

    def test_install_and_refuse_overwrite(self):
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        binary = self.home / ".local/bin/t3-code-bar"
        self.assertEqual(binary.read_bytes(), self.payload)
        self.assertTrue(os.access(binary, os.X_OK))
        launcher = self.home / ".local/share/applications/t3-code-bar.desktop"
        self.assertIn(f'Exec="{binary.resolve()}"', launcher.read_text())
        if shutil.which("desktop-file-validate"):
            validation = subprocess.run(["desktop-file-validate", str(launcher)], capture_output=True, text=True)
            self.assertEqual(validation.returncode, 0, validation.stdout + validation.stderr)
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Already exists", result.stderr)
        self.assertEqual(binary.read_bytes(), self.payload)

    def test_checksum_failure_leaves_no_install(self):
        self.checksum("0" * 64)
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Checksum mismatch", result.stderr)
        self.assert_not_installed()

    def test_missing_architecture_leaves_no_install(self):
        self.release([])
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not published", result.stderr)
        self.assert_not_installed()

    def test_missing_checksum_leaves_no_install(self):
        self.release([self.name])
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not published", result.stderr)
        self.assert_not_installed()

    def test_aarch64_asset_selection(self):
        self.mock("uname", '#!/bin/sh\ncase "$1" in -s) echo Linux;; -m) echo aarch64;; esac\n')
        self.name = self.name.replace("x86_64", "aarch64")
        self.release()
        self.checksum()
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_invalid_version_leaves_no_install(self):
        result = self.run_installer("../bad")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Invalid release version", result.stderr)
        self.assert_not_installed()

    def test_unsafe_desktop_path_leaves_no_install(self):
        for character in ('"', '`', '$', '%', '\\', '\n'):
            with self.subTest(character=character):
                self.env["XDG_DATA_HOME"] = str(self.home / ("unsafe" + character))
                result = self.run_installer()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("XDG_DATA_HOME", result.stderr)
                self.assert_not_installed()


if __name__ == "__main__":
    unittest.main()
