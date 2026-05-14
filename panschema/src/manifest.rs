//! `panschema.toml` — the consumer-side schema manifest.
//!
//! Consumer projects place this file at their root to declare schema
//! dependencies and per-schema codegen configuration. It is the equivalent
//! of `Cargo.toml`'s `[dependencies]` for LinkML schemas.
//!
//! Reference: [`docs/features/05-schema-manager.md`](../../docs/features/05-schema-manager.md)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Filename used for the manifest. Cargo-style: walked up from CWD.
pub const MANIFEST_FILENAME: &str = "panschema.toml";

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("failed to read manifest: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid manifest: {0}")]
    Parse(#[from] toml::de::Error),
}

/// Top-level structure of `panschema.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Schema dependencies, keyed by schema name.
    #[serde(default)]
    pub schemas: BTreeMap<String, SchemaDep>,
    /// Per-schema codegen configuration, keyed by the same name as `schemas`.
    #[serde(default)]
    pub generate: BTreeMap<String, GenerateConfig>,
}

/// One entry under `[schemas]` — declares where a schema package lives.
///
/// Every schema dependency is a "package" consisting of a
/// `panschema-publish.toml` plus the main schema file it references at
/// `[files].main`. Two source shapes (semantic validation lives in
/// [`crate::source::SchemaSource::from_dep`], not in serde):
/// - `path:` source: `path = "./local-pkg"` — directory on disk
/// - `github:` source: `source = "github:owner/repo"` + `version = "0.1.3"`
///   — extracted from `codeload.github.com` tarball into the shared cache
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SchemaDep {
    /// Path to the schema file, resolved relative to the manifest's location.
    /// Required for `path:` sources; absent for remote sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Source specifier (e.g. `"github:owner/repo"`).
    /// Required for remote sources; absent for `path:` sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Version (tag for `github:` sources, modulo a leading `v`).
    /// Required when `source` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// One entry under `[generate.<name>]` — maps writer kinds to output paths.
/// Each field corresponds to a writer; absence means that writer isn't run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct GenerateConfig {
    /// HTML documentation output directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<PathBuf>,
}

impl FromStr for Manifest {
    type Err = ManifestError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(toml::from_str(s)?)
    }
}

impl Manifest {
    /// Parse a manifest from disk.
    pub fn from_path(path: &Path) -> Result<Self, ManifestError> {
        let content = std::fs::read_to_string(path)?;
        content.parse()
    }
}

/// A parsed positional spec for `panschema add` and (eventually)
/// `remove` / `update`.
///
/// Two shapes:
/// - `Source { uri, version }` — `github:owner/repo@0.1.3`. The
///   `@<version>` suffix is mandatory in v0.3 (HEAD tracking is
///   deferred to v0.4).
/// - `Path` — anything else; assumed to be a filesystem path to a
///   package directory (containing `panschema-publish.toml`).
///
/// Implements [`FromStr`] so clap parses the positional CLI arg
/// natively — malformed specs surface as parse errors before any
/// handler runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaSpec {
    /// Remote source URI with version, e.g. `github:owner/repo@0.1.3`.
    Source { uri: String, version: String },
    /// Local filesystem path to a package (file or directory).
    Path(PathBuf),
}

/// Errors raised while parsing a [`SchemaSpec`] from a CLI string.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SpecError {
    #[error("empty schema spec")]
    Empty,
    #[error("remote source `{uri}` requires a version: use `<uri>@<version>`")]
    SourceWithoutVersion { uri: String },
    #[error("empty version after `@` in spec `{0}`")]
    EmptyVersion(String),
    #[error(
        "unrecognized source protocol in spec `{0}`; \
         v0.3 supports `github:owner/repo@version` or a local filesystem path"
    )]
    UnknownProtocol(String),
}

