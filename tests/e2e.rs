//! End-to-end browser tests using Playwright.
//!
//! These tests verify the generated documentation renders correctly in a real browser.
//!
//! ## Setup
//! Install Playwright browsers: `npx playwright install`
//!
//! ## Running
//! - Default (chromium): `cargo nextest run e2e`
//! - Specific browser: `BROWSER=firefox cargo nextest run e2e`
//! - All browsers (CI): `BROWSER=all cargo nextest run e2e`

use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use playwright_rs::Playwright;
use tokio::sync::oneshot;

/// Find an available port for the test server.
fn find_available_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("Failed to bind to port")
        .local_addr()
        .expect("Failed to get local address")
        .port()
}

/// Generate documentation to a temporary directory.
fn generate_docs() -> PathBuf {
    let output_dir = std::env::temp_dir().join(format!("rontodoc_e2e_{}", std::process::id()));
    let _ = fs::remove_dir_all(&output_dir);

    let status = Command::new(env!("CARGO_BIN_EXE_rontodoc"))
        .args([
            "--input",
            "tests/fixtures/reference.ttl",
            "--output",
            output_dir.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute rontodoc");

    assert!(status.success(), "rontodoc failed to generate docs");
    output_dir
}

/// Start a simple HTTP server serving static files.
async fn start_server(output_dir: PathBuf, port: u16, shutdown_rx: oneshot::Receiver<()>) {
    use axum::Router;
    use tower_http::services::ServeDir;

    let app = Router::new().fallback_service(ServeDir::new(output_dir));

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port))
        .await
        .expect("Failed to bind server");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        })
        .await
        .expect("Server error");
}

/// Get browsers to test based on BROWSER env var.
/// - "chromium" (default): just chromium
/// - "firefox": just firefox
/// - "webkit": just webkit
/// - "all": all three browsers
fn get_browsers_to_test() -> Vec<&'static str> {
    match std::env::var("BROWSER").as_deref() {
        Ok("firefox") => vec!["firefox"],
        Ok("webkit") => vec!["webkit"],
        Ok("all") => vec!["chromium", "firefox", "webkit"],
        _ => vec!["chromium"], // default
    }
}

/// Run the happy-path E2E test with a specific browser.
async fn run_happy_path_test(playwright: &Playwright, browser_name: &str, base_url: &str) {
    println!("Testing with browser: {}", browser_name);

    let browser = match browser_name {
        "firefox" => playwright
            .firefox()
            .launch()
            .await
            .expect("Failed to launch Firefox"),
        "webkit" => playwright
            .webkit()
            .launch()
            .await
            .expect("Failed to launch WebKit"),
        _ => playwright
            .chromium()
            .launch()
            .await
            .expect("Failed to launch Chromium"),
    };

    let page = browser.new_page().await.expect("Failed to create page");

    // === HAPPY PATH TEST ===
    // This single test verifies the core user journey through the documentation.

    // 1. Navigate to the index page
    let url = format!("{}/index.html", base_url);
    page.goto(&url, None)
        .await
        .expect("Failed to navigate to index page");

    // 2. Verify page title
    let title = page.title().await.expect("Failed to get page title");
    assert!(
        title.contains("Rontodoc Reference Ontology"),
        "[{}] Page title should contain ontology name, got: {}",
        browser_name,
        title
    );

    // 3. Verify hero section displays ontology info
    let hero_title = page.locator(".hero-title").await;
    let hero_count = hero_title
        .count()
        .await
        .expect("Failed to count hero titles");
    assert!(
        hero_count > 0,
        "[{}] Hero title should be present",
        browser_name
    );

    // 4. Verify sidebar is present
    let sidebar = page.locator(".sidebar").await;
    let sidebar_count = sidebar.count().await.expect("Failed to count sidebars");
    assert!(
        sidebar_count > 0,
        "[{}] Sidebar should be present",
        browser_name
    );

    // 5. Verify metadata card shows IRI and version
    let body = page.locator("body").await;
    let page_content = body.inner_html().await.expect("Failed to get page content");
    assert!(
        page_content.contains("http://example.org/rontodoc/reference"),
        "[{}] Page should display ontology IRI",
        browser_name
    );
    assert!(
        page_content.contains("0.1.0"),
        "[{}] Page should display version",
        browser_name
    );

    // 6. Test navigation links exist and are clickable
    let classes_link = page.locator("a[href='#classes']").await;
    let link_count = classes_link.count().await.expect("Failed to count links");
    assert!(
        link_count > 0,
        "[{}] Classes navigation link should exist",
        browser_name
    );

    // Verify link is clickable (doesn't throw)
    classes_link
        .click(None)
        .await
        .expect("Failed to click classes link");

    // Wait for URL hash to update by checking via JavaScript
    // (playwright-rs page.url() may not reflect hash changes immediately)
    let mut url_updated = false;
    for _ in 0..20 {
        // Poll for up to 2 seconds
        let current_hash = page
            .evaluate_value("window.location.hash")
            .await
            .unwrap_or_default();
        if current_hash.contains("#classes") {
            url_updated = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        url_updated,
        "[{}] URL hash should be #classes after click",
        browser_name
    );

    // Verify classes section exists (the target of the link)
    let classes_section = page.locator("#classes").await;
    let section_count = classes_section
        .count()
        .await
        .expect("Failed to count classes sections");
    assert!(
        section_count > 0,
        "[{}] Classes section should exist as link target",
        browser_name
    );

    // 7. Verify mobile menu toggle element exists (CSS hides it on desktop)
    let mobile_toggle = page.locator(".mobile-menu-toggle").await;
    let toggle_count = mobile_toggle
        .count()
        .await
        .expect("Failed to count mobile toggles");
    assert!(
        toggle_count > 0,
        "[{}] Mobile menu toggle element should exist in DOM",
        browser_name
    );

    // Cleanup
    browser.close().await.expect("Failed to close browser");

    println!("[{}] All checks passed!", browser_name);
}

#[test]
fn e2e_happy_path() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");

    rt.block_on(async {
        // Generate documentation
        let output_dir = generate_docs();
        let port = find_available_port();
        let base_url = format!("http://127.0.0.1:{}", port);

        // Start server
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), port, shutdown_rx));

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Initialize Playwright
        let playwright = Playwright::launch()
            .await
            .expect("Failed to initialize Playwright");

        // Run test for each configured browser
        let browsers = get_browsers_to_test();
        for browser_name in browsers {
            run_happy_path_test(&playwright, browser_name, &base_url).await;
        }

        // Cleanup
        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}
