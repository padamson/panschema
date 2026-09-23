//! End-to-end browser tests using Playwright.
//!
//! These tests verify the generated documentation renders correctly in a real browser.
//!
//! ## Setup
//! Install Playwright browsers matching the driver `playwright-rs` vendors:
//! ```bash
//! cargo run --example install-browsers
//! ```
//!
//! The example asks the crate which build that is, so nothing here names a
//! Playwright version; [`playwright_rs::PLAYWRIGHT_VERSION`] is the source of
//! truth.
//!
//! ## Running
//! - Default (chromium): `cargo nextest run e2e`
//! - Specific browser: `BROWSER=firefox cargo nextest run e2e`
//! - All browsers (CI): `BROWSER=all cargo nextest run e2e`

mod mdbook;

#[path = "../common/mod.rs"]
mod common;
use common::generate_site;

use std::fs;
use std::future::Future;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;

use playwright_rs::{Browser, Page, Playwright, expect};
use tokio::sync::oneshot;
use tower_http::services::ServeDir;

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

/// Generates the reference ontology into a scratch directory.
fn generate_docs() -> tempfile::TempDir {
    generate_site("tests/fixtures/reference.ttl", &[])
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

/// Wait until a JS readiness expression is truthy, polling on animation
/// frames through the driver with its default 30 s ceiling. Robust to
/// variable CI load — e.g. a page that renders both a schema graph and a
/// second instance graph, each loading wasm. An expression that throws
/// before the page is ready counts as not ready yet. The error names the
/// cause: the driver's timeout, a closed page, or a bad expression.
async fn wait_until_ready(
    page: &playwright_rs::Page,
    ready_expr: &str,
) -> Result<(), playwright_rs::Error> {
    let predicate =
        format!("() => {{ try {{ return !!({ready_expr}); }} catch (_) {{ return false; }} }}");
    page.wait_for_function(&predicate, None).await.map(|_| ())
}

/// A canvas click the drag gate accepts: press on the canvas, release on
/// the window, then the click. Spliced into page scripts as `clickAt`.
const CLICK_AT_JS: &str = r#"function clickAt(sx, sy) {
                        var opts = {clientX: sx, clientY: sy, bubbles: true};
                        canvas.dispatchEvent(new MouseEvent('mousedown', opts));
                        window.dispatchEvent(new MouseEvent('mouseup', opts));
                        canvas.dispatchEvent(new MouseEvent('click', opts));
                    }"#;

/// The schema graph's wasm viz is ready when `__panschema_viz` exists and
/// node 0 has a canvas position.
async fn wait_for_graph_viz_ready(page: &playwright_rs::Page) -> Result<(), playwright_rs::Error> {
    wait_until_ready(
        page,
        "window.__panschema_viz && typeof window.__panschema_viz.node_canvas_pos === 'function' \
         && window.__panschema_viz.node_canvas_pos(0).length >= 2",
    )
    .await
}

/// The origin the in-process service answers for. Interception decides
/// which URLs a service owns, so this needs no certificate and no listener —
/// and unlike `127.0.0.1:<port>` it is the same string in every test.
const SITE_ORIGIN: &str = "https://panschema.test";

/// Launch one of the three engines by name.
async fn launch_browser(playwright: &Playwright, browser_name: &str) -> Browser {
    match browser_name {
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
    }
}

/// Open a page that serves `site` at [`SITE_ORIGIN`] from an in-process
/// `ServeDir` — no port bound, no server task, so nothing to race for and
/// nothing to wait for before navigating.
///
/// The browser is returned with the page because dropping it closes the
/// page.
async fn open_served_page(
    playwright: &Playwright,
    browser_name: &str,
    site: &Path,
) -> (Browser, Page) {
    let browser = launch_browser(playwright, browser_name).await;
    let page = browser.new_page().await.expect("Failed to create page");
    serve_site(&page, site).await;
    (browser, page)
}

/// Serve `site` at [`SITE_ORIGIN`] on a page that already exists — for the
/// tests that need a context, a viewport, or an init script before they
/// navigate.
async fn serve_site(page: &Page, site: &Path) {
    page.route_service(&format!("{SITE_ORIGIN}/**"), ServeDir::new(site))
        .await
        .expect("Failed to serve the generated site in-process");
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

/// Opens `site` at its index on a fresh 1280×720 page in each of `browsers`
/// and runs `body` there. A browser launch per test is the suite's unit of
/// isolation; a panic unwinds through the Playwright handle, which closes
/// every browser it launched.
fn on_site<F>(site: &Path, browsers: &[&str], body: F)
where
    F: for<'a> Fn(&'a str, &'a Page) -> Pin<Box<dyn Future<Output = ()> + 'a>>,
{
    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    rt.block_on(async {
        let playwright = Playwright::launch()
            .await
            .expect("Failed to initialize Playwright");
        for browser_name in browsers {
            let (browser, page) = open_served_page(&playwright, browser_name, site).await;
            page.set_viewport_size(playwright_rs::Viewport {
                width: 1280,
                height: 720,
            })
            .await
            .expect("Failed to set the desktop viewport");
            page.goto(&format!("{SITE_ORIGIN}/index.html"), None)
                .await
                .expect("Failed to navigate to index page");
            body(browser_name, &page).await;
            browser.close().await.expect("Failed to close browser");
        }
    });
}

/// The reference site in every browser `BROWSER` names: for claims about
/// what the generated page renders, which every engine must agree on.
fn in_every_browser<F>(body: F)
where
    F: for<'a> Fn(&'a str, &'a Page) -> Pin<Box<dyn Future<Output = ()> + 'a>>,
{
    let site = generate_docs();
    on_site(site.path(), &get_browsers_to_test(), body);
}

/// `site` in Chromium only: for claims about one graph's behavior, where a
/// second engine would repeat the same wasm run.
fn in_chromium<F>(site: tempfile::TempDir, body: F)
where
    F: for<'a> Fn(&'a Page) -> Pin<Box<dyn Future<Output = ()> + 'a>>,
{
    on_site(site.path(), &["chromium"], move |_, page| body(page));
}

/// Clicks node `index` of the schema graph at the canvas position the viz
/// reports for it, through the press, release, click sequence the drag
/// gate expects. Nodes are canvas-drawn, so there is no DOM element to
/// click. Panics when the viz has no such node.
async fn click_schema_node(page: &Page, index: usize) {
    let script = format!(
        r#"(function(){{
            var viz = window.__panschema_viz;
            if (!viz || typeof viz.node_canvas_pos !== 'function') return 'no-viz';
            var pos = viz.node_canvas_pos({index});
            if (!pos || pos.length < 2) return 'no-pos';
            var canvas = document.getElementById('graph-canvas');
            var rect = canvas.getBoundingClientRect();
            var dpr = window.devicePixelRatio || 1;
            __CLICK_AT__
            clickAt(rect.left + pos[0] / dpr, rect.top + pos[1] / dpr);
            return 'clicked';
        }})()"#
    )
    .replace("__CLICK_AT__", CLICK_AT_JS);
    let clicked = page.evaluate_value(&script).await.unwrap_or_default();
    assert!(
        clicked.contains("clicked"),
        "expected to click schema node {index}; got: {clicked}"
    );
}

