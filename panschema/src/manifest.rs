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

/// One entry under `[schemas]` — declares where a schema lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaDep {
    /// Path to the schema file (or directory containing the publish spec).
    /// Resolved relative to the manifest's location.
    pub path: PathBuf,
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
my-local = { path = "./schema/my-schema.yaml" }
"#;
        let m = toml.parse::<Manifest>().expect("should parse");
        assert_eq!(m.schemas.len(), 1);
        assert_eq!(
            m.schemas.get("my-local").unwrap().path,
            PathBuf::from("./schema/my-schema.yaml")
        );
        assert!(m.generate.is_empty());
    }

    #[test]
    fn parses_manifest_with_generate_section() {
        let toml = r#"
[schemas]
my-local = { path = "./schema/my-schema.yaml" }

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
a = { path = "./a.yaml" }
b = { path = "./b.yaml" }
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
my-local = { path = "./x.yaml", version = "0.1.0" }
"#;
        let err = toml.parse::<Manifest>().expect_err("should reject");
        assert!(matches!(err, ManifestError::Parse(_)));
    }

    #[test]
    fn errors_on_unknown_writer_in_generate() {
        let toml = r#"
[schemas]
x = { path = "./x.yaml" }

[generate.x]
rust = "src/generated/x.rs"
"#;
        let err = toml.parse::<Manifest>().expect_err("should reject");
        assert!(matches!(err, ManifestError::Parse(_)));
    }

    #[test]
    fn errors_on_missing_path_in_schema_dep() {
        let toml = r#"
[schemas]
x = {}
"#;
        let err = toml.parse::<Manifest>().expect_err("should reject");
        assert!(matches!(err, ManifestError::Parse(_)));
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
x = { path = "./x.yaml" }
"#,
        )
        .expect("write");
        let m = Manifest::from_path(tmp.path()).expect("read");
        assert!(m.schemas.contains_key("x"));
    }
}
