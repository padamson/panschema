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

mod mdbook;

use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use playwright_rs::Playwright;
use tokio::sync::oneshot;

/// Find an available port for the test server.
/// Bind an ephemeral port and keep the socket: handing the live listener to
/// the server (instead of a port number to re-bind) means no window where a
/// concurrently starting test can be given the same port and end up serving
/// this test's browser the wrong site.
fn bind_ephemeral() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind to port");
    let port = listener
        .local_addr()
        .expect("Failed to get local address")
        .port();
    (listener, port)
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
            "generate",
            "--schema",
            fixture_path,
            "--output",
            output_dir.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute panschema");

    assert!(status.success(), "panschema failed to generate docs");
    output_dir
}

/// Generate docs from a LinkML schema plus a LinkML instance-data file,
/// rendering the data as the instance graph via `generate --instances`.
fn generate_docs_with_instances(schema_path: &str, instances_path: &str) -> PathBuf {
    let output_dir = std::env::temp_dir().join(format!(
        "panschema_e2e_instances_{}_{}",
        std::process::id(),
        schema_path
            .rsplit('/')
            .next()
            .unwrap_or("default")
            .replace('.', "_")
    ));
    let _ = fs::remove_dir_all(&output_dir);

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            schema_path,
            "--instances",
            instances_path,
            "--output",
            output_dir.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute panschema");

    assert!(
        status.success(),
        "panschema failed to generate docs with instances"
    );
    output_dir
}

/// Generate docs carrying several curated instance graphs, the
/// `generate --instances a --instances b` form behind the in-page selector.
fn generate_docs_with_several_instances(
    schema_path: &str,
    instance_paths: &[&str],
    tag: &str,
) -> PathBuf {
    let output_dir = std::env::temp_dir().join(format!(
        "panschema_e2e_multi_{}_{}",
        std::process::id(),
        tag
    ));
    let _ = fs::remove_dir_all(&output_dir);

    let mut args = vec!["generate", "--schema", schema_path];
    for path in instance_paths {
        args.push("--instances");
        args.push(path);
    }
    args.push("--output");
    args.push(output_dir.to_str().unwrap());

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args(&args)
        .status()
        .expect("Failed to execute panschema");
    assert!(
        status.success(),
        "panschema failed to generate docs with several instance graphs"
    );
    output_dir
}

/// Click an element through the DOM rather than Playwright's
/// actionability machinery. The embedded wasm force-graph can hog the
/// main thread on slow runners, starving the actionability wait
/// (visible/stable/receives-events) until its ~30s timeout even though
/// the element is fine. The trade is explicit: a DOM click fires on a
/// hidden element too, so callers assert presence beforehand and the
/// click's *effect* afterwards; interactions whose visibility is the
/// point should keep a real `locator.click`.
async fn dom_click(page: &playwright_rs::Page, selector: &str) {
    page.evaluate::<(), ()>(
        &format!(
            "document.querySelector({}).click()",
            serde_json::json!(selector)
        ),
        None,
    )
    .await
    .unwrap_or_else(|e| panic!("DOM click on `{selector}` failed: {e:?}"));
}

