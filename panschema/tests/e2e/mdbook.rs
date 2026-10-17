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
//! fetching and imports nothing. The one exception is ignored: a check of
//! a site built from a real schema repository's content. Setup is the
//! parent target's: install Playwright browsers matching
//! [`playwright_rs::PLAYWRIGHT_VERSION`].

use std::path::Path;

use panschema::mdbook::{EditionOption, EditionsContext, install, install_toolbar};
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
    format!("file://{}/index.html", dir.display())
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

/// The menu is dismissible both ways a reader will try — Escape and a
/// click outside — and the toggle is a real button carrying its expanded
/// state, so the control is reachable and legible to a screen reader
/// rather than being a mouse-only affordance.
#[test]
fn e2e_menu_dismisses_both_ways_and_toggle_reports_expanded_state() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let dir = installed_page(&[("a/", "A"), ("b/", "B")]);
        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&page_url(dir.path()), None).await.expect("goto");

        let toggle = page.locator(".schema-docs-menu .icon-button");
        let list = page.locator(".schema-docs-menu-list");

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

/// A site with the asset installed for an edition `own` of a book whose
/// current edition is `v0.2`, at its stable path `book/<own>/`, under a
/// `prefix/` directory standing in for a GitHub Pages project path. The
/// page mimics what an edition's page carries: mdbook's toolbar and
/// content markup, `path_to_root`, and the canonical link the editions
/// build injects, which the asset reads to find the site root. `links`
/// are what the build bakes after rewriting them for the edition. With
/// `nested`, the page sits one directory down, as a chapter in a part
/// does. `book/v0.2/` exists so choosing it has somewhere to go.
fn edition_site(own: &str, nested: bool, links: &[(&str, &str)]) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let site = dir.path().join("prefix");
    let edition_dir = site.join("book").join(own);
    let page_dir = if nested {
        edition_dir.join("part")
    } else {
        edition_dir.clone()
    };
    std::fs::create_dir_all(&page_dir).expect("page dir");
    std::fs::create_dir_all(site.join("book/v0.2")).expect("target dir");
    std::fs::write(edition_dir.join("book.toml"), "[book]\ntitle = \"T\"\n").expect("book.toml");

    let mut labels = vec!["v0.2", "v0.1", "main"];
    if !labels.contains(&own) {
        labels.insert(2, own);
    }
    let context = EditionsContext {
        editions: labels
            .iter()
            .map(|label| EditionOption {
                label: label.to_string(),
                unreleased: *label == "main",
            })
            .collect(),
        current: "v0.2".to_string(),
        own: own.to_string(),
    };
    let links: Vec<BookLinkEntry> = links
        .iter()
        .map(|(p, l)| BookLinkEntry {
            schema_path: p.to_string(),
            label: l.to_string(),
        })
        .collect();
    install_toolbar(&edition_dir, &links, Some(&context)).expect("install");

    // The canonical as the editions build writes it: page-relative, each
    // segment percent-encoded.
    let encoded_own = own.replace('#', "%23");
    let (path_to_root, canonical, page) = if nested {
        (
            "../",
            format!("../../../book/{encoded_own}/part/page.html"),
            "page.html",
        )
    } else {
        (
            "",
            format!("../../book/{encoded_own}/index.html"),
            "index.html",
        )
    };
    std::fs::write(
        page_dir.join(page),
        format!(
            r#"<!doctype html>
<html><head>
<link rel="canonical" href="{canonical}">
<link rel="stylesheet" href="{path_to_root}schema-link.css"></head>
<body>
  <div class="menu-bar"><div class="left-buttons"></div></div>
  <div id="mdbook-content" class="content"><main><h1>Chapter</h1></main></div>
  <script>var path_to_root = "{path_to_root}";</script>
  <script src="{path_to_root}schema-link.js"></script>
</body></html>
"#
        ),
    )
    .expect("page");
    let url = format!("file://{}/{page}", page_dir.display()).replace('#', "%23");
    (dir, url)
}

fn edition_page(own: &str) -> (tempfile::TempDir, String) {
    edition_site(own, false, &[])
}

