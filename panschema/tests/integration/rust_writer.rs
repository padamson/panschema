//! End-to-end integration tests for the Rust codegen writer.
//!
//! The default-run backstop is the checked-in `tests/fixtures/codegen.yaml`
//! schema: it exercises every codegen construct and is rendered, parsed,
//! and compiled in a scratch crate with no network access. Any codegen
//! branch that regresses fails this test at `cargo build` time rather
//! than slipping through a text assertion.
//!
//! The `scimantic_*` tests provide real-world dogfood against the
//! scimantic-schema v0.1.0 LinkML schema. The schema is a frozen vendored
//! snapshot checked in at `tests/fixtures/dogfood/scimantic-schema/v0.1.0.yaml`,
//! so these tests also run by default with no network access. Broader
//! render/compile coverage across every vendored dogfood release lives in
//! `tests/dogfood.rs`.

use std::path::{Path, PathBuf};
use std::process::Command;

use panschema::io::Reader;
use panschema::linkml::SchemaDefinition;
use panschema::rust_writer::RustWriter;
use panschema::yaml_reader::YamlReader;

/// Read the checked-in self-contained codegen fixture through the same
/// reader path the CLI uses. No network: the schema lives in-tree at
/// `tests/fixtures/codegen.yaml` and has no `imports` or external refs.
fn read_codegen_fixture() -> SchemaDefinition {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("codegen.yaml");
    YamlReader::new()
        .read(&path)
        .expect("parse local codegen fixture")
}

