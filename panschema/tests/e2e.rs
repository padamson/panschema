//! End-to-end browser tests using Playwright.
//!
//! These tests verify the generated documentation renders correctly in a real browser.
//!
//! ## Setup
//! Install Playwright browsers matching the version bundled with playwright-rs:
//! ```bash
//! npx playwright@1.56.1 install
//! ```
//!
//! The required version is exposed as [`playwright_rs::PLAYWRIGHT_VERSION`].
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
    let output_dir = std::env::temp_dir().join(format!("panschema_e2e_{}", std::process::id()));
    let _ = fs::remove_dir_all(&output_dir);

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "--input",
            "tests/fixtures/reference.ttl",
            "--output",
            output_dir.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute panschema");

    assert!(status.success(), "panschema failed to generate docs");
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
        title.contains("panschema Reference Ontology"),
        "[{}] Page title should contain ontology name, got: {}",
        browser_name,
        title
    );

    // 3. Verify sidebar is present
    let sidebar = page.locator(".sidebar").await;
    let sidebar_count = sidebar.count().await.expect("Failed to count sidebars");
    assert!(
        sidebar_count > 0,
        "[{}] Sidebar should be present",
        browser_name
    );

    // 5. Verify metadata card shows IRI and version
    let page_content = page.content().await.expect("Failed to get page content");
    assert!(
        page_content.contains("http://example.org/panschema/reference"),
        "[{}] Page should display ontology IRI",
        browser_name
    );
    assert!(
        page_content.contains("0.2.0"),
        "[{}] Page should display version",
        browser_name
    );

    // 6. Verify classes are extracted and displayed (not empty)
    // The section header should show count of 5 (Animal, Cat, Dog, Mammal, Person)
    let class_section = page.locator("#classes").await;
    let class_section_html = class_section
        .inner_html()
        .await
        .expect("Failed to get classes section");
    assert!(
        class_section_html.contains(">5<"),
        "[{}] Classes section should show count of 5, got: {}",
        browser_name,
        class_section_html
    );

    // Verify some class links are present
    let class_links = page.locator(".class-link").await;
    let class_link_count = class_links
        .count()
        .await
        .expect("Failed to count class links");
    assert_eq!(
        class_link_count, 5,
        "[{}] Should have 5 class links",
        browser_name
    );

    // Verify specific classes are present
    assert!(
        class_section_html.contains("Animal"),
        "[{}] Classes section should contain 'Animal'",
        browser_name
    );
    assert!(
        class_section_html.contains("Dog"),
        "[{}] Classes section should contain 'Dog'",
        browser_name
    );

    // 6b. Verify class cards are rendered with full content
    let class_cards = page.locator(".class-card").await;
    let class_card_count = class_cards
        .count()
        .await
        .expect("Failed to count class cards");
    assert_eq!(
        class_card_count, 5,
        "[{}] Should have 5 class cards",
        browser_name
    );

    // Verify class card content: Dog should show description
    let dog_card = page.locator("#class-Dog").await;
    let dog_card_html = dog_card.inner_html().await.expect("Failed to get Dog card");
    assert!(
        dog_card_html.contains("A domesticated carnivorous mammal"),
        "[{}] Dog card should show description, got: {}",
        browser_name,
        dog_card_html
    );

    // Verify class card shows IRI
    assert!(
        dog_card_html.contains("http://example.org/panschema/reference#Dog"),
        "[{}] Dog card should show IRI",
        browser_name
    );

    // 6c. Verify class hierarchy relationships are displayed
    // Dog should show "Subclass of" Mammal
    assert!(
        dog_card_html.contains("Subclass of"),
        "[{}] Dog card should show 'Subclass of'",
        browser_name
    );
    assert!(
        dog_card_html.contains("href=\"#class-Mammal\""),
        "[{}] Dog card should link to Mammal as superclass",
        browser_name
    );

    // Mammal should show "Superclass of" (Dog and Cat)
    let mammal_card = page.locator("#class-Mammal").await;
    let mammal_card_html = mammal_card
        .inner_html()
        .await
        .expect("Failed to get Mammal card");
    assert!(
        mammal_card_html.contains("Superclass of"),
        "[{}] Mammal card should show 'Superclass of'",
        browser_name
    );
    assert!(
        mammal_card_html.contains("href=\"#class-Dog\""),
        "[{}] Mammal card should link to Dog as subclass",
        browser_name
    );

    // Animal should show "Superclass of" Mammal (root class)
    let animal_card = page.locator("#class-Animal").await;
    let animal_card_html = animal_card
        .inner_html()
        .await
        .expect("Failed to get Animal card");
    assert!(
        animal_card_html.contains("Superclass of"),
        "[{}] Animal card should show 'Superclass of'",
        browser_name
    );

    // Person should NOT show "Subclass of" (it's a root class)
    let person_card = page.locator("#class-Person").await;
    let person_card_html = person_card
        .inner_html()
        .await
        .expect("Failed to get Person card");
    assert!(
        !person_card_html.contains("Subclass of"),
        "[{}] Person card should not show 'Subclass of' (it's a root class)",
        browser_name
    );

    // 6d. Verify properties are extracted and displayed
    let prop_section = page.locator("#properties").await;
    let prop_section_html = prop_section
        .inner_html()
        .await
        .expect("Failed to get properties section");
    assert!(
        prop_section_html.contains(">4<"),
        "[{}] Properties section should show count of 4, got: {}",
        browser_name,
        prop_section_html
    );

    // Verify property links are present
    let prop_links = page.locator(".prop-link").await;
    let prop_link_count = prop_links
        .count()
        .await
        .expect("Failed to count property links");
    assert_eq!(
        prop_link_count, 4,
        "[{}] Should have 4 property links",
        browser_name
    );

    // 6e. Verify property cards are rendered with full content
    let prop_cards = page.locator(".property-card").await;
    let prop_card_count = prop_cards
        .count()
        .await
        .expect("Failed to count property cards");
    assert_eq!(
        prop_card_count, 4,
        "[{}] Should have 4 property cards",
        browser_name
    );

    // Verify object property card: hasOwner
    let has_owner_card = page.locator("#prop-hasOwner").await;
    let has_owner_html = has_owner_card
        .inner_html()
        .await
        .expect("Failed to get hasOwner card");
    assert!(
        has_owner_html.contains("Object Property"),
        "[{}] hasOwner should show Object Property badge",
        browser_name
    );
    assert!(
        has_owner_html.contains("Relates an animal to its owner"),
        "[{}] hasOwner should show description",
        browser_name
    );
    assert!(
        has_owner_html.contains("Domain"),
        "[{}] hasOwner should show Domain",
        browser_name
    );
    assert!(
        has_owner_html.contains("href=\"#class-Animal\""),
        "[{}] hasOwner domain should link to Animal",
        browser_name
    );
    assert!(
        has_owner_html.contains("Range"),
        "[{}] hasOwner should show Range",
        browser_name
    );
    assert!(
        has_owner_html.contains("href=\"#class-Person\""),
        "[{}] hasOwner range should link to Person",
        browser_name
    );

    // Verify datatype property card: hasAge
    let has_age_card = page.locator("#prop-hasAge").await;
    let has_age_html = has_age_card
        .inner_html()
        .await
        .expect("Failed to get hasAge card");
    assert!(
        has_age_html.contains("Datatype Property"),
        "[{}] hasAge should show Datatype Property badge",
        browser_name
    );
    assert!(
        has_age_html.contains("integer"),
        "[{}] hasAge range should show integer datatype",
        browser_name
    );

    // Verify inverse property: owns shows inverseOf characteristic
    let owns_card = page.locator("#prop-owns").await;
    let owns_html = owns_card
        .inner_html()
        .await
        .expect("Failed to get owns card");
    assert!(
        owns_html.contains("Inverse of: has owner"),
        "[{}] owns should show inverse of characteristic",
        browser_name
    );

    // 6f. Verify individuals are extracted and displayed
    let ind_section = page.locator("#individuals").await;
    let ind_section_html = ind_section
        .inner_html()
        .await
        .expect("Failed to get individuals section");
    assert!(
        ind_section_html.contains(">1<"),
        "[{}] Individuals section should show count of 1, got: {}",
        browser_name,
        ind_section_html
    );

    // Verify individual links are present
    let ind_links = page.locator(".individual-link").await;
    let ind_link_count = ind_links
        .count()
        .await
        .expect("Failed to count individual links");
    assert_eq!(
        ind_link_count, 1,
        "[{}] Should have 1 individual link",
        browser_name
    );

    // Verify individual cards are rendered
    let ind_cards = page.locator(".individual-card").await;
    let ind_card_count = ind_cards
        .count()
        .await
        .expect("Failed to count individual cards");
    assert_eq!(
        ind_card_count, 1,
        "[{}] Should have 1 individual card",
        browser_name
    );

    // Verify individual card content: fido
    let fido_card = page.locator("#ind-fido").await;
    let fido_card_html = fido_card
        .inner_html()
        .await
        .expect("Failed to get fido card");
    assert!(
        fido_card_html.contains("Individual"),
        "[{}] Fido card should show Individual badge",
        browser_name
    );
    assert!(
        fido_card_html.contains("Fido"),
        "[{}] Fido card should show label 'Fido'",
        browser_name
    );
    assert!(
        fido_card_html.contains("href=\"#class-Dog\""),
        "[{}] Fido card should link to Dog class as type",
        browser_name
    );
    assert!(
        fido_card_html.contains("has name"),
        "[{}] Fido card should show 'has name' property",
        browser_name
    );
    assert!(
        fido_card_html.contains("has age"),
        "[{}] Fido card should show 'has age' property",
        browser_name
    );

    // Verify sidebar has individuals link
    let ind_sidebar_link = page.locator(".sidebar-link[href='#individuals']").await;
    let ind_sidebar_count = ind_sidebar_link
        .count()
        .await
        .expect("Failed to count individuals sidebar link");
    assert!(
        ind_sidebar_count > 0,
        "[{}] Individuals navigation link should exist in sidebar",
        browser_name
    );

    // 7. Test sidebar navigation links exist and are clickable
    let classes_link = page.locator(".sidebar-link[href='#classes']").await;
    let link_count = classes_link.count().await.expect("Failed to count links");
    assert!(
        link_count > 0,
        "[{}] Classes navigation link should exist in sidebar",
        browser_name
    );

    // Verify link is clickable (doesn't throw)
    classes_link
        .click(None)
        .await
        .expect("Failed to click classes link");

    // Wait for URL hash to update (page.url() now reflects hash changes in 0.8.3)
    let mut url_updated = false;
    for _ in 0..20 {
        // Poll for up to 2 seconds
        let current_url = page.url();
        if current_url.contains("#classes") {
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

    // 7b. Verify scroll spy: after scrolling to #classes, the "Classes" sidebar
    //     link should be active and "Overview" should not.
    let mut scroll_spy_updated = false;
    for _ in 0..30 {
        let classes_active = page
            .evaluate_value(
                "document.querySelector('.sidebar-link[href=\"#classes\"]')?.classList.contains('active') ?? false",
            )
            .await
            .unwrap_or_default();
        if classes_active.contains("true") {
            scroll_spy_updated = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        scroll_spy_updated,
        "[{}] Scroll spy should mark Classes sidebar link as active after scrolling to #classes",
        browser_name
    );

    // Metadata should no longer be active
    let metadata_active = page
        .evaluate_value(
            "document.querySelector('.sidebar-link[href=\"#metadata\"]')?.classList.contains('active') ?? false",
        )
        .await
        .unwrap_or_default();
    assert!(
        !metadata_active.contains("true"),
        "[{}] Metadata sidebar link should not be active when viewing #classes",
        browser_name
    );

    // 8. Responsive viewport tests using set_viewport_size()
    // First verify desktop behavior: sidebar visible, mobile toggle hidden
    page.set_viewport_size(playwright_rs::Viewport {
        width: 1280,
        height: 720,
    })
    .await
    .expect("Failed to set desktop viewport");

    let mobile_toggle = page.locator(".mobile-menu-toggle").await;
    let toggle_visible_desktop = mobile_toggle
        .is_visible()
        .await
        .expect("Failed to check toggle visibility");
    assert!(
        !toggle_visible_desktop,
        "[{}] Mobile menu toggle should be hidden on desktop viewport",
        browser_name
    );

    let sidebar = page.locator(".sidebar").await;
    let sidebar_visible_desktop = sidebar
        .is_visible()
        .await
        .expect("Failed to check sidebar visibility");
    assert!(
        sidebar_visible_desktop,
        "[{}] Sidebar should be visible on desktop viewport",
        browser_name
    );

    // 8b. Resize to mobile viewport and verify responsive behavior
    page.set_viewport_size(playwright_rs::Viewport {
        width: 375,
        height: 667,
    })
    .await
    .expect("Failed to set mobile viewport");

    // Give CSS time to respond to viewport change
    tokio::time::sleep(Duration::from_millis(100)).await;

    let toggle_visible_mobile = mobile_toggle
        .is_visible()
        .await
        .expect("Failed to check toggle visibility on mobile");
    assert!(
        toggle_visible_mobile,
        "[{}] Mobile menu toggle should be visible on mobile viewport",
        browser_name
    );

    // 8c. Test mobile menu toggle functionality
    mobile_toggle
        .click(None)
        .await
        .expect("Failed to click mobile menu toggle");

    // Wait for sidebar to become visible after toggle click
    tokio::time::sleep(Duration::from_millis(200)).await;

    let sidebar_visible_after_toggle = sidebar
        .is_visible()
        .await
        .expect("Failed to check sidebar visibility after toggle");
    assert!(
        sidebar_visible_after_toggle,
        "[{}] Sidebar should be visible after clicking mobile menu toggle",
        browser_name
    );

    // === GRAPH VISUALIZATION TESTS ===

    // 9. Verify graph visualization section exists
    let graph_section = page.locator("#graph-visualization").await;
    let graph_section_count = graph_section
        .count()
        .await
        .expect("Failed to count graph section");
    assert!(
        graph_section_count > 0,
        "[{}] Graph visualization section should exist",
        browser_name
    );

    // 10. Verify canvas is present and visible
    let canvas = page.locator("#graph-canvas").await;
    let canvas_count = canvas.count().await.expect("Failed to count canvas");
    assert!(
        canvas_count > 0,
        "[{}] Graph canvas should exist",
        browser_name
    );

    // Wait for canvas to be displayed (static fallback should show it)
    let mut canvas_visible = false;
    for _ in 0..20 {
        let visible = canvas
            .is_visible()
            .await
            .expect("Failed to check canvas visibility");
        if visible {
            canvas_visible = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        canvas_visible,
        "[{}] Graph canvas should become visible",
        browser_name
    );

    // 11. Verify node count badge shows correct count
    let node_count_badge = page.locator("#graph-node-count").await;
    let badge_text = node_count_badge
        .inner_text()
        .await
        .expect("Failed to get node count badge text");
    // Reference ontology should have nodes and edges (format: "X nodes, Y edges")
    assert!(
        badge_text.contains("nodes") && badge_text.contains("edges"),
        "[{}] Node count badge should show nodes and edges count, got: {}",
        browser_name,
        badge_text
    );

    // 12. Verify graph controls are present
    let reset_btn = page.locator("#graph-reset").await;
    let reset_count = reset_btn
        .count()
        .await
        .expect("Failed to count reset button");
    assert!(
        reset_count > 0,
        "[{}] Graph reset button should exist",
        browser_name
    );

    let zoom_in = page.locator("#graph-zoom-in").await;
    let zoom_in_count = zoom_in
        .count()
        .await
        .expect("Failed to count zoom-in button");
    assert!(
        zoom_in_count > 0,
        "[{}] Zoom in button should exist",
        browser_name
    );

    let zoom_out = page.locator("#graph-zoom-out").await;
    let zoom_out_count = zoom_out
        .count()
        .await
        .expect("Failed to count zoom-out button");
    assert!(
        zoom_out_count > 0,
        "[{}] Zoom out button should exist",
        browser_name
    );

    // 13. Verify loading indicator is hidden after initialization
    let loading = page.locator("#graph-loading").await;
    let loading_visible = loading
        .is_visible()
        .await
        .expect("Failed to check loading visibility");
    assert!(
        !loading_visible,
        "[{}] Loading indicator should be hidden after graph initializes",
        browser_name
    );

    // 14. Verify graph data contains node labels
    let has_node_labels = page
        .evaluate_value(
            "window.__PANSCHEMA_GRAPH_DATA__.nodes.every(n => n.label && n.label.length > 0)",
        )
        .await
        .expect("Failed to check node labels");
    assert!(
        has_node_labels.contains("true"),
        "[{}] All nodes should have labels",
        browser_name
    );

    // 15. Verify graph data contains edge types (used for edge labels)
    let has_edge_types = page
        .evaluate_value(
            "window.__PANSCHEMA_GRAPH_DATA__.edges.every(e => e.edge_type && e.edge_type.length > 0)",
        )
        .await
        .expect("Failed to check edge types");
    assert!(
        has_edge_types.contains("true"),
        "[{}] All edges should have edge_type for labeling",
        browser_name
    );

    // 16. Verify specific node labels exist (Animal, Dog, Person are in reference ontology)
    let has_animal_label = page
        .evaluate_value("window.__PANSCHEMA_GRAPH_DATA__.nodes.some(n => n.label === 'Animal')")
        .await
        .expect("Failed to check Animal label");
    assert!(
        has_animal_label.contains("true"),
        "[{}] Should have node with label 'Animal'",
        browser_name
    );

    // 17. Verify edge labels - subclass_of edges exist
    let has_subclass_edges = page
        .evaluate_value(
            "window.__PANSCHEMA_GRAPH_DATA__.edges.some(e => e.edge_type === 'subclass_of')",
        )
        .await
        .expect("Failed to check subclass edges");
    assert!(
        has_subclass_edges.contains("true"),
        "[{}] Should have subclass_of edges",
        browser_name
    );

    // 18. Verify Schema Graph is in sidebar navigation
    let graph_sidebar_link = page
        .locator(".sidebar-link[href='#graph-visualization']")
        .await;
    let graph_sidebar_count = graph_sidebar_link
        .count()
        .await
        .expect("Failed to count graph sidebar link");
    assert!(
        graph_sidebar_count > 0,
        "[{}] Schema Graph navigation link should exist in sidebar",
        browser_name
    );

    // 19. Reset to desktop viewport for interaction tests
    page.set_viewport_size(playwright_rs::Viewport {
        width: 1280,
        height: 720,
    })
    .await
    .expect("Failed to set desktop viewport for graph tests");

    // Give time for viewport change
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Scroll to graph section to ensure buttons are visible
    page.evaluate::<(), ()>(
        "document.getElementById('graph-visualization').scrollIntoView()",
        None,
    )
    .await
    .expect("Failed to scroll to graph section");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 20. Test zoom button interaction - click zoom in and verify no errors
    let zoom_in_btn = page.locator("#graph-zoom-in").await;
    zoom_in_btn
        .click(None)
        .await
        .expect("Failed to click zoom in button");
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify no error overlay appeared after zoom
    let error_overlay = page.locator("#graph-error").await;
    let error_visible = error_overlay
        .is_visible()
        .await
        .expect("Failed to check error visibility");
    assert!(
        !error_visible,
        "[{}] Error overlay should not appear after zoom interaction",
        browser_name
    );

    // 21. Test zoom out button
    let zoom_out_btn = page.locator("#graph-zoom-out").await;
    zoom_out_btn
        .click(None)
        .await
        .expect("Failed to click zoom out button");
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 22. Test reset button
    let reset_button = page.locator("#graph-reset").await;
    reset_button
        .click(None)
        .await
        .expect("Failed to click reset button");
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 23. Verify canvas has non-zero dimensions (was actually rendered)
    let canvas_width = page
        .evaluate_value("document.getElementById('graph-canvas').width")
        .await
        .expect("Failed to get canvas width");
    let canvas_height = page
        .evaluate_value("document.getElementById('graph-canvas').height")
        .await
        .expect("Failed to get canvas height");

    // Canvas dimensions should be positive (not 0)
    assert!(
        !canvas_width.contains("\"0\""),
        "[{}] Canvas should have non-zero width, got: {}",
        browser_name,
        canvas_width
    );
    assert!(
        !canvas_height.contains("\"0\""),
        "[{}] Canvas should have non-zero height, got: {}",
        browser_name,
        canvas_height
    );

    // 24. Verify 3D mode indicator and 2D fallback message elements exist
    // Note: WebGPU is typically not available in headless test environments,
    // and WASM may not load in some CI environments, resulting in static fallback.
    // We verify the elements exist in the DOM.
    let mode_indicator = page.locator("#graph-mode").await;
    let mode_indicator_count = mode_indicator
        .count()
        .await
        .expect("Failed to count 3D mode indicator");
    assert!(
        mode_indicator_count > 0,
        "[{}] 3D mode indicator element should exist",
        browser_name
    );

    let fallback_msg = page.locator("#graph-fallback-msg").await;
    let fallback_msg_count = fallback_msg
        .count()
        .await
        .expect("Failed to count 2D fallback message");
    assert!(
        fallback_msg_count > 0,
        "[{}] 2D fallback message element should exist",
        browser_name
    );

    // Check which mode is active (for logging purposes)
    let mode_visible = mode_indicator
        .is_visible()
        .await
        .expect("Failed to check 3D mode visibility");
    let fallback_visible = fallback_msg
        .is_visible()
        .await
        .expect("Failed to check fallback message visibility");

    // Log which mode is active for debugging
    // Note: In headless testing or without WASM, static fallback may be used
    // which doesn't show either badge
    if mode_visible {
        println!("[{}] 3D WebGPU mode is active", browser_name);
    } else if fallback_visible {
        println!("[{}] 2D Canvas fallback mode is active", browser_name);
    } else {
        println!("[{}] Static fallback mode (WASM not loaded)", browser_name);
    }

    // 25. Test sidebar navigation to Schema Graph section
    graph_sidebar_link
        .click(None)
        .await
        .expect("Failed to click Schema Graph sidebar link");

    // Wait for URL hash to update
    let mut graph_url_updated = false;
    for _ in 0..20 {
        let current_url = page.url();
        if current_url.contains("#graph-visualization") {
            graph_url_updated = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        graph_url_updated,
        "[{}] URL hash should be #graph-visualization after clicking sidebar link",
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
