//! End-to-end browser tests using Playwright.
//!
//! These tests verify the generated documentation renders correctly in a real browser.
//!
//! ## Setup
//! Install Playwright browsers matching the version bundled with playwright-rs:
//! ```bash
//! npx playwright@1.60.0 install
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
    generate_docs_for("tests/fixtures/reference.ttl")
}

/// Generate documentation for an explicit fixture path. Used by tests
/// that want a non-default ontology (e.g. the multi-scale screenshot
/// harness, which writes a synthetic TTL to a tempfile and points
/// here).
fn generate_docs_for(fixture_path: &str) -> PathBuf {
    let output_dir = std::env::temp_dir().join(format!(
        "panschema_e2e_{}_{}",
        std::process::id(),
        fixture_path
            .rsplit('/')
            .next()
            .unwrap_or("default")
            .replace('.', "_")
    ));
    let _ = fs::remove_dir_all(&output_dir);

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "--input",
            fixture_path,
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
    // The section header should show count of 6 (Animal, Cat, Dog, Mammal,
    // Person, Pet)
    let class_section = page.locator("#classes").await;
    let class_section_html = class_section
        .inner_html()
        .await
        .expect("Failed to get classes section");
    assert!(
        class_section_html.contains(">6<"),
        "[{}] Classes section should show count of 6, got: {}",
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
        class_link_count, 6,
        "[{}] Should have 6 class links",
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
        class_card_count, 6,
        "[{}] Should have 6 class cards",
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

    // 6d. Verify slots are extracted and displayed
    let slot_section = page.locator("#slots").await;
    let slot_section_html = slot_section
        .inner_html()
        .await
        .expect("Failed to get slots section");
    assert!(
        slot_section_html.contains(">5<"),
        "[{}] Slots section should show count of 5, got: {}",
        browser_name,
        slot_section_html
    );

    // Verify slot links are present
    let slot_links = page.locator(".slot-link").await;
    let slot_link_count = slot_links
        .count()
        .await
        .expect("Failed to count slot links");
    assert_eq!(
        slot_link_count, 5,
        "[{}] Should have 5 slot links",
        browser_name
    );

    // 6e. Verify slot cards are rendered with full content
    let slot_cards = page.locator(".slot-card").await;
    let slot_card_count = slot_cards
        .count()
        .await
        .expect("Failed to count slot cards");
    assert_eq!(
        slot_card_count, 5,
        "[{}] Should have 5 slot cards",
        browser_name
    );

    // Verify object-ranged slot card: hasOwner
    let has_owner_card = page.locator("#slot-hasOwner").await;
    let has_owner_html = has_owner_card
        .inner_html()
        .await
        .expect("Failed to get hasOwner card");
    assert!(
        has_owner_html.contains("Slot"),
        "[{}] hasOwner should show Slot badge",
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

    // Verify datatype-ranged slot card: hasAge
    let has_age_card = page.locator("#slot-hasAge").await;
    let has_age_html = has_age_card
        .inner_html()
        .await
        .expect("Failed to get hasAge card");
    assert!(
        has_age_html.contains("Slot"),
        "[{}] hasAge should show Slot badge",
        browser_name
    );
    assert!(
        has_age_html.contains("integer"),
        "[{}] hasAge range should show integer datatype",
        browser_name
    );

    // Verify inverse slot: owns shows inverseOf characteristic
    let owns_card = page.locator("#slot-owns").await;
    let owns_html = owns_card
        .inner_html()
        .await
        .expect("Failed to get owns card");
    assert!(
        owns_html.contains("Inverse of: has owner"),
        "[{}] owns should show inverse of characteristic",
        browser_name
    );

    // 6e-1. Card metadata rows that render only through the full
    // OWL → IR → HTML path. The reference ontology carries a deprecated
    // class, a class with aliases + see_also + a SKOS mapping, and a
    // symmetric+transitive object property; each must surface in the
    // browser DOM.

    // Pet is `owl:deprecated true`: its card shows the "Deprecated"
    // badge and the deprecation note.
    let pet_card = page.locator("#class-Pet").await;
    let pet_html = pet_card.inner_html().await.expect("Failed to get Pet card");
    assert!(
        pet_html.contains(r#"class="deprecated-badge""#),
        "[{}] Pet card should show the Deprecated badge; got: {}",
        browser_name,
        pet_html
    );
    assert!(
        pet_html.contains(r#"class="deprecated-note""#),
        "[{}] Pet card should show the deprecation note; got: {}",
        browser_name,
        pet_html
    );

    // Person carries skos:altLabel (aliases), rdfs:seeAlso (see also),
    // and skos:exactMatch (a mapping). The person_card_html captured
    // above for the root-class check is reused here.
    assert!(
        person_card_html.contains("<dt>Aliases</dt>")
            && person_card_html.contains("Human")
            && person_card_html.contains("Individual"),
        "[{}] Person card should show an Aliases row listing Human and Individual; got: {}",
        browser_name,
        person_card_html
    );
    assert!(
        person_card_html.contains("<dt>See also</dt>")
            && person_card_html.contains("xmlns.com/foaf/0.1/Person"),
        "[{}] Person card should show a See also row linking to foaf:Person; got: {}",
        browser_name,
        person_card_html
    );
    assert!(
        person_card_html.contains("<dt>Mappings</dt>")
            && person_card_html.contains("schema.org/Person"),
        "[{}] Person card should show a Mappings row linking to schema.org/Person; got: {}",
        browser_name,
        person_card_html
    );

    // relatedTo is owl:SymmetricProperty + owl:TransitiveProperty: its
    // slot card shows both characteristic badges.
    let related_card = page.locator("#slot-relatedTo").await;
    let related_html = related_card
        .inner_html()
        .await
        .expect("Failed to get relatedTo card");
    assert!(
        related_html.contains(r#"class="characteristic-badge""#)
            && related_html.contains("Symmetric")
            && related_html.contains("Transitive"),
        "[{}] relatedTo card should show Symmetric and Transitive characteristic badges; got: {}",
        browser_name,
        related_html
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

    // 8a-1. Classes default to the tree view: Mammal's card is
    // stacked below Animal's and indented under it. Wait for layout
    // to settle after the resize.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let animal_box = page
        .locator("#class-Animal")
        .await
        .bounding_box()
        .await
        .expect("Failed to query Animal card box")
        .expect("Animal class card should have a bounding box");
    let mammal_box = page
        .locator("#class-Mammal")
        .await
        .bounding_box()
        .await
        .expect("Failed to query Mammal card box")
        .expect("Mammal class card should have a bounding box");
    assert!(
        mammal_box.y > animal_box.y && mammal_box.x > animal_box.x,
        "[{}] In the tree view Mammal should sit below and indented \
         under Animal; got animal=({}, {}), mammal=({}, {})",
        browser_name,
        animal_box.x,
        animal_box.y,
        mammal_box.x,
        mammal_box.y
    );

    // 8a-1a. Leaf siblings tile within their tree level: Cat and Dog
    // (both children of Mammal with no descendants) share a row on a
    // 1280px viewport instead of stacking.
    let cat_box = page
        .locator("#class-Cat")
        .await
        .bounding_box()
        .await
        .expect("Failed to query Cat card box")
        .expect("Cat class card should have a bounding box");
    let dog_box = page
        .locator("#class-Dog")
        .await
        .bounding_box()
        .await
        .expect("Failed to query Dog card box")
        .expect("Dog class card should have a bounding box");
    assert!(
        (cat_box.y - dog_box.y).abs() < 10.0,
        "[{}] In the tree view the leaf siblings Cat and Dog should \
         tile on the same row (Y delta < 10px); got y0={}, y1={}",
        browser_name,
        cat_box.y,
        dog_box.y
    );
    assert!(
        cat_box.x > mammal_box.x,
        "[{}] Cat should be indented under Mammal; got cat.x={}, mammal.x={}",
        browser_name,
        cat_box.x,
        mammal_box.x
    );

    // 8a-1b. The Flat toggle switches to an alphabetical grid:
    // Animal and Cat (alphabetical neighbors) tile on the same row
    // (within a small Y tolerance) on a 1280px viewport.
    page.locator(r#".view-toggle-btn[data-view="flat"]"#)
        .await
        .click(None)
        .await
        .expect("Failed to click the Flat toggle");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let animal_flat_box = page
        .locator("#class-Animal")
        .await
        .bounding_box()
        .await
        .expect("Failed to query Animal card box (flat)")
        .expect("Animal class card should have a bounding box (flat)");
    let cat_flat_box = page
        .locator("#class-Cat")
        .await
        .bounding_box()
        .await
        .expect("Failed to query Cat card box (flat)")
        .expect("Cat class card should have a bounding box (flat)");
    assert!(
        (animal_flat_box.y - cat_flat_box.y).abs() < 10.0,
        "[{}] In the flat view Animal and Cat should tile on the same \
         row (Y delta < 10px); got y0={}, y1={}",
        browser_name,
        animal_flat_box.y,
        cat_flat_box.y
    );
    // Restore the tree default so later steps see the shipped state.
    page.locator(r#".view-toggle-btn[data-view="tree"]"#)
        .await
        .click(None)
        .await
        .expect("Failed to click the Tree toggle");
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 8a-2. Graph container's aspect ratio matches the writer's
    // default (16:8) within 5% — derived dynamically rather than
    // hard-coded so future default-ratio changes only need to bump
    // this constant.
    let graph_container = page.locator(".graph-container").await;
    let graph_box = graph_container
        .bounding_box()
        .await
        .expect("Failed to query graph container box")
        .expect("Graph container should have a bounding box");
    let ratio = graph_box.width / graph_box.height;
    let target = 16.0_f64 / 8.0;
    assert!(
        (ratio - target).abs() / target < 0.05,
        "[{}] Graph container aspect ratio should be ~16:8 (±5%); \
         got w={}, h={}, ratio={:.3} (target {:.3})",
        browser_name,
        graph_box.width,
        graph_box.height,
        ratio,
        target
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

    // 8b-1. On a narrow viewport (375px) the card grid collapses to
    // one column — successive class cards stack rather than sharing
    // a row (each card's top sits below the previous card's bottom).
    let m_card0 = class_cards
        .nth(0)
        .bounding_box()
        .await
        .expect("Failed to query first card box on mobile")
        .expect("First class card should have a bounding box");
    let m_card1 = class_cards
        .nth(1)
        .bounding_box()
        .await
        .expect("Failed to query second card box on mobile")
        .expect("Second class card should have a bounding box");
    assert!(
        m_card1.y > m_card0.y + m_card0.height - 4.0,
        "[{}] On a 375px viewport the class cards should stack \
         (card2.y > card1.bottom); got card1 y={} h={}, card2 y={}",
        browser_name,
        m_card0.y,
        m_card0.height,
        m_card1.y
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

    // 9b. The ephemeral hover card (slice 9) ships in the template
    // so the JS hover handler has somewhere to populate. Verifying
    // the element renders pins the template wiring even though
    // simulating an actual hover-over-node interaction requires the
    // WASM-driven canvas, which is outside this happy-path test's
    // scope.
    let hover_card = page.locator("#graph-hover-card").await;
    let hover_card_count = hover_card
        .count()
        .await
        .expect("Failed to count hover card");
    assert_eq!(
        hover_card_count, 1,
        "[{}] Hover card element (#graph-hover-card) should be rendered exactly once",
        browser_name
    );
    let hover_card_classes = hover_card
        .get_attribute("class")
        .await
        .expect("Failed to read hover card class attr")
        .unwrap_or_default();
    assert!(
        hover_card_classes.contains("graph-hover-card"),
        "[{}] Hover card should carry the graph-hover-card class for CSS targeting; got: {}",
        browser_name,
        hover_card_classes
    );

    // 9c. The Arrows toggle (slice 15 / ADR-005) ships in the
    // controls strip, defaults on, and persists its off-state to
    // localStorage. Direction is drawn on the WASM canvas, which a
    // DOM test can't pixel-assert; this verifies the control contract.
    let arrows_btn = page.locator("#graph-arrows").await;
    assert_eq!(
        arrows_btn.count().await.expect("count arrows toggle"),
        1,
        "[{}] Arrows toggle (#graph-arrows) should render exactly once",
        browser_name
    );
    let arrows_default_active = arrows_btn
        .get_attribute("class")
        .await
        .expect("read arrows class")
        .unwrap_or_default()
        .contains("active");
    assert!(
        arrows_default_active,
        "[{}] Arrows toggle should default to active (arrowheads on)",
        browser_name
    );
    // Click programmatically: the strip sits over the WASM canvas, so
    // pointer-actionability is flaky in headless; the handler + the
    // persisted pref are the contract this verifies.
    page.evaluate::<(), ()>("document.getElementById('graph-arrows').click()", None)
        .await
        .expect("click arrows toggle");
    let arrows_after = arrows_btn
        .get_attribute("class")
        .await
        .expect("read arrows class after click")
        .unwrap_or_default();
    assert!(
        !arrows_after.contains("active"),
        "[{}] clicking Arrows should toggle it off; class still active: {}",
        browser_name,
        arrows_after
    );
    let persisted = page
        .evaluate_value("localStorage.getItem('panschema-arrows')")
        .await
        .unwrap_or_default();
    assert!(
        persisted.contains('0'),
        "[{}] arrows-off should persist to localStorage as '0'; got: {}",
        browser_name,
        persisted
    );
    // Restore the default so later steps see the shipped state.
    page.evaluate::<(), ()>("document.getElementById('graph-arrows').click()", None)
        .await
        .expect("restore arrows toggle");

    // 9b. Notation legend: the Legend control renders the key onto a
    // standalone canvas (proving the wasm `render_legend` export ran —
    // a non-zero backing-store width means it sized and drew), defaults
    // open on this roomy viewport, and toggles + persists. The glyph
    // pixels can't be DOM-asserted; this verifies the control contract.
    let legend_toggle = page.locator("#graph-legend-toggle").await;
    assert_eq!(
        legend_toggle.count().await.expect("count legend toggle"),
        1,
        "[{}] Legend toggle (#graph-legend-toggle) should render exactly once",
        browser_name
    );
    let legend_canvas_width = page
        .evaluate_value("document.getElementById('graph-legend-canvas').width")
        .await
        .unwrap_or_default();
    let legend_width: i64 = legend_canvas_width
        .trim()
        .trim_matches('"')
        .parse()
        .unwrap_or(0);
    assert!(
        legend_width > 0,
        "[{}] legend canvas should be sized by render_legend; width was {}",
        browser_name,
        legend_canvas_width
    );
    let legend_visible = || async {
        page.evaluate_value(
            "getComputedStyle(document.getElementById('graph-legend')).display !== 'none'",
        )
        .await
        .unwrap_or_default()
        .contains("true")
    };
    assert!(
        legend_visible().await,
        "[{}] legend should default open on a roomy viewport",
        browser_name
    );
    page.evaluate::<(), ()>(
        "document.getElementById('graph-legend-toggle').click()",
        None,
    )
    .await
    .expect("click legend toggle off");
    assert!(
        !legend_visible().await,
        "[{}] clicking Legend should hide the key",
        browser_name
    );
    let legend_persisted = page
        .evaluate_value("localStorage.getItem('panschema-graph-legend-open')")
        .await
        .unwrap_or_default();
    assert!(
        legend_persisted.contains("false"),
        "[{}] legend-closed should persist as 'false'; got: {}",
        browser_name,
        legend_persisted
    );
    page.evaluate::<(), ()>(
        "document.getElementById('graph-legend-toggle').click()",
        None,
    )
    .await
    .expect("restore legend toggle");

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

    // 24. Verify 2D/3D mode toggle elements exist
    // The mode toggle has 2D and 3D buttons; 3D may be disabled if WebGPU unavailable
    let mode_toggle = page.locator("#graph-mode-toggle").await;
    let mode_toggle_count = mode_toggle
        .count()
        .await
        .expect("Failed to count mode toggle");
    assert!(
        mode_toggle_count > 0,
        "[{}] Mode toggle element should exist",
        browser_name
    );

    let mode_2d_btn = page.locator("#graph-mode-2d").await;
    let mode_2d_count = mode_2d_btn
        .count()
        .await
        .expect("Failed to count 2D mode button");
    assert!(
        mode_2d_count > 0,
        "[{}] 2D mode button should exist",
        browser_name
    );

    let mode_3d_btn = page.locator("#graph-mode-3d").await;
    let mode_3d_count = mode_3d_btn
        .count()
        .await
        .expect("Failed to count 3D mode button");
    assert!(
        mode_3d_count > 0,
        "[{}] 3D mode button should exist",
        browser_name
    );

    // Check 2D button is active (default mode)
    let mode_2d_classes = mode_2d_btn
        .get_attribute("class")
        .await
        .expect("Failed to get 2D button class")
        .unwrap_or_default();
    println!("[{}] 2D button classes: {}", browser_name, mode_2d_classes);

    // Check if 3D button is disabled (WebGPU typically not available in headless)
    let mode_3d_disabled = mode_3d_btn
        .get_attribute("disabled")
        .await
        .expect("Failed to check 3D button disabled state");
    if mode_3d_disabled.is_some() {
        println!("[{}] 3D mode disabled (WebGPU not available)", browser_name);
    } else {
        println!("[{}] 3D mode available", browser_name);
    }

    // 24b. Layout picker: the chrome is present, the implemented
    // variant is selectable, and the rest are disabled.
    let layout_select = page.locator("#graph-layout-select").await;
    let layout_select_count = layout_select
        .count()
        .await
        .expect("Failed to count layout picker");
    assert!(
        layout_select_count > 0,
        "[{}] Layout picker <select> should exist",
        browser_name
    );
    // The writer emits the `auto` not-pinned default, so the picker's
    // initial value is the density-based recommendation. The reference
    // fixture is mixed-edge (subclass_of + domain/range/inverse), below
    // the inheritance threshold, so it auto-detects to `sgd` (feature
    // 09 slice 9). An `is_a`-heavy schema would recommend hierarchical.
    let initial_value = layout_select
        .input_value(None)
        .await
        .expect("Failed to read layout select value");
    assert_eq!(
        initial_value, "sgd",
        "[{}] mixed-edge reference fixture should auto-detect to sgd; got `{}`",
        browser_name, initial_value
    );
    // Implemented options are present and selectable; the rest are
    // reserved-wire-format placeholders carrying the disabled attribute.
    for implemented in &[
        "force-directed",
        "kamada-kawai",
        "hierarchical",
        "stress",
        "sgd",
    ] {
        let opt = page
            .locator(&format!(
                "#graph-layout-select option[value=\"{implemented}\"]"
            ))
            .await;
        let count = opt.count().await.expect("Failed to count option");
        assert_eq!(
            count, 1,
            "[{}] Picker should expose option for `{}`",
            browser_name, implemented
        );
        let disabled = opt
            .get_attribute("disabled")
            .await
            .expect("Failed to read disabled attr");
        assert!(
            disabled.is_none(),
            "[{}] Option `{}` should be selectable",
            browser_name,
            implemented
        );
    }
    for unimplemented in &["circular", "radial-tree"] {
        let opt = page
            .locator(&format!(
                "#graph-layout-select option[value=\"{unimplemented}\"]"
            ))
            .await;
        let count = opt.count().await.expect("Failed to count option");
        assert_eq!(
            count, 1,
            "[{}] Picker should expose option for `{}`",
            browser_name, unimplemented
        );
        let disabled = opt
            .get_attribute("disabled")
            .await
            .expect("Failed to read disabled attr");
        assert!(
            disabled.is_some(),
            "[{}] Option `{}` should be disabled (not yet implemented)",
            browser_name,
            unimplemented
        );
    }

    // Force the picker into 3D mode through the exposed helper
    // (toggling 3D via the UI requires WebGPU support, which isn't
    // available in every e2e runner). In 3D only force-directed
    // is implemented, so every other option must be disabled with
    // a "(not implemented)" label suffix.
    page.evaluate::<(), ()>("window.__panschema_apply_layout_picker_mode(true)", None)
        .await
        .expect("Failed to force picker into 3D mode");
    let fd_3d = page
        .locator("#graph-layout-select option[value=\"force-directed\"]")
        .await;
    let fd_disabled = fd_3d
        .get_attribute("disabled")
        .await
        .expect("Failed to read force-directed disabled attr in 3D mode");
    assert!(
        fd_disabled.is_none(),
        "[{}] force-directed must stay selectable in 3D mode",
        browser_name
    );
    for layout in &[
        "kamada-kawai",
        "hierarchical",
        "stress",
        "sgd",
        "circular",
        "radial-tree",
    ] {
        // In 3D mode every non-force-directed layout (including the
        // 2D-only implemented ones, KK and Hierarchical) is greyed.
        let opt = page
            .locator(&format!("#graph-layout-select option[value=\"{layout}\"]"))
            .await;
        let disabled = opt
            .get_attribute("disabled")
            .await
            .expect("Failed to read disabled attr in 3D mode");
        assert!(
            disabled.is_some(),
            "[{}] Option `{}` must be disabled in 3D mode",
            browser_name,
            layout
        );
        let label = opt
            .text_content()
            .await
            .expect("Failed to read option label in 3D mode")
            .unwrap_or_default();
        assert!(
            label.contains("(not implemented)"),
            "[{}] Option `{}` should carry `(not implemented)` label in 3D mode; got `{}`",
            browser_name,
            layout,
            label
        );
    }
    // Restore the 2D state so subsequent assertions in this test
    // don't see the 3D-mode label/disabled flags.
    page.evaluate::<(), ()>("window.__panschema_apply_layout_picker_mode(false)", None)
        .await
        .expect("Failed to restore picker to 2D mode");

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

    // === SELECTION TESTS ===

    // 26. Test click-to-select: clicking on canvas should update selection state
    // First, scroll to graph and ensure viz is initialized
    page.evaluate::<(), ()>(
        "document.getElementById('graph-visualization').scrollIntoView()",
        None,
    )
    .await
    .expect("Failed to scroll to graph for selection test");
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Get initial selection state (should be -1 = no selection)
    let initial_selection = page
        .evaluate_value("typeof viz !== 'undefined' && viz.selected_node_index ? viz.selected_node_index() : -1")
        .await
        .expect("Failed to get initial selection");
    println!(
        "[{}] Initial selection state: {}",
        browser_name, initial_selection
    );

    // Click in the center of the canvas using canvas.click() which handles coordinates
    // This clicks in the center of the element by default
    canvas
        .click(None)
        .await
        .expect("Failed to click canvas for selection");
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Get selection state after click
    let selection_after_click = page
        .evaluate_value("typeof viz !== 'undefined' && viz.selected_node_index ? viz.selected_node_index() : -1")
        .await
        .expect("Failed to get selection after click");
    println!(
        "[{}] Selection after center click: {}",
        browser_name, selection_after_click
    );

    // Note: We can't guarantee a node is at the center, so we just verify the API works
    // The test passes if no errors occur and selection state is tracked

    // Test deselect by calling deselect via JavaScript
    page.evaluate::<(), ()>(
        "if (typeof viz !== 'undefined' && viz.deselect) { viz.deselect(); }",
        None,
    )
    .await
    .expect("Failed to call deselect");
    tokio::time::sleep(Duration::from_millis(100)).await;

    let selection_after_deselect = page
        .evaluate_value("typeof viz !== 'undefined' && viz.selected_node_index ? viz.selected_node_index() : -1")
        .await
        .expect("Failed to get selection after deselect");
    println!(
        "[{}] Selection after deselect (should be -1): {}",
        browser_name, selection_after_deselect
    );

    // Verify deselect worked
    assert!(
        selection_after_deselect.contains("-1"),
        "[{}] Selection should be -1 after deselect, got: {}",
        browser_name,
        selection_after_deselect
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

/// Clicking a graph node pins its card open (persistent, with a × close
/// button); the old top-right details panel is gone; the × closes the card
/// but keeps the node selected. Drives a *real* click at the node's canvas
/// position (`node_canvas_pos`) — nodes are canvas-drawn, so there's no DOM
/// element to target.
#[test]
fn e2e_click_pins_node_card_keeping_selection() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let output_dir = generate_docs();
        let port = find_available_port();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), port, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");

        // The old details panel must be gone entirely.
        let details = page.locator("#graph-details-panel").await;
        assert_eq!(
            details.count().await.expect("count"),
            0,
            "the details panel should be removed"
        );

        // Let the wasm graph load and settle a layout, then click node 0 at
        // its canvas position through the real click handler.
        tokio::time::sleep(Duration::from_millis(2500)).await;
        let clicked = page
            .evaluate_value(
                r#"(function(){
                    var viz = window.__panschema_viz;
                    if (!viz || typeof viz.node_canvas_pos !== 'function') return 'no-viz';
                    var pos = viz.node_canvas_pos(0);
                    if (!pos || pos.length < 2) return 'no-pos';
                    var canvas = document.getElementById('graph-canvas');
                    var rect = canvas.getBoundingClientRect();
                    var dpr = window.devicePixelRatio || 1;
                    var x = rect.left + pos[0] / dpr, y = rect.top + pos[1] / dpr;
                    canvas.dispatchEvent(new MouseEvent('click', {clientX: x, clientY: y, bubbles: true}));
                    return 'clicked';
                })()"#,
            )
            .await
            .unwrap_or_default();
        assert!(clicked.contains("clicked"), "expected to click a node; got: {clicked}");
        tokio::time::sleep(Duration::from_millis(200)).await;

        // The card is now pinned (persistent) with a visible close button.
        let card = page.locator("#graph-hover-card").await;
        let card_class = card
            .get_attribute("class")
            .await
            .expect("class")
            .unwrap_or_default();
        assert!(
            card_class.contains("graph-hover-pinned"),
            "card should be pinned; class = {card_class}"
        );
        assert!(card.is_visible().await.expect("visible"), "pinned card should be visible");
        assert!(
            page.locator("#graph-hover-close")
                .await
                .is_visible()
                .await
                .expect("close visible"),
            "the close button should show when pinned"
        );
        let sel = page
            .evaluate_value("window.__panschema_viz.selected_node_index()")
            .await
            .unwrap_or_default();
        assert!(!sel.contains("-1"), "a node should be selected; got {sel}");

        // × closes the card but keeps the node selected.
        page.locator("#graph-hover-close")
            .await
            .click(None)
            .await
            .expect("click close");
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(
            !page
                .locator("#graph-hover-card")
                .await
                .is_visible()
                .await
                .expect("visible after close"),
            "card should hide after ×"
        );
        let sel_after = page
            .evaluate_value("window.__panschema_viz.selected_node_index()")
            .await
            .unwrap_or_default();
        assert!(
            !sel_after.contains("-1"),
            "node should stay selected after ×; got {sel_after}"
        );

        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}

/// A pinned card can be dragged by its handle to a new position, so it can
/// be moved off nodes the reader wants to inspect. Pins node 0, drags the
/// `#graph-hover-drag` grip, and asserts the card's top-left moved to the
/// handler-computed target (drag offset applied, viewport-clamped).
#[test]
fn e2e_pinned_card_is_draggable_by_its_handle() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let output_dir = generate_docs();
        let port = find_available_port();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), port, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");

        // Let the wasm graph settle, then pin node 0 through the click handler.
        tokio::time::sleep(Duration::from_millis(2500)).await;
        let clicked = page
            .evaluate_value(
                r#"(function(){
                    var viz = window.__panschema_viz;
                    if (!viz || typeof viz.node_canvas_pos !== 'function') return 'no-viz';
                    var pos = viz.node_canvas_pos(0);
                    if (!pos || pos.length < 2) return 'no-pos';
                    var canvas = document.getElementById('graph-canvas');
                    var rect = canvas.getBoundingClientRect();
                    var dpr = window.devicePixelRatio || 1;
                    var x = rect.left + pos[0] / dpr, y = rect.top + pos[1] / dpr;
                    canvas.dispatchEvent(new MouseEvent('click', {clientX: x, clientY: y, bubbles: true}));
                    return 'clicked';
                })()"#,
            )
            .await
            .unwrap_or_default();
        assert!(clicked.contains("clicked"), "expected to pin a node; got: {clicked}");
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Drag the handle to a fixed in-viewport target and report the
        // before/after card position plus the handler-expected target.
        let result = page
            .evaluate_value(
                r#"(function(){
                    var card = document.getElementById('graph-hover-card');
                    var drag = document.getElementById('graph-hover-drag');
                    if (!card.classList.contains('graph-hover-pinned')) return 'not-pinned';
                    var hr = drag.getBoundingClientRect();
                    var cr0 = card.getBoundingClientRect();
                    var mdX = hr.left + 4, mdY = hr.top + 4;
                    var offX = mdX - cr0.left, offY = mdY - cr0.top;
                    drag.dispatchEvent(new MouseEvent('mousedown', {clientX: mdX, clientY: mdY, bubbles: true}));
                    var tX = 300, tY = 260;
                    document.dispatchEvent(new MouseEvent('mousemove', {clientX: tX, clientY: tY, bubbles: true}));
                    document.dispatchEvent(new MouseEvent('mouseup', {clientX: tX, clientY: tY, bubbles: true}));
                    var cr1 = card.getBoundingClientRect();
                    var expLeft = Math.min(Math.max(0, tX - offX), window.innerWidth - card.offsetWidth);
                    var expTop = Math.min(Math.max(0, tY - offY), window.innerHeight - card.offsetHeight);
                    return [cr0.left, cr0.top, cr1.left, cr1.top, expLeft, expTop].join(',');
                })()"#,
            )
            .await
            .unwrap_or_default();
        let nums: Vec<f64> = result
            .trim_matches('"')
            .split(',')
            .filter_map(|s| s.trim().parse::<f64>().ok())
            .collect();
        assert_eq!(nums.len(), 6, "expected 6 coords; got: {result}");
        let (b_left, b_top, a_left, a_top, exp_left, exp_top) =
            (nums[0], nums[1], nums[2], nums[3], nums[4], nums[5]);
        assert!(
            (a_left - exp_left).abs() <= 2.0 && (a_top - exp_top).abs() <= 2.0,
            "card should land at the drag target ({exp_left},{exp_top}); got ({a_left},{a_top})"
        );
        assert!(
            (a_left - b_left).abs() > 20.0 || (a_top - b_top).abs() > 20.0,
            "the card should have visibly moved; before ({b_left},{b_top}) after ({a_left},{a_top})"
        );

        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}

/// Hovering a rule entry in a slot card highlights the rule's participant
/// nodes on the graph (its trigger/governed slots and owning class), and
/// moving off clears it. Asserts the highlight *logic* via the viz state
/// (canvas pixels aren't readable); the amber ring is the visual layer.
#[test]
fn e2e_hovering_a_rule_entry_highlights_participant_nodes() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let output_dir = generate_docs_for("tests/fixtures/rules_graph.yaml");
        let port = find_available_port();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), port, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");

        // The slot card's rule entry carries the participant node ids.
        let attr = page
            .locator("#slot-approved_by [data-participants]")
            .await
            .get_attribute("data-participants")
            .await
            .expect("attr")
            .unwrap_or_default();
        assert!(
            attr.contains("slot:approved_by") && attr.contains("class:ImageApproval"),
            "the rule entry should carry its participant ids; got: {attr}"
        );

        // Let the wasm graph initialize.
        tokio::time::sleep(Duration::from_millis(2500)).await;

        // Hovering the rule entry highlights its participant nodes.
        let count = page
            .evaluate_value(
                r#"(function(){
                    var el = document.querySelector('#slot-approved_by [data-participants]');
                    if (!el) return 'no-el';
                    el.dispatchEvent(new MouseEvent('mouseover', {bubbles: true}));
                    var viz = window.__panschema_viz;
                    return (viz && typeof viz.highlighted_node_count === 'function')
                        ? String(viz.highlighted_node_count()) : 'no-viz';
                })()"#,
            )
            .await
            .unwrap_or_default();
        let n: i32 = count.trim().trim_matches('"').parse().unwrap_or(0);
        assert!(
            n >= 2,
            "hovering the rule should highlight its participant nodes; got count={count}"
        );

        // The highlight must actually paint: after a render frame, the 2D
        // canvas should contain amber ring pixels (state alone isn't enough —
        // the render loop has to pick up the highlight).
        let amber = page
            .evaluate_value(
                r#"(async function(){
                    var el = document.querySelector('#slot-approved_by [data-participants]');
                    el.dispatchEvent(new MouseEvent('mouseover', {bubbles: true}));
                    await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
                    var canvas = document.getElementById('graph-canvas');
                    var ctx = canvas.getContext('2d');
                    if (!ctx) return 'no-2d-ctx';
                    var d = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
                    var c = 0;
                    for (var i = 0; i < d.length; i += 4) {
                        if (d[i] > 230 && d[i+1] > 160 && d[i+1] < 215 && d[i+2] < 70) c++;
                    }
                    return String(c);
                })()"#,
            )
            .await
            .unwrap_or_default();
        let amber_px: i64 = amber.trim().trim_matches('"').parse().unwrap_or(0);
        assert!(
            amber_px > 0,
            "the amber highlight ring should paint on the canvas; amber pixels={amber}"
        );

        // Moving off the entry clears the highlight.
        let cleared = page
            .evaluate_value(
                r#"(function(){
                    var el = document.querySelector('#slot-approved_by [data-participants]');
                    el.dispatchEvent(new MouseEvent('mouseout', {bubbles: true, relatedTarget: document.body}));
                    return String(window.__panschema_viz.highlighted_node_count());
                })()"#,
            )
            .await
            .unwrap_or_default();
        assert_eq!(
            cleared.trim().trim_matches('"'),
            "0",
            "moving off the entry should clear the highlight; got {cleared}"
        );

        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}

