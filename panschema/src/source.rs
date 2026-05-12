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

/// Resolved schema dependency: the on-disk path to the schema's main
/// file plus the version declared in `panschema-publish.toml` and (for
/// remote sources) the commit SHA to record in the lockfile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Absolute path to the main schema file.
    pub schema_path: PathBuf,
    /// Version declared in `panschema-publish.toml`. Always populated —
    /// both source types are now "packages" with a publish file.
    pub version: String,
    /// Commit SHA for `github:` sources. `None` for `path:` sources.
    pub revision: Option<String>,
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
    Cache(#[from] crate::cache::CacheError),
    #[error(transparent)]
    Publish(#[from] crate::publish::PublishError),
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
    let sha = populate_cache(source, owner, repo, &tag, &version_dir)?;
    let extracted_dir = version_dir.join(format!("{owner}-{repo}-{sha}"));

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
        schema_path: main_path,
        version: publish.schema.version,
        revision: Some(sha),
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
        schema_path: main_path,
        version: publish.schema.version,
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
    /// that serves it.
    fn fixture_tarball(
        dir: &std::path::Path,
        owner: &str,
        repo: &str,
        sha: &str,
        publish_toml: &str,
        schema_yaml: &str,
    ) -> (PathBuf, LocalTarballFixture) {
        let tarball_path = dir.join("fixture.tar.gz");
        write_fixture_tarball(
            &tarball_path,
            owner,
            repo,
            sha,
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
    fn resolve_github_happy_path_writes_to_cache_and_returns_sha() {
        let tmp = TempDir::new().unwrap();
        let cache_root = tmp.path().join("cache");
        let fix_dir = tmp.path().join("fix");
        std::fs::create_dir_all(&fix_dir).unwrap();
        let (_t, src) = fixture_tarball(
            &fix_dir,
            "ownerco",
            "myrepo",
            "abc123",
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
        assert_eq!(resolved.revision.as_deref(), Some("abc123"));
        assert!(resolved.schema_path.ends_with("schema/example.yaml"));
        assert!(resolved.schema_path.exists());
    }

    #[test]
    fn resolve_github_errors_on_version_mismatch() {
        let tmp = TempDir::new().unwrap();
        let cache_root = tmp.path().join("cache");
        let fix_dir = tmp.path().join("fix");
        std::fs::create_dir_all(&fix_dir).unwrap();
        let (_t, src) = fixture_tarball(
            &fix_dir,
            "ownerco",
            "myrepo",
            "abc123",
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
            "ownerco",
            "myrepo",
            "abc123",
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
    fn resolve_github_second_call_is_cache_hit_and_returns_same_sha() {
        let tmp = TempDir::new().unwrap();
        let cache_root = tmp.path().join("cache");
        let fix_dir = tmp.path().join("fix");
        std::fs::create_dir_all(&fix_dir).unwrap();
        let (tarball_path, src) = fixture_tarball(
            &fix_dir,
            "ownerco",
            "myrepo",
            "feedface",
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

        // Replace the fixture with a different SHA to prove the cache hit
        // doesn't go back to the source.
        write_fixture_tarball(
            &tarball_path,
            "ownerco",
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
        assert_eq!(first.revision, second.revision);
        assert_eq!(first.revision.as_deref(), Some("feedface"));
    }
}
