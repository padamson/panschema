use assert_cmd::Command;
use playwright_rs::Playwright;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

#[tokio::test]
#[allow(clippy::expect_used)]
async fn test_end_to_end_generation() -> anyhow::Result<()> {
    // 1. Setup paths
    let reference_ttl = Path::new("tests/fixtures/reference.ttl");
    let golden_html_path = Path::new("tests/fixtures/golden_w3c.html");

    // Ensure fixtures exist
    assert!(reference_ttl.exists(), "Reference TTL not found");
    assert!(golden_html_path.exists(), "Golden HTML not found");

    // 2. Prepare output directory
    let output_dir = tempdir()?;
    let output_html_path = output_dir.path().join("index.html");

    // 3. Run rontodoc CLI
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rontodoc"));
    cmd.arg("-i")
        .arg(reference_ttl)
        .arg("-o")
        .arg(output_dir.path());

    // Expect success (This will FAIL until we implement CLI)
    cmd.assert().success();

    // 4. Snapshot Assertion (Golden Master)
    // Read generated file
    let generated_html = fs::read_to_string(&output_html_path)
        .expect("Failed to read generated index.html. Did the CLI run produce it?");
    let golden_html = fs::read_to_string(golden_html_path)?;

    // Normalize newlines just in case
    let generated_normalized = generated_html.replace("\r\n", "\n");
    let golden_normalized = golden_html.replace("\r\n", "\n");

    // Robust comparison: Ignore whitespace differences caused by template engine vs manual indentation
    // We care that the *content* and *tokens* are identical.
    let gen_tokens: Vec<&str> = generated_normalized.split_whitespace().collect();
    let golden_tokens: Vec<&str> = golden_normalized.split_whitespace().collect();

    pretty_assertions::assert_eq!(
        gen_tokens,
        golden_tokens,
        "Generated HTML tokens do not match Golden Master content"
    );

    // 5. Playwright Verification (Browser Loading)
    // Initialize Playwright
    let playwright = Playwright::launch().await?;

    let chromium = playwright.chromium();
    let browser = chromium.launch().await?;
    let page = browser.new_page().await?;

    // Open the generated file
    let url = format!(
        "file://{}",
        output_html_path.canonicalize()?.to_string_lossy()
    );
    page.goto(&url, None).await?;

    // Verify Title
    let title = page.title().await?;
    assert_eq!(title, "Example Ontology");

    // Verify "Person" class exists and is visible
    let person_header = page.locator("#Person h3").await;
    assert_eq!(
        person_header.text_content().await?,
        Some("Person".to_string())
    );

    Ok(())
}