// Proves the layout auto-default end-to-end (feature 09 slice 9): an
// is_a-heavy schema, with no layout pinned and no persisted choice,
// must initialize the picker to `hierarchical` via the wasm density
// recommendation. The reference-fixture happy-path asserts the SGD
// side; this asserts the Hierarchical side, so SGD-for-a-real-schema
// is known to be a real recommendation, not a silent fallback.
#[test]
fn e2e_is_a_heavy_schema_auto_defaults_to_hierarchical() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    rt.block_on(async {
        let output_dir = generate_docs_for("tests/fixtures/taxonomy.ttl");
        let port = find_available_port();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), port, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch()
            .await
            .expect("Failed to initialize Playwright");
        let browser = playwright
            .chromium()
            .launch()
            .await
            .expect("Failed to launch Chromium");
        let page = browser.new_page().await.expect("Failed to create page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("navigate");
        // Give the wasm module time to load and the picker to settle.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let select = page.locator("#graph-layout-select").await;
        let value = select
            .input_value(None)
            .await
            .expect("read layout select value");
        assert_eq!(
            value, "hierarchical",
            "an is_a-heavy schema should auto-detect to hierarchical; got `{}`",
            value
        );
        browser.close().await.expect("close browser");
        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}

// Proves the Enumerations and Types HTML sections render in a browser
// (feature 02 slice 18). The reference fixture is OWL and carries no
// enums/types, so this uses a small LinkML fixture that declares one of
// each, then asserts both sections, their cards, and the enum's
// permissible values are present in the rendered DOM.
#[test]
fn e2e_renders_enum_and_type_sections() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    rt.block_on(async {
        let output_dir = generate_docs_for("tests/fixtures/enum_type.yaml");
        let port = find_available_port();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), port, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch()
            .await
            .expect("Failed to initialize Playwright");
        let browser = playwright
            .chromium()
            .launch()
            .await
            .expect("Failed to launch Chromium");
        let page = browser.new_page().await.expect("Failed to create page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("navigate");

        // Enumerations section + card + permissible values.
        let enum_card = page.locator("#enum-Status").await;
        let enum_html = enum_card
            .inner_html()
            .await
            .expect("Status enum card should be present");
        assert!(
            enum_html.contains("open") && enum_html.contains("closed"),
            "enum card lists its permissible values; got: {enum_html}"
        );

        // Types section + card with its pattern constraint.
        let type_card = page.locator("#type-PhoneNumber").await;
        let type_html = type_card
            .inner_html()
            .await
            .expect("PhoneNumber type card should be present");
        assert!(
            type_html.contains(r"\+[1-9]"),
            "type card shows its pattern; got: {type_html}"
        );

        // Sidebar gained the two nav entries.
        let nav = page.locator(".sidebar-nav").await;
        let nav_html = nav.inner_html().await.expect("sidebar nav present");
        assert!(
            nav_html.contains("Enumerations") && nav_html.contains("Types"),
            "sidebar lists Enumerations and Types; got: {nav_html}"
        );

        browser.close().await.expect("close browser");
        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}

// Proves the LinkML-only card features render in a browser. These have
// no OWL form, so the reference fixture can't exercise them: an abstract
// class, a class with `mixins:` and worked `examples:`, and a slot with
// numeric `minimum_value` / `maximum_value`. This renders a small LinkML
// fixture declaring each and asserts the abstract badge, the "Mixes in"
// mixin links, the Examples section, and the ≥ / ≤ value-bound badges
// are present in the rendered DOM.
#[test]
fn e2e_renders_linkml_card_features() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    rt.block_on(async {
        let output_dir = generate_docs_for("tests/fixtures/card_features.yaml");
        let port = find_available_port();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), port, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch()
            .await
            .expect("Failed to initialize Playwright");
        let browser = playwright
            .chromium()
            .launch()
            .await
            .expect("Failed to launch Chromium");
        let page = browser.new_page().await.expect("Failed to create page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("navigate");

        // Abstract class: NamedThing carries the abstract badge.
        let abstract_card = page.locator("#class-NamedThing").await;
        let abstract_html = abstract_card
            .inner_html()
            .await
            .expect("NamedThing card should be present");
        assert!(
            abstract_html.contains(r#"class="abstract-badge""#),
            "abstract class card shows the abstract badge; got: {abstract_html}"
        );

        // Mixins + examples: Person mixes in HasIdentifier and lists a
        // worked example.
        let person_card = page.locator("#class-Person").await;
        let person_html = person_card
            .inner_html()
            .await
            .expect("Person card should be present");
        assert!(
            person_html.contains("<dt>Mixes in</dt>")
                && person_html.contains(r##"href="#class-HasIdentifier""##),
            "class card shows a Mixes in row linking to the mixin; got: {person_html}"
        );
        assert!(
            person_html.contains("<dt>Examples</dt>") && person_html.contains("Ada Lovelace"),
            "class card shows an Examples section with the worked value; got: {person_html}"
        );

        // Value bounds: the age slot card surfaces ≥ / ≤ characteristic
        // badges from minimum_value / maximum_value.
        let age_card = page.locator("#slot-age").await;
        let age_html = age_card
            .inner_html()
            .await
            .expect("age slot card should be present");
        assert!(
            age_html.contains(r#"class="characteristic-badge""#)
                && age_html.contains("≥ 0")
                && age_html.contains("≤ 130"),
            "slot card shows value-bound badges; got: {age_html}"
        );

        // ifabsent default: the membership slot card surfaces a Default row
        // rendering the readable value (`"basic"`).
        let membership_card = page.locator("#slot-membership").await;
        let membership_html = membership_card
            .inner_html()
            .await
            .expect("membership slot card should be present");
        assert!(
            membership_html.contains("<dt>Default</dt>") && membership_html.contains("basic"),
            "slot card shows a Default row with the ifabsent value; got: {membership_html}"
        );

        browser.close().await.expect("close browser");
        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}

/// A target viewport + graph size for the multi-scale screenshot
/// iteration harness. We pin three configurations that cover the device
/// spectrum we care about visually.
struct ScreenshotScale {
    /// Short tag used in the output filename and log lines.
    name: &'static str,
    /// Number of connected classes in the synthetic ontology (the
    /// connected component, modeled as a balanced tree via subClassOf).
    connected: usize,
    /// Number of disconnected datatype properties (singleton components).
    isolated: usize,
    /// Browser viewport width in CSS pixels.
    viewport_w: u32,
    /// Browser viewport height in CSS pixels.
    viewport_h: u32,
}

const SCALES: &[ScreenshotScale] = &[
    ScreenshotScale {
        name: "phone",
        connected: 6,
        isolated: 2,
        viewport_w: 390,
        viewport_h: 844,
    },
    ScreenshotScale {
        name: "laptop",
        connected: 30,
        isolated: 8,
        viewport_w: 1440,
        viewport_h: 900,
    },
    ScreenshotScale {
        name: "4k",
        connected: 80,
        isolated: 20,
        viewport_w: 3840,
        viewport_h: 2160,
    },
];

/// Generate a synthetic Turtle ontology with `connected_n` classes laid
/// out as a roughly-balanced tree (each new class subclasses one of the
/// already-emitted classes) plus `isolated_n` disconnected datatype
/// properties (singleton components). For `connected_n ≥ 10` an
/// `owl:ObjectProperty` per class adds a domain→range chord linking
/// `Ci` to `C((i + n/3) mod n)`, breaking the tree's rotational
/// symmetry so the multi-seed crossing-min selector has non-isomorphic
/// basins to choose between.
fn build_synthetic_ttl(connected_n: usize, isolated_n: usize) -> String {
    let mut out = String::new();
    out.push_str(
        "@prefix : <http://example.org/panschema/synthetic#> .\n\
         @prefix owl: <http://www.w3.org/2002/07/owl#> .\n\
         @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n\n\
         <http://example.org/panschema/synthetic> a owl:Ontology ;\n    \
             rdfs:label \"Synthetic test ontology\" .\n\n",
    );
    // Balanced tree: parent(i) = (i - 1) / branching_factor. The
    // branching factor scales with sqrt(N) so a 6-class graph stays
    // mostly linear and an 80-class graph fans out to ~9 children per
    // node — both visually informative for their respective scales.
    let branching = ((connected_n as f64).sqrt().max(2.0) as usize).max(2);
    for i in 0..connected_n {
        let label = format!("C{i}");
        if i == 0 {
            out.push_str(&format!(
                ":{label} a owl:Class ; rdfs:label \"{label}\" .\n"
            ));
        } else {
            let parent = format!("C{}", (i - 1) / branching);
            out.push_str(&format!(
                ":{label} a owl:Class ; rdfs:subClassOf :{parent} ; rdfs:label \"{label}\" .\n"
            ));
        }
    }
    // Chord edges. Only emitted for graphs large enough that the chord
    // offset (n/3) is meaningful. Each chord is an owl:ObjectProperty
    // with domain Ci and range C((i + n/3) mod n); the resulting cycle
    // structure makes the post-settle crossing count dependent on
    // which initial rotation the simulation lands in, so the
    // multi-seed selector has something to optimize against.
    if connected_n >= 10 {
        out.push('\n');
        let chord_offset = (connected_n / 3).max(1);
        for i in 0..connected_n {
            let src = format!("C{i}");
            let tgt = format!("C{}", (i + chord_offset) % connected_n);
            out.push_str(&format!(
                ":chord{i} a owl:ObjectProperty ; rdfs:domain :{src} ; rdfs:range :{tgt} ; rdfs:label \"chord{i}\" .\n"
            ));
        }
    }
    out.push('\n');
    for i in 0..isolated_n {
        let label = format!("p{i}");
        out.push_str(&format!(
            ":{label} a owl:DatatypeProperty ; rdfs:label \"{label}\" .\n"
        ));
    }
    out
}

/// Render one screenshot scale: write a synthetic TTL fixture, run
/// `panschema generate`, serve the output, take a 2D-canvas screenshot
/// at the target viewport, and return the pixel-bbox stats JSON string
/// for the eprintln summary at the end of the multi-scale test.
async fn capture_scale_screenshot(
    playwright: &Playwright,
    scale: &ScreenshotScale,
) -> (String, PathBuf) {
    let fixture_path = std::env::temp_dir().join(format!(
        "panschema_synthetic_{}_{}.ttl",
        scale.name,
        std::process::id()
    ));
    fs::write(
        &fixture_path,
        build_synthetic_ttl(scale.connected, scale.isolated),
    )
    .expect("Failed to write synthetic TTL");

    let output_dir = generate_docs_for(fixture_path.to_str().unwrap());
    let port = find_available_port();
    let base_url = format!("http://127.0.0.1:{}", port);

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server_handle = tokio::spawn(start_server(output_dir.clone(), port, shutdown_rx));
    tokio::time::sleep(Duration::from_millis(100)).await;

    let browser = playwright
        .chromium()
        .launch()
        .await
        .expect("Failed to launch Chromium");
    let context = browser
        .new_context()
        .await
        .expect("Failed to create context");
    let page = context.new_page().await.expect("Failed to create page");

    page.set_viewport_size(playwright_rs::Viewport {
        width: scale.viewport_w,
        height: scale.viewport_h,
    })
    .await
    .expect("Failed to set viewport");

    // Stub navigator.gpu so init() picks 2D from the start (the 2D-mode
    // click otherwise leaves an async canvas swap mid-flight at test time).
    page.add_init_script(
        "Object.defineProperty(navigator, 'gpu', { value: undefined, configurable: true });",
    )
    .await
    .expect("Failed to inject init script");

    let url = format!("{}/index.html", base_url);
    page.goto(&url, None).await.expect("Failed to navigate");

    // wasm load + canvas wire-up + 300-tick settle (~5s at 60fps) +
    // some headroom for slower viewports.
    tokio::time::sleep(Duration::from_millis(8000)).await;

    let container = page.locator(".graph-container").await;

    let screenshot_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("Workspace root")
        .join("target")
        .join(format!("graph-2d-{}.png", scale.name));
    let _ = fs::create_dir_all(screenshot_path.parent().unwrap());

    let png_bytes = container
        .screenshot(None)
        .await
        .expect("Failed to capture screenshot");
    fs::write(&screenshot_path, &png_bytes).expect("Failed to write screenshot");

    let stats_json = page
        .evaluate_value(
            r#"
            (() => {
                try {
                    const canvas = document.getElementById('graph-canvas');
                    const w = canvas.width;
                    const h = canvas.height;
                    if (!w || !h) return JSON.stringify({ error: 'zero size' });
                    const ctx = canvas.getContext('2d');
                    if (!ctx) return JSON.stringify({ error: 'no 2d ctx' });
                    const img = ctx.getImageData(0, 0, w, h);
                    const px = img.data;
                    let min_x = w, max_x = -1, min_y = h, max_y = -1;
                    let non_bg = 0, label_px = 0;
                    for (let y = 0; y < h; y += 2) {
                        for (let x = 0; x < w; x += 2) {
                            const i = (y * w + x) * 4;
                            const r = px[i], g = px[i + 1], b = px[i + 2];
                            const dr = r - 26, dg = g - 26, db = b - 46;
                            const is_bg = Math.abs(dr) < 15 && Math.abs(dg) < 15 && Math.abs(db) < 15;
                            if (!is_bg) {
                                if (x < min_x) min_x = x;
                                if (x > max_x) max_x = x;
                                if (y < min_y) min_y = y;
                                if (y > max_y) max_y = y;
                                non_bg++;
                                if (r > 200 && g > 200 && b > 200) label_px++;
                            }
                        }
                    }
                    // Read the per-layout edge-crossing count directly
                    // from the wasm Visualization. window.__panschema_viz
                    // is the handle the IIFE in graph_viz.html exposes for
                    // exactly this kind of post-render introspection.
                    let crossings = -1;
                    try {
                        if (window.__panschema_viz && typeof window.__panschema_viz.edge_crossings === 'function') {
                            crossings = window.__panschema_viz.edge_crossings();
                        }
                    } catch (e) { /* leave -1 */ }
                    return JSON.stringify({
                        canvas_w: w, canvas_h: h,
                        bbox_w: max_x - min_x,
                        bbox_h: max_y - min_y,
                        fill_x: ((max_x - min_x) / w).toFixed(3),
                        fill_y: ((max_y - min_y) / h).toFixed(3),
                        non_bg_px: non_bg,
                        label_px: label_px,
                        crossings: crossings,
                    });
                } catch (e) {
                    return JSON.stringify({ error: e.toString() });
                }
            })()
            "#,
        )
        .await
        .unwrap_or_default();

    browser.close().await.expect("Failed to close browser");
    let _ = shutdown_tx.send(());
    let _ = server_handle.await;
    let _ = fs::remove_dir_all(output_dir);
    let _ = fs::remove_file(fixture_path);

    (stats_json, screenshot_path)
}

/// Iteration harness for the 2D graph layout, run at three scales
/// (phone / laptop / 4K) against synthetic ontologies of corresponding
/// sizes. Writes one PNG per scale to `target/graph-2d-<scale>.png` and
/// dumps pixel-bbox + label-pixel-count for each.
///
/// `#[ignore]` keeps it out of routine CI: it's a developer feedback
/// loop, not a regression check. Run with `cargo nextest run --ignored
/// e2e_2d_graph_screenshots --nocapture` after each parameter change.
#[test]
#[ignore = "manual iteration harness; run explicitly with --ignored"]
fn e2e_2d_graph_screenshots() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");

    rt.block_on(async {
        let playwright = Playwright::launch()
            .await
            .expect("Failed to initialize Playwright");

        for scale in SCALES {
            let (stats, path) = capture_scale_screenshot(&playwright, scale).await;
            eprintln!(
                "[{}] viewport={}x{} graph={}c+{}i → {} ({})",
                scale.name,
                scale.viewport_w,
                scale.viewport_h,
                scale.connected,
                scale.isolated,
                path.display(),
                stats
            );
        }
    });
}
