use clap::Parser;

/// A blazing fast, Rust-based ontology documentation generator.
#[derive(Parser)]
#[command(name = "rontodoc")]
#[command(version, about, long_about = None)]
struct Cli {
    /// Input ontology file (.ttl)
    #[arg(short, long)]
    input: Option<std::path::PathBuf>,

    /// Output directory for generated documentation
    #[arg(short, long, default_value = "output")]
    output: std::path::PathBuf,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if let Some(input) = cli.input {
        println!(
            "rontodoc: will generate docs from {} to {}",
            input.display(),
            cli.output.display()
        );
    } else {
        println!("rontodoc: no input specified. Use --help for usage.");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_with_defaults() {
        // Verify CLI can be constructed with default output path
        let cli = Cli::try_parse_from(["rontodoc"]).unwrap();
        assert_eq!(cli.output, std::path::PathBuf::from("output"));
        assert!(cli.input.is_none());
    }

    #[test]
    fn cli_parses_input_and_output() {
        let cli =
            Cli::try_parse_from(["rontodoc", "--input", "test.ttl", "--output", "docs"]).unwrap();
        assert_eq!(cli.input, Some(std::path::PathBuf::from("test.ttl")));
        assert_eq!(cli.output, std::path::PathBuf::from("docs"));
    }
}
