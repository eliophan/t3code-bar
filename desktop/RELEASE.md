# Release packaging

The current version is an alpha. Build and test the actual packages before distributing them. These commands do not publish or install anything. Old Swift v1 release assets are not compatible with Tauri updates.

## Package Linux builds

Install the Linux native dependencies listed in `.github/workflows/ci.yml`, the pinned Rust toolchain, Node.js, and the locked npm dependencies in `desktop/`. Build both formats from one source snapshot:

```sh
python3 scripts/package-linux.py --output /var/tmp/t3-linux-packages
```

A nonzero exit means packaging failed. The output directory must not exist. Success creates one `.deb`, one `.AppImage`, SHA-256 sidecars, `build.log`, and `receipt.json`. The receipt records each product source file, including untracked additions, the Git revision and diff, build host, binary hash, and package hashes. Source changes during the build reject the result. `--target-dir` optionally reuses a Cargo cache; the helper clears its Linux bundle directories before building.

The Linux build links gtk-layer-shell (development package version 0.6 or newer). Wayland requires a compositor with layer-shell protocol version 4 for on-demand keyboard focus. The Debian package must depend on `libgtk-layer-shell0`; the AppImage must contain `libgtk-layer-shell.so.0`. Check both generated artifacts for those requirements before distribution.

Linux requires a StatusNotifier tray host that accepts the app's own icon. Omarchy's shell supplies this host. Without it, the app must show only a native Retry/Quit diagnostic. Registration alone does not prove the icon is visible or clickable. Run the packaged app on the target desktop and exercise Show, Hide, Settings, Quit, tray loss, and recovery before distributing it. Use the project `verify-t3-code-bar` skill for isolated native evidence and `verify-linux-tray` for policy checks.

The `.deb` targets Debian-family systems. Omarchy uses the AppImage. If FUSE is unavailable, try `APPIMAGE_EXTRACT_AND_RUN=1` for that process. Neither format guarantees compatibility with older glibc or every compositor. CI builds on Ubuntu 24.04; a local Omarchy build records its own libc in the receipt. A successful build does not establish a supported distro matrix. Linux updates use manual package replacement in this lane; no updater manifest is published.

## Publish Linux packages

Linux x86_64 packages share the `v2.0.0-alpha.3` release with macOS. A Linux build in CI does not publish a release. CI saves packages, checksums, the build log, and the source receipt as a `linux-packages-<commit>` workflow artifact for 14 days.

1. Build the release commit on the oldest distribution you intend to support. Use `npm ci` in `desktop/`, then run `scripts/package-linux.py` from the repository root as described above. Record the build distribution and glibc version from the receipt. Build each advertised CPU separately.
2. Test the exact AppImage on Omarchy and the exact `.deb` on your target Debian or Ubuntu system. Check launch, tray Show/Hide, Settings, Quit, tray loss/recovery, pairing, and reconnect after restart. Keep the release scope limited to the distributions and CPUs tested.
3. Create a GitHub prerelease for the configured version, or add the Linux assets to its existing prerelease. Upload the `.deb`, `.AppImage`, and both `.sha256` files from the same output directory. Preserve the generated filenames, such as `T3-Code-Bar-2.0.0-alpha.3-linux-x86_64.AppImage`. GitHub rewrites spaces in asset names, so release filenames use hyphens. Upload the receipt as `linux-<architecture>-receipt.json` to avoid colliding with macOS metadata. Include the tested distributions, CPU, glibc requirement, tray requirement, and manual update instructions in the release notes.
4. Merge the Linux documentation and installer into `main`. The Pages workflow publishes `scripts/install-linux.sh` at `https://eliophan.github.io/t3code-bar/install-linux.sh`. It preserves the macOS installer at `install.sh`.
5. Download the public assets and check their SHA-256 files. Run the documented installer on a clean account, launch from the application menu and `~/.local/bin/t3-code-bar`, then test saved reconnect. A nonzero exit, missing launcher, unusable tray, or lost saved pairing fails acceptance.
6. After the public download and installer checks pass, update the root README download links and the tested Linux targets in `docs/COMPATIBILITY.md`.

