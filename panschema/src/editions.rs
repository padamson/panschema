//! Book editions: one published book per schema release line.
//!
//! An edition is a minor line during 0.x and a major line after 1.0, built
//! from the newest tag in the line, so it spans exactly the releases a
//! consumer's pin moves within. The edge ref builds one more, unreleased,
//! edition. Editions read the release list `panschema publish` reads
//! (`[publishing]`), so the book and the schema docs agree on which
//! edition every version belongs to.
//!
//! Reference: [`docs/features/52-book-editions.md`](../../docs/features/52-book-editions.md)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use semver::Version;
use serde::Deserialize;

use crate::publish::{PUBLISH_FILENAME, PublishConfig, PublishError, PublishingConfig};

/// One edition of the book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edition {
    /// The directory the edition lands in under `book/`: `v0.1`, `v1`, or
    /// the edge label.
    pub label: String,
    /// The ref the edition builds from: the newest tag in its line, or the
    /// edge ref.
    pub source: String,
    /// The released versions the edition spans, oldest first. Empty for
    /// the unreleased edition.
    pub versions: Vec<String>,
}

impl Edition {
    /// Whether the edition is built from a release tag rather than the
    /// edge ref.
    pub fn is_released(&self) -> bool {
        !self.versions.is_empty()
    }
}

/// Every edition a release list produces, and which one is current.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditionPlan {
    /// Released editions newest first, then the unreleased one.
    pub editions: Vec<Edition>,
    /// The label of the edition the site root serves.
    pub current: String,
}

/// Group `[publishing]`'s release list into editions.
///
/// Fails when a version does not parse as semver (its line cannot be
/// known), when the tags are spelled two ways or one is listed twice
/// (edition names would be inconsistent or ambiguous), or when the edge
/// label is also an edition's label.
pub fn plan(publishing: &PublishingConfig) -> Result<EditionPlan, PublishError> {
    // Edition labels reuse the tags' prefix, so every tag must share one.
    let mut spelling: Option<(&str, &str)> = None;
    // Keyed by line, so iteration runs oldest line first; each line's
    // versions are sorted by semver below.
    let mut lines: BTreeMap<Line, Vec<(Version, &str)>> = BTreeMap::new();
    for tag in &publishing.versions {
        let prefix = if tag.starts_with(['v', 'V']) {
            &tag[..1]
        } else {
            ""
        };
        let version = Version::parse(&tag[prefix.len()..]).map_err(|_| {
            PublishError::EditionVersionNotSemver {
                version: tag.clone(),
            }
        })?;
        match spelling {
            None => spelling = Some((prefix, tag)),
            Some((first_prefix, first)) if first_prefix != prefix => {
                return Err(PublishError::MixedTagSpelling {
                    first: first.to_string(),
                    second: tag.clone(),
                });
            }
            Some(_) => {}
        }
        let line = lines.entry(Line::of(&version)).or_default();
        if line.iter().any(|(seen, _)| *seen == version) {
            return Err(PublishError::DuplicateVersion {
                version: tag.clone(),
            });
        }
        line.push((version, tag));
    }
    let prefix = spelling.map_or("", |(prefix, _)| prefix);

    let mut editions: Vec<Edition> = lines
        .into_iter()
        .rev()
        .map(|(line, mut tags)| {
            tags.sort_by(|a, b| a.0.cmp(&b.0));
            // A pin never resolves to a pre-release, so a line builds from
            // its newest release; a line of only pre-releases has none.
            let (_, source) = tags
                .iter()
                .rev()
                .find(|(version, _)| version.pre.is_empty())
                .or(tags.last())
                .expect("a line holds at least one tag");
            Edition {
                label: format!("{prefix}{}", line.label()),
                source: source.to_string(),
                versions: tags.iter().map(|(_, tag)| tag.to_string()).collect(),
            }
        })
        .collect();

    if let Some(edge) = &publishing.edge {
        if editions.iter().any(|e| &e.label == edge) {
            return Err(PublishError::EdgeShadowsEdition { edge: edge.clone() });
        }
        editions.push(Edition {
            label: edge.clone(),
            source: edge.clone(),
            versions: Vec::new(),
        });
    }

    let current = editions
        .iter()
        .find(|e| e.versions.contains(&publishing.current) || e.source == publishing.current)
        .map(|e| e.label.clone())
        .ok_or_else(|| PublishError::InvalidCurrent {
            current: publishing.current.clone(),
            versions: publishing.versions.clone(),
            edge: publishing.edge.clone(),
        })?;

    Ok(EditionPlan { editions, current })
}

/// A release line: what a Cargo-style caret pin moves within. Each 0.0.z
/// release is its own line, then the minor line during 0.x, then the
/// major line after 1.0. Variant order is release order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Line {
    Patch(u64),
    Minor(u64),
    Major(u64),
}

impl Line {
    fn of(version: &Version) -> Self {
        match (version.major, version.minor) {
            (0, 0) => Line::Patch(version.patch),
            (0, minor) => Line::Minor(minor),
            (major, _) => Line::Major(major),
        }
    }

    fn label(self) -> String {
        match self {
            Line::Patch(patch) => format!("0.0.{patch}"),
            Line::Minor(minor) => format!("0.{minor}"),
            Line::Major(major) => major.to_string(),
        }
    }
}

/// The file in a book directory naming the tools that build it.
pub const TOOLCHAIN_FILENAME: &str = "toolchain.toml";

/// The site-root file recording which top-level entries the root copy of
/// the current edition wrote, so the next run removes exactly those.
pub const ROOT_LEDGER_FILENAME: &str = ".panschema-editions-root";

/// Why publishing the editions failed.
#[derive(Debug, thiserror::Error)]
pub enum EditionsError {
    #[error(transparent)]
    Publish(#[from] PublishError),
    #[error("`mdbook-panschema editions` requires an [editions] section in {PUBLISH_FILENAME}")]
    MissingEditions,
    #[error("edition `{edition}`: could not read the tree at `{tag}`: {message}")]
    Extract {
        edition: String,
        tag: String,
        message: String,
    },
    #[error("edition `{edition}`: there is no `book.toml` in `{}` at `{tag}`", book.display())]
    NoBook {
        edition: String,
        tag: String,
        book: PathBuf,
    },
    #[error("edition `{edition}`: {TOOLCHAIN_FILENAME} at `{tag}` is malformed: {message}")]
    Toolchain {
        edition: String,
        tag: String,
        message: String,
    },
    #[error("edition `{edition}`: `{command}` failed at `{tag}`:\n{output}")]
    Install {
        edition: String,
        tag: String,
        command: String,
        output: String,
    },
    #[error("edition `{edition}`: the book at `{tag}` did not build:\n{output}")]
    Build {
        edition: String,
        tag: String,
        output: String,
    },
    #[error(
        "the current edition `{edition}` has a top-level `{entry}`, which at the site root {what}"
    )]
    RootCollision {
        edition: String,
        entry: String,
        what: String,
    },
    #[error(
        "the current edition `{edition}` has a top-level entry whose name is not UTF-8 (`{entry}`), which the site cannot serve"
    )]
    RootEntryNotUtf8 { edition: String, entry: String },
    #[error(
        "{ROOT_LEDGER_FILENAME} at the site root names `{entry}`, which {problem}; the site root was not written by `mdbook-panschema editions`, so remove the ledger or the site root and run again"
    )]
    RootLedger { entry: String, problem: String },
    #[error("`{}` is not inside a git repository: {message}", dir.display())]
    NotARepository { dir: PathBuf, message: String },
    #[error("could not use a scratch directory: {0}")]
    Scratch(std::io::Error),
    #[error("could not read {ROOT_LEDGER_FILENAME} at the site root: {0}")]
    LedgerUnreadable(std::io::Error),
    #[error("failed to write the site: {0}")]
    Write(std::io::Error),
}