#[test]
fn e2e_reference_page_shows_title_sidebar_and_metadata() {
    in_every_browser(|browser_name, page| {
        Box::pin(async move {
            let title = page.title().await.expect("Failed to get page title");
            assert!(
                title.contains("panschema Reference Ontology"),
                "[{}] Page title should contain ontology name, got: {}",
                browser_name,
                title
            );

            let sidebar_count = page
                .locator(".sidebar")
                .count()
                .await
                .expect("Failed to count sidebars");
            assert!(
                sidebar_count > 0,
                "[{}] Sidebar should be present",
                browser_name
            );

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
        })
    });
}

#[test]
fn e2e_class_cards_show_content_and_hierarchy() {
    in_every_browser(|browser_name, page| {
        Box::pin(async move {
            // The section header counts the six classes: Animal, Cat, Dog,
            // Mammal, Person, Pet.
            let class_section_html = page
                .locator("#classes")
                .inner_html()
                .await
                .expect("Failed to get classes section");
            assert!(
                class_section_html.contains(">6<"),
                "[{}] Classes section should show count of 6, got: {}",
                browser_name,
                class_section_html
            );
            let class_link_count = page
                .locator(".class-link")
                .count()
                .await
                .expect("Failed to count class links");
            assert_eq!(
                class_link_count, 6,
                "[{}] Should have 6 class links",
                browser_name
            );
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
            let class_card_count = page
                .locator(".class-card")
                .count()
                .await
                .expect("Failed to count class cards");
            assert_eq!(
                class_card_count, 6,
                "[{}] Should have 6 class cards",
                browser_name
            );

            let dog_card_html = page
                .locator("#class-Dog")
                .inner_html()
                .await
                .expect("Failed to get Dog card");
            assert!(
                dog_card_html.contains("A domesticated carnivorous mammal"),
                "[{}] Dog card should show description, got: {}",
                browser_name,
                dog_card_html
            );
            assert!(
                dog_card_html.contains("http://example.org/panschema/reference#Dog"),
                "[{}] Dog card should show IRI",
                browser_name
            );
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

            let mammal_card_html = page
                .locator("#class-Mammal")
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

            let animal_card_html = page
                .locator("#class-Animal")
                .inner_html()
                .await
                .expect("Failed to get Animal card");
            assert!(
                animal_card_html.contains("Superclass of"),
                "[{}] Animal card should show 'Superclass of'",
                browser_name
            );

            let person_card_html = page
                .locator("#class-Person")
                .inner_html()
                .await
                .expect("Failed to get Person card");
            assert!(
                !person_card_html.contains("Subclass of"),
                "[{}] Person card should not show 'Subclass of' (it's a root class)",
                browser_name
            );
        })
    });
}

/// Card metadata rows that render only through the full OWL → IR → HTML
/// path: a deprecated class, and a class with aliases, see-also, and a
/// SKOS mapping.
#[test]
fn e2e_class_cards_show_deprecation_aliases_and_mappings() {
    in_every_browser(|browser_name, page| {
        Box::pin(async move {
            let pet_html = page
                .locator("#class-Pet")
                .inner_html()
                .await
                .expect("Failed to get Pet card");
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

            let person_card_html = page
                .locator("#class-Person")
                .inner_html()
                .await
                .expect("Failed to get Person card");
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
        })
    });
}

#[test]
fn e2e_slot_cards_show_domain_range_and_characteristics() {
    in_every_browser(|browser_name, page| {
        Box::pin(async move {
            let slot_section_html = page
                .locator("#slots")
                .inner_html()
                .await
                .expect("Failed to get slots section");
            assert!(
                slot_section_html.contains(">5<"),
                "[{}] Slots section should show count of 5, got: {}",
                browser_name,
                slot_section_html
            );
            let slot_link_count = page
                .locator(".slot-link")
                .count()
                .await
                .expect("Failed to count slot links");
            assert_eq!(
                slot_link_count, 5,
                "[{}] Should have 5 slot links",
                browser_name
            );
            let slot_card_count = page
                .locator(".slot-card")
                .count()
                .await
                .expect("Failed to count slot cards");
            assert_eq!(
                slot_card_count, 5,
                "[{}] Should have 5 slot cards",
                browser_name
            );

            // Object-ranged: hasOwner links both ends.
            let has_owner_html = page
                .locator("#slot-hasOwner")
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

            // Datatype-ranged: hasAge names its datatype.
            let has_age_html = page
                .locator("#slot-hasAge")
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

            let owns_html = page
                .locator("#slot-owns")
                .inner_html()
                .await
                .expect("Failed to get owns card");
            assert!(
                owns_html.contains("Inverse of: has owner"),
                "[{}] owns should show inverse of characteristic",
                browser_name
            );

            // relatedTo is symmetric and transitive: both badges show.
            let related_html = page
                .locator("#slot-relatedTo")
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
        })
    });
}

/// The individuals heading counts the graph, one individual and no
/// assertions between individuals, so it reads like the schema graph's
/// badge rather than a bare individual count.
#[test]
fn e2e_individuals_section_counts_the_graph_and_renders_the_card() {
    in_every_browser(|browser_name, page| {
        Box::pin(async move {
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
            let ind_section_html = page
                .locator("#individuals")
                .inner_html()
                .await
                .expect("Failed to get individuals section");
            assert!(
                ind_section_html.contains("ind-fido"),
                "[{}] Individuals section should render the individual's card, got: {}",
                browser_name,
                ind_section_html
            );
            let ind_link_count = page
                .locator(".individual-link")
                .count()
                .await
                .expect("Failed to count individual links");
            assert_eq!(
                ind_link_count, 1,
                "[{}] Should have 1 individual link",
                browser_name
            );
            let ind_card_count = page
                .locator(".individual-card")
                .count()
                .await
                .expect("Failed to count individual cards");
            assert_eq!(
                ind_card_count, 1,
                "[{}] Should have 1 individual card",
                browser_name
            );

            let fido_card_html = page
                .locator("#ind-fido")
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

            let ind_sidebar_count = page
                .locator(".sidebar-link[href='#individuals']")
                .count()
                .await
                .expect("Failed to count individuals sidebar link");
            assert!(
                ind_sidebar_count > 0,
                "[{}] Individuals navigation link should exist in sidebar",
                browser_name
            );
        })
    });
}

#[test]
fn e2e_sidebar_link_navigates_and_scroll_spy_follows() {
    in_every_browser(|browser_name, page| {
        Box::pin(async move {
            let link_count = page
                .locator(".sidebar-link[href='#classes']")
                .count()
                .await
                .expect("Failed to count links");
            assert!(
                link_count > 0,
                "[{}] Classes navigation link should exist in sidebar",
                browser_name
            );

            // Presence is asserted above and the hash wait below verifies
            // the click took effect.
            dom_click(page, ".sidebar-link[href='#classes']").await;
            wait_until_ready(page, "location.hash === '#classes'")
                .await
                .unwrap_or_else(|e| {
                    panic!("[{browser_name}] URL hash should be #classes after click: {e}")
                });
            let section_count = page
                .locator("#classes")
                .count()
                .await
                .expect("Failed to count classes sections");
            assert!(
                section_count > 0,
                "[{}] Classes section should exist as link target",
                browser_name
            );

            // Scroll spy: the Classes link goes active and Metadata does not.
            wait_until_ready(
                page,
                "document.querySelector('.sidebar-link[href=\"#classes\"]')?.classList.contains('active') ?? false",
            )
            .await
            .unwrap_or_else(|e| panic!("[{browser_name}] Scroll spy should mark Classes sidebar link as active after scrolling to #classes: {e}"));
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
        })
    });
}

