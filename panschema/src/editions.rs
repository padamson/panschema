//! Book editions: the books an author declares, each covering a span of
//! schema releases.
//!
//! An edition is a book directory on the edge ref. It names the first
//! release it covers, and the next edition's start closes it, so the spans
//! partition the release list `panschema publish` reads (`[publishing]`):
//! the book and the schema docs agree on which edition every version
//! belongs to. The edge belongs to the last edition.
//!
//! Reference: [`docs/features/54-book-editions-as-declared-spans.md`](../../docs/features/54-book-editions-as-declared-spans.md)

use std::path::{Path, PathBuf};

use semver::Version;

use crate::publish::{
    BookLinkEntry, DeclaredEdition, EditionsConfig, PUBLISH_FILENAME, PublishConfig, PublishError,
    PublishingConfig,
};

pub mod toolchain;
pub use toolchain::{
    InstalledToolchain, PinConflict, PluginPin, ToolError, Toolchain, ToolchainPins,
};

/// One edition of the book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edition {
    /// The directory the edition lands in under `book/`.
    pub id: String,
    /// What the switcher and the banner call it.
    pub name: String,
    /// The directory holding its `book.toml`, relative to the publish spec.
    pub book: PathBuf,
    /// The versions in its span, pre-releases among them, oldest first.
    pub versions: Vec<String>,
    /// The version whose schema docs the book describes: its newest
    /// release, or the edge for an unreleased edition.
    pub schema_version: String,
    /// Whether new chapters and listings go into it.
    pub authoring: bool,
}

impl Edition {
    /// Whether the edition covers a release yet. A pre-release is not
    /// one, so an edition holding only those is still unreleased.
    pub fn is_released(&self) -> bool {
        crate::publish::newest_release(&self.versions).is_some()
    }
}

/// Every declared edition with the releases it covers, and which one is
/// current.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditionPlan {
    /// Newest first.
    pub editions: Vec<Edition>,
    /// The id of the edition the site root serves.
    pub current: String,
    /// The ref every edition builds from, which the newest edition holds.
    pub edge: String,
}

impl EditionPlan {
    /// The edition holding `version`: the span a release falls in, or the
    /// newest edition for the edge. The book's toolbar and the schema
    /// page's brand both ask this, so they agree.
    pub fn edition_of(&self, version: &str) -> Option<&Edition> {
        if version == self.edge {
            return self.editions.first();
        }
        self.editions
            .iter()
            .find(|e| e.versions.iter().any(|v| v == version))
    }

    /// The edition new chapters and listings go into.
    pub fn authoring(&self) -> &Edition {
        self.editions
            .iter()
            .find(|e| e.authoring)
            .expect("a plan marks one edition authoring")
    }

    /// What the toolbar asset of edition `own` is told: every edition in
    /// switcher order, the current one, and itself.
    pub fn toolbar_context(&self, own: &str) -> crate::mdbook::EditionsContext {
        crate::mdbook::EditionsContext {
            editions: self
                .editions
                .iter()
                .map(|e| crate::mdbook::EditionOption {
                    label: e.id.clone(),
                    unreleased: !e.is_released(),
                })
                .collect(),
            current: self.current.clone(),
            own: own.to_string(),
        }
    }
}

/// Check `editions` against `[publishing]`, partition its release list
/// among the declared editions, and name the current one.
///
/// Fails when the release list has not been read from the tags it was
/// left to, when the config is refused (`EditionsConfig::check` says
/// why), as `editions_of` does, and when `current` is neither a
/// release nor the edge.
pub fn plan(
    editions: &EditionsConfig,
    publishing: &PublishingConfig,
) -> Result<EditionPlan, PublishError> {
    let (versions, current) = publishing.releases()?;
    let spans = editions.check(publishing)?;
    let edge = publishing
        .edge
        .clone()
        .expect("the check refuses editions without an edge");
    let mut plan = EditionPlan {
        editions: editions_of(&spans, versions, &edge)?,
        current: String::new(),
        edge,
    };
    plan.current = plan
        .edition_of(current)
        .map(|e| e.id.clone())
        .ok_or_else(|| PublishError::InvalidCurrent {
            current: current.to_string(),
            versions: versions.to_vec(),
            edge: publishing.edge.clone(),
        })?;
    Ok(plan)
}

/// A declaration that passed `declared`'s checks, with its start parsed.
pub struct Span<'a> {
    edition: &'a DeclaredEdition,
    start: Version,
}

/// Check the declarations on their own, without a release list, and
/// return them oldest first.
///
/// Fails when there is none, when an id is not one path segment or a
/// `from` not semver, when two share an id (in any letter case, since a
/// case-insensitive filesystem would merge their directories), a start,
/// or a book, and when two are marked authoring.
pub(crate) fn declared(editions: &[DeclaredEdition]) -> Result<Vec<Span<'_>>, PublishError> {
    if editions.is_empty() {
        return Err(PublishError::NoEditionDeclared);
    }
    let mut spans: Vec<Span<'_>> = Vec::with_capacity(editions.len());
    for edition in editions {
        let id = || edition.id.clone();
        if !crate::publish::is_single_path_segment(&edition.id) {
            return Err(crate::publish::not_a_segment("an edition id", &edition.id));
        }
        // A Windows drive-relative path (`C:book`) has a prefix and no
        // root, and joining it to a directory replaces the directory.
        let anchored = edition.book.components().any(|c| {
            matches!(
                c,
                std::path::Component::RootDir | std::path::Component::Prefix(_)
            )
        });
        if anchored {
            return Err(PublishError::EditionBookNotRelative {
                id: id(),
                book: edition.book.clone(),
            });
        }
        let start = crate::publish::tag_semver(&edition.from)
            .map(|(_, version)| release_of(&version))
            .ok_or_else(|| PublishError::EditionFromNotSemver {
                id: id(),
                from: edition.from.clone(),
            })?;
        for seen in &spans {
            let shared = if seen.edition.id.eq_ignore_ascii_case(&edition.id) {
                Some(Shared::Id)
            } else if seen.start == start {
                Some(Shared::Start(start.clone()))
            } else if same_book(&seen.edition.book, &edition.book) {
                Some(Shared::Book(edition.book.clone()))
            } else {
                None
            };
            if let Some(shared) = shared {
                return Err(PublishError::EditionsShare {
                    first: seen.edition.id.clone(),
                    second: id(),
                    shared,
                });
            }
            if seen.edition.authoring && edition.authoring {
                return Err(PublishError::TwoAuthoringEditions {
                    first: seen.edition.id.clone(),
                    second: id(),
                });
            }
        }
        spans.push(Span { edition, start });
    }
    spans.sort_by(|a, b| a.start.cmp(&b.start));
    Ok(spans)
}

/// What two declared editions may not share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shared {
    /// The id, compared in any letter case.
    Id,
    /// The first release covered.
    Start(Version),
    /// The book directory.
    Book(PathBuf),
}

impl std::fmt::Display for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Shared::Id => f.write_str("an id"),
            Shared::Start(start) => write!(f, "the start `{start}`"),
            Shared::Book(book) => write!(f, "the book `{}`", book.display()),
        }
    }
}

/// Whether two book paths name one directory: compared after `.` and
/// `..` drop out, and in any letter case, since a case-insensitive
/// filesystem would read both from one place.
fn same_book(a: &Path, b: &Path) -> bool {
    let spelled = |path: &Path| {
        crate::publish::lexical_components(path)
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .collect::<Vec<_>>()
    };
    spelled(a) == spelled(b)
}

