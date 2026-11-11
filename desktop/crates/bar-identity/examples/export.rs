use bar_identity::{
    EnvironmentAccess, EnvironmentId, EnvironmentMachineKind, EnvironmentSelection,
    EnvironmentView, ThreadId, ThreadRef,
};
use ts_rs::TS;

fn main() {
    let output = format!("// Generated from Rust serde types. Run npm run generate:contracts.\n\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        EnvironmentId::decl(), ThreadId::decl(), ThreadRef::decl(),
        EnvironmentSelection::decl(), EnvironmentMachineKind::decl(), EnvironmentAccess::decl(), EnvironmentView::decl()).replace("type ", "export type ");
    // ts-rs leaves trailing spaces that editors strip from the tracked file.
    let output: String = output
        .lines()
        .map(|line| format!("{}\n", line.trim_end()))
        .collect();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../src/generated-environments.ts");
    match std::env::args().nth(1).as_deref() {
        Some("--write") => std::fs::write(path, output).unwrap(),
        Some("--check") => assert_eq!(
            std::fs::read_to_string(path).expect("run npm run generate:contracts"),
            output,
            "run npm run generate:contracts"
        ),
        _ => panic!("use --write or --check"),
    }
}
