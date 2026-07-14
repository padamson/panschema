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

    /// Output format: html, ttl, jsonld, rdfxml, ntriples, graph-json, rust, postgres, shacl
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

        /// Output format: html, ttl, jsonld, rdfxml, ntriples, graph-json, rust, postgres, shacl
        #[arg(short, long, default_value = "html")]
        format: String,

        /// Disable interactive graph visualization (HTML output only)
        #[arg(long = "no-graph")]
        no_graph: bool,

        /// Visualization mode: auto, 2d, 3d (requires --graph)
        #[arg(long, value_enum, default_value = "auto")]
        viz_mode: VizMode,

        /// Skip fetching upstream ontology labels (cached labels
        /// still render; uncached external references show CURIEs).
        #[arg(long)]
        offline: bool,

        /// Delete cached upstream labels for this schema's sources
        /// and refetch them before rendering.
        #[arg(long = "refresh-labels")]
        refresh_labels: bool,

        /// Fail (non-zero exit) instead of only warning when the schema uses
        /// a LinkML construct panschema parses but does not model, or contains
        /// a dangling reference (a `range`, `is_a`, `mixin`, or `inverse`
        /// naming nothing the schema defines).
        #[arg(long)]
        strict: bool,
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
    /// Build versioned HTML docs from a `panschema-publish.toml` with a
    /// `[publishing]` section. Produces `<output>/<tag>/` per version,
    /// `<output>/<edge>/` if edge is configured, and a `<output>/current/`
    /// alias that mirrors the configured version's output.
    Publish {
        /// Path to `panschema-publish.toml`. Defaults to the file in CWD.
        #[arg(long, default_value = "panschema-publish.toml")]
        manifest: PathBuf,
        /// Output directory for per-version doc trees. Overrides the
        /// manifest's `[publishing].output_dir`. Resolved relative to the
        /// manifest's parent directory when relative.
        ///
        /// Distinct long name (`--output-dir`) avoids collision with the
        /// global `--output` flag, whose `default_value` would otherwise
        /// shadow a `None` here through clap's arg propagation.
        #[arg(long = "output-dir")]
        output_dir: Option<PathBuf>,
        /// Read the `edge` ref's schema from the working tree instead
        /// of `git show <ref>:<path>`. Local dev preview pattern: the
        /// edit-and-refresh loop reflects uncommitted state. CI should
        /// NOT set this — released builds must stay reproducible from
        /// committed refs. Tagged versions in `[publishing].versions`
        /// are unaffected by this flag.
        #[arg(long = "edge-from-worktree")]
        edge_from_worktree: bool,
    },
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

        /// Bind all interfaces (0.0.0.0) instead of loopback only, exposing
        /// the server to the local network. Off by default.
        #[arg(long = "host-all")]
        host_all: bool,
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

/// Label-cache behavior for one generate run: offline skips fetching,
/// refresh evicts cached sources first, and overrides remap a prefix
/// to a custom source URL (from the manifest's `[label_sources]`).
struct LabelOptions<'a> {
    offline: bool,
    refresh: bool,
    overrides: &'a std::collections::BTreeMap<String, String>,
}

