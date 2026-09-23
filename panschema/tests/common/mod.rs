//! Fixtures shared by the test binaries under `tests/`. Each binary
//! includes this file with `#[path = "../common/mod.rs"] mod common;`.

use std::process::Command;

/// Runs `panschema generate --schema <schema> --output <dir> <extra_args>`
/// into a fresh scratch directory and returns its guard, so the generated
/// site lives exactly as long as the test that reads it and is removed
/// on every path, a panic included.
pub fn generate_site(schema: &str, extra_args: &[&str]) -> tempfile::TempDir {
    let scratch = tempfile::tempdir().expect("tempdir");
    let output = scratch.path().to_str().unwrap().to_string();
    let mut args = vec!["generate", "--schema", schema, "--output", output.as_str()];
    args.extend_from_slice(extra_args);
    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args(&args)
        .status()
        .expect("Failed to execute panschema");
    assert!(
        status.success(),
        "panschema {} exited with error",
        args.join(" ")
    );
    scratch
}
