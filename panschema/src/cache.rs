//! Local cache for fetched remote schema sources.
//!
//! Layout (cargo-style, hierarchical):
//!
//! ```text
//! ~/.cache/panschema/
//!   github/<owner>/<repo>/<version>/
//!     <owner>-<repo>-<commit-sha>/    # extracted tarball
//!     .lock                            # fs2 exclusive lock for the version
//! ```
//!
//! Reference: [`docs/features/05-schema-manager.md`](../../docs/features/05-schema-manager.md)

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::source::{TarballFetchError, TarballSource};

/// Errors raised by the cache module.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("could not determine the user's cache directory")]
    NoCacheDir,
    #[error("I/O error in cache: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to extract tarball: {0}")]
    Extract(String),
    #[error("path `{0}` escapes the cache directory")]
    PathEscape(PathBuf),
    #[error("network/source error: {0}")]
    Source(#[from] TarballFetchError),
    #[error("could not acquire cache lock at `{path}`: {source}")]
    Lock {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Returns the panschema cache root, e.g.
/// `~/.cache/panschema` on Linux, `~/Library/Caches/panschema` on macOS.
pub fn cache_root() -> Result<PathBuf, CacheError> {
    let dirs = directories::ProjectDirs::from("dev", "padamson", "panschema")
        .ok_or(CacheError::NoCacheDir)?;
    Ok(dirs.cache_dir().to_path_buf())
}

/// Cache directory for a specific `github:` version. Does not create the dir.
pub fn github_version_dir(cache_root: &Path, owner: &str, repo: &str, version: &str) -> PathBuf {
    cache_root
        .join("github")
        .join(owner)
        .join(repo)
        .join(version)
}

/// Reject paths that — after symlink resolution — escape `base`.
///
/// Use this when reading any file path that came from a tarball or other
/// untrusted source. The caller is responsible for canonicalizing `base`
/// separately; we canonicalize `target` here.
pub fn validate_within(base: &Path, target: &Path) -> Result<(), CacheError> {
    let canon_target = match target.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return Err(CacheError::Io(e));
        }
    };
    if !canon_target.starts_with(base) {
        return Err(CacheError::PathEscape(target.to_path_buf()));
    }
    Ok(())
}

/// Extract a gzipped-tar reader into `target_dir` and return the top-level
/// directory name (which is `<owner>-<repo>-<commit-sha>` for codeload.github.com
/// tarballs).
///
/// Rejects entries with absolute paths or `..` components (the `tar` crate
/// does this by default via `Archive::unpack`, but we double-check by
/// inspecting each entry's path before extraction).
pub fn extract_tarball<R: Read>(reader: R, target_dir: &Path) -> Result<String, CacheError> {
    fs::create_dir_all(target_dir)?;
    let gz = flate2::read::GzDecoder::new(reader);
    let mut archive = tar::Archive::new(gz);
    archive.set_preserve_permissions(false);
    archive.set_preserve_mtime(false);

    let mut top_level: Option<String> = None;
    for entry in archive
        .entries()
        .map_err(|e| CacheError::Extract(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| CacheError::Extract(e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e| CacheError::Extract(e.to_string()))?
            .into_owned();

        // Reject absolute paths and `..` traversal.
        if path.is_absolute() {
            return Err(CacheError::Extract(format!(
                "tarball entry has absolute path: {}",
                path.display()
            )));
        }
        if path.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::RootDir
            )
        }) {
            return Err(CacheError::Extract(format!(
                "tarball entry has parent/root traversal: {}",
                path.display()
            )));
        }

        // Track the top-level directory name (codeload tarballs put everything
        // under a single `<owner>-<repo>-<sha>/` prefix).
        if let Some(first) = path.components().next() {
            let first = first.as_os_str().to_string_lossy().to_string();
            if let Some(existing) = &top_level {
                if existing != &first {
                    return Err(CacheError::Extract(format!(
                        "tarball has multiple top-level entries: `{existing}` and `{first}`"
                    )));
                }
            } else {
                top_level = Some(first);
            }
        }

        entry
            .unpack_in(target_dir)
            .map_err(|e| CacheError::Extract(e.to_string()))?;
    }

    top_level.ok_or_else(|| CacheError::Extract("tarball is empty".to_string()))
}