/// Recognized source-URI protocols for [`SchemaSpec::Source`].
///
/// The detector distinguishes a URI-shaped spec from a filesystem
/// path. Keeping the list explicit (not "anything with a colon")
/// avoids accidentally parsing Windows-style `C:/...` paths as URIs.
const SOURCE_PROTOCOLS: &[&str] = &["github:"];

impl FromStr for SchemaSpec {
    type Err = SpecError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(SpecError::Empty);
        }

        // URI form: <protocol>:<rest>[@<version>]
        if SOURCE_PROTOCOLS.iter().any(|p| s.starts_with(p)) {
            return match s.rsplit_once('@') {
                None => Err(SpecError::SourceWithoutVersion { uri: s.to_string() }),
                Some((_, "")) => Err(SpecError::EmptyVersion(s.to_string())),
                Some((uri, version)) => Ok(Self::Source {
                    uri: uri.to_string(),
                    version: version.to_string(),
                }),
            };
        }

        // Detect ambiguous specs that aren't paths but aren't recognised
        // protocols either — e.g. `unknown:foo`. Anything containing `:`
        // before the first `/` or `.` is treated as a protocol claim.
        if let Some(colon) = s.find(':') {
            let prefix = &s[..=colon];
            let first_sep = s.find(['/', '.', '\\']).unwrap_or(s.len());
            if colon < first_sep && !SOURCE_PROTOCOLS.contains(&prefix) {
                return Err(SpecError::UnknownProtocol(s.to_string()));
            }
        }

        Ok(Self::Path(PathBuf::from(s)))
    }
}

/// A validated "where does this new schema come from?" request, ready
/// to write into `panschema.toml`. Construct via [`AddRequest::from_cli`]
/// — by then the name has been inferred from `panschema-publish.toml`
/// (or supplied as an alias via `--name`), so both variants carry the
/// final manifest key.
///
/// The two variants mirror [`crate::source::SchemaSource`] but keep
/// the source URI string verbatim so it round-trips into the manifest
/// unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddRequest {
    /// Local path source. `path` is the *directory* containing
    /// `panschema-publish.toml`, already relativized to the manifest
    /// directory.
    Path { name: String, path: PathBuf },
    /// Remote source. `source` is the verbatim URI
    /// (e.g. `github:owner/repo`).
    Remote {
        name: String,
        source: String,
        version: String,
    },
}

impl AddRequest {
    /// The schema name. Same for both variants.
    pub fn name(&self) -> &str {
        match self {
            Self::Path { name, .. } | Self::Remote { name, .. } => name,
        }
    }
}

/// Outcome of [`insert_schema`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddOutcome {
    /// A new schema entry was inserted into `[schemas]`.
    Inserted,
    /// An entry already existed with the same shape — nothing was written.
    AlreadyPresent,
}

/// Errors raised by [`insert_schema`].
#[derive(Debug, thiserror::Error)]
pub enum AddError {
    #[error("manifest I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("manifest parse: {0}")]
    Parse(#[from] toml_edit::TomlError),
    #[error(
        "schema `{name}` is already in the manifest with a different version (`{existing}`); \
         `panschema update` would change it, but that command is not yet implemented \
         (v0.3 ships `add` only)"
    )]
    VersionMismatch { name: String, existing: String },
    #[error(
        "schema `{name}` is already in the manifest with a different source (`{existing}`); \
         remove it first if you really want to swap sources"
    )]
    SourceMismatch { name: String, existing: String },
}

