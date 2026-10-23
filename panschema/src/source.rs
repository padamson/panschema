//! Source-spec dispatch and the [`TarballSource`] trait.
//!
//! [`SchemaSource`] is the parsed, validated form of a [`SchemaDep`] —
//! one variant per supported source protocol. Each command handler
//! converts the manifest's raw `SchemaDep` into a `SchemaSource` and
//! dispatches on the variant.
//!
//! Reference: [`docs/features/05-schema-manager.md`](../../docs/features/05-schema-manager.md)

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::manifest::SchemaDep;

/// The prefix a `path:` source spec carries in the lockfile's `source`
/// field. The one string both `source_spec` and the lockfile's reading of an
/// entry key on, so they cannot disagree about what a path entry looks like.
pub const PATH_SPEC_PREFIX: &str = "path:";

/// The `[schemas]` names split into pins and path sources, in name order. A
/// malformed entry is an error here rather than a third bucket: the caller
/// reports it before deciding anything else, exactly as `generate` does.
pub fn partition_pins(
    schemas: &std::collections::BTreeMap<String, SchemaDep>,
) -> Result<(Vec<&str>, Vec<&str>), SourceError> {
    let (mut pinned, mut unpinned) = (Vec::new(), Vec::new());
    for (name, dep) in schemas {
        if SchemaSource::from_dep(name, dep)?.is_pinned() {
            pinned.push(name.as_str());
        } else {
            unpinned.push(name.as_str());
        }
    }
    Ok((pinned, unpinned))
}

/// Validated source spec for one entry under `[schemas]`.
///
/// Both variants point at a "package" (directory containing
/// `panschema-publish.toml`); the variant just says how the package is
/// located.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaSource {
    /// `path = "./local-pkg"` — a directory on disk, relative to the
    /// manifest.
    Path { path: PathBuf },
    /// `source = "github:owner/repo"` + `version = "0.1.3"` — a tagged
    /// GitHub commit, fetched as a tarball and cached.
    Github {
        owner: String,
        repo: String,
        version: String,
    },
}

/// Errors raised during semantic validation of a [`SchemaDep`].
///
/// Serde catches structural problems (unknown fields, wrong types).
/// `SourceError` catches *combinational* problems — e.g. `path` and
/// `source` set together, or `source` without a `version`.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SourceError {
    #[error("schema `{0}`: must declare either `path` or `source` + `version`")]
    Empty(String),
    #[error("schema `{0}`: `path` and `source` are mutually exclusive")]
    PathAndSource(String),
    #[error("schema `{0}`: `source` requires a `version` field")]
    SourceWithoutVersion(String),
    #[error(
        "schema `{0}`: `version` is only valid alongside `source`; path sources \
         get their version from the package's `panschema-publish.toml` instead"
    )]
    VersionWithoutSource(String),
    #[error("schema `{name}`: unrecognized source protocol in `{spec}`")]
    UnknownProtocol { name: String, spec: String },
    #[error("schema `{name}`: malformed github source `{spec}`; expected `github:owner/repo`")]
    MalformedGithub { name: String, spec: String },
}

impl SchemaSource {
    /// Stable lockfile/representation string — e.g. `"path:./local-pkg"`
    /// or `"github:owner/repo"`.
    pub fn source_spec(&self) -> String {
        match self {
            Self::Path { path } => format!("{PATH_SPEC_PREFIX}{}", path.display()),
            Self::Github { owner, repo, .. } => format!("github:{owner}/{repo}"),
        }
    }

    /// Whether this source is a pin: a released version that the lockfile
    /// records and `fetch --check` verifies. A `path:` source is the working
    /// tree — its content is whatever is on disk right now, and pinning that
    /// would only report every edit as drift — so it is never a pin.
    pub fn is_pinned(&self) -> bool {
        matches!(self, Self::Github { .. })
    }

    /// [`is_pinned`](Self::is_pinned) for a spec string as the lockfile
    /// stores it, where the kind is only recoverable from the prefix. The
    /// two answers must agree for every spec `source_spec` can produce, so a
    /// new source kind changes both here or neither.
    pub fn spec_is_pinned(spec: &str) -> bool {
        !spec.starts_with(PATH_SPEC_PREFIX)
    }

    /// Tag string corresponding to this source's version, if any.
    /// For `github:` sources, this prepends `v` to the version.
    pub fn tag(&self) -> Option<String> {
        match self {
            Self::Path { .. } => None,
            Self::Github { version, .. } => Some(format!("v{version}")),
        }
    }

    /// Parse and validate a `SchemaDep`.
    pub fn from_dep(name: &str, dep: &SchemaDep) -> Result<Self, SourceError> {
        match (&dep.path, &dep.source, &dep.version) {
            (Some(path), None, None) => Ok(Self::Path { path: path.clone() }),
            (Some(_), Some(_), _) => Err(SourceError::PathAndSource(name.to_string())),
            (Some(_), None, Some(_)) => Err(SourceError::VersionWithoutSource(name.to_string())),
            (None, Some(spec), Some(version)) => Self::parse_remote(name, spec, version),
            (None, Some(_), None) => Err(SourceError::SourceWithoutVersion(name.to_string())),
            (None, None, _) => Err(SourceError::Empty(name.to_string())),
        }
    }

    fn parse_remote(name: &str, spec: &str, version: &str) -> Result<Self, SourceError> {
        if let Some(rest) = spec.strip_prefix("github:") {
            let (owner, repo) =
                rest.split_once('/')
                    .ok_or_else(|| SourceError::MalformedGithub {
                        name: name.to_string(),
                        spec: spec.to_string(),
                    })?;
            if owner.is_empty() || repo.is_empty() || repo.contains('/') {
                return Err(SourceError::MalformedGithub {
                    name: name.to_string(),
                    spec: spec.to_string(),
                });
            }
            Ok(Self::Github {
                owner: owner.to_string(),
                repo: repo.to_string(),
                version: version.to_string(),
            })
        } else {
            Err(SourceError::UnknownProtocol {
                name: name.to_string(),
                spec: spec.to_string(),
            })
        }
    }
}

/// Pluggable tarball fetcher — fetches the gzipped-tar bytes for a
/// (owner, repo, tag) triple and writes them to the given sink.
///
/// Production uses [`CodeloadGithubSource`] (hits `codeload.github.com`
/// via `ureq`). Tests substitute a local-fixture impl so the test
/// suite has no HTTP dependencies.
pub trait TarballSource {
    /// Download the tarball for `<owner>/<repo>` at tag `<tag>` into `sink`.
    ///
    /// `tag` is the full tag name *including* any `v` prefix
    /// (e.g. `"v0.1.3"`) — caller is responsible for choosing the
    /// exact tag scheme.
    fn fetch(
        &self,
        owner: &str,
        repo: &str,
        tag: &str,
        sink: &mut dyn Write,
    ) -> Result<(), TarballFetchError>;
}

/// Errors raised by [`TarballSource::fetch`] implementations.
#[derive(Debug, thiserror::Error)]
pub enum TarballFetchError {
    #[error("tag `{tag}` not found for {owner}/{repo}")]
    TagNotFound {
        owner: String,
        repo: String,
        tag: String,
    },
    #[error("network error fetching {owner}/{repo}@{tag}: {source}")]
    Network {
        owner: String,
        repo: String,
        tag: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("I/O error writing tarball: {0}")]
    Io(#[from] std::io::Error),
}

/// Production [`TarballSource`]: fetches anonymously from
/// `https://codeload.github.com/<owner>/<repo>/tar.gz/refs/tags/<tag>`.
///
/// No GitHub API calls, no auth — this stays well inside the 60/hr
/// anonymous limit and works for any public repo.
pub struct CodeloadGithubSource;

/// A dataset a package publishes: the name a consumer refers to it by,
/// where its data actually lives, and the schema it conforms to when that
/// is a dependency rather than the package's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedDataset {
    /// The `[[instances]]` entry's `name`.
    pub name: String,
    /// Absolute path to the data file, resolved against the package
    /// directory the publish manifest sits in. Not yet confined to the
    /// package: [`resolve_named_datasets`] refuses one that leads out of it
    /// before a consumer reads it.
    pub data: PathBuf,
    /// The `data` path as the publish manifest writes it, for messages:
    /// `data` cannot always give it back, since joining a Windows verbatim
    /// path resolves `..` on the spot.
    pub written: PathBuf,
    /// The `[schemas.<dep>]` dependency this dataset conforms to, when the
    /// publish entry names one. `None` means the package's own schema.
    pub schema: Option<String>,
}

impl PublishedDataset {
    fn from_entries(pkg_dir: &Path, entries: &[crate::publish::InstanceEntry]) -> Vec<Self> {
        entries
            .iter()
            .map(|entry| PublishedDataset {
                name: entry.name.clone(),
                data: pkg_dir.join(&entry.data),
                written: entry.data.clone(),
                schema: entry.schema.clone(),
            })
            .collect()
    }
}