/// Poll (up to ~12s) until a JS readiness expression is truthy. Robust to
/// variable CI load — e.g. a page that renders both a schema graph and a
/// second instance graph, each loading wasm — where a fixed sleep would
/// race. Returns `true` once ready, `false` if it never became ready.
async fn wait_until_ready(page: &playwright_rs::Page, ready_expr: &str) -> bool {
    let js = format!("(function(){{ return ({ready_expr}) ? 'ready' : 'no'; }})()");
    // Generous window: the suite launches a browser per test, and a page
    // can take many seconds to become interactive under that contention.
    for _ in 0..150 {
        let r = page.evaluate_value(&js).await.unwrap_or_default();
        if r.contains("ready") {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

/// The schema graph's wasm viz is ready when `__panschema_viz` exists and
/// node 0 has a canvas position.
async fn wait_for_graph_viz_ready(page: &playwright_rs::Page) -> bool {
    wait_until_ready(
        page,
        "window.__panschema_viz && typeof window.__panschema_viz.node_canvas_pos === 'function' \
         && window.__panschema_viz.node_canvas_pos(0).length >= 2",
    )
    .await
}

/// Start a simple HTTP server serving static files.
async fn start_server(
    output_dir: PathBuf,
    listener: TcpListener,
    shutdown_rx: oneshot::Receiver<()>,
) {
    use axum::Router;
    use tower_http::services::ServeDir;

    let app = Router::new().fallback_service(ServeDir::new(output_dir));

    listener
        .set_nonblocking(true)
        .expect("Failed to set nonblocking");
    let listener = tokio::net::TcpListener::from_std(listener).expect("Failed to adopt listener");

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
    let sidebar = page.locator(".sidebar");
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
    let class_section = page.locator("#classes");
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
    let class_links = page.locator(".class-link");
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
    let class_cards = page.locator(".class-card");
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
    let dog_card = page.locator("#class-Dog");
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
    let mammal_card = page.locator("#class-Mammal");
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
    let animal_card = page.locator("#class-Animal");
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
    let person_card = page.locator("#class-Person");
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
    let slot_section = page.locator("#slots");
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
    let slot_links = page.locator(".slot-link");
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
    let slot_cards = page.locator(".slot-card");
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
    let has_owner_card = page.locator("#slot-hasOwner");
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
    let has_age_card = page.locator("#slot-hasAge");
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
    let owns_card = page.locator("#slot-owns");
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
    let pet_card = page.locator("#class-Pet");
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
    let related_card = page.locator("#slot-relatedTo");
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

    // 6f. Verify individuals are extracted and displayed. The heading counts
    // the graph — one individual, no assertions between individuals — rather
    // than a bare individual count, so it reads like the schema graph's badge.
    let ind_count = page
        .locator("#instance-graph-count")
        .inner_text()
        .await
        .expect("instance graph count");
    assert_eq!(
        ind_count.trim(),
        "1 / 0",
        "[{}] the instance heading should count nodes and edges, got: {}",
        browser_name,
        ind_count
    );
    let ind_section = page.locator("#individuals");
    let ind_section_html = ind_section
        .inner_html()
        .await
        .expect("Failed to get individuals section");
    assert!(
        ind_section_html.contains("ind-fido"),
        "[{}] Individuals section should render the individual's card, got: {}",
        browser_name,
        ind_section_html
    );

    // Verify individual links are present
    let ind_links = page.locator(".individual-link");
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
    let ind_cards = page.locator(".individual-card");
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
    let fido_card = page.locator("#ind-fido");
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
    let ind_sidebar_link = page.locator(".sidebar-link[href='#individuals']");
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
    let classes_link = page.locator(".sidebar-link[href='#classes']");
    let link_count = classes_link.count().await.expect("Failed to count links");
    assert!(
        link_count > 0,
        "[{}] Classes navigation link should exist in sidebar",
        browser_name
    );

    // Presence is asserted above and the hash poll below verifies the
    // click took effect.
    dom_click(&page, ".sidebar-link[href='#classes']").await;

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
    let classes_section = page.locator("#classes");
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

    let mobile_toggle = page.locator(".mobile-menu-toggle");
    let toggle_visible_desktop = mobile_toggle
        .is_visible()
        .await
        .expect("Failed to check toggle visibility");
    assert!(
        !toggle_visible_desktop,
        "[{}] Mobile menu toggle should be hidden on desktop viewport",
        browser_name
    );

    let sidebar = page.locator(".sidebar");
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
        .bounding_box()
        .await
        .expect("Failed to query Animal card box")
        .expect("Animal class card should have a bounding box");
    let mammal_box = page
        .locator("#class-Mammal")
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
        .bounding_box()
        .await
        .expect("Failed to query Cat card box")
        .expect("Cat class card should have a bounding box");
    let dog_box = page
        .locator("#class-Dog")
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
        .click(None)
        .await
        .expect("Failed to click the Flat toggle");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let animal_flat_box = page
        .locator("#class-Animal")
        .bounding_box()
        .await
        .expect("Failed to query Animal card box (flat)")
        .expect("Animal class card should have a bounding box (flat)");
    let cat_flat_box = page
        .locator("#class-Cat")
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
        .click(None)
        .await
        .expect("Failed to click the Tree toggle");
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 8a-2. Graph container's aspect ratio matches the writer's
    // default (16:8) within 5% — derived dynamically rather than
    // hard-coded so future default-ratio changes only need to bump
    // this constant.
    let graph_container = page.locator(".graph-container");
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
    let graph_section = page.locator("#graph-visualization");
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
    let hover_card = page.locator("#graph-hover-card");
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
    let arrows_btn = page.locator("#graph-arrows");
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
    let legend_toggle = page.locator("#graph-legend-toggle");
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
    let canvas = page.locator("#graph-canvas");
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

    // 11. The graph badge reads `nodes / edges`, the same format every graph
    // count uses, with the spelled-out reading carried as a label.
    let node_count_badge = page.locator("#graph-node-count");
    let badge_text = node_count_badge
        .inner_text()
        .await
        .expect("Failed to get node count badge text");
    let parts: Vec<&str> = badge_text.trim().split(" / ").collect();
    assert!(
        parts.len() == 2 && parts.iter().all(|p| p.parse::<usize>().is_ok()),
        "[{}] the graph badge should read `nodes / edges`, got: {}",
        browser_name,
        badge_text
    );
    let badge_label = node_count_badge
        .get_attribute("aria-label")
        .await
        .unwrap_or_default()
        .unwrap_or_default();
    assert!(
        badge_label.contains("node") && badge_label.contains("edge"),
        "[{}] the badge needs a label saying which number is which, got: {:?}",
        browser_name,
        badge_label
    );

    // 12. Verify graph controls are present
    let reset_btn = page.locator("#graph-reset");
    let reset_count = reset_btn
        .count()
        .await
        .expect("Failed to count reset button");
    assert!(
        reset_count > 0,
        "[{}] Graph reset button should exist",
        browser_name
    );

    let zoom_in = page.locator("#graph-zoom-in");
    let zoom_in_count = zoom_in
        .count()
        .await
        .expect("Failed to count zoom-in button");
    assert!(
        zoom_in_count > 0,
        "[{}] Zoom in button should exist",
        browser_name
    );

    let zoom_out = page.locator("#graph-zoom-out");
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
    let loading = page.locator("#graph-loading");
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
    let graph_sidebar_link = page.locator(".sidebar-link[href='#graph-visualization']");
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
        // Driver 1.62.1+: scrollIntoView() evaluates to a result object
        // ({interrupted: bool}); void keeps the expression unit-shaped.
        "void document.getElementById('graph-visualization').scrollIntoView()",
        None,
    )
    .await
    .expect("Failed to scroll to graph section");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 20. Test zoom button interaction - click zoom in and verify no errors
    let zoom_in_btn = page.locator("#graph-zoom-in");
    zoom_in_btn
        .click(None)
        .await
        .expect("Failed to click zoom in button");
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify no error overlay appeared after zoom
    let error_overlay = page.locator("#graph-error");
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
    let zoom_out_btn = page.locator("#graph-zoom-out");
    zoom_out_btn
        .click(None)
        .await
        .expect("Failed to click zoom out button");
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 22. Test reset button
    let reset_button = page.locator("#graph-reset");
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
    let mode_toggle = page.locator("#graph-mode-toggle");
    let mode_toggle_count = mode_toggle
        .count()
        .await
        .expect("Failed to count mode toggle");
    assert!(
        mode_toggle_count > 0,
        "[{}] Mode toggle element should exist",
        browser_name
    );

    let mode_2d_btn = page.locator("#graph-mode-2d");
    let mode_2d_count = mode_2d_btn
        .count()
        .await
        .expect("Failed to count 2D mode button");
    assert!(
        mode_2d_count > 0,
        "[{}] 2D mode button should exist",
        browser_name
    );

    let mode_3d_btn = page.locator("#graph-mode-3d");
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
    let layout_select = page.locator("#graph-layout-select");
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
        let opt = page.locator(format!(
            "#graph-layout-select option[value=\"{implemented}\"]"
        ));
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
        let opt = page.locator(format!(
            "#graph-layout-select option[value=\"{unimplemented}\"]"
        ));
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
    let fd_3d = page.locator("#graph-layout-select option[value=\"force-directed\"]");
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
        let opt = page.locator(format!("#graph-layout-select option[value=\"{layout}\"]"));
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
        // Driver 1.62.1+: scrollIntoView() evaluates to a result object
        // ({interrupted: bool}); void keeps the expression unit-shaped.
        "void document.getElementById('graph-visualization').scrollIntoView()",
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
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);

        // Start server
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));

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
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");

        // The old details panel must be gone entirely.
        let details = page.locator("#graph-details-panel");
        assert_eq!(
            details.count().await.expect("count"),
            0,
            "the details panel should be removed"
        );

        // Wait for the wasm graph to be interrogable (robust to CI load),
        // then click node 0 at its canvas position through the real handler.
        assert!(
            wait_for_graph_viz_ready(&page).await,
            "schema graph viz never became ready"
        );
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
        let card = page.locator("#graph-hover-card");
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
            .click(None)
            .await
            .expect("click close");
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(
            !page
                .locator("#graph-hover-card")
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
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");

        // Wait for the wasm graph to be interrogable, then pin node 0.
        assert!(
            wait_for_graph_viz_ready(&page).await,
            "schema graph viz never became ready"
        );
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
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
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
            .get_attribute("data-participants")
            .await
            .expect("attr")
            .unwrap_or_default();
        assert!(
            attr.contains("slot:approved_by") && attr.contains("class:ImageApproval"),
            "the rule entry should carry its participant ids; got: {attr}"
        );

        // Poll until the wasm graph is loaded and laid out — a fixed sleep
        // flakes as `no-viz` under CI load.
        assert!(
            wait_for_graph_viz_ready(&page).await,
            "graph viz never became ready"
        );

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

/// Every node a class rule touches — a trigger *or* governed slot, and the
/// class that declares the rule — wears a persistent amber ring on the
/// graph at rest. Asserts the flagged set covers all of them (not just the
/// governed slot) and that the amber ring actually paints in the canvas
/// pixels around a rule node — with no hover active, the only amber is the
/// persistent ring.
#[test]
fn e2e_rule_touched_nodes_draw_a_persistent_amber_ring() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let output_dir = generate_docs_for("tests/fixtures/rules_graph.yaml");
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");

        // Poll until the wasm graph is loaded and laid out — a fixed sleep
        // flakes as `no-viz` under CI load.
        assert!(
            wait_for_graph_viz_ready(&page).await,
            "graph viz never became ready"
        );

        // Assert the governed set resolved and its ring paints: scan the
        // canvas pixels in a box around the governed node for amber. No
        // hover is active, so the only amber is the persistent ring.
        let result = page
            .evaluate_value(
                r#"(async function(){
                    var viz = window.__panschema_viz;
                    if (!viz || typeof viz.rule_node_count !== 'function') return 'no-viz';
                    var count = viz.rule_node_count();
                    var pos = viz.rule_node_canvas_positions();
                    await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
                    var canvas = document.getElementById('graph-canvas');
                    var ctx = canvas.getContext('2d');
                    if (!ctx) return 'no-2d-ctx';
                    var amber = 0;
                    if (pos.length >= 2) {
                        var cx = Math.round(pos[0]), cy = Math.round(pos[1]);
                        var x0 = Math.max(0, cx - 24), y0 = Math.max(0, cy - 24);
                        var w = Math.min(canvas.width - x0, 48), h = Math.min(canvas.height - y0, 48);
                        var d = ctx.getImageData(x0, y0, w, h).data;
                        for (var i = 0; i < d.length; i += 4) {
                            if (d[i] > 230 && d[i+1] > 160 && d[i+1] < 215 && d[i+2] < 70) amber++;
                        }
                    }
                    return count + '|' + amber;
                })()"#,
            )
            .await
            .unwrap_or_default();
        let parts: Vec<i64> = result
            .trim_matches('"')
            .split('|')
            .filter_map(|s| s.trim().parse::<i64>().ok())
            .collect();
        assert_eq!(parts.len(), 2, "expected 'count|amber'; got: {result}");
        // The fixture's one rule touches a trigger slot (`verdict`), a
        // governed slot (`approved_by`), and the owning class — all three
        // ring at rest, not just the governed slot.
        assert!(
            parts[0] >= 3,
            "the rule's trigger slot, governed slot, and class should all be flagged; got count={}",
            parts[0]
        );
        assert!(
            parts[1] > 0,
            "the persistent rule ring should paint amber near the node; amber pixels={}",
            parts[1]
        );

        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}

/// A class grounded via `subclass_of` into an upstream ontology draws a muted
/// external node in the schema graph. Asserts the viz reports a node of type
/// `External` and that its muted grey fill actually paints on the canvas —
/// distinct from the blue class nodes.
#[test]
fn e2e_external_grounding_paints_a_muted_node() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let output_dir = generate_docs_for("tests/fixtures/external_grounding.yaml");
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");

        assert!(
            wait_for_graph_viz_ready(&page).await,
            "graph viz never became ready"
        );

        // Find the external node, then sample the canvas around it for the
        // muted grey fill (roughly equal r/g/b, b highest) — the blue class
        // fill (b ≫ r) can't match, so grey pixels prove the external node
        // itself painted.
        let result = page
            .evaluate_value(
                r#"(async function(){
                    var viz = window.__panschema_viz;
                    if (!viz || typeof viz.node_count !== 'function') return 'no-viz';
                    var n = viz.node_count();
                    var idx = -1;
                    for (var i = 0; i < n; i++) {
                        if (viz.get_node_type(i) === 'External') { idx = i; break; }
                    }
                    if (idx < 0) return 'no-external';
                    var pos = viz.node_canvas_pos(idx);
                    await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
                    var canvas = document.getElementById('graph-canvas');
                    var ctx = canvas.getContext('2d');
                    if (!ctx) return 'no-2d-ctx';
                    var cx = Math.round(pos[0]), cy = Math.round(pos[1]);
                    var x0 = Math.max(0, cx - 24), y0 = Math.max(0, cy - 24);
                    var w = Math.min(canvas.width - x0, 48), h = Math.min(canvas.height - y0, 48);
                    var d = ctx.getImageData(x0, y0, w, h).data;
                    var grey = 0;
                    for (var i = 0; i < d.length; i += 4) {
                        var r = d[i], g = d[i+1], b = d[i+2], a = d[i+3];
                        if (a > 0 && r >= 90 && r <= 205 &&
                            Math.abs(r - g) < 40 && Math.abs(g - b) < 45 && b >= r) grey++;
                    }
                    return 'external|' + grey;
                })()"#,
            )
            .await
            .unwrap_or_default();
        let result = result.trim_matches('"');
        let grey: i64 = result
            .strip_prefix("external|")
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or_else(|| panic!("expected 'external|<count>'; got: {result}"));
        assert!(
            grey > 0,
            "the external grounding node's muted grey fill should paint; grey pixels={grey}"
        );

        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}

