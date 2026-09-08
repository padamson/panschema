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
    /// or `"github:owner/repo"`. Mirrors the format already used by
    /// [`crate::lockfile::path_source_spec`].
    pub fn source_spec(&self) -> String {
        match self {
            Self::Path { path } => format!("path:{}", path.display()),
            Self::Github { owner, repo, .. } => format!("github:{owner}/{repo}"),
        }
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
    /// directory the publish manifest sits in.
    pub data: PathBuf,
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
        // A dataset conforms to the schema its publish entry names, or — when
        // it names none — to its own package's. Either way it belongs under
        // the block for that schema, so this entry must be it.
        //
        // The publish entry names that schema by the *publisher's* manifest
        // key, which the consumer need not spell the same way, so either that
        // key or the published name of this entry's own package counts as a
        // match.
        let conforms_to = dataset.schema.as_deref().unwrap_or(dep_key);
        let entry_names = |key: &str| {
            key == entry
                || resolved
                    .get(entry)
                    .is_some_and(|r| r.published_name == conforms_to)
        };
        if !entry_names(conforms_to) {
            // Point at a block the consumer actually has, spelled its way.
            let block = resolved
                .iter()
                .find(|(key, r)| key.as_str() == conforms_to || r.published_name == conforms_to)
                .map(|(key, _)| key.clone());
            return Err(Box::new(DatasetError::WrongSchema {
                entry: entry.to_string(),
                dep: dep.published_name.clone(),
                dep_key: dep_key.to_string(),
                name: dataset_name.to_string(),
                schema: conforms_to.to_string(),
                block,
            }));
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
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(transparent)]
    Cache(#[from] crate::cache::CacheError),
    #[error(transparent)]
    Publish(#[from] crate::publish::PublishError),
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
                    schema: None,
                },
                PublishedDataset {
                    name: "benchmark".to_string(),
                    data: PathBuf::from(pkg).join("data/wine-benchmark.yaml"),
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
                schema: Some("wine".to_string()),
            });
        let paths =
            resolve_named_datasets("wine", &all, &["restated".to_string()]).expect("resolves");
        assert_eq!(paths, [PathBuf::from("/pkg/wine/data/restated.yaml")]);
    }
}