/// The switcher lists the released editions newest first and the
/// unreleased one last, says which is unreleased, and starts on the
/// page's own edition.
#[test]
fn e2e_the_switcher_lists_editions_with_the_pages_own_selected() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (_dir, url) = edition_page("v0.1");
        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&url, None).await.expect("goto");

        let options = page
            .evaluate_value(
                "Array.from(document.querySelectorAll('select.edition-switcher option'))\
                 .map(o => o.value + '|' + o.textContent + '|' + o.selected).join(',')",
            )
            .await
            .expect("read options");
        assert_eq!(
            options, "v0.2|v0.2|false,v0.1|v0.1|true,main|main (unreleased)|false",
            "newest first, the unreleased one last and marked, own selected"
        );
    });
}

#[test]
fn e2e_choosing_an_edition_opens_its_first_page() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (_dir, url) = edition_page("v0.1");
        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&url, None).await.expect("goto");

        page.locator("select.edition-switcher")
            .select_option("v0.2", None)
            .await
            .expect("select");

        page.wait_for_url("**/book/v0.2/", None)
            .await
            .expect("the v0.2 edition's root should open");
    });
}

/// A reader of an older edition is told so and pointed at the current
/// one; a reader of the unreleased edition is told that instead.
#[test]
fn e2e_a_non_current_edition_shows_a_banner_linking_to_the_current() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");

        let (_dir, url) = edition_page("v0.1");
        let page = browser.new_page().await.expect("page");
        page.goto(&url, None).await.expect("goto");
        let banner = page.locator(".edition-banner");
        assert_eq!(banner.count().await.expect("count"), 1);
        let text = banner
            .text_content()
            .await
            .expect("text")
            .unwrap_or_default();
        assert!(
            text.contains("v0.1") && text.contains("v0.2"),
            "the banner should name the page's edition and the current one; got: {text}"
        );
        let href = page
            .evaluate_value("document.querySelector('.edition-banner a').href")
            .await
            .expect("href");
        assert!(
            href.ends_with("/book/v0.2/"),
            "the banner should link to the current edition's root; got: {href}"
        );

        let (_dir, url) = edition_page("main");
        let page = browser.new_page().await.expect("page");
        page.goto(&url, None).await.expect("goto");
        let text = page
            .locator(".edition-banner")
            .text_content()
            .await
            .expect("text")
            .unwrap_or_default();
        assert!(
            text.contains("unreleased"),
            "the unreleased edition's banner should say so; got: {text}"
        );
    });
}

#[test]
fn e2e_the_current_edition_shows_no_banner() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (_dir, url) = edition_page("v0.2");
        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&url, None).await.expect("goto");

        assert_eq!(
            page.locator(".edition-banner")
                .count()
                .await
                .expect("count"),
            0,
            "the current edition needs no banner"
        );
        assert_eq!(
            page.locator("select.edition-switcher")
                .count()
                .await
                .expect("count"),
            1,
            "the switcher is still there"
        );
    });
}

/// From a chapter one directory down, under a project prefix: the schema
/// button lands on the site's schema docs for the edition's release (not
/// inside the edition, where `path_to_root` would put it), and the
/// switcher still finds the site root.
#[test]
fn e2e_a_nested_page_resolves_links_and_editions_from_the_site_root() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (_dir, url) = edition_site("v0.1", true, &[("schema/v0.1.1/", "Schema")]);
        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&url, None).await.expect("goto");

        // The tail, not the whole URL: the browser spells the tempdir's
        // path its own way (`file:///C:/…` for a Windows `C:\…`). Resolved
        // inside the edition instead, it would end in
        // `/book/v0.1/schema/v0.1.1/`.
        let href = page
            .evaluate_value("document.querySelector('a.schema-docs-button').href")
            .await
            .expect("href");
        assert!(
            href.ends_with("/prefix/schema/v0.1.1/"),
            "the schema button resolves against the site root; got {href}"
        );

        page.locator("select.edition-switcher")
            .select_option("v0.2", None)
            .await
            .expect("select");
        page.wait_for_url("**/prefix/book/v0.2/", None)
            .await
            .expect("the v0.2 edition's root under the prefix should open");
    });
}

