//! Install the Playwright browsers the e2e suite drives.
//!
//! The browsers must match the driver `playwright-rs` vendors, and that
//! version moves whenever the dependency does. Asking the crate to install
//! them keeps the two together by construction; naming a version here — or
//! in a workflow, or in a `package.json` — is a pin that goes stale on the
//! next bump and fails as "Executable doesn't exist" rather than as a
//! version mismatch. Nothing in this repository names one.
//!
//! ```bash
//! cargo run --example install-browsers                 # all three
//! cargo run --example install-browsers -- chromium     # or a subset
//! cargo run --example install-browsers -- --with-deps  # Linux: system libraries too
//! ```
//!
//! A plain install installs browsers only. On Linux they also need system
//! libraries — WebKit needs some three dozen — and without them the install
//! still exits 0 and the browsers fail to launch. `--with-deps` installs
//! those too, through the driver's own `apt` step (which uses `sudo`), and
//! is what CI runs on Linux.

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The crate reports missing system libraries as a warning rather than an
    // error, since the driver exits 0 either way; give it somewhere to go.
    tracing_subscriber::fmt().with_env_filter("warn").init();

    let mut with_deps = false;
    let mut browsers = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--with-deps" => with_deps = true,
            flag if flag.starts_with('-') => {
                eprintln!(
                    "unknown option {flag}\nusage: install-browsers [--with-deps] [browser...]"
                );
                std::process::exit(2);
            }
            name => browsers.push(name.to_string()),
        }
    }

    let names: Vec<&str> = browsers.iter().map(String::as_str).collect();
    let selected = (!names.is_empty()).then_some(names.as_slice());

    println!(
        "Installing Playwright {} browsers: {}{}",
        playwright_rs::PLAYWRIGHT_VERSION,
        selected.map_or_else(|| "all".to_string(), |s| s.join(", ")),
        if with_deps {
            " (with system dependencies)"
        } else {
            ""
        }
    );
    if with_deps {
        playwright_rs::install_browsers_with_deps(selected).await?;
    } else {
        playwright_rs::install_browsers(selected).await?;
    }
    Ok(())
}
