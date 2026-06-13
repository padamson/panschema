//! Builds `panschema-viz/pkg/` (the wasm bundle `html_writer.rs`
//! pulls in via `include_str!` / `include_bytes!`) so a fresh
//! checkout compiles without a manual `wasm-pack` step.
//!
//! The script watches the viz crate's sources, so a change there
//! re-runs it and refreshes the embedded bundle — otherwise a
//! wire-format change in `panschema-viz` would re-embed a stale
//! bundle and break the rendered graph with no compile error.
//! Cargo only re-runs the script when a watched input actually
//! changed, so a build that doesn't touch the viz crate pays
//! nothing. CI's lint job stubs the pkg/ files with empty
//! placeholders and has no wasm-pack; the script detects the
//! missing tool and uses the stubs as-is for type-checking.

use std::path::Path;
use std::process::{Command, Stdio};

const PKG_JS: &str = "../panschema-viz/pkg/panschema_viz.js";
const PKG_WASM: &str = "../panschema-viz/pkg/panschema_viz_bg.wasm";

fn main() {
    // Watch the viz crate's sources (not just the pkg/ outputs) so
    // an edit there re-triggers this script. This is the freshness
    // mechanism — we let Cargo decide *when* a rebuild is needed
    // rather than hand-rolling an mtime/walkdir staleness check.
    println!("cargo:rerun-if-changed=../panschema-viz/src");
    println!("cargo:rerun-if-changed=../panschema-viz/Cargo.toml");
    println!("cargo:rerun-if-changed={PKG_JS}");
    println!("cargo:rerun-if-changed={PKG_WASM}");

    let pkg_exists = Path::new(PKG_JS).exists() && Path::new(PKG_WASM).exists();

    if !wasm_pack_available() {
        // No wasm-pack: the lint runner stubs pkg/ for type-checking,
        // so use whatever bundle exists. A fresh checkout with neither
        // tool nor bundle can't proceed.
        if pkg_exists {
            return;
        }
        eprintln!();
        eprintln!("error: `wasm-pack` is not on PATH. The HTML writer");
        eprintln!("       embeds the WebGPU visualization bundle at");
        eprintln!("       compile time, so wasm-pack must be installed.");
        eprintln!();
        eprintln!("Fix: `cargo install wasm-pack`");
        eprintln!();
        std::process::exit(1);
    }

    // wasm-pack is available and a watched input changed (or this is
    // a fresh build): (re)build the bundle so the embedded WASM
    // matches the current viz sources.

    // Debug builds get `--dev` (skips wasm-opt, ~15s/run); release
    // builds keep `--release` so shipped bundles stay size-optimized.
    let profile_flag = match std::env::var("PROFILE").as_deref() {
        Ok("release") => "--release",
        _ => "--dev",
    };

    // Use panschema-viz/target/ so wasm-pack's recursive cargo
    // doesn't fight the outer cargo for the workspace target lock.
    let status = Command::new("wasm-pack")
        .args([
            "build",
            "--target",
            "web",
            profile_flag,
            "--features",
            "webgpu",
        ])
        .current_dir("../panschema-viz")
        .env("CARGO_TARGET_DIR", "target")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .expect("failed to spawn wasm-pack");

    if !status.success() {
        panic!("wasm-pack build failed (exit {status})");
    }
}

fn wasm_pack_available() -> bool {
    Command::new("wasm-pack")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
