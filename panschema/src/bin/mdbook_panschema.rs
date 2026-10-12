use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use panschema::editions::{self, PathToolchain};
use panschema::mdbook::{InstallReport, run};

/// mdbook plugin for panschema: install a toolbar link from an mdbook
/// book to its panschema-generated schema docs, and publish the book in
/// editions that track the schema's releases.
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
    /// Build the book once per schema release line, each from its git
    /// tag, plus the unreleased edition from `[publishing].edge`. Writes
    /// `book/<edition>/` under `[editions].output_dir` and copies the
    /// current edition to its root. Configured by `[editions]` and
    /// `[publishing]` in `panschema-publish.toml`.
    Editions {
        /// Any directory at or below `panschema-publish.toml`. Defaults
        /// to the current directory.
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
        Command::Editions { dir } => {
            let report = editions::run(&dir, &PathToolchain::default())?;
            for edition in &report.plan.editions {
                println!(
                    "{}/book/{}/  from {}",
                    report.site.display(),
                    edition.label,
                    edition.source
                );
            }
            println!(
                "{}/  copy of {}",
                report.site.display(),
                report.plan.current
            );
            Ok(())
        }
    }
}
