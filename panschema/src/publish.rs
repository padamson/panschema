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
    #[error("at most one `[[instances]]` entry may set `exemplar = true`; found: {names}")]
    MultipleExemplars { names: String },

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
    #[error(
        "`panschema publish` requires a [publishing] section in {}",
        PUBLISH_FILENAME
    )]
    MissingPublishingSection,
    #[error("failed to generate docs for version `{version}`: {message}")]
    GenerateFailed { version: String, message: String },
    #[error(
        "[publishing.pages.{dep}] configures a page, but no [[instances]] entry publishes to it (none has schema = \"{dep}\")"
    )]
    PageWithoutEntries { dep: String },
    #[error("could not read the repo's panschema.toml manifest: {message}")]
    ManifestUnreadable { message: String },
    #[error("[[instances]] entry `{entry}` publishes to dependency `{dep}`, but {reason}")]
    UnknownDependency {
        entry: String,
        dep: String,
        reason: String,
    },
    #[error("`{value}` cannot be used as {what}: {problem}")]
    InvalidSegment {
        what: String,
        value: String,
        problem: String,
    },
}

/// Whether a configured value may become one directory name in the
/// publish output tree: non-empty, no path separators, and not the
/// `.`/`..` traversal names. Version labels, the edge label, and page
/// directories all pass through this — one rule for every path segment
/// the publish tree writes.
fn is_single_path_segment(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && !s.contains(['/', '\\'])
}

/// The full gate a value passes before it may become one fresh
/// directory name in the publish output tree: a single path segment,
/// and not the reserved `current`. Version labels, the edge label, and
/// every page directory — configured or defaulted — go through here.
fn require_fresh_segment(what: &str, value: &str) -> Result<(), PublishError> {
    if !is_single_path_segment(value) {
        return Err(PublishError::InvalidSegment {
            what: what.to_string(),
            value: value.to_string(),
            problem: "it must be a single path segment (no `/`, `.`, or `..`)".to_string(),
        });
    }
    if value == "current" {
        return Err(PublishError::InvalidSegment {
            what: what.to_string(),
            value: value.to_string(),
            problem: "`current` is reserved for the alias directory".to_string(),
        });
    }
    Ok(())
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
    /// Per-prefix overrides for the upstream-label source URL,
    /// keyed by prefix name. Entries win over the built-in map.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub label_sources: std::collections::BTreeMap<String, String>,
    /// Optional mdbook→schema cross-link config, consumed by the
    /// `mdbook-panschema install` command. Absent means the feature is
    /// off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub book_link: Option<BookLinkConfig>,
    /// Zero-or-more instance graphs published alongside the schema
    /// (`[[instances]]`). The one marked `exemplar` embeds in the schema
    /// page (ADR-009); each version renders its own ref's data file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instances: Vec<InstanceEntry>,
}

/// One `[[instances]]` entry: a named instance-data file published with
/// the schema. Unknown keys are rejected so a typo'd setting fails
/// loudly instead of silently reverting to its default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceEntry {
    /// Dataset identity (the directory name once sibling instance pages
    /// exist).
    pub name: String,
    /// Path to the LinkML instance-data file, relative to the publish
    /// spec's location.
    pub data: PathBuf,
    /// Embed this dataset in the schema page as the exemplar. At most
    /// one entry may set it.
    #[serde(default)]
    pub exemplar: bool,
    /// Names a `[schemas.<dep>]` dependency from the repo's manifest.
    /// The dataset then publishes on that dependency's page instead of
    /// the own-schema page; entries naming the same dependency share
    /// one page. `None` keeps the dataset on the own-schema page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
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

/// `[book_link]` — mdbook→schema cross-link config. Read by
/// `mdbook-panschema install` to bake the schema-docs location and
/// button label into the toolbar asset it drops into an mdbook book.
/// The reverse direction of `[publishing].site_root_url`.
///
/// Two spellings parse, because every book that exists today writes the
/// first one:
///
/// - `[book_link]` — one target, with an `enabled` switch. Unchanged.
/// - `[[book_link]]` — one entry per schema the book fronts. Writing an
///   entry is itself the opt-in, so there is no `enabled` to set; an
///   empty list means off.
///
/// Consumers read [`BookLinkConfig::entries`] and
/// [`BookLinkConfig::enabled`] rather than matching on the shape, so the
/// difference stops at the parse boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum BookLinkConfig {
    /// `[[book_link]]` — one entry per schema.
    List(Vec<BookLinkEntry>),
    /// `[book_link]` — the single-target table form.
    Single(BookLinkTable),
}

/// Dispatch on the written shape — an array is the list form, anything
/// else the table form — rather than deriving `untagged`.
///
/// Two reasons, both about being wrong loudly rather than quietly. Serde
/// can build a struct from a *sequence*, and every field of
/// [`BookLinkTable`] has a default, so an untagged derive matches
/// `book_link = []` against `Single` and yields one link out of an empty
/// list. And an untagged derive reports only "data did not match any
/// variant" — a typo'd key in an entry would tell the author nothing about
/// which key, where the table form has always named it.
impl<'de> Deserialize<'de> for BookLinkConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;
        let value = toml::Value::deserialize(deserializer)?;
        match value {
            toml::Value::Array(_) => Vec::<BookLinkEntry>::deserialize(value)
                .map(BookLinkConfig::List)
                .map_err(D::Error::custom),
            other => BookLinkTable::deserialize(other)
                .map(BookLinkConfig::Single)
                .map_err(D::Error::custom),
        }
    }
}

impl BookLinkConfig {
    /// Every configured link, in declaration order — what was written,
    /// independent of whether the feature is switched on.
    pub fn entries(&self) -> Vec<BookLinkEntry> {
        match self {
            BookLinkConfig::Single(t) => vec![BookLinkEntry {
                schema_path: t.schema_path.clone(),
                label: t.label.clone(),
            }],
            BookLinkConfig::List(entries) => entries.clone(),
        }
    }

    /// Whether `install` should do anything.
    ///
    /// The table form has an explicit switch, because a bare `[book_link]`
    /// header must not turn the button on by itself. The list form has
    /// none: writing an entry is the opt-in, so an empty list is the only
    /// way to be off.
    pub fn enabled(&self) -> bool {
        match self {
            BookLinkConfig::Single(t) => t.enabled,
            BookLinkConfig::List(entries) => !entries.is_empty(),
        }
    }
}

/// The `[book_link]` table form: one target plus a master switch.
///
/// Unknown keys are rejected so a typo'd setting fails loudly instead
/// of silently reverting to its default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BookLinkTable {
    /// Master switch — `install` is a no-op unless this is `true`.
    #[serde(default)]
    pub enabled: bool,
    /// Book-relative path to the schema docs the button links to.
    #[serde(default = "default_book_link_schema_path")]
    pub schema_path: String,
    /// Button aria-label / tooltip / prose text.
    #[serde(default = "default_book_link_label")]
    pub label: String,
}

/// One `[[book_link]]` entry: a schema-docs page the book links out to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BookLinkEntry {
    /// Book-relative path to this schema's docs.
    #[serde(default = "default_book_link_schema_path")]
    pub schema_path: String,
    /// The entry's label — the button's tooltip when it is the only one,
    /// and its name in the drop-down when it is not.
    #[serde(default = "default_book_link_label")]
    pub label: String,
}

fn default_book_link_schema_path() -> String {
    "schema/current/".to_string()
}

fn default_book_link_label() -> String {
    "Schema reference".to_string()
}

/// `[publishing]` table — multi-version doc orchestration config. Drives
/// `panschema publish`: which git refs to build, where they land on disk,
/// and which version the `current/` alias points to. Defaults are chosen
/// so a minimal block (`versions = [...]`, `current = "..."`) works
/// out-of-the-box.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// URL template for cross-version links. `{version}` placeholder is
    /// substituted with the target version's ref name. Defaults to
    /// `"../{version}/"`, a parent-relative form that resolves correctly
    /// regardless of deploy depth — set to an absolute pattern only when
    /// the consumer specifically needs one (e.g. a non-standard host
    /// where parent-relative wouldn't reach the right subtree).
    #[serde(default = "default_url_pattern")]
    pub url_pattern: String,
    /// URL the header brand link points to from each per-version page.
    /// Default `"../current/"` — points to the canonical current-version
    /// docs within the publish output, symmetric with `url_pattern`'s
    /// parent-relative default. Override when the publish output is
    /// nested under a parent site (e.g. `"../../"` to escape into a
    /// containing book) or when an absolute URL is genuinely needed.
    #[serde(default = "default_site_root_url")]
    pub site_root_url: String,
    /// The site identity the header brand link carries, on every page
    /// of the publish output — the own page and each dependency page
    /// alike. Unset, each page's brand falls back to its schema's
    /// title, which reads as one site per schema; set it when a parent
    /// site (a book) fronts the pages and the brand should name it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_title: Option<String>,
    /// Where per-version subdirs land, relative to repo root.
    #[serde(default = "default_output_dir")]
    pub output_dir: PathBuf,
    /// Output format — reserved for future writer fan-out.
    #[serde(default = "default_format")]
    pub format: String,
    /// Page composition: which half of the page leads.
    #[serde(default)]
    pub layout: crate::html_writer::PageLayout,
    /// Render the schema reference sections — the schema graph and the
    /// class/slot/enumeration/type cards. `false` builds the page around
    /// its data alone.
    #[serde(default = "default_schema_sections")]
    pub schema_sections: bool,
    /// Per-dependency-page settings, keyed by the dependency name the
    /// page renders (`[publishing.pages.<dep>]`). A table may only name
    /// a dependency some `[[instances]]` entry publishes to; pages for
    /// unconfigured dependencies use every default.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub pages: std::collections::BTreeMap<String, PageConfig>,
}

/// One `[publishing.pages.<dep>]` table: where a dependency's page
/// lives inside the output tree and how it composes. Unknown keys are
/// rejected so a typo'd setting fails loudly instead of silently
/// reverting to its default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PageConfig {
    /// Directory the page's version tree lands in, relative to (and
    /// always inside) the publish output dir. Defaults to the
    /// dependency's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    /// Page composition: which half of the page leads. Defaults to the
    /// own page's default, not the own page's setting — each page
    /// composes independently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<crate::html_writer::PageLayout>,
    /// Render the schema reference sections on this page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_sections: Option<bool>,
}

fn default_schema_sections() -> bool {
    true
}

fn default_url_pattern() -> String {
    // Parent-relative so cross-version links resolve correctly at any
    // deploy depth. An absolute pattern like `/schema/{version}/` would
    // 404 on subpath deploys (GitHub Pages, anything not at a domain
    // root). Consumers who genuinely need an absolute URL can still
    // override `url_pattern` in the manifest.
    "../{version}/".to_string()
}

fn default_site_root_url() -> String {
    // Parent-relative within the publish output so it resolves regardless
    // of deploy depth, and points at the canonical current-version page —
    // symmetric with `url_pattern`'s `../{version}/` default. Override
    // when the publish output is nested under a parent site (e.g.
    // scimantic-schema lays the publish dir under a book; `"../../"`
    // escapes back to the book root).
    "../current/".to_string()
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
        // A relative `site_root_url` is authored against a version
        // directory and re-based per page by prepending `../` — sound
        // only for a value that climbs. Anything else resolves
        // somewhere different on every page, so it is refused here
        // rather than published broken.
        if !is_absolute_site_root(&self.site_root_url) && !self.site_root_url.starts_with("../") {
            return Err(PublishError::InvalidSegment {
                what: "`[publishing] site_root_url`".to_string(),
                value: self.site_root_url.clone(),
                problem: "a relative value must climb out of the version directory (begin \
                          with `../`) or be absolute, so every page's brand link can reach \
                          the same site root"
                    .to_string(),
            });
        }
        let in_versions = self.versions.iter().any(|v| v == &self.current);
        let matches_edge = self.edge.as_deref() == Some(self.current.as_str());
        if !in_versions && !matches_edge {
            return Err(PublishError::InvalidCurrent {
                current: self.current.clone(),
                versions: self.versions.clone(),
                edge: self.edge.clone(),
            });
        }
        for label in self.versions.iter().chain(self.edge.as_ref()) {
            require_fresh_segment("a version label", label)?;
        }
        Ok(())
    }

    /// Where dependency `dep`'s page lands, relative to the output dir:
    /// the configured `[publishing.pages.<dep>] dir`, defaulting to the
    /// dependency's name. The one derivation validation and page
    /// planning both use, so what is validated is what gets written.
    fn page_dir(&self, dep: &str) -> String {
        self.pages
            .get(dep)
            .and_then(|c| c.dir.clone())
            .unwrap_or_else(|| dep.to_string())
    }
}