/// On a desktop viewport the sidebar shows without a menu toggle, and the
/// classes render as a tree: a child sits below and indented under its
/// parent, and leaf siblings tile on one row.
#[test]
fn e2e_desktop_viewport_shows_sidebar_and_trees_the_classes() {
    in_every_browser(|browser_name, page| {
        Box::pin(async move {
            let toggle_visible_desktop = page
                .locator(".mobile-menu-toggle")
                .is_visible()
                .await
                .expect("Failed to check toggle visibility");
            assert!(
                !toggle_visible_desktop,
                "[{}] Mobile menu toggle should be hidden on desktop viewport",
                browser_name
            );
            let sidebar_visible_desktop = page
                .locator(".sidebar")
                .is_visible()
                .await
                .expect("Failed to check sidebar visibility");
            assert!(
                sidebar_visible_desktop,
                "[{}] Sidebar should be visible on desktop viewport",
                browser_name
            );

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

            // Cat and Dog, both leaf children of Mammal, share a row.
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
        })
    });
}

/// The Flat toggle switches the class grid to an alphabetical tiling:
/// Animal and Cat, alphabetical neighbors, share a row on a 1280px
/// viewport.
#[test]
fn e2e_flat_toggle_tiles_classes_alphabetically() {
    in_every_browser(|browser_name, page| {
        Box::pin(async move {
            page.locator(r#".view-toggle-btn[data-view="flat"]"#)
                .click(None)
                .await
                .expect("Failed to click the Flat toggle");
            expect(page.locator("#class-cards"))
                .to_have_attribute("data-view", "flat")
                .await
                .expect("the Flat toggle switches the card grid to the flat view");
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
        })
    });
}

/// On a 375px viewport the class cards stack in one column and the menu
/// toggle opens the sidebar.
#[test]
fn e2e_mobile_viewport_stacks_cards_and_menu_opens_sidebar() {
    in_every_browser(|browser_name, page| {
        Box::pin(async move {
            page.set_viewport_size(playwright_rs::Viewport {
                width: 375,
                height: 667,
            })
            .await
            .expect("Failed to set mobile viewport");

            let mobile_toggle = page.locator(".mobile-menu-toggle");
            expect(mobile_toggle.clone())
                .to_be_visible()
                .await
                .unwrap_or_else(|e| {
                    panic!("[{browser_name}] Mobile menu toggle should be visible on mobile viewport: {e}")
                });

            let class_cards = page.locator(".class-card");
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

            mobile_toggle
                .click(None)
                .await
                .expect("Failed to click mobile menu toggle");
            expect(page.locator(".sidebar"))
                .to_be_visible()
                .await
                .unwrap_or_else(|e| {
                    panic!("[{browser_name}] Sidebar should be visible after clicking mobile menu toggle: {e}")
                });
        })
    });
}

#[test]
fn e2e_graph_section_ships_the_hover_card() {
    in_every_browser(|browser_name, page| {
        Box::pin(async move {
            let graph_section_count = page
                .locator("#graph-visualization")
                .count()
                .await
                .expect("Failed to count graph section");
            assert!(
                graph_section_count > 0,
                "[{}] Graph visualization section should exist",
                browser_name
            );
            let hover_card = page.locator("#graph-hover-card");
            assert_eq!(
                hover_card
                    .count()
                    .await
                    .expect("Failed to count hover card"),
                1,
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
        })
    });
}

/// The Arrows toggle defaults on, and clicking it flips the viz's own state
/// rather than only the button's styling, then persists to localStorage.
/// Clicks go through the DOM: the control strip sits over the wasm canvas,
/// so pointer actionability is flaky in headless.
#[test]
fn e2e_arrows_toggle_flips_the_viz_and_persists() {
    in_every_browser(|browser_name, page| {
        Box::pin(async move {
            wait_for_graph_viz_ready(page).await.unwrap_or_else(|e| {
                panic!("[{browser_name}] schema graph viz never became ready: {e}")
            });
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

            let toggled = page
                .evaluate_value(
                    r#"(function(){
                        var viz = window.__panschema_viz;
                        var before = viz.node_labels_enabled() + ':' + viz.show_arrows();
                        document.getElementById('graph-labels-nodes').click();
                        document.getElementById('graph-arrows').click();
                        var after = viz.node_labels_enabled() + ':' + viz.show_arrows();
                        return before + ' -> ' + after;
                    })()"#,
                )
                .await
                .unwrap_or_default();
            assert!(
                toggled.contains("true:true -> false:false"),
                "[{}] the label and arrow toggles should flip viz state; got: {}",
                browser_name,
                toggled
            );
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
        })
    });
}

/// The Legend control renders the key onto its own canvas (a non-zero
/// backing-store width means the wasm `render_legend` export sized and
/// drew), defaults open on the fixture's 1280px viewport, and toggles and
/// persists. The glyph pixels cannot be DOM-asserted.
#[test]
fn e2e_legend_renders_defaults_open_and_persists() {
    in_every_browser(|browser_name, page| {
        Box::pin(async move {
            wait_for_graph_viz_ready(page).await.unwrap_or_else(|e| {
                panic!("[{browser_name}] schema graph viz never became ready: {e}")
            });
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
        })
    });
}

/// The static fallback shows the canvas even before wasm; once the viz is
/// up the loading indicator is gone.
#[test]
fn e2e_graph_canvas_shows_and_loading_hides_once_the_viz_is_up() {
    in_every_browser(|browser_name, page| {
        Box::pin(async move {
            let canvas = page.locator("#graph-canvas");
            assert!(
                canvas.count().await.expect("Failed to count canvas") > 0,
                "[{}] Graph canvas should exist",
                browser_name
            );
            expect(canvas).to_be_visible().await.unwrap_or_else(|e| {
                panic!("[{browser_name}] Graph canvas should become visible: {e}")
            });
            wait_for_graph_viz_ready(page).await.unwrap_or_else(|e| {
                panic!("[{browser_name}] schema graph viz never became ready: {e}")
            });
            expect(page.locator("#graph-loading"))
                .to_be_hidden()
                .await
                .unwrap_or_else(|e| {
                    panic!("[{browser_name}] Loading indicator should be hidden after graph initializes: {e}")
                });
        })
    });
}

/// The graph badge reads `nodes / edges`, the format every graph count
/// uses, with the spelled-out reading carried as a label.
#[test]
fn e2e_graph_badge_reads_nodes_over_edges_with_a_label() {
    in_every_browser(|browser_name, page| {
        Box::pin(async move {
            wait_for_graph_viz_ready(page).await.unwrap_or_else(|e| {
                panic!("[{browser_name}] schema graph viz never became ready: {e}")
            });
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
        })
    });
}