/// The editions `spans` form over a release list, newest first, without
/// deciding which is current; an edition covering no release describes
/// `edge`.
///
/// Fails when a version does not parse as semver or is listed twice
/// (under one spelling or two), when a release is older than every
/// edition, or when an edition but the last covers no release.
pub(crate) fn editions_of(
    spans: &[Span<'_>],
    versions: &[String],
    edge: &str,
) -> Result<Vec<Edition>, PublishError> {
    let mut held: Vec<Vec<(Version, &str)>> = vec![Vec::new(); spans.len()];
    for tag in versions {
        let (_, version) = crate::publish::tag_semver(tag).ok_or_else(|| {
            PublishError::EditionVersionNotSemver {
                version: tag.clone(),
            }
        })?;
        // A pre-release belongs to the edition of the release it precedes.
        let release = release_of(&version);
        let index = spans
            .iter()
            .rposition(|span| span.start <= release)
            .ok_or_else(|| PublishError::ReleaseBeforeFirstEdition {
                version: tag.clone(),
                edition: spans[0].edition.id.clone(),
                from: spans[0].edition.from.clone(),
            })?;
        if let Some((_, first)) = held[index].iter().find(|(seen, _)| *seen == version) {
            return Err(PublishError::DuplicateVersion {
                first: first.to_string(),
                second: tag.clone(),
            });
        }
        held[index].push((version, tag));
    }

    let last = spans.len() - 1;
    let authoring = spans
        .iter()
        .position(|s| s.edition.authoring)
        .unwrap_or(last);
    let mut out = Vec::with_capacity(spans.len());
    for (index, (span, mut tags)) in spans.iter().zip(held).enumerate() {
        tags.sort_by(|a, b| a.0.cmp(&b.0));
        let versions: Vec<String> = tags.iter().map(|(_, tag)| tag.to_string()).collect();
        // A pin never resolves to a pre-release, so the book describes the
        // span's newest release. Without one the edition is unreleased
        // and describes the edge it is built from.
        let released = crate::publish::newest_release(&versions);
        if released.is_none() && index != last {
            return Err(PublishError::EmptyEdition {
                id: span.edition.id.clone(),
            });
        }
        let schema_version = released.unwrap_or_else(|| edge.to_string());
        let decl = span.edition;
        out.push(Edition {
            id: decl.id.clone(),
            name: decl.name.clone().unwrap_or_else(|| decl.id.clone()),
            book: decl.book.clone(),
            versions,
            schema_version,
            authoring: index == authoring,
        });
    }
    out.reverse();
    Ok(out)
}

/// `version` without its pre-release and build metadata: the release a
/// span boundary is compared on.
fn release_of(version: &Version) -> Version {
    Version::new(version.major, version.minor, version.patch)
}

/// The file a book directory pinned its tools in before
/// `[editions.toolchain]` did.
const SUPERSEDED_TOOLCHAIN_FILE: &str = "toolchain.toml";

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
    #[error("could not read the tree at `{reference}`: {message}")]
    Extract { reference: String, message: String },
    #[error("edition `{edition}`: there is no `book.toml` in `{}` at the edge", book.display())]
    NoBook { edition: String, book: PathBuf },
    #[error(
        "edition `{edition}`: `{}` is not read; put its pins in [editions.toolchain] of {PUBLISH_FILENAME} and delete it",
        file.display()
    )]
    ToolchainFileNotRead { edition: String, file: PathBuf },
    #[error(
        "edition `{edition}`: its book `{}` lies outside the repository, which is all the edge holds",
        book.display()
    )]
    BookOutsideRepository { edition: String, book: PathBuf },
    #[error("edition `{edition}`: could not install `{tool}` {pin}:\n{output}")]
    ToolchainInstall {
        edition: String,
        tool: String,
        pin: String,
        output: String,
    },
    #[error("edition `{edition}`: `{command}` failed:\n{output}")]
    Install {
        edition: String,
        command: String,
        output: String,
    },
    #[error("edition `{edition}` did not build:\n{output}")]
    Build { edition: String, output: String },
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

/// What [`build_editions`] published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditionsReport {
    /// The editions built, and the current one.
    pub plan: EditionPlan,
    /// The site root they were written under.
    pub site: PathBuf,
}

/// Build every edition `cfg` declares and write them under the site root:
/// each at `book/<id>/`, and the current one copied to the root.
///
/// `spec_dir` holds the publish spec and is where git is run; every path
/// in `cfg` is relative to it. The editions build from the edge ref's
/// tree, checked out through a scratch index, so the working tree is never
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
    let listed = cfg
        .publishing
        .as_ref()
        .ok_or(PublishError::EditionsWithoutPublishing)?;
    let publishing = &listed.resolve_releases(spec_dir)?;
    // Planning checks the config again: `cfg` need not have come through
    // the publish spec's own parse, and a release list left to the tags
    // was not known to it.
    let plan = plan(editions_cfg, publishing)
        .map_err(|err| crate::publish::hint_if_derived(err, listed, publishing))?;

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
    // Every book is placed before any is built, so a refused one stops the
    // run before the others spend a build. The checkout holds the
    // repository and nothing above it; a book that climbs out would be
    // read, and written to, somewhere else.
    let books: Vec<PathBuf> = plan
        .editions
        .iter()
        .map(|edition| {
            let in_repo = Path::new(&prefix).join(&edition.book);
            if crate::publish::climbs_out(&in_repo) {
                return Err(EditionsError::BookOutsideRepository {
                    edition: edition.id.clone(),
                    book: edition.book.clone(),
                });
            }
            Ok(in_repo)
        })
        .collect::<Result<_, _>>()?;
    let commits = crate::publish::resolve_refs(spec_dir, &[plan.edge.as_str()])?;

    let scratch = tempfile::tempdir().map_err(EditionsError::Scratch)?;
    let tree = scratch.path().join("tree");
    extract_tree(&top, &commits[&plan.edge], &tree).map_err(|message| EditionsError::Extract {
        reference: plan.edge.clone(),
        message,
    })?;
    for (edition, book) in plan.editions.iter().zip(&books) {
        if !tree.join(book).join("book.toml").is_file() {
            return Err(EditionsError::NoBook {
                edition: edition.id.clone(),
                book: edition.book.clone(),
            });
        }
        // Pins once lived beside the book. Building past the file would
        // take every tool from `PATH` without saying so.
        if tree.join(book).join(SUPERSEDED_TOOLCHAIN_FILE).is_file() {
            return Err(EditionsError::ToolchainFileNotRead {
                edition: edition.id.clone(),
                file: edition.book.join(SUPERSEDED_TOOLCHAIN_FILE),
            });
        }
    }
    let mut built: Vec<(String, PathBuf)> = Vec::new();
    for (edition, book) in plan.editions.iter().zip(&books) {
        let html = build_one(
            edition,
            &plan,
            &tree.join(book),
            cfg,
            &editions_cfg.toolchain,
            toolchain,
            &scratch.path().join("out").join(&edition.id),
        )?;
        built.push((edition.id.clone(), html));
    }

    let site = spec_dir.join(&editions_cfg.output_dir);
    let schema_dir = schema_dir(publishing, editions_cfg);
    let current = &built
        .iter()
        .find(|(id, _)| *id == plan.current)
        .expect("the plan's current edition was built")
        .1;
    let entries = root_entries(current, &plan.current, &schema_dir)?;
    // The copy at the site root is two directories shallower than the
    // stable path, so its canonicals differ: it is taken from the build
    // before the stable copy gets its own.
    let root_copy = scratch.path().join("root");
    crate::publish::copy_dir_recursive(current, &root_copy).map_err(EditionsError::Scratch)?;
    inject_canonicals(&root_copy, &root_copy, &plan.current, 0).map_err(EditionsError::Scratch)?;
    for (id, html) in &built {
        inject_canonicals(html, html, id, 2).map_err(EditionsError::Scratch)?;
    }
    write_site(&site, &built, &root_copy, &entries, &schema_dir)?;
    Ok(EditionsReport { plan, site })
}

/// Find `panschema-publish.toml` at or above `dir`, then build every
/// edition it declares with `toolchain`. What `mdbook-panschema editions`
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

/// Install one edition's assets into its book in the checkout and build
/// it into `dest`, returning the directory holding the rendered HTML.
fn build_one(
    edition: &Edition,
    plan: &EditionPlan,
    book_dir: &Path,
    cfg: &PublishConfig,
    pins: &ToolchainPins,
    toolchain: &dyn Toolchain,
    dest: &Path,
) -> Result<PathBuf, EditionsError> {
    let id = || edition.id.clone();
    for plugin in pins.plugins.iter().filter(|p| !p.install.is_empty()) {
        toolchain
            .install_assets(pins, &plugin.name, &plugin.install, book_dir)
            .map_err(|e| {
                tool_error(edition, e, |output| EditionsError::Install {
                    edition: id(),
                    command: format!("{} {}", plugin.name, plugin.install.join(" ")),
                    output,
                })
            })?;
    }
    // The toolbar asset always comes from the running binary: the book's
    // own copy, if it has one, is gitignored anyway. An entry aliasing
    // `current/` is pointed at the version the edition describes, so a
    // reader reaches the schema docs the book was written against.
    let links: Vec<BookLinkEntry> = cfg
        .book_link
        .as_ref()
        .filter(|link| link.enabled())
        .map(|link| link.entries())
        .unwrap_or_default()
        .into_iter()
        .map(|mut entry| {
            let alias = entry.schema_path == "current/" || entry.schema_path.ends_with("/current/");
            if alias {
                let base = &entry.schema_path[..entry.schema_path.len() - "current/".len()];
                entry.schema_path = format!("{base}{}/", edition.schema_version);
            }
            entry
        })
        .collect();
    let context = plan.toolbar_context(&edition.id);
    crate::mdbook::install_toolbar(book_dir, &links, Some(&context)).map_err(|e| {
        EditionsError::Install {
            edition: id(),
            command: "mdbook-panschema install".to_string(),
            output: format!("{e:#}"),
        }
    })?;
    toolchain.build(pins, book_dir, dest).map_err(|e| {
        tool_error(edition, e, |output| EditionsError::Build {
            edition: id(),
            output,
        })
    })
}