/// The "Groundings" control shows only when the graph has external nodes, and
/// clicking it hides them. Asserts the button is visible for a grounded schema
/// and that a click flips external visibility off and clears the muted node's
/// pixels from the canvas.
#[test]
fn e2e_groundings_toggle_hides_external_nodes() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let output_dir = generate_docs_for("tests/fixtures/external_grounding.yaml");
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");

        assert!(
            wait_for_graph_viz_ready(&page).await,
            "graph viz never became ready"
        );

        // The toggle is revealed only when external nodes exist.
        let visible = page
            .evaluate_value(
                r#"(function(){
                    var b = document.getElementById('graph-toggle-external');
                    if (!b) return 'no-button';
                    return getComputedStyle(b).display !== 'none' ? 'shown' : 'hidden';
                })()"#,
            )
            .await
            .unwrap_or_default();
        assert_eq!(
            visible.trim_matches('"'),
            "shown",
            "the Groundings toggle should be visible for a grounded schema"
        );

        // Hover the external node so its label renders regardless of zoom
        // (a hovered label always draws), then toggle groundings off: both the
        // muted fill and the label must vanish, not linger.
        let result = page
            .evaluate_value(
                r#"(async function(){
                    var viz = window.__panschema_viz;
                    if (!viz || typeof viz.node_count !== 'function') return 'no-viz';
                    var idx = -1, n = viz.node_count();
                    for (var i = 0; i < n; i++) {
                        if (viz.get_node_type(i) === 'External') { idx = i; break; }
                    }
                    if (idx < 0) return 'no-external';
                    var canvas = document.getElementById('graph-canvas');
                    var ctx = canvas.getContext('2d');
                    function sample(box, pred){
                        var x0 = Math.max(0, box[0]), y0 = Math.max(0, box[1]);
                        var w = Math.min(canvas.width - x0, box[2]);
                        var h = Math.min(canvas.height - y0, box[3]);
                        if (w <= 0 || h <= 0) return 0;
                        var d = ctx.getImageData(x0, y0, w, h).data, c = 0;
                        for (var i = 0; i < d.length; i += 4) {
                            if (pred(d[i], d[i+1], d[i+2], d[i+3])) c++;
                        }
                        return c;
                    }
                    var isGrey = function(r,g,b,a){ return a>0 && r>=90 && r<=205 &&
                        Math.abs(r-g)<40 && Math.abs(g-b)<45 && b>=r; };
                    // Hovered label text is fully-opaque white on a blue chip;
                    // count the bright text pixels to the right of the node.
                    var isText = function(r,g,b,a){ return a>0 && r>=200 && g>=200 && b>=200; };
                    var raf2 = function(){ return new Promise(r =>
                        requestAnimationFrame(() => requestAnimationFrame(r))); };
                    function labelBox(){
                        var p = viz.node_canvas_pos(idx);
                        return [Math.round(p[0])+6, Math.round(p[1])-12, 160, 24];
                    }
                    function fillBox(){
                        var p = viz.node_canvas_pos(idx);
                        return [Math.round(p[0])-24, Math.round(p[1])-24, 48, 48];
                    }
                    // Frame the graph so the node is on-canvas. Turn the bulk
                    // node labels off (they're zoom-gated and would drop at
                    // this scale) so only a *hovered* node draws its label —
                    // which renders at a readable size regardless of zoom.
                    viz.fit_to_bounds(40);
                    document.getElementById('graph-labels-nodes').click();
                    await raf2();
                    var p = viz.node_canvas_pos(idx);
                    viz.update_hover(p[0], p[1]);
                    if (typeof viz.render === 'function') viz.render();
                    await raf2();
                    var labelBefore = sample(labelBox(), isText);
                    // Toggle groundings off; the node (and its hovered label) go.
                    document.getElementById('graph-toggle-external').click();
                    await raf2();
                    if (viz.is_type_visible('External')) return 'still-visible';
                    var greyAfter = sample(fillBox(), isGrey);
                    var labelAfter = sample(labelBox(), isText);
                    return labelBefore + '|' + greyAfter + '|' + labelAfter;
                })()"#,
            )
            .await
            .unwrap_or_default();
        let result = result.trim_matches('"');
        let parts: Vec<i64> = result
            .split('|')
            .map(|s| {
                s.trim().parse().unwrap_or_else(|_| {
                    panic!("expected 'labelBefore|greyAfter|labelAfter'; got: {result}")
                })
            })
            .collect();
        assert_eq!(parts.len(), 3, "expected three counts; got: {result}");
        assert!(
            parts[0] > 0,
            "the external node's hovered label should paint before toggling off; label pixels={}",
            parts[0]
        );
        assert_eq!(
            parts[1], 0,
            "after toggling groundings off, the external node fill should not paint; grey pixels={}",
            parts[1]
        );
        assert_eq!(
            parts[2], 0,
            "after toggling groundings off, the external node label should not linger; label pixels={}",
            parts[2]
        );

        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}

/// Hovering an external grounding node shows the full IRI and the cached
/// upstream definition, and the legend documents the muted external node.
/// Self-contained: the upstream "cache" is seeded through the label-store
/// API into a temp cache root the CLI is pointed at, and `--offline` keeps
/// generate from fetching — no network.
#[test]
fn e2e_external_node_hover_shows_iri_and_definition_and_legend_documents_it() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        // Seed the label cache the way a prior online run would have.
        let cache_root = std::env::temp_dir().join(format!(
            "panschema_e2e_labelcache_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&cache_root);
        {
            use panschema::labels::{LabelStore, TermInfo};
            let mut store =
                LabelStore::open(cache_root.join("labels")).expect("open label store");
            let mut terms = std::collections::BTreeMap::new();
            terms.insert(
                "https://www.commoncoreontologies.org/ont00000995".to_string(),
                TermInfo {
                    label: Some("Act of Service".to_string()),
                    definitions: vec![
                        "An act in which a service is provided.".to_string(),
                    ],
                },
            );
            store
                .insert_source("https://www.commoncoreontologies.org/", terms)
                .expect("seed label cache");
        }

        let output_dir = std::env::temp_dir().join(format!(
            "panschema_e2e_grounding_hover_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&output_dir);
        let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
            .env("PANSCHEMA_CACHE_ROOT", &cache_root)
            .args([
                "generate",
                "--schema",
                "tests/fixtures/external_grounding.yaml",
                "--output",
                output_dir.to_str().unwrap(),
                "--offline",
            ])
            .status()
            .expect("Failed to execute panschema");
        assert!(status.success(), "panschema failed to generate docs");

        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");

        assert!(
            wait_for_graph_viz_ready(&page).await,
            "graph viz never became ready"
        );

        // Hover the external node with a real mousemove over the canvas so
        // the DOM hover card fills, then read its text.
        let hover_text = page
            .evaluate_value(
                r#"(async function(){
                    var viz = window.__panschema_viz;
                    if (!viz) return 'no-viz';
                    var idx = -1, n = viz.node_count();
                    for (var i = 0; i < n; i++) {
                        if (viz.get_node_type(i) === 'External') { idx = i; break; }
                    }
                    if (idx < 0) return 'no-external';
                    viz.fit_to_bounds(40);
                    await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
                    var pos = viz.node_canvas_pos(idx);
                    var canvas = document.getElementById('graph-canvas');
                    var rect = canvas.getBoundingClientRect();
                    var dpr = window.devicePixelRatio || 1;
                    var x = rect.left + pos[0] / dpr, y = rect.top + pos[1] / dpr;
                    canvas.dispatchEvent(new MouseEvent('mousemove', {clientX: x, clientY: y, bubbles: true}));
                    await new Promise(r => setTimeout(r, 250));
                    var content = document.getElementById('graph-hover-content');
                    return 'HOVER:' + (content ? content.innerText : 'no-content');
                })()"#,
            )
            .await
            .unwrap_or_default();
        for expected in [
            "Act of Service",
            "https://www.commoncoreontologies.org/ont00000995",
            "An act in which a service is provided.",
        ] {
            assert!(
                hover_text.contains(expected),
                "hover card should show {expected:?}; got: {hover_text}"
            );
        }

        // The legend (open by default on a wide viewport) documents the
        // external node: its muted grey swatch at 0.65 alpha over the
        // #1a1a2e canvas blends to ≈(103,107,123) — a color no other
        // legend element produces, so finding it proves the row painted.
        let legend = page
            .evaluate_value(
                r#"(function(){
                    var panel = document.getElementById('graph-legend');
                    if (!panel) return 'no-panel';
                    if (getComputedStyle(panel).display === 'none') {
                        document.getElementById('graph-legend-toggle').click();
                    }
                    var lc = document.getElementById('graph-legend-canvas');
                    var ctx = lc.getContext('2d');
                    var d = ctx.getImageData(0, 0, lc.width, lc.height).data;
                    var hits = 0;
                    for (var i = 0; i < d.length; i += 4) {
                        if (Math.abs(d[i] - 103) <= 8 && Math.abs(d[i+1] - 107) <= 8
                            && Math.abs(d[i+2] - 123) <= 8) hits++;
                    }
                    return 'LEGEND:' + hits;
                })()"#,
            )
            .await
            .unwrap_or_default();
        let hits: i64 = legend
            .trim_matches('"')
            .strip_prefix("LEGEND:")
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or_else(|| panic!("expected 'LEGEND:<count>'; got: {legend}"));
        assert!(
            hits > 0,
            "the legend should paint the external grounding swatch; matching pixels={hits}"
        );

        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
        let _ = fs::remove_dir_all(cache_root);
    });
}