/// Fetch and cache a `github:` source if not already present.
///
/// Returns the commit SHA portion of the extracted top-level dir name,
/// e.g. `"abc123def..."` from `<owner>-<repo>-abc123def...`.
///
/// Takes an exclusive `fs2` lock on `<version_dir>/.lock` for the duration
/// of the fetch+extract, so concurrent fetches of the same version block.
pub fn populate_cache(
    source: &dyn TarballSource,
    owner: &str,
    repo: &str,
    tag: &str,
    version_dir: &Path,
) -> Result<String, CacheError> {
    fs::create_dir_all(version_dir)?;
    let lock_path = version_dir.join(".lock");
    let lock_file = File::create(&lock_path).map_err(|e| CacheError::Lock {
        path: lock_path.clone(),
        source: e,
    })?;
    use fs2::FileExt;
    lock_file.lock_exclusive().map_err(|e| CacheError::Lock {
        path: lock_path.clone(),
        source: e,
    })?;

    // If a sibling top-level dir already exists (from a previous successful
    // fetch), reuse it — extract the SHA from its name and we're done.
    if let Some(existing_sha) = find_existing_extraction(version_dir, owner, repo)? {
        return Ok(existing_sha);
    }

    // Fetch tarball bytes via the source trait into an in-memory buffer.
    let mut bytes = Vec::new();
    source.fetch(owner, repo, tag, &mut bytes)?;

    // Extract into a temp subdir under version_dir; rename on success.
    let temp_dir = version_dir.join(".tmp-extract");
    // Best-effort cleanup of any prior partial extraction.
    let _ = fs::remove_dir_all(&temp_dir);
    let top_level = extract_tarball(&bytes[..], &temp_dir)?;
    let extracted = temp_dir.join(&top_level);

    let final_dir = version_dir.join(&top_level);
    // Best-effort cleanup of any prior tenant; rename succeeds either way.
    let _ = fs::remove_dir_all(&final_dir);
    fs::rename(&extracted, &final_dir)?;
    let _ = fs::remove_dir_all(&temp_dir);

    // Validate prefix and extract SHA.
    let expected_prefix = format!("{owner}-{repo}-");
    let sha = top_level
        .strip_prefix(&expected_prefix)
        .ok_or_else(|| {
            CacheError::Extract(format!(
                "tarball top-level `{top_level}` doesn't start with `{expected_prefix}`"
            ))
        })?
        .to_string();
    if sha.is_empty() {
        return Err(CacheError::Extract(format!(
            "tarball top-level `{top_level}` has no SHA suffix"
        )));
    }

    Ok(sha)
}

/// Look for a previously-extracted directory matching `<owner>-<repo>-<sha>`
/// in `version_dir`. Returns the SHA if exactly one is found, `None` otherwise.
fn find_existing_extraction(
    version_dir: &Path,
    owner: &str,
    repo: &str,
) -> Result<Option<String>, CacheError> {
    if !version_dir.exists() {
        return Ok(None);
    }
    let prefix = format!("{owner}-{repo}-");
    let mut matches = Vec::new();
    for entry in fs::read_dir(version_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(sha) = name.strip_prefix(&prefix)
            && !sha.is_empty()
        {
            matches.push(sha.to_string());
        }
    }
    match matches.len() {
        0 => Ok(None),
        1 => Ok(Some(matches.into_iter().next().unwrap())),
        _ => Err(CacheError::Extract(format!(
            "multiple cached extractions in {}; cache is inconsistent",
            version_dir.display()
        ))),
    }
}

/// Write the bytes of a fixture tarball to `dest` — helper for tests that
/// want to build a fake codeload tarball on the fly.
///
/// `entries` is a slice of `(relative_path_within_archive, contents)` pairs.
/// All entries are placed under a single top-level dir named
/// `<owner>-<repo>-<sha>`.
#[cfg(any(test, feature = "test-fixtures"))]
pub fn write_fixture_tarball(
    dest: &Path,
    owner: &str,
    repo: &str,
    sha: &str,
    entries: &[(&str, &[u8])],
) -> Result<(), CacheError> {
    let file = File::create(dest)?;
    let gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut builder = tar::Builder::new(gz);
    let top = format!("{owner}-{repo}-{sha}");

    for (rel, content) in entries {
        let path = format!("{top}/{rel}");
        let mut header = tar::Header::new_gnu();
        header.set_path(&path).map_err(CacheError::Io)?;
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append(&header, std::io::Cursor::new(content))
            .map_err(CacheError::Io)?;
    }
    builder.finish().map_err(CacheError::Io)?;
    Ok(())
}