/// Resolved schema dependency: the canonical package directory, the
/// on-disk path to the schema's main file, the version declared in
/// `panschema-publish.toml`, and (for remote sources) a revision to
/// record in the lockfile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Canonical absolute path to the package directory (the directory
    /// containing `panschema-publish.toml`). Callers that need to read
    /// other files from the package — like `panschema-publish.toml`
    /// itself — must use this rather than `schema_path.parent()`, which
    /// is only the package root when `[files].main` lives at the root.
    pub pkg_dir: PathBuf,
    /// Absolute path to the main schema file.
    pub schema_path: PathBuf,
    /// Version declared in `panschema-publish.toml`. Always populated —
    /// both source types are now "packages" with a publish file.
    pub version: String,
    /// The package name its publish manifest declares.
    pub published_name: String,
    /// The datasets its publish manifest lists (`[[instances]]`), in
    /// declaration order. A consumer names one of these instead of
    /// pathing into the package.
    pub datasets: Vec<PublishedDataset>,
    /// Reserved for future commit-identifier provenance. Currently
    /// always `None`: `path:` sources have no commit; `github:` sources
    /// use a tag URL that doesn't expose a commit SHA without an extra
    /// API call we don't make.
    pub revision: Option<String>,
}

/// Errors raised while resolving a dataset a consumer named.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DatasetError {
    #[error(
        "[generate.{entry}]: `{dep}` publishes no dataset named `{name}`{}",
        if available.is_empty() {
            String::from("; it publishes none")
        } else {
            format!("; it publishes {}", available.join(", "))
        }
    )]
    Unknown {
        entry: String,
        dep: String,
        name: String,
        available: Vec<String>,
    },
    #[error(
        "[generate.{entry}]: `{dep}`'s dataset `{name}` conforms to schema `{schema}`, \
         not `{entry}` — {}",
        match block {
            Some(block) => format!("name it under [generate.{block}], as `{dep_key}:{name}`"),
            None => format!(
                "this manifest declares no [schemas] entry for `{schema}`, so there is no \
                 block it belongs under"
            ),
        }
    )]
    WrongSchema {
        entry: String,
        /// The publishing package's name, for reading.
        dep: String,
        /// The consumer's `[schemas]` key for it — the qualifier that
        /// actually resolves, which is not always the published name.
        dep_key: String,
        name: String,
        schema: String,
        /// The consumer's key for the schema this dataset conforms to, when
        /// it declares one.
        block: Option<String>,
    },
    #[error(
        "[generate.{entry}]: `{name}` names dependency `{dep}`, which no [schemas] entry \
         declares; declared are {}",
        declared.join(", ")
    )]
    UnknownDependency {
        entry: String,
        dep: String,
        name: String,
        declared: Vec<String>,
    },
    #[error(
        "[generate.{entry}]: `{dep}`'s dataset `{name}` names `{}`, which does not resolve \
         inside the package",
        data.display()
    )]
    OutsidePackage {
        entry: String,
        dep: String,
        name: String,
        /// The `data` path as the publish manifest writes it.
        data: PathBuf,
    },
    #[error("[generate.{entry}]: `{dep}`'s dataset `{name}` at `{}`: {message}", data.display())]
    Unreadable {
        entry: String,
        dep: String,
        name: String,
        /// The `data` path as the publish manifest writes it.
        data: PathBuf,
        message: String,
    },
}

impl Resolved {
    /// The dataset this package publishes under `name`.
    pub fn dataset(&self, name: &str) -> Option<&PublishedDataset> {
        self.datasets.iter().find(|d| d.name == name)
    }

    /// The names it publishes, for an error that has to say what was on
    /// offer.
    pub fn dataset_names(&self) -> Vec<String> {
        self.datasets.iter().map(|d| d.name.clone()).collect()
    }
}

/// A dataset some package declares, as a generated graph names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredDataset {
    /// The `[[instances]]` entry's `name`.
    pub name: String,
    /// The declaring package's `[schema] version`, which is the dataset's
    /// version: the package ships its data at the version it releases.
    pub version: String,
    /// The consumer's `[schemas]` key for the declaring package.
    pub dep_key: String,
    /// The schema the `[[instances]]` entry says the data conforms to, by
    /// the publisher's key; `None` means the declaring package's own.
    pub schema: Option<String>,
}

/// Every data file the resolved packages declare in `[[instances]]`, keyed
/// by where the file sits on disk, so a file a `[generate]` entry reaches
/// by a path list or by name finds the dataset it is.
#[derive(Debug, Default)]
pub struct DeclaredDatasets {
    by_file: std::collections::BTreeMap<PathBuf, DeclaredDataset>,
}

impl DeclaredDatasets {
    /// Several manifest keys can resolve to one package (an alias per
    /// dataset, each `path = "."`), so a file is recorded once; were two
    /// packages to declare one file, the first in key order names it.
    ///
    /// A data path that leads out of its package declares nothing here, as
    /// [`resolve_named_datasets`] refuses to resolve one by name.
    pub fn from_resolved(resolved: &std::collections::BTreeMap<String, Resolved>) -> Self {
        let mut by_file = std::collections::BTreeMap::new();
        for (dep_key, package) in resolved {
            for dataset in &package.datasets {
                if !crate::cache::lies_within(&package.pkg_dir, &dataset.data).unwrap_or(false) {
                    continue;
                }
                by_file
                    .entry(on_disk(&dataset.data))
                    .or_insert_with(|| DeclaredDataset {
                        name: dataset.name.clone(),
                        version: package.version.clone(),
                        dep_key: dep_key.clone(),
                        schema: dataset.schema.clone(),
                    });
            }
        }
        Self { by_file }
    }

    /// The dataset `file` is, or `None` when no resolved package declares it.
    pub fn of(&self, file: &Path) -> Option<&DeclaredDataset> {
        self.by_file.get(&on_disk(file))
    }

    /// Refuse a declared dataset that `[generate.<entry>]` or
    /// `[check.<entry>]` lists by path although its package says it
    /// conforms to another schema, as [`resolve_named_datasets`] refuses it
    /// by name, so a graph never states `dcterms:conformsTo` a schema its
    /// publisher contradicts. A file no package declares passes.
    pub fn check_listed(
        &self,
        entry: &str,
        resolved: &std::collections::BTreeMap<String, Resolved>,
        files: &[PathBuf],
    ) -> Result<(), Box<DatasetError>> {
        for declared in files.iter().filter_map(|file| self.of(file)) {
            require_block(
                entry,
                resolved,
                &declared.dep_key,
                &declared.name,
                declared.schema.as_deref(),
            )?;
        }
        Ok(())
    }
}

/// Refuse dataset `name`, declared by the package at `dep_key`, unless
/// `[generate.<entry>]` is the block for the schema it conforms to: the
/// one its publish entry names, or with none named its own package's.
///
/// The publish entry names that schema by the *publisher's* manifest key,
/// which the consumer need not spell the same way, so the entry's key, the
/// published name of the entry's package, or (for a package's own data)
/// the same package under another key all count as a match.
fn require_block(
    entry: &str,
    resolved: &std::collections::BTreeMap<String, Resolved>,
    dep_key: &str,
    name: &str,
    schema: Option<&str>,
) -> Result<(), Box<DatasetError>> {
    let conforms_to = schema.unwrap_or(dep_key);
    let own = resolved.get(entry);
    let dep = resolved.get(dep_key);
    let same_package = schema.is_none()
        && own
            .zip(dep)
            .is_some_and(|(own, dep)| own.pkg_dir == dep.pkg_dir);
    if conforms_to == entry || own.is_some_and(|r| r.published_name == conforms_to) || same_package
    {
        return Ok(());
    }
    Err(Box::new(DatasetError::WrongSchema {
        entry: entry.to_string(),
        dep: dep
            .map_or(dep_key, |d| d.published_name.as_str())
            .to_string(),
        dep_key: dep_key.to_string(),
        name: name.to_string(),
        schema: conforms_to.to_string(),
        block: block_for(resolved, conforms_to),
    }))
}

/// The consumer's key for the block a dataset conforming to `schema`
/// belongs under, spelled the consumer's way.
fn block_for(
    resolved: &std::collections::BTreeMap<String, Resolved>,
    schema: &str,
) -> Option<String> {
    resolved
        .iter()
        .find(|(key, r)| key.as_str() == schema || r.published_name == schema)
        .map(|(key, _)| key.clone())
}

/// One spelling per file: the canonical path when the file exists, the
/// path as given when it does not (it then matches only the same spelling).
fn on_disk(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Resolve the dataset names a `[generate.<entry>]` block declares against
/// the dependency that block is about, to the data files they name.
///
/// A dataset whose publish entry names a different schema belongs under
/// that schema's block: the package already states which schema its data
/// conforms to, so a consumer that puts it elsewhere is asking for data to
/// be rendered against a schema it does not conform to.
pub fn resolve_named_datasets(
    entry: &str,
    resolved: &std::collections::BTreeMap<String, Resolved>,
    names: &[String],
) -> Result<Vec<PathBuf>, Box<DatasetError>> {
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        // `<dep>:<name>` names the package that publishes the dataset; a bare
        // name means this entry's own dependency.
        let (dep_key, dataset_name) = match name.split_once(':') {
            Some((dep, dataset)) => (dep, dataset),
            None => (entry, name.as_str()),
        };
        let Some(dep) = resolved.get(dep_key) else {
            return Err(Box::new(DatasetError::UnknownDependency {
                entry: entry.to_string(),
                dep: dep_key.to_string(),
                name: name.clone(),
                declared: resolved.keys().cloned().collect(),
            }));
        };
        let Some(dataset) = dep.dataset(dataset_name) else {
            return Err(Box::new(DatasetError::Unknown {
                entry: entry.to_string(),
                dep: dep.published_name.clone(),
                name: dataset_name.to_string(),
                available: dep.dataset_names(),
            }));
        };
        require_block(
            entry,
            resolved,
            dep_key,
            dataset_name,
            dataset.schema.as_deref(),
        )?;
        let written = dataset.written.clone();
        match crate::cache::lies_within(&dep.pkg_dir, &dataset.data) {
            Ok(true) => {}
            Ok(false) => {
                return Err(Box::new(DatasetError::OutsidePackage {
                    entry: entry.to_string(),
                    dep: dep.published_name.clone(),
                    name: dataset_name.to_string(),
                    data: written,
                }));
            }
            Err(e) => {
                return Err(Box::new(DatasetError::Unreadable {
                    entry: entry.to_string(),
                    dep: dep.published_name.clone(),
                    name: dataset_name.to_string(),
                    data: written,
                    message: e.to_string(),
                }));
            }
        }
        out.push(dataset.data.clone());
    }
    Ok(out)
}

