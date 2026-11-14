use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use panschema::editions::{self, InstalledToolchain};
use panschema::mdbook::{InstallReport, run};

/// mdbook plugin for panschema: install a toolbar link from an mdbook
/// book to its panschema-generated schema docs, and publish the book in
/// editions that track the schema's releases.
#[derive(Parser)]
#[command(
    name = "mdbook-panschema",
    version,
    about,
    after_help = "Exit status: 0 when the command did what it was asked, 2 when it could not run."
)]
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
    /// Build every edition `[[editions.edition]]` declares from
    /// `[publishing].edge`, each covering the releases from its `from` to
    /// the next edition's. Writes `book/<id>/` under
    /// `[editions].output_dir` and copies the current edition to its
    /// root. Configured by `[editions]` and `[publishing]` in
    /// `panschema-publish.toml`.
    Editions {
        /// Any directory at or below `panschema-publish.toml`. Defaults
        /// to the current directory.
        #[arg(default_value = ".")]
        dir: PathBuf,
        /// Print the book directory of the authoring edition, the one
        /// new chapters and listings go into, and build nothing.
        #[arg(long)]
        authoring: bool,
    },
}

/// Exits 2 for a command that could not run, as clap does on its own
/// for a bad flag, printing the error the way `anyhow` prints one
/// returned from `main`.
fn main() -> ExitCode {
    match run_command(Cli::parse().command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Error: {err:?}");
            ExitCode::from(2)
        }
    }
}

fn run_command(command: Command) -> Result<()> {
    match command {
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
        Command::Editions { dir, authoring } => {
            if authoring {
                // The path's own bytes, through a write that can fail:
                // a script reads this line as a path, and a closed pipe
                // is an error like any other.
                let book = editions::authoring_book(&dir)?;
                let mut out = std::io::stdout().lock();
                out.write_all(book.as_os_str().as_encoded_bytes())?;
                out.write_all(b"\n")?;
                return Ok(());
            }
            let report = editions::run(&dir, &InstalledToolchain::in_cache()?)?;
            for edition in &report.plan.editions {
                println!(
                    "{}/book/{}/  from {}",
                    report.site.display(),
                    edition.id,
                    edition.book.display()
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