/// A site that `mdbook-panschema editions` and `panschema publish` built
/// from a real schema repository with a released line (`v0.1`, from
/// `v0.1.1`) and a `main` that has moved on, read in a browser: the
/// switcher, the banners and the schema links on the book side, and the
/// brand links on the schema side, agree about which edition describes
/// which version. The site is not checked in: build it from such a
/// repository with both commands and copy its site root to
/// `target/editions-site`.
#[test]
#[ignore = "needs a site built from a real schema repo at target/editions-site; run with --run-ignored only -E 'test(a_built_editions_site)'"]
fn a_built_editions_site_links_agree_both_ways_in_a_browser() {
    let site = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/editions-site");
    assert!(
        site.join("book/v0.1/index.html").is_file(),
        "no built editions site at {}",
        site.display()
    );
    let site = site.canonicalize().expect("site path");
    let url = |rel: &str| format!("file://{}/{rel}", site.display());
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");

        // The released edition at its stable path is the current one.
        let page = browser.new_page().await.expect("page");
        page.goto(&url("book/v0.1/index.html"), None)
            .await
            .expect("goto");
        let options = page
            .evaluate_value(
                "Array.from(document.querySelectorAll('select.edition-switcher option'))\
                 .map(o => o.value + '|' + o.textContent + '|' + o.selected).join(',')",
            )
            .await
            .expect("read options");
        assert_eq!(options, "v0.1|v0.1|true,main|main (unreleased)|false");
        assert_eq!(
            page.locator(".edition-banner")
                .count()
                .await
                .expect("count"),
            0,
            "the current edition carries no banner"
        );
        let href = page
            .evaluate_value("document.querySelector('a.schema-docs-button').href")
            .await
            .expect("href");
        assert!(
            href.ends_with("/editions-site/schema/v0.1.1/"),
            "the schema button targets the release the edition was built from; got {href}"
        );
        page.locator("select.edition-switcher")
            .select_option("main", None)
            .await
            .expect("select");
        page.wait_for_url("**/editions-site/book/main/", None)
            .await
            .expect("choosing main opens the unreleased edition");

        // The unreleased edition carries main's revision and says so.
        let page = browser.new_page().await.expect("page");
        page.goto(&url("book/main/introduction.html"), None)
            .await
            .expect("goto");
        let text = page
            .evaluate_value("document.body.innerText")
            .await
            .expect("text");
        assert!(
            text.contains("not released yet"),
            "main's own introduction should render; got {text:.200}"
        );
        let banner = page
            .locator(".edition-banner")
            .text_content()
            .await
            .expect("banner")
            .unwrap_or_default();
        assert!(banner.contains("unreleased"), "got: {banner}");
        let banner_href = page
            .evaluate_value("document.querySelector('.edition-banner a').href")
            .await
            .expect("banner href");
        assert!(
            banner_href.ends_with("/editions-site/book/v0.1/"),
            "the banner points at the current edition; got {banner_href}"
        );
        let href = page
            .evaluate_value("document.querySelector('a.schema-docs-button').href")
            .await
            .expect("href");
        assert!(
            href.ends_with("/editions-site/schema/main/"),
            "the unreleased edition links to the edge schema; got {href}"
        );

        // The root copy is the current edition, with its links intact.
        let page = browser.new_page().await.expect("page");
        page.goto(&url("index.html"), None).await.expect("goto");
        assert_eq!(
            page.locator(".edition-banner")
                .count()
                .await
                .expect("count"),
            0
        );
        let selected = page
            .evaluate_value("document.querySelector('select.edition-switcher').value")
            .await
            .expect("selected");
        assert_eq!(selected, "v0.1");
        let href = page
            .evaluate_value("document.querySelector('a.schema-docs-button').href")
            .await
            .expect("href");
        assert!(
            href.ends_with("/editions-site/schema/v0.1.1/"),
            "got {href}"
        );

        // The schema pages link back to the edition that describes them.
        for (version, edition) in [
            ("v0.1.0", "v0.1"),
            ("v0.1.1", "v0.1"),
            ("current", "v0.1"),
            ("main", "main"),
        ] {
            let page = browser.new_page().await.expect("page");
            page.goto(&url(&format!("schema/{version}/index.html")), None)
                .await
                .expect("goto");
            let href = page
                .evaluate_value("document.querySelector('a.site-title').href")
                .await
                .expect("brand href");
            assert!(
                href.ends_with(&format!("/editions-site/book/{edition}/")),
                "schema/{version}/ brands the {edition} edition; got {href}"
            );
        }
    });
}

/// The canonical's path is percent-encoded, so an edition label the URL
/// spells differently from the label (`#` is legal in a git ref and
/// starts a URL fragment) still has its switcher.
#[test]
fn e2e_an_edition_label_the_url_spells_differently_still_gets_the_switcher() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (_dir, url) = edition_page("rc#2");
        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&url, None).await.expect("goto");

        let selected = page
            .evaluate_value("document.querySelector('select.edition-switcher').value")
            .await
            .expect("selected");
        assert_eq!(selected, "rc#2");
    });
}