/// Errors raised while resolving a [`SchemaSource`].
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error(
        "schema `{name}`: package path `{}` (resolved to `{}`) does not exist",
        path.display(), resolved.display()
    )]
    PathMissing {
        name: String,
        path: PathBuf,
        resolved: PathBuf,
    },
    #[error(
        "schema `{name}`: `panschema-publish.toml` is missing in package `{}`",
        pkg.display()
    )]
    PublishMissing { name: String, pkg: PathBuf },
    #[error(
        "schema `{name}`: manifest version `{want}` disagrees with `panschema-publish.toml` version `{got}`"
    )]
    VersionMismatch {
        name: String,
        want: String,
        got: String,
    },
    #[error(
        "schema `{name}`: manifest key disagrees with `panschema-publish.toml` name `{declared}` \
         (pass `--name {declared}` to use it, or `--name <alias>` to override)"
    )]
    NameMismatch { name: String, declared: String },
    #[error("schema `{name}`: {message}")]
    Other { name: String, message: String },
    #[error("{0}")]
    TagMoved(Box<MovedTag>),
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(transparent)]
    Cache(#[from] crate::cache::CacheError),
    #[error(transparent)]
    Publish(#[from] crate::publish::PublishError),
}

/// A locked tag whose content changed after it was locked, from
/// [`resolve_github_against_lock`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "schema `{name}`: the tag for {version} no longer matches the lockfile: its {what} was \
     locked as {locked} and downloads as {observed}. The tag was re-published after it was \
     locked; the lockfile and the cache are left as they were. To accept the new content, \
     remove the `{name}` entry from panschema.lock and the cached copy at `{}`, then run \
     `panschema fetch` again",
    cached.display()
)]
pub struct MovedTag {
    pub name: String,
    pub version: String,
    /// Which checksum disagrees: `main schema file` or `package`.
    pub what: &'static str,
    pub locked: String,
    pub observed: String,
    /// The version's cache directory, which a re-fetch would otherwise
    /// record from.
    pub cached: PathBuf,
}

/// Resolve one manifest dependency to its package, dispatching on the
/// source kind: `path:` sources resolve relative to `manifest_dir`,
/// `github:` sources through the local cache, populated via `tarballs`
/// when absent. Callers choose the network policy by the source they
/// inject — [`CodeloadGithubSource`] to allow fetching, a refusing
/// implementation to stay offline. The one dispatcher every consumer of
/// `[schemas]` entries shares, so a manifest entry means the same
/// package everywhere.
pub fn resolve_dep(
    name: &str,
    dep: &SchemaDep,
    manifest_dir: &Path,
    tarballs: &dyn TarballSource,
) -> Result<Resolved, ResolveError> {
    match SchemaSource::from_dep(name, dep)? {
        SchemaSource::Path { path } => resolve_path(name, &path, manifest_dir),
        SchemaSource::Github {
            owner,
            repo,
            version,
        } => {
            let cache = crate::cache::cache_root()?;
            resolve_github(name, &owner, &repo, &version, &cache, tarballs)
        }
    }
}

/// Open a "package directory" (or the `panschema-publish.toml` inside one),
/// parse the publish file, and return the canonical path to the package
/// directory along with the parsed publish config.
///
/// Symlink hygiene: the package directory is canonicalized; the main
/// schema file (derived later) is validated against this canonical base
/// to refuse paths that escape the package.
pub fn open_package(
    name: &str,
    pkg: &Path,
) -> Result<(PathBuf, crate::publish::PublishConfig), ResolveError> {
    if !pkg.exists() {
        return Err(ResolveError::PathMissing {
            name: name.to_string(),
            path: pkg.to_path_buf(),
            resolved: pkg.to_path_buf(),
        });
    }

    // Allow callers to point at the publish file directly OR at the dir.
    let pkg_dir = if pkg.is_file() {
        pkg.parent()
            .ok_or_else(|| ResolveError::Other {
                name: name.to_string(),
                message: format!("publish file `{}` has no parent directory", pkg.display()),
            })?
            .to_path_buf()
    } else {
        pkg.to_path_buf()
    };

    let publish_path = pkg_dir.join("panschema-publish.toml");
    if !publish_path.exists() {
        return Err(ResolveError::PublishMissing {
            name: name.to_string(),
            pkg: pkg_dir,
        });
    }
    let publish = crate::publish::PublishConfig::from_path(&publish_path)?;

    let canon_pkg = pkg_dir.canonicalize().map_err(|e| ResolveError::Other {
        name: name.to_string(),
        message: format!("canonicalize package dir `{}`: {e}", pkg_dir.display()),
    })?;
    Ok((canon_pkg, publish))
}

/// Resolve the main schema file inside a (canonical) package directory
/// and verify it doesn't escape via symlinks.
fn resolve_main_in_package(
    canon_pkg: &Path,
    publish: &crate::publish::PublishConfig,
) -> Result<PathBuf, ResolveError> {
    let main_path = canon_pkg.join(&publish.files.main);
    crate::cache::validate_within(canon_pkg, &main_path)?;
    Ok(main_path)
}

/// Resolve a `github:owner/repo@<version>` source against the local cache.
///
/// Populates the cache (using the supplied [`TarballSource`]) if not
/// already present, reads `panschema-publish.toml` from the tagged
/// commit, validates the declared version matches `version`, canonicalizes
/// the main schema path and verifies it doesn't escape the extracted
/// directory.
pub fn resolve_github(
    name: &str,
    owner: &str,
    repo: &str,
    version: &str,
    cache_root: &Path,
    source: &dyn TarballSource,
) -> Result<Resolved, ResolveError> {
    use crate::cache::{github_version_dir, populate_cache};

    let version_dir = github_version_dir(cache_root, owner, repo, version);
    let tag = format!("v{version}");
    let top_level = populate_cache(source, owner, repo, &tag, &version_dir)?;
    let extracted_dir = version_dir.join(&top_level);

    let (canon_pkg, publish) = open_package(name, &extracted_dir)?;
    if publish.schema.version != version {
        return Err(ResolveError::VersionMismatch {
            name: name.to_string(),
            want: version.to_string(),
            got: publish.schema.version,
        });
    }
    let main_path = resolve_main_in_package(&canon_pkg, &publish)?;

    Ok(Resolved {
        datasets: PublishedDataset::from_entries(&canon_pkg, &publish.instances),
        pkg_dir: canon_pkg,
        schema_path: main_path,
        version: publish.schema.version,
        published_name: publish.schema.name,
        revision: None,
    })
}

/// A [`TarballSource`] that never downloads, so [`resolve_github`] only reads
/// what is already cached.
struct CacheOnly;

impl TarballSource for CacheOnly {
    fn fetch(
        &self,
        owner: &str,
        repo: &str,
        tag: &str,
        _sink: &mut dyn Write,
    ) -> Result<(), TarballFetchError> {
        Err(TarballFetchError::TagNotFound {
            owner: owner.to_string(),
            repo: repo.to_string(),
            tag: tag.to_string(),
        })
    }
}

/// Resolve a `github:` pin for `fetch` to lock, checked against the entry
/// the lockfile already holds for it, if any. Only an entry recording the
/// same source at the same version holds the pin; with one, resolution goes
/// through [`resolve_github_against_lock`]. Otherwise the pin is new (or its
/// entry stale), and it resolves from the cache as [`resolve_github`] does.
pub fn resolve_pin_against_lock(
    name: &str,
    source: &SchemaSource,
    cache_root: &Path,
    locked: Option<&crate::lockfile::LockEntry>,
    tarballs: &dyn TarballSource,
) -> Result<(Resolved, crate::lockfile::Content), ResolveError> {
    let SchemaSource::Github {
        owner,
        repo,
        version,
    } = source
    else {
        return Err(ResolveError::Other {
            name: name.to_string(),
            message: "a path source is the working tree, not a pin".to_string(),
        });
    };
    let held = locked.filter(|entry| {
        entry.source == source.source_spec() && entry.version.as_deref() == Some(version.as_str())
    });
    match held {
        Some(entry) => {
            resolve_github_against_lock(name, owner, repo, version, cache_root, entry, tarballs)
        }
        None => {
            let resolved = resolve_github(name, owner, repo, version, cache_root, tarballs)?;
            let content = checksum_resolved(name, &resolved)?;
            Ok((resolved, content))
        }
    }
}

fn checksum_resolved(
    name: &str,
    resolved: &Resolved,
) -> Result<crate::lockfile::Content, ResolveError> {
    crate::lockfile::Content::of(&resolved.schema_path, &resolved.pkg_dir).map_err(|e| {
        ResolveError::Other {
            name: name.to_string(),
            message: format!("checksumming the cached package: {e}"),
        }
    })
}

