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
    #[error("malformed publish spec (toml_edit): {0}")]
    Edit(#[from] toml_edit::TomlError),
    #[error("`[schema].version` is missing or not a string in the publish file")]
    MissingVersionField,
    #[error("`{value}` is not a valid semver version")]
    InvalidVersion { value: String },
    #[error(
        "[publishing].current = `{current}` must appear in [publishing].versions = {versions:?} or equal [publishing].edge = {edge:?}"
    )]
    InvalidCurrent {
        current: String,
        versions: Vec<String>,
        edge: Option<String>,
    },
}

/// Which component of a semver version to bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BumpLevel {
    Patch,
    Minor,
    Major,
}

/// Top-level structure of `panschema-publish.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishConfig {
    pub schema: SchemaInfo,
    pub files: FileMapping,
    /// Optional multi-version doc-publish orchestration config. Absent
    /// for single-version schemas; presence enables `panschema publish`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publishing: Option<PublishingConfig>,
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

/// `[publishing]` table — multi-version doc orchestration config. Drives
/// `panschema publish`: which git refs to build, where they land on disk,
/// and which version the `current/` alias points to. Defaults are chosen
/// so a minimal block (`versions = [...]`, `current = "..."`) works
/// out-of-the-box.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishingConfig {
    /// Git tag names whose docs should be built. Each must resolve via
    /// `git rev-parse` (validated at extraction time, not parse time).
    #[serde(default)]
    pub versions: Vec<String>,
    /// Optional ref (branch or commit-ish) whose HEAD is also built.
    /// `None` means skip the edge build. Typical value: `"main"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<String>,
    /// Alias target — `current/` mirrors this version's output. Must be
    /// in `versions` OR equal `edge` (validated at parse time).
    pub current: String,
    /// URL template for cross-version links. `{version}` placeholder.
    #[serde(default = "default_url_pattern")]
    pub url_pattern: String,
    /// Where per-version subdirs land, relative to repo root.
    #[serde(default = "default_output_dir")]
    pub output_dir: PathBuf,
    /// Output format — reserved for future writer fan-out.
    #[serde(default = "default_format")]
    pub format: String,
}

fn default_url_pattern() -> String {
    "/schema/{version}/".to_string()
}

fn default_output_dir() -> PathBuf {
    PathBuf::from("site/schema")
}

fn default_format() -> String {
    "html".to_string()
}

impl PublishingConfig {
    /// Validate cross-field invariants that pure serde can't express.
    /// Currently: `current` must appear in `versions` or equal `edge`.
    fn validate(&self) -> Result<(), PublishError> {
        let in_versions = self.versions.iter().any(|v| v == &self.current);
        let matches_edge = self.edge.as_deref() == Some(self.current.as_str());
        if !in_versions && !matches_edge {
            return Err(PublishError::InvalidCurrent {
                current: self.current.clone(),
                versions: self.versions.clone(),
                edge: self.edge.clone(),
            });
        }
        Ok(())
    }
}

