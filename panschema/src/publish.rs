//! `panschema-publish.toml` — the schema-side publishing standard.
//!
//! Schema repositories include this file at their root to declare what they
//! publish: name, version, the LinkML spec version they target, and which
//! files contain the schema. panschema reads it during `fetch` to know what
//! to pull from the repo.
//!
//! Reference: [`docs/features/05-schema-manager.md`](../../docs/features/05-schema-manager.md)

use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Standard filename for the schema-side publishing standard.
pub const PUBLISH_FILENAME: &str = "panschema-publish.toml";

/// Parse error specific to `panschema-publish.toml`.
#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error("failed to read publish spec: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid publish spec: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("`{}` already exists in `{}` (pass `--force` to overwrite)", PUBLISH_FILENAME, dir.display())]
    AlreadyExists { dir: PathBuf },
}

/// Top-level structure of `panschema-publish.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishConfig {
    pub schema: SchemaInfo,
    pub files: FileMapping,
}

/// `[schema]` table — identity and versioning metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaInfo {
    /// Schema package name (matches the dict key in consumer `[schemas]`).
    pub name: String,
    /// Schema version (matches the git tag for `github:` sources, modulo `v` prefix).
    pub version: String,
    /// LinkML spec version this schema targets.
    pub linkml: String,
}

/// `[files]` table — where the schema's content lives within the repo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMapping {
    /// Path to the main schema file, relative to the publish-spec's location.
    pub main: PathBuf,
}

impl FromStr for PublishConfig {
    type Err = PublishError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(toml::from_str(s)?)
    }
}

impl PublishConfig {
    /// Parse a `panschema-publish.toml` from disk.
    pub fn from_path(path: &Path) -> Result<Self, PublishError> {
        let content = std::fs::read_to_string(path)?;
        content.parse()
    }
}

/// Create a `panschema-publish.toml` at `dir/panschema-publish.toml`.
///
/// Used by `panschema init`. Writes a hand-formatted TOML body (stable
/// key order, light blank-line layout) rather than serializing
/// [`PublishConfig`] — the round-trip would lose layout we care about
/// for a user-facing config file. Refuses to overwrite an existing
/// file unless `force` is `true`.
///
/// Returns the absolute path the file was written to.
pub fn init_publish_file(
    dir: &Path,
    name: &str,
    version: &str,
    main: &Path,
    linkml: &str,
    force: bool,
) -> Result<PathBuf, PublishError> {
    let target = dir.join(PUBLISH_FILENAME);
    if target.exists() && !force {
        return Err(PublishError::AlreadyExists {
            dir: dir.to_path_buf(),
        });
    }

    let body = format!(
        r#"[schema]
name = "{name}"
version = "{version}"
linkml = "{linkml}"

[files]
main = "{main}"
"#,
        main = main.display()
    );

    std::fs::write(&target, body)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_valid_publish_spec() {
        let toml = r#"
[schema]
name = "scimantic-schema"
version = "0.1.3"
linkml = "1.7.0"

[files]
main = "schema/scimantic.yaml"
"#;
        let cfg = toml.parse::<PublishConfig>().expect("should parse");
        assert_eq!(cfg.schema.name, "scimantic-schema");
        assert_eq!(cfg.schema.version, "0.1.3");
        assert_eq!(cfg.schema.linkml, "1.7.0");
        assert_eq!(cfg.files.main, PathBuf::from("schema/scimantic.yaml"));
    }

    #[test]
    fn errors_on_missing_required_field() {
        // No `linkml` in [schema].
        let toml = r#"
[schema]
name = "x"
version = "0.1.0"

[files]
main = "x.yaml"
"#;
        let err = toml.parse::<PublishConfig>().expect_err("should reject");
        let msg = err.to_string();
        assert!(
            msg.contains("linkml") || msg.contains("missing"),
            "error should mention the missing field; got: {msg}"
        );
    }

    #[test]
    fn errors_on_invalid_toml() {
        let err = "not = valid = toml"
            .parse::<PublishConfig>()
            .expect_err("should reject");
        assert!(matches!(err, PublishError::Parse(_)));
    }

    #[test]
    fn errors_on_missing_files_section() {
        let toml = r#"
[schema]
name = "x"
version = "0.1.0"
linkml = "1.7.0"
"#;
        let err = toml.parse::<PublishConfig>().expect_err("should reject");
        assert!(matches!(err, PublishError::Parse(_)));
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
[schema]
name = "x"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "x.yaml"
"#,
        )
        .expect("write");
        let cfg = PublishConfig::from_path(tmp.path()).expect("read");
        assert_eq!(cfg.schema.name, "x");
    }

    // ----- init_publish_file -----

    #[test]
    fn init_writes_a_round_trippable_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = init_publish_file(
            tmp.path(),
            "demo",
            "0.1.0",
            Path::new("schema.yaml"),
            "1.7.0",
            false,
        )
        .unwrap();
        assert_eq!(path, tmp.path().join(PUBLISH_FILENAME));
        let cfg = PublishConfig::from_path(&path).unwrap();
        assert_eq!(cfg.schema.name, "demo");
        assert_eq!(cfg.schema.version, "0.1.0");
        assert_eq!(cfg.schema.linkml, "1.7.0");
        assert_eq!(cfg.files.main, PathBuf::from("schema.yaml"));
    }

    #[test]
    fn init_refuses_to_clobber_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        init_publish_file(
            tmp.path(),
            "first",
            "0.1.0",
            Path::new("a.yaml"),
            "1.7.0",
            false,
        )
        .unwrap();
        let err = init_publish_file(
            tmp.path(),
            "second",
            "0.2.0",
            Path::new("b.yaml"),
            "1.7.0",
            false,
        )
        .unwrap_err();
        assert!(matches!(err, PublishError::AlreadyExists { .. }));

        // First file's contents must be unchanged.
        let cfg = PublishConfig::from_path(&tmp.path().join(PUBLISH_FILENAME)).unwrap();
        assert_eq!(cfg.schema.name, "first");
    }

    #[test]
    fn init_force_overwrites() {
        let tmp = tempfile::tempdir().unwrap();
        init_publish_file(
            tmp.path(),
            "first",
            "0.1.0",
            Path::new("a.yaml"),
            "1.7.0",
            false,
        )
        .unwrap();
        init_publish_file(
            tmp.path(),
            "second",
            "0.2.0",
            Path::new("b.yaml"),
            "1.7.0",
            true,
        )
        .unwrap();
        let cfg = PublishConfig::from_path(&tmp.path().join(PUBLISH_FILENAME)).unwrap();
        assert_eq!(cfg.schema.name, "second");
        assert_eq!(cfg.schema.version, "0.2.0");
    }

    #[test]
    fn init_writes_stable_key_order() {
        // The key order is part of the user-facing layout — schema fields
        // before files, name/version/linkml in that order. We exercise this
        // by checking the line layout rather than the parsed form.
        let tmp = tempfile::tempdir().unwrap();
        let path = init_publish_file(
            tmp.path(),
            "x",
            "0.1.0",
            Path::new("x.yaml"),
            "1.7.0",
            false,
        )
        .unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        let schema_pos = body.find("[schema]").unwrap();
        let name_pos = body.find("name").unwrap();
        let version_pos = body.find("version").unwrap();
        let linkml_pos = body.find("linkml").unwrap();
        let files_pos = body.find("[files]").unwrap();
        assert!(
            schema_pos < name_pos
                && name_pos < version_pos
                && version_pos < linkml_pos
                && linkml_pos < files_pos
        );
    }
}
