#!/usr/bin/env python3
"""Build local Linux packages and record exactly which sources produced them."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def run(args, **kwargs):
    return subprocess.check_output(args, cwd=ROOT, text=True, **kwargs).strip()


def sha(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sources():
    paths = subprocess.check_output(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", "desktop", "scripts/package-linux.py"], cwd=ROOT
    ).split(b"\0")
    files = {}
    for raw in sorted(set(paths)):
        if raw:
            relative = os.fsdecode(raw)
            path = ROOT / relative
            files[relative] = {"sha256": sha(path), "executable": bool(path.stat().st_mode & 0o111)} if path.is_file() else None
    return files


def packaged_binary_sha(path):
    with tempfile.TemporaryDirectory(prefix="t3-linux-inspect-") as scratch:
        directory = Path(scratch)
        if path.suffix == ".deb":
            subprocess.run(["ar", "x", str(path)], cwd=directory, check=True, stdout=subprocess.DEVNULL)
            archives = list(directory.glob("data.tar.*"))
            if len(archives) != 1:
                raise RuntimeError("Deb has no unique data archive")
            subprocess.run(["tar", "xf", str(archives[0])], cwd=directory, check=True)
            controls = list(directory.glob("control.tar.*"))
            if len(controls) != 1:
                raise RuntimeError("Deb has no unique control archive")
            names = subprocess.check_output(["tar", "tf", str(controls[0])], text=True).splitlines()
            control_name = next(name for name in names if name.removeprefix("./") == "control")
            control = subprocess.check_output(["tar", "xOf", str(controls[0]), control_name], text=True)
            depends = next((line for line in control.splitlines() if line.startswith("Depends:")), "")
            if "libgtk-layer-shell0" not in depends:
                raise RuntimeError("Deb lacks the gtk-layer-shell runtime dependency")
        else:
            subprocess.run([str(path), "--appimage-extract"], cwd=directory, check=True, stdout=subprocess.DEVNULL)
            directory /= "squashfs-root"
            libraries = list(directory.rglob("libgtk-layer-shell.so.0*"))
            if not libraries or not any(p.is_file() and p.resolve().is_relative_to(directory)
                                        and p.read_bytes().startswith(b"\x7fELF") for p in libraries):
                raise RuntimeError("AppImage lacks its gtk-layer-shell runtime library")
        binary = directory / "usr/bin/t3-code-bar-desktop"
        if not binary.is_file() or not os.access(binary, os.X_OK):
            raise RuntimeError("Package lacks executable app binary")
        return sha(binary)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--target-dir", type=Path, help="Optional reusable Cargo cache; bundle output is cleared before building")
    args = parser.parse_args()
    if platform.system() != "Linux":
        parser.error("Build on Linux")
    output = args.output.resolve()
    if output.exists():
        parser.error("Output must not exist")
    output.mkdir(parents=True)
    target = args.target_dir.resolve() if args.target_dir else Path(tempfile.mkdtemp(prefix="t3-linux-target-"))
    bundle = target / "release/bundle"
    for kind in ("deb", "appimage"):
        if (bundle / kind).exists():
            shutil.rmtree(bundle / kind)
    before = sources()
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(target)
    command = ["npm", "--prefix", "desktop", "run", "tauri", "--", "build", "--config", "src-tauri/tauri.linux.conf.json", "--bundles", "deb,appimage"]
    with (output / "build.log").open("w") as log:
        subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
    if before != sources():
        raise RuntimeError("Sources changed during build; packages cannot be accepted")
    packages = [*bundle.glob("deb/*.deb"), *bundle.glob("appimage/*.AppImage")]
    if len(packages) != 2 or {p.suffix for p in packages} != {".deb", ".AppImage"}:
        raise RuntimeError("Expected exactly one deb and one AppImage from this build")
    version = json.loads((ROOT / "desktop/src-tauri/tauri.conf.json").read_text())["version"]
    machine = platform.machine()
    if machine not in {"x86_64", "aarch64"}:
        raise RuntimeError(f"Unsupported release architecture: {machine}")
    artifacts = {}
    for path in packages:
        dest = output / f"T3-Code-Bar-{version}-linux-{machine}{path.suffix}"
        shutil.copy2(path, dest)
        artifacts[dest.name] = {"sha256": sha(dest), "bytes": dest.stat().st_size,
                                "executable_sha256": packaged_binary_sha(dest)}
        (output / (dest.name + ".sha256")).write_text(f"{sha(dest)}  {dest.name}\n")
    binary = target / "release/t3-code-bar-desktop"
    receipt = {
        "revision": run(["git", "rev-parse", "HEAD"]),
        "source_files": before,
        "source_sha256": hashlib.sha256(json.dumps(before, sort_keys=True).encode()).hexdigest(),
        "tracked_diff": run(["git", "diff", "HEAD", "--", "desktop", "scripts/package-linux.py"]),
        "machine": platform.machine(), "platform": platform.platform(),
        "libc": platform.libc_ver(), "command": command,
        "runtime_dependencies": {"gtk-layer-shell": run(["pkg-config", "--modversion", "gtk-layer-shell-0"])},
        "binary": str(binary), "binary_sha256": sha(binary),
        "artifacts": artifacts,
        "scope": "Local manual distribution; no signing, publishing, installation, GUI or cross-distro certification",
    }
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(f"Built deb and AppImage; receipt: {output / 'receipt.json'}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"Packaging failed: {error}", file=sys.stderr)
        sys.exit(1)