impl FromStr for PublishConfig {
    type Err = PublishError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let cfg: PublishConfig = toml::from_str(s)?;
        if let Some(publishing) = &cfg.publishing {
            publishing.validate()?;
        }
        Ok(cfg)
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

/// Bump `[schema].version` in the publish file at `path` per `level` and
/// write the result back. Preserves comments and key order via `toml_edit`.
///
/// Returns `(old, new)` version strings.
pub fn bump_version(path: &Path, level: BumpLevel) -> Result<(String, String), PublishError> {
    use semver::Version;
    use toml_edit::DocumentMut;

    let content = std::fs::read_to_string(path)?;
    let mut doc: DocumentMut = content.parse()?;

    let old_str = doc
        .get("schema")
        .and_then(|s| s.get("version"))
        .and_then(|v| v.as_str())
        .ok_or(PublishError::MissingVersionField)?
        .to_string();

    let mut v = Version::parse(&old_str).map_err(|_| PublishError::InvalidVersion {
        value: old_str.clone(),
    })?;

    match level {
        BumpLevel::Patch => v.patch += 1,
        BumpLevel::Minor => {
            v.minor += 1;
            v.patch = 0;
        }
        BumpLevel::Major => {
            v.major += 1;
            v.minor = 0;
            v.patch = 0;
        }
    }
    // Drop any pre-release / build metadata on bump — we're cutting a stable release.
    v.pre = semver::Prerelease::EMPTY;
    v.build = semver::BuildMetadata::EMPTY;
    let new_str = v.to_string();

    doc["schema"]["version"] = toml_edit::value(new_str.as_str());
    std::fs::write(path, doc.to_string())?;

    Ok((old_str, new_str))
}

/// Set `[schema].version` to an exact value (parsed as semver). Returns
/// the previous version string. Preserves comments + key order.
pub fn set_version(path: &Path, new: &str) -> Result<String, PublishError> {
    use semver::Version;
    use toml_edit::DocumentMut;

    // Validate up-front so we don't write garbage.
    Version::parse(new).map_err(|_| PublishError::InvalidVersion {
        value: new.to_string(),
    })?;

    let content = std::fs::read_to_string(path)?;
    let mut doc: DocumentMut = content.parse()?;

    let old_str = doc
        .get("schema")
        .and_then(|s| s.get("version"))
        .and_then(|v| v.as_str())
        .ok_or(PublishError::MissingVersionField)?
        .to_string();

    doc["schema"]["version"] = toml_edit::value(new);
    std::fs::write(path, doc.to_string())?;

    Ok(old_str)
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

    // ----- bump_version / set_version -----

    fn pkg_with_version(dir: &std::path::Path, version: &str) -> std::path::PathBuf {
        init_publish_file(dir, "x", version, Path::new("schema.yaml"), "1.7.0", false).unwrap()
    }

    #[test]
    fn bump_patch_increments_z() {
        let tmp = tempfile::tempdir().unwrap();
        let path = pkg_with_version(tmp.path(), "0.1.3");
        let (old, new) = bump_version(&path, BumpLevel::Patch).unwrap();
        assert_eq!(old, "0.1.3");
        assert_eq!(new, "0.1.4");
        let cfg = PublishConfig::from_path(&path).unwrap();
        assert_eq!(cfg.schema.version, "0.1.4");
    }

    #[test]
    fn bump_minor_increments_y_and_resets_z() {
        let tmp = tempfile::tempdir().unwrap();
        let path = pkg_with_version(tmp.path(), "0.1.3");
        let (_, new) = bump_version(&path, BumpLevel::Minor).unwrap();
        assert_eq!(new, "0.2.0");
    }

    #[test]
    fn bump_major_from_pre_1_0_goes_to_1_0_0() {
        let tmp = tempfile::tempdir().unwrap();
        let path = pkg_with_version(tmp.path(), "0.5.7");
        let (_, new) = bump_version(&path, BumpLevel::Major).unwrap();
        assert_eq!(new, "1.0.0");
    }

    #[test]
    fn bump_drops_pre_release_suffix() {
        let tmp = tempfile::tempdir().unwrap();
        let path = pkg_with_version(tmp.path(), "0.2.0-rc1");
        let (_, new) = bump_version(&path, BumpLevel::Patch).unwrap();
        // 0.2.0-rc1 + patch → 0.2.1 (rc suffix dropped on bump).
        assert_eq!(new, "0.2.1");
    }

    #[test]
    fn bump_preserves_comments_and_other_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(PUBLISH_FILENAME);
        std::fs::write(
            &path,
            r#"# top-level comment
[schema]
name = "x"
# version comment
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"
"#,
        )
        .unwrap();
        bump_version(&path, BumpLevel::Minor).unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("# top-level comment"));
        assert!(after.contains("# version comment"));
        assert!(after.contains(r#"version = "0.2.0""#));
        assert!(after.contains(r#"name = "x""#));
        assert!(after.contains(r#"linkml = "1.7.0""#));
    }

    #[test]
    fn bump_errors_when_version_field_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(PUBLISH_FILENAME);
        std::fs::write(
            &path,
            "[schema]\nname = \"x\"\nlinkml = \"1.7.0\"\n[files]\nmain = \"s.yaml\"\n",
        )
        .unwrap();
        let err = bump_version(&path, BumpLevel::Patch).unwrap_err();
        assert!(matches!(err, PublishError::MissingVersionField));
    }

    #[test]
    fn bump_errors_on_non_semver_version() {
        let tmp = tempfile::tempdir().unwrap();
        let path = pkg_with_version(tmp.path(), "not-a-version");
        let err = bump_version(&path, BumpLevel::Patch).unwrap_err();
        assert!(matches!(err, PublishError::InvalidVersion { .. }));
    }

    #[test]
    fn set_version_overrides_existing_value() {
        let tmp = tempfile::tempdir().unwrap();
        let path = pkg_with_version(tmp.path(), "0.1.0");
        let old = set_version(&path, "1.2.3").unwrap();
        assert_eq!(old, "0.1.0");
        let cfg = PublishConfig::from_path(&path).unwrap();
        assert_eq!(cfg.schema.version, "1.2.3");
    }

    #[test]
    fn set_version_rejects_invalid_semver() {
        let tmp = tempfile::tempdir().unwrap();
        let path = pkg_with_version(tmp.path(), "0.1.0");
        let err = set_version(&path, "not-semver").unwrap_err();
        assert!(matches!(err, PublishError::InvalidVersion { .. }));
        // File must be unchanged.
        let cfg = PublishConfig::from_path(&path).unwrap();
        assert_eq!(cfg.schema.version, "0.1.0");
    }

    // ----- [publishing] section -----

    #[test]
    fn parses_publish_spec_without_publishing_section() {
        // Absent `[publishing]` means single-version generation — the
        // pre-feature-11 behavior. Must continue to work.
        let toml = r#"
[schema]
name = "x"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"
"#;
        let cfg: PublishConfig = toml.parse().expect("should parse");
        assert!(cfg.publishing.is_none());
    }

    #[test]
    fn parses_minimal_publishing_block_with_defaults() {
        // Minimal block: just `versions` + `current`. Optional fields
        // (`edge`, `url_pattern`, `output_dir`, `format`) come from
        // their serde defaults.
        let toml = r#"
[schema]
name = "x"
version = "0.2.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[publishing]
versions = ["v0.1.0", "v0.2.0"]
current = "v0.2.0"
"#;
        let cfg: PublishConfig = toml.parse().expect("should parse");
        let publishing = cfg.publishing.expect("publishing should be present");
        assert_eq!(publishing.versions, vec!["v0.1.0", "v0.2.0"]);
        assert_eq!(publishing.current, "v0.2.0");
        assert!(publishing.edge.is_none());
        assert_eq!(publishing.url_pattern, "/schema/{version}/");
        assert_eq!(publishing.output_dir, PathBuf::from("site/schema"));
        assert_eq!(publishing.format, "html");
    }

    #[test]
    fn parses_full_publishing_block_with_overrides() {
        // Every optional field overridden. Round-trips through serde
        // without losing values.
        let toml = r#"
[schema]
name = "x"
version = "0.3.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[publishing]
versions = ["v0.1.0", "v0.2.0"]
edge = "main"
current = "main"
url_pattern = "/docs/{version}/"
output_dir = "build/site"
format = "html"
"#;
        let cfg: PublishConfig = toml.parse().expect("should parse");
        let publishing = cfg.publishing.expect("publishing should be present");
        assert_eq!(publishing.edge.as_deref(), Some("main"));
        assert_eq!(publishing.current, "main");
        assert_eq!(publishing.url_pattern, "/docs/{version}/");
        assert_eq!(publishing.output_dir, PathBuf::from("build/site"));
    }

    #[test]
    fn accepts_current_matching_edge_even_when_not_in_versions() {
        // The validation rule: `current` is OK if it matches `edge`,
        // even when not listed in `versions`. Useful for "publish only
        // edge" setups.
        let toml = r#"
[schema]
name = "x"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[publishing]
versions = []
edge = "main"
current = "main"
"#;
        toml.parse::<PublishConfig>().expect("should parse");
    }

    #[test]
    fn rejects_current_not_in_versions_and_not_equal_edge() {
        // `current = "v9.9.9"` is neither in `versions` nor `== edge`.
        // Parse must fail at parse time with InvalidCurrent.
        let toml = r#"
[schema]
name = "x"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[publishing]
versions = ["v0.1.0", "v0.2.0"]
edge = "main"
current = "v9.9.9"
"#;
        let err = toml
            .parse::<PublishConfig>()
            .expect_err("should reject invalid current");
        assert!(
            matches!(err, PublishError::InvalidCurrent { ref current, .. } if current == "v9.9.9"),
            "expected InvalidCurrent with current=v9.9.9; got {err:?}"
        );
        // Error message should be actionable — name the offending field
        // and what it can be.
        let msg = err.to_string();
        assert!(msg.contains("current"));
        assert!(msg.contains("v9.9.9"));
        assert!(msg.contains("versions"));
    }

    #[test]
    fn rejects_current_when_versions_empty_and_no_edge() {
        // Empty versions + no edge means there's nothing `current` could
        // legitimately match. Reject rather than silently produce an
        // unusable manifest.
        let toml = r#"
[schema]
name = "x"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[publishing]
versions = []
current = "v0.1.0"
"#;
        let err = toml
            .parse::<PublishConfig>()
            .expect_err("should reject when current has nothing to match");
        assert!(matches!(err, PublishError::InvalidCurrent { .. }));
    }

    #[test]
    fn rejects_missing_current_field() {
        // `current` is required when `[publishing]` is present —
        // there's no sensible default.
        let toml = r#"
[schema]
name = "x"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[publishing]
versions = ["v0.1.0"]
"#;
        let err = toml
            .parse::<PublishConfig>()
            .expect_err("should reject missing current");
        // serde gives a generic Parse error pointing at the missing field.
        assert!(matches!(err, PublishError::Parse(_)));
        assert!(err.to_string().contains("current"));
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