The installer defaults to `2.0.0-alpha.3` and also accepts a version argument, such as `bash install-linux.sh 2.0.0-alpha.3`. Update its default when publishing a new version. It selects the exact Tauri AppImage name for the version and architecture from GitHub's release API. Missing CPU builds and checksum sidecars stop installation. It installs without sudo, refuses to overwrite an existing installation, and does not launch the app automatically. SHA-256 sidecars detect corruption, not independent publisher authenticity.

## Package macOS builds

The Tauri package lane is separate from the Swift release and installer. It does not publish or install anything. Use a Mac with Xcode command line tools, the Rust target, `npm ci` in `desktop/`, and Tauri CLI 2.12.4. Each command uses a fresh temporary Cargo target directory and refuses an existing output path. Select `darwin-aarch64` or `darwin-x86_64` and install its Rust target first.

Build an ad hoc package for manual GitHub distribution. This creates `.app.zip`, `.app.tar.gz` and SHA-256 sidecars. It has no updater endpoint and is not notarized. The current downloadable alpha supports Apple Silicon.

```sh
python3 scripts/package-macos.py adhoc --target darwin-aarch64 --output /tmp/bar-adhoc
codesign --verify --deep --strict --verbose=2 '/tmp/bar-adhoc/T3 Code Bar Tauri.app'
```

For a Developer ID release, install `minisign` and configure these values in the environment. Use the exact 40-character SHA-1 fingerprint from `security find-identity -v -p codesigning` for a valid Developer ID Application certificate. The notary profile must already exist in the keychain. Do not put private keys or passwords in the repository. `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` may be supplied in the environment for a password-protected key. The Tauri public key is the base64 text in the existing `.pub` file, not the private key.

```sh
export BAR_MACOS_SIGNING_IDENTITY='<Developer ID certificate SHA-1>'
export BAR_NOTARY_KEYCHAIN_PROFILE='<existing profile name>'
export BAR_UPDATER_ENDPOINT='https://example.org/updates/manifest.json'
export BAR_UPDATER_PUBLIC_KEY="$(cat /secure/path/updater.key.pub)"
export TAURI_SIGNING_PRIVATE_KEY_PATH='/secure/path/updater.key'
export BAR_UPDATER_ARTIFACT_URL='https://example.org/releases/T3-Code-Bar-2.0.0-alpha.3-darwin-aarch64.app.tar.gz'
python3 scripts/package-macos.py release --target darwin-aarch64 --output /tmp/bar-release
python3 scripts/package-macos.py verify --output /tmp/bar-release
xcrun stapler validate '/tmp/bar-release/T3 Code Bar Tauri.app'
codesign --verify --deep --strict --verbose=2 '/tmp/bar-release/T3 Code Bar Tauri.app'
spctl --assess --type execute --verbose=2 '/tmp/bar-release/T3 Code Bar Tauri.app'
```

The artifact URL must end with the generated filename for the configured version and target. Replace the example URLs before building. The release command checks the requested certificate, enables hardened runtime, submits a ZIP to Apple's notary service using the existing profile, staples the accepted ticket, and checks Gatekeeper. It then creates and signs the stapled `.app.tar.gz`. `manifest.json` contains the updater target, version, HTTPS URL, and signature. `metadata.json` records the version and target with SHA-256 hashes of the archive, signature, and manifest. The `verify` command checks those fields and verifies the archive signature against `BAR_UPDATER_PUBLIC_KEY` using `minisign`. It also binds the authenticated signature comment to the version and filename, and inspects the archived app identifier, bundle version and Mach-O architecture without extracting it. The metadata and manifest are not themselves signed. Protect their distribution separately. Verification does not check the app's notarization or test an update installation. The updater accepts a Tauri `.app.tar.gz`, not the Swift ZIP.

Release packaging requires a valid Developer ID identity and notarization profile. Developer ID signing, Apple notarization, Gatekeeper on a distributed copy, and updater installation and restart remain external acceptance gates. A local ad hoc build or a verified manifest does not certify them.