/// Test impl of [`TarballSource`] that copies bytes from a local file —
/// available to lib tests and to integration tests via the
/// `test-fixtures` feature flag.
#[cfg(any(test, feature = "test-fixtures"))]
pub struct LocalTarballFixture {
    pub path: PathBuf,
}

#[cfg(any(test, feature = "test-fixtures"))]
impl TarballSource for LocalTarballFixture {
    fn fetch(
        &self,
        _owner: &str,
        _repo: &str,
        _tag: &str,
        sink: &mut dyn std::io::Write,
    ) -> Result<(), TarballFetchError> {
        let bytes = std::fs::read(&self.path)?;
        sink.write_all(&bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn cache_root_returns_a_path() {
        // We can't assert the exact path (varies by OS/user), but we can
        // verify the helper succeeds in a typical environment.
        let root = cache_root().expect("cache root should be available");
        assert!(root.is_absolute(), "cache root should be absolute");
        assert!(
            root.ends_with("panschema") || root.to_string_lossy().contains("panschema"),
            "cache root should include the app name; got {}",
            root.display()
        );
    }

    #[test]
    fn github_version_dir_composes_path() {
        let root = Path::new("/cache/panschema");
        let dir = github_version_dir(root, "padamson", "scimantic-schema", "0.1.3");
        assert_eq!(
            dir,
            PathBuf::from("/cache/panschema/github/padamson/scimantic-schema/0.1.3")
        );
    }

    #[test]
    fn validate_within_accepts_in_bounds_path() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let inside = base.join("sub").join("file");
        fs::create_dir_all(inside.parent().unwrap()).unwrap();
        fs::write(&inside, b"x").unwrap();
        validate_within(&base, &inside).expect("inside should pass");
    }

    #[test]
    fn validate_within_rejects_symlink_escape() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        // Create a sibling outside `base` and a symlink inside pointing to it.
        let outside_dir = tmp.path().parent().unwrap().join("escape-target");
        fs::create_dir_all(&outside_dir).unwrap();
        let outside_file = outside_dir.join("secret");
        fs::write(&outside_file, b"x").unwrap();
        let link = base.join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside_file, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&outside_file, &link).unwrap();