/// The graph container keeps the writer's default 16:8 aspect within 5%.
/// The ratio is derived rather than hard-coded, so a change to the writer's
/// default only moves this constant.
#[test]
fn e2e_graph_container_keeps_the_writer_aspect() {
    in_every_browser(|browser_name, page| {
        Box::pin(async move {
            let graph_box = page
                .locator(".graph-container")
                .bounding_box()
                .await
                .expect("Failed to query graph container box")
                .expect("Graph container should have a bounding box");
            let ratio = graph_box.width / graph_box.height;
            let target = 16.0_f64 / 8.0;
            assert!(
                (ratio - target).abs() / target < 0.05,
                "[{}] Graph container aspect ratio should be ~16:8 (±5%);
                 got w={}, h={}, ratio={:.3} (target {:.3})",
                browser_name,
                graph_box.width,
                graph_box.height,
                ratio,
                target
            );
        })
    });
}

#[test]
fn e2e_graph_data_carries_node_labels_and_edge_types() {
    in_every_browser(|browser_name, page| {
        Box::pin(async move {
            for (expr, claim) in [
                (
                    "window.__PANSCHEMA_GRAPH_DATA__.nodes.every(n => n.label && n.label.length > 0)",
                    "All nodes should have labels",
                ),
                (
                    "window.__PANSCHEMA_GRAPH_DATA__.edges.every(e => e.edge_type && e.edge_type.length > 0)",
                    "All edges should have edge_type for labeling",
                ),
                (
                    "window.__PANSCHEMA_GRAPH_DATA__.nodes.some(n => n.label === 'Animal')",
                    "Should have node with label 'Animal'",
                ),
                (
                    "window.__PANSCHEMA_GRAPH_DATA__.edges.some(e => e.edge_type === 'subclass_of')",
                    "Should have subclass_of edges",
                ),
            ] {
                let holds = page
                    .evaluate_value(expr)
                    .await
                    .unwrap_or_else(|e| panic!("Failed to evaluate `{expr}`: {e}"));
                assert!(
                    holds.contains("true"),
                    "[{}] {}; `{}` gave {}",
                    browser_name,
                    claim,
                    expr,
                    holds
                );
            }
        })
    });
}

#[test]
fn e2e_schema_graph_sidebar_link_navigates_to_the_section() {
    in_every_browser(|browser_name, page| {
        Box::pin(async move {
            let graph_sidebar_link = page.locator(".sidebar-link[href='#graph-visualization']");
            assert!(
                graph_sidebar_link
                    .count()
                    .await
                    .expect("Failed to count graph sidebar link")
                    > 0,
                "[{}] Schema Graph navigation link should exist in sidebar",
                browser_name
            );
            graph_sidebar_link
                .click(None)
                .await
                .expect("Failed to click Schema Graph sidebar link");
            wait_until_ready(page, "location.hash === '#graph-visualization'")
                .await
                .unwrap_or_else(|e| panic!("[{browser_name}] URL hash should be #graph-visualization after clicking sidebar link: {e}"));
        })
    });
}

/// The zoom and reset buttons act on the viz without raising the error
/// overlay, the canvas has a real backing store, and the layout picker
/// offers every implemented layout, auto-detecting `sgd` for the
/// mixed-edge reference fixture, with the reserved layouts disabled.
#[test]
fn e2e_graph_controls_zoom_reset_and_layout_picker() {
    in_every_browser(|browser_name, page| {
        Box::pin(async move {
            wait_for_graph_viz_ready(page).await.unwrap_or_else(|e| {
                panic!("[{browser_name}] schema graph viz never became ready: {e}")
            });
            page.evaluate::<(), ()>(
                // Driver 1.62.1+: scrollIntoView() evaluates to a result object
                // ({interrupted: bool}); void keeps the expression unit-shaped.
                "void document.getElementById('graph-visualization').scrollIntoView()",
                None,
            )
            .await
            .expect("Failed to scroll to graph section");

            for id in ["#graph-zoom-in", "#graph-zoom-out", "#graph-reset"] {
                page.locator(id)
                    .click(None)
                    .await
                    .unwrap_or_else(|e| panic!("Failed to click {id}: {e}"));
            }
            expect(page.locator("#graph-error"))
                .to_be_hidden()
                .await
                .unwrap_or_else(|e| {
                    panic!(
                        "[{browser_name}] Error overlay should not appear after zoom and reset: {e}"
                    )
                });

            for dimension in ["width", "height"] {
                let value = page
                    .evaluate_value(&format!(
                        "document.getElementById('graph-canvas').{dimension}"
                    ))
                    .await
                    .unwrap_or_else(|e| panic!("Failed to get canvas {dimension}: {e}"));
                let pixels: u64 = value.trim().trim_matches('"').parse().unwrap_or(0);
                assert!(
                    pixels > 0,
                    "[{}] Canvas should have non-zero {}, got: {}",
                    browser_name,
                    dimension,
                    value
                );
            }

            let layout_select = page.locator("#graph-layout-select");
            assert!(
                layout_select
                    .count()
                    .await
                    .expect("Failed to count layout picker")
                    > 0,
                "[{}] Layout picker <select> should exist",
                browser_name
            );
            // The writer emits the `auto` not-pinned default, so the picker's
            // initial value is the density-based recommendation. The reference
            // fixture is mixed-edge (subclass_of + domain/range/inverse), below
            // the inheritance threshold, so it auto-detects to `sgd`; an
            // `is_a`-heavy schema would recommend hierarchical.
            let initial_value = layout_select
                .input_value(None)
                .await
                .expect("Failed to read layout select value");
            assert_eq!(
                initial_value, "sgd",
                "[{}] mixed-edge reference fixture should auto-detect to sgd; got `{}`",
                browser_name, initial_value
            );
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
                assert_eq!(
                    opt.count().await.expect("Failed to count option"),
                    1,
                    "[{}] Picker should expose option for `{}`",
                    browser_name,
                    implemented
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
                assert_eq!(
                    opt.count().await.expect("Failed to count option"),
                    1,
                    "[{}] Picker should expose option for `{}`",
                    browser_name,
                    unimplemented
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
        })
    });
}

/// Clicking a graph node pins its card open (persistent, with a × close
/// button); the old top-right details panel is gone; the × closes the card
/// but keeps the node selected, and `deselect` clears it. Drives a *real*
/// click at the node's canvas
/// position (`node_canvas_pos`) — nodes are canvas-drawn, so there's no DOM
/// element to target.
#[test]
fn e2e_click_pins_node_card_keeping_selection() {
    in_chromium(generate_docs(), |page| {
        Box::pin(async move {
            // The old details panel must be gone entirely.
            let details = page.locator("#graph-details-panel");
            assert_eq!(
                details.count().await.expect("count"),
                0,
                "the details panel should be removed"
            );

            // Wait for the wasm graph to be interrogable (robust to CI load),
            // then click node 0 at its canvas position through the real handler.
            wait_for_graph_viz_ready(page)
                .await
                .expect("schema graph viz never became ready");
            click_schema_node(page, 0).await;
            wait_until_ready(page, "document.getElementById('graph-hover-card').classList.contains('graph-hover-pinned')").await.expect("the clicked node's card never pinned");

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
            assert!(
                card.is_visible().await.expect("visible"),
                "pinned card should be visible"
            );
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
            expect(page.locator("#graph-hover-card"))
                .to_be_hidden()
                .await
                .expect("card should hide after ×");
            let sel_after = page
                .evaluate_value("window.__panschema_viz.selected_node_index()")
                .await
                .unwrap_or_default();
            assert!(
                !sel_after.contains("-1"),
                "node should stay selected after ×; got {sel_after}"
            );

            // `deselect` clears the selection the click made.
            page.evaluate::<(), ()>("window.__panschema_viz.deselect()", None)
                .await
                .expect("deselect");
            let cleared = page
                .evaluate_value("window.__panschema_viz.selected_node_index()")
                .await
                .unwrap_or_default();
            assert!(
                cleared.contains("-1"),
                "deselect should clear the selection; got {cleared}"
            );
        })
    });
}

/// Hovering an edge shows the triple it stands for — source label, edge
/// type, target label — plus a one-line LinkML gloss for the edge kind.
/// Walks the edges until one whose midpoint is clear of any node is under
/// the cursor, so the assertion reads a genuine edge hover.
#[test]
fn e2e_edge_hover_shows_the_triple_and_its_kind_blurb() {
    in_chromium(generate_docs(), |page| {
        Box::pin(async move {
            wait_for_graph_viz_ready(page)
                .await
                .expect("schema graph viz never became ready");

            let states = page
            .evaluate_value(
                r#"(function(){
                    var viz = window.__panschema_viz;
                    var canvas = document.getElementById('graph-canvas');
                    var card = document.getElementById('graph-hover-card');
                    var rect = canvas.getBoundingClientRect();
                    var dpr = window.devicePixelRatio || 1;
                    var idOf = {};
                    for (var i = 0; i < viz.node_count(); i++) {
                        idOf[JSON.parse(viz.get_node_details(i)).id] = i;
                    }
                    for (var e = 0; e < viz.edge_count(); e++) {
                        var d = JSON.parse(viz.get_edge_details(e));
                        var s = idOf[d.source.id], t = idOf[d.target.id];
                        if (s === undefined || t === undefined) continue;
                        var ps = viz.node_canvas_pos(s), pt = viz.node_canvas_pos(t);
                        canvas.dispatchEvent(new MouseEvent('mousemove', {
                            clientX: rect.left + (ps[0] + pt[0]) / 2 / dpr,
                            clientY: rect.top + (ps[1] + pt[1]) / 2 / dpr,
                            bubbles: true}));
                        if (viz.hovered_edge_index() !== e || viz.hovered_node_index() >= 0) continue;
                        var text = card.textContent || '';
                        return ['edge:' + e,
                                'visible:' + (card.style.display === 'block'),
                                'src:' + (text.indexOf(d.source.label) >= 0),
                                'type:' + (text.indexOf(d.type) >= 0),
                                'tgt:' + (text.indexOf(d.target.label) >= 0),
                                'blurb:' + !!card.querySelector('.graph-hover-description')].join(' ');
                    }
                    return 'no-edge-hovered';
                })()"#,
            )
            .await
            .unwrap_or_default();
            assert!(
                states.contains("visible:true")
                    && states.contains("src:true")
                    && states.contains("type:true")
                    && states.contains("tgt:true")
                    && states.contains("blurb:true"),
                "an edge hover shows source, type, target, and the kind blurb; got: {states}"
            );
        })
    });
}