To test the metadata checks with temporary synthetic keys, install `minisign` and run `python3 scripts/test-package-macos.py`. The test does not use the release key or install an update.

## Manual GitHub release

Publish the ad hoc ZIP, tar archive and both `.sha256` sidecars as a GitHub prerelease matching the configured version. Keep the release marked as a prerelease while the version is alpha. Include this warning in the release notes:

> This app is ad hoc signed and not notarized by Apple. Download the ZIP, unzip and move the app to Applications. If macOS blocks it, try opening the app, then go to **System Settings → Privacy & Security → Open Anyway** and confirm. Managed Macs may restrict this option.

Verify the extracted app with `codesign --verify --deep --strict` and confirm its bundle version and architecture before publishing. This checks integrity, not Apple verification. Do not publish an updater manifest for this manual-release lane.

## Homebrew (macOS)

The public [eliophan/homebrew-tap](https://github.com/eliophan/homebrew-tap) cask installs the Apple Silicon alpha from its versioned GitHub Release archive and verifies SHA-256. It requires macOS 14 or later. Its caveat explains **System Settings → Privacy & Security → Open Anyway** for the current ad hoc build; it does not remove quarantine or disable Gatekeeper.

```sh
brew install --cask eliophan/tap/t3-code-bar
```

If Homebrew requests trust, run `brew trust --cask eliophan/tap/t3-code-bar`, then retry.

For future Developer ID releases, generate the cask from signed updater packages:

1. Build and verify a release package using the commands above. Each architecture must use a GitHub asset URL of the form `https://github.com/eliophan/t3code-bar/releases/download/vVERSION/T3-Code-Bar-VERSION-darwin-aarch64.app.tar.gz` (or `darwin-x86_64`).
2. Generate the cask from the verified package directories:

```sh
python3 scripts/homebrew-cask.py --package /tmp/bar-release --output /tmp/bar-cask/t3-code-bar.rb
```

`BAR_UPDATER_PUBLIC_KEY` must be set. Repeat `--package` for the Intel package to support both architectures. The generator verifies package signatures, hashes, bundle identity, architecture, version and GitHub asset URLs. A single-architecture cask refuses unsupported CPUs. It does not establish native installation acceptance.

3. Review the output and place it at `Casks/t3-code-bar.rb` in a separate `eliophan/homebrew-tap` repository. Publish the exact verified assets and cask only when authorized. The generator performs no uploads, repository creation or installation.
4. After publication, test installation and launch on a clean Mac, including pairing, saved reconnect and upgrading an existing installation:

```sh
brew install --cask eliophan/tap/t3-code-bar
```

A non-zero exit from either command is a failure. A successful generation is reported as `Wrote ...; nothing installed or published.` A successful Homebrew install still needs native app checks. Keep macOS quarantine protections enabled; do not add an installer that removes quarantine or disables Gatekeeper. Uninstalling the cask leaves user settings and credentials intact.

## Command-line installer (macOS)

`scripts/install-macos.sh` installs the current Tauri version from GitHub Releases into `~/Applications/T3 Code Bar Tauri.app`. It detects Apple Silicon (including Rosetta) or Intel, verifies the archive SHA-256, bundle identity/version and code-signature integrity, then opens the app. The current alpha is ad hoc signed; it does not authenticate the developer with Apple. It refuses to replace an existing install and does not change Swift settings or credentials. No Node.js, Rust, Python or Homebrew is needed on the user's Mac.

Publish the release archive and its generated `.sha256` sidecar together. The sidecar detects corruption, but is not independent proof of publisher identity. Missing assets or failed verification stop installation. The installer sets download quarantine because curl does not do so, and never removes quarantine or disables Gatekeeper. If macOS blocks the app, try opening it, then use **System Settings → Privacy & Security → Open Anyway** and confirm.

```sh
curl -fsSL https://eliophan.github.io/t3code-bar/install.sh | bash
```

This command becomes usable after the version in `tauri.conf.json` has matching GitHub Release assets. Changing that version before publishing its packages makes the installer stop at the missing release.