/// A schema with OWL individuals renders a separate instance (A-box) graph
/// beneath the Individuals cards. Asserts the exporter emitted the right
/// A-box (2 individuals, 1 assertion edge), that it embedded into the page,
/// and that its own canvas actually paints the individual nodes (probed
/// as the class-blue pixel band) — a
/// distinct viz from the schema graph.
#[test]
fn e2e_instance_graph_renders_individuals_beneath_the_cards() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let output_dir = generate_docs_for("tests/fixtures/instance_graph.ttl");
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");

        // The instance graph canvas exists — a second, distinct canvas.
        assert_eq!(
            page.locator("#instance-graph-canvas")
                .count()
                .await
                .expect("count"),
            1,
            "the Individuals section should carry an instance-graph canvas"
        );

        // The embedded A-box is exactly what the exporter built.
        let counts = page
            .evaluate_value(
                r#"(function(){
                    var g = window.__PANSCHEMA_INSTANCE_GRAPHS__;
                    var d = g && g[0] && g[0].data;
                    return d ? (d.nodes.length + ',' + d.edges.length) : 'none';
                })()"#,
            )
            .await
            .unwrap_or_default();
        assert_eq!(
            counts.trim().trim_matches('"'),
            "3,1",
            "three individuals + one object-property assertion; got {counts}"
        );

        // Wait for the instance viz to load its (separate) wasm module.
        assert!(
            wait_until_ready(&page, "!!window.__panschema_instance_viz").await,
            "instance graph viz never became ready"
        );

        // The viz initialized and its canvas painted the individual
        // nodes — class-colored per the shared vocabulary, probed as the
        // class-blue band around #4A90D9 — proof the A-box graph actually
        // renders, not just that the data embedded.
        let result = page
            .evaluate_value(
                r#"(async function(){
                    var viz = window.__panschema_instance_viz;
                    if (!viz) return 'no-viz';
                    await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
                    var c = document.getElementById('instance-graph-canvas');
                    var ctx = c.getContext('2d');
                    if (!ctx) return 'no-2d-ctx';
                    var d = ctx.getImageData(0, 0, c.width, c.height).data;
                    var teal = 0;
                    for (var i = 0; i < d.length; i += 4) {
                        if (d[i] < 110 && d[i+1] > 110 && d[i+1] < 180 && d[i+2] > 190) teal++;
                    }
                    return 'ok:' + teal;
                })()"#,
            )
            .await
            .unwrap_or_default();
        let result = result.trim().trim_matches('"').to_string();
        assert!(
            result.starts_with("ok:"),
            "the instance viz should have initialized; got {result}"
        );
        let teal: i64 = result.trim_start_matches("ok:").parse().unwrap_or(0);
        assert!(
            teal > 0,
            "the instance graph should paint individual nodes; class-blue pixels={teal}"
        );

        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}

/// A data-only composition (`html_schema_sections = false`) still boots
/// the instance viz: the graph shell script ships with the page even
/// though the schema sections that normally carry it are omitted, and no
/// schema reference section renders.
#[test]
fn e2e_data_only_composition_boots_the_instance_viz() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        // Composition keys are manifest keys, so this page builds through
        // a minimal consumer manifest around the embedded-individuals
        // fixture.
        let consumer = std::env::temp_dir().join(format!(
            "panschema_e2e_composed_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&consumer);
        fs::create_dir_all(consumer.join("pkg")).expect("mkdir pkg");
        fs::copy(
            "tests/fixtures/instance_graph.ttl",
            consumer.join("pkg/schema.ttl"),
        )
        .expect("copy fixture");
        fs::write(
            consumer.join("pkg/panschema-publish.toml"),
            "[schema]\nname = \"ig\"\nversion = \"0.1.0\"\nlinkml = \"1.7.0\"\n\n[files]\nmain = \"schema.ttl\"\n",
        )
        .expect("write publish toml");
        fs::write(
            consumer.join("panschema.toml"),
            "[schemas]\nig = { path = \"./pkg\" }\n\n[generate.ig]\nhtml = \"docs/\"\nhtml_schema_sections = false\n",
        )
        .expect("write manifest");
        let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
            .arg("generate")
            .current_dir(&consumer)
            .status()
            .expect("run panschema");
        assert!(status.success(), "composed generate failed");
        let output_dir = consumer.join("docs");

        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");

        assert_eq!(
            page.locator("section#classes").count().await.expect("count"),
            0,
            "no schema reference section renders"
        );
        assert_eq!(
            page.locator("#instance-graph-canvas")
                .count()
                .await
                .expect("count"),
            1,
            "the instance canvas renders"
        );
        assert!(
            wait_until_ready(&page, "!!window.PanschemaGraphShell").await,
            "the graph shell script must load on a data-only page"
        );
        assert!(
            wait_until_ready(&page, "!!window.__panschema_instance_viz").await,
            "instance graph viz never became ready on the data-only page"
        );

        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(&consumer);
    });
}

/// Several curated instance graphs share the schema page: the selector names
/// each, and picking one swaps the cards, the provenance, and the rendered
/// graph together. A selector that moved the canvas but left the cards
/// describing the previous dataset is the defect this pins down.
#[test]
fn e2e_instance_dataset_selector_switches_cards_and_graph() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let output_dir = generate_docs_with_several_instances(
            "tests/fixtures/wine_catalog.yaml",
            &[
                "tests/fixtures/wine_instances_preview.yaml",
                "tests/fixtures/wine_instances.yaml",
            ],
            "selector",
        );
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");

        // Both datasets are offered, and the first is the one selected.
        let tabs = page.locator(".instance-dataset-tab");
        assert_eq!(
            tabs.count().await.expect("count"),
            2,
            "each declared dataset needs a selector entry"
        );
        let selected = page
            .locator(".instance-dataset-tab[aria-selected='true']")
            .inner_text()
            .await
            .expect("selected tab text");
        assert!(
            selected.contains("wine_instances_preview"),
            "the first declared dataset starts selected; got: {selected}"
        );

        // The preview's card is visible; the worked example's is not, because
        // its panel is hidden.
        assert!(
            page.locator("#d0-ind-previewWine")
                .is_visible()
                .await
                .unwrap_or(false),
            "the selected dataset's individual card should be visible"
        );
        assert!(
            !page
                .locator("#d1-ind-chateauMorgon")
                .is_visible()
                .await
                .unwrap_or(true),
            "the unselected dataset's cards should be hidden"
        );
        assert_eq!(
            page.locator("#instance-graph-count")
                .inner_text()
                .await
                .expect("heading count")
                .trim(),
            "2 / 1",
            "on load the heading describes the default dataset"
        );

        // Switching: click the second tab. Cards, provenance, and the graph
        // all follow to the worked example. The tabs are wired independently
        // of the wasm viz, so this works without waiting for it.
        page.locator(".instance-dataset-tab[data-instance-dataset='1']")
            .click(None)
            .await
            .expect("click second dataset");

        assert!(
            page.locator("#d1-ind-chateauMorgon")
                .is_visible()
                .await
                .unwrap_or(false),
            "the newly selected dataset's cards should be visible"
        );
        assert!(
            !page
                .locator("#d0-ind-previewWine")
                .is_visible()
                .await
                .unwrap_or(true),
            "the previously selected dataset's cards should be hidden"
        );
        // The heading describes the dataset on screen: the worked example has
        // two nodes and one edge where the preview had one node and none.
        let heading = page
            .locator("#instance-graph-count")
            .inner_text()
            .await
            .expect("heading count");
        assert_eq!(
            heading.trim(),
            "4 / 2",
            "the heading count should follow the selected dataset; got: {heading}"
        );
        // The sidebar describes the same graph, so it must not be left showing
        // the landing dataset's numbers.
        assert_eq!(
            page.locator("#instance-graph-sidebar-count")
                .inner_text()
                .await
                .expect("sidebar count")
                .trim(),
            "4 / 2",
            "the sidebar count should agree with the heading after switching"
        );

        let prov = page
            .locator(".instance-dataset-panel:not([hidden]) .instance-provenance")
            .inner_text()
            .await
            .expect("provenance");
        assert!(
            prov.contains("wine_instances.yaml") && !prov.contains("preview"),
            "the visible panel names the selected dataset's source; got: {prov}"
        );

        // The canvas is re-initialized over the newly selected A-box. The viz
        // may still have been loading when the tab was clicked; whenever it
        // lands it paints the dataset that is active by then.
        assert!(
            wait_until_ready(&page, "!!window.__panschema_instance_viz").await,
            "instance graph viz never became ready"
        );
        assert_eq!(
            page.evaluate_value("window.__panschema_instance_active")
                .await
                .unwrap_or_default()
                .trim()
                .trim_matches('"'),
            "1",
            "the selected dataset is the one the viz was asked to paint"
        );
        let painted = page
            .evaluate_value(
                r#"(async function(){
                    var viz = window.__panschema_instance_viz;
                    if (!viz) return 'no-viz';
                    await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
                    var c = document.getElementById('instance-graph-canvas');
                    var ctx = c.getContext('2d');
                    if (!ctx) return 'no-2d-ctx';
                    var d = ctx.getImageData(0, 0, c.width, c.height).data;
                    var teal = 0;
                    for (var i = 0; i < d.length; i += 4) {
                        if (d[i] < 110 && d[i+1] > 110 && d[i+1] < 180 && d[i+2] > 190) teal++;
                    }
                    return 'ok:' + teal;
                })()"#,
            )
            .await
            .unwrap_or_default();
        let teal: u32 = painted
            .trim()
            .trim_matches('"')
            .strip_prefix("ok:")
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);
        assert!(
            teal > 0,
            "the swapped-in A-box should paint individual nodes; got: {painted}"
        );

        browser.close().await.ok();
        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}