/// `<book>/toolchain.toml`: the mdbook and plugin versions a tag's book
/// builds with.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolchainFile {
    /// The mdbook version.
    #[serde(default)]
    pub mdbook: Option<String>,
    /// The preprocessors and other plugins, in the order their assets
    /// are installed.
    #[serde(default, rename = "plugin")]
    pub plugins: Vec<PluginPin>,
}

/// One `[[plugin]]` in `toolchain.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginPin {
    /// The binary's name, which is also how it is run.
    pub name: String,
    /// A crates.io version.
    #[serde(default)]
    pub version: Option<String>,
    /// A git repository to install from, with `rev` or `branch`.
    #[serde(default)]
    pub git: Option<String>,
    /// An exact commit in `git`.
    #[serde(default)]
    pub rev: Option<String>,
    /// A branch in `git`; only the unreleased edition may build from one.
    #[serde(default)]
    pub branch: Option<String>,
    /// The arguments that generate the plugin's book assets, run in the
    /// book directory before the build. Empty means nothing to install.
    #[serde(default)]
    pub install: Vec<String>,
}

/// The tools an edition builds with. [`PathToolchain`] runs them from
/// `PATH`; tests substitute one that runs in-process.
pub trait Toolchain {
    /// Run `plugin` with `args` in `book_dir`. An error carries the
    /// tool's output.
    fn install_assets(&self, plugin: &str, args: &[String], book_dir: &Path) -> Result<(), String>;
    /// Build the book in `book_dir` into `dest`, returning the directory
    /// holding the rendered HTML: `dest` itself, or a subdirectory of it
    /// when the book renders through more than one backend. An error
    /// carries the tool's output.
    fn build(&self, book_dir: &Path, dest: &Path) -> Result<PathBuf, String>;
}

/// Runs mdbook and every plugin as whatever `PATH` resolves them to.
#[derive(Debug, Clone)]
pub struct PathToolchain {
    mdbook: PathBuf,
}

impl Default for PathToolchain {
    fn default() -> Self {
        PathToolchain {
            mdbook: PathBuf::from("mdbook"),
        }
    }
}

impl Toolchain for PathToolchain {
    fn install_assets(&self, plugin: &str, args: &[String], book_dir: &Path) -> Result<(), String> {
        let mut command = Command::new(plugin);
        command.args(args).current_dir(book_dir);
        run_tool(command, plugin)
    }

    fn build(&self, book_dir: &Path, dest: &Path) -> Result<PathBuf, String> {
        let mut command = Command::new(&self.mdbook);
        command
            .arg("build")
            .arg(book_dir)
            .arg("--dest-dir")
            .arg(dest);
        run_tool(command, &self.mdbook.to_string_lossy())?;
        // mdbook just read this file, so it is there and parses.
        let book_toml = std::fs::read_to_string(book_dir.join("book.toml")).unwrap_or_default();
        Ok(html_root(&book_toml, dest))
    }
}

/// Where mdbook put the HTML for a book configured by `book_toml`:
/// `html/` under the destination when several `[output.*]` tables
/// declare several backends, the destination itself otherwise (one
/// table, or none, which means the HTML backend alone).
fn html_root(book_toml: &str, dest: &Path) -> PathBuf {
    let backends = toml::from_str::<toml::Value>(book_toml)
        .ok()
        .and_then(|book| book.get("output")?.as_table().map(|outputs| outputs.len()))
        .unwrap_or(0);
    if backends > 1 {
        dest.join("html")
    } else {
        dest.to_path_buf()
    }
}

/// Run `command`, returning its combined output as the error when it
/// cannot start or exits non-zero.
fn run_tool(mut command: Command, name: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|e| format!("could not run `{name}`: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

/// What [`build_editions`] published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditionsReport {
    /// The editions built, and the current one.
    pub plan: EditionPlan,
    /// The site root they were written under.
    pub site: PathBuf,
}

/// Build every edition `cfg` plans and write them under the site root:
/// each at `book/<edition>/`, and the current one copied to the root.
///
/// `spec_dir` holds the publish spec and is where git is run; every path
/// in `cfg` is relative to it. Each edition builds from its ref's tree,
/// checked out through a scratch index, so the working tree is never
/// touched. All editions build before anything is written, so a failed
/// edition leaves the previous site in place.
pub fn build_editions(
    spec_dir: &Path,
    cfg: &PublishConfig,
    toolchain: &dyn Toolchain,
) -> Result<EditionsReport, EditionsError> {
    let editions_cfg = cfg
        .editions
        .as_ref()
        .ok_or(EditionsError::MissingEditions)?;
    let publishing = cfg
        .publishing
        .as_ref()
        .ok_or(PublishError::EditionsWithoutPublishing)?;
    // `cfg` need not have come through the publish spec's own parse.
    editions_cfg.validate(publishing)?;
    let plan = plan(publishing)?;

    // Check out from the repo's top level so a book can include files
    // from outside its own directory; the spec's paths then start from
    // its directory inside the checkout.
    let repo_relative = |arg: &str| {
        crate::publish::run_git_capture(spec_dir, &["rev-parse", arg]).map_err(|e| {
            EditionsError::NotARepository {
                dir: spec_dir.to_path_buf(),
                message: e.to_string(),
            }
        })
    };
    let top = spec_dir.join(repo_relative("--show-cdup")?.trim());
    let prefix = repo_relative("--show-prefix")?.trim().to_string();
    let sources: Vec<&str> = plan.editions.iter().map(|e| e.source.as_str()).collect();
    let commits = crate::publish::resolve_refs(spec_dir, &sources)?;

    let scratch = tempfile::tempdir().map_err(EditionsError::Scratch)?;
    let mut built: Vec<(String, PathBuf)> = Vec::new();
    for edition in &plan.editions {
        let tree = scratch.path().join("tree").join(&edition.label);
        extract_tree(&top, &commits[&edition.source], &tree).map_err(|message| {
            EditionsError::Extract {
                edition: edition.label.clone(),
                tag: edition.source.clone(),
                message,
            }
        })?;
        let book_dir = tree.join(&prefix).join(&editions_cfg.book);
        let html = build_one(
            edition,
            &book_dir,
            cfg,
            editions_cfg,
            toolchain,
            &scratch.path().join("out").join(&edition.label),
        )?;
        built.push((edition.label.clone(), html));
    }

    let site = spec_dir.join(&editions_cfg.output_dir);
    let schema_dir = schema_dir(publishing, editions_cfg);
    let current = &built
        .iter()
        .find(|(label, _)| *label == plan.current)
        .expect("the plan's current edition was built")
        .1;
    let entries = root_entries(current, &plan.current, &schema_dir)?;
    write_site(&site, &built, current, &entries, &schema_dir)?;
    Ok(EditionsReport { plan, site })
}

/// Find `panschema-publish.toml` at or above `dir`, then build every
/// edition it plans with `toolchain`. What `mdbook-panschema editions`
/// runs.
pub fn run(dir: &Path, toolchain: &dyn Toolchain) -> anyhow::Result<EditionsReport> {
    use anyhow::Context;
    let spec = crate::manifest::discover_file(dir, PUBLISH_FILENAME)
        .with_context(|| format!("no {PUBLISH_FILENAME} found at or above {}", dir.display()))?;
    let cfg =
        PublishConfig::from_path(&spec).with_context(|| format!("reading {}", spec.display()))?;
    let spec_dir = spec
        .parent()
        .context("the publish spec has no parent directory")?;
    Ok(build_editions(spec_dir, &cfg, toolchain)?)
}

/// Install one edition's assets into its extracted book and build it
/// into `dest`, returning the directory holding the rendered HTML.
fn build_one(
    edition: &Edition,
    book_dir: &Path,
    cfg: &PublishConfig,
    editions_cfg: &crate::publish::EditionsConfig,
    toolchain: &dyn Toolchain,
    dest: &Path,
) -> Result<PathBuf, EditionsError> {
    let label = || edition.label.clone();
    let tag = || edition.source.clone();
    if !book_dir.join("book.toml").is_file() {
        return Err(EditionsError::NoBook {
            edition: label(),
            tag: tag(),
            book: editions_cfg.book.clone(),
        });
    }
    let pins = read_toolchain(book_dir).map_err(|message| EditionsError::Toolchain {
        edition: label(),
        tag: tag(),
        message,
    })?;
    for plugin in pins.plugins.iter().filter(|p| !p.install.is_empty()) {
        toolchain
            .install_assets(&plugin.name, &plugin.install, book_dir)
            .map_err(|output| EditionsError::Install {
                edition: label(),
                tag: tag(),
                command: format!("{} {}", plugin.name, plugin.install.join(" ")),
                output,
            })?;
    }
    // The toolbar asset always comes from the running binary, never from
    // the tag: a tag's own copy, if it had one, is gitignored anyway.
    if let Some(link) = &cfg.book_link {
        crate::mdbook::install(book_dir, link).map_err(|e| EditionsError::Install {
            edition: label(),
            tag: tag(),
            command: "mdbook-panschema install".to_string(),
            output: format!("{e:#}"),
        })?;
    }
    toolchain
        .build(book_dir, dest)
        .map_err(|output| EditionsError::Build {
            edition: label(),
            tag: tag(),
            output,
        })
}

/// `<book>/toolchain.toml`, or the empty toolchain when the tag has none.
fn read_toolchain(book_dir: &Path) -> Result<ToolchainFile, String> {
    match std::fs::read_to_string(book_dir.join(TOOLCHAIN_FILENAME)) {
        Ok(text) => toml::from_str(&text).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ToolchainFile::default()),
        Err(e) => Err(e.to_string()),
    }
}