/// Insert a new `[schemas.<name>]` entry into `panschema.toml`.
///
/// Uses `toml_edit` so comments, key order, and whitespace in the rest
/// of the manifest survive the edit. If the schema already exists with
/// the same shape, returns [`AddOutcome::AlreadyPresent`] without
/// touching the file. Conflicting version or source raises a structured
/// error rather than silently overwriting.
///
/// Only `[schemas]` is touched. `[generate.<name>]` is owned by the
/// `generate` command — consumers opt in by writing their own writer
/// keys (e.g. `html = "docs/"`) when they want codegen. `generate`
/// itself prints a clear "no [generate.<name>] block; skipping" hint
/// for any schema without one.
pub fn insert_schema(manifest_path: &Path, request: &AddRequest) -> Result<AddOutcome, AddError> {
    use toml_edit::{DocumentMut, InlineTable, Item, Table, value};

    let name = request.name();
    let content = std::fs::read_to_string(manifest_path)?;
    let mut doc: DocumentMut = content.parse()?;

    // Ensure `[schemas]` exists as a table.
    if doc.get("schemas").is_none() {
        doc["schemas"] = Item::Table(Table::new());
    }
    let schemas = doc["schemas"]
        .as_table_mut()
        .expect("schemas should be a table");

    // Idempotency / conflict detection.
    if let Some(existing) = schemas.get(name) {
        let existing_inline = existing.as_inline_table();
        let existing_table = existing.as_table();

        let read = |k: &str| -> Option<String> {
            let from_inline = existing_inline.and_then(|t| t.get(k));
            let from_table = existing_table
                .and_then(|t| t.get(k))
                .and_then(|i| i.as_value());
            from_inline
                .or(from_table)
                .and_then(|v| v.as_str())
                .map(str::to_string)
        };

        let existing_path = read("path");
        let existing_source = read("source");
        let existing_version = read("version");

        let same_shape = match request {
            AddRequest::Path { path: want, .. } => {
                existing_path.as_deref() == Some(want.to_str().unwrap_or(""))
                    && existing_source.is_none()
            }
            AddRequest::Remote {
                source: want_source,
                version: want_version,
                ..
            } => {
                existing_source.as_deref() == Some(want_source.as_str())
                    && existing_version.as_deref() == Some(want_version.as_str())
            }
        };
        if same_shape {
            return Ok(AddOutcome::AlreadyPresent);
        }

        // Surface the most informative mismatch.
        if let AddRequest::Remote {
            source: want_source,
            version: want_version,
            ..
        } = request
            && existing_source.as_deref() == Some(want_source.as_str())
            && let Some(existing_v) = &existing_version
            && existing_v != want_version
        {
            return Err(AddError::VersionMismatch {
                name: name.to_string(),
                existing: existing_v.clone(),
            });
        }
        let existing_spec = existing_source
            .clone()
            .or_else(|| existing_path.clone().map(|p| format!("path:{p}")))
            .unwrap_or_else(|| "<unknown>".to_string());
        return Err(AddError::SourceMismatch {
            name: name.to_string(),
            existing: existing_spec,
        });
    }

    // Build the inline-table representation for the new entry.
    let mut entry = InlineTable::new();
    match request {
        AddRequest::Path { path, .. } => {
            entry.insert("path", path.to_string_lossy().into_owned().into());
        }
        AddRequest::Remote {
            source, version, ..
        } => {
            entry.insert("source", source.as_str().into());
            entry.insert("version", version.as_str().into());
        }
    }
    schemas.insert(name, value(entry));

    std::fs::write(manifest_path, doc.to_string())?;
    Ok(AddOutcome::Inserted)
}

