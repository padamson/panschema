//! `panschema-publish.toml` — the schema-side publishing standard.
//!
//! Schema repositories include this file at their root to declare what they
//! publish: name, version, the LinkML spec version they target, and which
//! files contain the schema. panschema reads it during `fetch` to know what
//! to pull from the repo.
//!
//! Reference: [`docs/features/05-schema-manager.md`](../../docs/features/05-schema-manager.md)

use std::path::{Path, PathBuf};
use std::process::Command;
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
    #[error(
        "the following git refs failed to resolve in `{}`: {}",
        repo_root.display(),
        refs.join(", ")
    )]
    RefsUnresolvable {
        repo_root: PathBuf,
        refs: Vec<String>,
    },
    #[error("`git show {ref_}:{path}` failed in `{repo_root}`: {stderr}")]
    ExtractFailed {
        repo_root: String,
        ref_: String,
        path: String,
        stderr: String,
    },
    #[error("`git` not found on PATH — required for versioned publish")]
    GitNotFound,
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

/// Resolve a list of git refs in `repo_root` via `git rev-parse`,
/// returning the resolved commit IDs in the same order as the input.
/// On any failure, collects *every* unresolved ref into a single
/// [`PublishError::RefsUnresolvable`] rather than failing fast — the
/// caller usually wants to know the full damage before retrying.
///
/// Uses `--verify` plus `^{commit}` to force resolution to a commit
/// object specifically (catches the case where a name resolves but
/// points at a tag object or tree rather than a commit).
pub fn resolve_refs(repo_root: &Path, refs: &[&str]) -> Result<Vec<String>, PublishError> {
    let mut resolved = Vec::with_capacity(refs.len());
    let mut failed: Vec<String> = Vec::new();
    for r in refs {
        let arg = format!("{r}^{{commit}}");
        match run_git_capture(repo_root, &["rev-parse", "--verify", "--quiet", &arg]) {
            Ok(out) => resolved.push(out.trim().to_string()),
            Err(_) => failed.push((*r).to_string()),
        }
    }
    if !failed.is_empty() {
        return Err(PublishError::RefsUnresolvable {
            repo_root: repo_root.to_path_buf(),
            refs: failed,
        });
    }
    Ok(resolved)
}

