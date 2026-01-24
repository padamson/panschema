use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

#[cfg(feature = "dev")]
mod components;
mod model;
mod parser;
mod renderer;
mod server;

/// A blazing fast, Rust-based ontology documentation generator.
#[derive(Parser)]
#[command(name = "rontodoc")]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Input ontology file (.ttl) - used when no subcommand specified
    #[arg(short, long, global = true)]
    input: Option<PathBuf>,

    /// Output directory for generated documentation
    #[arg(short, long, global = true, default_value = "output")]
    output: PathBuf,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate documentation (default behavior)
    Generate {
        /// Input ontology file (.ttl)
        #[arg(short, long)]
        input: PathBuf,

        /// Output directory for generated documentation
        #[arg(short, long, default_value = "output")]
        output: PathBuf,
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

fn generate(input: &Path, output: &Path) -> anyhow::Result<()> {
    let metadata = parser::parse_ontology(input)?;
    renderer::render(&metadata, output)?;
    println!(
        "Generated documentation for '{}' in {}",
        metadata.title(),
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
        Some(Commands::Generate { input, output }) => {
            generate(&input, &output)?;
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
                generate(&input, &cli.output)?;
            } else {
                println!("rontodoc: no input specified. Use --help for usage.");
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
        let cli = Cli::try_parse_from(["rontodoc"]).unwrap();
        assert_eq!(cli.output, PathBuf::from("output"));
        assert!(cli.input.is_none());
        assert!(cli.command.is_none());
    }

    #[test]
    fn cli_parses_generate_subcommand() {
        let cli = Cli::try_parse_from([
            "rontodoc", "generate", "--input", "test.ttl", "--output", "docs",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Generate { input, output }) => {
                assert_eq!(input, PathBuf::from("test.ttl"));
                assert_eq!(output, PathBuf::from("docs"));
            }
            _ => panic!("Expected Generate command"),
        }
    }

    #[test]
    fn cli_parses_serve_subcommand() {
        let cli =
            Cli::try_parse_from(["rontodoc", "serve", "--input", "test.ttl", "--port", "8080"])
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
            "rontodoc",
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
