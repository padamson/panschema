//! Regression coverage against every vendored release of the real dogfood
//! schemas (`scimantic-schema`, `scidatica-schema`, …).
//!
//! Each released tag is checked in as a frozen snapshot under
//! `tests/fixtures/dogfood/<repo>/<tag>.yaml` (vendored by hand via
//! `scripts/vendor-dogfood-schemas.sh`). Released tags are immutable, so the
//! snapshots never drift and the whole suite runs offline by default — no
//! network at test time. The vendor script is the only network path.
//!
//! Two tiers of coverage, by cost:
//!
//! * Every vendored release is read through the LinkML reader, rendered to
//!   Rust, and checked with `syn::parse_file` (cheap — runs for all tags).
//! * The latest vendored release per repo is additionally `cargo build`-
//!   compiled in a scratch crate (expensive — bounded to one tag per repo to
//!   keep CI time in check; see [`compile_latest_dogfood_release_per_repo`]).
//!
//! If a vendored release fails any tier, that is a real finding: panschema
//! cannot handle a schema it shipped support for. The fix is to repair the
//! writer, not to drop the fixture.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use panschema::io::Reader;
use panschema::rust_writer::RustWriter;
use panschema::yaml_reader::YamlReader;
use semver::Version;

/// One vendored snapshot: its source repo, its tag, and its on-disk path.
struct DogfoodFixture {
    repo: String,
    tag: String,
    path: PathBuf,
}

/// Absolute path to `tests/fixtures/dogfood`.
fn dogfood_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("dogfood")
}

/// Discover every `tests/fixtures/dogfood/<repo>/<tag>.yaml` snapshot. The
/// directory name is the source repo; the file stem is the release tag.
fn discover_fixtures() -> Vec<DogfoodFixture> {
    let root = dogfood_root();
    let mut fixtures = Vec::new();

    let repo_dirs = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("read dogfood fixture root {}: {e}", root.display()));
    for repo_entry in repo_dirs {
        let repo_entry = repo_entry.expect("read dogfood repo dir entry");
        let repo_path = repo_entry.path();
        if !repo_path.is_dir() {
            continue;
        }
        let repo = repo_entry.file_name().to_string_lossy().into_owned();

        let tag_files = std::fs::read_dir(&repo_path)
            .unwrap_or_else(|e| panic!("read dogfood repo dir {}: {e}", repo_path.display()));
        for tag_entry in tag_files {
            let tag_entry = tag_entry.expect("read dogfood tag file entry");
            let path = tag_entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
                continue;
            }
            let tag = path
                .file_stem()
                .and_then(|s| s.to_str())
                .expect("dogfood fixture file stem is valid UTF-8")
                .to_owned();
            fixtures.push(DogfoodFixture {
                repo: repo.clone(),
                tag,
                path,
            });
        }
    }

    fixtures
        .sort_by(|a, b| (a.repo.as_str(), a.tag.as_str()).cmp(&(b.repo.as_str(), b.tag.as_str())));
    fixtures
}

/// Parse a tag like `v0.2.0` (or `0.2.0`) into a semver `Version`, dropping a
/// leading `v`. Used only to pick the latest tag per repo for compile
/// coverage; tags that don't parse are skipped from the "latest" race.
fn tag_to_version(tag: &str) -> Option<Version> {
    Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()
}

// ---------------------------------------------------------------------------
// Tier 1: every vendored release renders to parseable Rust
// ---------------------------------------------------------------------------

/// Every vendored dogfood release reads through the LinkML reader and renders
/// to Rust that `syn::parse_file` accepts. A render regression against any
/// real shipped schema fails here — no network, fixtures are in-tree.
#[test]
fn every_vendored_release_renders_to_parseable_rust() {
    let fixtures = discover_fixtures();
    assert!(
        !fixtures.is_empty(),
        "no vendored dogfood fixtures found under {}",
        dogfood_root().display()
    );

    for fixture in &fixtures {
        let schema = YamlReader::new().read(&fixture.path).unwrap_or_else(|e| {
            panic!(
                "dogfood {}@{} failed to read via the LinkML reader: {e}\n  ({})",
                fixture.repo,
                fixture.tag,
                fixture.path.display()
            )
        });

        let body = RustWriter::new().render(&schema);

        syn::parse_file(&body).unwrap_or_else(|e| {
            let preview = body.chars().take(2000).collect::<String>();
            panic!(
                "dogfood {}@{} rendered Rust that failed to parse: {e}\n\
                 --- preview (first 2k chars) ---\n{preview}",
                fixture.repo, fixture.tag
            )
        });
    }
}