/// The instance graph offers the schema graph's inspection affordances:
/// a hover detail card (the only surface for a shared value's usage count)
/// and the toolbar toggles, wired once through the shared shell.
#[test]
fn e2e_instance_graph_has_hover_card_and_toolbar_parity() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let output_dir = generate_docs_with_instances(
            "tests/fixtures/typed_wine.yaml",
            "tests/fixtures/typed_wine_instances.yaml",
        );
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");
        assert!(
            wait_until_ready(&page, "!!window.__panschema_instance_viz").await,
            "instance graph viz never became ready"
        );

        // The toolbar is present with the schema graph's controls.
        for id in [
            "instance-graph-reset",
            "instance-graph-zoom-in",
            "instance-graph-zoom-out",
            "instance-graph-labels-all",
            "instance-graph-labels-nodes",
            "instance-graph-labels-edges",
            "instance-graph-focus-on-hover",
            "instance-graph-arrows",
        ] {
            assert_eq!(
                page.locator(format!("#{id}")).count().await.expect("count"),
                1,
                "missing toolbar control #{id}"
            );
        }

        // Toggles drive the visualization, not just their own styling.
        let toggled = page
            .evaluate_value(
                r#"(function(){
                    var viz = window.__panschema_instance_viz;
                    var before = viz.node_labels_enabled() + ':' + viz.show_arrows();
                    document.getElementById('instance-graph-labels-nodes').click();
                    document.getElementById('instance-graph-arrows').click();
                    var after = viz.node_labels_enabled() + ':' + viz.show_arrows();
                    return before + ' -> ' + after;
                })()"#,
            )
            .await
            .unwrap_or_default();
        assert!(
            toggled.contains("true:true -> false:false"),
            "label and arrow toggles should flip viz state; got: {toggled}"
        );

        // Focus-on-hover honours its toggle: off means hovering focuses
        // nothing; back on, hovering focuses the node's neighborhood.
        let focus = page
            .evaluate_value(
                r#"(function(){
                    var viz = window.__panschema_instance_viz;
                    var canvas = document.getElementById('instance-graph-canvas');
                    var rect = canvas.getBoundingClientRect();
                    var dpr = window.devicePixelRatio || 1;
                    function hoverNode(i) {
                        var pos = viz.node_canvas_pos(i);
                        canvas.dispatchEvent(new MouseEvent('mousemove', {
                            clientX: rect.left + pos[0] / dpr,
                            clientY: rect.top + pos[1] / dpr,
                            bubbles: true
                        }));
                    }
                    document.getElementById('instance-graph-focus-on-hover').click(); // off
                    hoverNode(0);
                    var whileOff = viz.focused_node_index();
                    canvas.dispatchEvent(new MouseEvent('mouseleave', {bubbles: true}));
                    document.getElementById('instance-graph-focus-on-hover').click(); // on
                    hoverNode(0);
                    var whileOn = viz.focused_node_index();
                    return 'off:' + whileOff + ' on:' + whileOn;
                })()"#,
            )
            .await
            .unwrap_or_default();
        assert!(
            focus.contains("off:-1") && focus.contains("on:0"),
            "the focus toggle should gate hover focusing; got: {focus}"
        );

        // The legend button reflects its state like every other toggle.
        let legend_state = page
            .evaluate_value(
                r#"(function(){
                    var b = document.getElementById('instance-graph-legend-toggle');
                    b.click();
                    var on = b.classList.contains('active') + ':' + b.getAttribute('aria-pressed');
                    b.click();
                    var off = b.classList.contains('active') + ':' + b.getAttribute('aria-pressed');
                    return on + ' / ' + off;
                })()"#,
            )
            .await
            .unwrap_or_default();
        assert!(
            legend_state.contains("true:true / false:false"),
            "the legend button should light while open and dim when closed; got: {legend_state}"
        );

        // The hover card: an individual shows its class; a shared value node
        // shows its enum and how many individuals chose it — the wire's
        // usage_count has no other surface.
        let card = page
            .evaluate_value(
                r#"(function(){
                    var viz = window.__panschema_instance_viz;
                    var g = (window.__PANSCHEMA_INSTANCE_GRAPHS__ || [])[0];
                    var canvas = document.getElementById('instance-graph-canvas');
                    var rect = canvas.getBoundingClientRect();
                    var dpr = window.devicePixelRatio || 1;
                    function hoverNode(i) {
                        var pos = viz.node_canvas_pos(i);
                        canvas.dispatchEvent(new MouseEvent('mousemove', {
                            clientX: rect.left + pos[0] / dpr,
                            clientY: rect.top + pos[1] / dpr,
                            bubbles: true
                        }));
                    }
                    var redIdx = g.data.nodes.findIndex(function(n){
                        return n.node_type === 'enum_value' && n.label === 'red';
                    });
                    if (redIdx < 0) return 'no-red';
                    hoverNode(redIdx);
                    var el = document.getElementById('instance-graph-hover-card');
                    var value = el && el.style.display !== 'none' ? el.textContent : '(hidden)';
                    var morgonIdx = g.data.nodes.findIndex(function(n){
                        return n.id === 'individual:morgon';
                    });
                    hoverNode(morgonIdx);
                    var ind = el && el.style.display !== 'none' ? el.textContent : '(hidden)';
                    return 'value[' + value + '] individual[' + ind + ']';
                })()"#,
            )
            .await
            .unwrap_or_default();
        assert!(
            card.contains("WineColorEnum") && card.contains('2'),
            "the value card should name its enum and usage count; got: {card}"
        );
        assert!(
            card.contains("Morgon") && card.contains("Wine"),
            "the individual card should show its label and class; got: {card}"
        );

        browser.close().await.ok();
        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}

/// Grabbing a node on the instance canvas drags THE NODE, as on the schema
/// canvas — not the camera. A pan moves every node together; a node drag
/// changes the dragged node's position relative to the others.
#[test]
fn e2e_instance_graph_nodes_are_draggable() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let output_dir = generate_docs_with_instances(
            "tests/fixtures/typed_wine.yaml",
            "tests/fixtures/typed_wine_instances.yaml",
        );
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");
        assert!(
            wait_until_ready(&page, "!!window.__panschema_instance_viz").await,
            "instance graph viz never became ready"
        );

        let dragged = page
            .evaluate_value(
                r#"(function(){
                    var viz = window.__panschema_instance_viz;
                    var canvas = document.getElementById('instance-graph-canvas');
                    var rect = canvas.getBoundingClientRect();
                    var dpr = window.devicePixelRatio || 1;
                    function rel() {
                        var a = viz.node_canvas_pos(0), b = viz.node_canvas_pos(1);
                        return [a[0] - b[0], a[1] - b[1]];
                    }
                    var before = rel();
                    var pos = viz.node_canvas_pos(0);
                    var sx = rect.left + pos[0] / dpr, sy = rect.top + pos[1] / dpr;
                    canvas.dispatchEvent(new MouseEvent('mousedown', {clientX: sx, clientY: sy, bubbles: true}));
                    window.dispatchEvent(new MouseEvent('mousemove', {clientX: sx + 60, clientY: sy + 40, bubbles: true}));
                    window.dispatchEvent(new MouseEvent('mouseup', {clientX: sx + 60, clientY: sy + 40, bubbles: true}));
                    var after = rel();
                    var moved = Math.hypot(after[0] - before[0], after[1] - before[1]);
                    return 'relMoved:' + Math.round(moved);
                })()"#,
            )
            .await
            .unwrap_or_default();
        let moved: i64 = dragged
            .trim()
            .trim_matches('"')
            .strip_prefix("relMoved:")
            .and_then(|v| v.parse().ok())
            .unwrap_or(-1);
        assert!(
            moved > 20,
            "grabbing a node should move it relative to its neighbors (a pan moves \
             everything together); got: {dragged}"
        );

        browser.close().await.ok();
        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}