#[test]
fn e2e_node_hover_reuses_the_doc_card_in_full_mode() {
    in_chromium(generate_docs(), |page| {
        Box::pin(async move {
            wait_for_graph_viz_ready(page)
                .await
                .expect("schema graph viz never became ready");

            let states = page
            .evaluate_value(
                r#"(function(){
                    var viz = window.__panschema_viz;
                    var canvas = document.getElementById('graph-canvas');
                    var card = document.getElementById('graph-hover-card');
                    var content = document.getElementById('graph-hover-content');
                    var rect = canvas.getBoundingClientRect();
                    var dpr = window.devicePixelRatio || 1;
                    var pos = viz.node_canvas_pos(0);
                    canvas.dispatchEvent(new MouseEvent('mousemove', {
                        clientX: rect.left + pos[0] / dpr,
                        clientY: rect.top + pos[1] / dpr,
                        bubbles: true}));
                    var d = JSON.parse(viz.get_node_details(0));
                    var colon = d.id.indexOf(':');
                    var doc = document.getElementById(d.id.slice(0, colon) + '-' + d.id.slice(colon + 1));
                    return ['hovered:' + viz.hovered_node_index(),
                            'visible:' + (card.style.display === 'block'),
                            'full:' + card.classList.contains('graph-hover-full'),
                            'docCard:' + !!doc,
                            'same:' + (!!doc && content.innerHTML === doc.innerHTML)].join(' ');
                })()"#,
            )
            .await
            .unwrap_or_default();
            assert!(
                states.contains("hovered:0")
                    && states.contains("visible:true")
                    && states.contains("full:true")
                    && states.contains("docCard:true")
                    && states.contains("same:true"),
                "a node hover shows its doc card in full mode; got: {states}"
            );
        })
    });
}

/// A pinned card can be dragged by its handle to a new position, so it can
/// be moved off nodes the reader wants to inspect. Pins node 0, drags the
/// `#graph-hover-drag` grip, and asserts the card's top-left moved to the
/// handler-computed target (drag offset applied, viewport-clamped).
#[test]
fn e2e_pinned_card_is_draggable_by_its_handle() {
    in_chromium(generate_docs(), |page| {
        Box::pin(async move {
            // Wait for the wasm graph to be interrogable, then pin node 0.
            wait_for_graph_viz_ready(page)
                .await
                .expect("schema graph viz never became ready");
            click_schema_node(page, 0).await;
            wait_until_ready(page, "document.getElementById('graph-hover-card').classList.contains('graph-hover-pinned')").await.expect("the clicked node's card never pinned");

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
        })
    });
}

/// Hovering a rule entry in a slot card highlights the rule's participant
/// nodes on the graph (its trigger/governed slots and owning class), and
/// moving off clears it. Asserts the highlight *logic* via the viz state
/// (canvas pixels aren't readable); the amber ring is the visual layer.
#[test]
fn e2e_hovering_a_rule_entry_highlights_participant_nodes() {
    in_chromium(
        generate_site("tests/fixtures/rules_graph.yaml", &[]),
        |page| {
            Box::pin(async move {
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
                wait_for_graph_viz_ready(page)
                    .await
                    .expect("graph viz never became ready");

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
            })
        },
    );
}

/// Every node a class rule touches — a trigger *or* governed slot, and the
/// class that declares the rule — wears a persistent amber ring on the
/// graph at rest. Asserts the flagged set covers all of them (not just the
/// governed slot) and that the amber ring actually paints in the canvas
/// pixels around a rule node — with no hover active, the only amber is the
/// persistent ring.
#[test]
fn e2e_rule_touched_nodes_draw_a_persistent_amber_ring() {
    in_chromium(
        generate_site("tests/fixtures/rules_graph.yaml", &[]),
        |page| {
            Box::pin(async move {
                // Poll until the wasm graph is loaded and laid out — a fixed sleep
                // flakes as `no-viz` under CI load.
                wait_for_graph_viz_ready(page)
                    .await
                    .expect("graph viz never became ready");

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
            })
        },
    );
}

