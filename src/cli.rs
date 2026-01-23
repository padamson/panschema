use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Path to the input ontology file (Turtle/RDF)
    #[arg(short, long)]
    pub input: PathBuf,

    /// Directory to output the generated documentation
    #[arg(short, long)]
    pub output: PathBuf,
}
