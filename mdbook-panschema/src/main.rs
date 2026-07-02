use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use mdbook_panschema::{InstallReport, run};

/// mdbook plugin for panschema: install a toolbar link from an mdbook
/// book to its panschema-generated schema docs.
#[derive(Parser)]
#[command(name = "mdbook-panschema", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Install the mdbook→schema toolbar-link assets into a book and wire
    /// them into its `book.toml`. Configured by `[book_link]` in
    /// `panschema-publish.toml`. Re-run after upgrading to refresh.
    Install {
        /// Book directory (the one containing `book.toml`). Defaults to
        /// the current directory.
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Install { dir } => {
            match run(&dir)? {
                InstallReport::Installed => println!(
                    "Installed schema-link assets into {} and wired book.toml.",
                    dir.display()
                ),
                InstallReport::Disabled => {
                    println!("`[book_link].enabled` is false; nothing to install.")
                }
                InstallReport::NoBookLink => {
                    println!(
                        "No `[book_link]` section in panschema-publish.toml; nothing to install."
                    )
                }
            }
            Ok(())
        }
    }
}
