//! Browser tests for the installed toolbar asset.
//!
//! The asset is JavaScript that builds DOM at page load. The unit tests in
//! `lib.rs` can only prove the configured entries reached the emitted file
//! — they cannot show that one entry renders a plain link, that several
//! render a menu that opens and lists them, or that the menu closes again.
//! That is what these cover.
//!
//! No mdbook and no server: the page is a minimal stand-in for mdbook's
//! toolbar loaded over `file://`, which is enough because the asset does no
//! fetching and imports nothing.
//!
//! ## Setup
//! Install Playwright browsers matching the bundled version:
//! ```bash
//! npx playwright@1.60.0 install
//! ```

use std::path::Path;

use mdbook_panschema::install;
use panschema::publish::{BookLinkConfig, BookLinkEntry};
use playwright_rs::Playwright;

/// A book dir with the assets installed for `entries`, plus a page that
/// mimics mdbook's toolbar markup closely enough for the asset to find it:
/// the `.menu-bar .left-buttons` container it appends to, and the
/// `path_to_root` global mdbook defines per page.
fn installed_page(entries: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("book.toml"), "[book]\ntitle = \"T\"\n").expect("book.toml");

    let cfg = BookLinkConfig::List(
        entries
            .iter()
            .map(|(p, l)| BookLinkEntry {
                schema_path: p.to_string(),
                label: l.to_string(),
            })
            .collect(),
    );
    install(dir.path(), &cfg).expect("install");

    std::fs::write(
        dir.path().join("index.html"),
        r#"<!doctype html>
<html><head><link rel="stylesheet" href="schema-link.css"></head>
<body>
  <div class="menu-bar"><div class="left-buttons"></div></div>
  <script>var path_to_root = "";</script>
  <script src="schema-link.js"></script>
</body></html>
"#,
    )
    .expect("index.html");
    dir
}

fn page_url(dir: &Path) -> String {
    format!("file://{}/index.html", dir.path_display())
}

/// `Path::display()` returns a `Display`, not a `String`; this keeps the
/// call sites readable.
trait PathDisplay {
    fn path_display(&self) -> String;
}
impl PathDisplay for Path {
    fn path_display(&self) -> String {
        self.display().to_string()
    }
}

/// One schema: the plain anchor a single-schema book has always had, with
/// no menu anywhere. This is the property that lets a book adopt the list
/// form before it has a second schema.
#[test]
fn e2e_one_entry_renders_a_plain_link() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let dir = installed_page(&[("schema/current/", "Wine schema")]);
        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&page_url(dir.path()), None).await.expect("goto");

        let anchors = page.locator("a.schema-docs-button");
        assert_eq!(
            anchors.count().await.expect("count"),
            1,
            "a single entry should render one anchor"
        );
        assert_eq!(
            anchors.get_attribute("href").await.expect("href"),
            Some("schema/current/".to_string()),
            "href should be path_to_root-relative"
        );
        assert_eq!(
            anchors.get_attribute("title").await.expect("title"),
            Some("Wine schema".to_string())
        );
        assert_eq!(
            page.locator(".schema-docs-menu")
                .count()
                .await
                .expect("count"),
            0,
            "one entry must not render a menu"
        );
    });
}

/// Several schemas: one control in the toolbar that opens a list of every
/// entry, in declaration order, each linking to its own page.
#[test]
fn e2e_several_entries_render_a_menu_that_opens() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let dir = installed_page(&[
            ("schema/current/", "Wine schema"),
            ("schema/cqa/current/", "CQ&A contract"),
        ]);
        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&page_url(dir.path()), None).await.expect("goto");

        assert_eq!(
            page.locator(".schema-docs-menu")
                .count()
                .await
                .expect("count"),
            1,
            "two entries should render one menu control"
        );
        let list = page.locator(".schema-docs-menu-list");
        assert!(
            !list.is_visible().await.expect("visible"),
            "the menu should start closed"
        );

        page.locator(".schema-docs-menu .icon-button")
            .click(None)
            .await
            .expect("click toggle");
        assert!(
            list.is_visible().await.expect("visible"),
            "clicking the toggle should open the menu"
        );

        let items = page
            .evaluate_value(
                "Array.from(document.querySelectorAll('.schema-docs-menu-list a'))\
                 .map(a => a.textContent + '|' + a.getAttribute('href')).join(',')",
            )
            .await
            .expect("read menu items");
        assert!(
            items.contains("Wine schema|schema/current/")
                && items.contains("CQ&A contract|schema/cqa/current/"),
            "the menu should list every entry with its own root-relative href; \
             got: {items}"
        );
        assert!(
            items.find("Wine schema").unwrap() < items.find("CQ&A contract").unwrap(),
            "declaration order should survive into the menu; got: {items}"
        );
    });
}

/// The menu is dismissible both ways a reader will try. A control that
/// opens but will not close is worse than no control.
#[test]
fn e2e_menu_closes_on_escape_and_on_outside_click() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let dir = installed_page(&[("a/", "A"), ("b/", "B")]);
        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&page_url(dir.path()), None).await.expect("goto");

        let toggle = page.locator(".schema-docs-menu .icon-button");
        let list = page.locator(".schema-docs-menu-list");

        toggle.click(None).await.expect("open");
        assert!(list.is_visible().await.expect("visible"), "opened");
        let escaped = page
            .evaluate_value(
                "(() => { document.dispatchEvent(new KeyboardEvent('keydown', \
                 {key: 'Escape', bubbles: true})); \
                 return document.querySelector('.schema-docs-menu-list').hidden; })()",
            )
            .await
            .expect("escape");
        assert!(
            escaped.contains("true"),
            "Escape should close the menu; hidden was: {escaped}"
        );

        toggle.click(None).await.expect("reopen");
        assert!(list.is_visible().await.expect("visible"), "reopened");
        page.locator("body")
            .click(None)
            .await
            .expect("outside click");
        assert!(
            !list.is_visible().await.expect("visible"),
            "a click outside should close the menu"
        );
    });
}

/// The toggle is a real button carrying its expanded state, so the control
/// is reachable and legible to a screen reader rather than being a
/// mouse-only affordance.
#[test]
fn e2e_menu_toggle_reports_its_expanded_state() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let dir = installed_page(&[("a/", "A"), ("b/", "B")]);
        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&page_url(dir.path()), None).await.expect("goto");

        let toggle = page.locator(".schema-docs-menu .icon-button");
        assert_eq!(
            toggle.get_attribute("aria-expanded").await.expect("attr"),
            Some("false".to_string()),
            "closed menu should report aria-expanded=false"
        );
        toggle.click(None).await.expect("open");
        assert_eq!(
            toggle.get_attribute("aria-expanded").await.expect("attr"),
            Some("true".to_string()),
            "opening should update aria-expanded"
        );
    });
}
