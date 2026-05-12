use std::path::{Path, PathBuf};

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

#[cfg(feature = "dev")]
mod components;
mod server;

use panschema::io::FormatRegistry;

/// `panschema release --level <X>` choices.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ReleaseLevel {
    Patch,
    Minor,
    Major,
}

impl From<ReleaseLevel> for panschema::publish::BumpLevel {
    fn from(r: ReleaseLevel) -> Self {
        match r {
            ReleaseLevel::Patch => panschema::publish::BumpLevel::Patch,
            ReleaseLevel::Minor => panschema::publish::BumpLevel::Minor,
            ReleaseLevel::Major => panschema::publish::BumpLevel::Major,
        }
    }
}

/// Visualization mode for HTML output
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum VizMode {
    /// Auto-detect: try WebGPU, fall back to 2D Canvas
    #[default]
    Auto,
    /// Force 2D Canvas rendering (CPU simulation)
    #[value(name = "2d")]
    Canvas2D,
    /// Force 3D WebGPU rendering (GPU simulation)
    #[value(name = "3d")]
    WebGPU3D,
}

/// A universal CLI for schema conversion, documentation, validation, and comparison.
#[derive(Parser)]
#[command(name = "panschema")]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Input ontology file (.ttl) - used when no subcommand specified
    #[arg(short, long, global = true)]
    input: Option<PathBuf>,

    /// Output path (file for RDF formats, directory for HTML)
    #[arg(short, long, global = true, default_value = "output")]
    output: PathBuf,

    /// Output format: html, ttl, jsonld, rdfxml, ntriples
    #[arg(short, long, global = true, default_value = "html")]
    format: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate documentation or convert to other formats. With no `--input`,
    /// discovers a `panschema.toml` (cargo-style walk up from CWD) and runs
    /// codegen for each manifested schema.
    Generate {
        /// Input ontology file (.ttl, .yaml, .yml). When omitted, uses the manifest.
        #[arg(short, long)]
        input: Option<PathBuf>,

        /// Output path (file for RDF formats, directory for HTML)
        #[arg(short, long, default_value = "output")]
        output: PathBuf,

        /// Output format: html, ttl, jsonld, rdfxml, ntriples, graph-json
        #[arg(short, long, default_value = "html")]
        format: String,

        /// Disable interactive graph visualization (HTML output only)
        #[arg(long = "no-graph")]
        no_graph: bool,

        /// Visualization mode: auto, 2d, 3d (requires --graph)
        #[arg(long, value_enum, default_value = "auto")]
        viz_mode: VizMode,
    },
    /// Scaffold `panschema-publish.toml` in the current directory.
    ///
    /// Producer-side counterpart to `panschema add`. Three input modes:
    ///   panschema init --name X --version 0.1.0 --main schema.yaml
    ///   panschema init --from path/to/schema.yaml   # pre-fill from a LinkML file
    ///   panschema init                              # bare defaults
    Init {
        /// Schema package name (defaults to the CWD's basename or, with
        /// `--from`, the name declared in the LinkML file).
        #[arg(long)]
        name: Option<String>,

        /// Initial version (defaults to `0.1.0` or, with `--from`, the
        /// version declared in the LinkML file).
        #[arg(long)]
        version: Option<String>,

        /// Path to the main schema file, relative to the publish file
        /// (defaults to `schema.yaml` or, with `--from`, the path passed).
        #[arg(long)]
        main: Option<PathBuf>,

        /// Target LinkML metamodel version (defaults to `1.7.0`).
        #[arg(long, default_value = "1.7.0")]
        linkml: String,

        /// Pre-fill name/version (and `--main` default) from an existing
        /// LinkML schema file at the given path.
        #[arg(long)]
        from: Option<PathBuf>,

        /// Overwrite an existing `panschema-publish.toml`.
        #[arg(long)]
        force: bool,
    },
    /// Add a schema dependency to `panschema.toml` and fetch it.
    ///
    /// Examples:
    ///   panschema add github:padamson/scimantic-schema@0.1.3
    ///   panschema add ./local-pkg
    ///   panschema add ./local-pkg --name custom-alias
    ///
    /// The schema name is read from `panschema-publish.toml` at the
    /// resolved location. Pass `--name` to install under a different
    /// local key.
    Add {
        /// Source spec: `<protocol>:<args>@<version>` (e.g.
        /// `github:owner/repo@0.1.3`) or a filesystem path to a package
        /// directory containing `panschema-publish.toml`.
        spec: panschema::manifest::SchemaSpec,

        /// Install under a local alias instead of the name declared in
        /// `panschema-publish.toml`. Useful when two schemas would
        /// otherwise collide on name.
        #[arg(long)]
        name: Option<String>,

        /// Skip writing a starter `[generate.<name>]` block. By default,
        /// an empty block is added so you can fill in writers (e.g.
        /// `html = "docs/"`).
        #[arg(long = "no-generate-config")]
        no_generate_config: bool,
    },
    /// Cut a new release of the schema package in CWD.
    ///
    /// Producer-side counterpart to `cargo release`. By default, just bumps
    /// `[schema].version` in `panschema-publish.toml` and prints the
    /// suggested git commands. `--git` runs commit + tag; `--push` also
    /// pushes.
    Release {
        /// Semver bump level (mutually exclusive with `--version`).
        #[arg(long, value_enum)]
        level: Option<ReleaseLevel>,

        /// Set the version to an exact value (mutually exclusive with `--level`).
        #[arg(long, conflicts_with = "level")]
        version: Option<String>,

        /// After bumping, stage publish.toml, commit `release: v<ver>`, and
        /// tag `v<ver>`. Refuses on a dirty working tree or an existing tag.
        #[arg(long)]
        git: bool,

        /// After committing + tagging, also `git push --follow-tags`.
        /// Requires `--git`.
        #[arg(long, requires = "git")]
        push: bool,

        /// Print the plan without writing files or running any git commands.
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Resolve every schema in the manifest, compute checksums, and write
    /// `panschema.lock`. Run this when you add or update a schema dependency.
    Fetch,
    /// Verify that on-disk schemas match the checksums recorded in
    /// `panschema.lock`. Errors on drift.
    Verify,
    /// Start development server with hot reload
    Serve {
        /// Input ontology file (.ttl, .yaml, .yml)
        #[arg(short, long)]
        input: PathBuf,

        /// Output directory for generated documentation
        #[arg(short, long, default_value = "output")]
        output: PathBuf,

        /// Port to run the server on
        #[arg(short, long, default_value = "3000")]
        port: u16,
    },
    /// Generate shell completion script (source the output, e.g. `panschema completions zsh > ~/.zfunc/_panschema`)
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Generate style guide showing all UI components (dev feature only)
    #[cfg(feature = "dev")]
    Styleguide {
        /// Output directory for style guide
        #[arg(short, long, default_value = "output")]
        output: PathBuf,

        /// Start dev server to preview style guide
        #[arg(long)]
        serve: bool,

        /// Port for dev server (requires --serve)
        #[arg(short, long, default_value = "3000")]
        port: u16,
    },
}