/// Resolve a `github:` pin the lockfile already records at this source and
/// version, for `fetch` to record again without trusting an edited cache.
///
/// The cached copy is used when it resolves and agrees with `locked`.
/// Otherwise the tag is downloaded again into a scratch directory beside the
/// cache and compared there before anything in the cache moves: a copy that
/// agrees replaces the cached one; a copy that still disagrees means the tag
/// was re-published after it was locked, which is refused
/// ([`ResolveError::TagMoved`]) with the cache left as it was, since the
/// lockfile records the content that was reviewed. A failed download leaves
/// the cache as it was too.
pub fn resolve_github_against_lock(
    name: &str,
    owner: &str,
    repo: &str,
    version: &str,
    cache_root: &Path,
    locked: &crate::lockfile::LockEntry,
    tarballs: &dyn TarballSource,
) -> Result<(Resolved, crate::lockfile::Content), ResolveError> {
    use crate::lockfile::ContentDrift;

    if let Ok(cached) = resolve_github(name, owner, repo, version, cache_root, &CacheOnly)
        && let Ok(content) = checksum_resolved(name, &cached)
        && locked.content_drift(content.clone()).is_none()
    {
        return Ok((cached, content));
    }

    std::fs::create_dir_all(cache_root).map_err(crate::cache::CacheError::from)?;
    let scratch = tempfile::Builder::new()
        .prefix(".verify-")
        .tempdir_in(cache_root)
        .map_err(crate::cache::CacheError::from)?;
    let fresh = resolve_github(name, owner, repo, version, scratch.path(), tarballs)?;
    let content = checksum_resolved(name, &fresh)?;
    let version_dir = crate::cache::github_version_dir(cache_root, owner, repo, version);
    let moved = |what, locked: &str, observed: String| {
        ResolveError::TagMoved(Box::new(MovedTag {
            name: name.to_string(),
            version: version.to_string(),
            what,
            locked: locked.to_string(),
            observed,
            cached: version_dir.clone(),
        }))
    };
    match locked.content_drift(content.clone()) {
        None => {}
        Some(ContentDrift::MainFile { locked, observed }) => {
            return Err(moved("main schema file", locked, observed));
        }
        Some(ContentDrift::Package { locked, observed }) => {
            return Err(moved("package", locked, observed));
        }
    }
    crate::cache::replace_extraction(&version_dir, &fresh.pkg_dir)?;
    let resolved = resolve_github(name, owner, repo, version, cache_root, tarballs)?;
    Ok((resolved, content))
}

/// Resolve a `path:` source against the manifest directory.
///
/// `path` points at a package — either the directory containing
/// `panschema-publish.toml`, or the publish file itself. Reads the
/// publish file to learn the version and the main file's relative
/// location.
pub fn resolve_path(
    name: &str,
    path: &Path,
    manifest_dir: &Path,
) -> Result<Resolved, ResolveError> {
    let resolved = manifest_dir.join(path);
    let (canon_pkg, publish) = open_package(name, &resolved)?;
    let main_path = resolve_main_in_package(&canon_pkg, &publish)?;
    Ok(Resolved {
        datasets: PublishedDataset::from_entries(&canon_pkg, &publish.instances),
        pkg_dir: canon_pkg,
        schema_path: main_path,
        version: publish.schema.version,
        published_name: publish.schema.name,
        revision: None,
    })
}

impl TarballSource for CodeloadGithubSource {
    fn fetch(
        &self,
        owner: &str,
        repo: &str,
        tag: &str,
        sink: &mut dyn Write,
    ) -> Result<(), TarballFetchError> {
        let url = format!("https://codeload.github.com/{owner}/{repo}/tar.gz/refs/tags/{tag}");
        let response = ureq::get(&url).call().map_err(|e| match &e {
            ureq::Error::Status(404, _) => TarballFetchError::TagNotFound {
                owner: owner.to_string(),
                repo: repo.to_string(),
                tag: tag.to_string(),
            },
            _ => TarballFetchError::Network {
                owner: owner.to_string(),
                repo: repo.to_string(),
                tag: tag.to_string(),
                source: Box::new(e),
            },
        })?;
        let mut reader = response.into_reader();
        std::io::copy(&mut reader, sink)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dep_path(p: &str) -> SchemaDep {
        SchemaDep {
            path: Some(PathBuf::from(p)),
            source: None,
            version: None,
        }
    }

    fn dep_github(spec: &str, version: &str) -> SchemaDep {
        SchemaDep {
            path: None,
            source: Some(spec.to_string()),
            version: Some(version.to_string()),
        }
    }

    #[test]
    fn parses_path_dep() {
        let s = SchemaSource::from_dep("x", &dep_path("./x.yaml")).unwrap();
        assert_eq!(
            s,
            SchemaSource::Path {
                path: PathBuf::from("./x.yaml")
            }
        );
    }

    #[test]
    fn parses_github_dep() {
        let s = SchemaSource::from_dep(
            "remote",
            &dep_github("github:padamson/scimantic-schema", "0.1.3"),
        )
        .unwrap();
        assert_eq!(
            s,
            SchemaSource::Github {
                owner: "padamson".to_string(),
                repo: "scimantic-schema".to_string(),
                version: "0.1.3".to_string(),
            }
        );
    }

    #[test]
    fn rejects_empty_dep() {
        let dep = SchemaDep::default();
        let err = SchemaSource::from_dep("x", &dep).unwrap_err();
        assert_eq!(err, SourceError::Empty("x".to_string()));
    }

    #[test]
    fn source_spec_format_round_trips_path() {
        // Lockfile + manifest both treat `source_spec` as a key for
        // matching; the exact format must round-trip through the
        // lockfile parser's expectations (`path:<rel>`).
        let src = SchemaSource::Path {
            path: PathBuf::from("./local-pkg"),
        };
        assert_eq!(src.source_spec(), "path:./local-pkg");
    }

    #[test]
    fn source_spec_format_round_trips_github() {
        let src = SchemaSource::Github {
            owner: "padamson".to_string(),
            repo: "scimantic-schema".to_string(),
            version: "0.1.0".to_string(),
        };
        assert_eq!(src.source_spec(), "github:padamson/scimantic-schema");
    }

    #[test]
    fn tag_is_none_for_path_source() {
        let src = SchemaSource::Path {
            path: PathBuf::from("./x"),
        };
        assert_eq!(src.tag(), None);
    }

    #[test]
    fn tag_prepends_v_for_github_source() {
        // The codeload URL format requires `v<version>` as the tag,
        // not the bare version string.
        let src = SchemaSource::Github {
            owner: "a".to_string(),
            repo: "b".to_string(),
            version: "0.1.3".to_string(),
        };
        assert_eq!(src.tag(), Some("v0.1.3".to_string()));
    }

    #[test]
    fn rejects_path_and_source_together() {
        let dep = SchemaDep {
            path: Some(PathBuf::from("./x.yaml")),
            source: Some("github:a/b".to_string()),
            version: None,
        };
        let err = SchemaSource::from_dep("x", &dep).unwrap_err();
        assert_eq!(err, SourceError::PathAndSource("x".to_string()));
    }

    #[test]
    fn rejects_source_without_version() {
        let dep = SchemaDep {
            path: None,
            source: Some("github:a/b".to_string()),
            version: None,
        };
        let err = SchemaSource::from_dep("x", &dep).unwrap_err();
        assert_eq!(err, SourceError::SourceWithoutVersion("x".to_string()));
    }

    #[test]
    fn rejects_version_without_source() {
        let dep = SchemaDep {
            path: Some(PathBuf::from("./x.yaml")),
            source: None,
            version: Some("0.1.0".to_string()),
        };
        let err = SchemaSource::from_dep("x", &dep).unwrap_err();
        assert_eq!(err, SourceError::VersionWithoutSource("x".to_string()));
    }

    #[test]
    fn rejects_unknown_protocol() {
        let dep = dep_github("gitlab:a/b", "0.1.0");
        let err = SchemaSource::from_dep("x", &dep).unwrap_err();
        assert!(matches!(err, SourceError::UnknownProtocol { .. }));
    }

    #[test]
    fn rejects_malformed_github_missing_slash() {
        let dep = dep_github("github:padamson", "0.1.0");
        let err = SchemaSource::from_dep("x", &dep).unwrap_err();
        assert!(matches!(err, SourceError::MalformedGithub { .. }));
    }

    #[test]
    fn rejects_malformed_github_extra_segment() {
        let dep = dep_github("github:a/b/c", "0.1.0");
        let err = SchemaSource::from_dep("x", &dep).unwrap_err();
        assert!(matches!(err, SourceError::MalformedGithub { .. }));
    }

    #[test]
    fn rejects_malformed_github_empty_owner() {
        let dep = dep_github("github:/repo", "0.1.0");
        let err = SchemaSource::from_dep("x", &dep).unwrap_err();
        assert!(matches!(err, SourceError::MalformedGithub { .. }));
    }

    // -----------------------------------------------------------------
    // End-to-end `resolve_github` tests using a LocalTarballFixture.
    // -----------------------------------------------------------------

    use crate::cache::{LocalTarballFixture, write_fixture_tarball};
    use tempfile::TempDir;

    /// Build a fixture tarball at `tarball_path` and return a fixture source
    /// that serves it. The tarball's top-level directory is `<repo>-<version_id>`,
    /// matching what real codeload `refs/tags/<tag>` and sha-based URLs produce.
    fn fixture_tarball(
        dir: &std::path::Path,
        repo: &str,
        version_id: &str,
        publish_toml: &str,
        schema_yaml: &str,
    ) -> (PathBuf, LocalTarballFixture) {
        let tarball_path = dir.join("fixture.tar.gz");
        write_fixture_tarball(
            &tarball_path,
            repo,
            version_id,
            &[
                ("panschema-publish.toml", publish_toml.as_bytes()),
                ("schema/example.yaml", schema_yaml.as_bytes()),
            ],
        )
        .unwrap();
        let source = LocalTarballFixture {
            path: tarball_path.clone(),
        };
        (tarball_path, source)
    }

    #[test]
    fn resolve_github_happy_path_writes_to_cache() {
        let tmp = TempDir::new().unwrap();
        let cache_root = tmp.path().join("cache");
        let fix_dir = tmp.path().join("fix");
        std::fs::create_dir_all(&fix_dir).unwrap();
        let (_t, src) = fixture_tarball(
            &fix_dir,
            "myrepo",
            "0.1.0",
            r#"
[schema]
name = "myrepo"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema/example.yaml"
"#,
            "id: https://example.org/\nname: example\n",
        );

        let resolved = crate::source::resolve_github(
            "myrepo",
            "ownerco",
            "myrepo",
            "0.1.0",
            &cache_root,
            &src,
        )
        .unwrap();
        // `refs/tags/<tag>` URLs don't expose a commit SHA without an extra
        // API call. We don't make that call, so revision is None.
        assert!(resolved.revision.is_none());
        assert!(resolved.schema_path.ends_with("schema/example.yaml"));
        assert!(resolved.schema_path.exists());

        // Regression: `pkg_dir` must point at the package *root*, not
        // at the parent of the schema file. For a publish file with
        // `main = "schema/example.yaml"`, `schema_path.parent()` would
        // land inside `schema/`, and reading `panschema-publish.toml`
        // from that wrong directory would fail with ENOENT — which is
        // exactly what `panschema add` did before this field existed.
        assert!(resolved.pkg_dir.ends_with("myrepo-0.1.0"));
        assert_ne!(
            resolved.pkg_dir.as_path(),
            resolved.schema_path.parent().unwrap()
        );
        assert!(resolved.pkg_dir.join("panschema-publish.toml").exists());
    }

    #[test]
    fn resolve_github_errors_on_version_mismatch() {
        let tmp = TempDir::new().unwrap();
        let cache_root = tmp.path().join("cache");
        let fix_dir = tmp.path().join("fix");
        std::fs::create_dir_all(&fix_dir).unwrap();
        let (_t, src) = fixture_tarball(
            &fix_dir,
            "myrepo",
            "0.1.0",
            r#"
[schema]
name = "myrepo"
version = "0.2.0"
linkml = "1.7.0"

[files]
main = "schema/example.yaml"
"#,
            "id: https://example.org/\nname: example\n",
        );

        let err = crate::source::resolve_github(
            "myrepo",
            "ownerco",
            "myrepo",
            "0.1.0",
            &cache_root,
            &src,
        )
        .unwrap_err();
        match err {
            crate::source::ResolveError::VersionMismatch { want, got, .. } => {
                assert_eq!(want, "0.1.0");
                assert_eq!(got, "0.2.0");
            }
            other => panic!("expected VersionMismatch, got {other:?}"),
        }
    }

    #[test]
    fn resolve_github_errors_when_publish_toml_missing() {
        let tmp = TempDir::new().unwrap();
        let cache_root = tmp.path().join("cache");
        let fix_dir = tmp.path().join("fix");
        std::fs::create_dir_all(&fix_dir).unwrap();
        // Build a tarball that has the schema but NO panschema-publish.toml.
        let tarball_path = fix_dir.join("fixture.tar.gz");
        write_fixture_tarball(
            &tarball_path,
            "myrepo",
            "0.1.0",
            &[("schema/example.yaml", b"name: example\n")],
        )
        .unwrap();
        let src = LocalTarballFixture { path: tarball_path };

        let err = crate::source::resolve_github(
            "myrepo",
            "ownerco",
            "myrepo",
            "0.1.0",
            &cache_root,
            &src,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            crate::source::ResolveError::PublishMissing { .. }
        ));
    }