/// Check the tree of `commit` out under `dest` through a scratch index,
/// leaving the repo's own index and working tree alone. (`git archive`
/// would drop every path a `.gitattributes` marks `export-ignore`, which
/// repos do to keep their docs out of source tarballs.)
fn extract_tree(repo: &Path, commit: &str, dest: &Path) -> Result<(), String> {
    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    let index = scratch.path().join("index");
    let env = [("GIT_INDEX_FILE", index.as_os_str())];
    let git = |args: &[&str]| {
        crate::publish::run_git_capture_env(repo, &env, args).map_err(|e| e.to_string())
    };
    git(&["read-tree", commit])?;
    // git creates each file's leading directories by walking the `/`
    // separators in its path, so the checkout directory has to exist
    // first and the prefix has to be spelled with `/`: on Windows the
    // path's own separators are `\`, and git's first `mkdir` would be the
    // whole prefix at once, under a parent that is not there. The prefix
    // must also end in a separator, or git takes it as a filename prefix.
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let dest = dest.to_string_lossy();
    let prefix = format!(
        "--prefix={}/",
        if cfg!(windows) {
            dest.replace('\\', "/")
        } else {
            dest.into_owned()
        }
    );
    git(&["checkout-index", "--all", &prefix])?;
    Ok(())
}

/// The schema docs' directory at the site root, which the root copy may
/// not write over. `[editions]` validation guarantees the docs lie inside
/// the site, so the component exists.
fn schema_dir(
    publishing: &PublishingConfig,
    editions_cfg: &crate::publish::EditionsConfig,
) -> std::ffi::OsString {
    let site = crate::publish::lexical_components(&editions_cfg.output_dir);
    crate::publish::lexical_components(&publishing.output_dir)[site.len()]
        .as_os_str()
        .to_os_string()
}

/// Why `name` may not be an entry of the root copy: it is not one fresh
/// directory name at the site root, or it is a name the site reserves,
/// in any letter case, since a case-insensitive filesystem would merge
/// them.
fn root_entry_problem(name: &str, schema_dir: &std::ffi::OsStr) -> Option<&'static str> {
    if !crate::publish::is_single_path_segment(name) {
        Some("is not a single path segment")
    } else if name.eq_ignore_ascii_case("book") {
        Some("is the editions' `book/` directory")
    } else if std::ffi::OsStr::new(name).eq_ignore_ascii_case(schema_dir) {
        Some("is the schema docs' directory")
    } else if name.eq_ignore_ascii_case(ROOT_LEDGER_FILENAME) {
        Some("is the root ledger")
    } else {
        None
    }
}

/// The current edition's top-level entries, sorted, refusing any the copy
/// at the site root would write over.
fn root_entries(
    current: &Path,
    edition: &str,
    schema_dir: &std::ffi::OsStr,
) -> Result<Vec<String>, EditionsError> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(current).map_err(EditionsError::Scratch)? {
        let name = entry.map_err(EditionsError::Scratch)?.file_name();
        let name = name
            .into_string()
            .map_err(|name| EditionsError::RootEntryNotUtf8 {
                edition: edition.to_string(),
                entry: name.to_string_lossy().into_owned(),
            })?;
        if let Some(what) = root_entry_problem(&name, schema_dir) {
            return Err(EditionsError::RootCollision {
                edition: edition.to_string(),
                entry: name,
                what: what.to_string(),
            });
        }
        entries.push(name);
    }
    entries.sort();
    Ok(entries)
}

/// The entries a previous run's root copy wrote, from the ledger at the
/// site root; none when there is no ledger. Each is gated as a current
/// entry is, since every one is about to be removed.
fn read_ledger(site: &Path, schema_dir: &std::ffi::OsStr) -> Result<Vec<String>, EditionsError> {
    let text = match std::fs::read_to_string(site.join(ROOT_LEDGER_FILENAME)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(EditionsError::LedgerUnreadable(e)),
    };
    let mut entries = Vec::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        if let Some(problem) = root_entry_problem(line, schema_dir) {
            return Err(EditionsError::RootLedger {
                entry: line.to_string(),
                problem: problem.to_string(),
            });
        }
        entries.push(line.to_string());
    }
    Ok(entries)
}

fn write_ledger(site: &Path, entries: &[String]) -> Result<(), EditionsError> {
    let mut text = entries.join("\n");
    text.push('\n');
    std::fs::write(site.join(ROOT_LEDGER_FILENAME), text).map_err(EditionsError::Write)
}