/// Read the frozen vendored scimantic-schema v0.1.0 snapshot through the
/// same reader path the CLI uses. No network: the schema lives in-tree at
/// `tests/fixtures/dogfood/scimantic-schema/v0.1.0.yaml`.
fn read_scimantic() -> SchemaDefinition {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("dogfood")
        .join("scimantic-schema")
        .join("v0.1.0.yaml");
    YamlReader::new()
        .read(&path)
        .expect("parse vendored scimantic schema")
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
// Local-fixture compile backstop (default run, no network)
// ---------------------------------------------------------------------------

/// The self-contained `codegen.yaml` fixture renders to Rust that parses,
/// compiles in a downstream crate, and behaves correctly under a serde
/// JSON round-trip. This is the compiled backstop for the writer: every
/// codegen construct in the fixture is exercised by `cargo build`, so a
/// regression in any of them (keyword escaping, `ifabsent` defaults,
/// `Box` recursion, `any_of` unions, derive selection, …) fails here
/// rather than slipping past a text assertion.
///
/// The consumer source (`CODEGEN_CONSUMER`) constructs instances using
/// the keyword-escaped fields (`r#type`, `r#move`) and the
/// `ifabsent`-defaulted field (`status`), then round-trips through JSON:
/// an absent `status` must deserialize to its `ItemStatus::planned`
/// default, and the keyword permissible value `virtual` must serialize
/// back to its original wire name `"virtual"`.
#[test]
fn codegen_fixture_compiles_and_round_trips_in_downstream_crate() {
    let schema = read_codegen_fixture();
    let body = RustWriter::new().render(&schema);

    syn::parse_file(&body).unwrap_or_else(|e| {
        let preview = body.chars().take(2000).collect::<String>();
        panic!("generated Rust failed to parse: {e}\n--- preview (first 2k chars) ---\n{preview}")
    });

    let tmp = tempfile::tempdir().expect("tempdir for codegen scratch crate");
    write_codegen_scratch_crate(tmp.path(), &body);
    cargo_run_scratch(tmp.path());
}

/// The codegen fixture renders deliberately non-canonical layout, so
/// `rustfmt --check` passing proves the in-file skip pragma — not luck —
/// keeps generated code stable. Skipped when `rustfmt` is absent.
#[test]
fn rustfmt_leaves_generated_code_untouched() {
    let Some(rustfmt) = rustfmt_bin() else {
        eprintln!("rustfmt not found on this host; skipping formatter-skip check");
        return;
    };
    let body = RustWriter::new().render(&read_codegen_fixture());
    assert!(
        body.contains("#![cfg_attr(rustfmt, rustfmt_skip)]"),
        "render must emit the file-level rustfmt skip for this check to mean anything"
    );

    let tmp = tempfile::tempdir().expect("tempdir for rustfmt check");
    let file = tmp.path().join("generated.rs");
    std::fs::write(&file, &body).expect("write generated file");

    let status = Command::new(&rustfmt)
        .args(["--edition", "2021", "--check"])
        .arg(&file)
        .status()
        .expect("invoke rustfmt --check");
    assert!(
        status.success(),
        "rustfmt --check must report no changes for skip-pragma'd generated code (exit {:?})",
        status.code()
    );
}

/// Locate `rustfmt` via `rustup which`, falling back to `PATH`; `None`
/// when neither resolves.
fn rustfmt_bin() -> Option<PathBuf> {
    if let Ok(out) = Command::new("rustup").args(["which", "rustfmt"]).output()
        && out.status.success()
    {
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    Command::new("rustfmt")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| PathBuf::from("rustfmt"))
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
    cargo_build_scratch(tmp.path());
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

/// Invoke `cargo build --quiet` on a scratch crate rooted at `root`.
///
/// `CARGO_TARGET_DIR` is overridden so the scratch crate's build
/// artifacts land inside `CARGO_TARGET_TMPDIR` rather than polluting the
/// workspace target dir. `CARGO_HOME` is inherited so registry downloads
/// (serde, chrono) are cached across runs.
fn cargo_build_scratch(root: &Path) {
    let status = Command::new("cargo")
        .args(["build", "--quiet"])
        .current_dir(root)
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

/// Invoke `cargo run --quiet` on a scratch binary crate rooted at `root`,
/// asserting the program exits successfully. Used when the consumer's
/// runtime assertions (serde round-trips) must actually execute, not just
/// type-check. Shares the out-of-tree target dir with [`cargo_build_scratch`].
fn cargo_run_scratch(root: &Path) {
    let status = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(root)
        .env(
            "CARGO_TARGET_DIR",
            PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("scratch-crate-target"),
        )
        .status()
        .expect("invoke `cargo run` on scratch crate");
    assert!(
        status.success(),
        "scratch crate's runtime assertions failed (or it didn't compile)"
    );
}

/// Build a self-contained binary Cargo project around the codegen
/// fixture's generated module. `src/codegen.rs` holds the generated
/// code; `src/main.rs` exercises the public API and serde round-trips at
/// runtime (see [`CODEGEN_CONSUMER`]). The crate is a binary so
/// [`cargo_run_scratch`] can execute the assertions rather than merely
/// compile them.
fn write_codegen_scratch_crate(root: &Path, generated_module_body: &str) {
    std::fs::write(
        root.join("Cargo.toml"),
        r#"[package]
name = "codegen-fixture-test"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
"#,
    )
    .expect("write Cargo.toml");
    std::fs::create_dir_all(root.join("src")).expect("mkdir src/");
    std::fs::write(root.join("src/codegen.rs"), generated_module_body).expect("write codegen.rs");
    std::fs::write(root.join("src/main.rs"), CODEGEN_CONSUMER).expect("write main.rs");
}

/// Consumer-side program that constructs and round-trips the generated
/// codegen-fixture types. Lives outside the generated module so the
/// writer's output stays purely schema-derived. Asserts at runtime so a
/// behavioral codegen regression (wrong `ifabsent` default, dropped
/// `#[serde(rename)]` on a keyword variant/field) fails the test even
/// though the code still compiles.
const CODEGEN_CONSUMER: &str = r##"
#![allow(dead_code, unused_variables)]

mod codegen;

fn main() {
    // Constructor: required `label` only; optional/multivalued fields
    // default, and the `ifabsent` field initializes to its enum default.
    let q = codegen::Question::new("Why is the sky blue?".to_string());
    assert!(q.was_generated_by.is_none(), "optional class field defaults to None");
    assert!(q.was_derived_from.is_empty(), "multivalued field defaults to empty");
    assert_eq!(q.status, codegen::ItemStatus::planned, "ifabsent default applied by ctor");

    // Keyword-escaped fields construct via their raw idents and rename to
    // the original wire names under serde.
    let kw = codegen::Keyworded {
        id: "k1".to_string(),
        r#type: Some("scalar".to_string()),
        r#move: Some("castle".to_string()),
        r#virtual: None,
        noted: None,
    };
    let kw_json = serde_json::to_string(&kw).expect("serialize Keyworded");
    assert!(kw_json.contains("\"type\""), "field `r#type` renames to wire `type`");
    assert!(kw_json.contains("\"move\""), "field `r#move` renames to wire `move`");
    let kw_back: codegen::Keyworded =
        serde_json::from_str(&kw_json).expect("deserialize Keyworded");
    assert_eq!(kw_back.r#type.as_deref(), Some("scalar"), "keyword field round-trips");

    // ifabsent default: an absent `status` deserializes to the default.
    let q_default: codegen::Question =
        serde_json::from_str(r#"{"label":"x"}"#).expect("deserialize Question without status");
    assert_eq!(
        q_default.status,
        codegen::ItemStatus::planned,
        "absent `status` deserializes to the `ifabsent` enum default"
    );

    // Scalar ifabsent defaults: each field renders as the bare Rust type
    // and is constructed here so every scalar default is actually compiled.
    let cfg = codegen::ServiceConfig {
        port: 9090,
        ratio: 0.5,
        scale: 4.0,
        prefix: "db".to_string(),
        enabled: false,
        verbose: true,
    };
    let cfg_json = serde_json::to_string(&cfg).expect("serialize ServiceConfig");
    let _cfg_back: codegen::ServiceConfig =
        serde_json::from_str(&cfg_json).expect("ServiceConfig round-trips");
    // An empty object deserializes every field to its `ifabsent` literal.
    let cfg_default: codegen::ServiceConfig =
        serde_json::from_str("{}").expect("deserialize ServiceConfig with no fields");
    assert_eq!(cfg_default.port, 8080, "int ifabsent default");
    assert_eq!(cfg_default.ratio, 1.0, "float ifabsent default");
    assert_eq!(cfg_default.scale, 2.0, "whole-number double ifabsent default");
    assert_eq!(cfg_default.prefix, "svc", "string ifabsent default");
    assert!(cfg_default.enabled, "boolean true ifabsent default");
    assert!(!cfg_default.verbose, "boolean false ifabsent default");

    // Keyword permissible value serializes back to its original wire name.
    let v = codegen::ItemStatus::r#virtual;
    assert_eq!(
        serde_json::to_string(&v).expect("serialize variant"),
        "\"virtual\"",
        "keyword enum variant renames to wire `virtual`"
    );

    // Space-sanitized permissible value round-trips through its wire name.
    let dirty: codegen::ItemStatus =
        serde_json::from_str("\"in progress\"").expect("deserialize dirty variant");
    assert_eq!(
        dirty,
        codegen::ItemStatus::in_progress,
        "`in progress` wire value maps to the sanitized variant"
    );

    // any_of union: a `wasDerivedFrom` entry round-trips untagged.
    let mut q2 = codegen::Question::new("derived".to_string());
    q2.was_derived_from
        .push(codegen::QuestionWasDerivedFrom::Question(Box::new(
            codegen::Question::new("source".to_string()),
        )));
    let q2_json = serde_json::to_string(&q2).expect("serialize Question with union");
    let q2_back: codegen::Question =
        serde_json::from_str(&q2_json).expect("deserialize Question with union");
    assert_eq!(q2_back.was_derived_from.len(), 1, "any_of union round-trips");

    // Keyword-named type: the class `move` is `pub struct r#move` and the
    // enum `type` is `pub enum r#type`. Constructing them here proves the
    // type-name escaping is consistent across definition and references.
    let m = codegen::r#move {
        caption: "castle short".to_string(),
        kind: Some(codegen::r#type::tactical),
    };
    let m_json = serde_json::to_string(&m).expect("serialize keyword-named type");
    let m_back: codegen::r#move =
        serde_json::from_str(&m_json).expect("deserialize keyword-named type");
    assert_eq!(m_back.caption, "castle short", "keyword-named struct round-trips");

    // The keyword-named class is referenced as a field range on
    // `Keyworded.noted` -> `Option<Box<r#move>>`. Construct through that
    // reference site to prove the escaped ident matches the definition.
    let kw2 = codegen::Keyworded {
        id: "k2".to_string(),
        r#type: None,
        r#move: None,
        r#virtual: None,
        noted: Some(Box::new(m)),
    };
    assert!(kw2.noted.is_some(), "keyword-named class usable as a field range");
}
"##;

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