/// Extract the contents of `path_in_repo` at git ref `ref_` into a
/// fresh [`tempfile::NamedTempFile`]. Uses `git show <ref>:<path>` so
/// the working tree stays exactly as the user left it.
///
/// `path_in_repo` is interpreted relative to the repo root; pass the
/// publish-spec's `files.main` value here when the spec lives at the
/// repo root (the typical case).
///
/// Returns a [`PublishError::ExtractFailed`] when the file doesn't
/// exist at that ref or `git show` fails for any reason; the stderr
/// captured in the error gives the caller the underlying cause.
pub fn extract_main_at_ref(
    repo_root: &Path,
    ref_: &str,
    path_in_repo: &Path,
) -> Result<tempfile::NamedTempFile, PublishError> {
    let spec = format!("{ref_}:{}", path_in_repo.display());
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["show", &spec])
        .output()
        .map_err(classify_git_spawn_error)?;
    if !output.status.success() {
        return Err(PublishError::ExtractFailed {
            repo_root: repo_root.display().to_string(),
            ref_: ref_.to_string(),
            path: path_in_repo.display().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    let extension = path_in_repo
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("dat");
    let mut file = tempfile::Builder::new()
        .prefix("panschema-extract-")
        .suffix(&format!(".{extension}"))
        .tempfile()
        .map_err(PublishError::Io)?;
    use std::io::Write;
    file.write_all(&output.stdout).map_err(PublishError::Io)?;
    Ok(file)
}

/// Translate a failure from spawning `git` into the right
/// [`PublishError`] variant: `ErrorKind::NotFound` means `git` isn't
/// installed (actionable hint), anything else is a generic IO error.
/// Extracted into its own function so `#[mutants::skip]` can suppress
/// the boundary check — there's no portable test for "is `git` on
/// PATH right now" without mutating the test runner's environment.
#[mutants::skip]
fn classify_git_spawn_error(e: std::io::Error) -> PublishError {
    if e.kind() == std::io::ErrorKind::NotFound {
        PublishError::GitNotFound
    } else {
        PublishError::Io(e)
    }
}

/// Run `git <args>` in `repo_root`, returning captured stdout on
/// success or a generic `io::Error` carrying stderr otherwise.
fn run_git_capture(repo_root: &Path, args: &[&str]) -> Result<String, std::io::Error> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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

    // ----- resolve_refs / extract_main_at_ref (slice 2) -----

    /// Build a synthetic git repo with two committed tags + an extra
    /// HEAD commit on `main`. Each commit rewrites `schema.yaml` with
    /// a per-version marker line so extraction can be verified
    /// byte-exactly.
    fn make_versioned_fixture_repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();

        // Init repo with deterministic identity so commits hash stably
        // across runs (not strictly required for these tests, but
        // avoids depending on the runner's git config).
        run(path, &["init", "--initial-branch=main", "--quiet"]);
        run(path, &["config", "user.email", "test@example.com"]);
        run(path, &["config", "user.name", "Test"]);
        run(path, &["config", "commit.gpgsign", "false"]);

        for (version, marker) in [("v0.1.0", "v0.1.0"), ("v0.2.0", "v0.2.0")] {
            std::fs::write(path.join("schema.yaml"), format!("version: {marker}\n")).unwrap();
            run(path, &["add", "schema.yaml"]);
            run(
                path,
                &["commit", "-m", &format!("release {version}"), "--quiet"],
            );
            run(path, &["tag", version]);
        }
        // Move main beyond v0.2.0 so HEAD differs from any tag.
        std::fs::write(path.join("schema.yaml"), "version: 0.3.0-dev\n").unwrap();
        run(path, &["add", "schema.yaml"]);
        run(path, &["commit", "-m", "WIP", "--quiet"]);

        tmp
    }

    fn run(cwd: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .status()
            .expect("git available on PATH");
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn resolve_refs_returns_commits_in_input_order() {
        let repo = make_versioned_fixture_repo();
        let resolved =
            resolve_refs(repo.path(), &["v0.1.0", "v0.2.0", "main"]).expect("all refs resolve");
        assert_eq!(resolved.len(), 3);
        // Each entry is a 40-char hex commit ID and all three are distinct.
        for sha in &resolved {
            assert_eq!(sha.len(), 40);
            assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
        }
        assert!(resolved[0] != resolved[1]);
        assert!(resolved[1] != resolved[2]);
    }

    #[test]
    fn resolve_refs_surfaces_combined_error_for_unresolved() {
        let repo = make_versioned_fixture_repo();
        // Mix one good, one bad, one good. The error must list the
        // bad one but the good ones must NOT short-circuit the loop
        // (the AC: surface combined error for any failures).
        let err = resolve_refs(repo.path(), &["v0.1.0", "v9.9.9", "main"]).expect_err("bad ref");
        match err {
            PublishError::RefsUnresolvable { ref refs, .. } => {
                assert_eq!(refs.len(), 1);
                assert_eq!(refs[0], "v9.9.9");
            }
            other => panic!("expected RefsUnresolvable, got {other:?}"),
        }
    }

    #[test]
    fn resolve_refs_combines_multiple_failures_in_one_error() {
        let repo = make_versioned_fixture_repo();
        let err = resolve_refs(repo.path(), &["nope1", "v0.1.0", "nope2"]).expect_err("bad refs");
        match err {
            PublishError::RefsUnresolvable { refs, .. } => {
                // Both bad refs in the error, in input order.
                assert_eq!(refs, vec!["nope1".to_string(), "nope2".to_string()]);
            }
            other => panic!("expected RefsUnresolvable, got {other:?}"),
        }
    }

    #[test]
    fn extract_main_at_ref_returns_per_version_contents() {
        let repo = make_versioned_fixture_repo();
        for (ref_, expected_marker) in [("v0.1.0", "v0.1.0"), ("v0.2.0", "v0.2.0")] {
            let file = extract_main_at_ref(repo.path(), ref_, Path::new("schema.yaml")).unwrap();
            let contents = std::fs::read_to_string(file.path()).unwrap();
            assert_eq!(contents, format!("version: {expected_marker}\n"));
        }
    }

    #[test]
    fn extract_main_at_ref_reads_main_branch_separately_from_tags() {
        // HEAD on `main` carries the v0.3.0-dev marker, distinct from
        // either of the committed tags. The extraction must read each
        // ref's content at that ref's snapshot, not the working tree.
        let repo = make_versioned_fixture_repo();
        let file = extract_main_at_ref(repo.path(), "main", Path::new("schema.yaml")).unwrap();
        let contents = std::fs::read_to_string(file.path()).unwrap();
        assert_eq!(contents, "version: 0.3.0-dev\n");
    }

    #[test]
    fn extract_main_at_ref_errors_for_unknown_ref() {
        let repo = make_versioned_fixture_repo();
        let err = extract_main_at_ref(repo.path(), "v9.9.9", Path::new("schema.yaml"))
            .expect_err("unknown ref");
        match err {
            PublishError::ExtractFailed { ref_, path, .. } => {
                assert_eq!(ref_, "v9.9.9");
                assert_eq!(path, "schema.yaml");
            }
            other => panic!("expected ExtractFailed, got {other:?}"),
        }
    }

    #[test]
    fn extract_main_at_ref_errors_for_unknown_path_at_ref() {
        // The ref exists, but the path doesn't exist at that ref.
        // Common failure mode: the manifest's `files.main` was added
        // *after* the tag we're trying to extract from.
        let repo = make_versioned_fixture_repo();
        let err = extract_main_at_ref(repo.path(), "v0.1.0", Path::new("missing/file.yaml"))
            .expect_err("missing path");
        assert!(matches!(err, PublishError::ExtractFailed { .. }));
    }

    #[test]
    fn extract_main_at_ref_does_not_mutate_working_tree() {
        // Critical contract: the user's working tree stays as they
        // left it. We change a file in the working tree, extract a
        // *different* version, and assert the working-tree file
        // wasn't touched.
        let repo = make_versioned_fixture_repo();
        let working_tree_file = repo.path().join("schema.yaml");
        let before = std::fs::read_to_string(&working_tree_file).unwrap();
        // Set the working tree to a unique marker.
        std::fs::write(&working_tree_file, "version: wt-marker\n").unwrap();

        let _file = extract_main_at_ref(repo.path(), "v0.1.0", Path::new("schema.yaml")).unwrap();

        let after = std::fs::read_to_string(&working_tree_file).unwrap();
        assert_eq!(after, "version: wt-marker\n");
        // Sanity check that the test is exercising what we think.
        assert_ne!(after, before);
    }
}