/// Clicking a node on the instance canvas selects it, as on the schema
/// canvas: the card pins open (surviving the cursor moving away) until the
/// node is deselected by clicking empty space.
#[test]
fn e2e_instance_graph_click_pins_the_card_and_empty_space_deselects() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let output_dir = generate_docs_with_instances(
            "tests/fixtures/typed_wine.yaml",
            "tests/fixtures/typed_wine_instances.yaml",
        );
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");
        assert!(
            wait_until_ready(&page, "!!window.__panschema_instance_viz").await,
            "instance graph viz never became ready"
        );

        let states = page
            .evaluate_value(
                r#"(function(){
                    var viz = window.__panschema_instance_viz;
                    var canvas = document.getElementById('instance-graph-canvas');
                    var card = document.getElementById('instance-graph-hover-card');
                    var rect = canvas.getBoundingClientRect();
                    var dpr = window.devicePixelRatio || 1;
                    function cardVisible() {
                        return card && card.style.display !== 'none' && card.style.display !== '';
                    }
                    function clickAt(sx, sy) {
                        var opts = {clientX: sx, clientY: sy, bubbles: true};
                        canvas.dispatchEvent(new MouseEvent('mousedown', opts));
                        window.dispatchEvent(new MouseEvent('mouseup', opts));
                        canvas.dispatchEvent(new MouseEvent('click', opts));
                    }
                    var pos = viz.node_canvas_pos(0);
                    clickAt(rect.left + pos[0] / dpr, rect.top + pos[1] / dpr);
                    var out = ['sel:' + viz.selected_node_index(), 'card:' + cardVisible()];
                    canvas.dispatchEvent(new MouseEvent('mousemove',
                        {clientX: rect.left + 3, clientY: rect.top + 3, bubbles: true}));
                    out.push('cardAfterMoveAway:' + cardVisible());
                    clickAt(rect.left + 3, rect.top + 3);
                    out.push('selAfterEmptyClick:' + viz.selected_node_index());
                    out.push('cardAfterEmptyClick:' + cardVisible());
                    return out.join(' ');
                })()"#,
            )
            .await
            .unwrap_or_default();
        assert!(
            states.contains("sel:0") && states.contains("card:true"),
            "clicking a node should select it and pin its card open; got: {states}"
        );
        assert!(
            states.contains("cardAfterMoveAway:true"),
            "the pinned card should survive the cursor moving off the node; got: {states}"
        );
        assert!(
            states.contains("selAfterEmptyClick:-1")
                && states.contains("cardAfterEmptyClick:false"),
            "clicking empty space should deselect and close the card; got: {states}"
        );

        browser.close().await.ok();
        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}

/// A click is a click even when the pointer jitters. A trackpad click moves
/// a pixel or two between press and release, and treating any movement at
/// all as a drag meant selection silently failed for real hands while
/// passing for synthetic events dispatched at one coordinate.
#[test]
fn e2e_instance_graph_selection_survives_pointer_jitter_and_escape_deselects() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let output_dir = generate_docs_with_instances(
            "tests/fixtures/typed_wine.yaml",
            "tests/fixtures/typed_wine_instances.yaml",
        );
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");
        assert!(
            wait_until_ready(&page, "!!window.__panschema_instance_viz").await,
            "instance graph viz never became ready"
        );

        let states = page
            .evaluate_value(
                r#"(function(){
                    var viz = window.__panschema_instance_viz;
                    var canvas = document.getElementById('instance-graph-canvas');
                    var card = document.getElementById('instance-graph-hover-card');
                    var rect = canvas.getBoundingClientRect();
                    var dpr = window.devicePixelRatio || 1;
                    function shown() {
                        return card && card.style.display !== 'none' && card.style.display !== '';
                    }
                    // A press, a small wobble, then a release — what a real
                    // trackpad click looks like.
                    function jitterClick(sx, sy) {
                        canvas.dispatchEvent(new MouseEvent('mousedown',
                            {clientX: sx, clientY: sy, bubbles: true}));
                        window.dispatchEvent(new MouseEvent('mousemove',
                            {clientX: sx + 2, clientY: sy + 1, bubbles: true}));
                        window.dispatchEvent(new MouseEvent('mouseup',
                            {clientX: sx + 2, clientY: sy + 1, bubbles: true}));
                        canvas.dispatchEvent(new MouseEvent('click',
                            {clientX: sx + 2, clientY: sy + 1, bubbles: true}));
                    }
                    var pos = viz.node_canvas_pos(0);
                    var nx = rect.left + pos[0] / dpr, ny = rect.top + pos[1] / dpr;
                    jitterClick(nx, ny);
                    var out = ['pinnedAfterJitter:' + shown()];

                    // Escape must clear the selection, as on the schema canvas.
                    document.dispatchEvent(new KeyboardEvent('keydown',
                        {key: 'Escape', bubbles: true}));
                    out.push('afterEscape_card:' + shown());
                    out.push('afterEscape_sel:' + viz.selected_node_index());

                    // Re-pin, then clear by clicking empty space with jitter.
                    jitterClick(nx, ny);
                    out.push('rePinned:' + shown());
                    jitterClick(rect.left + 3, rect.top + 3);
                    out.push('afterEmptyJitter_card:' + shown());
                    out.push('afterEmptyJitter_sel:' + viz.selected_node_index());
                    return out.join(' ');
                })()"#,
            )
            .await
            .unwrap_or_default();

        assert!(
            states.contains("pinnedAfterJitter:true"),
            "a click that wobbles a couple of pixels must still pin the card; got: {states}"
        );
        assert!(
            states.contains("afterEscape_card:false") && states.contains("afterEscape_sel:-1"),
            "Escape must deselect and close the card; got: {states}"
        );
        assert!(
            states.contains("rePinned:true"),
            "clicking the node again must re-pin; got: {states}"
        );
        assert!(
            states.contains("afterEmptyJitter_card:false")
                && states.contains("afterEmptyJitter_sel:-1"),
            "a wobbling click on empty space must still deselect; got: {states}"
        );

        browser.close().await.ok();
        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}

/// The instance graph is typed: individuals wear their class's circle and
/// colour, and each enum value in use is one shared diamond node the
/// choosing individuals link to — checked on the real rendered page, since
/// writer↔viz wire changes can pass every Rust test while the browser
/// renders nothing.
#[test]
fn e2e_typed_instance_graph_renders_class_symbols_and_shared_values() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let output_dir = generate_docs_with_instances(
            "tests/fixtures/typed_wine.yaml",
            "tests/fixtures/typed_wine_instances.yaml",
        );
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");

        assert!(
            wait_until_ready(&page, "!!window.__panschema_instance_viz").await,
            "instance graph viz never became ready"
        );

        // The wire document carries the typed encoding: two shared value
        // nodes (red, white — unused rose mints nothing), each red wine
        // linking to the ONE red node.
        let wire = page
            .evaluate_value(
                r#"(function(){
                    var g = (window.__PANSCHEMA_INSTANCE_GRAPHS__ || [])[0];
                    if (!g || !g.data) return 'no-data';
                    var values = g.data.nodes.filter(function(n){ return n.node_type === 'enum_value'; });
                    var red = values.find(function(n){ return n.label === 'red'; });
                    if (!red) return 'no-red:' + JSON.stringify(values);
                    var redEdges = g.data.edges.filter(function(e){ return e.target === red.id; });
                    return 'values:' + values.length +
                        ' redSources:' + redEdges.map(function(e){ return e.source; }).sort().join(',') +
                        ' labels:' + redEdges.map(function(e){ return e.label; }).join(',') +
                        ' usage:' + (red.kind_metadata ? red.kind_metadata.usageCount : '?') +
                        ' version:' + g.data.format_version;
                })()"#,
            )
            .await
            .unwrap_or_default();
        assert!(
            wire.contains("values:2")
                && wire.contains("redSources:individual:fleurie,individual:morgon")
                && wire.contains("labels:color,color")
                && wire.contains("usage:2")
                && wire.contains("version:1.2"),
            "the typed wire encoding should reach the page; got: {wire}"
        );

        // The legend describes the typed key.
        let summary = page
            .evaluate_value(
                r#"(function(){
                    var viz = window.__panschema_instance_viz;
                    return viz && typeof viz.legend_summary_json === 'function'
                        ? viz.legend_summary_json() : 'no-api';
                })()"#,
            )
            .await
            .unwrap_or_default();
        assert!(
            summary.contains("Individual") && summary.contains("Enum value"),
            "the key lists both typed kinds; got: {summary}"
        );

        // And the canvas actually paints them: class-blue circles for the
        // wines and enum-purple diamonds for the shared values.
        let painted = page
            .evaluate_value(
                r#"(async function(){
                    await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
                    var c = document.getElementById('instance-graph-canvas');
                    var ctx = c.getContext('2d');
                    if (!ctx) return 'no-ctx';
                    var d = ctx.getImageData(0, 0, c.width, c.height).data;
                    var blue = 0, purple = 0, teal = 0;
                    for (var i = 0; i < d.length; i += 4) {
                        var r = d[i], g = d[i+1], b = d[i+2];
                        if (r < 110 && g > 110 && g < 180 && b > 180) blue++;
                        if (r > 120 && r < 190 && g < 120 && b > 140) purple++;
                        if (r < 100 && g > 150 && g < 215 && b > 150 && b < 215) teal++;
                    }
                    return 'blue:' + blue + ' purple:' + purple + ' teal:' + teal;
                })()"#,
            )
            .await
            .unwrap_or_default();
        let count_of = |k: &str| -> i64 {
            painted
                .split_whitespace()
                .find_map(|p| p.strip_prefix(&format!("{k}:")))
                .and_then(|v| v.trim_matches('"').parse().ok())
                .unwrap_or(-1)
        };
        assert!(
            count_of("blue") > 0 && count_of("purple") > 0,
            "class-coloured individuals and enum-coloured values should paint; got: {painted}"
        );
        assert_eq!(
            count_of("teal"),
            0,
            "no generic teal markers remain; got: {painted}"
        );

        browser.close().await.ok();
        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}

