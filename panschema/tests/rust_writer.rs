//! End-to-end integration tests for the Rust codegen writer against
//! the scimantic-schema v0.1.0 LinkML schema — the real-world target
//! the writer is built for.
//!
//! The fixture is fetched via `panschema add github:padamson/scimantic-schema@0.1.0`
//! into a workspace-local cache (`CARGO_TARGET_TMPDIR/scimantic-fixture-cache/`).
//! The first invocation in a fresh workspace requires network access;
//! warm runs short-circuit through the cache.

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
fn scimantic_renders_as_syntactically_valid_rust() {
    let schema = read_scimantic();
    let body = RustWriter::new().render(&schema);
    syn::parse_file(&body).unwrap_or_else(|e| {
        let preview = body.chars().take(2000).collect::<String>();
        panic!("generated Rust failed to parse: {e}\n--- preview (first 2k chars) ---\n{preview}")
    });
}

/// A class declared with `is_a: Parent` produces a `pub trait Parent`
/// with supertrait bounds following the LinkML inheritance chain, plus
/// an `impl Parent for Child` block for every concrete descendant.
/// Verified against the `UncertaintyModel → Vagueness` chain because
/// `UncertaintyModel` has non-empty direct slots, exercising both the
/// trait emission and the inheritance-flattening path.
#[test]
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
///
/// `QuestionFormation` is a leaf struct (no subclasses) so class-typed
/// single-valued fields are wrapped in `Box` to keep struct layouts
/// finite when classes transitively reference themselves.
#[test]
fn scimantic_slot_usage_overrides_apply_to_subclass_fields() {
    let schema = read_scimantic();
    let body = RustWriter::new().render(&schema);
    let question = extract_struct(&body, "Question");
    assert!(
        question.contains("pub was_generated_by: Option<Box<QuestionFormation>>")
            || question.contains("pub was_generated_by: Box<QuestionFormation>"),
        "expected `Question.was_generated_by` to use the refined range `QuestionFormation`; \
         got struct body:\n{question}"
    );
}

/// A slot whose range is declared via `any_of: [A, B, C]` produces a
/// per-slot union enum with `#[serde(untagged)]` and one variant per
/// member type. scimantic uses this on `Question.wasDerivedFrom`
/// (any of `Question | Annotation | Evidence`), among others.
#[test]
fn scimantic_any_of_ranges_become_untagged_enums() {
    let schema = read_scimantic();
    let body = RustWriter::new().render(&schema);
    assert!(
        body.contains("#[serde(untagged)]"),
        "expected at least one `#[serde(untagged)]` enum (any_of union); not found in render"
    );
}

/// The generated module compiles in a downstream consumer crate that
/// depends on `serde` + `chrono`, and the generated public API is
/// usable: a `Question` is constructible via struct literal with the
/// expected fields. Catches three failure modes that `syn::parse_file`
/// cannot see:
///
/// 1. Undefined type references and missing imports in the generated
///    code (caught at `cargo build` time).
/// 2. Derive bounds that the generated types don't satisfy (e.g. a
///    field whose type isn't `Clone` when `#[derive(Clone)]` is asked).
/// 3. Field-shape regressions on the public-facing struct API
///    (extra/missing/renamed fields would fail the struct-literal
///    construction).
#[test]
fn scimantic_question_can_be_constructed_in_downstream_crate() {
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

/// Re-running the writer over the same schema produces byte-identical
/// output. Idempotency falls out for free from sorted `BTreeMap`
/// iteration in `render()`, but it's worth asserting against a real
/// schema in case a `HashMap` ever sneaks into the writer or the IR.
#[test]
fn scimantic_renders_idempotently() {
    let schema = read_scimantic();
    let writer = RustWriter::new();
    let first = writer.render(&schema);
    let second = writer.render(&schema);
    assert_eq!(
        first, second,
        "writer output should be deterministic across runs"
    );
}

/// Build a self-contained Cargo project around the generated module.
/// `src/scimantic.rs` holds the generated code; `src/lib.rs` exercises
/// the public API by constructing a `Question` value, ensuring the
/// generated struct's fields and visibility round-trip through a
/// downstream consumer.
fn write_scratch_crate(root: &Path, generated_module_body: &str) {
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
    std::fs::write(root.join("src/scimantic.rs"), generated_module_body)
        .expect("write scimantic.rs");
    std::fs::write(root.join("src/lib.rs"), CONSUMER_SMOKE).expect("write lib.rs");
}

/// Consumer-side smoke that uses the generated types. Lives inside the
/// test crate rather than the generated module so the writer's output
/// stays purely schema-derived. If this fails to compile, the writer
/// emitted a struct shape that no downstream consumer can actually use.
const CONSUMER_SMOKE: &str = r#"
#![allow(dead_code, unused_variables)]

pub mod scimantic;

/// Constructs a `Question` value via struct literal. Every reachable
/// field must be named explicitly — extra fields would error
/// "unknown field" and missing fields would error "missing struct
/// field". The test asserts only that this function compiles; it
/// isn't called.
pub fn make_a_question() -> scimantic::Question {
    scimantic::Question {
        // `label` is `required: true` on the global slot, so it's
        // emitted as bare `String`, not `Option<String>`.
        label: "Why is the sky blue?".to_string(),
        was_generated_by: None,
        was_derived_from: vec![],
        was_attributed_to: None,
        motivates: vec![],
    }
}

/// Construct via `Default::default()`. Every field of `Question` is
/// either `Option<T>`, `Vec<T>`, or a required primitive that's itself
/// `Default` (`String`), so the writer's conservative Default analysis
/// should derive `Default` on `Question`. If this fails to compile,
/// the analysis incorrectly declined to derive `Default`.
pub fn make_default_question() -> scimantic::Question {
    scimantic::Question::default()
}

/// Two equal `Question` values compare equal under the derived
/// `PartialEq`. Forces the writer to actually emit `PartialEq` on the
/// struct, not just claim it in the doc comment.
pub fn questions_compare_equal() -> bool {
    let a = make_a_question();
    let b = make_a_question();
    a == b
}

/// Construct a `Question` via the generated `new` constructor: pass
/// only required fields, expect optional / multivalued ones to default.
/// If the writer adds a new optional field to the schema, this call
/// site continues to compile — the schema-evolution-stable contract
/// the slice-6.9 constructor exists for.
pub fn make_question_via_constructor() -> scimantic::Question {
    let q = scimantic::Question::new("Why is the sky blue?".to_string());
    assert!(q.was_generated_by.is_none());
    assert!(q.was_derived_from.is_empty());
    q
}
"#;
