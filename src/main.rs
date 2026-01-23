use anyhow::Result;
use askama::Template;
use clap::Parser;
use rontodoc::{
    cli,
    // generator::Entity, // Removed unused import
    parser,
};
use std::fs;

fn main() -> Result<()> {
    let args = cli::Args::parse();

    // 1. Ensure output dir exists
    if !args.output.exists() {
        fs::create_dir_all(&args.output)?;
    }

    // 2. Load Ontology (Verification that it parses, even if we don't query it yet)
    let (store, prefixes) = parser::load_ontology(&args.input)?;

    // 3. Prepare Template Data (Dynamic Extraction)
    let template = rontodoc::extractor::extract_metadata(&store, prefixes)?;
    // 4. Render and Write
    let output_content = template.render()?;
    fs::write(args.output.join("index.html"), output_content)?;

    Ok(())
}