/// Replace `site/book/` with the built editions, and the previous root
/// copy with the current edition's `entries`. The ledger lists both the
/// old and the new entries before any is touched, so a copy that fails
/// part way leaves nothing at the root the next run does not know about.
fn write_site(
    site: &Path,
    built: &[(String, PathBuf)],
    current: &Path,
    entries: &[String],
    schema_dir: &std::ffi::OsStr,
) -> Result<(), EditionsError> {
    std::fs::create_dir_all(site).map_err(EditionsError::Write)?;
    let previous = read_ledger(site, schema_dir)?;
    let union: std::collections::BTreeSet<&String> = previous.iter().chain(entries).collect();
    write_ledger(site, &union.into_iter().cloned().collect::<Vec<_>>())?;

    let book = site.join("book");
    remove_entry(&book).map_err(EditionsError::Write)?;
    for (label, html) in built {
        crate::publish::copy_dir_recursive(html, &book.join(label))
            .map_err(EditionsError::Write)?;
    }

    for name in &previous {
        remove_entry(&site.join(name)).map_err(EditionsError::Write)?;
    }
    for name in entries {
        let from = current.join(name);
        let to = site.join(name);
        remove_entry(&to).map_err(EditionsError::Write)?;
        if from.is_dir() {
            crate::publish::copy_dir_recursive(&from, &to).map_err(EditionsError::Write)?;
        } else {
            std::fs::copy(&from, &to).map_err(EditionsError::Write)?;
        }
    }
    write_ledger(site, entries)
}