    #[test]
    fn resolve_github_second_call_is_cache_hit() {
        let tmp = TempDir::new().unwrap();
        let cache_root = tmp.path().join("cache");
        let fix_dir = tmp.path().join("fix");
        std::fs::create_dir_all(&fix_dir).unwrap();
        let (tarball_path, src) = fixture_tarball(
            &fix_dir,
            "myrepo",
            "0.1.0",
            r#"
[schema]
name = "myrepo"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema/example.yaml"
"#,
            "id: https://example.org/\nname: example\n",
        );

        let first = crate::source::resolve_github(
            "myrepo",
            "ownerco",
            "myrepo",
            "0.1.0",
            &cache_root,
            &src,
        )
        .unwrap();
        let first_contents = std::fs::read_to_string(&first.schema_path).unwrap();

        // Replace the fixture with mutated contents (and a different version_id
        // suffix so a re-fetch would land in a *different* cache directory).
        // The cache hit should reuse the existing extracted dir and ignore the
        // mutated tarball.
        write_fixture_tarball(
            &tarball_path,
            "myrepo",
            "different",
            &[
                (
                    "panschema-publish.toml",
                    br#"
[schema]
name = "myrepo"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema/example.yaml"
"# as &[u8],
                ),
                ("schema/example.yaml", b"name: changed\n"),
            ],
        )
        .unwrap();

        let second = crate::source::resolve_github(
            "myrepo",
            "ownerco",
            "myrepo",
            "0.1.0",
            &cache_root,
            &src,
        )
        .unwrap();
        assert_eq!(first.schema_path, second.schema_path);
        let second_contents = std::fs::read_to_string(&second.schema_path).unwrap();
        assert_eq!(first_contents, second_contents);
        assert!(first.revision.is_none());
        assert!(second.revision.is_none());
    }

    const LOCKED_PUBLISH: &str = "[schema]\nname = \"myrepo\"\nversion = \"0.1.0\"\nlinkml = \"1.7.0\"\n\n[files]\nmain = \"schema/example.yaml\"\n";
    const LOCKED_SCHEMA: &str = "id: https://example.org/\nname: example\n";

    /// A source that fails every download, so a test can tell whether one
    /// was attempted.
    struct UnreachableSource;

    impl TarballSource for UnreachableSource {
        fn fetch(
            &self,
            owner: &str,
            repo: &str,
            tag: &str,
            _sink: &mut dyn Write,
        ) -> Result<(), TarballFetchError> {
            Err(TarballFetchError::TagNotFound {
                owner: owner.to_string(),
                repo: repo.to_string(),
                tag: tag.to_string(),
            })
        }
    }

    /// `myrepo` 0.1.0 resolved once into a fresh cache, and the lock entry
    /// `fetch` would write for it.
    fn locked_pin(tmp: &TempDir) -> (PathBuf, LocalTarballFixture, crate::lockfile::LockEntry) {
        let cache_root = tmp.path().join("cache");
        let fix_dir = tmp.path().join("fix");
        std::fs::create_dir_all(&fix_dir).unwrap();
        let (_t, src) = fixture_tarball(&fix_dir, "myrepo", "0.1.0", LOCKED_PUBLISH, LOCKED_SCHEMA);
        let resolved =
            resolve_github("myrepo", "ownerco", "myrepo", "0.1.0", &cache_root, &src).unwrap();
        let content =
            crate::lockfile::Content::of(&resolved.schema_path, &resolved.pkg_dir).unwrap();
        let entry = crate::lockfile::LockEntry {
            name: "myrepo".to_string(),
            version: Some("0.1.0".to_string()),
            source: "github:ownerco/myrepo".to_string(),
            revision: None,
            checksum: content.checksum,
            package_checksum: Some(content.package_checksum),
        };
        (cache_root, src, entry)
    }

    fn against_lock(
        cache_root: &Path,
        locked: &crate::lockfile::LockEntry,
        tarballs: &dyn TarballSource,
    ) -> Result<(Resolved, crate::lockfile::Content), ResolveError> {
        resolve_github_against_lock(
            "myrepo", "ownerco", "myrepo", "0.1.0", cache_root, locked, tarballs,
        )
    }

    #[test]
    fn a_cache_that_agrees_with_the_lock_is_used_without_a_download() {
        let tmp = TempDir::new().unwrap();
        let (cache_root, _src, entry) = locked_pin(&tmp);
        let (_, content) = against_lock(&cache_root, &entry, &UnreachableSource)
            .expect("an intact cache needs no download");
        assert_eq!(
            Some(content.package_checksum),
            entry.package_checksum,
            "the content returned is the cache's"
        );
    }

    #[test]
    fn an_edited_cache_is_downloaded_again_rather_than_locked() {
        let tmp = TempDir::new().unwrap();
        let (cache_root, src, entry) = locked_pin(&tmp);
        let pkg = crate::cache::github_version_dir(&cache_root, "ownerco", "myrepo", "0.1.0")
            .join("myrepo-0.1.0");
        std::fs::write(
            pkg.join("panschema-publish.toml"),
            format!("{LOCKED_PUBLISH}# edited\n"),
        )
        .unwrap();
        let (resolved, content) =
            against_lock(&cache_root, &entry, &src).expect("a fresh download restores the cache");
        assert_eq!(
            entry.content_drift(content),
            None,
            "the restored cache agrees with the lock"
        );
        assert_eq!(
            std::fs::read_to_string(resolved.pkg_dir.join("panschema-publish.toml")).unwrap(),
            LOCKED_PUBLISH,
            "the edit is gone from the cache"
        );
    }

