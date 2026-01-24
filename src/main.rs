use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

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
}