fn generate(input: &Path, output: &Path, format: &str, include_graph: bool) -> anyhow::Result<()> {
    let registry = FormatRegistry::with_defaults();

    let reader = registry
        .reader_for_path(input)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let schema = reader.read(input).map_err(|e| anyhow::anyhow!("{}", e))?;

    // For HTML format, use HtmlWriter with custom options
    if format.eq_ignore_ascii_case("html") {
        use panschema::html_writer::HtmlWriter;
        use panschema::io::Writer;
        let writer = HtmlWriter::with_options(include_graph);
        writer
            .write(&schema, output)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
    } else {
        let writer = registry
            .writer_for_format(format)
            .ok_or_else(|| anyhow::anyhow!("Unsupported output format: {}", format))?;
        writer
            .write(&schema, output)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
    }

    let title = schema.title.as_deref().unwrap_or(&schema.name);
    let format_desc = match format.to_lowercase().as_str() {
        "html" => "documentation",
        "ttl" => "Turtle",
        "jsonld" => "JSON-LD",
        "rdfxml" => "RDF/XML",
        "ntriples" => "N-Triples",
        _ => format,
    };
    println!(
        "Generated {} for '{}' at {}",
        format_desc,
        title,
        output.display()
    );
    Ok(())
}

/// Discover the manifest and load it. Returns the parsed manifest plus the
/// directory it lives in (paths in the manifest are resolved relative to this).
fn load_manifest() -> anyhow::Result<(panschema::manifest::Manifest, PathBuf)> {
    use panschema::manifest::{Manifest, discover_manifest};

    let cwd = std::env::current_dir()?;
    let manifest_path = discover_manifest(&cwd).ok_or_else(|| {
        anyhow::anyhow!(
            "no `panschema.toml` found in `{}` or any ancestor directory. \
             Create a manifest, or pass `--input <file>` for one-off generate. \
             See docs/features/05-schema-manager.md.",
            cwd.display()
        )
    })?;
    let manifest_dir = manifest_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("manifest path has no parent directory"))?
        .to_path_buf();
    eprintln!("Using manifest: {}", manifest_path.display());
    let manifest = Manifest::from_path(&manifest_path)?;
    Ok((manifest, manifest_dir))
}

