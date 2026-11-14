//! Fixtures shared by the test binaries under `tests/`. Each binary
//! includes this file with `#[path = "../common/mod.rs"] mod common;`.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

/// `program`, run with no `GIT_*` variable from the environment the
/// tests run in. A pre-commit hook exports `GIT_INDEX_FILE`, absolute when
/// the commit runs from a subdirectory, and git run against a scratch
/// repository would read it and write into this repository's index.
pub fn without_git_env(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
}

/// Run `git` in the scratch repository `repo`, asserting success. git is
/// a standing requirement of this repo, so a host without it fails here.
/// No hook runs: the hooks path names a directory that does not exist,
/// whatever the global config says.
pub fn git(repo: &Path, args: &[&str]) {
    let status = without_git_env("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "core.hooksPath=no-hooks"])
        .args(args)
        .status()
        .expect("git on PATH");
    assert!(status.success(), "git {args:?} failed");
}

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