/// A class grounded via `subclass_of` into an upstream ontology draws a muted
/// external node in the schema graph. Asserts the viz reports a node of type
/// `External` and that its muted grey fill actually paints on the canvas —
/// distinct from the blue class nodes.
#[test]
fn e2e_external_grounding_paints_a_muted_node() {
    in_chromium(
        generate_site("tests/fixtures/external_grounding.yaml", &[]),
        |page| {
            Box::pin(async move {
                wait_for_graph_viz_ready(page)
                    .await
                    .expect("graph viz never became ready");

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
            })
        },
    );
}

/// The "Groundings" control shows only when the graph has external nodes, and
/// clicking it hides them. Asserts the button is visible for a grounded schema
/// and that a click flips external visibility off and clears the muted node's
/// pixels from the canvas.
#[test]
fn e2e_groundings_toggle_hides_external_nodes() {
    in_chromium(
        generate_site("tests/fixtures/external_grounding.yaml", &[]),
        |page| {
            Box::pin(async move {
                wait_for_graph_viz_ready(page)
                    .await
                    .expect("graph viz never became ready");

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
            })
        },
    );
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
        let cache_scratch = tempfile::tempdir().expect("tempdir");
        let cache_root = cache_scratch.path();
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

        let site = tempfile::tempdir().expect("tempdir");
        let output_dir = site.path();
        let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
            .env("PANSCHEMA_CACHE_ROOT", cache_root)
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

        let playwright = Playwright::launch().await.expect("playwright");
        let (_browser, page) = open_served_page(&playwright, "chromium", output_dir).await;
        page.goto(&format!("{SITE_ORIGIN}/index.html"), None)
            .await
            .expect("goto");

        wait_for_graph_viz_ready(&page).await.expect("graph viz never became ready");

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
    in_chromium(
        generate_site("tests/fixtures/instance_graph.ttl", &[]),
        |page| {
            Box::pin(async move {
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
                wait_until_ready(page, "!!window.__panschema_instance_viz")
                    .await
                    .expect("instance graph viz never became ready");

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
            })
        },
    );
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
        let scratch = tempfile::tempdir().expect("tempdir");
        let consumer = scratch.path();
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
            .current_dir(consumer)
            .status()
            .expect("run panschema");
        assert!(status.success(), "composed generate failed");
        let output_dir = consumer.join("docs");

        let playwright = Playwright::launch().await.expect("playwright");
        let (_browser, page) = open_served_page(&playwright, "chromium", &output_dir).await;
        page.goto(&format!("{SITE_ORIGIN}/index.html"), None)
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
        wait_until_ready(&page, "!!window.PanschemaGraphShell").await.expect("the graph shell script must load on a data-only page");
        wait_until_ready(&page, "!!window.__panschema_instance_viz").await.expect("instance graph viz never became ready on the data-only page");
    });
}

/// Several curated instance graphs share the schema page: the selector names
/// each, and picking one swaps the cards, the provenance, and the rendered
/// graph together. A selector that moved the canvas but left the cards
/// describing the previous dataset is the defect this pins down.
#[test]
fn e2e_instance_dataset_selector_switches_cards_and_graph() {
    in_chromium(
        generate_site(
            "tests/fixtures/wine_catalog.yaml",
            &[
                "--instances",
                "tests/fixtures/wine_instances_preview.yaml",
                "--instances",
                "tests/fixtures/wine_instances.yaml",
            ],
        ),
        |page| {
            Box::pin(async move {
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

                // A swap while a card is up: the card resets with the dataset, and
                // the next hover shows the new dataset's node, not the old one's
                // cached under the same index. Activating the tab from script keeps
                // the pointer on the canvas, as a keyboard switch does.
                wait_until_ready(page, "!!window.__panschema_instance_viz")
                    .await
                    .expect("instance graph viz never became ready");
                let swap = page
            .evaluate_value(
                r#"(function(){
                    var viz = window.__panschema_instance_viz;
                    var canvas = document.getElementById('instance-graph-canvas');
                    var card = document.getElementById('instance-graph-hover-card');
                    var rect = canvas.getBoundingClientRect();
                    var dpr = window.devicePixelRatio || 1;
                    function hoverNode0() {
                        var pos = window.__panschema_instance_viz.node_canvas_pos(0);
                        canvas.dispatchEvent(new MouseEvent('mousemove',
                            {clientX: rect.left + pos[0] / dpr, clientY: rect.top + pos[1] / dpr, bubbles: true}));
                    }
                    hoverNode0();
                    var out = ['before:' + (card.style.display === 'block')];
                    document.querySelector(".instance-dataset-tab[data-instance-dataset='1']").click();
                    out.push('afterSwap:' + (card.style.display === 'block'));
                    hoverNode0();
                    out.push('hoverAfter:' + (card.style.display === 'block'));
                    out.push('stale:' + ((card.textContent || '').indexOf('previewWine') >= 0));
                    document.querySelector(".instance-dataset-tab[data-instance-dataset='0']").click();
                    return out.join(' ');
                })()"#,
            )
            .await
            .unwrap_or_default();
                assert!(
                    swap.contains("before:true") && swap.contains("afterSwap:false"),
                    "switching datasets clears the hover card; got: {swap}"
                );
                assert!(
                    swap.contains("hoverAfter:true") && swap.contains("stale:false"),
                    "the next hover renders the new dataset's node; got: {swap}"
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
                wait_until_ready(page, "!!window.__panschema_instance_viz")
                    .await
                    .expect("instance graph viz never became ready");
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
            })
        },
    );
}

/// The instance graph offers the schema graph's inspection affordances:
/// a hover detail card (the only surface for a shared value's usage count)
/// and the toolbar toggles, wired once through the shared shell.
#[test]
fn e2e_instance_graph_has_hover_card_and_toolbar_parity() {
    in_chromium(
        generate_site(
            "tests/fixtures/typed_wine.yaml",
            &["--instances", "tests/fixtures/typed_wine_instances.yaml"],
        ),
        |page| {
            Box::pin(async move {
                wait_until_ready(page, "!!window.__panschema_instance_viz")
                    .await
                    .expect("instance graph viz never became ready");
                wait_for_graph_viz_ready(page)
                    .await
                    .expect("schema graph viz never became ready");

                let wasm_fetches = page
                    .evaluate_value(
                        "String(performance.getEntriesByType('resource')\
                 .filter(r => r.name.includes('panschema_viz_bg.wasm')).length)",
                    )
                    .await
                    .unwrap_or_default();
                assert_eq!(
                    wasm_fetches.trim().trim_matches('"'),
                    "1",
                    "a page with both graphs must fetch the wasm exactly once"
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

                // The keyboard gap is closed: pressing L flips label state on the
                // instance canvas exactly as the schema graph's L key does. The
                // toggles above left node labels off; L (all labels) drives the
                // viz, proving the shared toolbar's (L) hint is honest here.
                let keyed = page
                    .evaluate_value(
                        r#"(function(){
                    var viz = window.__panschema_instance_viz;
                    var container = document.querySelector('.instance-graph-container');
                    // Scoped to the hovered graph: L fires while the pointer
                    // is over the graph, and is inert once it leaves — so on
                    // a two-graph page a keypress never drives both.
                    container.dispatchEvent(new MouseEvent('mouseenter'));
                    var before = viz.labels_enabled();
                    document.dispatchEvent(new KeyboardEvent('keydown', {key: 'l'}));
                    var whileHovered = viz.labels_enabled();
                    container.dispatchEvent(new MouseEvent('mouseleave'));
                    document.dispatchEvent(new KeyboardEvent('keydown', {key: 'l'}));
                    var afterLeave = viz.labels_enabled();
                    return before + ' -> ' + whileHovered + ' -> ' + afterLeave;
                })()"#,
                    )
                    .await
                    .unwrap_or_default();
                assert!(
                    keyed.contains("true -> false -> false"),
                    "L toggles labels while hovering the graph and is inert once the              pointer leaves (so a two-graph page never drives both); got: {keyed}"
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
            })
        },
    );
}