    #[test]
    fn a_tag_that_moved_after_it_was_locked_is_refused() {
        let tmp = TempDir::new().unwrap();
        let (cache_root, _src, entry) = locked_pin(&tmp);
        let moved_dir = tmp.path().join("moved");
        std::fs::create_dir_all(&moved_dir).unwrap();
        let (_t, moved) = fixture_tarball(
            &moved_dir,
            "myrepo",
            "0.1.0",
            LOCKED_PUBLISH,
            "name: republished\n",
        );
        let pkg = crate::cache::github_version_dir(&cache_root, "ownerco", "myrepo", "0.1.0")
            .join("myrepo-0.1.0");
        std::fs::write(pkg.join("schema/example.yaml"), "name: edited\n").unwrap();
        match against_lock(&cache_root, &entry, &moved) {
            Err(ResolveError::TagMoved(moved)) => {
                assert_eq!(
                    (
                        moved.name.as_str(),
                        moved.version.as_str(),
                        moved.locked.as_str()
                    ),
                    ("myrepo", "0.1.0", entry.checksum.as_str())
                );
            }
            other => panic!("expected TagMoved, got {:?}", other.map(|(r, _)| r)),
        }
        assert_eq!(
            std::fs::read_to_string(pkg.join("schema/example.yaml")).unwrap(),
            "name: edited\n",
            "the re-published content is kept out of the cache"
        );
        assert_no_scratch_left(&cache_root);
    }

