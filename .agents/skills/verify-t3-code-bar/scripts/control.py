#!/usr/bin/env python3
"""Own and drive one synthetic T3 Code Bar native Linux instance."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time
import uuid
import importlib.util

ROOT = Path(__file__).resolve().parents[4]
BASE = ROOT / ".audit/verify-t3-code-bar"
ACTIVE = BASE / "active.json"
DRIVER = shutil.which("cua-driver") or str(Path.home() / ".local/bin/cua-driver")


def product_sources():
    spec = importlib.util.spec_from_file_location("linux_package", ROOT / "scripts/package-linux.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.sources()


def dump(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")


def command(args, **kwargs):
    result = subprocess.run([str(x) for x in args], text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, timeout=30, **kwargs)
    if result.returncode:
        raise RuntimeError(f"Command exit {result.returncode}: {args[0]}\n{result.stderr or result.stdout}")
    return result.stdout


def identity(pid):
    try:
        fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
        if fields[0] == "Z":
            return None
        return {"pid": pid, "start": fields[19]}
    except FileNotFoundError:
        return None


def alive(owner):
    return identity(owner["pid"]) == owner


def stop(owner):
    if not alive(owner):
        return
    pid = owner["pid"]
    if os.getpgid(pid) != pid:
        raise RuntimeError("Owned process no longer owns its process group")
    os.killpg(pid, signal.SIGTERM)
    deadline = time.monotonic() + 5
    while alive(owner) and time.monotonic() < deadline:
        time.sleep(0.1)
    if alive(owner):
        os.killpg(pid, signal.SIGKILL)
    if alive(owner):
        time.sleep(0.1)
    if alive(owner):
        raise RuntimeError(f"Owned PID {pid} did not stop")


def load():
    if not ACTIVE.exists():
        raise RuntimeError("No owned run. Launch first.")
    return json.loads(ACTIVE.read_text())


def save(state):
    dump(ACTIVE, state)
    dump(Path(state["evidence"]) / "run.json", state)


def spawn(args, env, log):
    with log.open("ab") as stream:
        proc = subprocess.Popen([str(x) for x in args], env=env, stdout=stream,
                                stderr=subprocess.STDOUT, start_new_session=True)
    owner = identity(proc.pid)
    if owner is None:
        raise RuntimeError(f"Process exited during launch; see {log}")
    return owner


def event(state, name, payload):
    evidence = Path(state["evidence"])
    serial = len(list(evidence.glob("step-*.json"))) + 1
    target = evidence / f"step-{serial:03d}-{name}.json"
    # Preserve returned window or full desktop captures without cropping.
    def strip_image(value, prefix):
        if isinstance(value, dict):
            value = dict(value)
            image = value.pop("screenshot_png_b64", None)
            if image:
                image_path = target.with_name(target.stem + prefix + ".png")
                data = base64.b64decode(image, validate=True)
                if not data.startswith(b"\x89PNG\r\n\x1a\n"):
                    raise RuntimeError("Cua screenshot is not a PNG")
                image_path.write_bytes(data)
                value["saved_screenshot"] = str(image_path)
            return {key: strip_image(child, prefix + "-" + key) for key, child in value.items()}
        if isinstance(value, list):
            return [strip_image(child, prefix + f"-{index}") for index, child in enumerate(value)]
        return value
    dump(target, strip_image(payload, ""))
    return target


def cua(state, tool, args):
    payload = {"session": state["session"], **args}
    result = subprocess.run([DRIVER, "--socket", state["socket"], "call", tool, json.dumps(payload)],
                            text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30)
    raw = result.stdout or result.stderr
    try:
        response = json.loads(raw)
    except json.JSONDecodeError:
        event(state, "cua-error", {"tool": tool, "request": payload, "exit": result.returncode,
                                    "stdout": result.stdout, "stderr": result.stderr})
        raise RuntimeError(f"Cua {tool} did not return JSON; failure evidence retained")
    if result.returncode:
        event(state, "cua-error", {"tool": tool, "request": payload, "exit": result.returncode,
                                    "response": response})
        raise RuntimeError(f"Cua {tool} exited {result.returncode}; failure evidence retained: {response}")
    return response


def layer_surfaces(state):
    layers = json.loads(command(["hyprctl", "-j", "layers"]))
    return [surface for output in layers.values() for surfaces in output["levels"].values()
            for surface in surfaces if surface.get("pid") == state["app"]["pid"]]


def window(state):
    response = cua(state, "list_windows", {})
    own = [item for item in response.get("windows", []) if item.get("pid") == state["app"]["pid"]]
    if not own:
        layers = layer_surfaces(state)
        if len(layers) == 1:
            return {"pid": state["app"]["pid"], "layer_surface": layers[0],
                    "is_on_screen": layers[0].get("alpha", 1) > 0}
    if len(own) != 1:
        raise RuntimeError(f"Expected one owned window, found {len(own)}")
    return own[0]


def snapshot(state, label="snapshot", picture=True):
    target = window(state)
    layer = "layer_surface" in target
    args = {"pid": target["pid"], "max_elements": 160, "timeout_ms": 5000,
            "include_screenshot": picture and not layer}
    if "window_id" in target:
        args["window_id"] = target["window_id"]
    if layer:
        response = json.loads(command([sys.executable, Path(__file__).with_name("layer_accessibility.py"),
                                       "--pid", target["pid"]]))
    else:
        response = cua(state, "get_window_state", args)
    event(state, label, response)
    if response.get("degraded") or not response.get("elements_complete"):
        raise RuntimeError(f"Native accessibility is incomplete: {response.get('degraded_reason', response)}")
    if picture and layer:
        event(state, label + "-layer-geometry", target)
        event(state, label + "-desktop", cua(state, "get_desktop_state", {"max_image_dimension": 1568}))
    elif picture and not response.get("screenshot_frame_valid"):
        raise RuntimeError(f"No attested window screenshot: {response.get('screenshot_error')}")
    if (not layer and not response.get("snapshot_id")) or not response.get("tree_markdown"):
        raise RuntimeError("Native snapshot has no usable controls")
    return response


def find(snapshot, label):
    found = []
    for line in snapshot["tree_markdown"].splitlines():
        match = re.search(r'\[(\d+)\].*?("(?:\\.|[^"\\])*")\s+\[actions=\[([^]]*)\]\]', line)
        if match and json.loads(match[2]) == label and match[3]:
            found.append((f"{snapshot['snapshot_id']}:{match[1]}", match[3]))
    if len(found) != 1:
        raise RuntimeError(f"Expected one actionable control {label!r}, found {len(found)}")
    return found[0]


def click(state, label):
    if "layer_surface" in window(state):
        raise RuntimeError("Cua has no layer-window handles; use an observed desktop pointer action and verify with native AT-SPI")
    before = snapshot(state, "before-click", picture=False)
    token, _ = find(before, label)
    target = window(state)
    args = {"pid": target["pid"],
            "steps": [{"tool": "click", "args": {"element_token": token}}]}
    if "window_id" in target:
        args["window_id"] = target["window_id"]
    if label != "Quit T3 Code Bar" and "layer_surface" not in target:
        args["observe"] = {"include_screenshot": False, "max_elements": 160}
    response = cua(state, "run_steps", args)
    event(state, "click", {"label": label, "response": response})
    if response.get("ok") is not True or response.get("executed") != 1:
        raise RuntimeError(f"Native click did not execute: {response}")
    if label == "Quit T3 Code Bar":
        deadline = time.monotonic() + 5
        while alive(state["app"]) and time.monotonic() < deadline:
            time.sleep(0.1)
        if alive(state["app"]):
            raise RuntimeError("Quit did not terminate the owned process")
    return response


def tray(state):
    bus = ["busctl", "--address=" + state["bus_address"]] if state.get("bus_address") else ["busctl", "--user"]
    exists = command([*bus, "call", "org.freedesktop.DBus", "/org/freedesktop/DBus",
                      "org.freedesktop.DBus", "NameHasOwner", "s", "org.kde.StatusNotifierWatcher"]).strip()
    if exists == "b false":
        return {"host_registered": False, "own_items": [], "watcher_present": False}
    if exists != "b true":
        raise RuntimeError("Unexpected watcher ownership response")
    prefix = [*bus, "get-property", "org.kde.StatusNotifierWatcher",
              "/StatusNotifierWatcher", "org.kde.StatusNotifierWatcher"]
    host = command([*prefix, "IsStatusNotifierHostRegistered"]).strip() == "b true"
    entries = shlex.split(command([*prefix, "RegisteredStatusNotifierItems"]))[2:]
    own = []
    for entry in entries:
        service = entry.split("/")[0]
        value = command([*bus, "call", "org.freedesktop.DBus",
                         "/org/freedesktop/DBus", "org.freedesktop.DBus",
                         "GetConnectionUnixProcessID", "s", service]).split()
        if value == ["u", str(state["app"]["pid"])]:
            own.append(entry)
    return {"host_registered": host, "own_items": own}


def doctor(state):
    if not alive(state["app"]) or not alive(state["driver"]):
        raise RuntimeError("Owned app or Cua daemon is not alive")
    binary = Path(state["binary"])
    if hashlib.sha256(binary.read_bytes()).hexdigest() != state["binary_sha256"]:
        raise RuntimeError("Owned executable changed")
    environment = dict(item.split(b"=", 1) for item in Path(f"/proc/{state['app']['pid']}/environ").read_bytes().split(b"\0") if b"=" in item)
    for key, expected in ((b"T3_BAR_FIXTURE_STATE", state["state"]),
                          (b"T3_BAR_FIXTURE_SETTINGS", state["settings"])):
        if environment.get(key) != expected.encode():
            raise RuntimeError("Owned process is not using the isolated fixture paths")
    if b"T3_BAR_FIXTURE_KEYRING_NAMESPACE" in environment or b"T3_BAR_FIXTURE_NOTIFICATIONS" in environment:
        raise RuntimeError("Unexpected credential/notification opt-in in the owned process")
    settings = json.loads(Path(state["settings"]).read_text())
    native = snapshot(state, "doctor", picture=False)
    text = native["tree_markdown"]
    if not any(label in text for label in ("Connect T3 Code", "Settings", "Review a proposed action", "Retry")):
        raise RuntimeError("Window has not reached a usable UI")
    registered = tray(state)
    result = {"pid": state["app"]["pid"], "revision": state["revision"],
              "binary_sha256": state["binary_sha256"], "version": state["version"],
              "tray": registered, "fixture_state": state["state"], "settings": settings}
    event(state, "doctor-result", result)
    if registered["host_registered"] and registered["own_items"]:
        if "Retry" in text:
            raise RuntimeError("Registered tray did not recover working UI")
    elif "Retry" not in text or "Connect T3 Code" in text or "Settings" in text:
        raise RuntimeError("Missing tray must expose only Retry/Quit diagnostic")
    return result


def app_env(state):
    env = {key: value for key, value in os.environ.items() if not key.startswith("T3_BAR_")}
    env.pop("ELECTRON_RUN_AS_NODE", None)
    env.update(T3_BAR_FIXTURE_STATE=state["state"], T3_BAR_FIXTURE_SETTINGS=state["settings"],
               T3_BAR_FROZEN_NOW="2026-11-06T09:08:00Z", T3_BAR_TIMEZONE="UTC",
               GTK_MODULES="gail:atk-bridge", GTK_A11Y="always", NO_AT_BRIDGE="0")
    if state.get("bus_address"):
        env["DBUS_SESSION_BUS_ADDRESS"] = state["bus_address"]
        env["AT_SPI_BUS_ADDRESS"] = state["at_spi_bus_address"]
    return env


def start_app(state):
    env = app_env(state)
    if state.get("appimage"):
        env["APPIMAGE_EXTRACT_AND_RUN"] = "1"
        owner = spawn([state["appimage"]], env, Path(state["evidence"]) / "app.log")
        state["launcher"] = owner
        save(state)
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            found = []
            for entry in Path("/proc").iterdir():
                if not entry.name.isdigit():
                    continue
                try:
                    pid = int(entry.name)
                    if os.getpgid(pid) == owner["pid"] and Path(os.readlink(entry / "exe")).name == "t3-code-bar-desktop":
                        if hashlib.sha256((entry / "exe").read_bytes()).hexdigest() != state["binary_sha256"]:
                            raise RuntimeError("AppImage launched a binary that differs from its receipt")
                        found.append(identity(pid))
                except (FileNotFoundError, PermissionError, ProcessLookupError):
                    continue
            if len(found) == 1:
                state["app"] = found[0]
                break
            time.sleep(.1)
        else:
            raise RuntimeError("AppImage did not launch its verified executable")
    else:
        state["app"] = spawn([state["binary"]], env, Path(state["evidence"]) / "app.log")
    save(state)
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if not alive(state["app"]):
            raise RuntimeError("App exited during startup; inspect app.log")
        if Path(state["settings"]).exists():
            try:
                native = snapshot(state, "startup", picture=False)
                if any(label in native["tree_markdown"] for label in ("Connect T3 Code", "Retry")):
                    return
            except (RuntimeError, json.JSONDecodeError):
                pass
        time.sleep(0.3)
    raise RuntimeError("App did not reach usable onboarding within 15 seconds")


def build(target):
    BASE.mkdir(parents=True, exist_ok=True)
    target = target.resolve()
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(target)
    env["PATH"] = str(Path.home() / ".cargo/bin") + os.pathsep + env["PATH"]
    with (BASE / "build.log").open("w") as log:
        for args in (["npm", "--prefix", "desktop", "ci"],
                     ["npm", "--prefix", "desktop", "run", "tauri", "--", "build", "--no-bundle", "--config", "src-tauri/tauri.linux.conf.json"]):
            result = subprocess.run(["mise", "exec", "node@26.13.0", "--", *args], cwd=ROOT,
                                    env=env, stdout=log, stderr=subprocess.STDOUT)
            if result.returncode:
                raise RuntimeError(f"Build failed: inspect {BASE / 'build.log'}")
    binary = target / "release/t3-code-bar-desktop"
    if not binary.is_file():
        raise RuntimeError("Build reported success without a binary")
    receipt = {"binary": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
               "revision": command(["git", "rev-parse", "HEAD"], cwd=ROOT).strip(),
               "source_diff": command(["git", "diff", "--", "desktop"], cwd=ROOT),
               "source_files": product_sources()}
    dump(BASE / "build.json", receipt)
    print(f"BUILD READY: {binary}; receipt={BASE / 'build.json'}")


def launch(package_receipt=None, package_binary=None, bus_address=None, appimage=None):
    if ACTIVE.exists():
        raise RuntimeError("An owned run exists; cleanup it first")
    for proc in Path("/proc").iterdir():
        if proc.name.isdigit():
            try:
                if Path(os.readlink(proc / "exe")).name == "t3-code-bar-desktop":
                    raise RuntimeError(f"Another Bar instance exists (PID {proc.name}); do not double-drive the single-instance app")
            except (FileNotFoundError, PermissionError, ProcessLookupError):
                pass
    if package_receipt:
        package = json.loads(package_receipt.read_text())
        if package["source_files"] != product_sources():
            raise RuntimeError("Product source changed since packaging")
        if not package_binary:
            raise RuntimeError("--package-binary must name the executable extracted from the verified package")
        digest = hashlib.sha256(package_binary.read_bytes()).hexdigest()
        matched = []
        for name, artifact in package["artifacts"].items():
            archive = package_receipt.parent / name
            if hashlib.sha256(archive.read_bytes()).hexdigest() != artifact["sha256"]:
                raise RuntimeError("Package archive no longer matches receipt")
            if artifact["executable_sha256"] == digest:
                matched.append(name)
        if len(matched) != 1:
            raise RuntimeError("Executable does not uniquely match an inspected package")
        receipt = {"binary": str(package_binary.resolve()), "sha256": digest, "package_name": matched[0],
                   "revision": package["revision"], "source_files": package["source_files"],
                   "source_diff": command(["git", "diff", "--", "desktop"], cwd=ROOT),
                   "package_receipt": str(package_receipt.resolve()), "package": package}
    else:
        receipt = json.loads((BASE / "build.json").read_text())
    source_binary = Path(receipt["binary"])
    if hashlib.sha256(source_binary.read_bytes()).hexdigest() != receipt["sha256"]:
        raise RuntimeError("Built binary no longer matches build receipt; rebuild")
    if command(["git", "rev-parse", "HEAD"], cwd=ROOT).strip() != receipt["revision"] or command(["git", "diff", "--", "desktop"], cwd=ROOT) != receipt["source_diff"]:
        raise RuntimeError("Product source changed since build; rebuild")
    if receipt.get("source_files") != product_sources():
        raise RuntimeError("Product files changed since build; rebuild")
    tag = uuid.uuid4().hex[:12]
    evidence = BASE / "runs" / tag
    evidence.mkdir(parents=True)
    runtime = Path(tempfile.mkdtemp(prefix="t3bar-gui-"))
    state_dir = runtime / "state"
    state_dir.mkdir()
    binary = source_binary if package_receipt else runtime / "t3-code-bar-desktop"
    if not package_receipt:
        shutil.copy2(source_binary, binary)
    with sqlite3.connect(state_dir / "statev2.sqlite") as db:
        db.executescript((ROOT / "desktop/tests/fixtures/threads.sql").read_text())
    for fixture in (ROOT / "desktop/tests/fixtures/usage/state").iterdir():
        shutil.copy2(fixture, state_dir / fixture.name)
    seed_hashes = {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in state_dir.iterdir()}
    state = {"session": "bar-" + tag, "evidence": str(evidence), "runtime": str(runtime),
             "state": str(state_dir), "settings": str(state_dir / "tauri-bar-settings.json"),
             "binary": str(binary), "binary_sha256": receipt["sha256"],
             "revision": receipt["revision"], "version": json.loads((ROOT / "desktop/package.json").read_text())["version"],
             "socket": str(runtime / "cua.sock"), "seed_hashes": seed_hashes}
    if bus_address:
        state["bus_address"] = bus_address
        state["at_spi_bus_address"] = shlex.split(command(["busctl", "--user", "call", "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus", "GetAddress"]))[1]
    if appimage:
        if not package_receipt or appimage.resolve() != (package_receipt.parent / receipt["package_name"]).resolve() or appimage.suffix != ".AppImage":
            raise RuntimeError("AppImage launcher must be the verified package containing this binary")
        state["appimage"] = str(appimage.resolve())
    dump(evidence / "build.json", receipt)
    env = os.environ.copy()
    env["CUA_DRIVER_RS_ENABLE_WAYLAND"] = "1"
    state["driver"] = spawn([DRIVER, "serve", "--socket", state["socket"], "--pid-file",
                              runtime / "cua.pid", "--permission-mode", "standard"], env, evidence / "driver.log")
    save(state)
    try:
        deadline = time.monotonic() + 5
        while not Path(state["socket"]).exists() and time.monotonic() < deadline:
            time.sleep(0.1)
        if not Path(state["socket"]).exists():
            raise RuntimeError("Owned Cua daemon did not become ready")
        start_app(state)
        doctor(state)
    except Exception:
        cleanup(state)
        raise
    print(f"READY: owned native app PID={state['app']['pid']}; evidence={evidence}")


def cleanup(state):
    # Retain proofs even after failed iterations; never stop a shared daemon.
    session_error = None
    if "driver" in state and alive(state["driver"]):
        try:
            response = cua(state, "end_session", {})
            event(state, "end-session", response)
        except (RuntimeError, subprocess.SubprocessError) as error:
            session_error = str(error)
    if "launcher" in state:
        stop(state["launcher"])
        deadline = time.monotonic() + 3
        while "app" in state and alive(state["app"]) and time.monotonic() < deadline:
            time.sleep(0.1)
        if "app" in state and alive(state["app"]):
            raise RuntimeError("AppImage child outlived owned launcher; retain runtime for investigation")
    for key in (("driver",) if "launcher" in state else ("app", "driver")):
        if key in state:
            stop(state[key])
    remaining = [key for key in ("app", "driver") if key in state and alive(state[key])]
    if remaining:
        raise RuntimeError(f"Owned processes remain: {remaining}")
    if Path(state["settings"]).exists():
        shutil.copy2(state["settings"], Path(state["evidence"]) / "settings-at-cleanup.json")
    changed_seeds = []
    for name, expected_hash in state["seed_hashes"].items():
        path = Path(state["state"]) / name
        if not path.exists() or hashlib.sha256(path.read_bytes()).hexdigest() != expected_hash:
            changed_seeds.append(name)
            if path.is_file():
                retained = Path(state["evidence"]) / "changed-seeds" / name
                retained.parent.mkdir(exist_ok=True)
                shutil.copy2(path, retained)
    shutil.rmtree(state["runtime"])
    evidence = Path(state["evidence"])
    files = [str(path.relative_to(evidence)) for path in evidence.iterdir() if path.is_file()]
    dump(evidence / "cleanup.json", {"owned_processes_stopped": True, "runtime_removed": True,
                                     "evidence_files_retained": files, "session_end_error": session_error,
                                     "changed_read_only_seeds": changed_seeds})
    ACTIVE.unlink(missing_ok=True)
    if not list(evidence.glob("step-*.json")):
        raise RuntimeError("Cleanup retained no observation evidence")
    if session_error:
        raise RuntimeError(f"Owned processes stopped and evidence retained, but Cua session end failed: {session_error}")
    if changed_seeds:
        raise RuntimeError(f"App modified seeded read-only data; evidence retained: {changed_seeds}")
    print(f"CLEANUP OK: evidence retained at {evidence}")


def settings_journey(state):
    doctor(state)
    snapshot(state, "onboarding")
    click(state, "Settings")
    before = json.loads(Path(state["settings"]).read_text())
    event(state, "settings-before", before)
    snapshot(state, "settings-before")
    click(state, "Hover previews")
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        after = json.loads(Path(state["settings"]).read_text())
        if after["showsPreviews"] is not before["showsPreviews"]:
            break
        time.sleep(0.1)
    else:
        raise RuntimeError("UI toggle did not change the real settings file")
    native = snapshot(state, "settings-changed")
    _, actions = find(native, "Hover previews")
    if actions != ("uncheck" if after["showsPreviews"] else "check"):
        raise RuntimeError("Visible switch and stored setting disagree")
    event(state, "settings-after", after)
    click(state, "Quit T3 Code Bar")
    start_app(state)
    doctor(state)
    click(state, "Settings")
    restored = json.loads(Path(state["settings"]).read_text())
    native = snapshot(state, "settings-after-relaunch")
    _, actions = find(native, "Hover previews")
    if restored != after or actions != ("uncheck" if after["showsPreviews"] else "check"):
        raise RuntimeError("Preference did not persist through real Quit and relaunch")
    result = {"feature": "settings-preview-persistence", "status": "PASS",
              "before": before, "after": after, "after_relaunch": restored,
              "scope": "Native Tauri binary on Omarchy; synthetic data; AT-SPI user actions; no package or tray pointer certification"}
    dump(Path(state["evidence"]) / "settings-result.json", result)
    print(f"PASS settings-preview-persistence: {state['evidence']}")


def navigation_journey(state):
    doctor(state)
    initial = snapshot(state, "navigation-onboarding")
    if "Connect T3 Code" not in initial["tree_markdown"]:
        raise RuntimeError("Navigation journey must start at onboarding")
    before = json.loads(Path(state["settings"]).read_text())
    click(state, "Settings")
    settings = snapshot(state, "navigation-settings")
    for label in ("Hover previews", "Notifications", "Quit T3 Code Bar"):
        find(settings, label)
    click(state, "Back to threads")
    returned = snapshot(state, "navigation-returned")
    if "Connect T3 Code" not in returned["tree_markdown"]:
        raise RuntimeError("Back to threads did not return to unpaired onboarding")
    click(state, "Settings")
    snapshot(state, "navigation-before-quit")
    click(state, "Quit T3 Code Bar")
    after = json.loads(Path(state["settings"]).read_text())
    if before != after:
        raise RuntimeError("Navigation unexpectedly modified preferences")
    result = {"feature": "settings-onboarding-navigation-quit", "status": "PASS",
              "quit_pid_terminated": not alive(state["app"]),
              "preferences_unchanged": before == after,
              "scope": "Native Tauri binary on Omarchy; synthetic unpaired state; AT-SPI navigation and Quit; no switch/tray pointer/package certification"}
    dump(Path(state["evidence"]) / "navigation-result.json", result)
    print(f"PASS settings-onboarding-navigation-quit: {state['evidence']}")


def primary_click_journey(state, icon_x, icon_y, outside_x, outside_y, expand_x=None, expand_y=None):
    """Use coordinates observed in this run's fresh desktop capture, never constants."""
    def pointer(x, y, label):
        capture = cua(state, "get_desktop_state", {"max_image_dimension": 1568})
        event(state, label + "-before", capture)
        result = cua(state, "run_steps", {"steps": [{"tool": "click", "args": {
            "target": {"kind": "desktop", "display_id": "primary"},
            "capture_id": capture["capture_id"], "x": x, "y": y}}]})
        event(state, label + "-action", result)
        if result.get("ok") is not True or result.get("executed") != 1:
            raise RuntimeError("Primary-click journey input did not execute")

    def visible():
        result = cua(state, "list_windows", {})
        return (any(w.get("pid") == state["app"]["pid"] and w.get("is_on_screen")
                    for w in result.get("windows", []))
                or any(surface.get("alpha", 1) > 0 for surface in layer_surfaces(state)))

    if (expand_x is None) != (expand_y is None):
        raise ValueError("Supply both tray expansion coordinates")
    for iteration in range(3):
        if expand_x is not None:
            pointer(expand_x, expand_y, f"primary-{iteration}-expand")
            time.sleep(.2)
        pointer(icon_x, icon_y, f"primary-{iteration}-icon")
        time.sleep(.4)
        if not visible():
            raise RuntimeError("Tray primary click did not open the panel")
        snapshot(state, f"primary-{iteration}-opened")
        capture = cua(state, "get_desktop_state", {"max_image_dimension": 1568})
        event(state, f"primary-{iteration}-hover", cua(state, "move_cursor", {
            "scope": "desktop", "x": outside_x, "y": outside_y}))
        time.sleep(.4)
        if not visible():
            raise RuntimeError("Pointer movement outside dismissed the panel")
        pointer(outside_x, outside_y, f"primary-{iteration}-outside")
        time.sleep(.4)
        if visible():
            raise RuntimeError("Actual outside click did not dismiss the panel")
        if not alive(state["app"]):
            raise RuntimeError("App exited instead of hiding after the outside click")
    dump(Path(state["evidence"]) / "primary-click-result.json", {
        "pass": True, "cycles": 3,
        "scope": "Native Omarchy tray click opens, outside hover preserves panel, outside click dismisses"})
    print(f"PASS tray-primary-click: {state['evidence']}")