// ---------------------------------------------------------------------------
// Tier 2: the latest vendored release per repo compiles
// ---------------------------------------------------------------------------

/// The latest vendored release of each repo `cargo build`-compiles in a
/// scratch crate that depends on `serde` + `chrono`. This catches Rust that
/// parses but doesn't compile (undefined type references, unsatisfied derive
/// bounds) — failure modes `syn::parse_file` can't see.
///
/// Policy: only the **latest** tag per repo is compiled, not every vendored
/// tag, to keep CI time bounded (compiling drags in serde/chrono). Older tags
/// stay at the tier-1 parse check. If the per-repo compile time stays small,
/// widening this to all tags is fine; the cap is here deliberately, not by
/// accident.
#[test]
fn compile_latest_dogfood_release_per_repo() {
    let fixtures = discover_fixtures();

    // Pick the highest-semver tag per repo.
    let mut latest: BTreeMap<&str, (&DogfoodFixture, Version)> = BTreeMap::new();
    for fixture in &fixtures {
        let Some(version) = tag_to_version(&fixture.tag) else {
            continue;
        };
        latest
            .entry(fixture.repo.as_str())
            .and_modify(|(cur_fix, cur_ver)| {
                if version > *cur_ver {
                    *cur_fix = fixture;
                    *cur_ver = version.clone();
                }
            })
            .or_insert((fixture, version));
    }

    assert!(
        !latest.is_empty(),
        "no vendored dogfood release has a parseable semver tag to compile"
    );

    for (repo, (fixture, _version)) in &latest {
        let schema = YamlReader::new()
            .read(&fixture.path)
            .unwrap_or_else(|e| panic!("dogfood {repo}@{} failed to read: {e}", fixture.tag));
        let body = RustWriter::new().render(&schema);

        syn::parse_file(&body).unwrap_or_else(|e| {
            panic!(
                "dogfood {repo}@{} rendered unparseable Rust: {e}",
                fixture.tag
            )
        });

        let tmp = tempfile::tempdir().expect("tempdir for dogfood scratch crate");
        write_scratch_crate(tmp.path(), &body);
        cargo_build_scratch(tmp.path(), repo, &fixture.tag);
    }
}

/// Build a self-contained library crate around the generated module so its
/// public API is `cargo build`-checked. `src/generated.rs` holds the rendered
/// code; `src/lib.rs` just re-exports it as a module — the build alone proves
/// the generated code type-checks against serde/chrono.
fn write_scratch_crate(root: &Path, generated_module_body: &str) {
    std::fs::write(
        root.join("Cargo.toml"),
        r#"[package]
name = "dogfood-codegen-test"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
serde = { version = "1", features = ["derive"] }
chrono = { version = "0.4", features = ["serde"] }
"#,
    )
    .expect("write Cargo.toml");
    std::fs::create_dir_all(root.join("src")).expect("mkdir src/");
    std::fs::write(root.join("src/generated.rs"), generated_module_body)
        .expect("write generated.rs");
    std::fs::write(
        root.join("src/lib.rs"),
        "#![allow(dead_code, unused_variables)]\npub mod generated;\n",
    )
    .expect("write lib.rs");
}

/// Invoke `cargo build --quiet` on the scratch crate. `CARGO_TARGET_DIR` is
/// redirected into `CARGO_TARGET_TMPDIR` so build artifacts don't pollute the
/// workspace target dir; `CARGO_HOME` is inherited so serde/chrono downloads
/// are cached across runs. A failure names the offending schema + tag.
fn cargo_build_scratch(root: &Path, repo: &str, tag: &str) {
    let status = Command::new("cargo")
        .args(["build", "--quiet"])
        .current_dir(root)
        .env(
            "CARGO_TARGET_DIR",
            PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("dogfood-scratch-target"),
        )
        .status()
        .expect("invoke `cargo build` on dogfood scratch crate");
    assert!(
        status.success(),
        "dogfood {repo}@{tag}: generated Rust failed to compile in the scratch crate"
    );
}
