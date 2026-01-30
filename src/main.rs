use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

#[cfg(feature = "dev")]
mod components;
mod server;

use panschema::io::FormatRegistry;

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
    /// Generate documentation or convert to other formats
    Generate {
        /// Input ontology file (.ttl, .yaml, .yml)
        #[arg(short, long)]
        input: PathBuf,

        /// Output path (file for RDF formats, directory for HTML)
        #[arg(short, long, default_value = "output")]
        output: PathBuf,

        /// Output format: html, ttl, jsonld, rdfxml, ntriples
        #[arg(short, long, default_value = "html")]
        format: String,
    },
    /// Start development server with hot reload
    Serve {
        /// Input ontology file (.ttl)
        #[arg(short, long)]
        input: PathBuf,

        /// Output directory for generated documentation
        #[arg(short, long, default_value = "output")]
        output: PathBuf,

        /// Port to run the server on
        #[arg(short, long, default_value = "3000")]
        port: u16,
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

fn generate(input: &Path, output: &Path, format: &str) -> anyhow::Result<()> {
    let registry = FormatRegistry::with_defaults();

    let reader = registry
        .reader_for_path(input)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let schema = reader.read(input).map_err(|e| anyhow::anyhow!("{}", e))?;

    let writer = registry
        .writer_for_format(format)
        .ok_or_else(|| anyhow::anyhow!("Unsupported output format: {}", format))?;
    writer
        .write(&schema, output)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

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
        }) => {
            generate(&input, &output, &format)?;
        }
        Some(Commands::Serve {
            input,
            output,
            port,
        }) => {
            server::serve(&input, &output, port).await?;
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
            // Default behavior: generate if input provided
            if let Some(input) = cli.input {
                generate(&input, &cli.output, &cli.format)?;
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
            }) => {
                assert_eq!(input, PathBuf::from("test.ttl"));
                assert_eq!(output, PathBuf::from("docs"));
                assert_eq!(format, "html");
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
            }) => {
                assert_eq!(input, PathBuf::from("test.ttl"));
                assert_eq!(output, PathBuf::from("output.jsonld"));
                assert_eq!(format, "jsonld");
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