        let err = validate_within(&base, &link).unwrap_err();
        assert!(matches!(err, CacheError::PathEscape(_)));
    }

    #[test]
    fn extract_tarball_unpacks_and_returns_top_level() {
        let tmp = TempDir::new().unwrap();
        let tarball_path = tmp.path().join("fixture.tar.gz");
        write_fixture_tarball(
            &tarball_path,
            "fix-owner",
            "fix-repo",
            "abc123",
            &[
                ("panschema-publish.toml", b"# fixture"),
                ("schema/example.yaml", b"name: example\n"),
            ],
        )
        .unwrap();

        let target = tmp.path().join("extracted");
        let reader = File::open(&tarball_path).unwrap();
        let top = extract_tarball(reader, &target).unwrap();
        assert_eq!(top, "fix-owner-fix-repo-abc123");
        assert!(target.join(&top).join("panschema-publish.toml").exists());
        assert!(
            target
                .join(&top)
                .join("schema")
                .join("example.yaml")
                .exists()
        );
    }

    #[test]
    fn extract_tarball_rejects_parent_dir_traversal() {
        // The `tar` crate's `Header::set_path` refuses to *write* a `..`
        // component (the trusted-side defense), so we have to inject the
        // malicious path directly into the raw tar header bytes — that
        // simulates a hostile tarball found in the wild.
        let tmp = TempDir::new().unwrap();
        let tarball_path = tmp.path().join("evil.tar.gz");

        // A tar header is 512 bytes; the path lives in the first 100.
        let mut header = [0u8; 512];
        let malicious_path = b"../escape.txt";
        header[..malicious_path.len()].copy_from_slice(malicious_path);
        // mode = "0000644 " (NUL-terminated octal in 8 bytes)
        header[100..108].copy_from_slice(b"0000644\0");
        // uid, gid = "0000000 "
        header[108..116].copy_from_slice(b"0000000\0");
        header[116..124].copy_from_slice(b"0000000\0");
        // size = 3 in octal: "0000003 "
        header[124..136].copy_from_slice(b"00000000003\0");
        // mtime = 0
        header[136..148].copy_from_slice(b"00000000000\0");
        // checksum placeholder (8 spaces) for cksum computation
        header[148..156].copy_from_slice(b"        ");
        // typeflag = '0' (regular file)
        header[156] = b'0';
        // ustar magic
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        // Compute checksum: sum of all bytes in header (with the 8 spaces
        // we wrote at 148..156 as a placeholder), then write it in.
        let cksum: u32 = header.iter().map(|&b| b as u32).sum();
        let cksum_str = format!("{:06o}\0 ", cksum);
        header[148..156].copy_from_slice(cksum_str.as_bytes());

        // Write header + 512-byte-padded data + two empty terminator blocks.
        let mut raw = Vec::new();
        raw.extend_from_slice(&header);
        raw.extend_from_slice(b"hi\n\0\0\0");
        raw.resize(raw.len() + (512 - raw.len() % 512), 0);
        raw.extend_from_slice(&[0u8; 1024]);

        // Gzip-compress and write out.
        use std::io::Write as _;
        let file = File::create(&tarball_path).unwrap();
        let mut gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        gz.write_all(&raw).unwrap();
        gz.finish().unwrap();

        let target = tmp.path().join("extracted");
        let reader = File::open(&tarball_path).unwrap();
        let err = extract_tarball(reader, &target).unwrap_err();
        match err {
            CacheError::Extract(msg) => {
                assert!(
                    msg.contains("parent") || msg.contains(".."),
                    "expected mention of parent/..; got: {msg}"
                );
            }
            other => panic!("expected Extract error, got {other:?}"),
        }
    }

    #[test]
    fn populate_cache_extracts_and_returns_sha() {
        let tmp = TempDir::new().unwrap();
        let tarball = tmp.path().join("fixture.tar.gz");
        write_fixture_tarball(
            &tarball,
            "owner",
            "repo",
            "deadbeef",
            &[("panschema-publish.toml", b"# fixture")],
        )
        .unwrap();

        let source = LocalTarballFixture {
            path: tarball.clone(),
        };
        let version_dir = tmp.path().join("cache").join("v1");
        let sha = populate_cache(&source, "owner", "repo", "v1", &version_dir).unwrap();
        assert_eq!(sha, "deadbeef");
        assert!(version_dir.join("owner-repo-deadbeef").exists());
        assert!(version_dir.join(".lock").exists());
    }

    #[test]
    fn populate_cache_is_idempotent_when_extracted_dir_already_present() {
        let tmp = TempDir::new().unwrap();
        let tarball = tmp.path().join("fixture.tar.gz");
        write_fixture_tarball(
            &tarball,
            "owner",
            "repo",
            "feedface",
            &[("panschema-publish.toml", b"# fixture")],
        )
        .unwrap();

        let source = LocalTarballFixture {
            path: tarball.clone(),
        };
        let version_dir = tmp.path().join("cache").join("v1");

        // First populate.
        let sha1 = populate_cache(&source, "owner", "repo", "v1", &version_dir).unwrap();
        // Mutate the fixture so the SECOND fetch would produce a different
        // top-level dir name — but since cache lookup short-circuits, the
        // mutation should have no effect.
        write_fixture_tarball(
            &tarball,
            "owner",
            "repo",
            "different",
            &[("panschema-publish.toml", b"# fixture")],
        )
        .unwrap();
        let sha2 = populate_cache(&source, "owner", "repo", "v1", &version_dir).unwrap();
        assert_eq!(sha1, sha2);
        assert_eq!(sha2, "feedface");
    }
}
