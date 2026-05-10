use std::path::{Path, PathBuf};

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

#[cfg(feature = "dev")]
mod components;
mod server;

use panschema::io::FormatRegistry;

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
            revision,
        } = resolve_source(name, dep, &manifest_dir)?;
        let source = SchemaSource::from_dep(name, dep)?;
        entries.push(LockEntry {
            name: name.clone(),
            version: dep.version.clone(),
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