impl FromStr for PublishConfig {
    type Err = PublishError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let cfg: PublishConfig = toml::from_str(s)?;
        if let Some(publishing) = &cfg.publishing {
            publishing.validate()?;
            cfg.validate_pages(publishing)?;
        }
        let exemplars: Vec<&str> = cfg
            .instances
            .iter()
            .filter(|e| e.exemplar)
            .map(|e| e.name.as_str())
            .collect();
        if exemplars.len() > 1 {
            return Err(PublishError::MultipleExemplars {
                names: exemplars.join(", "),
            });
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

    /// Cross-check `[publishing.pages.*]` against the `[[instances]]`
    /// entries: every configured page must have at least one dataset,
    /// and every page directory must be a fresh single path segment —
    /// not the reserved `current`, not a version or edge label, and not
    /// another page's directory.
    fn validate_pages(&self, publishing: &PublishingConfig) -> Result<(), PublishError> {
        for dep in publishing.pages.keys() {
            if !self
                .instances
                .iter()
                .any(|e| e.schema.as_deref() == Some(dep.as_str()))
            {
                return Err(PublishError::PageWithoutEntries { dep: dep.clone() });
            }
        }
        // Every page directory goes through the gate — a dependency
        // without a `[publishing.pages]` table still becomes a directory
        // (its own name), and manifest keys are arbitrary strings.
        let mut deps: Vec<&str> = Vec::new();
        for entry in &self.instances {
            if let Some(dep) = entry.schema.as_deref()
                && !deps.contains(&dep)
            {
                deps.push(dep);
            }
        }
        let mut seen_dirs = std::collections::BTreeSet::new();
        for dep in deps {
            let dir = publishing.page_dir(dep);
            let what = format!("the page directory for dependency `{dep}`");
            require_fresh_segment(&what, &dir)?;
            if publishing.versions.iter().any(|v| v == &dir)
                || publishing.edge.as_deref() == Some(dir.as_str())
            {
                return Err(PublishError::InvalidSegment {
                    what,
                    value: dir.clone(),
                    problem: format!("it collides with the version label `{dir}`"),
                });
            }
            if !seen_dirs.insert(dir.clone()) {
                return Err(PublishError::InvalidSegment {
                    what,
                    value: dir,
                    problem: "another page already uses this directory".to_string(),
                });
            }
        }
        Ok(())
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

/// Orchestrate the multi-version doc build described by
/// `publish_cfg`'s `[publishing]` section. For each entry in
/// `versions` (and `edge` if set), extracts the schema's main file
/// at that ref into a temp file, runs the HTML generator against it,
/// and writes output to `<output_dir>/<ref>/`. Finally copies the
/// `current` version's output to `<output_dir>/current/` so consumers
/// can link to `/schema/current/` without hard-coding a version.
///
/// `output_dir` may be relative; resolved against the caller's CWD.
///
/// Resolves all refs up-front via [`resolve_refs`] so a bad tag
/// fails fast with a single combined error rather than after a
/// partial build.
/// Source for a per-version build's input schema file. Tagged
/// versions always come from a git ref; the optional edge build may
/// instead come from the working tree, for the local-preview use case
/// where the dev wants to see uncommitted edits.
#[derive(Debug, Clone, Copy)]
enum BuildSource<'a> {
    GitRef(&'a str),
    WorkingTree,
}

pub fn publish_versioned(
    repo_root: &Path,
    publish_cfg: &PublishConfig,
    output_dir: &Path,
    edge_from_worktree: bool,
) -> Result<(), PublishError> {
    let publishing = publish_cfg
        .publishing
        .as_ref()
        .ok_or(PublishError::MissingPublishingSection)?;

    // Fail fast on any unresolvable ref. Combined error names every bad
    // ref at once so the user fixes all of them in one editor pass.
    // Skip the edge ref's resolution when `edge_from_worktree` is set:
    // the working-tree path is the source of truth in that mode and
    // the ref name is only used as a subdir label.
    let mut all_refs: Vec<&str> = publishing.versions.iter().map(String::as_str).collect();
    if let Some(edge) = &publishing.edge
        && !edge_from_worktree
    {
        all_refs.push(edge.as_str());
    }
    resolve_refs(repo_root, &all_refs)?;

    // The build order for every page: edge first (it heads the version
    // dropdown), then released versions in manifest order.
    let mut refs: Vec<(String, BuildSource)> = Vec::new();
    if let Some(edge) = &publishing.edge {
        let source = if edge_from_worktree {
            BuildSource::WorkingTree
        } else {
            BuildSource::GitRef(edge)
        };
        refs.push((edge.clone(), source));
    }
    // A version label equal to the edge label is one build, not two:
    // the edge's source wins, so `--edge-from-worktree` keeps meaning
    // the worktree, and the label appears once in every dropdown.
    refs.extend(
        publishing
            .versions
            .iter()
            .filter(|v| Some(v.as_str()) != publishing.edge.as_deref())
            .map(|v| (v.clone(), BuildSource::GitRef(v))),
    );

    let pages = plan_pages(repo_root, publish_cfg, publishing, output_dir)?;
    let planned: Vec<PagePlan> = pages
        .iter()
        .map(|page| plan_page(repo_root, page, &refs))
        .collect();
    for (i, plan) in planned.iter().enumerate() {
        if plan.plans.is_empty() {
            continue;
        }
        let links = links_for(&planned, i, publish_cfg, publishing);
        render_page(repo_root, plan, links, publish_cfg, publishing)?;
    }

    Ok(())
}

/// One published schema-docs page: the own-schema page, or one
/// dependency's page with the datasets that publish to it.
struct PageSpec<'a> {
    /// `None` is the own-schema page; `Some(dep)` renders that
    /// dependency's schema.
    dep: Option<String>,
    /// Where the page's version tree lands. The own page sits at the
    /// output root; every dependency page in a directory inside it,
    /// derived by [`PublishingConfig::page_dir`] — the same derivation
    /// sibling links use, so what links navigate into is what was
    /// written.
    out_dir: PathBuf,
    entries: Vec<&'a InstanceEntry>,
    instances_first: bool,
    schema_sections: bool,
}

/// Group the `[[instances]]` entries into pages and cross-check every
/// named dependency against the repo's `panschema.toml` — publishing to
/// a dependency the manifest doesn't declare is a configuration error,
/// caught before any output is written.
fn plan_pages<'a>(
    repo_root: &Path,
    publish_cfg: &'a PublishConfig,
    publishing: &PublishingConfig,
    output_dir: &Path,
) -> Result<Vec<PageSpec<'a>>, PublishError> {
    let mut pages: Vec<PageSpec<'a>> = vec![PageSpec {
        dep: None,
        out_dir: output_dir.to_path_buf(),
        entries: Vec::new(),
        instances_first: publishing.layout == crate::html_writer::PageLayout::InstancesFirst,
        schema_sections: publishing.schema_sections,
    }];

    // The manifest is consulted only once some entry names a dependency
    // — a publish with no dependency pages must not fail on a manifest
    // it never needed (one written for a newer panschema, say).
    let mut manifest: Option<Option<crate::manifest::Manifest>> = None;

    for entry in &publish_cfg.instances {
        let Some(dep) = &entry.schema else {
            pages[0].entries.push(entry);
            continue;
        };
        let loaded = match &manifest {
            Some(m) => m,
            None => {
                let path = repo_root.join(crate::manifest::MANIFEST_FILENAME);
                let m = if path.is_file() {
                    Some(crate::manifest::Manifest::from_path(&path).map_err(|e| {
                        PublishError::ManifestUnreadable {
                            message: e.to_string(),
                        }
                    })?)
                } else {
                    None
                };
                manifest.insert(m)
            }
        };
        let declared = loaded.as_ref().is_some_and(|m| m.schemas.contains_key(dep));
        if !declared {
            let reason = if loaded.is_some() {
                format!(
                    "the repo's {} declares no [schemas.{dep}]",
                    crate::manifest::MANIFEST_FILENAME
                )
            } else {
                format!(
                    "the repo has no {} manifest to declare it in",
                    crate::manifest::MANIFEST_FILENAME
                )
            };
            return Err(PublishError::UnknownDependency {
                entry: entry.name.clone(),
                dep: dep.clone(),
                reason,
            });
        }
        match pages.iter_mut().find(|p| p.dep.as_ref() == Some(dep)) {
            Some(page) => page.entries.push(entry),
            None => {
                let cfg = publishing.pages.get(dep);
                pages.push(PageSpec {
                    dep: Some(dep.clone()),
                    out_dir: output_dir.join(publishing.page_dir(dep)),
                    entries: vec![entry],
                    instances_first: cfg.and_then(|c| c.layout)
                        == Some(crate::html_writer::PageLayout::InstancesFirst),
                    schema_sections: cfg.and_then(|c| c.schema_sections).unwrap_or(true),
                });
            }
        }
    }
    Ok(pages)
}

/// A schema or data file pinned down for one ref's build — either
/// extracted out of git into a tempfile, or already on disk (the
/// working tree, or a resolved dependency package).
enum Materialized {
    Extracted(tempfile::NamedTempFile),
    OnDisk(PathBuf),
}

impl Materialized {
    fn path(&self) -> &Path {
        match self {
            Materialized::Extracted(f) => f.path(),
            Materialized::OnDisk(p) => p.as_path(),
        }
    }
}

/// Whether `path_in_repo` exists at `ref_` — a metadata-only probe
/// (`git cat-file -e`), so a page's presence at a ref can be decided
/// without extracting content or holding open files.
fn exists_at_ref(repo_root: &Path, ref_: &str, path_in_repo: &Path) -> bool {
    let spec = format!("{ref_}:{}", path_in_repo.display());
    Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["cat-file", "-e", &spec])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// One ref's contribution to one page, decided by the presence pass:
/// which files exist and where the schema comes from, held as paths
/// only — nothing is extracted until the render pass.
struct RefPlan<'a> {
    label: &'a str,
    source: BuildSource<'a>,
    /// A dependency page's resolved schema file; `None` on the own
    /// page, whose schema comes from the publish spec's `files.main`.
    dep_schema: Option<PathBuf>,
    datasets: Vec<&'a InstanceEntry>,
}

/// One page's full build plan: the refs it exists at, in dropdown
/// order. A dependency page with no live refs has an empty `plans` and
/// publishes nothing.
struct PagePlan<'a> {
    page: &'a PageSpec<'a>,
    plans: Vec<RefPlan<'a>>,
}

impl PagePlan<'_> {
    /// The refs this page exists at, in dropdown order — always derived
    /// from `plans`, so the dropdown, the current decision, and what
    /// actually renders cannot drift apart.
    fn present(&self) -> Vec<String> {
        self.plans.iter().map(|p| p.label.to_string()).collect()
    }
}

/// The presence pass for one page: decide which refs it exists at — a
/// dependency page exists only where the dependency resolves and at
/// least one of its datasets does — using metadata probes and path
/// resolution alone, so every page's version list is known before any
/// page renders. The own page builds at every ref, as it always has.
fn plan_page<'a>(
    repo_root: &Path,
    page: &'a PageSpec<'a>,
    refs: &'a [(String, BuildSource<'a>)],
) -> PagePlan<'a> {
    let mut plans: Vec<RefPlan<'a>> = Vec::new();
    for (label, source) in refs {
        let dep_schema = match &page.dep {
            None => None,
            Some(dep) => match resolve_dep_schema_at(repo_root, *source, dep) {
                DepAtRef::Resolved(path) => Some(path),
                DepAtRef::NotDeclared => {
                    eprintln!(
                        "note: {label}: dependency `{dep}` is not declared at this ref; \
                         publishing without its page for this version"
                    );
                    continue;
                }
                DepAtRef::Failed(reason) => {
                    eprintln!(
                        "note: {label}: dependency `{dep}` cannot be published at this ref \
                         ({reason}); publishing without its page for this version"
                    );
                    continue;
                }
            },
        };

        let datasets: Vec<&InstanceEntry> = page
            .entries
            .iter()
            .copied()
            .filter(|entry| {
                let present = match source {
                    BuildSource::GitRef(ref_) => exists_at_ref(repo_root, ref_, &entry.data),
                    BuildSource::WorkingTree => repo_root.join(&entry.data).is_file(),
                };
                if present {
                    return true;
                }
                let place = match source {
                    BuildSource::GitRef(ref_) => format!("at `{ref_}`"),
                    BuildSource::WorkingTree => "in the working tree".to_string(),
                };
                eprintln!(
                    "note: {label}: instance data `{}` not present {place}; \
                     publishing this version without that instance graph",
                    entry.data.display()
                );
                false
            })
            .collect();

        // A dependency page is its datasets; without any there is
        // nothing to publish at this ref. The own page still builds
        // dataless — the schema reference is its content.
        if let Some(dep) = &page.dep
            && datasets.is_empty()
        {
            eprintln!(
                "note: {label}: no instance data for dependency `{dep}` at this ref; \
                 publishing without its page for this version"
            );
            continue;
        }

        plans.push(RefPlan {
            label,
            source: *source,
            dep_schema,
            datasets,
        });
    }

    if let Some(dep) = &page.dep
        && plans.is_empty()
    {
        eprintln!(
            "warning: dependency `{dep}` has a page configured but no ref where both the \
             dependency and its data resolve; no page was published for it"
        );
    }

    PagePlan { page, plans }
}