/// Each graph's notation key is adaptive: it lists only the node and edge
/// kinds that graph actually uses, from one code path serving both canvases./// Each graph's notation key is adaptive: it lists only the node and edge
/// kinds that graph actually uses, from one code path serving both canvases.
#[test]
fn e2e_legends_adapt_to_what_each_graph_contains() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        // wine_catalog declares classes and slots but no enums, so the
        // schema key must not advertise the enum diamond; the instance
        // graph's key must describe individuals and assertions only.
        let output_dir = generate_docs_with_instances(
            "tests/fixtures/wine_catalog.yaml",
            "tests/fixtures/wine_instances.yaml",
        );
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");

        assert!(
            wait_until_ready(
                &page,
                "!!window.__panschema_instance_viz && !!window.__panschema_viz"
            )
            .await,
            "both graph visualizations should come up"
        );

        // The summary is built from the same row selectors the drawing
        // uses, so these assertions are assertions about the drawn key.
        let instance_summary = page
            .evaluate_value(
                r#"(function(){
                    var viz = window.__panschema_instance_viz;
                    if (!viz || typeof viz.legend_summary_json !== 'function') return 'no-api';
                    return viz.legend_summary_json();
                })()"#,
            )
            .await
            .unwrap_or_default();
        assert!(
            instance_summary.contains("Individual") && instance_summary.contains("assertion"),
            "the instance key describes individuals and assertions; got: {instance_summary}"
        );
        assert!(
            !instance_summary.contains("\"Class\"") && !instance_summary.contains("Enum"),
            "the instance key must not advertise schema-only symbols; got: {instance_summary}"
        );
        assert!(
            instance_summary.contains("\"cardinality\":false"),
            "assertions carry no crow's-feet; got: {instance_summary}"
        );

        let schema_summary = page
            .evaluate_value(
                r#"(function(){
                    var viz = window.__panschema_viz;
                    if (!viz || typeof viz.legend_summary_json !== 'function') return 'no-api';
                    return viz.legend_summary_json();
                })()"#,
            )
            .await
            .unwrap_or_default();
        // This fixture mixes shared top-level slots (drawn as slot pills)
        // with inline attributes (drawn as direct edges), so the key lists
        // classes, slots, and the range edges.
        assert!(
            schema_summary.contains("\"Class\"") && schema_summary.contains("\"range\""),
            "the schema key lists the kinds present; got: {schema_summary}"
        );
        assert!(
            schema_summary.contains("\"Slot\""),
            "shared top-level slots draw slot pills, so the key has a Slot row; \
             got: {schema_summary}"
        );
        assert!(
            !schema_summary.contains("Enum"),
            "a schema with no enums must not advertise the diamond; got: {schema_summary}"
        );

        // An attributes-only schema draws no slot pills, so its key has no
        // Slot row — the half of the adaptation the mixed fixture above can
        // no longer show.
        let attr_only_dir = generate_docs_for("tests/fixtures/scoped_estate.yaml");
        let (attr_listener, attr_port) = bind_ephemeral();
        let (attr_shutdown_tx, attr_shutdown_rx) = oneshot::channel();
        let attr_server = tokio::spawn(start_server(
            attr_only_dir.clone(),
            attr_listener,
            attr_shutdown_rx,
        ));
        tokio::time::sleep(Duration::from_millis(100)).await;
        let attr_page = browser.new_page().await.expect("attributes-only page");
        attr_page
            .goto(&format!("http://127.0.0.1:{attr_port}/index.html"), None)
            .await
            .expect("goto attributes-only docs");
        assert!(
            wait_for_graph_viz_ready(&attr_page).await,
            "attributes-only schema graph should become ready"
        );
        let attr_summary = attr_page
            .evaluate_value(
                r#"(function(){
                    var viz = window.__panschema_viz;
                    if (!viz || typeof viz.legend_summary_json !== 'function') return 'no-api';
                    return viz.legend_summary_json();
                })()"#,
            )
            .await
            .unwrap_or_default();
        assert!(
            attr_summary.contains("\"Class\""),
            "the attributes-only key still lists classes; got: {attr_summary}"
        );
        assert!(
            !attr_summary.contains("\"Slot\""),
            "no slot pills are drawn for inline attributes, so no Slot row; \
             got: {attr_summary}"
        );
        let _ = attr_shutdown_tx.send(());
        let _ = attr_server.await;
        let _ = fs::remove_dir_all(attr_only_dir);

        // The instance graph's key is reachable: toggling shows the panel.
        page.locator("#instance-graph-legend-toggle")
            .click(None)
            .await
            .expect("toggle instance legend");
        assert!(
            page.locator("#instance-graph-legend")
                .is_visible()
                .await
                .unwrap_or(false),
            "the instance legend panel should open on toggle"
        );

        // The panel sizes to its rows: its height tracks the extent the
        // viz reports for this key, with no fixed-box dead space below the
        // last row.
        let sizing = page
            .evaluate_value(
                r#"(function(){
                    var viz = window.__panschema_instance_viz;
                    var panel = document.getElementById('instance-graph-legend');
                    if (!viz || !panel || typeof viz.legend_extent_json !== 'function') return 'no-api';
                    var extent = JSON.parse(viz.legend_extent_json());
                    var slack = panel.getBoundingClientRect().height - extent.height;
                    return 'slack:' + Math.round(slack) + ' extent:' + Math.round(extent.height);
                })()"#,
            )
            .await
            .unwrap_or_default();
        let slack: i64 = sizing
            .trim()
            .trim_matches('"')
            .strip_prefix("slack:")
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse().ok())
            .unwrap_or(i64::MAX);
        assert!(
            (0..=12).contains(&slack),
            "the panel should wrap the key with only border/padding slack; got: {sizing}"
        );

        // And the two keys genuinely differ in size: the instance key is a
        // fraction of the schema key's height.
        let heights = page
            .evaluate_value(
                r#"(function(){
                    var a = window.__panschema_instance_viz, b = window.__panschema_viz;
                    if (!a || !b) return 'no-viz';
                    return JSON.parse(a.legend_extent_json()).height + ' vs ' +
                           JSON.parse(b.legend_extent_json()).height;
                })()"#,
            )
            .await
            .unwrap_or_default();
        let parts: Vec<f64> = heights
            .trim()
            .trim_matches('"')
            .split(" vs ")
            .filter_map(|p| p.parse().ok())
            .collect();
        assert!(
            parts.len() == 2 && parts[0] < parts[1],
            "the instance key should be shorter than the schema key; got: {heights}"
        );

        browser.close().await.ok();
        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}