/// Dispatch on a schema's source kind and resolve it.
///
/// `path:` sources resolve relative to the manifest directory; `github:`
/// sources resolve through the local cache (populating it from
/// `codeload.github.com` if absent).
fn resolve_source(
    name: &str,
    dep: &panschema::manifest::SchemaDep,
    manifest_dir: &Path,
) -> anyhow::Result<panschema::source::Resolved> {
    use panschema::cache::cache_root;
    use panschema::source::{CodeloadGithubSource, SchemaSource, resolve_github, resolve_path};

    let source = SchemaSource::from_dep(name, dep)?;
    match source {
        SchemaSource::Path { path } => {
            Ok(resolve_path(name, &path, manifest_dir).map_err(|e| anyhow::anyhow!("{e}"))?)
        }
        SchemaSource::Github {
            owner,
            repo,
            version,
        } => {
            let cache = cache_root().map_err(|e| anyhow::anyhow!("{e}"))?;
            let github = CodeloadGithubSource;
            Ok(
                resolve_github(name, &owner, &repo, &version, &cache, &github)
                    .map_err(|e| anyhow::anyhow!("{e}"))?,
            )
        }
    }
}

/// `panschema generate` (no --input): walk the manifest and run configured writers.
fn generate_from_manifest() -> anyhow::Result<()> {
    let (manifest, manifest_dir) = load_manifest()?;

    if manifest.schemas.is_empty() {
        eprintln!("Manifest has no `[schemas]` entries; nothing to do.");
        return Ok(());
    }

    let mut produced_anything = false;
    for (name, dep) in &manifest.schemas {
        let panschema::source::Resolved { schema_path, .. } =
            resolve_source(name, dep, &manifest_dir)?;
        let Some(gen_cfg) = manifest.generate.get(name) else {
            eprintln!("schema `{name}`: no [generate.{name}] block; skipping");
            continue;
        };
        if let Some(html_out) = &gen_cfg.html {
            let html_out = manifest_dir.join(html_out);
            generate(&schema_path, &html_out, "html", true)?;
            produced_anything = true;
        }
    }

    if !produced_anything {
        eprintln!(
            "No outputs generated. Add an `[generate.<schema>]` block with at least one writer key (e.g. `html = \"docs/\"`)."
        );
    }
    Ok(())
}

/// `panschema init`: scaffold `panschema-publish.toml` in CWD.
///
/// Argument-resolution precedence (highest first):
///   1. Explicit `--name` / `--version` / `--main` flags
///   2. Values extracted from `--from <linkml-file>`
///   3. Defaults: name = CWD basename, version = "0.1.0", main = "schema.yaml"
///
/// After writing, parses the file back and tries to read the main file
/// via the format registry. Validation failures print a warning but
/// don't undo the write — the user may be mid-edit.
fn init_schema_package(
    name: Option<&str>,
    version: Option<&str>,
    main: Option<&Path>,
    linkml: &str,
    from: Option<&Path>,
    force: bool,
) -> anyhow::Result<()> {
    use panschema::io::FormatRegistry;
    use panschema::publish::init_publish_file;

    let cwd = std::env::current_dir()?;

    // Extract defaults from `--from` if provided.
    let (from_name, from_version) = match from {
        Some(path) => {
            let registry = FormatRegistry::with_defaults();
            let reader = registry
                .reader_for_path(path)
                .map_err(|e| anyhow::anyhow!("--from `{}`: {e}", path.display()))?;
            let schema = reader
                .read(path)
                .map_err(|e| anyhow::anyhow!("--from `{}`: {e}", path.display()))?;
            (Some(schema.name), schema.version)
        }
        None => (None, None),
    };

    let resolved_name: String = name.map(str::to_string).or(from_name).unwrap_or_else(|| {
        cwd.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("schema")
            .to_string()
    });
    let resolved_version: String = version
        .map(str::to_string)
        .or(from_version)
        .unwrap_or_else(|| "0.1.0".to_string());
    let resolved_main: PathBuf = main
        .map(Path::to_path_buf)
        .or_else(|| from.map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("schema.yaml"));

    let path = init_publish_file(
        &cwd,
        &resolved_name,
        &resolved_version,
        &resolved_main,
        linkml,
        force,
    )?;
    println!(
        "Wrote {} (name = \"{}\", version = \"{}\", main = \"{}\")",
        path.display(),
        resolved_name,
        resolved_version,
        resolved_main.display(),
    );

    // Post-write validation — informational only.
    let main_full = cwd.join(&resolved_main);
    if !main_full.exists() {
        eprintln!(
            "warning: `{}` does not exist yet. Create it before running \
             `panschema fetch` or `panschema add`.",
            main_full.display()
        );
    } else {
        let registry = FormatRegistry::with_defaults();
        if let Ok(reader) = registry.reader_for_path(&main_full)
            && let Err(e) = reader.read(&main_full)
        {
            eprintln!(
                "warning: `{}` exists but failed to parse as a schema: {e}",
                main_full.display()
            );
        }
    }

    Ok(())
}

