//! Builds `panschema-viz/pkg/` (the wasm bundle `html_writer.rs`
//! pulls in via `include_str!` / `include_bytes!`) so a fresh
//! checkout compiles without a manual `wasm-pack` step.
//!
//! The script rebuilds the bundle only when it's missing or older
//! than the viz crate's sources — otherwise a wire-format change in
//! `panschema-viz` would re-embed a stale bundle and break the
//! rendered graph with no compile error. The mtime check is what
//! distinguishes "a developer just edited a viz source" (rebuild)
//! from "the bundle was already produced for this build" (skip):
//! CI's Test job builds the bundle in an explicit `wasm-pack` step
//! (cached on the viz source hash) before `cargo test`, so the
//! bundle is no older than the sources and this script must not
//! redundantly rebuild it — that second build, into the same target
//! dir with a different profile, fails on the Windows wasm+wgpu
//! toolchain. CI's lint job stubs the pkg/ files and has no
//! wasm-pack; the script detects the missing tool and uses the stubs
//! as-is for type-checking.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::SystemTime;

const PKG_JS: &str = "../panschema-viz/pkg/panschema_viz.js";
const PKG_WASM: &str = "../panschema-viz/pkg/panschema_viz_bg.wasm";
const VIZ_SRC: &str = "../panschema-viz/src";
const VIZ_MANIFEST: &str = "../panschema-viz/Cargo.toml";

fn main() {
    // Watch the viz crate's sources (not just the pkg/ outputs) so an
    // edit there re-triggers this script; the mtime check below then
    // decides whether the bundle is actually out of date.
    println!("cargo:rerun-if-changed={VIZ_SRC}");
    println!("cargo:rerun-if-changed={VIZ_MANIFEST}");
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

    // Skip the rebuild when the bundle already reflects the current
    // sources — a present bundle no older than every viz source.
    if pkg_exists && !bundle_is_stale() {
        return;
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

/// `true` when any viz source is newer than the built bundle, so the
/// embedded WASM no longer matches the sources. A source we can't
/// stat (or a bundle mtime we can't read) is treated as stale —
/// rebuilding is the safe default.
fn bundle_is_stale() -> bool {
    let Some(bundle_mtime) = [PKG_JS, PKG_WASM]
        .iter()
        .map(|p| mtime(Path::new(p)))
        .min()
        .flatten()
    else {
        return true;
    };
    let newest_source = newest_mtime(Path::new(VIZ_SRC)).max(mtime(Path::new(VIZ_MANIFEST)));
    match newest_source {
        Some(src) => src > bundle_mtime,
        None => true,
    }
}

/// Newest modification time among all files under `dir` (recursive),
/// or `None` if it can't be read.
fn newest_mtime(dir: &Path) -> Option<SystemTime> {
    let mut newest: Option<SystemTime> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let t = if path.is_dir() {
            newest_mtime(&path)
        } else {
            mtime(&path)
        };
        newest = newest.max(t);
    }
    newest
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn wasm_pack_available() -> bool {
    Command::new("wasm-pack")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