/// Remove a file or directory, if there is one.
fn remove_entry(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        // Nothing there to remove; anything else wrong with the path
        // surfaces from the copy that follows.
        Err(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn publishing(body: &str) -> PublishingConfig {
        toml::from_str(body).expect("test [publishing] body should parse")
    }

    fn labels_and_sources(plan: &EditionPlan) -> Vec<(&str, &str)> {
        plan.editions
            .iter()
            .map(|e| (e.label.as_str(), e.source.as_str()))
            .collect()
    }

    #[test]
    fn a_minor_line_is_one_edition_built_from_its_newest_patch() {
        let plan = plan(&publishing(
            r#"versions = ["v0.1.0", "v0.1.1", "v0.2.0"]
               current = "v0.2.0""#,
        ))
        .unwrap();

        assert_eq!(
            labels_and_sources(&plan),
            vec![("v0.2", "v0.2.0"), ("v0.1", "v0.1.1")]
        );
        let v01 = plan.editions.iter().find(|e| e.label == "v0.1").unwrap();
        assert_eq!(v01.versions, vec!["v0.1.0", "v0.1.1"]);
    }

    #[test]
    fn after_one_point_oh_an_edition_is_a_major_line() {
        let plan = plan(&publishing(
            r#"versions = ["v1.0.0", "v1.2.3", "v2.0.0"]
               current = "v2.0.0""#,
        ))
        .unwrap();

        assert_eq!(
            labels_and_sources(&plan),
            vec![("v2", "v2.0.0"), ("v1", "v1.2.3")]
        );
    }

    /// Ordering by version, not by position, is the whole difference
    /// between building `v0.1` from its newest patch and from whichever
    /// tag the author happened to write last.
    #[test]
    fn the_newest_patch_is_chosen_by_semver_not_list_order() {
        let plan = plan(&publishing(
            r#"versions = ["v0.1.10", "v0.1.9", "v0.1.2"]
               current = "v0.1.10""#,
        ))
        .unwrap();

        assert_eq!(labels_and_sources(&plan), vec![("v0.1", "v0.1.10")]);
        assert_eq!(
            plan.editions[0].versions,
            vec!["v0.1.2", "v0.1.9", "v0.1.10"]
        );
    }

    #[test]
    fn a_release_beats_its_own_pre_release() {
        let plan = plan(&publishing(
            r#"versions = ["v0.2.0", "v0.2.0-rc.1"]
               current = "v0.2.0""#,
        ))
        .unwrap();

        assert_eq!(labels_and_sources(&plan), vec![("v0.2", "v0.2.0")]);
    }

    /// A consumer's pin never resolves to a pre-release, so the line's
    /// book describes its newest release, not a candidate after it.
    #[test]
    fn a_line_builds_from_its_newest_release_not_a_later_pre_release() {
        let plan = plan(&publishing(
            r#"versions = ["v0.2.0", "v0.2.1-rc.1"]
               current = "v0.2.0""#,
        ))
        .unwrap();

        assert_eq!(labels_and_sources(&plan), vec![("v0.2", "v0.2.0")]);
    }

    #[test]
    fn a_line_of_only_pre_releases_builds_from_the_newest_one() {
        let plan = plan(&publishing(
            r#"versions = ["v0.3.0-rc.1", "v0.3.0-rc.2"]
               current = "v0.3.0-rc.2""#,
        ))
        .unwrap();

        assert_eq!(labels_and_sources(&plan), vec![("v0.3", "v0.3.0-rc.2")]);
    }

    /// Cargo's caret treats every 0.0.z as its own incompatible line, so
    /// a pin on `0.0.1` never moves to `0.0.2`.
    #[test]
    fn each_zero_zero_release_is_its_own_edition() {
        let plan = plan(&publishing(
            r#"versions = ["v0.0.1", "v0.0.2"]
               current = "v0.0.2""#,
        ))
        .unwrap();

        assert_eq!(
            labels_and_sources(&plan),
            vec![("v0.0.2", "v0.0.2"), ("v0.0.1", "v0.0.1")]
        );
    }

    #[test]
    fn an_upper_case_v_prefix_is_a_prefix_too() {
        let plan = plan(&publishing(
            r#"versions = ["V1.0.0"]
               current = "V1.0.0""#,
        ))
        .unwrap();

        assert_eq!(labels_and_sources(&plan), vec![("V1", "V1.0.0")]);
    }

    /// Edition directories are named from the tags, so two spellings would
    /// publish `book/v0.1/` beside `book/0.2/`.
    #[test]
    fn tags_spelled_two_ways_are_refused() {
        let err = plan(&publishing(
            r#"versions = ["0.1.0", "v0.1.1", "0.2.0"]
               current = "0.2.0""#,
        ))
        .unwrap_err();

        match err {
            PublishError::MixedTagSpelling { first, second } => {
                assert_eq!((first.as_str(), second.as_str()), ("0.1.0", "v0.1.1"))
            }
            other => panic!("expected MixedTagSpelling, got {other:?}"),
        }
    }

    #[test]
    fn a_version_listed_twice_is_refused() {
        let err = plan(&publishing(
            r#"versions = ["v0.1.0", "v0.1.1", "v0.1.0"]
               current = "v0.1.1""#,
        ))
        .unwrap_err();

        match err {
            PublishError::DuplicateVersion { version } => assert_eq!(version, "v0.1.0"),
            other => panic!("expected DuplicateVersion, got {other:?}"),
        }
    }

    #[test]
    fn tags_without_a_v_prefix_keep_that_spelling() {
        let plan = plan(&publishing(
            r#"versions = ["0.1.0", "0.1.1"]
               current = "0.1.1""#,
        ))
        .unwrap();

        assert_eq!(labels_and_sources(&plan), vec![("0.1", "0.1.1")]);
    }

    #[test]
    fn the_edge_is_an_unreleased_edition_after_the_released_ones() {
        let plan = plan(&publishing(
            r#"versions = ["v0.1.1"]
               edge = "main"
               current = "v0.1.1""#,
        ))
        .unwrap();

        assert_eq!(
            labels_and_sources(&plan),
            vec![("v0.1", "v0.1.1"), ("main", "main")]
        );
        let main = &plan.editions[1];
        assert!(
            !main.is_released(),
            "the edge edition must not count as released; got {main:?}"
        );
        assert!(
            plan.editions[0].is_released(),
            "a tagged edition is released; got {:?}",
            plan.editions[0]
        );
    }

    #[test]
    fn the_current_edition_is_the_line_holding_current() {
        let plan = plan(&publishing(
            r#"versions = ["v0.1.0", "v0.1.1", "v0.2.0"]
               edge = "main"
               current = "v0.1.0""#,
        ))
        .unwrap();

        assert_eq!(plan.current, "v0.1");
    }

    #[test]
    fn current_naming_the_edge_makes_the_unreleased_edition_current() {
        let plan = plan(&publishing(
            r#"versions = ["v0.1.1"]
               edge = "main"
               current = "main""#,
        ))
        .unwrap();

        assert_eq!(plan.current, "main");
    }

    /// `plan` is public, so it can be handed a `[publishing]` that never
    /// went through the publish spec's own validation.
    #[test]
    fn a_current_in_no_edition_is_refused() {
        let err = plan(&publishing(
            r#"versions = ["v0.1.1"]
               edge = "main"
               current = "v0.9.0""#,
        ))
        .unwrap_err();

        match err {
            PublishError::InvalidCurrent { current, .. } => assert_eq!(current, "v0.9.0"),
            other => panic!("expected InvalidCurrent, got {other:?}"),
        }
    }

    #[test]
    fn a_version_that_is_not_semver_is_refused_by_name() {
        let err = plan(&publishing(
            r#"versions = ["v0.1.0", "release-2"]
               current = "v0.1.0""#,
        ))
        .unwrap_err();

        match err {
            PublishError::EditionVersionNotSemver { version } => assert_eq!(version, "release-2"),
            other => panic!("expected EditionVersionNotSemver, got {other:?}"),
        }
    }

    #[test]
    fn an_edge_label_that_is_also_an_edition_label_is_refused() {
        let err = plan(&publishing(
            r#"versions = ["v0.1.1"]
               edge = "v0.1"
               current = "v0.1.1""#,
        ))
        .unwrap_err();

        match err {
            PublishError::EdgeShadowsEdition { edge } => assert_eq!(edge, "v0.1"),
            other => panic!("expected EdgeShadowsEdition, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod build_tests {
    use super::*;
    use crate::publish::tests::{init_repo, run as git};
    use std::cell::RefCell;
    use std::fs;

    /// Builds a book so a test can see what went in: every `src/**/*.md`
    /// becomes the same path ending `.html`, holding the markdown's text,
    /// and `assets.txt` lists the book directory's top-level entries, so the
    /// installs that ran before the build are visible. A chapter holding
    /// `BROKEN` fails the build, and a plugin named `broken-plugin` fails its
    /// install.
    #[derive(Default)]
    struct FakeToolchain {
        installs: RefCell<Vec<(String, Vec<String>)>>,
        builds: RefCell<usize>,
    }

    impl Toolchain for FakeToolchain {
        fn install_assets(
            &self,
            plugin: &str,
            args: &[String],
            book_dir: &Path,
        ) -> Result<(), String> {
            self.installs
                .borrow_mut()
                .push((plugin.to_string(), args.to_vec()));
            if plugin == "broken-plugin" {
                return Err("broken-plugin: no such asset".to_string());
            }
            fs::write(book_dir.join(format!("{plugin}.css")), args.join(" "))
                .map_err(|e| e.to_string())
        }

        fn build(&self, book_dir: &Path, dest: &Path) -> Result<PathBuf, String> {
            *self.builds.borrow_mut() += 1;
            render(&book_dir.join("src"), dest)?;
            let mut entries: Vec<String> = fs::read_dir(book_dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            entries.sort();
            fs::write(dest.join("assets.txt"), entries.join("\n")).unwrap();
            Ok(dest.to_path_buf())
        }
    }

    fn render(src: &Path, dest: &Path) -> Result<(), String> {
        fs::create_dir_all(dest).unwrap();
        for entry in fs::read_dir(src).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap();
            if path.is_dir() {
                render(&path, &dest.join(name))?;
                continue;
            }
            // As mdbook does, anything but a chapter is copied as it is.
            if path.extension().is_none_or(|ext| ext != "md") {
                fs::copy(&path, dest.join(name)).unwrap();
                continue;
            }
            let text = fs::read_to_string(&path).unwrap();
            if text.contains("BROKEN") {
                return Err(format!("mdbook: cannot render {}", name.to_string_lossy()));
            }
            fs::write(dest.join(Path::new(name).with_extension("html")), text).unwrap();
        }
        Ok(())
    }

    /// Write `files` into `repo`, commit them, and tag the commit.
    fn commit(repo: &Path, files: &[(&str, &str)], tag: Option<&str>) {
        for (path, contents) in files {
            let path = repo.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        git(repo, &["add", "-A"]);
        git(repo, &["commit", "-m", tag.unwrap_or("wip"), "--quiet"]);
        if let Some(tag) = tag {
            git(repo, &["tag", tag]);
        }
    }

    /// A repo whose book's chapter names the release it was written at:
    /// `v0.1.0`, `v0.1.1`, `v0.2.0` (which adds a second chapter and a
    /// directory), then `main` past them. `site/` is ignored, as a
    /// consumer's would be.
    fn book_repo() -> tempfile::TempDir {
        let repo = init_repo();
        let p = repo.path();
        commit(
            p,
            &[
                (".gitignore", "site/\n"),
                ("book/book.toml", "[book]\ntitle = \"T\"\n"),
                ("book/src/chapter.md", "v0.1.0"),
            ],
            Some("v0.1.0"),
        );
        commit(p, &[("book/src/chapter.md", "v0.1.1")], Some("v0.1.1"));
        commit(
            p,
            &[
                ("book/src/chapter.md", "v0.2.0"),
                ("book/src/extra.md", "only in v0.2"),
                ("book/src/part/page.md", "only in v0.2"),
            ],
            Some("v0.2.0"),
        );
        commit(p, &[("book/src/chapter.md", "main")], None);
        repo
    }

    /// The fixture's publish spec, with `extra` appended to `[editions]`
    /// (or starting a later section).
    fn spec(current: &str, extra: &str) -> PublishConfig {
        format!(
            "[schema]\nname = \"x\"\nversion = \"0.2.0\"\nlinkml = \"1.7.0\"\n\
             [files]\nmain = \"schema.yaml\"\n\
             [publishing]\nversions = [\"v0.1.0\", \"v0.1.1\", \"v0.2.0\"]\n\
             edge = \"main\"\ncurrent = \"{current}\"\n\
             [editions]\n{extra}\n"
        )
        .parse()
        .expect("fixture spec should parse")
    }

    fn read(path: impl AsRef<Path>) -> String {
        let path = path.as_ref();
        fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
    }

    fn git_out(repo: &Path, args: &[&str]) -> String {
        crate::publish::run_git_capture(repo, args).unwrap()
    }

    /// Every file under `dir`, by relative path.
    fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(root, &path, out);
                } else {
                    out.insert(
                        path.strip_prefix(root).unwrap().to_path_buf(),
                        fs::read(&path).unwrap(),
                    );
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(dir, dir, &mut out);
        out
    }

    #[test]
    fn every_edition_lands_at_its_stable_path_built_from_its_own_tree() {
        let repo = book_repo();
        build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default()).unwrap();

        let book = repo.path().join("site/book");
        assert_eq!(read(book.join("v0.1/chapter.html")), "v0.1.1");
        assert_eq!(read(book.join("v0.2/chapter.html")), "v0.2.0");
        assert_eq!(read(book.join("main/chapter.html")), "main");
    }

    #[test]
    fn the_site_root_serves_a_copy_of_the_current_edition() {
        let repo = book_repo();
        build_editions(repo.path(), &spec("v0.1.0", ""), &FakeToolchain::default()).unwrap();

        assert_eq!(read(repo.path().join("site/chapter.html")), "v0.1.1");
    }

    #[test]
    fn the_schema_docs_already_in_the_site_survive() {
        let repo = book_repo();
        let schema_page = repo.path().join("site/schema/v0.1.0/index.html");
        fs::create_dir_all(schema_page.parent().unwrap()).unwrap();
        fs::write(&schema_page, "schema docs").unwrap();

        build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default()).unwrap();

        assert_eq!(read(&schema_page), "schema docs");
    }

    #[test]
    fn a_new_current_edition_leaves_nothing_of_the_old_one_at_the_root() {
        let repo = book_repo();
        let toolchain = FakeToolchain::default();
        build_editions(repo.path(), &spec("v0.2.0", ""), &toolchain).unwrap();
        assert!(
            repo.path().join("site/extra.html").is_file(),
            "v0.2's extra chapter should be at the root while v0.2 is current"
        );

        build_editions(repo.path(), &spec("v0.1.0", ""), &toolchain).unwrap();

        assert!(
            !repo.path().join("site/extra.html").exists(),
            "v0.2's extra chapter must leave the root when v0.1 becomes current"
        );
        assert!(
            !repo.path().join("site/part").exists(),
            "v0.2's directory must leave the root when v0.1 becomes current"
        );
        assert_eq!(read(repo.path().join("site/chapter.html")), "v0.1.1");
    }

    #[test]
    fn a_root_entry_that_would_overwrite_the_schema_docs_or_the_editions_is_refused() {
        for (page, entry) in [
            ("book/src/schema/page.md", "schema"),
            ("book/src/Schema/page.md", "Schema"),
            ("book/src/Book/page.md", "Book"),
            (
                "book/src/.panschema-editions-root",
                ".panschema-editions-root",
            ),
        ] {
            let repo = book_repo();
            commit(repo.path(), &[(page, "collides")], None);

            let err = build_editions(repo.path(), &spec("main", ""), &FakeToolchain::default())
                .unwrap_err();

            match err {
                EditionsError::RootCollision {
                    edition,
                    entry: got,
                    ..
                } => {
                    assert_eq!(
                        (edition.as_str(), got.as_str()),
                        ("main", entry),
                        "case {page}"
                    )
                }
                other => panic!("case {page}: expected RootCollision, got {other:?}"),
            }
            assert!(
                !repo.path().join("site").exists(),
                "case {page}: nothing may be written before the refusal"
            );
        }
    }

    #[test]
    fn the_working_tree_is_untouched() {
        let repo = book_repo();
        build_editions(
            repo.path(),
            &spec("v0.2.0", "[book_link]\nenabled = true\n"),
            &FakeToolchain::default(),
        )
        .unwrap();

        assert_eq!(git_out(repo.path(), &["status", "--porcelain"]), "");
        assert_eq!(
            git_out(repo.path(), &["worktree", "list", "--porcelain"])
                .matches("worktree ")
                .count(),
            1,
            "no worktree may be registered"
        );
    }

    #[test]
    fn plugins_the_tags_toolchain_lists_install_before_the_build() {
        let repo = book_repo();
        commit(
            repo.path(),
            &[(
                "book/toolchain.toml",
                "mdbook = \"0.5.2\"\n[[plugin]]\nname = \"mdbook-fake\"\nversion = \"1.0.0\"\ninstall = [\"install\", \".\"]\n",
            )],
            None,
        );
        let toolchain = FakeToolchain::default();

        build_editions(repo.path(), &spec("v0.2.0", ""), &toolchain).unwrap();

        let book = repo.path().join("site/book");
        assert!(
            read(book.join("main/assets.txt"))
                .lines()
                .any(|l| l == "mdbook-fake.css"),
            "main's toolchain lists mdbook-fake; got {}",
            read(book.join("main/assets.txt"))
        );
        assert!(
            !read(book.join("v0.2/assets.txt")).contains("mdbook-fake"),
            "v0.2 has no toolchain file, so nothing installs there"
        );
        assert_eq!(
            *toolchain.installs.borrow(),
            vec![(
                "mdbook-fake".to_string(),
                vec!["install".to_string(), ".".to_string()]
            )]
        );
    }

    #[test]
    fn the_toolbar_asset_installs_into_every_edition() {
        let repo = book_repo();
        build_editions(
            repo.path(),
            &spec("v0.2.0", "[book_link]\nenabled = true\n"),
            &FakeToolchain::default(),
        )
        .unwrap();

        for edition in ["v0.1", "v0.2", "main"] {
            let assets = read(repo.path().join(format!("site/book/{edition}/assets.txt")));
            assert!(
                assets.lines().any(|l| l == "schema-link.js"),
                "edition {edition} should have the toolbar asset; got {assets}"
            );
        }
    }

    #[test]
    fn a_failed_build_names_the_edition_and_tag_and_carries_the_output() {
        let repo = book_repo();
        commit(repo.path(), &[("book/src/chapter.md", "BROKEN")], None);

        let err = build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default())
            .unwrap_err();

        match err {
            EditionsError::Build {
                edition,
                tag,
                output,
            } => {
                assert_eq!((edition.as_str(), tag.as_str()), ("main", "main"));
                assert!(output.contains("cannot render chapter.md"), "got {output}");
            }
            other => panic!("expected Build, got {other:?}"),
        }
        assert!(
            !repo.path().join("site").exists(),
            "a failed edition must leave the site unwritten"
        );
    }

    #[test]
    fn a_failed_install_names_the_command_and_carries_the_output() {
        let repo = book_repo();
        commit(
            repo.path(),
            &[(
                "book/toolchain.toml",
                "[[plugin]]\nname = \"broken-plugin\"\ninstall = [\"install\"]\n",
            )],
            None,
        );

        let err = build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default())
            .unwrap_err();

        match err {
            EditionsError::Install {
                edition,
                tag,
                command,
                output,
            } => {
                assert_eq!(
                    (edition.as_str(), tag.as_str(), command.as_str()),
                    ("main", "main", "broken-plugin install")
                );
                assert!(output.contains("no such asset"), "got {output}");
            }
            other => panic!("expected Install, got {other:?}"),
        }
    }

    #[test]
    fn a_malformed_toolchain_file_names_the_edition_and_tag() {
        let repo = book_repo();
        commit(
            repo.path(),
            &[("book/toolchain.toml", "[[plugin]]\nnmae = \"typo\"\n")],
            None,
        );

        let err = build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default())
            .unwrap_err();

        match err {
            EditionsError::Toolchain {
                edition,
                tag,
                message,
            } => {
                assert_eq!((edition.as_str(), tag.as_str()), ("main", "main"));
                assert!(
                    message.contains("nmae"),
                    "the typo'd key should be named; got {message}"
                );
            }
            other => panic!("expected Toolchain, got {other:?}"),
        }
    }

    #[test]
    fn a_toolchain_path_that_cannot_be_read_is_refused_not_ignored() {
        let repo = book_repo();
        commit(
            repo.path(),
            &[("book/toolchain.toml/README", "a directory")],
            None,
        );

        let err = build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default())
            .unwrap_err();

        assert!(
            matches!(&err, EditionsError::Toolchain { edition, .. } if edition == "main"),
            "expected Toolchain for main, got {err:?}"
        );
    }

    /// The common layout this guards: the publish spec beside the schema in
    /// `schema/`, the book at the repo root.
    #[test]
    fn a_book_outside_the_specs_directory_is_read_from_the_same_tag() {
        let repo = init_repo();
        commit(
            repo.path(),
            &[
                ("sub/schema.yaml", "id: x"),
                ("book/book.toml", "[book]\n"),
                ("book/src/chapter.md", "beside"),
            ],
            Some("v0.1.0"),
        );
        let cfg: PublishConfig =
            "[schema]\nname = \"x\"\nversion = \"0.1.0\"\nlinkml = \"1.7.0\"\n\
                                  [files]\nmain = \"schema.yaml\"\n\
                                  [publishing]\nversions = [\"v0.1.0\"]\ncurrent = \"v0.1.0\"\n\
                                  [editions]\nbook = \"../book\"\n"
                .parse()
                .unwrap();

        build_editions(&repo.path().join("sub"), &cfg, &FakeToolchain::default()).unwrap();

        assert_eq!(
            read(repo.path().join("sub/site/book/v0.1/chapter.html")),
            "beside"
        );
    }

    #[test]
    fn an_edition_without_a_book_is_refused() {
        let repo = book_repo();

        let err = build_editions(
            repo.path(),
            &spec("v0.2.0", "book = \"docs\""),
            &FakeToolchain::default(),
        )
        .unwrap_err();

        match err {
            EditionsError::NoBook { book, .. } => assert_eq!(book, PathBuf::from("docs")),
            other => panic!("expected NoBook, got {other:?}"),
        }
    }

    #[test]
    fn a_spec_in_a_subdirectory_reads_its_book_relative_to_itself() {
        let repo = init_repo();
        commit(
            repo.path(),
            &[
                ("sub/book/book.toml", "[book]\n"),
                ("sub/book/src/chapter.md", "nested"),
            ],
            Some("v0.1.0"),
        );
        let cfg: PublishConfig =
            "[schema]\nname = \"x\"\nversion = \"0.1.0\"\nlinkml = \"1.7.0\"\n\
                                  [files]\nmain = \"schema.yaml\"\n\
                                  [publishing]\nversions = [\"v0.1.0\"]\ncurrent = \"v0.1.0\"\n\
                                  [editions]\n"
                .parse()
                .unwrap();

        build_editions(&repo.path().join("sub"), &cfg, &FakeToolchain::default()).unwrap();

        assert_eq!(
            read(repo.path().join("sub/site/book/v0.1/chapter.html")),
            "nested"
        );
    }

    #[test]
    fn two_runs_write_byte_identical_sites() {
        let repo = book_repo();
        let toolchain = FakeToolchain::default();
        build_editions(
            repo.path(),
            &spec("v0.2.0", "[book_link]\nenabled = true\n"),
            &toolchain,
        )
        .unwrap();
        let first = snapshot(&repo.path().join("site"));

        build_editions(
            repo.path(),
            &spec("v0.2.0", "[book_link]\nenabled = true\n"),
            &toolchain,
        )
        .unwrap();

        assert_eq!(snapshot(&repo.path().join("site")), first);
    }

    #[test]
    fn run_finds_the_publish_spec_above_the_directory_it_is_given() {
        let repo = book_repo();
        fs::write(
            repo.path().join(PUBLISH_FILENAME),
            "[schema]\nname = \"x\"\nversion = \"0.2.0\"\nlinkml = \"1.7.0\"\n\
             [files]\nmain = \"schema.yaml\"\n\
             [publishing]\nversions = [\"v0.1.1\"]\ncurrent = \"v0.1.1\"\n[editions]\n",
        )
        .unwrap();

        let report = run(&repo.path().join("book/src"), &FakeToolchain::default()).unwrap();

        assert_eq!(report.plan.current, "v0.1");
        assert_eq!(read(repo.path().join("site/chapter.html")), "v0.1.1");
    }

    /// A ledger line is trusted with a `remove_dir_all`, so a ledger that
    /// something else wrote (or a merge mangled) is refused before any of
    /// it is acted on.
    #[test]
    fn a_ledger_naming_anything_outside_the_root_copy_is_refused() {
        for (line, problem_word) in [
            ("..", "segment"),
            ("a/b", "segment"),
            ("schema", "schema docs"),
            ("Book", "book/"),
            (".panschema-editions-root", "ledger"),
        ] {
            let repo = book_repo();
            let site = repo.path().join("site");
            let schema_page = site.join("schema/v0.1.0/index.html");
            fs::create_dir_all(schema_page.parent().unwrap()).unwrap();
            fs::write(&schema_page, "schema docs").unwrap();
            let sibling = repo.path().join("keep.txt");
            fs::write(&sibling, "outside the site").unwrap();
            fs::write(site.join(ROOT_LEDGER_FILENAME), format!("{line}\n")).unwrap();

            let err = build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default())
                .unwrap_err();

            match err {
                EditionsError::RootLedger { entry, problem } => {
                    assert_eq!(entry, line);
                    assert!(
                        problem.contains(problem_word),
                        "line `{line}`: the problem should mention {problem_word}; got {problem}"
                    );
                }
                other => panic!("line `{line}`: expected RootLedger, got {other:?}"),
            }
            assert!(
                schema_page.is_file(),
                "line `{line}`: the schema docs must survive"
            );
            assert!(
                sibling.is_file(),
                "line `{line}`: nothing outside the site may go"
            );
            assert!(
                !site.join("book").exists(),
                "line `{line}`: a refused ledger must stop the run before the editions are written"
            );
        }
    }

    /// The copy is recorded before it starts, so a copy that fails part
    /// way leaves nothing at the root the next run does not know about.
    /// `part` is made unremovable by permission, so this needs a user the
    /// permission binds, which a CI runner is.
    #[cfg(unix)]
    #[test]
    fn an_interrupted_root_copy_is_still_recorded_for_the_next_run() {
        use std::os::unix::fs::PermissionsExt;
        let repo = book_repo();
        let toolchain = FakeToolchain::default();
        let site = repo.path().join("site");
        // v0.2's root entries copy in name order: chapter.html, extra.html,
        // then part, which cannot be replaced.
        let blocker = site.join("part");
        fs::create_dir_all(&blocker).unwrap();
        fs::write(blocker.join("x"), "").unwrap();
        fs::set_permissions(&blocker, fs::Permissions::from_mode(0o000)).unwrap();

        let result = build_editions(repo.path(), &spec("v0.2.0", ""), &toolchain);
        fs::set_permissions(&blocker, fs::Permissions::from_mode(0o755)).unwrap();

        assert!(
            matches!(result, Err(EditionsError::Write(_))),
            "an unremovable root entry should fail the write; got {result:?}"
        );
        assert!(
            site.join("extra.html").is_file(),
            "the entries before the failure were already written"
        );
        build_editions(repo.path(), &spec("v0.1.0", ""), &toolchain).unwrap();
        assert!(
            !site.join("extra.html").exists(),
            "the interrupted copy's entries must leave the root with the next run"
        );
    }

    /// `git archive` drops every path `.gitattributes` marks `export-ignore`,
    /// which a repo does to keep its docs out of source tarballs; an
    /// edition must still have them.
    #[test]
    fn a_path_marked_export_ignore_is_still_in_its_edition() {
        let repo = book_repo();
        commit(
            repo.path(),
            &[(".gitattributes", "book/src/extra.md export-ignore\n")],
            None,
        );

        build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default()).unwrap();

        assert_eq!(
            read(repo.path().join("site/book/main/extra.html")),
            "only in v0.2"
        );
    }

    /// A config built without going through the publish spec's own
    /// parse can place the schema docs anywhere; the checks run here too,
    /// before any edition builds.
    #[test]
    fn an_unvalidated_config_is_checked_before_anything_builds() {
        let repo = book_repo();
        let cfg: PublishConfig = toml::from_str(
            "[schema]\nname = \"x\"\nversion = \"0.2.0\"\nlinkml = \"1.7.0\"\n\
             [files]\nmain = \"schema.yaml\"\n\
             [publishing]\nversions = [\"v0.1.1\"]\ncurrent = \"v0.1.1\"\noutput_dir = \"site\"\n\
             [editions]\noutput_dir = \"site\"\n",
        )
        .unwrap();
        let toolchain = FakeToolchain::default();

        let err = build_editions(repo.path(), &cfg, &toolchain).unwrap_err();

        assert!(
            matches!(
                err,
                EditionsError::Publish(PublishError::SchemaOutsideSite { .. })
            ),
            "expected SchemaOutsideSite, got {err:?}"
        );
        assert_eq!(*toolchain.builds.borrow(), 0, "nothing may build first");
    }

    /// A name the site cannot serve is refused rather than recorded in
    /// the ledger as something else. Only Linux lets a test create one.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_root_entry_whose_name_is_not_utf8_is_refused() {
        use std::os::unix::ffi::OsStrExt;
        let repo = book_repo();
        fs::write(
            repo.path()
                .join("book/src")
                .join(std::ffi::OsStr::from_bytes(b"caf\xe9.png")),
            "",
        )
        .unwrap();
        commit(repo.path(), &[], None);

        let err =
            build_editions(repo.path(), &spec("main", ""), &FakeToolchain::default()).unwrap_err();

        match err {
            EditionsError::RootEntryNotUtf8 { edition, entry } => {
                assert_eq!(edition, "main");
                assert!(entry.starts_with("caf"), "got {entry}");
            }
            other => panic!("expected RootEntryNotUtf8, got {other:?}"),
        }
    }

    #[test]
    fn a_stray_file_named_book_at_the_site_root_is_replaced() {
        let repo = book_repo();
        fs::create_dir_all(repo.path().join("site")).unwrap();
        fs::write(repo.path().join("site/book"), "not a directory").unwrap();

        build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default()).unwrap();

        assert_eq!(
            read(repo.path().join("site/book/v0.2/chapter.html")),
            "v0.2.0"
        );
    }

    #[test]
    fn a_spec_outside_a_repository_says_so() {
        let dir = tempfile::tempdir().unwrap();

        let err =
            build_editions(dir.path(), &spec("v0.2.0", ""), &FakeToolchain::default()).unwrap_err();

        match err {
            EditionsError::NotARepository { dir: got, .. } => assert_eq!(got, dir.path()),
            other => panic!("expected NotARepository, got {other:?}"),
        }
    }

    #[test]
    fn the_html_root_is_a_subdirectory_only_with_several_backends() {
        let dest = Path::new("out");
        for (book_toml, root) in [
            ("[book]\ntitle = \"T\"\n", "out"),
            ("[output]\n", "out"),
            ("[output.html]\n", "out"),
            ("[output.markdown]\n", "out"),
            ("[output.html]\n[output.markdown]\n", "out/html"),
            (
                "[output.html]\n[output.markdown]\n[output.linkcheck]\n",
                "out/html",
            ),
        ] {
            assert_eq!(
                html_root(book_toml, dest),
                Path::new(root),
                "book.toml:\n{book_toml}"
            );
        }
    }

    #[test]
    fn a_ledger_that_cannot_be_read_is_refused_not_ignored() {
        let repo = book_repo();
        fs::create_dir_all(repo.path().join("site").join(ROOT_LEDGER_FILENAME)).unwrap();

        let err = build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default())
            .unwrap_err();

        assert!(
            matches!(err, EditionsError::LedgerUnreadable(_)),
            "a directory where the ledger should be is an error, not an absent ledger; got {err:?}"
        );
    }

    #[test]
    #[ignore = "needs mdbook 0.5 on PATH; run with cargo nextest run --run-ignored only -E 'test(real_mdbook)'"]
    fn real_mdbook_with_two_backends_reports_the_html_root() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book");
        fs::create_dir_all(book.join("src")).unwrap();
        fs::write(
            book.join("book.toml"),
            "[book]\ntitle = \"T\"\n[output.html]\n[output.markdown]\n",
        )
        .unwrap();
        fs::write(
            book.join("src/SUMMARY.md"),
            "# Summary\n\n- [Chapter](chapter.md)\n",
        )
        .unwrap();
        fs::write(book.join("src/chapter.md"), "# Chapter\n").unwrap();

        let root = PathToolchain::default()
            .build(&book, &dir.path().join("out"))
            .unwrap();

        assert_eq!(root, dir.path().join("out/html"));
        assert!(root.join("chapter.html").is_file());
    }

    #[test]
    fn a_spec_without_editions_is_refused() {
        let repo = book_repo();
        let cfg: PublishConfig =
            "[schema]\nname = \"x\"\nversion = \"0.1.0\"\nlinkml = \"1.7.0\"\n\
                                  [files]\nmain = \"schema.yaml\"\n\
                                  [publishing]\nversions = [\"v0.1.0\"]\ncurrent = \"v0.1.0\"\n"
                .parse()
                .unwrap();

        let err = build_editions(repo.path(), &cfg, &FakeToolchain::default()).unwrap_err();

        assert!(
            matches!(err, EditionsError::MissingEditions),
            "expected MissingEditions, got {err:?}"
        );
    }

    #[test]
    fn the_path_toolchain_reports_a_failing_tools_output() {
        let dir = tempfile::tempdir().unwrap();
        let toolchain = PathToolchain {
            mdbook: PathBuf::from("git"),
        };

        PathToolchain::default()
            .install_assets("git", &["--version".to_string()], dir.path())
            .expect("a tool that succeeds is Ok");
        let install = PathToolchain::default()
            .install_assets("git", &["no-such-subcommand".to_string()], dir.path())
            .unwrap_err();
        let build = toolchain
            .build(dir.path(), &dir.path().join("out"))
            .unwrap_err();

        assert!(install.contains("no-such-subcommand"), "got {install}");
        assert!(build.contains("build"), "got {build}");
    }

    #[test]
    fn the_path_toolchain_names_a_tool_it_cannot_find() {
        let dir = tempfile::tempdir().unwrap();

        let err = PathToolchain::default()
            .install_assets("panschema-no-such-tool", &[], dir.path())
            .unwrap_err();

        assert!(err.contains("panschema-no-such-tool"), "got {err}");
    }

    #[test]
    #[ignore = "needs mdbook 0.5 on PATH; run with cargo nextest run --run-ignored only -E 'test(real_mdbook)'"]
    fn real_mdbook_builds_into_the_destination() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book");
        fs::create_dir_all(book.join("src")).unwrap();
        fs::write(book.join("book.toml"), "[book]\ntitle = \"T\"\n").unwrap();
        fs::write(
            book.join("src/SUMMARY.md"),
            "# Summary\n\n- [Chapter](chapter.md)\n",
        )
        .unwrap();
        fs::write(book.join("src/chapter.md"), "# Chapter\n").unwrap();

        let root = PathToolchain::default()
            .build(&book, &dir.path().join("out"))
            .unwrap();

        assert_eq!(root, dir.path().join("out"));
        assert!(root.join("chapter.html").is_file());
    }
}