    fn assert_no_scratch_left(cache_root: &Path) {
        let left: Vec<_> = std::fs::read_dir(cache_root)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|n| n != "github")
            .collect();
        assert!(
            left.is_empty(),
            "no scratch download is left behind: {left:?}"
        );
    }

    #[test]
    fn a_cache_edited_past_resolving_is_downloaded_again() {
        let tmp = TempDir::new().unwrap();
        let (cache_root, src, entry) = locked_pin(&tmp);
        let pkg = crate::cache::github_version_dir(&cache_root, "ownerco", "myrepo", "0.1.0")
            .join("myrepo-0.1.0");
        std::fs::remove_file(pkg.join("schema/example.yaml")).unwrap();
        let (resolved, content) = against_lock(&cache_root, &entry, &src)
            .expect("a cache missing its main file is downloaded again");
        assert_eq!(entry.content_drift(content), None);
        assert!(resolved.schema_path.exists(), "the main file is back");
        assert_no_scratch_left(&cache_root);
    }

    /// With nothing cached, the only download goes to scratch, so a tag
    /// re-published since it was locked never reaches the cache.
    #[test]
    fn a_moved_tag_never_reaches_an_empty_cache() {
        let tmp = TempDir::new().unwrap();
        let (cache_root, _src, entry) = locked_pin(&tmp);
        std::fs::remove_dir_all(cache_root.join("github")).unwrap();
        let moved_dir = tmp.path().join("moved");
        std::fs::create_dir_all(&moved_dir).unwrap();
        let (_t, moved) = fixture_tarball(
            &moved_dir,
            "myrepo",
            "0.1.0",
            LOCKED_PUBLISH,
            "name: republished\n",
        );
        match against_lock(&cache_root, &entry, &moved) {
            Err(ResolveError::TagMoved(moved)) => assert_eq!(
                moved.cached,
                crate::cache::github_version_dir(&cache_root, "ownerco", "myrepo", "0.1.0"),
                "the refusal names the cached copy to remove"
            ),
            other => panic!("expected TagMoved, got {:?}", other.map(|(r, _)| r)),
        }
        let cached: Vec<_> = std::fs::read_dir(crate::cache::github_version_dir(
            &cache_root,
            "ownerco",
            "myrepo",
            "0.1.0",
        ))
        .map(|dir| {
            dir.map(|e| e.unwrap().file_name())
                .filter(|n| n != ".lock")
                .collect()
        })
        .unwrap_or_default();
        assert!(
            cached.is_empty(),
            "the re-published tag is not cached: {cached:?}"
        );
        assert_no_scratch_left(&cache_root);
    }

    #[test]
    fn a_failed_download_leaves_the_cached_copy_alone() {
        let tmp = TempDir::new().unwrap();
        let (cache_root, _src, entry) = locked_pin(&tmp);
        let pkg = crate::cache::github_version_dir(&cache_root, "ownerco", "myrepo", "0.1.0")
            .join("myrepo-0.1.0");
        std::fs::write(pkg.join("extra.yaml"), "x: 1\n").unwrap();
        let err = against_lock(&cache_root, &entry, &UnreachableSource)
            .expect_err("an edited cache that cannot be downloaded again is not locked");
        assert!(matches!(err, ResolveError::Cache(_)), "got: {err:?}");
        assert!(
            pkg.join("extra.yaml").exists(),
            "the cached copy is left as it was"
        );
        assert_no_scratch_left(&cache_root);
    }

    #[test]
    fn an_entry_without_a_package_checksum_is_held_to_its_main_file() {
        let tmp = TempDir::new().unwrap();
        let (cache_root, _src, mut entry) = locked_pin(&tmp);
        entry.package_checksum = None;
        let pkg = crate::cache::github_version_dir(&cache_root, "ownerco", "myrepo", "0.1.0")
            .join("myrepo-0.1.0");
        std::fs::write(pkg.join("extra.yaml"), "x: 1\n").unwrap();
        against_lock(&cache_root, &entry, &UnreachableSource)
            .expect("with no package checksum recorded, an intact main file needs no download");
    }

    /// A pin is checked against its entry only when the entry records the
    /// same source at the same version; any other entry is stale, and the
    /// pin is recorded afresh.
    #[test]
    fn only_an_entry_at_the_same_source_and_version_holds_a_pin() {
        let tmp = TempDir::new().unwrap();
        let (cache_root, _src, entry) = locked_pin(&tmp);
        let pkg = crate::cache::github_version_dir(&cache_root, "ownerco", "myrepo", "0.1.0")
            .join("myrepo-0.1.0");
        std::fs::write(pkg.join("extra.yaml"), "x: 1\n").unwrap();
        let source = SchemaSource::Github {
            owner: "ownerco".to_string(),
            repo: "myrepo".to_string(),
            version: "0.1.0".to_string(),
        };
        let pin = |locked: Option<&crate::lockfile::LockEntry>| {
            resolve_pin_against_lock("myrepo", &source, &cache_root, locked, &UnreachableSource)
        };
        assert!(
            pin(Some(&entry)).is_err(),
            "the entry holds the pin, so the edited cache is downloaded again (and cannot be)"
        );
        let mut other_version = entry.clone();
        other_version.version = Some("0.0.9".to_string());
        let mut other_source = entry.clone();
        other_source.source = "github:someone-else/myrepo".to_string();
        for (stale, why) in [
            (Some(&other_version), "another version"),
            (Some(&other_source), "another source"),
            (None, "no entry"),
        ] {
            assert!(
                pin(stale).is_ok(),
                "with {why} the pin is recorded from the cache as it is"
            );
        }
    }

    /// Resolving a package carries its published datasets across: the
    /// names a consumer refers to them by, each data file resolved against
    /// the package directory rather than the consumer's, and the schema a
    /// dataset conforms to when its entry names one.
    #[test]
    fn resolving_a_package_carries_the_datasets_it_publishes() {
        let tmp = TempDir::new().unwrap();
        let pkg = tmp.path().join("wine");
        std::fs::create_dir_all(pkg.join("data")).unwrap();
        std::fs::write(
            pkg.join("panschema-publish.toml"),
            r#"
[schema]
name = "wine"
version = "1.0.0"
linkml = "1.7.0"

[files]
main = "wine.yaml"

[[instances]]
name = "worked-example"
data = "data/wine-instances.yaml"

[[instances]]
name = "benchmark"
data = "data/wine-benchmark.yaml"
schema = "cqa"
"#,
        )
        .unwrap();
        std::fs::write(
            pkg.join("wine.yaml"),
            "name: wine\nid: https://example.org/wine\n",
        )
        .unwrap();

        let resolved = resolve_path("wine", Path::new("wine"), tmp.path()).expect("resolves");
        assert_eq!(
            resolved.dataset_names(),
            ["worked-example", "benchmark"],
            "in publish order"
        );
        let worked = resolved.dataset("worked-example").expect("published");
        assert_eq!(
            worked.data,
            resolved.pkg_dir.join("data/wine-instances.yaml"),
            "the data file resolves inside the package, not the consumer"
        );
        assert_eq!(
            worked.schema, None,
            "it conforms to the package's own schema"
        );
        assert_eq!(
            resolved.dataset("benchmark").expect("published").schema,
            Some("cqa".to_string()),
            "and this one names the dependency it conforms to"
        );
    }

    /// The consumer's one dependency, a real package directory holding
    /// `data/in.yaml` beside a file outside it, whose `worked-example`
    /// dataset names `data`.
    fn depending_on_data(
        tmp: &tempfile::TempDir,
        data: &str,
    ) -> std::collections::BTreeMap<String, Resolved> {
        let pkg = tmp.path().join("pkg");
        std::fs::create_dir_all(pkg.join("data")).unwrap();
        std::fs::write(pkg.join("data/in.yaml"), "x: 1\n").unwrap();
        std::fs::write(tmp.path().join("outside.yaml"), "secret: 1\n").unwrap();
        let pkg = pkg.canonicalize().unwrap();
        let mut wine = published(pkg.to_str().unwrap());
        wine.datasets[0].data = pkg.join(data);
        wine.datasets[0].written = PathBuf::from(data);
        std::collections::BTreeMap::from([("wine".to_string(), wine)])
    }

    /// A fetched package is untrusted, so the data file a consumer names
    /// must lie inside it, as its main file and its `imports:` must.
    #[test]
    fn a_named_dataset_must_lie_inside_the_package_that_publishes_it() {
        let named = ["worked-example".to_string()];
        let tmp = tempfile::tempdir().unwrap();
        let deps = depending_on_data(&tmp, "data/in.yaml");
        assert_eq!(
            resolve_named_datasets("wine", &deps, &named).expect("a file inside resolves"),
            [deps["wine"].pkg_dir.join("data/in.yaml")]
        );

        let tmp = tempfile::tempdir().unwrap();
        let err =
            resolve_named_datasets("wine", &depending_on_data(&tmp, "../outside.yaml"), &named)
                .expect_err("a path out of the package is refused");
        assert_eq!(
            *err,
            DatasetError::OutsidePackage {
                entry: "wine".to_string(),
                dep: "wine".to_string(),
                name: "worked-example".to_string(),
                data: PathBuf::from("../outside.yaml"),
            },
            "the error names the dataset and its `data` path as written"
        );
    }

    // Unix reports a path below a file as "not a directory". Windows reports
    // it as not found, so there the path counts as inside and the load
    // reports the missing file.
    #[cfg(unix)]
    #[test]
    fn a_dataset_path_that_cannot_be_read_is_reported_with_its_path_not_as_an_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let deps = depending_on_data(&tmp, "data/in.yaml/below-a-file.yaml");
        let err = resolve_named_datasets("wine", &deps, &["worked-example".to_string()])
            .expect_err("a path below a file cannot be checked");
        assert!(
            matches!(
                &*err,
                DatasetError::Unreadable { data, .. }
                    if data == Path::new("data/in.yaml/below-a-file.yaml")
            ),
            "got: {err:?}"
        );
    }

    /// A package with two datasets: one its own schema, one conforming to
    /// a dependency's.
    fn published(pkg: &str) -> Resolved {
        Resolved {
            pkg_dir: PathBuf::from(pkg),
            schema_path: PathBuf::from(pkg).join("schema.yaml"),
            version: "1.0.0".to_string(),
            published_name: "wine".to_string(),
            datasets: vec![
                PublishedDataset {
                    name: "worked-example".to_string(),
                    data: PathBuf::from(pkg).join("data/wine-instances.yaml"),
                    written: PathBuf::from("data/wine-instances.yaml"),
                    schema: None,
                },
                PublishedDataset {
                    name: "benchmark".to_string(),
                    data: PathBuf::from(pkg).join("data/wine-benchmark.yaml"),
                    written: PathBuf::from("data/wine-benchmark.yaml"),
                    schema: Some("cqa".to_string()),
                },
            ],
            revision: None,
        }
    }

    /// The consumer's dependencies, keyed as its manifest declares them.
    fn deps() -> std::collections::BTreeMap<String, Resolved> {
        let mut map = std::collections::BTreeMap::new();
        map.insert("wine".to_string(), published("/pkg/wine"));
        let mut cqa = published("/pkg/cqa");
        cqa.published_name = "cqa".to_string();
        cqa.datasets.clear();
        map.insert("cqa".to_string(), cqa);
        map
    }

    #[test]
    fn a_declared_file_is_the_dataset_its_package_names() {
        let declared = DeclaredDatasets::from_resolved(&deps());
        assert_eq!(
            declared.of(Path::new("/pkg/wine/data/wine-benchmark.yaml")),
            Some(&DeclaredDataset {
                name: "benchmark".to_string(),
                version: "1.0.0".to_string(),
                dep_key: "wine".to_string(),
                schema: Some("cqa".to_string()),
            })
        );
    }

    #[test]
    fn a_file_no_package_declares_is_no_dataset() {
        let declared = DeclaredDatasets::from_resolved(&deps());
        assert_eq!(declared.of(Path::new("/pkg/wine/data/scratch.yaml")), None);
    }

    #[test]
    fn a_data_path_leading_out_of_its_package_declares_no_dataset() {
        let tmp = tempfile::tempdir().unwrap();
        let deps = depending_on_data(&tmp, "../outside.yaml");
        let declared = DeclaredDatasets::from_resolved(&deps);
        assert_eq!(declared.of(&tmp.path().join("outside.yaml")), None);
    }

    #[test]
    fn a_dataset_listed_under_the_block_for_its_schema_passes() {
        let declared = DeclaredDatasets::from_resolved(&deps());
        let listed = [
            PathBuf::from("/pkg/wine/data/wine-benchmark.yaml"),
            PathBuf::from("/pkg/wine/data/scratch.yaml"),
        ];
        assert_eq!(declared.check_listed("cqa", &deps(), &listed), Ok(()));
    }

    #[test]
    fn a_packages_own_dataset_listed_under_its_own_block_passes() {
        let declared = DeclaredDatasets::from_resolved(&deps());
        let listed = [PathBuf::from("/pkg/wine/data/wine-instances.yaml")];
        assert_eq!(declared.check_listed("wine", &deps(), &listed), Ok(()));
    }

    /// The publisher names cqa by its own key; a consumer keying it
    /// `grader` lists the benchmark under `[generate.grader]`, matched
    /// through the package's published name.
    #[test]
    fn a_dataset_listed_under_a_block_the_consumer_keys_differently_passes() {
        let mut aliased = std::collections::BTreeMap::new();
        aliased.insert("vino".to_string(), published("/pkg/wine"));
        let mut cqa = published("/pkg/cqa");
        cqa.published_name = "cqa".to_string();
        cqa.datasets.clear();
        aliased.insert("grader".to_string(), cqa);
        let declared = DeclaredDatasets::from_resolved(&aliased);
        let listed = [PathBuf::from("/pkg/wine/data/wine-benchmark.yaml")];
        assert_eq!(declared.check_listed("grader", &aliased, &listed), Ok(()));
    }

    /// The by-path twin of naming a dataset under the wrong block: the
    /// publisher says `benchmark` conforms to cqa, so `[generate.wine]`
    /// listing its file would render it against a schema it does not meet.
    #[test]
    fn a_dataset_listed_under_another_schemas_block_is_refused() {
        let declared = DeclaredDatasets::from_resolved(&deps());
        let listed = [PathBuf::from("/pkg/wine/data/wine-benchmark.yaml")];
        let err = declared
            .check_listed("wine", &deps(), &listed)
            .expect_err("benchmark conforms to cqa");
        assert_eq!(
            *err,
            DatasetError::WrongSchema {
                entry: "wine".to_string(),
                dep: "wine".to_string(),
                dep_key: "wine".to_string(),
                name: "benchmark".to_string(),
                schema: "cqa".to_string(),
                block: Some("cqa".to_string()),
            }
        );
    }

    /// A dataset declaring no schema conforms to its own package's, which
    /// another package's block is not.
    #[test]
    fn a_packages_own_dataset_listed_under_another_packages_block_is_refused() {
        let declared = DeclaredDatasets::from_resolved(&deps());
        let listed = [PathBuf::from("/pkg/wine/data/wine-instances.yaml")];
        let err = declared
            .check_listed("cqa", &deps(), &listed)
            .expect_err("worked-example conforms to wine");
        assert_eq!(
            *err,
            DatasetError::WrongSchema {
                entry: "cqa".to_string(),
                dep: "wine".to_string(),
                dep_key: "wine".to_string(),
                name: "worked-example".to_string(),
                schema: "wine".to_string(),
                block: Some("wine".to_string()),
            }
        );
    }

    /// Aliases of one package (`path = "."` under several keys) declare the
    /// same files; two different packages declaring one file is a tie the
    /// first key breaks, so the answer does not depend on map internals.
    #[test]
    fn a_file_two_packages_declare_is_named_by_the_first_key() {
        let mut deps = deps();
        let mut zinfandel = published("/pkg/wine");
        zinfandel.published_name = "zinfandel".to_string();
        deps.insert("zinfandel".to_string(), zinfandel);
        let declared = DeclaredDatasets::from_resolved(&deps);
        assert_eq!(
            declared
                .of(Path::new("/pkg/wine/data/wine-instances.yaml"))
                .map(|d| d.dep_key.as_str()),
            Some("wine")
        );
    }

    /// A consumer reaches a sibling's file as `../pkg/data/…` joined to its
    /// own directory, which names the same file the package declares by a
    /// different spelling.
    #[test]
    fn a_file_reached_through_a_relative_path_is_still_its_dataset() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let pkg = tmp.path().join("pkg");
        std::fs::create_dir_all(pkg.join("data")).expect("mkdir");
        std::fs::create_dir_all(tmp.path().join("consumer")).expect("mkdir");
        std::fs::write(pkg.join("data/records.yaml"), "records: []\n").expect("write");
        let mut resolved = published(pkg.to_str().expect("utf-8 path"));
        resolved.datasets[0].data = pkg.join("data/records.yaml");
        let mut deps = std::collections::BTreeMap::new();
        deps.insert("wine".to_string(), resolved);

        let declared = DeclaredDatasets::from_resolved(&deps);
        let reached = tmp.path().join("consumer/../pkg/data/records.yaml");
        assert_eq!(
            declared.of(&reached).map(|d| d.name.as_str()),
            Some("worked-example")
        );
    }

    /// A dataset one package publishes against another's schema is named by
    /// both: which package ships it, and — by the block it sits under —
    /// which schema it conforms to. This is the shape a benchmark takes,
    /// written in the grader's schema but shipped with the ontology it
    /// grades.
    #[test]
    fn a_qualified_name_resolves_a_datasets_publisher_and_its_schema() {
        let paths = resolve_named_datasets("cqa", &deps(), &["wine:benchmark".to_string()])
            .expect("wine publishes `benchmark` against cqa");
        assert_eq!(paths, [PathBuf::from("/pkg/wine/data/wine-benchmark.yaml")]);
    }

    /// The qualifier must name a dependency the consumer declared, or there
    /// is no package to look in.
    #[test]
    fn a_qualified_name_for_an_undeclared_dependency_lists_the_declared_ones() {
        let err = resolve_named_datasets("cqa", &deps(), &["merlot:benchmark".to_string()])
            .expect_err("`merlot` is not declared");
        let message = err.to_string();
        assert!(
            message.contains("`merlot`") && message.contains("cqa, wine"),
            "got {message}"
        );
    }

    /// A dataset that conforms to its own package's schema does not belong
    /// under another block, however it is spelled.
    #[test]
    fn a_qualified_name_still_checks_the_schema_the_dataset_conforms_to() {
        let err = resolve_named_datasets("cqa", &deps(), &["wine:worked-example".to_string()])
            .expect_err("`worked-example` conforms to wine's own schema");
        assert!(
            err.to_string().contains("conforms to schema `wine`"),
            "got {err}"
        );
    }

    /// Naming this entry's own dependency explicitly is the bare form spelled
    /// long, and resolves the same way.
    #[test]
    fn a_qualified_name_may_name_this_entrys_own_dependency() {
        let paths = resolve_named_datasets("wine", &deps(), &["wine:worked-example".to_string()])
            .expect("resolves");
        assert_eq!(paths, [PathBuf::from("/pkg/wine/data/wine-instances.yaml")]);
    }

    /// A named dataset resolves to the file the package publishes, under
    /// the package's own directory — the consumer never writes that path.
    #[test]
    fn a_named_dataset_resolves_to_the_packages_own_file() {
        let paths = resolve_named_datasets("wine", &deps(), &["worked-example".to_string()])
            .expect("resolves");
        assert_eq!(paths, [PathBuf::from("/pkg/wine/data/wine-instances.yaml")]);
    }

    /// Names resolve in declaration order, so the rendered order is the
    /// order the consumer wrote.
    #[test]
    fn named_datasets_resolve_in_declaration_order() {
        let paths = resolve_named_datasets(
            "wine",
            &deps(),
            &["worked-example".to_string(), "worked-example".to_string()],
        )
        .expect("resolves");
        assert_eq!(paths.len(), 2, "a repeated name resolves twice, not once");
    }

    /// A name the package does not publish fails naming what it does, so a
    /// typo is one message away from the fix.
    #[test]
    fn an_unpublished_dataset_name_lists_what_is_published() {
        let err = resolve_named_datasets("wine", &deps(), &["exemplar".to_string()])
            .expect_err("no such dataset");
        let message = err.to_string();
        assert!(
            message.contains("`wine` publishes no dataset named `exemplar`")
                && message.contains("worked-example")
                && message.contains("benchmark"),
            "got {message}"
        );
    }

    /// The package states which schema each dataset conforms to, so a
    /// dataset named under a block for a different schema is refused rather
    /// than rendered against a schema it does not conform to — and the
    /// message names the block it belongs under, in the qualified spelling
    /// that resolves there.
    #[test]
    fn a_dataset_conforming_to_another_schema_is_refused_with_a_usable_remedy() {
        let err = resolve_named_datasets("wine", &deps(), &["benchmark".to_string()])
            .expect_err("benchmark conforms to cqa, not wine");
        let message = err.to_string();
        assert!(
            message.contains("conforms to schema `cqa`")
                && message.contains("[generate.cqa]")
                && message.contains("`wine:benchmark`"),
            "got {message}"
        );

        // And that spelling is the one that works.
        let paths = resolve_named_datasets("cqa", &deps(), &["wine:benchmark".to_string()])
            .expect("the remedy the message gives resolves");
        assert_eq!(paths, [PathBuf::from("/pkg/wine/data/wine-benchmark.yaml")]);
    }

    /// A consumer may key a dependency by any name it likes, so the
    /// qualifier that resolves is *its* key, not the package's published
    /// name. The refusal has to spell it that way, or it sends the author to
    /// a spelling that fails as an unknown dependency.
    #[test]
    fn the_remedy_spells_the_qualifier_the_consumer_would_have_to_write() {
        let mut aliased = std::collections::BTreeMap::new();
        aliased.insert("vino".to_string(), published("/pkg/wine")); // published_name: wine
        let mut cqa = published("/pkg/cqa");
        cqa.published_name = "cqa".to_string();
        cqa.datasets.clear();
        aliased.insert("grader".to_string(), cqa);

        let err = resolve_named_datasets("vino", &aliased, &["benchmark".to_string()])
            .expect_err("benchmark conforms to cqa, not wine");
        let message = err.to_string();
        assert!(
            message.contains("`vino:benchmark`"),
            "the qualifier must be the consumer's key; got {message}"
        );
        assert!(
            message.contains("[generate.grader]"),
            "the block must be the consumer's key for that schema; got {message}"
        );

        // And the spelling the message gives is the one that resolves.
        let paths = resolve_named_datasets("grader", &aliased, &["vino:benchmark".to_string()])
            .expect("the remedy resolves");
        assert_eq!(paths, [PathBuf::from("/pkg/wine/data/wine-benchmark.yaml")]);
    }

    /// A dataset conforming to a schema this manifest does not declare has no
    /// block to move to, and the message says that rather than naming one.
    #[test]
    fn a_dataset_for_an_undeclared_schema_says_there_is_no_block() {
        let mut only_wine = std::collections::BTreeMap::new();
        only_wine.insert("wine".to_string(), published("/pkg/wine"));
        let err = resolve_named_datasets("wine", &only_wine, &["benchmark".to_string()])
            .expect_err("cqa is not declared here");
        assert!(
            err.to_string().contains("no [schemas] entry for `cqa`"),
            "got {err}"
        );
    }

    /// A publish entry may name its own package's schema — redundant, but
    /// legal — and that dataset resolves under the block for that package,
    /// which is the only block a bare name is resolved against.
    #[test]
    fn a_dataset_naming_its_own_packages_schema_resolves() {
        let mut all = deps();
        all.get_mut("wine")
            .expect("wine")
            .datasets
            .push(PublishedDataset {
                name: "restated".to_string(),
                data: PathBuf::from("/pkg/wine/data/restated.yaml"),
                written: PathBuf::from("data/restated.yaml"),
                schema: Some("wine".to_string()),
            });
        let paths =
            resolve_named_datasets("wine", &all, &["restated".to_string()]).expect("resolves");
        assert_eq!(paths, [PathBuf::from("/pkg/wine/data/restated.yaml")]);
    }

    #[test]
    fn a_spec_string_is_pinned_exactly_when_its_source_is() {
        for dep in [
            dep_github("github:o/r", "1.0.0"),
            dep_path("./pkg"),
            dep_path("."),
        ] {
            let source = SchemaSource::from_dep("s", &dep).unwrap();
            assert_eq!(
                SchemaSource::spec_is_pinned(&source.source_spec()),
                source.is_pinned(),
                "the spec-string rule agrees with the kind for {source:?}"
            );
        }
    }

    #[test]
    fn only_a_released_version_is_a_pin() {
        assert!(
            SchemaSource::from_dep("c", &dep_github("github:o/r", "1.0.0"))
                .unwrap()
                .is_pinned(),
            "a github source at a version is a pin"
        );
        assert!(
            !SchemaSource::from_dep("p", &dep_path("./pkg"))
                .unwrap()
                .is_pinned(),
            "a path source is the working tree, not a pin"
        );
        assert!(
            !SchemaSource::from_dep("own", &dep_path("."))
                .unwrap()
                .is_pinned(),
            "the manifest's own package is a path source like any other"
        );
    }

    #[test]
    fn partition_splits_pins_from_path_sources_and_refuses_a_malformed_entry() {
        let mut schemas = std::collections::BTreeMap::new();
        schemas.insert("own".to_string(), dep_path("."));
        schemas.insert("contract".to_string(), dep_github("github:o/r", "1.0.0"));
        schemas.insert("sibling".to_string(), dep_path("../sibling"));
        let (pinned, unpinned) = partition_pins(&schemas).unwrap();
        assert_eq!(pinned, ["contract"]);
        assert_eq!(unpinned, ["own", "sibling"], "in name order");

        schemas.insert(
            "broken".to_string(),
            SchemaDep {
                path: Some(PathBuf::from(".")),
                source: None,
                version: Some("0.1.0".into()),
            },
        );
        assert!(
            partition_pins(&schemas).is_err(),
            "a malformed entry is reported, not silently dropped from both halves"
        );
    }
}