/// `panschema release`: bump the version in `panschema-publish.toml`,
/// optionally commit + tag (with `--git`), optionally push (with `--push`).
fn release_schema(
    level: Option<ReleaseLevel>,
    version: Option<&str>,
    git: bool,
    push: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    use panschema::publish::{
        BumpLevel, PUBLISH_FILENAME, PublishConfig, bump_version, set_version,
    };

    let cwd = std::env::current_dir()?;
    let publish_path = cwd.join(PUBLISH_FILENAME);
    if !publish_path.exists() {
        anyhow::bail!(
            "no `{PUBLISH_FILENAME}` in `{}`. Run `panschema init` first.",
            cwd.display()
        );
    }

    // Require exactly one of --level / --version.
    let action: ReleaseAction = match (level, version) {
        (Some(l), None) => ReleaseAction::Bump(BumpLevel::from(l)),
        (None, Some(v)) => ReleaseAction::Set(v.to_string()),
        (None, None) => {
            anyhow::bail!("must pass either `--level <patch|minor|major>` or `--version <x.y.z>`");
        }
        (Some(_), Some(_)) => unreachable!("clap conflicts_with prevents this"),
    };

    // Compute what the new version *would* be (read-only) so dry-run + git
    // safety checks can report it without writing.
    let current_cfg = PublishConfig::from_path(&publish_path)?;
    let projected_new = match &action {
        ReleaseAction::Bump(level) => {
            let mut v = semver::Version::parse(&current_cfg.schema.version).map_err(|_| {
                anyhow::anyhow!(
                    "`[schema].version` `{}` is not valid semver",
                    current_cfg.schema.version
                )
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
            v.pre = semver::Prerelease::EMPTY;
            v.build = semver::BuildMetadata::EMPTY;
            v.to_string()
        }
        ReleaseAction::Set(s) => {
            semver::Version::parse(s)
                .map_err(|_| anyhow::anyhow!("`{s}` is not a valid semver version"))?;
            s.clone()
        }
    };

    let old_version = current_cfg.schema.version.clone();
    let new_tag = format!("v{projected_new}");

    if git {
        ensure_git_available()?;
        ensure_working_tree_clean(&cwd)?;
        ensure_tag_does_not_exist(&cwd, &new_tag)?;
    }

    if dry_run {
        println!("Dry run: would bump {old_version} → {projected_new}");
        if git {
            println!("Would run:");
            println!("    git add {PUBLISH_FILENAME}");
            println!("    git commit -m 'release: {new_tag}'");
            println!("    git tag {new_tag}");
            if push {
                println!("    git push --follow-tags");
            }
        } else {
            println!("(bump-only — no git operations would run)");
        }
        return Ok(());
    }

    // Apply the bump.
    let (_old, new) = match action {
        ReleaseAction::Bump(level) => bump_version(&publish_path, level)?,
        ReleaseAction::Set(s) => (set_version(&publish_path, &s)?, s),
    };
    debug_assert_eq!(new, projected_new);
    println!("Bumped {PUBLISH_FILENAME}: {old_version} → {new}");

    if git {
        run_git(&cwd, &["add", PUBLISH_FILENAME])?;
        let msg = format!("release: {new_tag}");
        run_git(&cwd, &["commit", "-m", &msg])?;
        run_git(&cwd, &["tag", &new_tag])?;
        println!("Committed and tagged {new_tag}.");
        if push {
            run_git(&cwd, &["push", "--follow-tags"])?;
            println!("Pushed.");
        } else {
            println!("To publish the release:");
            println!("    git push --follow-tags");
        }
    } else {
        println!("Suggested next steps:");
        println!("    git commit -am 'release: {new_tag}'");
        println!("    git tag {new_tag}");
        println!("    git push --follow-tags");
    }

    Ok(())
}

enum ReleaseAction {
    Bump(panschema::publish::BumpLevel),
    Set(String),
}

fn ensure_git_available() -> anyhow::Result<()> {
    let out = std::process::Command::new("git")
        .arg("--version")
        .output()
        .map_err(|e| anyhow::anyhow!("`git` is not on PATH (required for --git): {e}"))?;
    if !out.status.success() {
        anyhow::bail!("`git --version` failed; is git installed?");
    }
    Ok(())
}

fn ensure_working_tree_clean(cwd: &Path) -> anyhow::Result<()> {
    // First check we're in a git repo.
    let inside = std::process::Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(cwd)
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run git: {e}"))?;
    if !inside.status.success() {
        anyhow::bail!(
            "not inside a git repository (`{}` not under git control). \
             Re-run without --git, or `git init` first.",
            cwd.display()
        );
    }
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(cwd)
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run git status: {e}"))?;
    if !status.stdout.is_empty() {
        anyhow::bail!(
            "git working tree is not clean. Commit or stash changes before `release --git`."
        );
    }
    Ok(())
}

fn ensure_tag_does_not_exist(cwd: &Path, tag: &str) -> anyhow::Result<()> {
    let out = std::process::Command::new("git")
        .args(["tag", "--list", tag])
        .current_dir(cwd)
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run git tag --list: {e}"))?;
    if !out.stdout.is_empty() {
        anyhow::bail!("tag `{tag}` already exists. Pick a different version.");
    }
    Ok(())
}

fn run_git(cwd: &Path, args: &[&str]) -> anyhow::Result<()> {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run git {args:?}: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("git {args:?} failed: {stderr}");
    }
    Ok(())
}

/// `panschema add <spec>`: read the package's `panschema-publish.toml`
/// to learn the schema's name + version, insert an entry into
/// `panschema.toml`, then run `fetch` to populate the cache and update
/// the lockfile.
///
/// `name_override` (`--name <alias>`) installs the schema under a
/// different local key than what `panschema-publish.toml` declares.
fn add_schema(
    spec: panschema::manifest::SchemaSpec,
    name_override: Option<&str>,
    with_generate_block: bool,
) -> anyhow::Result<()> {
    use panschema::manifest::{
        AddOutcome, AddRequest, MANIFEST_FILENAME, SchemaSpec, discover_manifest, insert_schema,
    };

    // Find the manifest first — we'll need its directory for path
    // relativization, and we want to fail fast if there isn't one.
    let cwd = std::env::current_dir()?;
    let manifest_path = discover_manifest(&cwd).ok_or_else(|| {
        anyhow::anyhow!(
            "no `{MANIFEST_FILENAME}` found in `{}` or any ancestor. \
             Create one (a minimal `[schemas]` table is enough) and re-run.",
            cwd.display()
        )
    })?;
    let manifest_dir = manifest_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("manifest path has no parent directory"))?;

    let request = match spec {
        SchemaSpec::Path(pkg) => {
            // Resolve the user's input from CWD (where they typed it),
            // not from the manifest dir.
            let resolved = if pkg.is_absolute() {
                pkg.clone()
            } else {
                cwd.join(&pkg)
            };
            let (canon_pkg, publish) = panschema::source::open_package("(new)", &resolved)
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            let name = name_override
                .map(str::to_string)
                .unwrap_or_else(|| publish.schema.name.clone());

            // Re-relativize the canonical package dir to the manifest's
            // directory so the stored `path` is stable regardless of
            // where the user typed it from.
            let canon_manifest_dir = manifest_dir.canonicalize().map_err(|e| {
                anyhow::anyhow!(
                    "canonicalize manifest dir `{}`: {e}",
                    manifest_dir.display()
                )
            })?;
            let stored = relative_path(&canon_manifest_dir, &canon_pkg);

            AddRequest::Path { name, path: stored }
        }
        SchemaSpec::Source { uri, version } => {
            // Resolve through the cache so we can read publish.toml and
            // discover the canonical name. resolve_github also verifies
            // the version matches.
            let source = parse_github_uri(&uri)?;
            let cache = panschema::cache::cache_root().map_err(|e| anyhow::anyhow!("{e}"))?;
            let github = panschema::source::CodeloadGithubSource;
            let resolved = panschema::source::resolve_github(
                name_override.unwrap_or("(new)"),
                &source.owner,
                &source.repo,
                &version,
                &cache,
                &github,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;
            // Reach into the cached publish file to grab the canonical name.
            // resolve_github already validated version + symlink hygiene.
            let pkg_dir = resolved
                .schema_path
                .parent()
                .expect("schema path has a parent (the package dir)");
            let publish = panschema::publish::PublishConfig::from_path(
                &pkg_dir.join("panschema-publish.toml"),
            )?;
            let name = name_override
                .map(str::to_string)
                .unwrap_or_else(|| publish.schema.name.clone());
            AddRequest::Remote {
                name,
                source: uri,
                version,
            }
        }
    };

    let outcome = insert_schema(&manifest_path, &request, with_generate_block)?;
    match outcome {
        AddOutcome::Inserted => {
            println!("Added `{}` to {}", request.name(), manifest_path.display());
        }
        AddOutcome::AlreadyPresent => {
            println!(
                "`{}` is already present in {} with the same source; nothing changed.",
                request.name(),
                manifest_path.display()
            );
        }
    }

    // Always re-run fetch so the new schema lands in the cache + lockfile,
    // and so an idempotent `add` still gives a freshly-verified state.
    fetch_from_manifest()?;
    Ok(())
}

struct GithubOwnerRepo {
    owner: String,
    repo: String,
}

/// Lightweight parser for `github:owner/repo` strings shared by `add`
/// (which uses `SchemaSpec::Source { uri, .. }`) and `SchemaSource`
/// (which has a richer parser of its own). Keeps `add` from depending
/// on `SchemaSource::from_dep`'s `SchemaDep` plumbing.
fn parse_github_uri(uri: &str) -> anyhow::Result<GithubOwnerRepo> {
    let rest = uri
        .strip_prefix("github:")
        .ok_or_else(|| anyhow::anyhow!("expected `github:owner/repo`, got `{uri}`"))?;
    let (owner, repo) = rest.split_once('/').ok_or_else(|| {
        anyhow::anyhow!("malformed github URI `{uri}`: expected `github:owner/repo`")
    })?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        anyhow::bail!("malformed github URI `{uri}`: expected `github:owner/repo`");
    }
    Ok(GithubOwnerRepo {
        owner: owner.to_string(),
        repo: repo.to_string(),
    })
}