/// The instance graph is the same explorable component as the schema graph:
/// it fills its viewport instead of clustering in a corner, offers the layout
/// picker, and focuses the hovered node's neighborhood.
#[test]
fn e2e_instance_graph_is_explorable_like_the_schema_graph() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let output_dir = generate_docs_with_instances(
            "tests/fixtures/wine_catalog.yaml",
            "tests/fixtures/wine_instances.yaml",
        );
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");

        assert!(
            wait_until_ready(&page, "!!window.__panschema_instance_viz").await,
            "instance graph viz never became ready"
        );

        // Viewport fill: after the layout settles and the camera fits, the
        // painted content spans a substantial share of the canvas rather than
        // clustering in one corner.
        assert!(
            wait_until_ready(
                &page,
                r#"(function(){
                    var c = document.getElementById('instance-graph-canvas');
                    if (!c) return false;
                    var ctx = c.getContext('2d');
                    if (!ctx) return false;
                    var d = ctx.getImageData(0, 0, c.width, c.height).data;
                    var minX = c.width, maxX = 0, minY = c.height, maxY = 0, any = false;
                    for (var y = 0; y < c.height; y += 4) {
                        for (var x = 0; x < c.width; x += 4) {
                            var i = (y * c.width + x) * 4;
                            // Painted = notably brighter than the dark bg.
                            if (d[i] + d[i+1] + d[i+2] > 140) {
                                any = true;
                                if (x < minX) minX = x;
                                if (x > maxX) maxX = x;
                                if (y < minY) minY = y;
                                if (y > maxY) maxY = y;
                            }
                        }
                    }
                    if (!any) return false;
                    var w = maxX - minX, h = maxY - minY;
                    var cx = (minX + maxX) / 2, cy = (minY + maxY) / 2;
                    // Fitted means wide AND roughly centered — an unfitted
                    // default view can be wide while sitting in a corner.
                    return w > c.width * 0.5 && h > c.height * 0.4 &&
                        Math.abs(cx - c.width / 2) < c.width * 0.25 &&
                        Math.abs(cy - c.height / 2) < c.height * 0.25;
                })()"#
            )
            .await,
            "the settled instance graph should fill and center in its viewport"
        );

        // Reset recovers from a far pan: after shoving the camera away, the
        // painted graph returns to a fitted, centered view.
        page.evaluate_value(
            "(function(){ window.__panschema_instance_viz.pan(4000, 4000); return 'panned'; })()",
        )
        .await
        .expect("pan");
        page.locator("#instance-graph-reset")
            .click(None)
            .await
            .expect("click reset");
        assert!(
            wait_until_ready(
                &page,
                r#"(function(){
                    var c = document.getElementById('instance-graph-canvas');
                    var ctx = c.getContext('2d');
                    if (!ctx) return false;
                    var d = ctx.getImageData(0, 0, c.width, c.height).data;
                    var minX = c.width, maxX = 0, minY = c.height, maxY = 0, any = false;
                    for (var y = 0; y < c.height; y += 4) {
                        for (var x = 0; x < c.width; x += 4) {
                            var i = (y * c.width + x) * 4;
                            if (d[i] + d[i+1] + d[i+2] > 140) {
                                any = true;
                                if (x < minX) minX = x;
                                if (x > maxX) maxX = x;
                                if (y < minY) minY = y;
                                if (y > maxY) maxY = y;
                            }
                        }
                    }
                    if (!any) return false;
                    var cx = (minX + maxX) / 2, cy = (minY + maxY) / 2;
                    return (maxX - minX) > c.width * 0.5 &&
                        Math.abs(cx - c.width / 2) < c.width * 0.25 &&
                        Math.abs(cy - c.height / 2) < c.height * 0.25;
                })()"#
            )
            .await,
            "reset should re-fit and re-center the panned-away graph"
        );

        // The layout picker is present with the same options as the schema
        // graph's, and choosing another implemented layout re-creates the viz.
        let picker = page.locator("#instance-graph-layout-select");
        assert_eq!(
            picker.count().await.expect("picker count"),
            1,
            "the instance graph should offer the layout picker"
        );
        let switched = page
            .evaluate_value(
                r#"(function(){
                    var s = document.getElementById('instance-graph-layout-select');
                    window.__instance_viz_before = window.__panschema_instance_viz;
                    s.value = 'force-directed';
                    s.dispatchEvent(new Event('change', {bubbles: true}));
                    return 'changed';
                })()"#,
            )
            .await
            .unwrap_or_default();
        assert!(switched.contains("changed"), "picker change failed: {switched}");
        assert!(
            wait_until_ready(
                &page,
                "window.__panschema_instance_viz && window.__panschema_instance_viz !== window.__instance_viz_before"
            )
            .await,
            "choosing a layout should re-create the instance viz"
        );

        // Focus-on-hover: hovering a node focuses its neighborhood, exactly
        // as the schema graph does.
        let focused = page
            .evaluate_value(
                r#"(function(){
                    var viz = window.__panschema_instance_viz;
                    if (!viz || typeof viz.node_canvas_pos !== 'function') return 'no-viz';
                    var pos = viz.node_canvas_pos(0);
                    if (!pos || pos.length < 2) return 'no-pos';
                    var canvas = document.getElementById('instance-graph-canvas');
                    var rect = canvas.getBoundingClientRect();
                    var dpr = window.devicePixelRatio || 1;
                    var x = rect.left + pos[0] / dpr, y = rect.top + pos[1] / dpr;
                    canvas.dispatchEvent(new MouseEvent('mousemove', {clientX: x, clientY: y, bubbles: true}));
                    return 'hovered:' + viz.hovered_node_index();
                })()"#,
            )
            .await
            .unwrap_or_default();
        assert!(
            focused.contains("hovered:0"),
            "hovering a node should register on the viz; got: {focused}"
        );

        browser.close().await.ok();
        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(output_dir);
    });
}

/// The `generate --instances` path renders a LinkML instance-data file as the
/// instance graph — the schema declares no OWL individuals, so the A-box comes
/// entirely from the data file — and its own canvas paints the
/// class-colored individual nodes.
#[test]
fn e2e_instance_graph_renders_from_linkml_data() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let output_dir = generate_docs_with_instances(
            "tests/fixtures/wine_catalog.yaml",
            "tests/fixtures/wine_instances.yaml",
        );
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");

        // The instance-graph canvas exists even though the schema has no
        // OWL individuals — the A-box is the LinkML data file.
        assert_eq!(
            page.locator("#instance-graph-canvas")
                .count()
                .await
                .expect("count"),
            1,
            "the LinkML instance data should render an instance-graph canvas"
        );

        // The sidebar carries an Instance Graph entry with node/edge badges
        // that navigates to the section.
        let sidebar_link = page.locator("a.sidebar-link[href='#individuals']");
        assert_eq!(
            sidebar_link.count().await.expect("count"),
            1,
            "sidebar should carry an Instance Graph entry"
        );
        let link_text = sidebar_link.inner_text().await.expect("link text");
        assert!(
            link_text.contains("Instance Graph"),
            "sidebar entry should be named Instance Graph; got: {link_text}"
        );
        assert!(
            link_text.contains("4 / 2"),
            "badge should show node/edge counts; got: {link_text}"
        );
        // Text asserted above, hash asserted below.
        dom_click(&page, "a.sidebar-link[href='#individuals']").await;
        let hash = page
            .evaluate_value("window.location.hash")
            .await
            .unwrap_or_default();
        assert!(
            hash.contains("#individuals"),
            "clicking the entry should navigate to the section; hash = {hash}"
        );

        // The section states where the A-box came from.
        let prov = page
            .locator(".instance-provenance")
            .inner_text()
            .await
            .expect("provenance");
        assert!(
            prov.contains("wine_instances.yaml"),
            "provenance should name the data file; got: {prov}"
        );

        // LinkML-data instances get cards through the same path as OWL
        // individuals: typed, with the reference linking to the referenced
        // individual's card.
        assert_eq!(
            page.locator("#ind-chateauMorgon")
                .count()
                .await
                .expect("count"),
            1,
            "a LinkML-data instance should render an individual card"
        );
        let ref_link = page.locator("#ind-chateauMorgon a[href='#ind-morgonEstate']");
        assert_eq!(
            ref_link.count().await.expect("count"),
            1,
            "the produced_by reference should link to the referenced individual's card"
        );

        // The A-box read from the data file: four records, two reference edges.
        let counts = page
            .evaluate_value(
                r#"(function(){
                    var g = window.__PANSCHEMA_INSTANCE_GRAPHS__;
                    var d = g && g[0] && g[0].data;
                    return d ? (d.nodes.length + ',' + d.edges.length) : 'none';
                })()"#,
            )
            .await
            .unwrap_or_default();
        assert_eq!(
            counts.trim().trim_matches('"'),
            "4,2",
            "two wines + two wineries + two produced_by edges; got {counts}"
        );

        assert!(
            wait_until_ready(&page, "!!window.__panschema_instance_viz").await,
            "instance graph viz never became ready"
        );

        // The canvas painted the teal individual nodes (RGB ~ 41,184,179) —
        // proof the LinkML-sourced A-box actually renders.
        let result = page
            .evaluate_value(
                r#"(async function(){
                    var viz = window.__panschema_instance_viz;
                    if (!viz) return 'no-viz';
                    await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
                    var c = document.getElementById('instance-graph-canvas');
                    var ctx = c.getContext('2d');
                    if (!ctx) return 'no-2d-ctx';
                    var d = ctx.getImageData(0, 0, c.width, c.height).data;
                    var teal = 0;
                    for (var i = 0; i < d.length; i += 4) {
                        if (d[i] < 110 && d[i+1] > 110 && d[i+1] < 180 && d[i+2] > 190) teal++;
                    }
                    return 'ok:' + teal;
                })()"#,
            )
            .await
            .unwrap_or_default();
        let result = result.trim().trim_matches('"').to_string();
        assert!(
            result.starts_with("ok:"),
            "the instance viz should have initialized; got {result}"
        );
        let teal: i64 = result.trim_start_matches("ok:").parse().unwrap_or(0);
        assert!(
            teal > 0,
            "the LinkML instance graph should paint individual nodes; class-blue pixels={teal}"
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
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
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
        let select = page.locator("#graph-layout-select");
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
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
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
        let enum_card = page.locator("#enum-Status");
        let enum_html = enum_card
            .inner_html()
            .await
            .expect("Status enum card should be present");
        assert!(
            enum_html.contains("open") && enum_html.contains("closed"),
            "enum card lists its permissible values; got: {enum_html}"
        );

        // Types section + card with its pattern constraint.
        let type_card = page.locator("#type-PhoneNumber");
        let type_html = type_card
            .inner_html()
            .await
            .expect("PhoneNumber type card should be present");
        assert!(
            type_html.contains(r"\+[1-9]"),
            "type card shows its pattern; got: {type_html}"
        );

        // Sidebar gained the two nav entries.
        let nav = page.locator(".sidebar-nav");
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
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
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
        let abstract_card = page.locator("#class-NamedThing");
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
        let person_card = page.locator("#class-Person");
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
        let age_card = page.locator("#slot-age");
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
        let membership_card = page.locator("#slot-membership");
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
    let (listener, port) = bind_ephemeral();
    let base_url = format!("http://127.0.0.1:{}", port);

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server_handle = tokio::spawn(start_server(output_dir.clone(), listener, shutdown_rx));
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

    let container = page.locator(".graph-container");

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
