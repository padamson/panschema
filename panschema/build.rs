//! Ensures `panschema-viz/pkg/` artifacts exist before `panschema`
//! compiles its `include_str!` / `include_bytes!` references in
//! `html_writer.rs`. Without this, `cargo install --git` fails on a
//! fresh checkout because wasm-pack output isn't tracked in git.

use std::path::Path;
use std::process::{Command, Stdio};

const PKG_DIR: &str = "../panschema-viz/pkg";
const PKG_JS: &str = "../panschema-viz/pkg/panschema_viz.js";
const PKG_WASM: &str = "../panschema-viz/pkg/panschema_viz_bg.wasm";

fn main() {
    println!("cargo:rerun-if-changed=../panschema-viz/src");
    println!("cargo:rerun-if-changed=../panschema-viz/Cargo.toml");
    println!("cargo:rerun-if-changed={PKG_JS}");
    println!("cargo:rerun-if-changed={PKG_WASM}");

    if Path::new(PKG_JS).exists() && Path::new(PKG_WASM).exists() {
        return;
    }

    if !wasm_pack_available() {
        eprintln!();
        eprintln!("error: panschema-viz/pkg/ artifacts are missing and `wasm-pack`");
        eprintln!("       is not on PATH. The HTML writer embeds the WebGPU");
        eprintln!("       visualization bundle at compile time, so it must exist");
        eprintln!("       before `cargo build`.");
        eprintln!();
        eprintln!("Fix: install wasm-pack, then re-run cargo:");
        eprintln!("     cargo install wasm-pack");
        eprintln!();
        std::process::exit(1);
    }

    let status = Command::new("wasm-pack")
        .args(["build", "--target", "web", "--release"])
        .current_dir("../panschema-viz")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .expect("failed to spawn wasm-pack");

    if !status.success() {
        panic!("wasm-pack build failed (exit {status})");
    }

    assert!(
        Path::new(PKG_JS).exists() && Path::new(PKG_WASM).exists(),
        "wasm-pack succeeded but {PKG_DIR}/ is missing expected outputs"
    );
}

fn wasm_pack_available() -> bool {
    Command::new("wasm-pack")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