/// Compute `target` relative to `base`, returning a relative `PathBuf`.
/// Both arguments must be canonical absolute paths. Falls back to the
/// absolute target if there's no shared ancestor (cross-volume etc.).
fn relative_path(base: &Path, target: &Path) -> PathBuf {
    use std::path::Component;

    let base_components: Vec<_> = base.components().collect();
    let target_components: Vec<_> = target.components().collect();

    let common_prefix = base_components
        .iter()
        .zip(target_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

    // No shared ancestor? Just return the absolute target.
    if common_prefix == 0
        || !matches!(
            base_components.first(),
            Some(Component::RootDir) | Some(Component::Prefix(_))
        )
    {
        return target.to_path_buf();
    }

    let mut rel = PathBuf::new();
    for _ in base_components.iter().skip(common_prefix) {
        rel.push("..");
    }
    for c in target_components.iter().skip(common_prefix) {
        rel.push(c);
    }
    if rel.as_os_str().is_empty() {
        rel.push(".");
    }
    rel
}

/// `panschema fetch`: resolve every manifested schema, compute its checksum,
/// and write `panschema.lock`.
fn fetch_from_manifest() -> anyhow::Result<()> {
    use panschema::lockfile::{LOCKFILE_FILENAME, LockEntry, Lockfile, checksum_file};
    use panschema::source::SchemaSource;

    let (manifest, manifest_dir) = load_manifest()?;

    let mut entries = Vec::with_capacity(manifest.schemas.len());
    for (name, dep) in &manifest.schemas {
        let panschema::source::Resolved {
            schema_path,
            version,
            revision,
        } = resolve_source(name, dep, &manifest_dir)?;
        let source = SchemaSource::from_dep(name, dep)?;
        entries.push(LockEntry {
            name: name.clone(),
            // Always populated now — both source types read publish.toml.
            version: Some(version),
            source: source.source_spec(),
            revision,
            checksum: checksum_file(&schema_path)?,
        });
    }

    let lockfile = Lockfile { entries };
    let lock_path = manifest_dir.join(LOCKFILE_FILENAME);
    lockfile.write_to_path(&lock_path)?;
    println!(
        "Fetched {} schema(s); wrote {}",
        lockfile.entries.len(),
        lock_path.display()
    );
    Ok(())
}

/// `panschema verify`: re-checksum every manifested schema and compare with
/// the lockfile. Errors with a clear diff on mismatch.
fn verify_from_manifest() -> anyhow::Result<()> {
    use panschema::lockfile::{LOCKFILE_FILENAME, Lockfile, checksum_file};

    let (manifest, manifest_dir) = load_manifest()?;
    let lock_path = manifest_dir.join(LOCKFILE_FILENAME);
    if !lock_path.exists() {
        anyhow::bail!(
            "no `{}` next to the manifest. Run `panschema fetch` first.",
            LOCKFILE_FILENAME
        );
    }
    let lockfile = Lockfile::from_path(&lock_path)?;

    let mut drift = Vec::new();
    let mut missing_in_lock = Vec::new();
    for (name, dep) in &manifest.schemas {
        let panschema::source::Resolved { schema_path, .. } =
            resolve_source(name, dep, &manifest_dir)?;
        let observed = checksum_file(&schema_path)?;
        match lockfile.entry(name) {
            Some(entry) if entry.checksum == observed => {}
            Some(entry) => drift.push((name.clone(), entry.checksum.clone(), observed)),
            None => missing_in_lock.push(name.clone()),
        }
    }
    let lockfile_only: Vec<_> = lockfile
        .entries
        .iter()
        .filter(|e| !manifest.schemas.contains_key(&e.name))
        .map(|e| e.name.clone())
        .collect();

    if drift.is_empty() && missing_in_lock.is_empty() && lockfile_only.is_empty() {
        println!("Verified {} schema(s).", manifest.schemas.len());
        return Ok(());
    }

    let mut msg = String::from("schema dependencies drifted from the lockfile:\n");
    for (name, locked, observed) in &drift {
        msg.push_str(&format!(
            "  - `{name}`: lockfile has {locked}, on-disk is {observed}\n"
        ));
    }
    for name in &missing_in_lock {
        msg.push_str(&format!(
            "  - `{name}`: in manifest but not in lockfile (run `panschema fetch`)\n"
        ));
    }
    for name in &lockfile_only {
        msg.push_str(&format!(
            "  - `{name}`: in lockfile but not in manifest (stale; run `panschema fetch` to refresh)\n"
        ));
    }
    anyhow::bail!("{msg}");
}

#[cfg(feature = "dev")]
fn generate_styleguide(output: &Path) -> anyhow::Result<()> {
    use std::fs;

    let data = components::SampleData::default();
    let html = components::ComponentRenderer::styleguide(&data)?;

    fs::create_dir_all(output)?;
    let output_path = output.join("styleguide.html");
    fs::write(&output_path, html)?;

    println!("Generated style guide at {}", output_path.display());
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Generate {
            input,
            output,
            format,
            no_graph,
            viz_mode,
        }) => match input {
            Some(input) => {
                if format.to_lowercase() == "html" && !no_graph {
                    let mode_str = match viz_mode {
                        VizMode::Auto => "auto (2D fallback, 3D when available)",
                        VizMode::Canvas2D => "2D Canvas (CPU)",
                        VizMode::WebGPU3D => "3D WebGPU (GPU)",
                    };
                    eprintln!("Graph visualization: {}", mode_str);
                }
                generate(&input, &output, &format, !no_graph)?;
            }
            None => generate_from_manifest()?,
        },
        Some(Commands::Init {
            name,
            version,
            main,
            linkml,
            from,
            force,
        }) => init_schema_package(
            name.as_deref(),
            version.as_deref(),
            main.as_deref(),
            &linkml,
            from.as_deref(),
            force,
        )?,
        Some(Commands::Add {
            spec,
            name,
            no_generate_config,
        }) => add_schema(spec, name.as_deref(), !no_generate_config)?,
        Some(Commands::Release {
            level,
            version,
            git,
            push,
            dry_run,
        }) => release_schema(level, version.as_deref(), git, push, dry_run)?,
        Some(Commands::Fetch) => fetch_from_manifest()?,
        Some(Commands::Verify) => verify_from_manifest()?,
        Some(Commands::Serve {
            input,
            output,
            port,
        }) => {
            server::serve(&input, &output, port).await?;
        }
        Some(Commands::Completions { shell }) => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "panschema", &mut std::io::stdout());
        }
        #[cfg(feature = "dev")]
        Some(Commands::Styleguide {
            output,
            serve,
            port,
        }) => {
            generate_styleguide(&output)?;
            if serve {
                println!("Starting style guide server on port {port}...");
                server::serve_static(&output, port).await?;
            }
        }
        None => {
            // Default behavior: generate if input provided (with graph enabled by default)
            if let Some(input) = cli.input {
                generate(&input, &cli.output, &cli.format, true)?;
            } else {
                println!("panschema: no input specified. Use --help for usage.");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_with_defaults() {
        let cli = Cli::try_parse_from(["panschema"]).unwrap();
        assert_eq!(cli.output, PathBuf::from("output"));
        assert_eq!(cli.format, "html");
        assert!(cli.input.is_none());
        assert!(cli.command.is_none());
    }

    #[test]
    fn cli_parses_generate_subcommand() {
        let cli = Cli::try_parse_from([
            "panschema",
            "generate",
            "--input",
            "test.ttl",
            "--output",
            "docs",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Generate {
                input,
                output,
                format,
                no_graph,
                viz_mode,
            }) => {
                assert_eq!(input, Some(PathBuf::from("test.ttl")));
                assert_eq!(output, PathBuf::from("docs"));
                assert_eq!(format, "html");
                assert!(!no_graph); // default false (graph enabled)
                assert!(matches!(viz_mode, VizMode::Auto)); // default auto
            }
            _ => panic!("Expected Generate command"),
        }
    }

    #[test]
    fn cli_parses_generate_with_format() {
        let cli = Cli::try_parse_from([
            "panschema",
            "generate",
            "--input",
            "test.ttl",
            "--output",
            "output.jsonld",
            "--format",
            "jsonld",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Generate {
                input,
                output,
                format,
                ..
            }) => {
                assert_eq!(input, Some(PathBuf::from("test.ttl")));
                assert_eq!(output, PathBuf::from("output.jsonld"));
                assert_eq!(format, "jsonld");
            }
            _ => panic!("Expected Generate command"),
        }
    }

    #[test]
    fn cli_parses_generate_with_viz_mode() {
        let cli = Cli::try_parse_from([
            "panschema",
            "generate",
            "--input",
            "test.ttl",
            "--viz-mode",
            "2d",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Generate { viz_mode, .. }) => {
                assert!(matches!(viz_mode, VizMode::Canvas2D));
            }
            _ => panic!("Expected Generate command"),
        }

        let cli = Cli::try_parse_from([
            "panschema",
            "generate",
            "--input",
            "test.ttl",
            "--viz-mode",
            "3d",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Generate { viz_mode, .. }) => {
                assert!(matches!(viz_mode, VizMode::WebGPU3D));
            }
            _ => panic!("Expected Generate command"),
        }
    }

    #[test]
    fn cli_parses_generate_no_graph() {
        let cli =
            Cli::try_parse_from(["panschema", "generate", "--input", "test.ttl", "--no-graph"])
                .unwrap();
        match cli.command {
            Some(Commands::Generate { no_graph, .. }) => {
                assert!(no_graph);
            }
            _ => panic!("Expected Generate command"),
        }
    }

    #[test]
    fn cli_parses_generate_with_no_input_for_manifest_mode() {
        let cli = Cli::try_parse_from(["panschema", "generate"]).unwrap();
        match cli.command {
            Some(Commands::Generate { input, .. }) => {
                assert!(
                    input.is_none(),
                    "input should be None to trigger manifest discovery"
                );
            }
            _ => panic!("Expected Generate command"),
        }
    }

    #[test]
    fn cli_parses_serve_subcommand() {
        let cli = Cli::try_parse_from([
            "panschema",
            "serve",
            "--input",
            "test.ttl",
            "--port",
            "8080",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Serve { input, port, .. }) => {
                assert_eq!(input, PathBuf::from("test.ttl"));
                assert_eq!(port, 8080);
            }
            _ => panic!("Expected Serve command"),
        }
    }

    #[test]
    #[cfg(feature = "dev")]
    fn cli_parses_styleguide_subcommand() {
        let cli = Cli::try_parse_from([
            "panschema",
            "styleguide",
            "--output",
            "styleguide-output",
            "--serve",
            "--port",
            "4000",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Styleguide {
                output,
                serve,
                port,
            }) => {
                assert_eq!(output, PathBuf::from("styleguide-output"));
                assert!(serve);
                assert_eq!(port, 4000);
            }
            _ => panic!("Expected Styleguide command"),
        }
    }
}