/// The header nav for page `me`: every live page of the site, self
/// marked active. A site with fewer than two live pages gets no nav —
/// the policy lives here, with the mechanism, so every caller inherits
/// it. Hrefs are relative to a version directory, climbing to the
/// output root by the page's own directory depth, and target the
/// sibling's `current/` alias when [`page_current`] says it will
/// exist, else the version standing as that page's current — so no
/// link points at a directory that was not built.
fn links_for(
    planned: &[PagePlan<'_>],
    me: usize,
    publish_cfg: &PublishConfig,
    publishing: &PublishingConfig,
) -> Vec<crate::html_writer::PageLink> {
    if planned.iter().filter(|p| !p.plans.is_empty()).count() < 2 {
        return Vec::new();
    }
    let dir_of = |page: &PageSpec<'_>| page.dep.as_ref().map(|d| publishing.page_dir(d));
    let up = "../".repeat(1 + page_dir_segments(publishing, planned[me].page));
    planned
        .iter()
        .enumerate()
        .filter(|(_, q)| !q.plans.is_empty())
        .map(|(j, q)| {
            let label = match &q.page.dep {
                None => publish_cfg.schema.name.clone(),
                Some(dep) => dep.clone(),
            };
            let current = page_current(publishing, &q.present());
            let seg = if current.aliased {
                "current".to_string()
            } else {
                current.version
            };
            let href = match dir_of(q.page) {
                None => format!("{up}{seg}/"),
                Some(dir) => format!("{up}{dir}/{seg}/"),
            };
            crate::html_writer::PageLink {
                label,
                href,
                active: j == me,
            }
        })
        .collect()
}

/// Render every planned version of one page, then refresh its
/// `current/` alias. Extraction happens here, one ref at a time, so
/// the number of open files stays bounded by one ref's needs however
/// long the version list grows.
fn render_page(
    repo_root: &Path,
    plan: &PagePlan<'_>,
    links: Vec<crate::html_writer::PageLink>,
    publish_cfg: &PublishConfig,
    publishing: &PublishingConfig,
) -> Result<(), PublishError> {
    let page = plan.page;
    let cohort = cohort_for(publishing, publish_cfg, page, &plan.present(), links);
    for rp in &plan.plans {
        let schema = match &rp.dep_schema {
            Some(path) => Materialized::OnDisk(path.clone()),
            None => match rp.source {
                BuildSource::GitRef(ref_) => Materialized::Extracted(extract_main_at_ref(
                    repo_root,
                    ref_,
                    &publish_cfg.files.main,
                )?),
                // The manifest's `files.main` is documented as relative
                // to the publish-spec's location; in the supported v1
                // layout that's the repo root. Resolve there.
                BuildSource::WorkingTree => {
                    Materialized::OnDisk(repo_root.join(&publish_cfg.files.main))
                }
            },
        };
        let extracted: Vec<(Materialized, &InstanceEntry)> = rp
            .datasets
            .iter()
            .filter_map(|entry| {
                let materialized = match rp.source {
                    BuildSource::GitRef(ref_) => extract_main_at_ref(repo_root, ref_, &entry.data)
                        .ok()
                        .map(Materialized::Extracted),
                    BuildSource::WorkingTree => {
                        let path = repo_root.join(&entry.data);
                        path.is_file().then(|| Materialized::OnDisk(path))
                    }
                };
                if materialized.is_none() {
                    eprintln!(
                        "note: {}: instance data `{}` could not be extracted; \
                         publishing this version without that instance graph",
                        rp.label,
                        entry.data.display()
                    );
                }
                materialized.map(|m| (m, *entry))
            })
            .collect();
        let version_out = page.out_dir.join(rp.label);
        std::fs::create_dir_all(&version_out)?;
        let datasets: Vec<(&Path, &InstanceEntry)> = extracted
            .iter()
            .map(|(file, entry)| (file.path(), *entry))
            .collect();
        generate_html_for_version(rp.label, schema.path(), &version_out, &cohort, &datasets)?;
    }

    // current/ is a copy of the page-current version's output, not a
    // symlink (static hosts handle directories cleanly; symlinks are
    // flaky on GH Pages) and not a re-render (would duplicate work and
    // risk byte divergence). For the own page the source always exists:
    // `current` is parse-time-validated to be in `versions` or equal
    // `edge`, both of which were just built.
    refresh_current(&page.out_dir, &publishing.current)?;
    Ok(())
}

/// Refresh one page's `current/` alias from its `<current>` version
/// directory. The source is checked before the old alias is touched, so
/// a missing source never destroys an alias it was about to replace —
/// but an alias whose source no longer exists is stale output from a
/// previous run into the same tree, and is removed rather than left
/// serving content the version dropdown no longer offers. A page ending
/// up without an alias is said out loud here, where the state is known.
fn refresh_current(page_out: &Path, current: &str) -> Result<(), PublishError> {
    let src = page_out.join(current);
    let dst = page_out.join("current");
    if !src.is_dir() {
        if dst.is_dir() {
            std::fs::remove_dir_all(&dst)?;
        }
        eprintln!(
            "note: `{current}` was not built for the page at `{}`; it publishes without a \
             current/ alias",
            page_out.display()
        );
        return Ok(());
    }
    if dst.exists() {
        std::fs::remove_dir_all(&dst)?;
    }
    copy_dir_recursive(&src, &dst)?;
    Ok(())
}

/// Resolve dependency `dep`'s main schema file as pinned at one ref:
/// the ref's own `panschema.toml` names the source, so a historical
/// page shows the contract as it was. `github:` sources resolve from
/// the local cache only — publish never fetches over the network —
/// and `path:` sources resolve against the working tree, since a path
/// dependency carries no pin to honor.
///
/// The two skip outcomes are different facts and print differently: a
/// ref that predates the dependency is quiet history, while a declared
/// dependency that fails to resolve — a cold cache, a corrupt package,
/// a malformed manifest at the ref — carries the resolver's own message
/// so the fix (often `panschema fetch`) reaches the user.
enum DepAtRef {
    Resolved(PathBuf),
    /// No manifest at this ref, or the dependency not declared there —
    /// the ref predates the page.
    NotDeclared,
    /// The dependency is declared at this ref but unusable; the reason
    /// to print.
    Failed(String),
}

fn resolve_dep_schema_at(repo_root: &Path, source: BuildSource<'_>, dep: &str) -> DepAtRef {
    let manifest = match content_at(
        repo_root,
        source,
        Path::new(crate::manifest::MANIFEST_FILENAME),
    ) {
        ContentAt::Absent => return DepAtRef::NotDeclared,
        ContentAt::Unreadable(e) => {
            return DepAtRef::Failed(format!("the ref's panschema.toml is unreadable: {e}"));
        }
        ContentAt::Content(content) => match content.parse::<crate::manifest::Manifest>() {
            Ok(m) => m,
            Err(e) => {
                return DepAtRef::Failed(format!("the ref's panschema.toml does not parse: {e}"));
            }
        },
    };
    let Some(dep_spec) = manifest.schemas.get(dep) else {
        return DepAtRef::NotDeclared;
    };
    // The manifest's parsed source kind decides whether a pin exists to
    // honor. The lock entry's own spelling never does: a stale entry
    // left from before a dependency switched to `path:` must not gate
    // the working tree, and one recorded as `path:` must not let a
    // pinned source escape the gate.
    let pinned = matches!(
        crate::source::SchemaSource::from_dep(dep, dep_spec),
        Ok(crate::source::SchemaSource::Github { .. })
    );
    let resolved = match crate::source::resolve_dep(dep, dep_spec, repo_root, &OfflineTarballs) {
        Ok(resolved) => resolved,
        Err(e) => return DepAtRef::Failed(e.to_string()),
    };

    // The ref's committed lockfile is the contract the page claims to
    // show; cached content that fails its checksum must not render as
    // "the contract as it was". A ref without a lockfile (or without an
    // entry for this dependency) publishes ungated, as before locking —
    // but a lockfile that is present and broken refuses the page, since
    // failing open would disable the gate exactly when a hand-edit or
    // version skew makes it matter.
    if pinned {
        match content_at(
            repo_root,
            source,
            Path::new(crate::lockfile::LOCKFILE_FILENAME),
        ) {
            ContentAt::Absent => {}
            ContentAt::Unreadable(e) => {
                return DepAtRef::Failed(format!("the ref's panschema.lock is unreadable: {e}"));
            }
            ContentAt::Content(content) => match content.parse::<crate::lockfile::Lockfile>() {
                Err(e) => {
                    return DepAtRef::Failed(format!(
                        "the ref's panschema.lock does not parse: {e}"
                    ));
                }
                Ok(lockfile) => {
                    if let Some(reason) = lockfile_drift(&lockfile, dep, dep_spec, &resolved) {
                        return DepAtRef::Failed(reason);
                    }
                }
            },
        }
    }
    DepAtRef::Resolved(resolved.schema_path)
}

/// One repo file's content as of a build source: read from the ref's
/// tree, or from the working tree. Absence — often legitimate history —
/// is distinguished from a file that is present but cannot be read,
/// which callers surface rather than swallow.
enum ContentAt {
    Absent,
    Content(String),
    Unreadable(String),
}

fn content_at(repo_root: &Path, source: BuildSource<'_>, rel: &Path) -> ContentAt {
    match source {
        BuildSource::GitRef(ref_) => {
            if !exists_at_ref(repo_root, ref_, rel) {
                return ContentAt::Absent;
            }
            let spec = format!("{ref_}:{}", rel.display());
            match run_git_capture(repo_root, &["show", &spec]) {
                Ok(content) => ContentAt::Content(content),
                Err(e) => ContentAt::Unreadable(e.to_string()),
            }
        }
        BuildSource::WorkingTree => {
            let path = repo_root.join(rel);
            if !path.is_file() {
                return ContentAt::Absent;
            }
            match std::fs::read_to_string(&path) {
                Ok(content) => ContentAt::Content(content),
                Err(e) => ContentAt::Unreadable(e.to_string()),
            }
        }
    }
}

/// Why the ref's lockfile refuses the resolved dependency, if it does.
/// `None` passes: the entry agrees with the ref's own manifest and the
/// cached content matches its checksum — or the lockfile never
/// recorded this dependency, which publishes ungated. An entry that
/// disagrees with the manifest is reported as a stale lock, never as
/// drifted content: the cache may be pristine, and no fetch can repair
/// a committed historical lockfile.
fn lockfile_drift(
    lockfile: &crate::lockfile::Lockfile,
    dep: &str,
    dep_spec: &crate::manifest::SchemaDep,
    resolved: &crate::source::Resolved,
) -> Option<String> {
    let entry = lockfile.entry(dep)?;
    let manifest_spec = crate::source::SchemaSource::from_dep(dep, dep_spec)
        .ok()
        .map(|s| s.source_spec());
    if manifest_spec.as_deref() != Some(entry.source.as_str()) {
        return Some(format!(
            "the ref's lockfile records source `{}` where its manifest declares `{}` — \
             the lock is stale relative to the manifest at this ref (re-run \
             `panschema fetch` before tagging)",
            entry.source,
            manifest_spec.unwrap_or_default(),
        ));
    }
    if let Some(locked_version) = &entry.version
        && locked_version != &resolved.version
    {
        return Some(format!(
            "the ref's lockfile records version {locked_version} where its manifest pins \
             {} — the lock is stale relative to the manifest at this ref (re-run \
             `panschema fetch` before tagging)",
            resolved.version,
        ));
    }
    match entry.checksum_drift(&resolved.schema_path) {
        Err(e) => Some(format!("the cached schema could not be checksummed: {e}")),
        Ok(Some(observed)) => Some(format!(
            "the cached content fails the ref's lockfile checksum (locked {}, cached \
             {observed}) — a re-published tag or an edited cache; `panschema fetch \
             --check` inspects, `panschema fetch` refreshes",
            entry.checksum
        )),
        Ok(None) => None,
    }
}

/// A [`crate::source::TarballSource`] that refuses to fetch: handed to
/// [`crate::source::resolve_github`], it turns resolution into a pure
/// cache lookup, keeping publish reproducible offline. The error names
/// the fix.
struct OfflineTarballs;

impl crate::source::TarballSource for OfflineTarballs {
    fn fetch(
        &self,
        owner: &str,
        repo: &str,
        tag: &str,
        _sink: &mut dyn std::io::Write,
    ) -> Result<(), crate::source::TarballFetchError> {
        Err(crate::source::TarballFetchError::Network {
            owner: owner.to_string(),
            repo: repo.to_string(),
            tag: tag.to_string(),
            source: "publish resolves dependencies from the local cache only; \
                     run `panschema fetch` first"
                .into(),
        })
    }
}

/// Builder data for [`crate::html_writer::VersionContext`]. Computed
/// once per publish run; specialised for each per-version page via
/// [`CohortContext::context_for`].
#[derive(Debug, Clone)]
struct CohortContext {
    all_versions: Vec<String>,
    current: String,
    edge: Option<String>,
    url_pattern: String,
    site_root_href: String,
    /// The site identity for the brand link, from `[publishing]
    /// site_title`; `None` falls back to each page's schema title.
    site_title: Option<String>,
    label_sources: std::collections::BTreeMap<String, String>,
    /// Page composition, from `[publishing]`: instance section first?
    instances_first: bool,
    /// Page composition, from `[publishing]`: schema reference rendered?
    schema_sections: bool,
    /// The site's pages for the header nav; empty on single-page sites.
    page_links: Vec<crate::html_writer::PageLink>,
}

/// One page's version context. `present` is the page's build list —
/// already in dropdown order (edge first, then released versions in
/// manifest order), already filtered to the refs where the page exists,
/// so the dropdown offers exactly what was built. A page absent at the
/// configured current treats its first present *released* ref as
/// current — never the edge build, which is unreleased by definition —
/// and, since such a page publishes without a `current/` alias, its
/// default brand link points at that version directly instead of at an
/// alias that does not exist.
fn cohort_for(
    publishing: &PublishingConfig,
    publish_cfg: &PublishConfig,
    page: &PageSpec<'_>,
    present: &[String],
    page_links: Vec<crate::html_writer::PageLink>,
) -> CohortContext {
    let current = page_current(publishing, present);
    let site_root_href = if publishing.site_root_url == default_site_root_url() {
        // The default resolves inside each page's own version tree, so
        // it means the same thing at any depth.
        if current.aliased {
            publishing.site_root_url.clone()
        } else {
            format!("../{}/", current.version)
        }
    } else {
        // A configured value names a site-level destination authored
        // against the own page's version directory; a dependency page's
        // version directory sits deeper by its page directory, so the
        // same destination needs one `../` per extra segment.
        depth_adjusted_site_root(
            &publishing.site_root_url,
            page_dir_segments(publishing, page),
        )
    };
    CohortContext {
        all_versions: present.to_vec(),
        current: current.version,
        edge: publishing.edge.clone(),
        url_pattern: publishing.url_pattern.clone(),
        site_root_href,
        site_title: publishing.site_title.clone(),
        label_sources: publish_cfg.label_sources.clone(),
        instances_first: page.instances_first,
        schema_sections: page.schema_sections,
        page_links,
    }
}

/// Directory segments between the output root and a page's version
/// directories: 0 for the own page, one per segment of a dependency
/// page's directory — validated to a single fresh segment today, and
/// derived here for every consumer, so the cross-page nav and the
/// brand link can never disagree about a page's depth.
fn page_dir_segments(publishing: &PublishingConfig, page: &PageSpec<'_>) -> usize {
    page.dep
        .as_ref()
        .map_or(0, |dep| publishing.page_dir(dep).split('/').count())
}

/// Whether a configured URL means the same destination at any page
/// depth: root-relative, or carrying an RFC 3986 scheme at the front —
/// anchored, so a URL buried in a query or fragment never makes a
/// relative value read as absolute.
fn is_absolute_site_root(value: &str) -> bool {
    if value.starts_with('/') {
        return true;
    }
    let mut chars = value.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    for c in chars {
        match c {
            ':' => return true,
            c if c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.') => {}
            _ => return false,
        }
    }
    false
}

/// A relative site-root URL, re-based for a page whose version
/// directory sits `extra_segments` deeper than the own page's — the
/// depth the URL was authored against. Absolute URLs pass through
/// unchanged. Sound only for climbing values, which validation
/// guarantees a configured relative override to be.
fn depth_adjusted_site_root(site_root_url: &str, extra_segments: usize) -> String {
    if extra_segments == 0 || is_absolute_site_root(site_root_url) {
        return site_root_url.to_string();
    }
    format!("{}{}", "../".repeat(extra_segments), site_root_url)
}

/// One page's current-version decision, made in one place so the nav
/// links, the banner, the brand link, and the alias can never disagree
/// about it.
struct PageCurrent {
    /// The version standing as the page's current: the configured
    /// current when the page exists there, else the first present
    /// released ref in the manifest's version order — never the edge
    /// build, which is unreleased by definition — else the edge itself
    /// when it is all the page has.
    version: String,
    /// The configured current is among the page's built refs, so
    /// [`refresh_current`] will build the `current/` alias from it.
    aliased: bool,
}

fn page_current(publishing: &PublishingConfig, present: &[String]) -> PageCurrent {
    if present.iter().any(|v| v == &publishing.current) {
        return PageCurrent {
            version: publishing.current.clone(),
            aliased: true,
        };
    }
    let version = present
        .iter()
        .find(|v| Some(v.as_str()) != publishing.edge.as_deref())
        .or_else(|| present.first())
        .expect("a page with no built refs is skipped before rendering")
        .clone();
    PageCurrent {
        version,
        aliased: false,
    }
}

impl CohortContext {
    fn context_for(&self, viewing: &str) -> crate::html_writer::VersionContext {
        crate::html_writer::VersionContext {
            all_versions: self.all_versions.clone(),
            viewing: viewing.to_string(),
            current: self.current.clone(),
            edge: self.edge.clone(),
            url_pattern: self.url_pattern.clone(),
        }
    }
}

/// Run the HTML generator against a single extracted schema file with
/// the cohort's version context attached, so the rendered page gets
/// the dropdown + banner UX. Wraps the Reader/Writer pipeline so any
/// failure is surfaced as [`PublishError::GenerateFailed`] tagged with
/// the version that failed.
fn generate_html_for_version(
    version: &str,
    input: &Path,
    output: &Path,
    cohort: &CohortContext,
    instances: &[(&Path, &InstanceEntry)],
) -> Result<(), PublishError> {
    use crate::html_writer::HtmlWriter;
    use crate::io::{FormatRegistry, Writer};

    let registry = FormatRegistry::with_defaults();
    // Read + resolve local `imports:` through the shared load path, so a
    // published version renders the same merged schema as `generate`/`serve`.
    let schema = crate::import_resolve::load_schema(input, &registry).map_err(|e| {
        PublishError::GenerateFailed {
            version: version.to_string(),
            message: e.to_string(),
        }
    })?;
    let mut writer = HtmlWriter::with_options(true)
        .with_version_context(cohort.context_for(version))
        .with_site_root_href(cohort.site_root_href.clone())
        .with_instances_first(cohort.instances_first)
        .with_schema_sections(cohort.schema_sections)
        .with_page_links(cohort.page_links.clone());
    if let Some(site_title) = &cohort.site_title {
        writer = writer.with_site_title(site_title.clone());
    }
    // The file is read from the first path (a per-ref extraction lands in a
    // tempfile) while provenance shows the declared name.
    let mut loaded: Vec<(String, crate::instances::InstanceSet, &InstanceEntry)> = Vec::new();
    for (data_path, entry) in instances {
        let declared = entry.data.as_path();
        let content =
            std::fs::read_to_string(data_path).map_err(|e| PublishError::GenerateFailed {
                version: version.to_string(),
                message: format!("reading instance data {}: {e}", declared.display()),
            })?;
        let data: serde_norway::Value =
            serde_norway::from_str(&content).map_err(|e| PublishError::GenerateFailed {
                version: version.to_string(),
                message: format!("parsing instance data {}: {e}", declared.display()),
            })?;
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        // Same check as `generate --instances` and `validate --data`: a
        // curated A-box is published page content, so it gets the conformance
        // gate rather than only a reference-integrity look.
        for v in crate::validate::validate_instances(&schema, &set) {
            eprintln!("warning: {version}: {v}");
        }
        loaded.push((declared.display().to_string(), set, entry));
    }

    // `publish` already knows the full declared set, so the cross-dataset
    // check costs nothing to run here. Reported, not fatal: published pairs
    // routinely share records on purpose.
    let borrowed: Vec<(&str, &crate::instances::InstanceSet)> = loaded
        .iter()
        .map(|(label, set, _)| (label.as_str(), set))
        .collect();
    for c in crate::diagnostics::cross_dataset_iri_collisions(&schema, &borrowed) {
        eprintln!("note: {version}: {}", c.message());
    }
    for split in crate::diagnostics::cross_dataset_unintended_splits(&schema, &borrowed) {
        eprintln!("note: {version}: {}", split.message());
    }

    // Declaration order drives the selector; `exemplar` decides which opens.
    for (declared, set, entry) in loaded {
        let mut dataset = crate::html_writer::InstanceDataset::new(entry.name.clone(), set);
        if let Some(name) = std::path::Path::new(&declared)
            .file_name()
            .and_then(|n| n.to_str())
        {
            dataset = dataset.with_provenance(name);
        }
        if entry.exemplar {
            dataset = dataset.as_default();
        }
        writer = writer.with_instance_dataset(dataset);
    }
    if let Some(store) =
        crate::labels::open_default_store(&schema, false, &cohort.label_sources, false)
    {
        writer = writer.with_label_store(store);
    }
    writer
        .write(&schema, output)
        .map_err(|e| PublishError::GenerateFailed {
            version: version.to_string(),
            message: e.to_string(),
        })?;
    Ok(())
}

/// Recursive directory copy. `std::fs` only ships single-file copy;
/// the `current/` alias is a small tree so a hand-rolled walker is
/// fine. Errors propagate via `std::io::Error` → `PublishError::Io`.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
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
        assert!(cfg.label_sources.is_empty());
    }

    #[test]
    fn parses_publish_spec_with_label_sources_section() {
        let toml = r#"
[schema]
name = "scimantic-schema"
version = "0.1.3"
linkml = "1.7.0"

[files]
main = "schema/scimantic.yaml"

[label_sources]
cco = "https://example.org/pinned/cco-v1.5.ttl"
"#;
        let cfg = toml.parse::<PublishConfig>().expect("should parse");
        assert_eq!(
            cfg.label_sources.get("cco").map(String::as_str),
            Some("https://example.org/pinned/cco-v1.5.ttl")
        );
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
        assert_eq!(publishing.url_pattern, "../{version}/");
        assert_eq!(publishing.site_root_url, "../current/");
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
site_root_url = "../../"
output_dir = "build/site"
format = "html"
"#;
        let cfg: PublishConfig = toml.parse().expect("should parse");
        let publishing = cfg.publishing.expect("publishing should be present");
        assert_eq!(publishing.edge.as_deref(), Some("main"));
        assert_eq!(publishing.current, "main");
        assert_eq!(publishing.url_pattern, "/docs/{version}/");
        assert_eq!(publishing.site_root_url, "../../");
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

        let bad_layout = toml.replace(
            "current = \"v9.9.9\"",
            "current = \"v0.1.0\"\nlayout = \"sideways\"",
        );
        let text = bad_layout
            .parse::<PublishConfig>()
            .expect_err("should reject an unknown layout")
            .to_string();
        assert!(
            text.contains("sideways")
                && text.contains("schema-first")
                && text.contains("instances-first"),
            "the error names the offending and accepted layout values; got: {text}"
        );
    }

    #[test]
    fn parses_publish_spec_without_book_link_section() {
        // `[book_link]` is opt-in: a spec without it must load, with no
        // book-link config present.
        let toml = r#"
[schema]
name = "x"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"
"#;
        let cfg: PublishConfig = toml.parse().expect("a spec without [book_link] must load");
        assert!(cfg.book_link.is_none());
    }

    #[test]
    fn parses_empty_book_link_with_documented_defaults() {
        // A bare `[book_link]` header takes every documented default:
        // disabled, schema docs at `schema/current/`, generic label.
        let toml = r#"
[schema]
name = "x"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[book_link]
"#;
        let cfg: PublishConfig = toml.parse().expect("should parse");
        let book_link = cfg.book_link.expect("book_link should be present");
        assert!(!book_link.enabled(), "enabled must default to false");
        let entry = &book_link.entries()[0];
        assert_eq!(entry.schema_path, "schema/current/");
        assert_eq!(entry.label, "Schema reference");
    }

    /// A book fronting several schemas writes `[[book_link]]`. Each entry
    /// carries its own target and label, in declaration order.
    #[test]
    fn parses_book_link_as_a_list_of_entries() {
        let toml = r#"
[schema]
name = "x"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[[book_link]]
schema_path = "schema/current/"
label = "Wine schema"

[[book_link]]
schema_path = "schema/cqa/current/"
label = "CQ&A contract"
"#;
        let cfg: PublishConfig = toml.parse().expect("the list form must parse");
        let links = cfg.book_link.expect("book_link should be present");
        let entries = links.entries();
        assert_eq!(
            entries.len(),
            2,
            "both entries should survive; got {entries:?}"
        );
        assert_eq!(entries[0].schema_path, "schema/current/");
        assert_eq!(entries[0].label, "Wine schema");
        assert_eq!(entries[1].schema_path, "schema/cqa/current/");
        assert_eq!(entries[1].label, "CQ&A contract");
        assert!(links.enabled(), "writing entries is itself the opt-in");
    }

    /// The table form is what every existing book writes. It must keep
    /// producing exactly one entry, with today's defaults.
    #[test]
    fn the_table_form_still_yields_a_single_entry() {
        let toml = r#"
[schema]
name = "x"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[book_link]
enabled = true
"#;
        let cfg: PublishConfig = toml.parse().expect("should parse");
        let links = cfg.book_link.expect("book_link should be present");
        assert!(links.enabled());
        let entries = links.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].schema_path, "schema/current/");
        assert_eq!(entries[0].label, "Schema reference");
    }

    /// `enabled = false` is the off switch in the table form, and an empty
    /// list is the same as no section at all.
    #[test]
    fn book_link_is_off_when_disabled_or_empty() {
        // `book_link` must sit at the top level. Appending a bare key after
        // a `[table]` header would silently land inside that table.
        let base = r#"
[schema]
name = "x"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"
"#;
        let disabled: PublishConfig = format!("{base}\n[book_link]\nenabled = false\n")
            .parse()
            .expect("should parse");
        assert!(!disabled.book_link.expect("present").enabled());

        let empty: PublishConfig = r#"
book_link = []

[schema]
name = "x"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"
"#
        .parse()
        .expect("an empty list should parse");
        let links = empty.book_link.expect("present");
        assert!(!links.enabled(), "an empty list means the feature is off");
        assert!(links.entries().is_empty());
    }

    /// A typo in a list entry must fail loudly. A silently dropped button
    /// is the failure this rejects — the author sees a missing link and no
    /// reason for it.
    #[test]
    fn rejects_a_list_entry_with_an_unknown_key() {
        let toml = r#"
[schema]
name = "x"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[[book_link]]
schema_path = "schema/current/"
labl = "typo"
"#;
        let err = toml
            .parse::<PublishConfig>()
            .expect_err("an unknown key in a list entry must be rejected");
        assert!(
            format!("{err}").contains("labl"),
            "the error should name the offending key; got: {err}"
        );
    }

    #[test]
    fn parses_full_book_link_with_overrides() {
        let toml = r#"
[schema]
name = "x"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[book_link]
enabled = true
schema_path = "docs/schema/"
label = "Data model"
"#;
        let cfg: PublishConfig = toml.parse().expect("should parse");
        let book_link = cfg.book_link.expect("book_link should be present");
        assert!(book_link.enabled());
        let entry = &book_link.entries()[0];
        assert_eq!(entry.schema_path, "docs/schema/");
        assert_eq!(entry.label, "Data model");
    }

    #[test]
    fn rejects_book_link_with_unknown_key() {
        // A typo'd key must fail loudly (naming the offending key), not
        // silently drop the setting the user thought they configured.
        let toml = r#"
[schema]
name = "x"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[book_link]
lable = "Data model"
"#;
        let err = toml
            .parse::<PublishConfig>()
            .expect_err("an unknown [book_link] key must be rejected");
        assert!(
            err.to_string().contains("lable"),
            "error should name the offending key; got: {err}"
        );
    }

    #[test]
    fn rejects_book_link_with_wrong_value_type() {
        let toml = r#"
[schema]
name = "x"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[book_link]
enabled = "yes"
"#;
        let err = toml
            .parse::<PublishConfig>()
            .expect_err("a non-boolean `enabled` must be rejected");
        assert!(
            err.to_string().contains("enabled"),
            "error should name the offending field; got: {err}"
        );
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

    // ----- resolve_refs / extract_main_at_ref -----

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

    // ----- publish_versioned -----

    /// Build a versioned fixture repo whose `schema.yaml` is a real
    /// (minimal) LinkML schema at each tag — enough that the HTML
    /// writer can run end-to-end against the extracted content.
    fn make_versioned_linkml_repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        run(path, &["init", "--initial-branch=main", "--quiet"]);
        run(path, &["config", "user.email", "test@example.com"]);
        run(path, &["config", "user.name", "Test"]);
        run(path, &["config", "commit.gpgsign", "false"]);
        for (tag, version_marker) in [("v0.1.0", "0.1.0"), ("v0.2.0", "0.2.0")] {
            let schema = format!(
                "id: https://example.org/{tag}\n\
                 name: fixture_{version_marker}\n\
                 version: {version_marker}\n\
                 prefixes:\n  schema: https://example.org/\n\
                 default_prefix: schema\n\
                 classes:\n  Thing:\n    description: a thing\n"
            );
            std::fs::write(path.join("schema.yaml"), schema).unwrap();
            run(path, &["add", "schema.yaml"]);
            run(
                path,
                &["commit", "-m", &format!("release {tag}"), "--quiet"],
            );
            run(path, &["tag", tag]);
        }
        // Move main beyond v0.2.0 so the edge build differs.
        let edge_schema = "id: https://example.org/main\n\
             name: fixture_edge\n\
             version: 0.3.0-dev\n\
             prefixes:\n  schema: https://example.org/\n\
             default_prefix: schema\n\
             classes:\n  Thing:\n    description: a thing\n  EdgeOnly:\n    description: only on edge\n";
        std::fs::write(path.join("schema.yaml"), edge_schema).unwrap();
        run(path, &["add", "schema.yaml"]);
        run(path, &["commit", "-m", "WIP", "--quiet"]);
        tmp
    }

    fn make_publish_cfg_with_versions(
        versions: Vec<&str>,
        edge: Option<&str>,
        current: &str,
    ) -> PublishConfig {
        PublishConfig {
            instances: Vec::new(),
            schema: SchemaInfo {
                name: "fixture".into(),
                version: "0.2.0".into(),
                linkml: "1.7.0".into(),
            },
            files: FileMapping {
                main: PathBuf::from("schema.yaml"),
            },
            publishing: Some(PublishingConfig {
                versions: versions.into_iter().map(String::from).collect(),
                edge: edge.map(String::from),
                current: current.into(),
                url_pattern: default_url_pattern(),
                site_root_url: default_site_root_url(),
                site_title: None,
                output_dir: PathBuf::from("site/schema"),
                format: default_format(),
                layout: crate::html_writer::PageLayout::default(),
                schema_sections: default_schema_sections(),
                pages: std::collections::BTreeMap::new(),
            }),
            label_sources: std::collections::BTreeMap::new(),
            book_link: None,
        }
    }

    #[test]
    fn user_supplied_url_pattern_survives_to_rendered_html() {
        // Back-compat: when the manifest sets `url_pattern` explicitly,
        // the rendered HTML must use that exact pattern (substituted),
        // not silently fall back to the new parent-relative default.
        // Consumers with non-standard hosting may need an absolute
        // pattern; this test pins that escape hatch.
        let repo = make_versioned_linkml_repo();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], None, "v0.2.0");
        cfg.publishing.as_mut().unwrap().url_pattern = "/custom/path/{version}/".into();
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");

        let stale = std::fs::read_to_string(out.path().join("v0.1.0/index.html")).unwrap();
        assert!(
            stale.contains("/custom/path/v0.2.0/"),
            "stale banner must use the user-supplied url_pattern verbatim"
        );
        // The new parent-relative default must NOT leak through when
        // the user overrides — otherwise we've broken back-compat.
        assert!(
            !stale.contains("../v0.2.0/"),
            "default parent-relative pattern must not leak when user supplies url_pattern"
        );
    }

    #[test]
    fn publish_versioned_errors_when_publishing_section_absent() {
        let repo = make_versioned_linkml_repo();
        let cfg = PublishConfig {
            instances: Vec::new(),
            schema: SchemaInfo {
                name: "x".into(),
                version: "0.1.0".into(),
                linkml: "1.7.0".into(),
            },
            files: FileMapping {
                main: PathBuf::from("schema.yaml"),
            },
            publishing: None,
            label_sources: std::collections::BTreeMap::new(),
            book_link: None,
        };
        let out = tempfile::tempdir().unwrap();
        let err =
            publish_versioned(repo.path(), &cfg, out.path(), false).expect_err("no publishing");
        assert!(matches!(err, PublishError::MissingPublishingSection));
    }

    #[test]
    fn parses_instances_entries() {
        let toml = r#"
[schema]
name = "fixture"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[[instances]]
name = "catalog"
data = "data/instances.yaml"
exemplar = true

[[instances]]
name = "extra"
data = "data/extra.yaml"
"#;
        let cfg: PublishConfig = toml.parse().expect("parses");
        assert_eq!(cfg.instances.len(), 2);
        assert!(cfg.instances[0].exemplar);
        assert_eq!(cfg.instances[0].name, "catalog");
        assert_eq!(cfg.instances[1].data, PathBuf::from("data/extra.yaml"));
        assert!(!cfg.instances[1].exemplar);
    }

    #[test]
    fn rejects_two_exemplar_instances() {
        let toml = r#"
[schema]
name = "fixture"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[[instances]]
name = "a"
data = "a.yaml"
exemplar = true

[[instances]]
name = "b"
data = "b.yaml"
exemplar = true
"#;
        let err = toml.parse::<PublishConfig>().unwrap_err();
        assert!(
            err.to_string().contains("exemplar"),
            "two exemplars must fail naming the conflict; got: {err}"
        );
    }

    #[test]
    fn rejects_unknown_instances_key() {
        let toml = r#"
[schema]
name = "fixture"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[[instances]]
name = "a"
data = "a.yaml"
exemplur = true
"#;
        assert!(toml.parse::<PublishConfig>().is_err());
    }

    /// A repo whose schema has a `tree_root` container: v0.1.0 predates the
    /// data file, v0.2.0 (and the worktree) carry it.
    fn make_repo_with_instance_data() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        run(path, &["init", "--initial-branch=main", "--quiet"]);
        run(path, &["config", "user.email", "test@example.com"]);
        run(path, &["config", "user.name", "Test"]);
        run(path, &["config", "commit.gpgsign", "false"]);
        let schema = concat!(
            "id: https://example.org/wine\n",
            "name: fixture_wine\n",
            "version: 0.1.0\n",
            "prefixes:\n",
            "  wine: https://example.org/wine/\n",
            "default_prefix: wine\n",
            "default_range: string\n",
            "classes:\n",
            "  Catalog:\n",
            "    tree_root: true\n",
            "    attributes:\n",
            "      wines:\n",
            "        range: Wine\n",
            "        multivalued: true\n",
            "  Wine:\n",
            "    attributes:\n",
            "      id:\n",
            "        identifier: true\n",
            "      name:\n",
            "        range: string\n",
        );
        std::fs::write(path.join("schema.yaml"), schema).unwrap();
        run(path, &["add", "schema.yaml"]);
        run(path, &["commit", "-m", "release v0.1.0", "--quiet"]);
        run(path, &["tag", "v0.1.0"]);
        std::fs::create_dir_all(path.join("data")).unwrap();
        std::fs::write(
            path.join("data/instances.yaml"),
            "wines:\n  - id: morgon\n    name: Morgon\n",
        )
        .unwrap();
        std::fs::write(
            path.join("data/preview.yaml"),
            "wines:\n  - id: previewWine\n    name: Preview Pinot\n",
        )
        .unwrap();
        run(path, &["add", "data/instances.yaml", "data/preview.yaml"]);
        run(path, &["commit", "-m", "release v0.2.0", "--quiet"]);
        run(path, &["tag", "v0.2.0"]);
        tmp
    }

    /// A repo that consumes a dependency schema (`path:` package in-tree)
    /// and curates instance data for it: v0.1.0 predates the dependency,
    /// the manifest, and the data; v0.2.0 (and the worktree) carry all
    /// three. The dep data's `related` reference points outside the
    /// dataset, so the page draws it as an external node.
    fn make_repo_with_dependency_data() -> tempfile::TempDir {
        let tmp = make_repo_with_instance_data();
        let path = tmp.path();
        std::fs::create_dir_all(path.join("dep-pkg")).unwrap();
        std::fs::write(
            path.join("dep-pkg/panschema-publish.toml"),
            concat!(
                "[schema]\n",
                "name = \"cqa\"\n",
                "version = \"0.1.0\"\n",
                "linkml = \"1.7.0\"\n",
                "\n",
                "[files]\n",
                "main = \"cqa.yaml\"\n",
            ),
        )
        .unwrap();
        std::fs::write(
            path.join("dep-pkg/cqa.yaml"),
            concat!(
                "id: https://example.org/cqa\n",
                "name: fixture_cqa\n",
                "version: 0.1.0\n",
                "prefixes:\n",
                "  cqa: https://example.org/cqa/\n",
                "default_prefix: cqa\n",
                "default_range: string\n",
                "classes:\n",
                "  Ledger:\n",
                "    tree_root: true\n",
                "    attributes:\n",
                "      assessments:\n",
                "        range: Assessment\n",
                "        multivalued: true\n",
                "  Assessment:\n",
                "    attributes:\n",
                "      id:\n",
                "        identifier: true\n",
                "      verdict:\n",
                "        range: string\n",
                "      related:\n",
                "        range: Assessment\n",
            ),
        )
        .unwrap();
        std::fs::write(
            path.join("panschema.toml"),
            "[schemas.cqa]\npath = \"./dep-pkg\"\n",
        )
        .unwrap();
        std::fs::write(
            path.join("data/assessments.yaml"),
            concat!(
                "assessments:\n",
                "  - id: a1\n",
                "    verdict: pass\n",
                "    related: https://example.org/wine/offsite\n",
            ),
        )
        .unwrap();
        std::fs::write(
            path.join("data/extra-assessments.yaml"),
            "assessments:\n  - id: x1\n    verdict: fail\n",
        )
        .unwrap();
        run(path, &["add", "."]);
        run(
            path,
            &["commit", "-m", "add cqa dependency and data", "--quiet"],
        );
        run(path, &["tag", "-f", "v0.2.0"]);
        tmp
    }

    fn dep_entry() -> InstanceEntry {
        InstanceEntry {
            name: "assessments".into(),
            data: PathBuf::from("data/assessments.yaml"),
            exemplar: false,
            schema: Some("cqa".into()),
        }
    }

    #[test]
    fn dependency_entries_publish_on_the_dependencys_page() {
        let repo = make_repo_with_dependency_data();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], None, "v0.2.0");
        cfg.instances.push(InstanceEntry {
            name: "catalog".into(),
            data: PathBuf::from("data/instances.yaml"),
            exemplar: true,
            schema: None,
        });
        cfg.instances.push(dep_entry());
        cfg.instances.push(InstanceEntry {
            name: "extra-assessments".into(),
            data: PathBuf::from("data/extra-assessments.yaml"),
            exemplar: false,
            schema: Some("cqa".into()),
        });
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");

        let dep_page = std::fs::read_to_string(out.path().join("cqa/v0.2.0/index.html"))
            .expect("the dependency page publishes under its own directory");
        assert!(
            dep_page.contains("ind-x1"),
            "every entry naming the dependency shares its one page"
        );
        assert!(
            dep_page.contains("Assessment"),
            "the dependency page renders the dependency's schema"
        );
        assert!(
            dep_page.contains("ind-a1"),
            "the dependency page embeds the entry's dataset"
        );
        assert!(
            dep_page.contains("external:"),
            "a reference leaving the dataset draws as an external node"
        );
        assert!(
            !dep_page.contains("v0.1.0"),
            "the dependency page's version dropdown offers only refs where the page exists"
        );

        assert!(
            !out.path().join("cqa/v0.1.0").exists(),
            "no dependency page is built for a ref that predates its data"
        );
        let dep_current = std::fs::read_to_string(out.path().join("cqa/current/index.html"))
            .expect("the dependency page gets a current/ alias when current is present there");
        assert!(dep_current.contains("ind-a1"));

        let own = std::fs::read_to_string(out.path().join("v0.2.0/index.html")).unwrap();
        assert!(
            own.contains("ind-morgon"),
            "the own page keeps its own datasets"
        );
        assert!(
            !own.contains("ind-a1"),
            "a dataset published to a dependency page leaves the own page"
        );
        assert!(
            out.path().join("v0.1.0/index.html").exists(),
            "the own page still builds at every ref"
        );
    }

    #[test]
    fn dependency_page_composes_independently_of_the_own_page() {
        let repo = make_repo_with_dependency_data();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.2.0"], None, "v0.2.0");
        cfg.instances.push(dep_entry());
        cfg.publishing.as_mut().unwrap().pages.insert(
            "cqa".into(),
            PageConfig {
                dir: None,
                layout: None,
                schema_sections: Some(false),
            },
        );
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");

        let own = std::fs::read_to_string(out.path().join("v0.2.0/index.html")).unwrap();
        assert!(
            own.contains(r#"<section id="classes">"#),
            "the own page keeps its schema sections"
        );
        let dep_page = std::fs::read_to_string(out.path().join("cqa/v0.2.0/index.html")).unwrap();
        assert!(
            !dep_page.contains(r#"<section id="classes">"#),
            "pages.cqa.schema_sections = false strips the dependency page's schema reference"
        );
        assert!(dep_page.contains("ind-a1"));

        // A configured `dir` moves the page inside the output tree, and a
        // per-page layout leads with the instance graph.
        cfg.publishing.as_mut().unwrap().pages.insert(
            "cqa".into(),
            PageConfig {
                dir: Some("contracts".into()),
                layout: Some(crate::html_writer::PageLayout::InstancesFirst),
                schema_sections: None,
            },
        );
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");
        assert!(
            !out.path().join("cqa").exists(),
            "a configured dir replaces the dependency-name default"
        );
        let dep_page =
            std::fs::read_to_string(out.path().join("contracts/v0.2.0/index.html")).unwrap();
        let instances_at = dep_page
            .find(r#"<section id="individuals">"#)
            .expect("instance section renders");
        let classes_at = dep_page
            .find(r#"<section id="classes">"#)
            .expect("schema sections render by default");
        assert!(
            instances_at < classes_at,
            "pages.cqa.layout = instances-first leads with the instance graph"
        );
    }

    #[test]
    fn a_dependency_page_absent_at_current_skips_its_alias() {
        let repo = make_repo_with_dependency_data();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], None, "v0.1.0");
        cfg.instances.push(dep_entry());
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");

        assert!(
            out.path().join("current/index.html").exists(),
            "the own page's alias still points at the configured current"
        );
        assert!(
            !out.path().join("cqa/current").exists(),
            "a page absent at the configured current publishes without an alias"
        );
        assert!(out.path().join("cqa/v0.2.0/index.html").exists());
    }

    #[test]
    fn publishing_to_an_undeclared_dependency_is_a_config_error() {
        let repo = make_repo_with_dependency_data();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.2.0"], None, "v0.2.0");
        let mut entry = dep_entry();
        entry.schema = Some("nimbly".into());
        cfg.instances.push(entry);
        let out = tempfile::tempdir().unwrap();
        let err = publish_versioned(repo.path(), &cfg, out.path(), false)
            .expect_err("an undeclared dependency must fail the publish");
        let msg = err.to_string();
        assert!(
            msg.contains("nimbly") && msg.contains("assessments"),
            "the error names the entry and the missing dependency; got: {msg}"
        );
    }

    #[test]
    fn edge_worktree_build_renders_the_working_tree_data() {
        let repo = make_repo_with_instance_data();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.2.0"], Some("main"), "v0.2.0");
        cfg.instances.push(InstanceEntry {
            name: "catalog".into(),
            data: PathBuf::from("data/instances.yaml"),
            exemplar: true,
            schema: None,
        });
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), true).expect("publish succeeds");
        let edge = std::fs::read_to_string(out.path().join("main/index.html")).unwrap();
        assert!(
            edge.contains("ind-morgon"),
            "the edge build must render the working tree's data file"
        );
    }

    #[test]
    fn a_non_conforming_a_box_is_reported_but_still_publishes() {
        // Conformance violations are a note, not an abort: aborting would make
        // an already-tagged version unpublishable because of data committed at
        // that ref. The page still builds, carrying the data as authored.
        let repo = make_repo_with_instance_data();
        std::fs::write(
            repo.path().join("data/instances.yaml"),
            "wines:\n  - id: morgon\n    name: Morgon\n  - id: morgon\n    name: Morgon Again\n",
        )
        .unwrap();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.2.0"], Some("main"), "v0.2.0");
        cfg.instances.push(InstanceEntry {
            name: "catalog".into(),
            data: PathBuf::from("data/instances.yaml"),
            exemplar: true,
            schema: None,
        });
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), true)
            .expect("a violation must not fail the publish");
        let edge = std::fs::read_to_string(out.path().join("main/index.html")).unwrap();
        assert!(
            edge.contains("ind-morgon"),
            "the page still renders the A-box it was given"
        );
    }

    #[test]
    fn publish_embeds_every_declared_instance_graph_with_the_exemplar_open() {
        // Two curated datasets declared, the second marked exemplar: both
        // must reach the published page, with the exemplar the one that opens.
        let repo = make_repo_with_instance_data();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.2.0"], None, "v0.2.0");
        cfg.instances.push(InstanceEntry {
            name: "preview".into(),
            data: PathBuf::from("data/preview.yaml"),
            exemplar: false,
            schema: None,
        });
        cfg.instances.push(InstanceEntry {
            name: "catalog".into(),
            data: PathBuf::from("data/instances.yaml"),
            exemplar: true,
            schema: None,
        });
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");
        let html = std::fs::read_to_string(out.path().join("v0.2.0/index.html")).unwrap();

        assert!(
            html.contains(">preview") && html.contains(">catalog"),
            "both declared datasets must be offered by name"
        );
        assert!(
            html.contains("ind-previewWine") && html.contains("ind-morgon"),
            "both datasets' individuals must render"
        );
        assert!(
            html.contains(r#"data-instance-dataset="0" hidden>"#),
            "the non-exemplar dataset's panel starts hidden"
        );
        assert!(
            html.contains(r#"data-instance-dataset="1">"#),
            "the exemplar is the dataset that opens"
        );
        assert!(
            !html.contains("declared but not published"),
            "nothing is left unpublished, so nothing should say so"
        );
    }

    #[test]
    fn publish_skips_a_dataset_absent_at_a_ref_and_keeps_the_rest() {
        // `data/preview.yaml` lands only in v0.2.0, so publishing v0.1.0 has
        // to skip it rather than fail — and still carry what does exist.
        let repo = make_repo_with_instance_data();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], None, "v0.2.0");
        cfg.instances.push(InstanceEntry {
            name: "preview".into(),
            data: PathBuf::from("data/preview.yaml"),
            exemplar: false,
            schema: None,
        });
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");

        let old_version = std::fs::read_to_string(out.path().join("v0.1.0/index.html")).unwrap();
        assert!(
            old_version.contains("No individuals defined in this ontology."),
            "a ref predating the data file publishes without that instance graph"
        );
        let new_version = std::fs::read_to_string(out.path().join("v0.2.0/index.html")).unwrap();
        assert!(
            new_version.contains("ind-previewWine"),
            "the ref that has the file still renders it"
        );
    }

    #[test]
    fn worktree_build_without_the_data_file_publishes_without_the_exemplar() {
        // The data file is committed for the tags but deleted from the
        // working tree: the edge/worktree build must note-and-skip, not
        // fail trying to read a missing file.
        let repo = make_repo_with_instance_data();
        std::fs::remove_file(repo.path().join("data/instances.yaml")).unwrap();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.2.0"], Some("main"), "v0.2.0");
        cfg.instances.push(InstanceEntry {
            name: "catalog".into(),
            data: PathBuf::from("data/instances.yaml"),
            exemplar: true,
            schema: None,
        });
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), true).expect("publish succeeds");
        let edge = std::fs::read_to_string(out.path().join("main/index.html")).unwrap();
        assert!(
            !edge.contains("ind-morgon"),
            "a missing working-tree data file publishes without the exemplar"
        );
    }

    #[test]
    fn publish_carries_the_exemplar_per_version() {
        let repo = make_repo_with_instance_data();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], None, "v0.2.0");
        cfg.instances.push(InstanceEntry {
            name: "catalog".into(),
            data: PathBuf::from("data/instances.yaml"),
            exemplar: true,
            schema: None,
        });
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");

        // v0.2.0's ref carries the data file → the page embeds the exemplar
        // (sidebar entry + individual card) with its provenance.
        let v02 = std::fs::read_to_string(out.path().join("v0.2.0/index.html")).unwrap();
        assert!(v02.contains("Instance Graph"), "sidebar entry present");
        assert!(v02.contains("ind-morgon"), "individual card present");
        assert!(v02.contains("instances.yaml"), "provenance names the file");

        // v0.1.0 predates the data file → published without an instance
        // graph, not failed.
        let v01 = std::fs::read_to_string(out.path().join("v0.1.0/index.html")).unwrap();
        assert!(
            !v01.contains("ind-morgon"),
            "a ref without the data file publishes without the exemplar"
        );
    }

    #[test]
    fn publish_versioned_writes_per_version_subdirs() {
        let repo = make_versioned_linkml_repo();
        let cfg = make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], None, "v0.2.0");
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");
        assert!(out.path().join("v0.1.0").is_dir());
        assert!(out.path().join("v0.2.0").is_dir());
        // HtmlWriter produces an index.html per version.
        assert!(out.path().join("v0.1.0/index.html").is_file());
        assert!(out.path().join("v0.2.0/index.html").is_file());
    }

    #[test]
    fn publish_versioned_builds_edge_when_configured() {
        let repo = make_versioned_linkml_repo();
        let cfg = make_publish_cfg_with_versions(vec!["v0.1.0"], Some("main"), "v0.1.0");
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");
        assert!(out.path().join("v0.1.0/index.html").is_file());
        assert!(out.path().join("main/index.html").is_file());
        // The edge schema declares an EdgeOnly class that v0.1.0 doesn't —
        // proves the HTML for `main` came from a different schema than v0.1.0.
        let edge_html = std::fs::read_to_string(out.path().join("main/index.html")).unwrap();
        assert!(edge_html.contains("EdgeOnly"));
        let v01_html = std::fs::read_to_string(out.path().join("v0.1.0/index.html")).unwrap();
        assert!(!v01_html.contains("EdgeOnly"));
    }

    #[test]
    fn publish_versioned_current_is_byte_copy_of_configured_version() {
        let repo = make_versioned_linkml_repo();
        let cfg = make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], None, "v0.2.0");
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");
        // current/ exists, contains an index.html byte-equal to v0.2.0's.
        let current_html = std::fs::read(out.path().join("current/index.html")).unwrap();
        let v02_html = std::fs::read(out.path().join("v0.2.0/index.html")).unwrap();
        assert_eq!(current_html, v02_html);
    }

    #[test]
    fn publish_versioned_current_can_alias_edge() {
        // Edge case: `current` points at `edge` instead of a released
        // version. Validated at parse time; orchestration must build
        // edge's output and copy it into current/.
        let repo = make_versioned_linkml_repo();
        let cfg = make_publish_cfg_with_versions(vec!["v0.1.0"], Some("main"), "main");
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");
        let current = std::fs::read(out.path().join("current/index.html")).unwrap();
        let edge = std::fs::read(out.path().join("main/index.html")).unwrap();
        assert_eq!(current, edge);
    }

    #[test]
    fn publish_versioned_combines_unresolved_refs_into_one_error() {
        let repo = make_versioned_linkml_repo();
        let cfg = make_publish_cfg_with_versions(
            vec!["v0.1.0", "v9.9.9"],
            Some("not-a-real-branch"),
            "v0.1.0",
        );
        let out = tempfile::tempdir().unwrap();
        let err = publish_versioned(repo.path(), &cfg, out.path(), false).expect_err("bad refs");
        match err {
            PublishError::RefsUnresolvable { refs, .. } => {
                assert!(refs.contains(&"v9.9.9".to_string()));
                assert!(refs.contains(&"not-a-real-branch".to_string()));
            }
            other => panic!("expected RefsUnresolvable, got {other:?}"),
        }
        // No partial state: outputs for any version must not exist.
        assert!(!out.path().join("v0.1.0").exists());
    }

    #[test]
    fn publish_versioned_injects_version_dropdown_into_each_page() {
        // Every per-version page must carry the dropdown listing all
        // versions in the cohort (edge first, then released versions
        // in manifest order). The dropdown's selected option matches
        // the page's own version.
        let repo = make_versioned_linkml_repo();
        let cfg = make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], Some("main"), "v0.2.0");
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");

        for page in ["v0.1.0", "v0.2.0", "main"] {
            let html = std::fs::read_to_string(out.path().join(page).join("index.html")).unwrap();
            // The dropdown is present.
            assert!(
                html.contains(r#"id="version-select""#),
                "page {page} missing version-select dropdown"
            );
            // Every cohort member shows up as an option.
            for v in ["v0.1.0", "v0.2.0", "main"] {
                assert!(
                    html.contains(&format!(r#"value="{v}""#)),
                    "page {page} missing option for {v}"
                );
            }
            // The page's own version is the selected one.
            assert!(
                html.contains(&format!(r#"value="{page}" selected"#)),
                "page {page} should have its own version selected; html excerpt did not match"
            );
        }
    }

    #[test]
    fn publish_versioned_header_brand_link_defaults_to_parent_current() {
        // The manifest's `site_root_url` defaults to `../current/` —
        // parent-relative within the publish output, symmetric with
        // `url_pattern`'s `../{version}/` default. It points each
        // per-version page's brand link at the canonical current
        // version's docs without making any assumption about a
        // containing parent site.
        let repo = make_versioned_linkml_repo();
        let cfg = make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], Some("main"), "v0.2.0");
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");

        for page in ["v0.1.0", "v0.2.0", "main"] {
            let html = std::fs::read_to_string(out.path().join(page).join("index.html")).unwrap();
            assert!(
                html.contains(r#"<a href="../current/" class="site-title""#),
                "page {page} must carry the default parent-relative brand link"
            );
            assert!(
                !html.contains(r#"<a href="/" class="site-title""#),
                "page {page} still has the absolute brand link"
            );
        }
    }

    #[test]
    fn publish_versioned_user_supplied_site_root_url_survives_to_rendered_html() {
        // Consumers whose publish output is nested under a parent site
        // (e.g. scimantic-schema lays the publish dir under a book at
        // `<book>/schema/<version>/`) override `site_root_url` in the
        // manifest. The value is emitted verbatim as the brand link.
        let repo = make_versioned_linkml_repo();
        let mut cfg =
            make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], Some("main"), "v0.2.0");
        cfg.publishing.as_mut().unwrap().site_root_url = "../../".into();
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");

        for page in ["v0.1.0", "v0.2.0", "main"] {
            let html = std::fs::read_to_string(out.path().join(page).join("index.html")).unwrap();
            assert!(
                html.contains(r#"<a href="../../" class="site-title""#),
                "page {page} must carry the user-supplied brand link"
            );
            assert!(
                !html.contains(r#"<a href="../current/" class="site-title""#),
                "default brand link must not leak when user supplies site_root_url"
            );
        }
    }

    /// `[publishing] site_title` names the site every page belongs to:
    /// the brand link carries it on the own page and every dependency
    /// page alike, one site identity in one place. Absent, each page's
    /// brand falls back to its own schema title, as before.
    #[test]
    fn site_title_brands_every_page_with_the_site_identity() {
        let repo = make_repo_with_dependency_data();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.2.0"], None, "v0.2.0");
        cfg.instances.push(dep_entry());
        cfg.publishing.as_mut().unwrap().site_title = Some("Building Wine".into());
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");

        for page in ["v0.2.0/index.html", "cqa/v0.2.0/index.html"] {
            let html = std::fs::read_to_string(out.path().join(page)).unwrap();
            assert!(
                html.contains(r#"class="site-title">Building Wine</a>"#),
                "page {page} must brand with the site title"
            );
        }
    }

    /// A relative `site_root_url` override is authored against the own
    /// page's version directory. A dependency page's version directory
    /// sits one segment deeper, so its brand link gains one `../` and
    /// both pages' links land on the same site root; an absolute value
    /// — root-relative or carrying a scheme — means the same
    /// destination at any depth and passes through unchanged, and a
    /// URL buried in a query never makes a relative value absolute.
    #[test]
    fn a_dependency_pages_brand_link_escapes_its_extra_depth() {
        let repo = make_repo_with_dependency_data();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.2.0"], None, "v0.2.0");
        cfg.instances.push(dep_entry());
        for (configured, own_expected, dep_expected) in [
            ("../../", "../../", "../../../"),
            (
                "../../?return=https://example.org/",
                "../../?return=https://example.org/",
                "../../../?return=https://example.org/",
            ),
            ("/book/", "/book/", "/book/"),
            (
                "https://example.org/book/",
                "https://example.org/book/",
                "https://example.org/book/",
            ),
        ] {
            cfg.publishing.as_mut().unwrap().site_root_url = configured.into();
            let out = tempfile::tempdir().unwrap();
            publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");
            let own = std::fs::read_to_string(out.path().join("v0.2.0/index.html")).unwrap();
            assert!(
                own.contains(&format!(r#"<a href="{own_expected}" class="site-title""#)),
                "own page for `{configured}` must emit `{own_expected}`"
            );
            let dep = std::fs::read_to_string(out.path().join("cqa/v0.2.0/index.html")).unwrap();
            assert!(
                dep.contains(&format!(r#"<a href="{dep_expected}" class="site-title""#)),
                "dependency page for `{configured}` must emit `{dep_expected}`"
            );
        }
    }

    /// A relative `site_root_url` override must climb out of the
    /// version directory: a value that does not begin with `../`
    /// resolves somewhere different on every page, so it is refused at
    /// parse rather than published broken. Root-relative and
    /// scheme-carrying values are absolute and pass.
    #[test]
    fn a_non_climbing_relative_site_root_url_is_refused() {
        let base = r#"[schema]
name = "s"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[publishing]
versions = ["v0.1.0"]
current = "v0.1.0"
site_root_url = "VALUE"
"#;
        for bad in [
            "",
            "./",
            "docs/",
            "index.html",
            "#top",
            "?v=2",
            "docs/v1:latest",
        ] {
            let toml = base.replace("VALUE", bad);
            let err = toml.parse::<PublishConfig>().unwrap_err().to_string();
            assert!(
                err.contains("site_root_url"),
                "`{bad}` must be refused naming the key; got: {err}"
            );
        }
        for good in [
            "../../",
            "../current/",
            "/site/",
            "https://example.org/book/",
            "mailto:owner@example.org",
        ] {
            let toml = base.replace("VALUE", good);
            assert!(
                toml.parse::<PublishConfig>().is_ok(),
                "`{good}` is climbing or absolute and must parse"
            );
        }
    }

    /// A blank `site_title` is no override: the brand falls back to
    /// the schema title rather than rendering a nameless link.
    #[test]
    fn a_blank_site_title_falls_back_to_the_schema_title() {
        let repo = make_repo_with_dependency_data();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.2.0"], None, "v0.2.0");
        cfg.instances.push(dep_entry());
        cfg.publishing.as_mut().unwrap().site_title = Some("   ".into());
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");
        let own = std::fs::read_to_string(out.path().join("v0.2.0/index.html")).unwrap();
        let brand_at = own.find(r#"class="site-title">"#).expect("brand anchor");
        let text = &own[brand_at + r#"class="site-title">"#.len()..];
        let text = &text[..text.find("</a>").expect("anchor closes")];
        assert!(
            !text.trim().is_empty(),
            "a blank override must not produce a nameless brand link; got {text:?}"
        );
    }

    #[test]
    fn publish_versioned_renders_stale_banner_on_non_current_page() {
        // A page rendered for a version other than `current` must
        // surface the "you're viewing X; current is Y" banner with a
        // working link to current/. The current page itself does NOT
        // get the banner.
        //
        // Search for the rendered `<div class="version-banner ...">`
        // rather than the bare class name — the CSS rules in the same
        // template carry the class string regardless of whether the
        // banner is actually rendered.
        let repo = make_versioned_linkml_repo();
        let cfg = make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], None, "v0.2.0");
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");

        let stale = std::fs::read_to_string(out.path().join("v0.1.0/index.html")).unwrap();
        assert!(
            stale.contains(r#"<div class="version-banner version-banner-stale""#),
            "v0.1.0 page must render the stale-banner <div>"
        );
        assert!(
            stale.contains("You're viewing v0.1.0"),
            "stale banner must name the viewing version"
        );
        assert!(
            stale.contains("../v0.2.0/"),
            "stale banner link must point at current via parent-relative URL"
        );

        let current = std::fs::read_to_string(out.path().join("v0.2.0/index.html")).unwrap();
        assert!(
            !current.contains(r#"<div class="version-banner version-banner-stale""#),
            "current page must NOT render the stale banner"
        );
    }

    #[test]
    fn publish_versioned_renders_edge_banner_on_edge_page() {
        // Page rendered for the edge ref gets a distinct "edge build
        // from HEAD" banner, not the stale banner.
        let repo = make_versioned_linkml_repo();
        let cfg = make_publish_cfg_with_versions(vec!["v0.1.0"], Some("main"), "v0.1.0");
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");

        let edge_html = std::fs::read_to_string(out.path().join("main/index.html")).unwrap();
        assert!(edge_html.contains(r#"<div class="version-banner version-banner-edge""#));
        assert!(edge_html.contains("edge build from HEAD"));
        // The edge page should NOT also carry the stale banner.
        assert!(!edge_html.contains(r#"<div class="version-banner version-banner-stale""#));
    }

    #[test]
    fn edge_from_worktree_reflects_uncommitted_edits() {
        // Committed v0.2.0 says `class: Thing`; we mutate the working
        // tree to add `EdgeOnly` after commit, run publish with the
        // flag, and the edge output must reflect the working tree.
        let repo = make_versioned_linkml_repo();
        // Replace schema.yaml with a version that adds a class only
        // visible in the worktree, NOT in any commit.
        let worktree_only_schema = "id: https://example.org/worktree\n\
             name: fixture_worktree\n\
             version: 0.3.0-wt\n\
             prefixes:\n  schema: https://example.org/\n\
             default_prefix: schema\n\
             classes:\n  Thing:\n    description: a thing\n  WorktreeOnly:\n    description: only on disk\n";
        std::fs::write(repo.path().join("schema.yaml"), worktree_only_schema).unwrap();

        let cfg = make_publish_cfg_with_versions(vec!["v0.1.0"], Some("main"), "v0.1.0");
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), true).expect("publish succeeds");

        let edge_html = std::fs::read_to_string(out.path().join("main/index.html")).unwrap();
        assert!(
            edge_html.contains("WorktreeOnly"),
            "edge build with --edge-from-worktree must reflect the working tree"
        );
    }

    #[test]
    fn edge_from_worktree_off_reads_last_commit_despite_dirty_worktree() {
        // Default behavior (flag off): even with a dirty worktree, the
        // edge build reads from the committed main branch via
        // `git show`. Required for CI reproducibility.
        let repo = make_versioned_linkml_repo();
        std::fs::write(
            repo.path().join("schema.yaml"),
            "id: https://example.org/wt\nname: x\nversion: 0.9.9\nclasses:\n  WorktreeOnly: {}\n",
        )
        .unwrap();

        let cfg = make_publish_cfg_with_versions(vec!["v0.1.0"], Some("main"), "v0.1.0");
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");

        let edge_html = std::fs::read_to_string(out.path().join("main/index.html")).unwrap();
        // The committed main HEAD declares EdgeOnly; the worktree
        // garbage we wrote should not leak through.
        assert!(
            edge_html.contains("EdgeOnly"),
            "default path must reflect committed main, not worktree"
        );
        assert!(
            !edge_html.contains("WorktreeOnly"),
            "default path must NOT reflect dirty worktree"
        );
    }

    #[test]
    fn edge_from_worktree_does_not_affect_tagged_versions() {
        // Tagged versions always come from their git refs. Mutating
        // the working tree must not bleed into v0.1.0's output even
        // when `--edge-from-worktree` is set.
        let repo = make_versioned_linkml_repo();
        std::fs::write(
            repo.path().join("schema.yaml"),
            "id: https://example.org/wt\nname: x\nversion: 0.9.9\nclasses:\n  WorktreeOnly: {}\n",
        )
        .unwrap();

        let cfg = make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], Some("main"), "v0.1.0");
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), true).expect("publish succeeds");

        // v0.1.0 and v0.2.0 must reflect their committed content.
        for ref_ in ["v0.1.0", "v0.2.0"] {
            let html = std::fs::read_to_string(out.path().join(ref_).join("index.html")).unwrap();
            assert!(
                !html.contains("WorktreeOnly"),
                "{ref_} must come from its git ref, not the dirty worktree"
            );
        }
    }

    #[test]
    fn edge_from_worktree_does_not_require_edge_ref_to_resolve() {
        // The edge ref name is just a label for the output subdir when
        // working-tree mode is active — `git rev-parse` on it is
        // unnecessary and could fail (e.g. a developer working on a
        // branch that hasn't been pushed yet). Verify a non-existent
        // edge ref name succeeds when `--edge-from-worktree` is set.
        let repo = make_versioned_linkml_repo();
        let cfg = make_publish_cfg_with_versions(
            vec!["v0.1.0"],
            Some("feature-branch-that-doesnt-exist"),
            "v0.1.0",
        );
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), true).expect("publish succeeds");
        assert!(
            out.path()
                .join("feature-branch-that-doesnt-exist/index.html")
                .is_file(),
            "edge subdir is named by the ref label even when working-tree mode bypasses ref resolution"
        );
    }

    #[test]
    fn dependency_page_config_parses_and_validates() {
        let base = r#"[schema]
name = "s"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[[instances]]
name = "wine-cqa"
data = "data/wine-cqa.yaml"
schema = "cqa"

[publishing]
versions = ["v0.1.0"]
current = "v0.1.0"

[publishing.pages.cqa]
layout = "instances-first"
schema_sections = false
"#;
        let cfg: PublishConfig = base.parse().expect("dependency page config parses");
        assert_eq!(cfg.instances[0].schema.as_deref(), Some("cqa"));
        let pages = &cfg.publishing.as_ref().unwrap().pages;
        assert_eq!(
            pages["cqa"].layout,
            Some(crate::html_writer::PageLayout::InstancesFirst)
        );
        assert_eq!(pages["cqa"].schema_sections, Some(false));

        let orphan = base.replace("[publishing.pages.cqa]", "[publishing.pages.nimbly]");
        let err = orphan.parse::<PublishConfig>().unwrap_err().to_string();
        assert!(
            err.contains("nimbly"),
            "a page for a dependency no entry names is a config error; got: {err}"
        );

        let bad_dir = base.replace(
            "layout = \"instances-first\"",
            "dir = \"../escape\"\nlayout = \"instances-first\"",
        );
        let err = bad_dir.parse::<PublishConfig>().unwrap_err().to_string();
        assert!(
            err.contains("../escape"),
            "a page dir must be a single path segment; got: {err}"
        );

        let reserved = base.replace(
            "layout = \"instances-first\"",
            "dir = \"current\"\nlayout = \"instances-first\"",
        );
        let err = reserved.parse::<PublishConfig>().unwrap_err().to_string();
        assert!(
            err.contains("current"),
            "`current` is the alias, never a page dir; got: {err}"
        );

        let colliding = base.replace(
            "layout = \"instances-first\"",
            "dir = \"v0.1.0\"\nlayout = \"instances-first\"",
        );
        let err = colliding.parse::<PublishConfig>().unwrap_err().to_string();
        assert!(
            err.contains("v0.1.0"),
            "a page dir colliding with a version label is a config error; got: {err}"
        );
    }

    #[test]
    fn default_page_dirs_pass_the_same_validation_as_configured_ones() {
        let base = r#"[schema]
name = "s"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[[instances]]
name = "records"
data = "data/records.yaml"
schema = "DEP"

[publishing]
versions = ["v0.1.0"]
current = "v0.1.0"
"#;
        for (dep, tell) in [
            ("../escape", "../escape"),
            ("current", "current"),
            ("v0.1.0", "v0.1.0"),
        ] {
            let toml = base.replace("DEP", dep);
            let err = toml.parse::<PublishConfig>().unwrap_err().to_string();
            assert!(
                err.contains(tell),
                "a dependency page's default directory (`{dep}`) must pass the segment \
                 checks even without a [publishing.pages] table; got: {err}"
            );
        }

        let colliding = format!(
            "{}\n[[instances]]\nname = \"more\"\ndata = \"data/more.yaml\"\nschema = \"b\"\n\n[publishing.pages.DEP]\ndir = \"b\"\n",
            base
        )
        .replace("DEP", "a");
        let err = colliding.parse::<PublishConfig>().unwrap_err().to_string();
        assert!(
            err.contains('b'),
            "a configured dir colliding with another dependency's default dir is an error; got: {err}"
        );
    }

    #[test]
    fn a_publish_without_dependency_entries_ignores_a_broken_manifest() {
        let repo = make_repo_with_instance_data();
        std::fs::write(repo.path().join("panschema.toml"), "not = valid = toml").unwrap();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.2.0"], None, "v0.2.0");
        cfg.instances.push(InstanceEntry {
            name: "catalog".into(),
            data: PathBuf::from("data/instances.yaml"),
            exemplar: true,
            schema: None,
        });
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false)
            .expect("no entry names a dependency, so the manifest is never consulted");
    }

    #[test]
    fn a_broken_manifest_error_names_the_manifest_not_a_version() {
        let repo = make_repo_with_dependency_data();
        std::fs::write(repo.path().join("panschema.toml"), "not = valid = toml").unwrap();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.2.0"], None, "v0.2.0");
        cfg.instances.push(dep_entry());
        let out = tempfile::tempdir().unwrap();
        let err = publish_versioned(repo.path(), &cfg, out.path(), false)
            .expect_err("a dependency entry needs the manifest, and this one is broken");
        let msg = err.to_string();
        assert!(
            msg.contains("panschema.toml") && !msg.contains("generate docs"),
            "the error says the manifest is unreadable, not that a version failed to \
             generate; got: {msg}"
        );
    }

    #[test]
    fn a_stale_alias_from_a_previous_run_is_removed_when_current_leaves_the_page() {
        let repo = make_repo_with_dependency_data();
        let out = tempfile::tempdir().unwrap();

        let mut cfg = make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], None, "v0.2.0");
        cfg.instances.push(dep_entry());
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("first publish");
        assert!(out.path().join("cqa/current/index.html").exists());

        let mut cfg = make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], None, "v0.1.0");
        cfg.instances.push(dep_entry());
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("second publish");
        assert!(
            !out.path().join("cqa/current").exists(),
            "a persistent output tree must not keep serving the previous run's alias \
             after current moved to a ref where the page does not exist"
        );
    }

    #[test]
    fn an_edge_label_matching_a_version_builds_once_with_the_edge_source() {
        let repo = make_repo_with_instance_data();
        std::fs::write(
            repo.path().join("data/instances.yaml"),
            "wines:\n  - id: worktreeWine\n    name: Worktree Wine\n",
        )
        .unwrap();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.2.0"], Some("v0.2.0"), "v0.2.0");
        cfg.instances.push(InstanceEntry {
            name: "catalog".into(),
            data: PathBuf::from("data/instances.yaml"),
            exemplar: true,
            schema: None,
        });
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), true).expect("publish succeeds");
        let page = std::fs::read_to_string(out.path().join("v0.2.0/index.html")).unwrap();
        assert!(
            page.contains("ind-worktreeWine"),
            "with --edge-from-worktree, the worktree build wins the shared label"
        );
        assert_eq!(
            page.matches(r#"<option value="v0.2.0""#).count(),
            1,
            "a label shared by edge and versions appears once in the dropdown"
        );
    }

    #[test]
    fn a_page_absent_at_current_treats_its_newest_release_as_current() {
        let repo = make_repo_with_dependency_data();
        let mut cfg =
            make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], Some("main"), "v0.1.0");
        cfg.instances.push(dep_entry());
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");

        let released = std::fs::read_to_string(out.path().join("cqa/v0.2.0/index.html")).unwrap();
        assert!(
            !released.contains(r#"<div class="version-banner version-banner-stale"#),
            "the released ref stands as the page's current, not the edge build"
        );
        let edge = std::fs::read_to_string(out.path().join("cqa/main/index.html")).unwrap();
        assert!(
            edge.contains(r#"<div class="version-banner version-banner-edge"#),
            "the edge build keeps its edge banner"
        );
        assert!(
            !released.contains(r#"href="../current/""#),
            "a page without a current/ alias must not link to one"
        );
    }

    #[test]
    fn a_ref_with_the_dependency_but_no_data_skips_its_page() {
        let repo = make_repo_with_dependency_data();
        let path = repo.path();
        run(path, &["rm", "--quiet", "data/assessments.yaml"]);
        run(path, &["commit", "-m", "retire the dataset", "--quiet"]);
        run(path, &["tag", "v0.3.0"]);
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.2.0", "v0.3.0"], None, "v0.2.0");
        cfg.instances.push(dep_entry());
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");
        assert!(out.path().join("cqa/v0.2.0/index.html").exists());
        assert!(
            !out.path().join("cqa/v0.3.0").exists(),
            "a ref where the dependency resolves but its data is gone gets no page"
        );
    }

    #[test]
    fn offline_tarballs_refuse_and_name_the_fix() {
        let mut sink: Vec<u8> = Vec::new();
        let err = crate::source::TarballSource::fetch(&OfflineTarballs, "o", "r", "v1", &mut sink)
            .expect_err("publish must never fetch over the network");
        assert!(
            err.to_string().contains("panschema fetch"),
            "the refusal names the fix; got: {err}"
        );
        assert!(sink.is_empty(), "nothing may be written to the cache");
    }

    #[test]
    fn published_pages_link_to_each_other_by_name() {
        let repo = make_repo_with_dependency_data();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], None, "v0.2.0");
        cfg.instances.push(InstanceEntry {
            name: "catalog".into(),
            data: PathBuf::from("data/instances.yaml"),
            exemplar: true,
            schema: None,
        });
        cfg.instances.push(dep_entry());
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");

        let own = std::fs::read_to_string(out.path().join("v0.2.0/index.html")).unwrap();
        assert!(
            own.contains(r#"href="../cqa/current/""#) && own.contains(">cqa<"),
            "the own page links to the dependency page by name"
        );
        assert!(
            own.contains(r#"aria-current="page""#) && own.contains(">fixture<"),
            "the page marks itself in the nav rather than linking to itself"
        );
        let dep_page = std::fs::read_to_string(out.path().join("cqa/v0.2.0/index.html")).unwrap();
        assert!(
            dep_page.contains(r#"href="../../current/""#) && dep_page.contains(">fixture<"),
            "the dependency page links back to the own page by schema name"
        );
    }

    #[test]
    fn a_sibling_without_an_alias_is_linked_at_its_page_current() {
        let repo = make_repo_with_dependency_data();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], None, "v0.1.0");
        cfg.instances.push(dep_entry());
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");
        let own = std::fs::read_to_string(out.path().join("v0.1.0/index.html")).unwrap();
        assert!(
            own.contains(r#"href="../cqa/v0.2.0/""#),
            "a sibling absent at current is linked at the version standing as its page current"
        );
        assert!(
            !own.contains(r#"href="../cqa/current/""#),
            "no link may point at an alias that does not exist"
        );
    }

    #[test]
    fn a_single_page_site_renders_no_page_nav() {
        let repo = make_repo_with_instance_data();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.2.0"], None, "v0.2.0");
        cfg.instances.push(InstanceEntry {
            name: "catalog".into(),
            data: PathBuf::from("data/instances.yaml"),
            exemplar: true,
            schema: None,
        });
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");
        let own = std::fs::read_to_string(out.path().join("v0.2.0/index.html")).unwrap();
        assert!(
            !own.contains("page-links"),
            "a site with one page has no other pages to offer"
        );
    }

    #[test]
    fn a_path_dependency_is_never_gated_by_a_stale_lock_entry() {
        // The manifest's parsed source decides whether a pin exists to
        // honor; a leftover `github:` lock entry from before the dep
        // switched to `path:` must not checksum the working tree.
        let repo = make_repo_with_dependency_data();
        std::fs::write(
            repo.path().join("panschema.lock"),
            concat!(
                "[[schema]]\n",
                "name = \"cqa\"\n",
                "version = \"0.1.0\"\n",
                "source = \"github:old-owner/cqa\"\n",
                "checksum = \"sha256:0000000000000000000000000000000000000000000000000000000000000000\"\n",
            ),
        )
        .unwrap();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.2.0"], Some("main"), "v0.2.0");
        cfg.instances.push(dep_entry());
        let out = tempfile::tempdir().unwrap();
        publish_versioned(repo.path(), &cfg, out.path(), true).expect("publish succeeds");
        assert!(
            out.path().join("cqa/main/index.html").exists(),
            "a path dependency carries no pin, so a stale lock entry cannot gate it"
        );
    }

    #[test]
    fn lockfile_drift_gates_cached_content_for_pinned_sources() {
        use crate::lockfile::{LockEntry, Lockfile, checksum_file};
        use crate::manifest::SchemaDep;
        let schema = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(schema.path(), "id: https://example.org/c\nname: c\n").unwrap();
        let good = checksum_file(schema.path()).unwrap();
        let dep_spec = SchemaDep {
            path: None,
            source: Some("github:o/r".into()),
            version: Some("0.1.0".into()),
        };
        let resolved = crate::source::Resolved {
            pkg_dir: schema.path().parent().unwrap().to_path_buf(),
            schema_path: schema.path().to_path_buf(),
            version: "0.1.0".into(),
            revision: None,
        };
        let lock = |version: &str, source: &str, checksum: &str| Lockfile {
            entries: vec![LockEntry {
                name: "cqa".into(),
                version: Some(version.into()),
                source: source.into(),
                revision: None,
                checksum: checksum.into(),
            }],
        };

        assert_eq!(
            lockfile_drift(
                &lock("0.1.0", "github:o/r", &good),
                "cqa",
                &dep_spec,
                &resolved
            ),
            None,
            "an entry agreeing with the manifest and the content passes the gate"
        );

        let drift = lockfile_drift(
            &lock("0.1.0", "github:o/r", "sha256:0000"),
            "cqa",
            &dep_spec,
            &resolved,
        )
        .expect("drifted content is refused");
        assert!(
            drift.contains("sha256:0000") && drift.contains(&good) && drift.contains("cache"),
            "the mismatch names both checksums and blames the cache side; got: {drift}"
        );

        let stale_version = lockfile_drift(
            &lock("0.9.9", "github:o/r", &good),
            "cqa",
            &dep_spec,
            &resolved,
        )
        .expect("a version disagreeing with the manifest is refused");
        assert!(
            stale_version.contains("0.9.9")
                && stale_version.contains("stale")
                && !stale_version.contains("edited cache"),
            "a stale lock is named as stale, never blamed on the cache; got: {stale_version}"
        );

        let stale_source = lockfile_drift(
            &lock("0.1.0", "github:other/r", &good),
            "cqa",
            &dep_spec,
            &resolved,
        )
        .expect("a source disagreeing with the manifest is refused");
        assert!(
            stale_source.contains("github:other/r") && stale_source.contains("github:o/r"),
            "the stale-source message names both specs; got: {stale_source}"
        );

        let other_dep = lock("0.1.0", "github:o/r", "sha256:0000");
        assert_eq!(
            lockfile_drift(&other_dep, "nimbly", &dep_spec, &resolved),
            None,
            "a dependency the lockfile does not record is ungated"
        );
    }

    #[test]
    fn publishing_layout_composes_the_page() {
        let repo = make_versioned_linkml_repo();
        let out = tempfile::tempdir().unwrap();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.2.0"], None, "v0.2.0");
        cfg.publishing.as_mut().unwrap().layout = crate::html_writer::PageLayout::InstancesFirst;
        cfg.publishing.as_mut().unwrap().schema_sections = false;
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");
        let html = std::fs::read_to_string(out.path().join("v0.2.0/index.html")).unwrap();
        assert!(
            !html.contains(r#"<section id="classes""#),
            "schema_sections = false omits the schema reference"
        );
        assert!(
            html.contains(r#"<section id="individuals""#),
            "the instance section renders"
        );

        // With the sections kept, the layout key decides the order.
        let out = tempfile::tempdir().unwrap();
        let mut cfg = make_publish_cfg_with_versions(vec!["v0.2.0"], None, "v0.2.0");
        cfg.publishing.as_mut().unwrap().layout = crate::html_writer::PageLayout::InstancesFirst;
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("publish succeeds");
        let html = std::fs::read_to_string(out.path().join("v0.2.0/index.html")).unwrap();
        let individuals = html
            .find(r#"<section id="individuals""#)
            .expect("instance section renders");
        let classes = html
            .find(r#"<section id="classes""#)
            .expect("schema sections render");
        assert!(
            individuals < classes,
            "instances-first leads the published page"
        );
    }

    #[test]
    fn publish_versioned_overwrites_existing_current_directory() {
        // Running publish twice in a row should produce the same
        // result — the second run's current/ overwrite must not leak
        // files from an earlier different `current` target.
        let repo = make_versioned_linkml_repo();
        let out = tempfile::tempdir().unwrap();

        // First run: current = v0.1.0
        let cfg = make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], None, "v0.1.0");
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("first publish");
        let after_first = std::fs::read(out.path().join("current/index.html")).unwrap();
        let v01 = std::fs::read(out.path().join("v0.1.0/index.html")).unwrap();
        assert_eq!(after_first, v01);

        // Second run with the same dir but current = v0.2.0
        let cfg = make_publish_cfg_with_versions(vec!["v0.1.0", "v0.2.0"], None, "v0.2.0");
        publish_versioned(repo.path(), &cfg, out.path(), false).expect("second publish");
        let after_second = std::fs::read(out.path().join("current/index.html")).unwrap();
        let v02 = std::fs::read(out.path().join("v0.2.0/index.html")).unwrap();
        assert_eq!(after_second, v02);
        // And it's not the v0.1.0 content from the first run.
        assert_ne!(after_second, after_first);
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

    #[test]
    fn publish_generation_resolves_local_imports() {
        // A published version must render the same resolved schema as
        // `generate`; the per-version generator previously read the root file
        // without resolving `imports:`, so imported elements were dropped.
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("common.yaml"),
            "id: https://example.org/common\nname: common\nclasses:\n  ImportedThing:\n    description: from the imported file\n",
        )
        .unwrap();
        let main = dir.path().join("main.yaml");
        std::fs::write(
            &main,
            "id: https://example.org/main\nname: main\nimports:\n  - common\nclasses:\n  RootThing:\n    description: root\n",
        )
        .unwrap();
        let out = dir.path().join("out");
        let cohort = CohortContext {
            all_versions: vec!["1.0.0".to_string()],
            current: "1.0.0".to_string(),
            edge: None,
            url_pattern: "../{version}/".to_string(),
            site_root_href: "../current/".to_string(),
            site_title: None,
            label_sources: std::collections::BTreeMap::new(),
            instances_first: false,
            schema_sections: true,
            page_links: Vec::new(),
        };

        generate_html_for_version("1.0.0", &main, &out, &cohort, &[])
            .expect("publish generation should succeed");
        let html = std::fs::read_to_string(out.join("index.html")).expect("index.html");
        assert!(
            html.contains("ImportedThing"),
            "published docs must include imported classes; `ImportedThing` was missing"
        );
    }
}
