//! Install the Playwright browsers the e2e suite drives.
//!
//! The browsers must match the driver `playwright-rs` vendors, and that
//! version moves whenever the dependency does. Asking the crate to install
//! them keeps the two together by construction; naming a version here — or
//! in a workflow, or in a `package.json` — is a pin that goes stale on the
//! next bump and fails as "Executable doesn't exist" rather than as a
//! version mismatch.
//!
//! ```bash
//! cargo run --example install-browsers                     # all three
//! cargo run --example install-browsers -- chromium         # or a subset
//! ```
//!
//! On Linux the driver also installs the system libraries the browsers
//! need, so CI needs no separate step for them.

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let browsers: Vec<String> = std::env::args().skip(1).collect();
    let selected: Vec<&str> = browsers.iter().map(String::as_str).collect();

    println!(
        "Installing Playwright {} browsers: {}",
        playwright_rs::PLAYWRIGHT_VERSION,
        if selected.is_empty() {
            "all".to_string()
        } else {
            selected.join(", ")
        }
    );
    playwright_rs::install_browsers(if selected.is_empty() {
        None
    } else {
        Some(&selected)
    })
    .await?;
    Ok(())
}
