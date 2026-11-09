#!/usr/bin/env python3
"""Compile source-backed observations, then exhaust the finite TLC model."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

SKILL = Path(__file__).resolve().parents[1]
ROOT = SKILL.parents[2]
SOURCE = Path("desktop/src-tauri/src/platform_linux.rs")
TLC_VERSION = "1.7.4"
TLC_SHA = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
TLC_TOOL = (
    "http:tla2tools[url=https://github.com/tlaplus/tlaplus/releases/download/"
    f"v{TLC_VERSION}/tla2tools.jar,checksum=sha256:{TLC_SHA}]@{TLC_VERSION}"
)
JAVA_TOOL = "java@temurin-25.0.5+8.0.LTS"
RULES = ("TrayRequiredForContent", "OnlyDiagnosticWithoutTray", "HideRequiresObservedTray",
         "SettingsRequiresObservedTray", "ResizePreservesUnavailableSurface")


def run(args, *, cwd=None):
    return subprocess.run(
        [str(arg) for arg in args], cwd=cwd, text=True,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60,
    )


def required(args):
    result = run(args)
    if result.returncode:
        raise RuntimeError(f"Command exited {result.returncode}: {args}\n{result.stdout}")
    return result.stdout.strip()


def mise_path(tool):
    return Path(required(["mise", "where", tool]))


def tools():
    jar = Path(os.environ["TLA2TOOLS"]) if "TLA2TOOLS" in os.environ else mise_path(TLC_TOOL) / "tla2tools.jar"
    if hashlib.sha256(jar.read_bytes()).hexdigest() != TLC_SHA:
        raise RuntimeError("TLC jar differs from pinned 1.7.4 SHA-256")
    java = os.environ.get("JAVA") or str(mise_path(JAVA_TOOL) / "bin/java")
    rustup = shutil.which("rustup") or str(Path.home() / ".cargo/bin/rustup")
    import tomllib
    with (ROOT / "desktop/rust-toolchain.toml").open("rb") as stream:
        rust_version = tomllib.load(stream)["toolchain"]["channel"]
    compiler = [rustup, "run", rust_version, "rustc"]
    return jar, java, compiler


def block(source, marker):
    """Select balanced Rust braces, ignoring strings and line comments.

    This narrow extractor is not a Rust parser. Reject missing/duplicate
    markers; unsupported source edits require a correspondence review.
    """
    if source.count(marker) != 1:
        raise RuntimeError(f"Expected one source marker: {marker!r}")
    start = source.index(marker)
    opening = source.index("{", start)
    masked = re.sub(r'"(?:\\.|[^"\\])*"|//[^\n]*', lambda m: " " * len(m[0]), source)
    depth = 0
    for position in range(opening, len(source)):
        if masked[position] == "{":
            depth += 1
        elif masked[position] == "}":
            depth -= 1
            if depth == 0:
                return source[start:position + 1]
    raise RuntimeError(f"Unclosed source block: {marker!r}")


def probe(source, output, compiler, *, source_checks=True):
    types = "\n\n".join([
        "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n" + block(source, "enum TrayState"),
        block(source, "struct LinuxUi"), block(source, "thread_local!"),
        "#[derive(Clone, Copy)]\n" + block(source, "struct Observation"),
        "#[derive(Default)]\n" + block(source, "struct Mailbox"),
    ])
    functions = "\n\n".join(block(source, name) for name in (
        "fn next_state(", "fn dispatch(", "fn present_diagnostic(", "fn rebuild_missing_tray(", "fn retry_on_main(",
        "fn present(", "fn hide_on_main(", "fn observe_on_main(", "fn publish(",
        "pub fn show(", "pub fn hide(", "pub fn resize(", "pub fn on_window_event(",
    ))
    # Compile the exact primary-click callback body; the native builder/backend is
    # still an API boundary. Reject callback drift just like named functions.
    activation = block(source, ".on_tray_icon_event(")
    functions += "\nfn tray_event(tray: &ProbeTray, event: TrayIconEvent) " + activation[activation.index("{"):]
    harness = (SKILL / "scripts/probe.rs").read_text()
    harness = harness.replace("// @SOURCE_TYPES@", types).replace("// @SOURCE_FUNCTIONS@", functions)
    (output / "probe.rs").write_text(harness)
    required([*compiler, "--edition=2021", "-Awarnings", output / "probe.rs", "-o", output / "probe"])
    if source_checks:
        checked = required([output / "probe", "--checks"])
        (output / "source-checks.txt").write_text(checked + "\n")
        if "SOURCE_CHECKS_PASS" not in checked or "FOCUS_CHECKS_PASS" not in checked:
            raise RuntimeError("Source API replay did not complete its assertions")
    observed = required([output / "probe"])
    (output / "observations.txt").write_text(observed + "\n")
    rows = [line.split() for line in observed.splitlines() if re.match(r"^[012] [01] [01] [01] ", line)]
    if len(rows) != 360 or len({tuple(row[:5]) for row in rows}) != 360:
        raise RuntimeError(f"Probe did not emit 360 unique observations: {len(rows)}")
    table = [[] for _ in range(24)]
    fields = ("state", "content", "diagnostic", "created", "exited", "settings", "apply")
    for row in rows:
        if len(row) != 12 or any(bit not in {"0", "1"} for bit in row[1:4] + row[6:]):
            raise RuntimeError(f"Malformed source observation: {row}")
        key = int(row[0]) * 8 + int(row[1]) * 4 + int(row[2]) * 2 + int(row[3])
        values = [row[5]] + ["TRUE" if bit == "1" else "FALSE" for bit in row[6:]]
        record = "[" + ", ".join(f"{f} |-> {v}" for f, v in zip(fields, values)) + "]"
        table[key].append(f"{row[4]} |-> {record}")
    (output / "Probe.tla").write_text(
        "---- MODULE Probe ----\nEXTENDS Naturals\n"
        "Steps == <<\n" + ",\n".join("[" + ",\n".join(group) + "]" for group in table) + "\n>>\n"
        "SourceStep(s, action) == Steps[1 + s.state * 8 + (IF s.content THEN 4 ELSE 0) + "
        "(IF s.diagnostic THEN 2 ELSE 0) + (IF s.created THEN 1 ELSE 0)][action]\n====\n"
    )
    print("  Compiled real-source policy and effects: 360 distinct input/action observations", flush=True)


def tlc(output, toolset, name, rules=RULES, *, recovery=False):
    jar, java, _ = toolset
    config = output / f"{name}.cfg"
    config.write_text(
        f"SPECIFICATION {'RecoverySpec' if recovery else 'Spec'}\nCONSTANT MaxPending = 2\n"
        f"CONSTANT RecoveryOnly = {'TRUE' if recovery else 'FALSE'}\nINVARIANT TypeOK\n"
        + "".join(f"INVARIANT {rule}\n" for rule in rules)
        + ("PROPERTY EventuallyRecovers\n" if recovery else "") + "CHECK_DEADLOCK FALSE\n"
    )
    result = run([
        java, "-XX:+UseParallelGC", "-Xmx2g", "-cp", jar, "tlc2.TLC", "-workers", "1",
        "-metadir", output / f"states-{name}", "-config", config, output / "LinuxTray.tla",
    ], cwd=output)
    (output / f"{name}.out").write_text(result.stdout)
    counts = re.findall(r"([\d,]+) states generated, ([\d,]+) distinct states found", result.stdout)
    if not counts or int(counts[-1][1].replace(",", "")) == 0:
        raise RuntimeError(f"TLC explored no states for {name}:\n{result.stdout}")
    violated = re.findall(r"Invariant (\w+) is violated", result.stdout)
    if result.returncode == 0 and "Model checking completed. No error has been found." in result.stdout:
        status = "PASS"
    elif result.returncode and violated and violated[-1] in rules:
        status = "FAIL"
    elif recovery and result.returncode and "Temporal properties were violated" in result.stdout:
        status = "FAIL"
    else:
        raise RuntimeError(f"TLC did not complete {name} (exit {result.returncode}):\n{result.stdout}")
    outcome = {"status": status, "states": int(counts[-1][1].replace(",", "")), "violated": violated}
    print(f"  {status} {name}: {counts[-1][1]} distinct states; raw TLC exit={result.returncode}", flush=True)
    return outcome


def check(source, output, toolset, *, rules=RULES, source_checks=True, recovery=True):
    output.mkdir(parents=True)
    probe(source, output, toolset[2], source_checks=source_checks)
    shutil.copyfile(SKILL / "model/LinuxTray.tla", output / "LinuxTray.tla")
    shutil.copyfile(SKILL / "model/LinuxTray.cfg", output / "LinuxTray.cfg")
    results = {"safety": tlc(output, toolset, "safety", rules)}
    if recovery:
        results["recovery"] = tlc(output, toolset, "recovery", recovery=True)
    (output / "result.json").write_text(json.dumps(results, indent=2) + "\n")
    return results


def replace_once(source, old, new):
    if source.count(old) != 1:
        raise RuntimeError(f"Mutation requires one real-source match: {old!r}")
    return source.replace(old, new, 1)


def mutations(source):
    # Each scratch copy changes real product code, then regenerates observations.
    return [
        ("content_without_tray", "TrayRequiredForContent", replace_once(source,
            "TrayState::Unavailable => {\n            present_diagnostic(app);",
            'TrayState::Unavailable => {\n            app.get_window("main").unwrap().show().unwrap();\n            present_diagnostic(app);')),
        ("missing_diagnostic", "OnlyDiagnosticWithoutTray", replace_once(source,
            "crate::panel_closed(app);\n        present_diagnostic(app);",
            "crate::panel_closed(app);")),
        ("hide_dismisses_diagnostic", "HideRequiresObservedTray", replace_once(source,
            "fn hide_on_main(app: &tauri::AppHandle) -> Result<(), String> {",
            "fn hide_on_main(app: &tauri::AppHandle) -> Result<(), String> {\n"
            "    UI.with(|ui| { if let Some(w) = &ui.borrow().diagnostic { w.hide(); } });")),
        ("settings_without_tray", "SettingsRequiresObservedTray", replace_once(source,
            "fn present(app: &tauri::AppHandle, settings: bool) -> Result<(), String> {",
            'fn present(app: &tauri::AppHandle, settings: bool) -> Result<(), String> {\n'
            '    if settings { app.emit("bar-settings", ()).unwrap(); }')),
        ("resize_without_tray", "ResizePreservesUnavailableSurface", replace_once(source,
            "pub fn resize(window: &tauri::Window, _height: u32) -> Result<(), String> {",
            "pub fn resize(window: &tauri::Window, _height: u32) -> Result<(), String> {\n    apply(window)?;")),
        ("activation_without_gate", "TrayRequiredForContent", replace_once(source,
            "show(tray.app_handle())",
            'tray.app_handle().get_window("main").unwrap().show()')),
        ("no_automatic_rebuild", "EventuallyRecovers", replace_once(source,
            "if observation.grace_elapsed {\n        rebuild_missing_tray(app);\n    }",
            "if observation.grace_elapsed { /* automatic rebuild removed */ }")),
        ("no_recovery", "EventuallyRecovers", replace_once(source,
            "if registered {\n        TrayState::Available",
            "if registered {\n        TrayState::Unavailable")),
    ]


def focus_mutation_check(source, output, toolset):
    # Preserve the exact historical handler independently of unrelated source
    # changes; insert it into the current real source and require rejection.
    old_handler = """pub fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if matches!(event, tauri::WindowEvent::Focused(false)) {
        let _ = dispatch(window.app_handle(), hide_on_main);
    }
}"""
    broken = replace_once(source, block(source, "pub fn on_window_event("), old_handler)
    prior_hash = hashlib.sha256(broken.encode()).hexdigest()
    if hashlib.sha256(old_handler.encode()).hexdigest() != "f22d7b15f1c5c664eaadc27d51d4a57aaca3d18c4e2fcdad22fcbb227f5b6666":
        raise RuntimeError("Historical focus handler differs from reviewed exact handler")
    scratch = output / "scratch-source" / "focus_loss_hides" / SOURCE
    scratch.parent.mkdir(parents=True, exist_ok=True)
    scratch.write_text(broken)
    target = output / "focus_loss_hides"
    target.mkdir()
    probe(broken, target, toolset[2], source_checks=False)
    result = run([target / "probe", "--focus-checks"])
    (target / "focus-checks.txt").write_text(result.stdout)
    if result.returncode == 0 or "FocusPreservesVisiblePanel" not in result.stdout:
        raise RuntimeError("Historical focus handler did not fail FocusPreservesVisiblePanel")
    scratch.unlink()
    print(f"  FAIL historical focus handler: FocusPreservesVisiblePanel; prior sha256={prior_hash}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true", help="Require real-source mutations to fail every named property")
    parser.add_argument("--out", type=Path, help="New or empty output directory (default: temporary directory)")
    args = parser.parse_args()
    raw = (ROOT / SOURCE).read_bytes()
    source = raw.decode()
    coverage = json.loads((SKILL / "coverage.json").read_text())
    actual_hash = hashlib.sha256(raw).hexdigest()
    if actual_hash != coverage["sha256"]:
        raise RuntimeError("SOURCE DRIFT: platform_linux.rs changed; follow SKILL.md to review correspondence before updating coverage.json")
    output = args.out.resolve() if args.out else Path(tempfile.mkdtemp(prefix="t3bar-linux-tray-"))
    if output.exists() and any(output.iterdir()):
        raise RuntimeError(f"Output must be new or empty: {output}")
    output.mkdir(parents=True, exist_ok=True)
    print(f"Artifacts: {output}", flush=True)
    toolset = tools()
    print("Baseline:", flush=True)
    baseline = check(source, output / "baseline", toolset)
    if args.self_test:
        if any(row["status"] != "PASS" for row in baseline.values()):
            raise RuntimeError("Baseline must pass before mutation evidence can certify the verifier")
        for name, rule, broken in mutations(source):
            scratch = output / "scratch-source" / name / SOURCE
            scratch.parent.mkdir(parents=True)
            scratch.write_text(broken)
            print(f"Real-source mutation {name}, requires FAIL {rule}:", flush=True)
            mutant = check(broken, output / name, toolset, rules=(rule,) if rule != "EventuallyRecovers" else (),
                           source_checks=False, recovery=rule == "EventuallyRecovers")
            result = mutant["recovery" if rule == "EventuallyRecovers" else "safety"]
            if result["status"] != "FAIL" or (rule != "EventuallyRecovers" and rule not in result["violated"]):
                raise RuntimeError(f"Mutation {name} was not killed by {rule}")
            scratch.unlink()
        print("Historical real-source focus handler, requires FAIL FocusPreservesVisiblePanel:", flush=True)
        focus_mutation_check(source, output, toolset)
        print("Scratch mutations dropped; repeat unmodified baseline:", flush=True)
        restored = check((ROOT / SOURCE).read_text(), output / "restored", toolset)
        if restored != baseline or hashlib.sha256((ROOT / SOURCE).read_bytes()).hexdigest() != actual_hash:
            raise RuntimeError("Restored check differs or product source changed during verification")
        print("SELF-TEST PASS: eight policy mutations and historical focus handler caught; product source unchanged; baseline reproduced.", flush=True)
    return 1 if any(row["status"] == "FAIL" for row in baseline.values()) else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (RuntimeError, OSError, subprocess.SubprocessError, ValueError, KeyError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        sys.exit(2)
