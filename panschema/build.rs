//! Builds `panschema-viz/pkg/` (the wasm bundle `html_writer.rs`
//! pulls in via `include_str!` / `include_bytes!`) so a fresh
//! checkout compiles without a manual `wasm-pack` step.
//!
//! cargo's `rerun-if-changed` keeps this out of the incremental
//! hot path — the script only runs when something under
//! `panschema-viz/` changes. Each run unconditionally invokes
//! wasm-pack, which is itself fast when its own incremental cache
//! is warm.

use std::process::{Command, Stdio};

fn main() {
    println!("cargo:rerun-if-changed=../panschema-viz/src");
    println!("cargo:rerun-if-changed=../panschema-viz/Cargo.toml");

    if !wasm_pack_available() {
        eprintln!();
        eprintln!("error: `wasm-pack` is not on PATH. The HTML writer");
        eprintln!("       embeds the WebGPU visualization bundle at");
        eprintln!("       compile time, so wasm-pack must be installed.");
        eprintln!();
        eprintln!("Fix: `cargo install wasm-pack`");
        eprintln!();
        std::process::exit(1);
    }

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