/// Grabbing a node on the instance canvas drags THE NODE, as on the schema
/// canvas — not the camera. A pan moves every node together; a node drag
/// changes the dragged node's position relative to the others.
#[test]
fn e2e_instance_graph_nodes_are_draggable() {
    in_chromium(
        generate_site(
            "tests/fixtures/typed_wine.yaml",
            &["--instances", "tests/fixtures/typed_wine_instances.yaml"],
        ),
        |page| {
            Box::pin(async move {
                wait_until_ready(page, "!!window.__panschema_instance_viz")
                    .await
                    .expect("instance graph viz never became ready");

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
            })
        },
    );
}

/// Clicking a node on the instance canvas selects it, as on the schema
/// canvas: the card pins open (surviving the cursor moving away) until the
/// node is deselected by clicking empty space.
#[test]
fn e2e_instance_graph_click_pins_the_card_and_empty_space_deselects() {
    in_chromium(
        generate_site(
            "tests/fixtures/typed_wine.yaml",
            &["--instances", "tests/fixtures/typed_wine_instances.yaml"],
        ),
        |page| {
            Box::pin(async move {
                wait_until_ready(page, "!!window.__panschema_instance_viz")
                    .await
                    .expect("instance graph viz never became ready");

                let states = page
                    .evaluate_value(
                        &r#"(function(){
                    var viz = window.__panschema_instance_viz;
                    var canvas = document.getElementById('instance-graph-canvas');
                    var card = document.getElementById('instance-graph-hover-card');
                    var rect = canvas.getBoundingClientRect();
                    var dpr = window.devicePixelRatio || 1;
                    function cardVisible() {
                        return card && card.style.display !== 'none' && card.style.display !== '';
                    }
                    __CLICK_AT__
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
                })()"#
                            .replace("__CLICK_AT__", CLICK_AT_JS),
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
            })
        },
    );
}

#[test]
fn e2e_instance_pinned_card_closes_by_its_button_keeping_selection() {
    in_chromium(
        generate_site(
            "tests/fixtures/typed_wine.yaml",
            &["--instances", "tests/fixtures/typed_wine_instances.yaml"],
        ),
        |page| {
            Box::pin(async move {
                wait_until_ready(page, "!!window.__panschema_instance_viz")
                    .await
                    .expect("instance graph viz never became ready");

                let states = page
            .evaluate_value(
                &r#"(function(){
                    var viz = window.__panschema_instance_viz;
                    var canvas = document.getElementById('instance-graph-canvas');
                    var card = document.getElementById('instance-graph-hover-card');
                    var close = document.getElementById('instance-graph-hover-close');
                    var rect = canvas.getBoundingClientRect();
                    var dpr = window.devicePixelRatio || 1;
                    __CLICK_AT__
                    var pos = viz.node_canvas_pos(0);
                    clickAt(rect.left + pos[0] / dpr, rect.top + pos[1] / dpr);
                    var out = ['sel:' + viz.selected_node_index(),
                               'pinned:' + card.classList.contains('graph-hover-pinned'),
                               'close:' + !!(close && getComputedStyle(close).display !== 'none')];
                    if (close) close.click();
                    out.push('cardAfterClose:' + (card.style.display === 'block'));
                    out.push('selAfterClose:' + viz.selected_node_index());
                    var onNode = {clientX: rect.left + pos[0] / dpr, clientY: rect.top + pos[1] / dpr, bubbles: true};
                    canvas.dispatchEvent(new MouseEvent('mousemove', onNode));
                    out.push('hoverAfterClose:' + (card.style.display === 'block'));
                    clickAt(rect.left + 3, rect.top + 3);
                    canvas.dispatchEvent(new MouseEvent('mousemove', onNode));
                    out.push('hoverAfterDeselect:' + (card.style.display === 'block'));
                    return out.join(' ');
                })()"#
                .replace("__CLICK_AT__", CLICK_AT_JS),
            )
            .await
            .unwrap_or_default();
                assert!(
                    states.contains("sel:0")
                        && states.contains("pinned:true")
                        && states.contains("close:true"),
                    "clicking a node pins its card with a visible close button; got: {states}"
                );
                assert!(
                    states.contains("cardAfterClose:false") && states.contains("selAfterClose:0"),
                    "the close button hides the card and keeps the node selected; got: {states}"
                );
                assert!(
                    states.contains("hoverAfterClose:true")
                        && states.contains("hoverAfterDeselect:true"),
                    "closing the card locks nothing: the still-selected node hovers normally, before and after deselect; got: {states}"
                );
            })
        },
    );
}

/// A click is a click even when the pointer jitters. A trackpad click moves
/// a pixel or two between press and release, and treating any movement at
/// all as a drag meant selection silently failed for real hands while
/// passing for synthetic events dispatched at one coordinate.
#[test]
fn e2e_instance_graph_selection_survives_pointer_jitter_and_escape_deselects() {
    in_chromium(
        generate_site(
            "tests/fixtures/typed_wine.yaml",
            &["--instances", "tests/fixtures/typed_wine_instances.yaml"],
        ),
        |page| {
            Box::pin(async move {
                wait_until_ready(page, "!!window.__panschema_instance_viz")
                    .await
                    .expect("instance graph viz never became ready");

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
                    states.contains("afterEscape_card:false")
                        && states.contains("afterEscape_sel:-1"),
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
            })
        },
    );
}

/// The instance graph is typed: individuals wear their class's circle and
/// colour, and each enum value in use is one shared diamond node the
/// choosing individuals link to — checked on the real rendered page, since
/// writer↔viz wire changes can pass every Rust test while the browser
/// renders nothing.
#[test]
fn e2e_typed_instance_graph_renders_class_symbols_and_shared_values() {
    in_chromium(
        generate_site(
            "tests/fixtures/typed_wine.yaml",
            &["--instances", "tests/fixtures/typed_wine_instances.yaml"],
        ),
        |page| {
            Box::pin(async move {
                wait_until_ready(page, "!!window.__panschema_instance_viz")
                    .await
                    .expect("instance graph viz never became ready");

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
            })
        },
    );
}