// A `generate` CLI-command handler: its parameters mirror the subcommand's
// flags, so the count exceeds clippy's default. (A `GenerateOptions` struct
// would tidy this — a future cleanup, orthogonal to any one feature.)
#[allow(clippy::too_many_arguments)]
fn generate(
    input: &Path,
    output: &Path,
    format: &str,
    include_graph: bool,
    html_graph_aspect: Option<&str>,
    html_default_layout: Option<&str>,
    labels: &LabelOptions,
    strict: bool,
    deps: &std::collections::BTreeMap<String, PathBuf>,
) -> anyhow::Result<()> {
    let registry = FormatRegistry::with_defaults();

    // Read the input and fold in any `imports:` through the shared load path,
    // so `generate` renders the same merged schema as `serve`/`publish`.
    // `deps` lets an `imports:` entry naming a manifest dependency resolve
    // across the package boundary; it's empty for a single-file `--input`.
    let schema = panschema::import_resolve::load_schema_with_deps(input, &registry, deps)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // The unmodeled-construct, unresolved-unique-key, and dangling-reference
    // warnings are emitted by the shared load path above (so `serve`/`publish`
    // surface them too). Under `--strict`, an unmodeled construct or a dangling
    // reference is additionally a hard error here.
    let unmodeled = panschema::diagnostics::unmodeled_class_constructs(&schema);
    let dangling = panschema::diagnostics::dangling_references(&schema);
    if panschema::diagnostics::should_fail_strict(&unmodeled, &dangling, strict) {
        anyhow::bail!(
            "{} unmodeled LinkML construct(s) and {} dangling reference(s) present; \
             failing because --strict is set",
            unmodeled.len(),
            dangling.len()
        );
    }

    // `rules` and `unique_keys` are IR-modeled (so `unmodeled_class_constructs`
    // above stays silent about them) but not every writer projects them — warn
    // for a format that doesn't, naming that format rather than assuming RDF.
    // (Empty for the formats that do project the construct, so this is safe to
    // call unconditionally.)
    for u in panschema::diagnostics::classes_with_unprojected_constructs(&schema, format) {
        eprintln!("warning: {}", u.message(format));
    }

    // The Postgres writer covers concrete classes with scalar/enum/
    // single-valued-class-reference slots only; a class using `is_a`, a
    // multivalued slot, or `any_of` — or referencing one of those — is
    // skipped rather than emitted as broken DDL. Warn so the omission is
    // visible instead of a schema silently producing a thinner script.
    if format.eq_ignore_ascii_case("postgres") {
        for skipped in panschema::postgres_writer::skipped_classes(&schema) {
            eprintln!(
                "warning: class `{}` has no postgres table: {}",
                skipped.class, skipped.reason
            );
        }
        // A rule on a table-bearing class that can't become a CHECK is
        // dropped from the DDL; warn so it isn't a silent gap (a class
        // with no table at all is already covered above).
        for skipped in panschema::postgres_writer::skipped_rules(&schema) {
            eprintln!(
                "warning: rule `{}` on class `{}` is not emitted as a postgres CHECK: {}",
                skipped.rule, skipped.class, skipped.reason
            );
        }
    }

    // The SHACL writer projects most `rules` as conditional shapes, but a
    // one-sided rule, an empty condition side, or a condition naming a slot
    // the class doesn't have has no shape form — dropped rather than
    // emitting a shape over a fabricated property IRI. Warn so it isn't a
    // silent gap.
    if format.eq_ignore_ascii_case("shacl") {
        for skipped in panschema::rdf_serializers::shacl_skipped_rules(&schema) {
            eprintln!(
                "warning: rule `{}` on class `{}` is not emitted as a SHACL shape: {}",
                skipped.rule, skipped.class, skipped.reason
            );
        }
    }

    // For HTML format, use HtmlWriter with custom options
    if format.eq_ignore_ascii_case("html") {
        use panschema::html_writer::{HtmlWriter, parse_graph_aspect};
        use panschema::io::Writer;
        use panschema::manifest::validate_layout_name;
        let (aw, ah) = match html_graph_aspect {
            Some(s) => parse_graph_aspect(s).map_err(|e| anyhow::anyhow!("{}", e))?,
            None => (16, 8),
        };
        // `auto` is the not-pinned sentinel: the viz picks a default
        // from the graph's inheritance density at render time. An
        // explicit manifest layout still validates and pins.
        let layout = html_default_layout.unwrap_or("auto");
        if layout != "auto" {
            validate_layout_name(layout).map_err(|e| anyhow::anyhow!("{}", e))?;
        }
        let mut writer = HtmlWriter::with_options(include_graph)
            .with_graph_aspect(aw, ah)
            .with_default_layout(layout);
        if let Some(store) = panschema::labels::open_default_store(
            &schema,
            labels.offline,
            labels.overrides,
            labels.refresh,
        ) {
            writer = writer.with_label_store(store);
        }
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
fn generate_from_manifest(offline: bool, refresh_labels: bool, strict: bool) -> anyhow::Result<()> {
    use anyhow::Context as _;
    let (manifest, manifest_dir) = load_manifest()?;
    let labels = LabelOptions {
        offline,
        refresh: refresh_labels,
        overrides: &manifest.label_sources,
    };

    if manifest.schemas.is_empty() {
        eprintln!("Manifest has no `[schemas]` entries; nothing to do.");
        return Ok(());
    }

    // Resolve every declared schema to its main file up front, so an
    // `imports:` entry naming another `[schemas]` dependency can resolve to
    // that dependency's schema across the package boundary. Import-only
    // dependencies (no `[generate]` block) still populate this map.
    let mut deps: std::collections::BTreeMap<String, PathBuf> = std::collections::BTreeMap::new();
    for (name, dep) in &manifest.schemas {
        let panschema::source::Resolved { schema_path, .. } =
            resolve_source(name, dep, &manifest_dir)?;
        deps.insert(name.clone(), schema_path);
    }

    let mut produced_anything = false;
    for name in manifest.schemas.keys() {
        let schema_path = &deps[name];
        let Some(gen_cfg) = manifest.generate.get(name) else {
            eprintln!("schema `{name}`: no [generate.{name}] block; skipping");
            continue;
        };
        if let Some(html_out) = &gen_cfg.html {
            let html_out = manifest_dir.join(html_out);
            generate(
                schema_path,
                &html_out,
                "html",
                true,
                gen_cfg.html_graph_aspect.as_deref(),
                gen_cfg.html_default_layout.as_deref(),
                &labels,
                strict,
                &deps,
            )
            .with_context(|| format!("schema `{name}`, format `html`"))?;
            produced_anything = true;
        }
        // Every non-HTML writer is a single output file with a uniform call
        // shape; fan out over the configured ones. HTML stays separate above
        // because it writes a directory and takes viz options.
        for (format, out_opt) in [
            ("rust", &gen_cfg.rust),
            ("postgres", &gen_cfg.postgres),
            ("shacl", &gen_cfg.shacl),
            ("ttl", &gen_cfg.ttl),
            ("jsonld", &gen_cfg.jsonld),
            ("rdfxml", &gen_cfg.rdfxml),
            ("ntriples", &gen_cfg.ntriples),
            ("graph-json", &gen_cfg.graph_json),
        ] {
            let Some(out) = out_opt else { continue };
            let out = manifest_dir.join(out);
            generate(
                schema_path,
                &out,
                format,
                false,
                None,
                None,
                &labels,
                strict,
                &deps,
            )
            .with_context(|| format!("schema `{name}`, format `{format}`"))?;
            produced_anything = true;
        }
    }

    if !produced_anything {
        eprintln!(
            "No outputs generated. Add an `[generate.<schema>]` block with at least one writer key (e.g. `html = \"docs/\"` or `rust = \"src/generated.rs\"`)."
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

    // Fix 4: track provenance so the user can see which fields were
    // explicit vs derived from --from vs defaulted.
    let (resolved_name, name_src) = match name {
        Some(n) => (n.to_string(), "explicit"),
        None => match from_name {
            Some(n) => (n, "from --from"),
            None => (
                cwd.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("schema")
                    .to_string(),
                "default (CWD basename)",
            ),
        },
    };
    let (resolved_version, version_src) = match version {
        Some(v) => (v.to_string(), "explicit"),
        None => match from_version {
            Some(v) => (v, "from --from"),
            None => ("0.1.0".to_string(), "default"),
        },
    };
    let (resolved_main, main_src) = match main {
        Some(m) => (m.to_path_buf(), "explicit"),
        None => match from {
            Some(p) => (p.to_path_buf(), "from --from"),
            None => (PathBuf::from("schema.yaml"), "default"),
        },
    };
    // The `--linkml` flag has a clap-level default of "1.7.0", so we can't
    // tell whether the user passed it explicitly. Label it as "(default
    // unless overridden)" — better to be slightly under-precise than to
    // claim provenance we don't know.
    let linkml_src = "default unless overridden via --linkml";

    let path = init_publish_file(
        &cwd,
        &resolved_name,
        &resolved_version,
        &resolved_main,
        linkml,
        force,
    )?;
    println!("Wrote {}", path.display());
    println!("    name    = \"{resolved_name}\"  ({name_src})");
    println!("    version = \"{resolved_version}\"  ({version_src})");
    println!(
        "    main    = \"{}\"  ({main_src})",
        resolved_main.display()
    );
    println!("    linkml  = \"{linkml}\"  ({linkml_src})");

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

    // Fix 1: refuse no-op bumps. `--version <V>` when publish.toml is
    // already at V plans a `git commit` with nothing staged, which would
    // fail at runtime. Catch it up-front with a clear message.
    if old_version == projected_new {
        anyhow::bail!(
            "version is already `{old_version}`; nothing to bump. \
             Either pass `--level patch|minor|major` to advance it, \
             or tag the current commit manually:\n    \
             git tag -a -m 'release v{old_version}' v{old_version} && git push --follow-tags"
        );
    }

    // Fix 3: refuse if the LinkML main file has a version field that
    // disagrees with publish.toml. The two are both versions of the
    // same package; releasing while they drift produces a published
    // schema with inconsistent self-description.
    enforce_linkml_version_in_sync(&cwd, &current_cfg)?;

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
            println!("    git tag -a -m 'release {new_tag}' {new_tag}");
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
        let commit_msg = format!("release: {new_tag}");
        run_git(&cwd, &["commit", "-m", &commit_msg])?;
        // Fix 2: annotated tag (-a -m) instead of lightweight. Annotated
        // tags carry author/date/message and — crucially — are the only
        // tag kind that `git push --follow-tags` will push.
        let tag_msg = format!("release {new_tag}");
        run_git(&cwd, &["tag", "-a", "-m", &tag_msg, &new_tag])?;
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
        println!("    git tag -a -m 'release {new_tag}' {new_tag}");
        println!("    git push --follow-tags");
    }

    Ok(())
}

/// Read the LinkML main file referenced by publish.toml and, if it
/// declares a `version:` field, refuse to release while it disagrees
/// with publish.toml's `[schema].version`. Files without a declared
/// version skip the check.
fn enforce_linkml_version_in_sync(
    cwd: &Path,
    publish: &panschema::publish::PublishConfig,
) -> anyhow::Result<()> {
    use panschema::io::FormatRegistry;

    let main_path = cwd.join(&publish.files.main);
    if !main_path.exists() {
        // `init` is allowed to write publish.toml before the main file
        // exists; the same lenience applies here. The user will notice
        // soon enough when nothing parses.
        return Ok(());
    }
    let registry = FormatRegistry::with_defaults();
    let reader = registry
        .reader_for_path(&main_path)
        .map_err(|e| anyhow::anyhow!("schema main file `{}`: {e}", main_path.display()))?;
    let schema = reader
        .read(&main_path)
        .map_err(|e| anyhow::anyhow!("schema main file `{}`: {e}", main_path.display()))?;
    if let Some(linkml_version) = schema.version
        && linkml_version != publish.schema.version
    {
        anyhow::bail!(
            "version drift: `panschema-publish.toml` declares `{}` \
             but `{}` declares `version: {linkml_version}`. \
             Bring them into sync before releasing (the two must agree).",
            publish.schema.version,
            main_path.display()
        );
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
) -> anyhow::Result<()> {
    use panschema::manifest::{
        AddOutcome, AddRequest, MANIFEST_FILENAME, SchemaSpec, discover_manifest, insert_schema,
    };

    // Find the manifest first — we'll need its directory for path
    // relativization, and we want to fail fast if there isn't one.
    let cwd = std::env::current_dir()?;
    let manifest_path = discover_manifest(&cwd).ok_or_else(|| {
        anyhow::anyhow!(
            "no `{MANIFEST_FILENAME}` found in `{}` or any ancestor.\n\
             \n\
             Run this to create one, then re-run `panschema add`:\n\
             \n    \
             echo '[schemas]' > {MANIFEST_FILENAME}",
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
            // Use `pkg_dir` (the package root) rather than
            // `schema_path.parent()`, which only points at the package root
            // for flat layouts (`[files].main = "schema.yaml"`) and lands
            // inside a subdirectory for the recommended nested layout
            // (`[files].main = "schema/<name>.yaml"`).
            let publish = panschema::publish::PublishConfig::from_path(
                &resolved.pkg_dir.join("panschema-publish.toml"),
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

    let outcome = insert_schema(&manifest_path, &request)?;
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

#[derive(Debug)]
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
            ..
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

/// `panschema publish`: build versioned HTML docs from a
/// `panschema-publish.toml` with a `[publishing]` section. Each entry
/// in `versions` (and `edge` if set) becomes `<output>/<ref>/`, and
/// `<output>/current/` mirrors the configured version.
///
/// Thin CLI plumbing around [`panschema::publish::publish_versioned`].
/// `#[mutants::skip]` because the real orchestration logic lives in
/// the library function (which has full unit coverage); CLI-level
/// regressions are caught by the integration test that shells out
/// to this binary, but those tests aren't visible to
/// `cargo mutants --lib`.
#[mutants::skip]
fn publish_command(
    manifest: &Path,
    output_override: Option<&Path>,
    edge_from_worktree: bool,
) -> anyhow::Result<()> {
    use panschema::publish::{PublishConfig, publish_versioned};

    let manifest = if manifest.is_absolute() {
        manifest.to_path_buf()
    } else {
        std::env::current_dir()?.join(manifest)
    };
    if !manifest.exists() {
        anyhow::bail!(
            "publish file not found: {}\n\
             Create one with `panschema init` or pass `--manifest <path>`.",
            manifest.display()
        );
    }
    let manifest_dir = manifest.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "publish file has no parent directory: {}",
            manifest.display()
        )
    })?;

    let cfg = PublishConfig::from_path(&manifest)?;
    let publishing = cfg.publishing.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "`{}` has no [publishing] section — `panschema publish` requires one. \
             See docs/features/11-versioned-docs-publish.md.",
            manifest.display()
        )
    })?;

    // Output dir precedence: CLI flag > manifest field. Relative paths
    // resolve against the manifest's parent (the repo root, typically).
    let output_dir = output_override
        .map(Path::to_path_buf)
        .unwrap_or_else(|| publishing.output_dir.clone());
    let output_dir = if output_dir.is_absolute() {
        output_dir
    } else {
        manifest_dir.join(output_dir)
    };

    publish_versioned(manifest_dir, &cfg, &output_dir, edge_from_worktree)?;

    println!(
        "Published {} versions to {}",
        publishing.versions.len() + publishing.edge.iter().count(),
        output_dir.display()
    );
    Ok(())
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
            offline,
            refresh_labels,
            strict,
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
                let no_overrides = std::collections::BTreeMap::new();
                let labels = LabelOptions {
                    offline,
                    refresh: refresh_labels,
                    overrides: &no_overrides,
                };
                let no_deps = std::collections::BTreeMap::new();
                generate(
                    &input, &output, &format, !no_graph, None, None, &labels, strict, &no_deps,
                )?;
            }
            None => generate_from_manifest(offline, refresh_labels, strict)?,
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
        Some(Commands::Add { spec, name }) => add_schema(spec, name.as_deref())?,
        Some(Commands::Release {
            level,
            version,
            git,
            push,
            dry_run,
        }) => release_schema(level, version.as_deref(), git, push, dry_run)?,
        Some(Commands::Fetch) => fetch_from_manifest()?,
        Some(Commands::Verify) => verify_from_manifest()?,
        Some(Commands::Publish {
            manifest,
            output_dir,
            edge_from_worktree,
        }) => publish_command(&manifest, output_dir.as_deref(), edge_from_worktree)?,
        Some(Commands::Serve {
            input,
            output,
            port,
            host_all,
        }) => {
            server::serve(&input, &output, port, host_all).await?;
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
                let no_overrides = std::collections::BTreeMap::new();
                let labels = LabelOptions {
                    offline: false,
                    refresh: false,
                    overrides: &no_overrides,
                };
                // Bare-CLI fallback (no subcommand): strict mode is a
                // `generate`-subcommand flag, so it's off here.
                let no_deps = std::collections::BTreeMap::new();
                generate(
                    &input,
                    &cli.output,
                    &cli.format,
                    true,
                    None,
                    None,
                    &labels,
                    false,
                    &no_deps,
                )?;
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
    fn help_text_lists_every_registered_writer_format() {
        // The `--format` help strings are hand-written, disconnected from
        // `FormatRegistry` — they can silently drift when a writer is
        // added (this is how `rust` was found missing from both the
        // top-level and `generate` subcommand help text). Assert every
        // registered format id actually appears in both.
        let registry = panschema::io::FormatRegistry::with_defaults();
        let format_ids = registry.writer_format_ids();

        let top_level_help = Cli::command().render_long_help().to_string();
        let generate_help = Cli::command()
            .find_subcommand("generate")
            .expect("generate subcommand")
            .clone()
            .render_long_help()
            .to_string();

        for id in &format_ids {
            assert!(
                top_level_help.contains(id),
                "top-level --help must list format `{id}`; got:\n{top_level_help}"
            );
            assert!(
                generate_help.contains(id),
                "generate --help must list format `{id}`; got:\n{generate_help}"
            );
        }
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
                offline,
                refresh_labels,
                strict,
            }) => {
                assert_eq!(input, Some(PathBuf::from("test.ttl")));
                assert_eq!(output, PathBuf::from("docs"));
                assert_eq!(format, "html");
                assert!(!no_graph); // default false (graph enabled)
                assert!(matches!(viz_mode, VizMode::Auto)); // default auto
                assert!(!offline); // default false (labels fetched)
                assert!(!refresh_labels); // default false (cache reused)
                assert!(!strict); // default false (warn, don't fail)
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

    // ----- parse_github_uri --------------------------------------------

    #[test]
    fn parse_github_uri_accepts_well_formed_spec() {
        let g = parse_github_uri("github:padamson/scimantic-schema").unwrap();
        assert_eq!(g.owner, "padamson");
        assert_eq!(g.repo, "scimantic-schema");
    }

    #[test]
    fn parse_github_uri_rejects_empty_owner() {
        let err = parse_github_uri("github:/repo").unwrap_err();
        assert!(err.to_string().contains("malformed"));
    }

    #[test]
    fn parse_github_uri_rejects_empty_repo() {
        let err = parse_github_uri("github:owner/").unwrap_err();
        assert!(err.to_string().contains("malformed"));
    }

    #[test]
    fn parse_github_uri_rejects_repo_containing_slash() {
        // `repo.contains('/')` rejects three-segment paths so the
        // url builder doesn't silently produce a malformed URL.
        let err = parse_github_uri("github:owner/sub/repo").unwrap_err();
        assert!(err.to_string().contains("malformed"));
    }

    #[test]
    fn parse_github_uri_rejects_missing_protocol_prefix() {
        let err = parse_github_uri("gitlab:owner/repo").unwrap_err();
        assert!(
            err.to_string().contains("expected `github:owner/repo`"),
            "got: {err}"
        );
    }

    #[test]
    fn parse_github_uri_rejects_missing_slash() {
        let err = parse_github_uri("github:owner-only").unwrap_err();
        assert!(err.to_string().contains("malformed"));
    }

    // ----- relative_path -----------------------------------------------

    #[test]
    fn relative_path_returns_absolute_target_when_no_shared_ancestor() {
        // When the two paths share no common root, fall back to the
        // absolute target rather than building a `../` walk that
        // crosses volume / filesystem boundaries.
        let base = Path::new("foo/bar"); // no RootDir prefix
        let target = Path::new("baz/qux");
        let rel = relative_path(base, target);
        // Without a RootDir prefix, the function returns the target as-is.
        assert_eq!(rel, target.to_path_buf());
    }

    #[test]
    fn relative_path_walks_up_to_common_ancestor() {
        let base = Path::new("/a/b/c");
        let target = Path::new("/a/x/y");
        let rel = relative_path(base, target);
        // From /a/b/c to /a/x/y: up twice (to /a), then down into x/y.
        assert_eq!(rel, PathBuf::from("../../x/y"));
    }

    #[test]
    fn relative_path_emits_dot_when_paths_equal() {
        // base == target: zero ../, zero descents → an empty PathBuf
        // becomes "." so callers can `.join()` against it.
        let base = Path::new("/a/b");
        let target = Path::new("/a/b");
        assert_eq!(relative_path(base, target), PathBuf::from("."));
    }

    #[test]
    fn relative_path_target_under_base_is_pure_descent() {
        // Sibling-of-self: target nested under base produces a
        // forward-only path.
        let base = Path::new("/a/b");
        let target = Path::new("/a/b/c/d");
        assert_eq!(relative_path(base, target), PathBuf::from("c/d"));
    }

    #[test]
    fn relative_path_anchored_base_relative_target_returns_target_as_is() {
        // Mixing an anchored base with a relative target produces
        // `common_prefix == 0` even though the base IS anchored. The
        // early-return must fire on the "no shared prefix" condition
        // alone — otherwise we'd build a meaningless `../../../c/d`
        // by walking up from every base component.
        let base = Path::new("/a/b");
        let target = Path::new("c/d");
        assert_eq!(relative_path(base, target), PathBuf::from("c/d"));
    }
}