def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    build_parser = sub.add_parser("build")
    build_parser.add_argument("--target-dir", type=Path, default=BASE / "target")
    launch_parser = sub.add_parser("launch")
    launch_parser.add_argument("--package-receipt", type=Path)
    launch_parser.add_argument("--package-binary", type=Path)
    launch_parser.add_argument("--bus-address", help="Owned private test bus; Cua stays on the desktop accessibility bus")
    launch_parser.add_argument("--appimage", type=Path, help="Launch the verified original AppImage with extract-and-run")
    for name in ("doctor", "snapshot", "settings", "navigation", "cleanup"):
        sub.add_parser(name)
    sub.add_parser("click").add_argument("label")
    primary = sub.add_parser("primary")
    for coordinate in ("icon-x", "icon-y", "outside-x", "outside-y"):
        primary.add_argument("--" + coordinate, required=True, type=float)
    primary.add_argument("--expand-x", type=float)
    primary.add_argument("--expand-y", type=float)
    args = parser.parse_args()
    if args.action == "build":
        build(args.target_dir)
    elif args.action == "launch":
        launch(args.package_receipt, args.package_binary, args.bus_address, args.appimage)
    else:
        state = load()
        if args.action == "doctor":
            print(json.dumps(doctor(state), indent=2))
        elif args.action == "snapshot":
            print(snapshot(state)["tree_markdown"])
        elif args.action == "click":
            click(state, args.label)
        elif args.action == "settings":
            settings_journey(state)
        elif args.action == "navigation":
            navigation_journey(state)
        elif args.action == "primary":
            primary_click_journey(state, args.icon_x, args.icon_y, args.outside_x, args.outside_y, args.expand_x, args.expand_y)
        else:
            cleanup(state)


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        print("If a run exists, execute control.py cleanup before retrying; evidence is retained.", file=sys.stderr)
        sys.exit(1)