/// Each graph's notation key is adaptive: it lists only the node and edge
/// kinds that graph actually uses, from one code path serving both canvases.
///
/// **The one test still served over a real listener, deliberately.** It is
/// the only one that serves two sites at once, which is awkward to express
/// as interception — and keeping it bound makes it the control. Every other
/// test reaches its page through `route_service`, so a regression there
/// reds the whole suite at once with nothing to compare against; when this
/// one passes and the intercepted tests fail, the fault is the serving path
/// rather than the app. Do not convert it without leaving some other test
/// on a socket.
#[test]
fn e2e_legends_adapt_to_what_each_graph_contains() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        // wine_catalog declares classes and slots but no enums, so the
        // schema key must not advertise the enum diamond; the instance
        // graph's key must describe individuals and assertions only.
        let site = generate_site("tests/fixtures/wine_catalog.yaml", &["--instances", "tests/fixtures/wine_instances.yaml"]);
        let output_dir = site.path();
        let (listener, port) = bind_ephemeral();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(start_server(output_dir.to_path_buf(), listener, shutdown_rx));

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&format!("{}/index.html", base_url), None)
            .await
            .expect("goto");

        wait_until_ready(
                &page,
                "!!window.__panschema_instance_viz && !!window.__panschema_viz"
            ).await.expect("both graph visualizations should come up");

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
        let attr_only_site = generate_site("tests/fixtures/scoped_estate.yaml", &[]);
        let attr_only_dir = attr_only_site.path();
        let (attr_listener, attr_port) = bind_ephemeral();
        let (attr_shutdown_tx, attr_shutdown_rx) = oneshot::channel();
        let attr_server = tokio::spawn(start_server(
            attr_only_dir.to_path_buf(),
            attr_listener,
            attr_shutdown_rx,
        ));
        let attr_page = browser.new_page().await.expect("attributes-only page");
        attr_page
            .goto(&format!("http://127.0.0.1:{attr_port}/index.html"), None)
            .await
            .expect("goto attributes-only docs");
        wait_for_graph_viz_ready(&attr_page).await.expect("attributes-only schema graph should become ready");
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
    });
}

/// The instance graph is the same explorable component as the schema graph:
/// it fills its viewport instead of clustering in a corner, offers the layout
/// picker, and focuses the hovered node's neighborhood.
#[test]
fn e2e_instance_graph_is_explorable_like_the_schema_graph() {
    in_chromium(
        generate_site(
            "tests/fixtures/wine_catalog.yaml",
            &["--instances", "tests/fixtures/wine_instances.yaml"],
        ),
        |page| {
            Box::pin(async move {
                wait_until_ready(page, "!!window.__panschema_instance_viz")
                    .await
                    .expect("instance graph viz never became ready");

                // Viewport fill: after the layout settles and the camera fits, the
                // painted content spans a substantial share of the canvas rather than
                // clustering in one corner.
                wait_until_ready(
                    page,
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
                })()"#,
                )
                .await
                .expect("the settled instance graph should fill and center in its viewport");

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
                wait_until_ready(
                    page,
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
                })()"#,
                )
                .await
                .expect("reset should re-fit and re-center the panned-away graph");

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
                assert!(
                    switched.contains("changed"),
                    "picker change failed: {switched}"
                );
                wait_until_ready(
                page,
                "window.__panschema_instance_viz && window.__panschema_instance_viz !== window.__instance_viz_before"
            ).await.expect("choosing a layout should re-create the instance viz");

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
            })
        },
    );
}

/// The `generate --instances` path renders a LinkML instance-data file as the
/// instance graph — the schema declares no OWL individuals, so the A-box comes
/// entirely from the data file — and its own canvas paints the
/// class-colored individual nodes.
#[test]
fn e2e_instance_graph_renders_from_linkml_data() {
    in_chromium(
        generate_site(
            "tests/fixtures/wine_catalog.yaml",
            &["--instances", "tests/fixtures/wine_instances.yaml"],
        ),
        |page| {
            Box::pin(async move {
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
                dom_click(page, "a.sidebar-link[href='#individuals']").await;
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

                wait_until_ready(page, "!!window.__panschema_instance_viz")
                    .await
                    .expect("instance graph viz never became ready");

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
            })
        },
    );
}

// Proves the layout auto-default end-to-end: an is_a-heavy schema, with
// no layout pinned and no persisted choice, must initialize the picker to
// `hierarchical` via the wasm density recommendation. The reference
// fixture's picker test (`e2e_graph_controls_zoom_reset_and_layout_picker`)
// asserts the SGD side; this asserts the Hierarchical side, so SGD for a
// real schema is known to be a real recommendation, not a silent fallback.
#[test]
fn e2e_is_a_heavy_schema_auto_defaults_to_hierarchical() {
    in_chromium(generate_site("tests/fixtures/taxonomy.ttl", &[]), |page| {
        Box::pin(async move {
            // Wait for the viz to boot rather than guessing: the picker reads
            // its markup default until the module sets the resolved layout, so a
            // fixed sleep asserts the default on any machine slower than the one
            // the number was picked on.
            wait_until_ready(page, "!!window.__panschema_viz")
                .await
                .expect("the schema viz should boot");
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
        })
    });
}

// Proves the Enumerations and Types HTML sections render in a browser
// (feature 02 slice 18). The reference fixture is OWL and carries no
// enums/types, so this uses a small LinkML fixture that declares one of
// each, then asserts both sections, their cards, and the enum's
// permissible values are present in the rendered DOM.
#[test]
fn e2e_renders_enum_and_type_sections() {
    in_chromium(
        generate_site("tests/fixtures/enum_type.yaml", &[]),
        |page| {
            Box::pin(async move {
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
            })
        },
    );
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
    in_chromium(
        generate_site("tests/fixtures/card_features.yaml", &[]),
        |page| {
            Box::pin(async move {
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
                    person_html.contains("<dt>Examples</dt>")
                        && person_html.contains("Ada Lovelace"),
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
                    membership_html.contains("<dt>Default</dt>")
                        && membership_html.contains("basic"),
                    "slot card shows a Default row with the ifabsent value; got: {membership_html}"
                );
            })
        },
    );
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
    let scratch = tempfile::tempdir().expect("tempdir");
    let fixture_path = scratch.path().join(format!("synthetic_{}.ttl", scale.name));
    fs::write(
        &fixture_path,
        build_synthetic_ttl(scale.connected, scale.isolated),
    )
    .expect("Failed to write synthetic TTL");

    let site = generate_site(fixture_path.to_str().unwrap(), &[]);

    let output_dir = site.path();
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
    serve_site(&page, output_dir).await;

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

    let url = format!("{SITE_ORIGIN}/index.html");
    page.goto(&url, None).await.expect("Failed to navigate");

    // The layout is scored, so capture a settled view: the shell refits the
    // camera on a fixed tick schedule that ends at tick 300, and the camera
    // then eases to the last fit.
    wait_for_graph_viz_ready(&page)
        .await
        .expect("graph viz never became ready for the screenshot");
    wait_until_ready(&page, "window.__panschema_viz_settled()")
        .await
        .expect("the graph never settled after its refit schedule");

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
