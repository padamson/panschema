//! Builds `panschema-viz/pkg/` (the wasm bundle `html_writer.rs`
//! pulls in via `include_str!` / `include_bytes!`) so a fresh
//! checkout compiles without a manual `wasm-pack` step.
//!
//! After the initial bundle exists, this script does nothing — it is
//! a one-time bootstrap, not a freshness tracker. Refreshing the
//! embedded bundle after a `panschema-viz` change is an explicit
//! step: run `scripts/dev-install.sh` (which rebuilds the bundle,
//! then `cargo install`s) for the dogfood loop, or `wasm-pack build`
//! by hand. Auto-detecting staleness here is deliberately avoided:
//! the bundle is produced by two paths (this script for fresh
//! checkouts, CI's explicit cached `wasm-pack` step), and any
//! freshness signal they'd have to share (mtime, hash) is fragile
//! across the CI cache boundary — an mtime check let a redundant
//! rebuild through on a cache-restored bundle and broke Windows CI.
//! CI's lint job stubs the pkg/ files with empty placeholders, which
//! satisfy `include_bytes!` for type-checking without needing
//! wasm-pack on the lint runner.

use std::path::Path;
use std::process::{Command, Stdio};

const PKG_JS: &str = "../panschema-viz/pkg/panschema_viz.js";
const PKG_WASM: &str = "../panschema-viz/pkg/panschema_viz_bg.wasm";

fn main() {
    println!("cargo:rerun-if-changed={PKG_JS}");
    println!("cargo:rerun-if-changed={PKG_WASM}");
    emit_build_version();

    if Path::new(PKG_JS).exists() && Path::new(PKG_WASM).exists() {
        return;
    }

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

/// Emit `PANSCHEMA_VERSION`: the crate version, plus the commit for builds
/// that aren't a tagged release.
///
/// Without this, a binary built from `main` and a binary from the last
/// release both report the crate version, so `--version` can't tell a
/// consumer which code they have — and "build from `main` to get the fix" is
/// unverifiable advice. Three cases:
///
/// - **No git** (a crates.io or tarball install): the crate version alone.
///   Released artifacts must not carry a commit they can't reproduce.
/// - **HEAD is exactly `v<version>`**: also the crate version alone — that
///   *is* the release, however it was built.
/// - **Anything else**: `<version> (<short sha>)`.
///
/// The commit is baked at compile time, so the rerun triggers below decide
/// how fresh it is: `.git/HEAD` covers checkouts and branch switches, the
/// resolved ref file covers commits on the current branch. A dirty-tree
/// marker is deliberately omitted — nothing here re-runs on an uncommitted
/// edit, so it would be wrong as often as right, and a confidently wrong
/// build id is worse than none.
fn emit_build_version() {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let build_id = match git_short_sha() {
        Some(sha) if !at_release_tag(&version) => format!("{version} ({sha})"),
        _ => version,
    };
    println!("cargo:rustc-env=PANSCHEMA_VERSION={build_id}");

    let git_dir = Path::new("../.git");
    if !git_dir.exists() {
        return;
    }
    println!("cargo:rerun-if-changed=../.git/HEAD");
    // `.git/HEAD` only changes when the checkout moves; the branch's ref file
    // is what changes on commit. Absent when refs are packed, which just
    // means the sha can lag until the next checkout.
    if let Ok(head) = std::fs::read_to_string(git_dir.join("HEAD"))
        && let Some(reference) = head.strip_prefix("ref: ")
        && git_dir.join(reference.trim()).exists()
    {
        println!("cargo:rerun-if-changed=../.git/{}", reference.trim());
    }
}

fn git_short_sha() -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!sha.is_empty()).then_some(sha)
}

/// Whether HEAD carries the tag this crate version would be released as.
fn at_release_tag(version: &str) -> bool {
    Command::new("git")
        .args(["describe", "--exact-match", "--tags", "HEAD"])
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .is_some_and(|tag| tag.trim() == format!("v{version}"))
}