/// Find a `panschema.toml` by walking up from `start_dir`. Returns the
/// absolute path to the manifest file, or `None` if no manifest is found
/// at any ancestor (mirrors cargo's manifest discovery).
pub fn discover_manifest(start_dir: &Path) -> Option<PathBuf> {
    let mut dir = if start_dir.is_absolute() {
        start_dir.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(start_dir)
    };
    loop {
        let candidate = dir.join(MANIFEST_FILENAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_manifest_one_schema_no_generate() {
        let toml = r#"
[schemas]
my-local = { path = "./local-pkg" }
"#;
        let m = toml.parse::<Manifest>().expect("should parse");
        assert_eq!(m.schemas.len(), 1);
        assert_eq!(
            m.schemas.get("my-local").unwrap().path.as_deref(),
            Some(Path::new("./local-pkg"))
        );
        assert!(m.generate.is_empty());
    }

    #[test]
    fn parses_manifest_with_generate_section() {
        let toml = r#"
[schemas]
my-local = { path = "./local-pkg" }

[generate.my-local]
html = "docs/"
"#;
        let m = toml.parse::<Manifest>().expect("should parse");
        assert_eq!(
            m.generate.get("my-local").unwrap().html,
            Some(PathBuf::from("docs/"))
        );
    }

    #[test]
    fn parses_multiple_schemas() {
        let toml = r#"
[schemas]
a = { path = "./a-pkg" }
b = { path = "./b-pkg" }
"#;
        let m = toml.parse::<Manifest>().expect("should parse");
        assert_eq!(m.schemas.len(), 2);
        assert!(m.schemas.contains_key("a"));
        assert!(m.schemas.contains_key("b"));
    }

    #[test]
    fn empty_manifest_is_valid() {
        let m = "".parse::<Manifest>().expect("empty manifest should parse");
        assert!(m.schemas.is_empty());
        assert!(m.generate.is_empty());
    }

    #[test]
    fn errors_on_unknown_top_level_key() {
        let toml = r#"
[unrecognized]
foo = "bar"
"#;
        let err = toml.parse::<Manifest>().expect_err("should reject");
        assert!(matches!(err, ManifestError::Parse(_)));
    }

    #[test]
    fn errors_on_unknown_schema_field() {
        let toml = r#"
[schemas]
my-local = { path = "./local-pkg", colour = "blue" }
"#;
        let err = toml.parse::<Manifest>().expect_err("should reject");
        assert!(matches!(err, ManifestError::Parse(_)));
    }

    #[test]
    fn parses_github_source_with_version() {
        let toml = r#"
[schemas]
remote = { source = "github:padamson/scimantic-schema", version = "0.1.3" }
"#;
        let m = toml.parse::<Manifest>().expect("should parse");
        let dep = m.schemas.get("remote").unwrap();
        assert!(dep.path.is_none());
        assert_eq!(
            dep.source.as_deref(),
            Some("github:padamson/scimantic-schema")
        );
        assert_eq!(dep.version.as_deref(), Some("0.1.3"));
    }

    #[test]
    fn errors_on_unknown_writer_in_generate() {
        let toml = r#"
[schemas]
x = { path = "./x-pkg" }

[generate.x]
rust = "src/generated/x.rs"
"#;
        let err = toml.parse::<Manifest>().expect_err("should reject");
        assert!(matches!(err, ManifestError::Parse(_)));
    }

    #[test]
    fn empty_schema_dep_parses_but_is_semantically_invalid() {
        // Serde accepts the empty table now that all fields are optional.
        // Semantic validation (must have either `path` or `source`+`version`)
        // lives in the `source` module.
        let toml = r#"
[schemas]
x = {}
"#;
        let m = toml
            .parse::<Manifest>()
            .expect("should parse at serde layer");
        let dep = m.schemas.get("x").unwrap();
        assert!(dep.path.is_none());
        assert!(dep.source.is_none());
        assert!(dep.version.is_none());
    }

    #[test]
    fn discover_finds_manifest_in_start_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let manifest_path = tmp.path().join(MANIFEST_FILENAME);
        std::fs::write(&manifest_path, "").expect("write");

        let found = discover_manifest(tmp.path()).expect("should find manifest");
        // Canonicalize both: macOS /tmp resolves to /private/tmp.
        let found = found.canonicalize().unwrap();
        let expected = manifest_path.canonicalize().unwrap();
        assert_eq!(found, expected);
    }

    #[test]
    fn discover_walks_up_to_ancestor_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let manifest_path = tmp.path().join(MANIFEST_FILENAME);
        std::fs::write(&manifest_path, "").expect("write");

        // Manifest is at tmp/, search starts at tmp/sub/sub2/.
        let nested = tmp.path().join("sub").join("sub2");
        std::fs::create_dir_all(&nested).expect("mkdir");

        let found = discover_manifest(&nested)
            .expect("should walk up and find manifest")
            .canonicalize()
            .unwrap();
        assert_eq!(found, manifest_path.canonicalize().unwrap());
    }

    // -----------------------------------------------------------------
    // SchemaSpec parsing
    // -----------------------------------------------------------------

    #[test]
    fn spec_parses_github_with_version() {
        let s: SchemaSpec = "github:padamson/scimantic-schema@0.1.3".parse().unwrap();
        assert_eq!(
            s,
            SchemaSpec::Source {
                uri: "github:padamson/scimantic-schema".to_string(),
                version: "0.1.3".to_string(),
            }
        );
    }

    #[test]
    fn spec_parses_local_dir_path() {
        let s: SchemaSpec = "./local-pkg".parse().unwrap();
        assert_eq!(s, SchemaSpec::Path(PathBuf::from("./local-pkg")));
    }

    #[test]
    fn spec_parses_bare_name_as_path() {
        // A bare name like `foo` (no `:`, `/`, `.`) is just a relative path.
        let s: SchemaSpec = "foo".parse().unwrap();
        assert_eq!(s, SchemaSpec::Path(PathBuf::from("foo")));
    }

    #[test]
    fn spec_rejects_empty_string() {
        assert_eq!("".parse::<SchemaSpec>().unwrap_err(), SpecError::Empty);
    }

    #[test]
    fn spec_rejects_github_without_version() {
        let e = "github:padamson/scimantic-schema"
            .parse::<SchemaSpec>()
            .unwrap_err();
        assert!(matches!(e, SpecError::SourceWithoutVersion { .. }));
    }

    #[test]
    fn spec_rejects_github_with_empty_version() {
        let e = "github:padamson/scimantic-schema@"
            .parse::<SchemaSpec>()
            .unwrap_err();
        assert!(matches!(e, SpecError::EmptyVersion(_)));
    }

    #[test]
    fn spec_rejects_unknown_protocol() {
        let e = "gitlab:foo/bar@0.1.0".parse::<SchemaSpec>().unwrap_err();
        assert!(matches!(e, SpecError::UnknownProtocol(_)));
    }

    #[test]
    fn spec_handles_windows_style_path_without_misparsing_protocol() {
        // `C:/foo` has a colon BEFORE the path separator, but `C:` isn't
        // a recognised protocol — so it should fall through to UnknownProtocol
        // rather than be silently treated as a source URI.
        let e = "C:foo".parse::<SchemaSpec>().unwrap_err();
        assert!(matches!(e, SpecError::UnknownProtocol(_)));
    }

    // -----------------------------------------------------------------
    // insert_schema (Slice 4)
    // -----------------------------------------------------------------

    fn write_manifest(dir: &std::path::Path, body: &str) -> PathBuf {
        let p = dir.join(MANIFEST_FILENAME);
        std::fs::write(&p, body).unwrap();
        p
    }

    fn remote_req(name: &str, source: &str, version: &str) -> AddRequest {
        AddRequest::Remote {
            name: name.to_string(),
            source: source.to_string(),
            version: version.to_string(),
        }
    }

    fn path_req(name: &str, p: &str) -> AddRequest {
        AddRequest::Path {
            name: name.to_string(),
            path: PathBuf::from(p),
        }
    }

    #[test]
    fn insert_schema_adds_github_entry_without_generate_block() {
        let tmp = tempfile::tempdir().unwrap();
        let m = write_manifest(tmp.path(), "[schemas]\n");
        let outcome = insert_schema(
            &m,
            &remote_req(
                "scimantic-schema",
                "github:padamson/scimantic-schema",
                "0.1.3",
            ),
        )
        .unwrap();
        assert_eq!(outcome, AddOutcome::Inserted);
        let after = std::fs::read_to_string(&m).unwrap();
        assert!(after.contains("scimantic-schema"));
        assert!(after.contains(r#"source = "github:padamson/scimantic-schema""#));
        assert!(after.contains(r#"version = "0.1.3""#));
        // `[generate.<name>]` is owned by the user, not auto-written by add.
        assert!(
            !after.contains("[generate.scimantic-schema]"),
            "add must not write a starter [generate.<name>] block: {after}"
        );
    }

    #[test]
    fn insert_schema_adds_path_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let m = write_manifest(tmp.path(), "[schemas]\n");
        let outcome = insert_schema(&m, &path_req("local", "./schema/local")).unwrap();
        assert_eq!(outcome, AddOutcome::Inserted);
        let after = std::fs::read_to_string(&m).unwrap();
        assert!(after.contains(r#"path = "./schema/local""#));
        assert!(!after.contains("[generate.local]"));
    }

    #[test]
    fn insert_schema_is_idempotent_for_same_shape() {
        let tmp = tempfile::tempdir().unwrap();
        let m = write_manifest(tmp.path(), "[schemas]\n");
        insert_schema(&m, &remote_req("x", "github:a/b", "0.1.0")).unwrap();
        let before = std::fs::read_to_string(&m).unwrap();
        let outcome = insert_schema(&m, &remote_req("x", "github:a/b", "0.1.0")).unwrap();
        assert_eq!(outcome, AddOutcome::AlreadyPresent);
        let after = std::fs::read_to_string(&m).unwrap();
        assert_eq!(before, after, "idempotent call must not rewrite the file");
    }

    #[test]
    fn insert_schema_rejects_version_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let m = write_manifest(tmp.path(), "[schemas]\n");
        insert_schema(&m, &remote_req("x", "github:a/b", "0.1.0")).unwrap();
        let err = insert_schema(&m, &remote_req("x", "github:a/b", "0.2.0")).unwrap_err();
        assert!(matches!(err, AddError::VersionMismatch { .. }));
    }

    #[test]
    fn insert_schema_rejects_source_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let m = write_manifest(tmp.path(), "[schemas]\n");
        insert_schema(&m, &remote_req("x", "github:a/b", "0.1.0")).unwrap();
        let err = insert_schema(&m, &remote_req("x", "github:c/d", "0.1.0")).unwrap_err();
        assert!(matches!(err, AddError::SourceMismatch { .. }));
    }

    #[test]
    fn insert_schema_preserves_existing_comments_and_other_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let body = r#"# top-level comment
[schemas]
# already-there
existing = { path = "./e" }
"#;
        let m = write_manifest(tmp.path(), body);
        insert_schema(&m, &path_req("newone", "./n")).unwrap();
        let after = std::fs::read_to_string(&m).unwrap();
        assert!(after.contains("# top-level comment"));
        assert!(after.contains("# already-there"));
        assert!(after.contains("existing"));
        assert!(after.contains("newone"));
    }

    #[test]
    fn discover_returns_none_when_no_manifest_anywhere() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // No panschema.toml anywhere under tmp.
        let nested = tmp.path().join("a").join("b");
        std::fs::create_dir_all(&nested).expect("mkdir");
        // Discovery walks up to filesystem root; we can't guarantee no
        // panschema.toml exists higher up on the dev machine, so just verify
        // discovery didn't error or return something inside the tempdir.
        let result = discover_manifest(&nested);
        if let Some(path) = result {
            assert!(
                !path.starts_with(tmp.path().canonicalize().unwrap()),
                "no manifest should be found inside the tempdir"
            );
        }
    }

    #[test]
    fn from_path_reads_disk() {
        let mut tmp = tempfile::Builder::new()
            .suffix(".toml")
            .tempfile()
            .expect("temp file");
        use std::io::Write;
        tmp.write_all(
            br#"
[schemas]
x = { path = "./x-pkg" }
"#,
        )
        .expect("write");
        let m = Manifest::from_path(tmp.path()).expect("read");
        assert!(m.schemas.contains_key("x"));
    }
}
