//! End-to-end integration tests for the Rust codegen writer against
//! the scimantic-schema v0.1.0 LinkML schema — the real-world target
//! the writer is built for.
//!
//! Each test is `#[ignore]`d by default because the fixture is fetched
//! via `panschema add github:padamson/scimantic-schema@0.1.0` into a
//! workspace-local cache (`CARGO_TARGET_TMPDIR/scimantic-fixture-cache/`).
//! The first invocation in a fresh workspace requires network; warm
//! runs short-circuit through the cache.
//!
//! Run them explicitly with:
//!
//! ```bash
//! cargo nextest run -p panschema --test rust_writer -- --ignored
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

use panschema::io::Reader;
use panschema::linkml::SchemaDefinition;
use panschema::rust_writer::RustWriter;
use panschema::yaml_reader::YamlReader;

/// Ensure scimantic-schema v0.1.0 is present in a workspace-local cache,
/// fetching it via `panschema add` if not. Returns the absolute path to
/// `schema/scimantic.yaml` inside the cache.
fn ensure_scimantic_cached() -> PathBuf {
    let cache_root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("scimantic-fixture-cache");
    let schema_path = cache_root.join(
        "github/padamson/scimantic-schema/0.1.0/scimantic-schema-0.1.0/schema/scimantic.yaml",
    );
    if schema_path.exists() {
        return schema_path;
    }

    let consumer = tempfile::tempdir().expect("tempdir for fixture consumer");
    std::fs::write(consumer.path().join("panschema.toml"), "[schemas]\n")
        .expect("write fixture manifest");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args(["add", "github:padamson/scimantic-schema@0.1.0"])
        .current_dir(consumer.path())
        .env("PANSCHEMA_CACHE_ROOT", &cache_root)
        .status()
        .expect("invoke `panschema add` to populate fixture cache");
    assert!(
        status.success(),
        "`panschema add github:padamson/scimantic-schema@0.1.0` failed; \
         verify network access or that the tag exists upstream"
    );

    assert!(
        schema_path.exists(),
        "expected scimantic schema at {} after fetch; cache layout may have changed",
        schema_path.display()
    );
    schema_path
}

fn read_scimantic() -> SchemaDefinition {
    YamlReader::new()
        .read(&ensure_scimantic_cached())
        .expect("parse scimantic schema")
}

/// Extract the body of `pub struct <name> { ... }` from a render output.
fn extract_struct<'a>(body: &'a str, name: &str) -> &'a str {
    let needle = format!("pub struct {name} {{");
    let start = body
        .find(&needle)
        .unwrap_or_else(|| panic!("`pub struct {name}` not found in render"));
    let after = &body[start + needle.len()..];
    let end = after
        .find("\n}")
        .unwrap_or_else(|| panic!("unterminated `pub struct {name}` block"));
    &after[..end]
}

// ---------------------------------------------------------------------------
// Acceptance tests
// ---------------------------------------------------------------------------

/// The full scimantic schema renders to syntactically valid Rust source.
#[test]
#[ignore = "requires network for cold scimantic cache"]
fn scimantic_renders_as_syntactically_valid_rust() {
    let schema = read_scimantic();
    let body = RustWriter::new().render(&schema);
    syn::parse_file(&body).unwrap_or_else(|e| {
        let preview = body.chars().take(2000).collect::<String>();
        panic!("generated Rust failed to parse: {e}\n--- preview (first 2k chars) ---\n{preview}")
    });
}

/// A class declared with `is_a: Parent` produces a `pub trait Parent`
/// with getter methods for the parent's direct slots, plus an `impl
/// Parent for Child` block for every subclass. Verified against the
/// `UncertaintyModel → Vagueness` chain because `UncertaintyModel` has
/// non-empty direct slots — an empty `pub trait Entity` would not
/// exercise the getter-method shape.
#[test]
#[ignore = "requires network for cold scimantic cache; writer does not emit inheritance traits"]
fn scimantic_classes_with_is_a_get_trait_impls() {
    let schema = read_scimantic();
    let body = RustWriter::new().render(&schema);
    assert!(
        body.contains("pub trait UncertaintyModel"),
        "expected `pub trait UncertaintyModel`; not found in render"
    );
    assert!(
        body.contains("impl UncertaintyModel for Vagueness"),
        "expected `impl UncertaintyModel for Vagueness`; not found in render"
    );
}

/// `slot_usage` on a subclass refines the inherited slot's range,
/// `required`, and `multivalued`. The generated subclass struct uses the
/// refined definition. `Question.wasGeneratedBy` has its range narrowed
/// from the parent slot's `Activity` to `QuestionFormation`, and the
/// generated field reflects the refinement.
#[test]
#[ignore = "requires network for cold scimantic cache; writer does not apply slot_usage overrides"]
fn scimantic_slot_usage_overrides_apply_to_subclass_fields() {
    let schema = read_scimantic();
    let body = RustWriter::new().render(&schema);
    let question = extract_struct(&body, "Question");
    assert!(
        question.contains("pub was_generated_by: Option<QuestionFormation>")
            || question.contains("pub was_generated_by: QuestionFormation"),
        "expected `Question.was_generated_by` to use the refined range `QuestionFormation`; \
         got struct body:\n{question}"
    );
}

/// A slot whose range is declared via `any_of: [A, B, C]` produces a
/// per-slot union enum with `#[serde(untagged)]` and one variant per
/// member type. scimantic uses this on `Question.wasDerivedFrom`
/// (any of `Question | Annotation | Evidence`), among others.
#[test]
#[ignore = "requires network for cold scimantic cache; writer does not emit any_of unions"]
fn scimantic_any_of_ranges_become_untagged_enums() {
    let schema = read_scimantic();
    let body = RustWriter::new().render(&schema);
    assert!(
        body.contains("#[serde(untagged)]"),
        "expected at least one `#[serde(untagged)]` enum (any_of union); not found in render"
    );
}

/// scimantic v0.1.0 renders into Rust that compiles against `serde` +
/// `chrono`. Catches semantic issues (undefined type references, missing
/// imports, derive bounds the generated code doesn't satisfy) that
/// `syn::parse_file` cannot see.
#[test]
#[ignore = "requires network for cold scimantic cache; spawns `cargo build`"]
fn scimantic_output_compiles_via_cargo_build() {
    let schema = read_scimantic();
    let body = RustWriter::new().render(&schema);

    let tmp = tempfile::tempdir().expect("tempdir for scratch crate");
    write_scratch_crate(tmp.path(), &body);

    let status = Command::new("cargo")
        .args(["build", "--quiet"])
        .current_dir(tmp.path())
        // Override CARGO_TARGET_DIR so the scratch crate's build artifacts
        // land inside CARGO_TARGET_TMPDIR rather than polluting the
        // workspace target dir. CARGO_HOME is inherited so registry
        // downloads (serde, chrono) are cached across runs.
        .env(
            "CARGO_TARGET_DIR",
            PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("scratch-crate-target"),
        )
        .status()
        .expect("invoke `cargo build` on scratch crate");
    assert!(
        status.success(),
        "scratch crate failed to compile against the generated module"
    );
}

fn write_scratch_crate(root: &Path, lib_body: &str) {
    std::fs::write(
        root.join("Cargo.toml"),
        r#"[package]
name = "scimantic-codegen-test"
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
    std::fs::write(root.join("src/lib.rs"), lib_body).expect("write lib.rs");
}