/// A tool's failure as the edition's: an install failure names the tool
/// and its pin; a run failure is whatever the caller makes of its output.
fn tool_error(
    edition: &Edition,
    error: ToolError,
    on_run: impl FnOnce(String) -> EditionsError,
) -> EditionsError {
    match error {
        ToolError::Install { tool, pin, output } => EditionsError::ToolchainInstall {
            edition: edition.id.clone(),
            tool,
            pin,
            output,
        },
        ToolError::Run { output } => on_run(output),
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

/// Percent-encode one URL path segment, leaving raw exactly the
/// characters `encodeURIComponent` leaves raw, so the toolbar asset can
/// spell an edition's label the way its canonical does.
pub(crate) fn encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The canonical URL of the page at `rel` (its path inside the edition,
/// one segment per component) in `edition`, relative to a copy of the
/// page whose edition root sits `dirs_above` directories below the site
/// root: 2 at the stable path `book/<edition>/`, 0 for the copy at the
/// site root. Relative, so it resolves under any site prefix; every
/// segment percent-encoded, which also keeps `"` and `&` out of the
/// attribute.
fn canonical_href(edition: &str, rel: &[String], dirs_above: usize) -> String {
    let climb = "../".repeat(dirs_above + rel.len().saturating_sub(1));
    let page = rel
        .iter()
        .map(|segment| encode_segment(segment))
        .collect::<Vec<_>>()
        .join("/");
    format!("{climb}book/{}/{page}", encode_segment(edition))
}

/// Where a page's `<head>` tag ends, if it has one. `<header>` is not it.
fn head_end(html: &str) -> Option<usize> {
    html.match_indices("<head").find_map(|(start, _)| {
        let after = html[start + "<head".len()..].chars().next()?;
        if after != '>' && !after.is_whitespace() {
            return None;
        }
        html[start..].find('>').map(|close| start + close + 1)
    })
}

/// Give every page under `dir`, a copy of `edition` whose root is `root`,
/// a canonical link to itself at the edition's stable path. A file with
/// no `<head>` is left alone, and so is the top-level `404.html`: GitHub
/// Pages serves it at whatever depth the missing URL had, where a
/// page-relative link points somewhere different on every miss.
fn inject_canonicals(
    root: &Path,
    dir: &Path,
    edition: &str,
    dirs_above: usize,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            inject_canonicals(root, &path, edition, dirs_above)?;
            continue;
        }
        if path.extension().is_none_or(|ext| ext != "html") {
            continue;
        }
        let rel: Vec<String> = path
            .strip_prefix(root)
            .expect("a page lies under its edition root")
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        if rel == ["404.html"] {
            continue;
        }
        let Ok(html) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(at) = head_end(&html) else {
            continue;
        };
        let href = canonical_href(edition, &rel, dirs_above);
        std::fs::write(
            &path,
            format!(
                r#"{}
<link rel="canonical" href="{href}">{}"#,
                &html[..at],
                &html[at..]
            ),
        )?;
    }
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
    root_copy: &Path,
    entries: &[String],
    schema_dir: &std::ffi::OsStr,
) -> Result<(), EditionsError> {
    std::fs::create_dir_all(site).map_err(EditionsError::Write)?;
    let previous = read_ledger(site, schema_dir)?;
    let union: std::collections::BTreeSet<&String> = previous.iter().chain(entries).collect();
    write_ledger(site, &union.into_iter().cloned().collect::<Vec<_>>())?;

    let book = site.join("book");
    remove_entry(&book).map_err(EditionsError::Write)?;
    for (id, html) in built {
        crate::publish::copy_dir_recursive(html, &book.join(id)).map_err(EditionsError::Write)?;
    }

    for name in &previous {
        remove_entry(&site.join(name)).map_err(EditionsError::Write)?;
    }
    for name in entries {
        let from = root_copy.join(name);
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
    use crate::publish::tests::edition_toml as edition;

    fn publishing(body: &str) -> PublishingConfig {
        toml::from_str(body).expect("test [publishing] body should parse")
    }

    /// `[[editions.edition]]` tables, parsed without the publish spec's
    /// checks.
    fn editions_config(body: &str) -> EditionsConfig {
        #[derive(serde::Deserialize)]
        struct Spec {
            editions: EditionsConfig,
        }
        toml::from_str::<Spec>(body)
            .expect("test [editions] body should parse")
            .editions
    }

    fn declared_editions(body: &str) -> Vec<DeclaredEdition> {
        editions_config(body).editions
    }

    /// Two editions: `1` from `v0.1.0`, `2` from `v0.3.0`.
    fn two_editions() -> String {
        edition("1", "book/1", "v0.1.0") + &edition("2", "book/2", "v0.3.0")
    }

    fn plan_of(publishing_body: &str, editions_body: &str) -> Result<EditionPlan, PublishError> {
        plan(
            &editions_config(editions_body),
            &publishing(publishing_body),
        )
    }

    fn spans(plan: &EditionPlan) -> Vec<(&str, Vec<&str>)> {
        plan.editions
            .iter()
            .map(|e| {
                (
                    e.id.as_str(),
                    e.versions.iter().map(String::as_str).collect(),
                )
            })
            .collect()
    }

    fn id_of<'a>(plan: &'a EditionPlan, version: &str) -> Option<&'a str> {
        plan.edition_of(version).map(|e| e.id.as_str())
    }

    #[test]
    fn head_end_is_the_index_after_the_head_tag_and_never_a_header() {
        for (html, end) in [
            ("<html><head></head>", Some(12)),
            ("<html><head lang=\"en\">x", Some(22)),
            ("<html>\n    <head>\n", Some(17)),
            ("<header>x</header><head>", Some(24)),
            ("<header>only</header>", None),
            ("<p>no head</p>", None),
            ("<head", None),
        ] {
            assert_eq!(head_end(html), end, "html: {html:?}");
        }
    }

    /// The asset rebuilds the same spelling with `encodeURIComponent`
    /// to find the page's edition in its canonical, so the two sets of
    /// characters left raw must agree.
    #[test]
    fn encode_segment_spells_as_encodeuricomponent_does() {
        for (raw, encoded) in [
            ("v0.1", "v0.1"),
            ("release candidate", "release%20candidate"),
            ("faq & tips.html", "faq%20%26%20tips.html"),
            ("what?.html", "what%3F.html"),
            ("a-b_c.d!e~f*g'h(i)", "a-b_c.d!e~f*g'h(i)"),
            ("caf\u{e9}", "caf%C3%A9"),
            ("x\"y#z", "x%22y%23z"),
        ] {
            assert_eq!(encode_segment(raw), encoded, "raw: {raw:?}");
        }
    }

    #[test]
    fn plan_refuses_a_release_list_that_has_not_been_read_from_the_tags() {
        // Either field left to the tags is enough: a listed `current`
        // beside a derived `versions` is not a plan of the edge alone.
        for body in [
            "edge = \"main\"",
            "current = \"main\"\nedge = \"main\"",
            "versions = [\"v0.1.0\"]\nedge = \"main\"",
        ] {
            let err = plan_of(body, &two_editions()).unwrap_err();
            assert!(
                matches!(err, PublishError::ReleasesUnresolved),
                "body {body:?}: expected ReleasesUnresolved, got {err:?}"
            );
        }
    }

    #[test]
    fn plan_refuses_a_release_list_without_an_edge_to_build_from() {
        let err = plan_of(
            "versions = [\"v0.1.0\"]\ncurrent = \"v0.1.0\"",
            &edition("1", "book/1", "v0.1.0"),
        )
        .unwrap_err();

        assert!(
            matches!(err, PublishError::EditionsWithoutEdge),
            "expected EditionsWithoutEdge, got {err:?}"
        );
    }

    #[test]
    fn each_release_belongs_to_the_edition_with_the_latest_start_at_or_below_it() {
        let plan = plan_of(
            r#"versions = ["v0.1.0", "v0.1.1", "v0.2.0", "v0.3.0", "v0.3.1"]
               edge = "main"
               current = "v0.3.1""#,
            &two_editions(),
        )
        .unwrap();

        assert_eq!(
            spans(&plan),
            vec![
                ("2", vec!["v0.3.0", "v0.3.1"]),
                ("1", vec!["v0.1.0", "v0.1.1", "v0.2.0"]),
            ]
        );
        assert_eq!(id_of(&plan, "main"), Some("2"), "the edge is the newest's");
    }

    #[test]
    fn the_order_editions_are_declared_in_does_not_matter() {
        let releases = r#"versions = ["v0.1.0", "v0.3.0"]
                          edge = "main"
                          current = "v0.3.0""#;
        let reversed = edition("2", "book/2", "v0.3.0") + &edition("1", "book/1", "v0.1.0");

        assert_eq!(
            plan_of(releases, &reversed).unwrap(),
            plan_of(releases, &two_editions()).unwrap()
        );
    }

    /// `v0.3.0-rc.1` sorts below `v0.3.0` in semver, but it previews the
    /// release the second edition starts at, so it belongs there.
    #[test]
    fn a_pre_release_belongs_to_the_edition_of_the_release_it_precedes() {
        let plan = plan_of(
            r#"versions = ["v0.1.0", "v0.3.0-rc.1"]
               edge = "main"
               current = "v0.1.0""#,
            &two_editions(),
        )
        .unwrap();

        assert_eq!(id_of(&plan, "v0.3.0-rc.1"), Some("2"));
    }

    /// Ordering by version, not by position, is the difference between
    /// linking the span's newest release and whichever tag the author
    /// listed last; a pin never resolves to a pre-release, so a release
    /// wins over a later candidate, and candidates alone leave the
    /// edition unreleased.
    #[test]
    fn an_edition_describes_its_newest_release_by_semver_and_the_edge_while_it_has_none() {
        for (versions, expected, released) in [
            (r#"["v0.1.10", "v0.1.9", "v0.1.2"]"#, "v0.1.10", true),
            (r#"["v0.1.0", "v0.1.1-rc.1"]"#, "v0.1.0", true),
            (r#"["v0.1.0-rc.1", "v0.1.0-rc.2"]"#, "main", false),
        ] {
            let plan = plan_of(
                &format!("versions = {versions}\nedge = \"main\"\ncurrent = \"main\""),
                &edition("1", "book/1", "v0.1.0"),
            )
            .unwrap();
            let only = &plan.editions[0];
            assert_eq!(
                (only.schema_version.as_str(), only.is_released()),
                (expected, released),
                "versions {versions}"
            );
        }
    }

    #[test]
    fn the_last_edition_without_a_release_is_unreleased_and_describes_the_edge() {
        let plan = plan_of(
            r#"versions = ["v0.1.0", "v0.2.0"]
               edge = "main"
               current = "v0.2.0""#,
            &two_editions(),
        )
        .unwrap();

        let second = &plan.editions[0];
        assert_eq!(second.id, "2");
        assert!(!second.is_released(), "got {second:?}");
        assert_eq!(second.schema_version, "main");
        assert!(
            plan.editions[1].is_released(),
            "the first edition covers both releases; got {:?}",
            plan.editions[1]
        );
    }

    #[test]
    fn edition_of_finds_the_span_holding_a_version_and_the_newest_edition_for_the_edge() {
        let plan = plan_of(
            r#"versions = ["v0.1.0", "v0.3.0"]
               edge = "main"
               current = "v0.3.0""#,
            &two_editions(),
        )
        .unwrap();

        assert_eq!(id_of(&plan, "v0.1.0"), Some("1"));
        assert_eq!(id_of(&plan, "v0.3.0"), Some("2"));
        assert_eq!(id_of(&plan, "main"), Some("2"));
        assert_eq!(id_of(&plan, "v0.2.0"), None, "an unlisted version");
    }

    #[test]
    fn the_current_edition_is_the_one_holding_current_or_the_newest_for_the_edge() {
        for (current, expected) in [("v0.1.0", "1"), ("v0.3.0", "2"), ("main", "2")] {
            let plan = plan_of(
                &format!(
                    "versions = [\"v0.1.0\", \"v0.3.0\"]\nedge = \"main\"\ncurrent = \"{current}\""
                ),
                &two_editions(),
            )
            .unwrap();
            assert_eq!(plan.current, expected, "current {current}");
        }
    }

    /// `plan` is public, so it can be handed a `[publishing]` that never
    /// went through the publish spec's own validation.
    #[test]
    fn a_current_in_no_edition_is_refused() {
        let err = plan_of(
            r#"versions = ["v0.1.0"]
               edge = "main"
               current = "v0.9.0""#,
            &two_editions(),
        )
        .unwrap_err();

        match err {
            PublishError::InvalidCurrent { current, .. } => assert_eq!(current, "v0.9.0"),
            other => panic!("expected InvalidCurrent, got {other:?}"),
        }
    }

    #[test]
    fn an_edition_is_named_by_its_id_unless_it_has_a_name() {
        let plan = plan_of(
            r#"versions = ["v0.1.0", "v0.3.0"]
               edge = "main"
               current = "v0.3.0""#,
            &(two_editions() + "name = \"Second edition\"\n"),
        )
        .unwrap();

        let names: Vec<(&str, &str)> = plan
            .editions
            .iter()
            .map(|e| (e.id.as_str(), e.name.as_str()))
            .collect();
        assert_eq!(names, vec![("2", "Second edition"), ("1", "1")]);
    }

    #[test]
    fn the_last_edition_is_authoring_unless_another_is_marked() {
        let releases = r#"versions = ["v0.1.0", "v0.3.0"]
                          edge = "main"
                          current = "v0.3.0""#;

        let unmarked = plan_of(releases, &two_editions()).unwrap();
        assert_eq!(unmarked.authoring().id, "2");

        let marked = edition("1", "book/1", "v0.1.0")
            + "authoring = true\n"
            + &edition("2", "book/2", "v0.3.0");
        let marked = plan_of(releases, &marked).unwrap();
        assert_eq!(marked.authoring().id, "1");
        assert_eq!(
            marked.editions.iter().filter(|e| e.authoring).count(),
            1,
            "exactly one edition is authoring; got {:?}",
            marked.editions
        );
    }

    #[test]
    fn two_authoring_editions_are_refused_naming_both() {
        let body = edition("1", "book/1", "v0.1.0")
            + "authoring = true\n"
            + &edition("2", "book/2", "v0.3.0")
            + "authoring = true\n";

        match declared(&declared_editions(&body)).err() {
            Some(PublishError::TwoAuthoringEditions { first, second }) => {
                assert_eq!((first.as_str(), second.as_str()), ("1", "2"))
            }
            other => panic!("expected TwoAuthoringEditions, got {other:?}"),
        }
    }

    /// Declared newest first, so the edition named is the first by start,
    /// not by position.
    #[test]
    fn a_release_older_than_every_edition_is_refused_naming_it_and_the_first_edition() {
        let err = plan_of(
            r#"versions = ["v0.0.9", "v0.1.0"]
               edge = "main"
               current = "v0.1.0""#,
            &(edition("2", "book/2", "v0.3.0") + &edition("1", "book/1", "v0.1.0")),
        )
        .unwrap_err();

        match err {
            PublishError::ReleaseBeforeFirstEdition {
                version,
                edition,
                from,
            } => assert_eq!(
                (version.as_str(), edition.as_str(), from.as_str()),
                ("v0.0.9", "1", "v0.1.0")
            ),
            other => panic!("expected ReleaseBeforeFirstEdition, got {other:?}"),
        }
    }

    /// A pre-release is not a release, so a span holding only one is as
    /// empty as a span holding nothing.
    #[test]
    fn an_edition_before_the_last_that_covers_no_release_is_refused() {
        let three = two_editions() + &edition("3", "book/3", "v0.4.0");

        for versions in [
            r#"["v0.1.0", "v0.4.0"]"#,
            r#"["v0.1.0", "v0.3.0-rc.1", "v0.4.0"]"#,
        ] {
            let err = plan_of(
                &format!("versions = {versions}\nedge = \"main\"\ncurrent = \"v0.4.0\""),
                &three,
            )
            .unwrap_err();

            match err {
                PublishError::EmptyEdition { id } => assert_eq!(id, "2", "versions {versions}"),
                other => panic!("versions {versions}: expected EmptyEdition, got {other:?}"),
            }
        }
    }

    /// A start is compared as a release, so two spellings of one version
    /// are one start; an id or a book in another letter case is the same
    /// directory on a case-insensitive filesystem; a book path is compared
    /// after `.` and a trailing separator drop out.
    #[test]
    fn editions_sharing_an_id_a_start_or_a_book_are_refused_naming_both() {
        let start = Version::new(0, 1, 0);
        for (second, shared) in [
            (edition("a", "book/2", "v0.3.0"), Shared::Id),
            (edition("A", "book/2", "v0.3.0"), Shared::Id),
            (
                edition("b", "book/2", "v0.1.0"),
                Shared::Start(start.clone()),
            ),
            (edition("b", "book/2", "0.1.0"), Shared::Start(start)),
            (
                edition("b", "./book/a/", "v0.3.0"),
                Shared::Book(PathBuf::from("./book/a/")),
            ),
            (
                edition("b", "Book/A", "v0.3.0"),
                Shared::Book(PathBuf::from("Book/A")),
            ),
        ] {
            let body = edition("a", "book/a", "v0.1.0") + &second;
            match declared(&declared_editions(&body)).err() {
                Some(PublishError::EditionsShare {
                    first,
                    second: got,
                    shared: got_shared,
                }) => assert_eq!(
                    (first.as_str(), got, got_shared),
                    ("a", declared_editions(&second)[0].id.clone(), shared),
                    "case:\n{second}"
                ),
                other => panic!("case:\n{second}\nexpected EditionsShare, got {other:?}"),
            }
        }
    }

    #[test]
    fn what_two_editions_share_is_named_with_its_value() {
        for (shared, token) in [
            (Shared::Id, "id"),
            (Shared::Start(Version::new(0, 1, 0)), "0.1.0"),
            (Shared::Book(PathBuf::from("book/a")), "book/a"),
        ] {
            let said = shared.to_string();
            assert!(
                said.contains(token),
                "{shared:?} should name `{token}`; got: {said}"
            );
        }
    }

    /// A book is read from the edge's checkout, never from wherever an
    /// absolute path points, nor a Windows path relative to a drive or to
    /// the current drive's root.
    #[test]
    fn a_book_that_is_not_a_relative_path_is_refused() {
        let books: &[&str] = if cfg!(windows) {
            &["C:\\\\book", "C:book", "\\\\book"]
        } else {
            &["/book"]
        };
        for book in books {
            match declared(&declared_editions(&edition("1", book, "v0.1.0"))).err() {
                Some(PublishError::EditionBookNotRelative { id, .. }) => {
                    assert_eq!(id, "1", "book {book}")
                }
                other => panic!("book {book}: expected EditionBookNotRelative, got {other:?}"),
            }
        }
    }

    #[test]
    fn an_id_that_is_not_one_path_segment_is_refused() {
        for id in ["", ".", "..", "a/b", "a\\\\b"] {
            match declared(&declared_editions(&edition(id, "book/1", "v0.1.0"))).err() {
                Some(PublishError::InvalidSegment { what, value, .. }) => assert_eq!(
                    (what.as_str(), value),
                    ("an edition id", id.replace("\\\\", "\\"))
                ),
                other => panic!("id {id:?}: expected InvalidSegment, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_from_that_is_not_semver_is_refused_naming_the_edition() {
        match declared(&declared_editions(&edition("1", "book/1", "first"))).err() {
            Some(PublishError::EditionFromNotSemver { id, from }) => {
                assert_eq!((id.as_str(), from.as_str()), ("1", "first"))
            }
            other => panic!("expected EditionFromNotSemver, got {other:?}"),
        }
    }

    #[test]
    fn no_declared_edition_is_refused() {
        let err = declared(&[]).err();
        assert!(
            matches!(err, Some(PublishError::NoEditionDeclared)),
            "expected NoEditionDeclared, got {err:?}"
        );
    }

    #[test]
    fn a_version_that_is_not_semver_is_refused_by_name() {
        let err = plan_of(
            r#"versions = ["v0.1.0", "release-2"]
               edge = "main"
               current = "v0.1.0""#,
            &two_editions(),
        )
        .unwrap_err();

        match err {
            PublishError::EditionVersionNotSemver { version } => assert_eq!(version, "release-2"),
            other => panic!("expected EditionVersionNotSemver, got {other:?}"),
        }
    }

    /// Two spellings of one version are one version, and the error names
    /// both so the author can find the pair.
    #[test]
    fn a_version_listed_twice_is_refused_naming_both_spellings() {
        for (versions, pair) in [
            (r#"["v0.1.0", "v0.1.1", "v0.1.0"]"#, ("v0.1.0", "v0.1.0")),
            (r#"["v0.1.0", "v0.1.1", "0.1.0"]"#, ("v0.1.0", "0.1.0")),
        ] {
            let err = plan_of(
                &format!("versions = {versions}\nedge = \"main\"\ncurrent = \"v0.1.1\""),
                &two_editions(),
            )
            .unwrap_err();

            match err {
                PublishError::DuplicateVersion { first, second } => {
                    assert_eq!((first.as_str(), second.as_str()), pair)
                }
                other => panic!("versions {versions}: expected DuplicateVersion, got {other:?}"),
            }
        }
    }
}

#[cfg(test)]
mod build_tests {
    use super::*;
    use crate::publish::tests::{edition_toml as edition, init_repo, run as git, spec_declaring};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::fs;

    /// Builds a book so a test can see what went in: every `src/**/*.md`
    /// becomes the same path ending `.html`, holding the markdown's text,
    /// and `assets.txt` lists the book directory's top-level entries, so the
    /// installs that ran before the build are visible. A chapter holding
    /// `BROKEN` fails the build, and in a book directory named `2` a plugin
    /// named `broken-plugin` fails its install and one named
    /// `uninstallable` cannot be installed, so which edition fails does not
    /// depend on the order they build in.
    #[derive(Default)]
    struct FakeToolchain {
        installs: RefCell<Vec<(String, Vec<String>)>>,
        /// The pins each build was given, in build order.
        builds: RefCell<Vec<ToolchainPins>>,
    }

    impl Toolchain for FakeToolchain {
        fn install_assets(
            &self,
            _pins: &ToolchainPins,
            plugin: &str,
            args: &[String],
            book_dir: &Path,
        ) -> Result<(), ToolError> {
            self.installs
                .borrow_mut()
                .push((plugin.to_string(), args.to_vec()));
            let second = book_dir.ends_with("2");
            if second && plugin == "broken-plugin" {
                return Err(ToolError::Run {
                    output: "broken-plugin: no such asset".to_string(),
                });
            }
            if second && plugin == "uninstallable" {
                return Err(ToolError::Install {
                    tool: plugin.to_string(),
                    pin: "9.9.9".to_string(),
                    output: "error: no such crate".to_string(),
                });
            }
            fs::write(book_dir.join(format!("{plugin}.css")), args.join(" ")).map_err(|e| {
                ToolError::Run {
                    output: e.to_string(),
                }
            })
        }

        fn build(
            &self,
            pins: &ToolchainPins,
            book_dir: &Path,
            dest: &Path,
        ) -> Result<PathBuf, ToolError> {
            self.builds.borrow_mut().push(pins.clone());
            render(&book_dir.join("src"), dest).map_err(|output| ToolError::Run { output })?;
            // As mdbook does, the additional assets land beside the pages.
            for entry in fs::read_dir(book_dir).unwrap() {
                let path = entry.unwrap().path();
                if path
                    .extension()
                    .is_some_and(|ext| ext == "js" || ext == "css")
                {
                    fs::copy(&path, dest.join(path.file_name().unwrap())).unwrap();
                }
            }
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
            fs::write(
                dest.join(Path::new(name).with_extension("html")),
                page(&text),
            )
            .unwrap();
        }
        Ok(())
    }

    /// A page shaped the way mdbook writes one: `<head>` on its own line,
    /// the chapter inside `<body>`.
    fn page(body: &str) -> String {
        format!(
            "<!DOCTYPE HTML>\n<html>\n    <head>\n    </head>\n    <body>{body}</body>\n</html>\n"
        )
    }

    /// The chapter text of a page [`page`] wrote.
    fn body(path: impl AsRef<Path>) -> String {
        let text = read(path);
        let start = text.find("<body>").expect("a rendered page has a body") + "<body>".len();
        let end = text
            .find("</body>")
            .expect("a rendered page closes its body");
        text[start..end].to_string()
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

    /// A repo with two edition books on `main`: `book/1/`, whose chapter
    /// reads `first`, and `book/2/`, whose chapter reads `second` beside an
    /// extra chapter and a directory. The tags `v0.1.0`, `v0.1.1` and
    /// `v0.2.0` hold earlier text (`first at v0.1.1`, and so on), so a page
    /// that reads `first` came from the edge. `site/` is ignored, as a
    /// consumer's would be.
    fn book_repo() -> tempfile::TempDir {
        let repo = init_repo();
        let p = repo.path();
        commit(
            p,
            &[
                (".gitignore", "site/\n"),
                ("book/1/book.toml", "[book]\ntitle = \"First\"\n"),
                ("book/1/src/chapter.md", "first at v0.1.0"),
            ],
            Some("v0.1.0"),
        );
        commit(
            p,
            &[("book/1/src/chapter.md", "first at v0.1.1")],
            Some("v0.1.1"),
        );
        commit(
            p,
            &[
                ("book/2/book.toml", "[book]\ntitle = \"Second\"\n"),
                ("book/2/src/chapter.md", "second at v0.2.0"),
                ("book/2/src/extra.md", "only in the second"),
                ("book/2/src/part/page.md", "only in the second"),
            ],
            Some("v0.2.0"),
        );
        commit(
            p,
            &[
                ("book/1/src/chapter.md", "first"),
                ("book/2/src/chapter.md", "second"),
            ],
            None,
        );
        repo
    }

    /// The fixture's two editions: `1` from `v0.1.0` and `2` from `v0.2.0`.
    const EDITIONS: &str = "[[editions.edition]]\nid = \"1\"\nbook = \"book/1\"\nfrom = \"v0.1.0\"\n\
                            [[editions.edition]]\nid = \"2\"\nbook = \"book/2\"\nfrom = \"v0.2.0\"\n";

    /// The fixture's publish spec, with `extra` after the editions (a key
    /// of the last edition, or a later table).
    fn spec(current: &str, extra: &str) -> PublishConfig {
        spec_with(
            &format!(
                "versions = [\"v0.1.0\", \"v0.1.1\", \"v0.2.0\"]\n\
                 edge = \"main\"\ncurrent = \"{current}\""
            ),
            &format!("{EDITIONS}{extra}"),
        )
    }

    /// A publish spec with the given `[publishing]` and `[editions]` bodies.
    fn spec_with(publishing: &str, editions: &str) -> PublishConfig {
        spec_declaring(publishing, "", editions)
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
    fn the_releases_come_from_the_tags_when_the_release_list_is_omitted() {
        let repo = book_repo();

        let report = build_editions(
            repo.path(),
            &spec_with("edge = \"main\"", EDITIONS),
            &FakeToolchain::default(),
        )
        .unwrap();

        let planned: Vec<(&str, &[String])> = report
            .plan
            .editions
            .iter()
            .map(|e| (e.id.as_str(), e.versions.as_slice()))
            .collect();
        assert_eq!(
            planned,
            [
                ("2", &["v0.2.0".to_string()][..]),
                ("1", &["v0.1.0".to_string(), "v0.1.1".to_string()][..])
            ]
        );
        assert_eq!(report.plan.current, "2");
    }

    #[test]
    fn every_edition_lands_at_its_id_built_from_the_edge() {
        let repo = book_repo();
        build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default()).unwrap();

        let book = repo.path().join("site/book");
        assert_eq!(body(book.join("1/chapter.html")), "first");
        assert_eq!(body(book.join("2/chapter.html")), "second");
    }

    /// The edge ref, not the checkout, is what publishes: an edit nobody
    /// committed is not in the site.
    #[test]
    fn an_uncommitted_edit_is_not_published() {
        let repo = book_repo();
        fs::write(repo.path().join("book/1/src/chapter.md"), "uncommitted").unwrap();

        build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default()).unwrap();

        assert_eq!(body(repo.path().join("site/book/1/chapter.html")), "first");
    }

    /// The second edition's book does not exist at `v0.1.1`, so a build
    /// that read the checked-out commit could not have written it.
    #[test]
    fn the_editions_build_from_the_edge_whatever_is_checked_out() {
        let repo = book_repo();
        git(repo.path(), &["checkout", "--quiet", "v0.1.1"]);

        build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default()).unwrap();

        let book = repo.path().join("site/book");
        assert_eq!(body(book.join("1/chapter.html")), "first");
        assert_eq!(body(book.join("2/chapter.html")), "second");
    }

    #[test]
    fn the_editions_land_under_the_configured_site_root() {
        let repo = book_repo();
        let cfg = spec_with(
            "versions = [\"v0.1.0\", \"v0.1.1\", \"v0.2.0\"]\nedge = \"main\"\n\
             current = \"v0.2.0\"\noutput_dir = \"public/schema\"",
            &format!("output_dir = \"public\"\n{EDITIONS}"),
        );

        let report = build_editions(repo.path(), &cfg, &FakeToolchain::default()).unwrap();

        assert_eq!(report.site, repo.path().join("public"));
        assert_eq!(
            body(repo.path().join("public/book/1/chapter.html")),
            "first"
        );
        assert_eq!(body(repo.path().join("public/chapter.html")), "second");
        assert!(
            !repo.path().join("site").exists(),
            "nothing lands in the default"
        );
    }

    /// The refused book is the older edition's, so a check made as each
    /// edition builds would already have built the newer one.
    #[test]
    fn a_book_outside_the_repository_is_refused_before_anything_builds() {
        let repo = book_repo();
        let toolchain = FakeToolchain::default();
        let cfg = spec_with(
            "versions = [\"v0.1.0\", \"v0.2.0\"]\nedge = \"main\"\ncurrent = \"v0.2.0\"",
            &(edition("1", "../elsewhere", "v0.1.0") + &edition("2", "book/2", "v0.2.0")),
        );

        let err = build_editions(repo.path(), &cfg, &toolchain).unwrap_err();

        match err {
            EditionsError::BookOutsideRepository { edition, book } => {
                assert_eq!(
                    (edition.as_str(), book),
                    ("1", PathBuf::from("../elsewhere"))
                )
            }
            other => panic!("expected BookOutsideRepository, got {other:?}"),
        }
        assert!(
            toolchain.installs.borrow().is_empty() && toolchain.builds.borrow().is_empty(),
            "nothing may install or build first"
        );
    }

    #[test]
    fn an_unreleased_last_edition_builds_and_is_current_for_the_edge() {
        let repo = book_repo();
        commit(
            repo.path(),
            &[
                ("book/3/book.toml", "[book]\n"),
                ("book/3/src/chapter.md", "third"),
            ],
            None,
        );
        let cfg = spec_with(
            "versions = [\"v0.1.0\", \"v0.1.1\", \"v0.2.0\"]\nedge = \"main\"\ncurrent = \"main\"",
            &(EDITIONS.to_string() + &edition("3", "book/3", "v0.3.0")),
        );

        let report = build_editions(repo.path(), &cfg, &FakeToolchain::default()).unwrap();

        assert_eq!(report.plan.current, "3");
        assert_eq!(body(repo.path().join("site/book/3/chapter.html")), "third");
        assert_eq!(body(repo.path().join("site/chapter.html")), "third");
    }

    #[test]
    fn the_site_root_serves_a_copy_of_the_current_edition() {
        let repo = book_repo();
        build_editions(repo.path(), &spec("v0.1.0", ""), &FakeToolchain::default()).unwrap();

        assert_eq!(body(repo.path().join("site/chapter.html")), "first");
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
            "the second edition's extra chapter should be at the root while it is current"
        );

        build_editions(repo.path(), &spec("v0.1.0", ""), &toolchain).unwrap();

        assert!(
            !repo.path().join("site/extra.html").exists(),
            "the second edition's extra chapter must leave the root when the first becomes current"
        );
        assert!(
            !repo.path().join("site/part").exists(),
            "the second edition's directory must leave the root when the first becomes current"
        );
        assert_eq!(body(repo.path().join("site/chapter.html")), "first");
    }

    #[test]
    fn a_root_entry_that_would_overwrite_the_schema_docs_or_the_editions_is_refused() {
        for (page, entry) in [
            ("book/2/src/schema/page.md", "schema"),
            ("book/2/src/Schema/page.md", "Schema"),
            ("book/2/src/Book/page.md", "Book"),
            (
                "book/2/src/.panschema-editions-root",
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
                        ("2", entry),
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
    fn the_toolchains_plugins_install_into_every_edition_before_its_build() {
        let repo = book_repo();
        let toolchain = FakeToolchain::default();
        let cfg = spec(
            "v0.2.0",
            "[editions.toolchain]\nmdbook = \"0.5.2\"\n\
             [[editions.toolchain.plugin]]\nname = \"mdbook-fake\"\nversion = \"1.0.0\"\ninstall = [\"install\", \".\"]\n",
        );

        build_editions(repo.path(), &cfg, &toolchain).unwrap();

        for id in ["1", "2"] {
            let assets = read(repo.path().join(format!("site/book/{id}/assets.txt")));
            assert!(
                assets.lines().any(|l| l == "mdbook-fake.css"),
                "edition {id} should have mdbook-fake's assets; got {assets}"
            );
        }
        let install = (
            "mdbook-fake".to_string(),
            vec!["install".to_string(), ".".to_string()],
        );
        assert_eq!(*toolchain.installs.borrow(), vec![install.clone(), install]);
    }

    #[test]
    fn every_edition_builds_with_the_one_toolchain() {
        let repo = book_repo();
        let toolchain = FakeToolchain::default();

        build_editions(
            repo.path(),
            &spec("v0.2.0", "[editions.toolchain]\nmdbook = \"0.5.3\"\n"),
            &toolchain,
        )
        .unwrap();

        let mdbooks: Vec<Option<String>> = toolchain
            .builds
            .borrow()
            .iter()
            .map(|pins| pins.mdbook.clone())
            .collect();
        assert_eq!(mdbooks, vec![Some("0.5.3".into()), Some("0.5.3".into())]);
    }

    #[test]
    fn without_a_toolchain_every_edition_builds_with_the_tools_on_path() {
        let repo = book_repo();
        let toolchain = FakeToolchain::default();

        build_editions(repo.path(), &spec("v0.2.0", ""), &toolchain).unwrap();

        assert_eq!(
            *toolchain.builds.borrow(),
            vec![ToolchainPins::default(), ToolchainPins::default()]
        );
    }

    /// The switcher needs the asset whether or not the book links to a
    /// schema, and a book whose `book.toml` never wired it gets it wired
    /// in the checkout.
    #[test]
    fn the_toolbar_asset_installs_into_every_edition() {
        let repo = book_repo();
        build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default()).unwrap();

        for edition in ["1", "2"] {
            let assets = read(repo.path().join(format!("site/book/{edition}/assets.txt")));
            assert!(
                assets.lines().any(|l| l == "schema-link.js"),
                "edition {edition} should have the toolbar asset; got {assets}"
            );
        }
    }

    #[test]
    fn a_failed_build_names_the_edition_and_carries_the_output() {
        let repo = book_repo();
        commit(repo.path(), &[("book/1/src/chapter.md", "BROKEN")], None);

        let err = build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default())
            .unwrap_err();

        match err {
            EditionsError::Build { edition, output } => {
                assert_eq!(edition, "1");
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
    fn a_failed_install_names_the_edition_and_command_and_carries_the_output() {
        let repo = book_repo();
        let cfg = spec(
            "v0.2.0",
            "[[editions.toolchain.plugin]]\nname = \"broken-plugin\"\ninstall = [\"install\"]\n",
        );

        let err = build_editions(repo.path(), &cfg, &FakeToolchain::default()).unwrap_err();

        match err {
            EditionsError::Install {
                edition,
                command,
                output,
            } => {
                assert_eq!(
                    (edition.as_str(), command.as_str()),
                    ("2", "broken-plugin install")
                );
                assert!(output.contains("no such asset"), "got {output}");
            }
            other => panic!("expected Install, got {other:?}"),
        }
    }

    #[test]
    fn a_tool_that_cannot_be_installed_names_the_edition_tool_and_pin() {
        let repo = book_repo();
        let cfg = spec(
            "v0.2.0",
            "[[editions.toolchain.plugin]]\nname = \"uninstallable\"\nversion = \"9.9.9\"\ninstall = [\"install\"]\n",
        );

        let err = build_editions(repo.path(), &cfg, &FakeToolchain::default()).unwrap_err();

        match err {
            EditionsError::ToolchainInstall {
                edition,
                tool,
                pin,
                output,
            } => {
                assert_eq!(
                    (edition.as_str(), tool.as_str(), pin.as_str()),
                    ("2", "uninstallable", "9.9.9")
                );
                assert!(output.contains("no such crate"), "got {output}");
            }
            other => panic!("expected ToolchainInstall, got {other:?}"),
        }
    }

    /// The common layout this guards: the publish spec beside the schema in
    /// `schema/`, the books at the repo root.
    #[test]
    fn a_book_outside_the_specs_directory_is_read_from_the_same_ref() {
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
        let cfg = spec_with(
            "versions = [\"v0.1.0\"]\nedge = \"main\"\ncurrent = \"v0.1.0\"",
            &edition("1", "../book", "v0.1.0"),
        );

        build_editions(&repo.path().join("sub"), &cfg, &FakeToolchain::default()).unwrap();

        assert_eq!(
            body(repo.path().join("sub/site/book/1/chapter.html")),
            "beside"
        );
    }

    /// The file is the older edition's, which builds last.
    #[test]
    fn a_toolchain_file_beside_a_book_is_refused_before_anything_builds() {
        let repo = book_repo();
        commit(
            repo.path(),
            &[("book/1/toolchain.toml", "mdbook = \"0.5.2\"\n")],
            None,
        );
        let toolchain = FakeToolchain::default();

        let err = build_editions(repo.path(), &spec("v0.2.0", ""), &toolchain).unwrap_err();

        match &err {
            EditionsError::ToolchainFileNotRead { edition, file } => assert_eq!(
                (edition.as_str(), file.as_path()),
                ("1", Path::new("book/1/toolchain.toml"))
            ),
            other => panic!("expected ToolchainFileNotRead, got {other:?}"),
        }
        assert!(err.to_string().contains("[editions.toolchain]"), "{err}");
        assert!(
            toolchain.builds.borrow().is_empty(),
            "nothing may build first; built {:?}",
            toolchain.builds.borrow()
        );
    }

    /// The fixture's tags start at `v0.1.0` and the only edition at
    /// `v0.2.0`, with the release list left to the tags.
    #[test]
    fn a_tag_older_than_every_edition_names_the_release_list_as_the_way_out() {
        let repo = book_repo();
        let from_tags = spec_with("edge = \"main\"", &edition("2", "book/2", "v0.2.0"));

        let err = build_editions(repo.path(), &from_tags, &FakeToolchain::default()).unwrap_err();

        assert!(
            matches!(&err, EditionsError::Publish(PublishError::DerivedTag { tag, source })
                if tag == "v0.1.0"
                    && matches!(**source, PublishError::ReleaseBeforeFirstEdition { .. })),
            "expected DerivedTag over ReleaseBeforeFirstEdition, got {err:?}"
        );
    }

    /// The missing book is the older edition's, which builds last.
    #[test]
    fn an_edition_without_a_book_is_refused_before_anything_builds() {
        let repo = book_repo();
        let toolchain = FakeToolchain::default();
        let cfg = spec_with(
            "versions = [\"v0.1.0\", \"v0.2.0\"]\nedge = \"main\"\ncurrent = \"v0.2.0\"",
            &(edition("1", "docs", "v0.1.0") + &edition("2", "book/2", "v0.2.0")),
        );

        let err = build_editions(repo.path(), &cfg, &toolchain).unwrap_err();

        match err {
            EditionsError::NoBook { edition, book } => {
                assert_eq!((edition.as_str(), book), ("1", PathBuf::from("docs")))
            }
            other => panic!("expected NoBook, got {other:?}"),
        }
        assert!(
            toolchain.builds.borrow().is_empty(),
            "nothing may build first; built {:?}",
            toolchain.builds.borrow()
        );
    }

    #[test]
    fn a_spec_in_a_subdirectory_reads_its_books_relative_to_itself() {
        let repo = init_repo();
        commit(
            repo.path(),
            &[
                ("sub/book/book.toml", "[book]\n"),
                ("sub/book/src/chapter.md", "nested"),
            ],
            Some("v0.1.0"),
        );
        let cfg = spec_with(
            "versions = [\"v0.1.0\"]\nedge = \"main\"\ncurrent = \"v0.1.0\"",
            &edition("1", "book", "v0.1.0"),
        );

        build_editions(&repo.path().join("sub"), &cfg, &FakeToolchain::default()).unwrap();

        assert_eq!(
            body(repo.path().join("sub/site/book/1/chapter.html")),
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
            spec_declaring(
                "versions = [\"v0.1.1\"]\nedge = \"main\"\ncurrent = \"v0.1.1\"",
                "",
                &edition("1", "book/1", "v0.1.0"),
            ),
        )
        .unwrap();

        let report = run(&repo.path().join("book/1/src"), &FakeToolchain::default()).unwrap();

        assert_eq!(report.plan.current, "1");
        assert_eq!(body(repo.path().join("site/chapter.html")), "first");
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
        // The second edition's root entries copy in name order:
        // chapter.html, extra.html, then part, which cannot be replaced.
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
            &[(".gitattributes", "book/2/src/extra.md export-ignore\n")],
            None,
        );

        build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default()).unwrap();

        assert_eq!(
            body(repo.path().join("site/book/2/extra.html")),
            "only in the second"
        );
    }

    /// A config built without going through the publish spec's own
    /// parse can place the schema docs anywhere; the checks run here too,
    /// before any edition builds.
    #[test]
    fn an_unvalidated_config_is_checked_before_anything_builds() {
        let repo = book_repo();
        let cfg: PublishConfig = toml::from_str(&format!(
            "[schema]\nname = \"x\"\nversion = \"0.2.0\"\nlinkml = \"1.7.0\"\n\
             [files]\nmain = \"schema.yaml\"\n\
             [publishing]\nversions = [\"v0.1.1\"]\nedge = \"main\"\ncurrent = \"v0.1.1\"\noutput_dir = \"site\"\n\
             [editions]\noutput_dir = \"site\"\n{EDITIONS}"
        ))
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
        assert!(
            toolchain.builds.borrow().is_empty(),
            "nothing may build first"
        );
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
                .join("book/2/src")
                .join(std::ffi::OsStr::from_bytes(b"caf\xe9.png")),
            "",
        )
        .unwrap();
        commit(repo.path(), &[], None);

        let err =
            build_editions(repo.path(), &spec("main", ""), &FakeToolchain::default()).unwrap_err();

        match err {
            EditionsError::RootEntryNotUtf8 { edition, entry } => {
                assert_eq!(edition, "2");
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

        assert_eq!(body(repo.path().join("site/book/2/chapter.html")), "second");
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

    fn canonical(path: impl AsRef<Path>) -> String {
        let text = read(path);
        let start = text
            .find(r#"<link rel="canonical" href=""#)
            .expect("a page carries a canonical link")
            + r#"<link rel="canonical" href=""#.len();
        let end = text[start..].find('"').unwrap() + start;
        text[start..end].to_string()
    }

    #[test]
    fn every_page_carries_a_canonical_to_its_stable_path() {
        let repo = book_repo();
        build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default()).unwrap();

        let book = repo.path().join("site/book");
        assert_eq!(
            canonical(book.join("1/chapter.html")),
            "../../book/1/chapter.html"
        );
        assert_eq!(
            canonical(book.join("2/part/page.html")),
            "../../../book/2/part/page.html"
        );
    }

    /// The root copy is two directories shallower, so its canonicals are
    /// the one place its bytes differ from the stable copy's.
    #[test]
    fn the_root_copys_canonicals_point_at_the_stable_path() {
        let repo = book_repo();
        build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default()).unwrap();

        let site = repo.path().join("site");
        assert_eq!(canonical(site.join("chapter.html")), "book/2/chapter.html");
        assert_eq!(
            canonical(site.join("part/page.html")),
            "../book/2/part/page.html"
        );
    }

    #[test]
    fn a_page_without_a_head_is_copied_as_it_is() {
        let repo = book_repo();
        commit(
            repo.path(),
            &[("book/2/src/raw.html", "<p>no head here</p>")],
            None,
        );

        build_editions(repo.path(), &spec("main", ""), &FakeToolchain::default()).unwrap();

        assert_eq!(
            read(repo.path().join("site/book/2/raw.html")),
            "<p>no head here</p>"
        );
        assert_eq!(
            read(repo.path().join("site/raw.html")),
            "<p>no head here</p>"
        );
    }

    /// The reader of an edition reaches the schema docs for the newest
    /// release it covers, not whatever `current/` aliases today.
    #[test]
    fn the_schema_link_points_at_the_newest_release_the_edition_covers() {
        let repo = book_repo();
        let links = "[[book_link]]\nschema_path = \"schema/current/\"\nlabel = \"Schema\"\n\
                     [[book_link]]\nschema_path = \"schema/cqa/current/\"\nlabel = \"CQ&A\"\n\
                     [[book_link]]\nschema_path = \"docs/model/\"\nlabel = \"Model\"\n\
                     [[book_link]]\nschema_path = \"docs/concurrent/\"\nlabel = \"Not an alias\"\n";
        build_editions(
            repo.path(),
            &spec("v0.2.0", links),
            &FakeToolchain::default(),
        )
        .unwrap();

        let book = repo.path().join("site/book");
        let first = read(book.join("1/schema-link.js"));
        assert!(
            first.contains(r#""schemaPath":"schema/v0.1.1/""#)
                && first.contains(r#""schemaPath":"schema/cqa/v0.1.1/""#)
                && first.contains(r#""schemaPath":"docs/model/""#)
                && first.contains(r#""schemaPath":"docs/concurrent/""#),
            "the first edition links its newest release, and keeps a path that is not an alias; got:\n{first}"
        );
        assert!(
            !first.contains(r#""schemaPath":"schema/current/""#)
                && !first.contains(r#""schemaPath":"schema/cqa/current/""#),
            "no alias may survive into an edition's links; got:\n{first}"
        );
        let second = read(book.join("2/schema-link.js"));
        assert!(
            second.contains(r#""schemaPath":"schema/v0.2.0/""#),
            "got:\n{second}"
        );
    }

    #[test]
    fn an_unreleased_edition_links_the_edge_build() {
        let repo = book_repo();
        commit(
            repo.path(),
            &[
                ("book/3/book.toml", "[book]\n"),
                ("book/3/src/chapter.md", "third"),
            ],
            None,
        );
        let editions = EDITIONS.to_string()
            + &edition("3", "book/3", "v0.3.0")
            + "[[book_link]]\nschema_path = \"schema/current/\"\nlabel = \"Schema\"\n";
        let cfg = spec_with(
            "versions = [\"v0.1.0\", \"v0.2.0\"]\nedge = \"main\"\ncurrent = \"v0.2.0\"",
            &editions,
        );

        build_editions(repo.path(), &cfg, &FakeToolchain::default()).unwrap();

        let third = read(repo.path().join("site/book/3/schema-link.js"));
        assert!(
            third.contains(r#""schemaPath":"schema/main/""#),
            "got:\n{third}"
        );
        assert_eq!(
            baked(repo.path().join("site/book/3/schema-link.js"))["editions"][0],
            serde_json::json!({"label": "3", "unreleased": true}),
            "the switcher marks it unreleased"
        );
    }

    /// The editions context the build baked into a toolbar asset.
    fn baked(asset: impl AsRef<Path>) -> serde_json::Value {
        let js = read(asset);
        let start = js
            .find("var editions = ")
            .expect("the asset declares its editions")
            + "var editions = ".len();
        // `lines` reads the asset whether its checkout ends lines in LF
        // or CRLF.
        let declaration = js[start..].lines().next().expect("the declaration ends");
        serde_json::from_str(declaration.trim_end_matches(';')).expect("the baked context is JSON")
    }

    #[test]
    fn the_baked_editions_name_the_pages_own_edition_and_the_current_one() {
        let repo = book_repo();
        build_editions(repo.path(), &spec("v0.2.0", ""), &FakeToolchain::default()).unwrap();

        assert_eq!(
            baked(repo.path().join("site/book/1/schema-link.js")),
            serde_json::json!({
                "editions": [
                    {"label": "2", "unreleased": false},
                    {"label": "1", "unreleased": false},
                ],
                "current": "2",
                "own": "1",
            })
        );
    }

    /// A chapter's file name and an edition's id both land in a URL, so
    /// each segment is percent-encoded, which also keeps `"` and `&` out
    /// of the attribute. `#` starts a URL fragment, which is exactly the
    /// case.
    #[test]
    fn a_canonical_percent_encodes_the_page_and_the_edition() {
        let repo = book_repo();
        commit(repo.path(), &[("book/1/src/faq & tips.md", "spaced")], None);
        let cfg = spec_with(
            "versions = [\"v0.1.0\"]\nedge = \"main\"\ncurrent = \"v0.1.0\"",
            &edition("rc#2", "book/1", "v0.1.0"),
        );

        build_editions(repo.path(), &cfg, &FakeToolchain::default()).unwrap();

        assert_eq!(
            canonical(repo.path().join("site/book/rc#2/faq & tips.html")),
            "../../book/rc%232/faq%20%26%20tips.html"
        );
    }

    /// GitHub Pages serves the root copy's `404.html` at whatever depth the
    /// missing URL had, so a page-relative canonical would point somewhere
    /// different on every miss.
    #[test]
    fn the_404_page_gets_no_canonical() {
        let repo = book_repo();
        commit(repo.path(), &[("book/2/src/404.md", "missing")], None);

        build_editions(repo.path(), &spec("main", ""), &FakeToolchain::default()).unwrap();

        for page in ["site/book/2/404.html", "site/404.html"] {
            let text = read(repo.path().join(page));
            assert!(
                !text.contains("canonical"),
                "{page} must carry no canonical; got:\n{text}"
            );
        }
        assert_eq!(
            canonical(repo.path().join("site/chapter.html")),
            "book/2/chapter.html",
            "the other pages still get theirs"
        );
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
}
