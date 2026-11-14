//! Browser tests for the installed toolbar asset.
//!
//! The asset is JavaScript that builds DOM at page load. The unit tests in
//! `lib.rs` can only prove the configured entries reached the emitted file
//! — they cannot show that one entry renders a plain link, that several
//! render a menu that opens and lists them, or that the menu closes again.
//! That is what these cover.
//!
//! Most use no mdbook and no server: the page is a minimal stand-in for
//! mdbook's toolbar loaded over `file://`, which is enough because the
//! asset does no fetching and imports nothing. One builds a whole site
//! from a scratch repository with the real `mdbook` and `panschema
//! publish`, so it needs `mdbook` on `PATH`. Setup is otherwise the parent
//! target's: install Playwright browsers matching
//! [`playwright_rs::PLAYWRIGHT_VERSION`].

use std::path::Path;
use std::process::Command;

use panschema::mdbook::{EditionOption, EditionsContext, ToolbarLink, install, install_toolbar};
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

/// One edition as the build bakes it for the fixture site: `3` covers no
/// release and holds only the edge, `2` covers `v0.3.0` and `v0.3.1` and
/// holds a pre-release newer than both, and `1` covers `v0.1.0` to
/// `v0.2.0` with a pre-release between. Any other id is an edition of one
/// release, named by its id.
fn baked_edition(id: &str) -> EditionOption {
    let (name, releases, versions): (&str, Option<(&str, &str)>, &[&str]) = match id {
        "3" => ("Third edition", None, &["main"]),
        "2" => (
            "Second edition",
            Some(("v0.3.0", "v0.3.1")),
            &["v0.3.2-rc.1", "v0.3.1", "v0.3.0"],
        ),
        "1" => (
            "First edition",
            Some(("v0.1.0", "v0.2.0")),
            &["v0.2.0", "v0.2.0-rc.1", "v0.1.0"],
        ),
        other => (other, Some(("v0.2.5", "v0.2.5")), &["v0.2.5"]),
    };
    EditionOption {
        id: id.to_string(),
        name: name.to_string(),
        releases: releases.map(|(first, newest)| (first.to_string(), newest.to_string())),
        versions: versions.iter().map(|v| v.to_string()).collect(),
    }
}

/// A link as the editions build bakes it: `versions_root` is set for an
/// entry that aliased `current/`.
fn link(schema_path: &str, label: &str, versions_root: Option<&str>) -> ToolbarLink {
    ToolbarLink {
        schema_path: schema_path.to_string(),
        label: label.to_string(),
        versions_root: versions_root.map(str::to_string),
    }
}

/// The link the build bakes on a page of edition `own` for an entry that
/// aliased `current/` under `root`: it targets the version the edition's
/// book describes, its newest release or the edge while it has none.
fn alias(own: &str, label: &str, root: &str) -> ToolbarLink {
    let described = baked_edition(own)
        .releases
        .map_or("main".to_string(), |(_, newest)| newest);
    link(&format!("{root}{described}/"), label, Some(root))
}

/// A site with the asset installed for an edition `own` of a book whose
/// editions are `3` (unreleased), `2` (current) and `1`, newest first as
/// the build bakes them, at its stable path `book/<own>/`, under a
/// `prefix/` directory standing in for a GitHub Pages project path. The
/// page mimics what an edition's page carries: mdbook's toolbar and
/// content markup, `path_to_root`, and the canonical link the editions
/// build injects, which the asset reads to find the site root. `links`
/// are what the build bakes after rewriting them for the edition. With
/// `nested`, the page sits one directory down, as a chapter in a part
/// does. `book/2/` exists so choosing it has somewhere to go.
fn edition_site(own: &str, nested: bool, links: &[ToolbarLink]) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let site = dir.path().join("prefix");
    let edition_dir = site.join("book").join(own);
    let page_dir = if nested {
        edition_dir.join("part")
    } else {
        edition_dir.clone()
    };
    std::fs::create_dir_all(&page_dir).expect("page dir");
    std::fs::create_dir_all(site.join("book/2")).expect("target dir");
    std::fs::write(edition_dir.join("book.toml"), "[book]\ntitle = \"T\"\n").expect("book.toml");

    let mut ids = vec!["3", "2", "1"];
    if !ids.contains(&own) {
        ids.insert(2, own);
    }
    let context = EditionsContext {
        editions: ids.iter().map(|id| baked_edition(id)).collect(),
        current: "2".to_string(),
        own: own.to_string(),
    };
    install_toolbar(&edition_dir, links, Some(&context)).expect("install");

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

/// The switcher lists the editions by name in the order the build bakes
/// them, says which is unreleased, and starts on the page's own edition.
#[test]
fn e2e_the_switcher_lists_editions_with_the_pages_own_selected() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (_dir, url) = edition_page("1");
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
            options,
            "3|Third edition (unreleased)|false,2|Second edition|false,1|First edition|true",
            "by name in baked order, the unreleased one marked, own selected"
        );
    });
}

#[test]
fn e2e_choosing_an_edition_opens_its_first_page() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (_dir, url) = edition_page("1");
        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&url, None).await.expect("goto");

        page.locator("select.edition-switcher")
            .select_option("2", None)
            .await
            .expect("select");

        page.wait_for_url("**/book/2/", None)
            .await
            .expect("edition 2's root should open");
    });
}

/// A reader of an edition that is not current is told which edition it
/// is and the releases it covers, and pointed at the current one; a
/// reader of the unreleased edition is told that instead.
#[test]
fn e2e_a_non_current_edition_shows_a_banner_linking_to_the_current() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");

        let (_dir, url) = edition_page("1");
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
            text.contains("First edition")
                && text.contains("v0.1.0")
                && text.find("v0.1.0") < text.find("v0.2.0"),
            "the banner should name the page's edition and the first and newest release \
             it covers, in that order; got: {text}"
        );
        let link = page
            .evaluate_value(
                "(a => a.textContent + '|' + a.href)(document.querySelector('.edition-banner a'))",
            )
            .await
            .expect("link");
        assert!(
            link.starts_with("Second edition|") && link.ends_with("/book/2/"),
            "the banner should name the current edition and link to its root; got: {link}"
        );

        let (_dir, url) = edition_page("rc#2");
        let page = browser.new_page().await.expect("page");
        page.goto(&url, None).await.expect("goto");
        let text = page
            .locator(".edition-banner")
            .text_content()
            .await
            .expect("text")
            .unwrap_or_default();
        assert_eq!(
            text.matches("v0.2.5").count(),
            1,
            "an edition of one release names that release once; got: {text}"
        );

        let (_dir, url) = edition_page("3");
        let page = browser.new_page().await.expect("page");
        page.goto(&url, None).await.expect("goto");
        let text = page
            .locator(".edition-banner")
            .text_content()
            .await
            .expect("text")
            .unwrap_or_default();
        assert!(
            text.contains("Third edition") && text.contains("unreleased"),
            "the unreleased edition's banner should name it and say so; got: {text}"
        );
    });
}

#[test]
fn e2e_the_current_edition_shows_no_banner() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (_dir, url) = edition_page("2");
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
        let (_dir, url) = edition_site("1", true, &[link("schema/v0.1.1/", "Schema", None)]);
        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&url, None).await.expect("goto");

        // The tail, not the whole URL: the browser spells the tempdir's
        // path its own way (`file:///C:/…` for a Windows `C:\…`). Resolved
        // inside the edition instead, it would end in
        // `/book/1/schema/v0.1.1/`.
        let href = page
            .evaluate_value("document.querySelector('a.schema-docs-button').href")
            .await
            .expect("href");
        assert!(
            href.ends_with("/prefix/schema/v0.1.1/"),
            "the schema button resolves against the site root; got {href}"
        );

        page.locator("select.edition-switcher")
            .select_option("2", None)
            .await
            .expect("select");
        page.wait_for_url("**/prefix/book/2/", None)
            .await
            .expect("edition 2's root under the prefix should open");
    });
}

/// The version list as text: each edition's name, then its versions in
/// order, `*` marking the version carrying `aria-current` and `+` the
/// page's own edition.
const VERSION_GROUPS: &str = "Array.from(document.querySelectorAll('.schema-docs-edition'))\
     .map(g => (g.classList.contains('own') ? '+' : '') \
       + g.querySelector(':scope > .schema-docs-group-label').textContent + ':' \
       + Array.from(g.querySelectorAll('a')).map(a => \
           a.textContent + (a.getAttribute('aria-current') === 'true' ? '*' : '')).join(' '))\
     .join('|')";

/// Under editions, the button of an entry that aliased `current/` opens
/// every version's schema docs, the edge included, grouped by edition
/// newest first, with the page's own edition and the version its book
/// describes marked: the newest release it covers, whatever is listed
/// ahead of it. The list is a real button and real links, so a reader
/// without a mouse opens it, walks into it, and closes it, landing back
/// on the button.
#[test]
fn e2e_the_schema_button_lists_every_version_by_edition_and_works_from_the_keyboard() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (_dir, url) = edition_site("2", true, &[alias("2", "Schema", "schema/")]);
        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&url, None).await.expect("goto");

        assert_eq!(
            page.locator("a.schema-docs-button")
                .count()
                .await
                .expect("count"),
            0,
            "one link cannot reach every version"
        );
        let list = page.locator(".schema-docs-menu-list");
        assert!(
            !list.is_visible().await.expect("visible"),
            "the list should start closed"
        );
        let toggle = page.locator(".schema-docs-menu .icon-button");
        assert_eq!(
            toggle.get_attribute("title").await.expect("title"),
            Some("Schema".to_string()),
            "a lone entry's label names the button"
        );
        toggle.click(None).await.expect("open");
        assert!(list.is_visible().await.expect("visible"), "opened");

        let groups = page.evaluate_value(VERSION_GROUPS).await.expect("groups");
        assert_eq!(
            groups,
            "Third edition:main|+Second edition:v0.3.2-rc.1 v0.3.1* v0.3.0|\
             First edition:v0.2.0 v0.2.0-rc.1 v0.1.0"
        );
        let stray = page
            .evaluate_value(
                "Array.from(document.querySelectorAll('.schema-docs-edition a'))\
                 .filter(a => !a.href.endsWith('/prefix/schema/' + a.textContent + '/'))\
                 .map(a => a.href).join(',')",
            )
            .await
            .expect("hrefs");
        assert_eq!(
            stray, "",
            "every version links its own schema docs under the site root"
        );

        toggle.click(None).await.expect("close");
        toggle.focus().await.expect("focus");
        page.keyboard().press("Enter", None).await.expect("enter");
        assert!(
            list.is_visible().await.expect("visible"),
            "Enter on the button should open the list"
        );
        page.keyboard().press("Tab", None).await.expect("tab");
        let focused = page
            .evaluate_value(
                "document.activeElement.closest('.schema-docs-menu-list') \
                 ? document.activeElement.textContent : 'outside the list'",
            )
            .await
            .expect("focused");
        assert_eq!(
            focused, "main",
            "Tab should move into the list's first link"
        );
        page.keyboard().press("Escape", None).await.expect("escape");
        assert!(
            !list.is_visible().await.expect("visible"),
            "Escape should close the list"
        );
        let focused = page
            .evaluate_value("document.activeElement.className")
            .await
            .expect("focused");
        assert_eq!(
            focused, "icon-button",
            "closing from inside the list should return focus to the button"
        );
    });
}

/// A book fronting several schemas keeps one control: each entry that
/// aliased `current/` lists its versions under its own label and its own
/// directory (the site root for a bare `current/`), and an entry naming a
/// fixed path stays a single link beside them. An unreleased edition's
/// book describes the edge.
#[test]
fn e2e_several_entries_under_editions_list_versions_under_each_alias() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (_dir, url) = edition_site(
            "3",
            false,
            &[
                alias("3", "Wine schema", "schema/"),
                alias("3", "CQ&A", "schema/cqa/"),
                alias("3", "At the root", ""),
                link("docs/model/", "Model", None),
            ],
        );
        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&url, None).await.expect("goto");

        let toggle = page.locator(".schema-docs-menu .icon-button");
        assert_eq!(
            toggle.get_attribute("title").await.expect("title"),
            Some("Schema reference".to_string())
        );
        toggle.click(None).await.expect("open");

        // Each top-level entry: a schema as its label and the directories
        // its version links sit in, a fixed path as its label and target.
        let entries = page
            .evaluate_value(
                "(() => { const tail = a => a.href.split('/prefix/')[1]; \
                 return Array.from(document.querySelectorAll('.schema-docs-menu-list > li'))\
                 .map(li => li.classList.contains('schema-docs-schema') \
                   ? li.querySelector(':scope > .schema-docs-group-label').textContent + '=' \
                     + Array.from(new Set(Array.from(li.querySelectorAll('a')).map(a => \
                         tail(a).slice(0, -(a.textContent.length + 1))))).join('+') \
                   : li.querySelector('a').textContent + '>' + tail(li.querySelector('a')))\
                 .join(','); })()",
            )
            .await
            .expect("entries");
        assert_eq!(
            entries,
            "Wine schema=schema/,CQ&A=schema/cqa/,At the root=,Model>docs/model/"
        );
        let one = "+Third edition:main*|Second edition:v0.3.2-rc.1 v0.3.1 v0.3.0|\
                   First edition:v0.2.0 v0.2.0-rc.1 v0.1.0";
        let groups = page.evaluate_value(VERSION_GROUPS).await.expect("groups");
        assert_eq!(
            groups,
            format!("{one}|{one}|{one}"),
            "once under each schema"
        );
    });
}

/// The asset finds the site root through the page's canonical link.
/// Without one there is nowhere to resolve another version or edition
/// against, so the button stays the link to the version the edition
/// describes and the switcher stays away.
#[test]
fn e2e_a_page_without_a_canonical_keeps_a_plain_link_and_no_switcher() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (dir, url) = edition_site("1", false, &[alias("1", "Schema", "schema/")]);
        let page_file = dir.path().join("prefix/book/1/index.html");
        let html = std::fs::read_to_string(&page_file).expect("page");
        let without: String = html
            .lines()
            .filter(|line| !line.contains("rel=\"canonical\""))
            .map(|line| format!("{line}\n"))
            .collect();
        assert_ne!(
            without.len(),
            html.len(),
            "the fixture page has a canonical"
        );
        std::fs::write(&page_file, without).expect("page");

        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let page = browser.new_page().await.expect("page");
        page.goto(&url, None).await.expect("goto");

        let href = page
            .evaluate_value("document.querySelector('a.schema-docs-button').href")
            .await
            .expect("href");
        assert!(
            href.ends_with("/book/1/schema/v0.2.0/"),
            "the button links the described version from the book's own root; got {href}"
        );
        assert_eq!(
            page.locator("select.edition-switcher, .schema-docs-menu")
                .count()
                .await
                .expect("count"),
            0
        );
    });
}

/// Run one of this package's binaries in `repo` with the cache kept in
/// `cache`, asserting success.
fn run_bin(bin: &str, repo: &Path, cache: &Path, args: &[&str]) {
    let out = super::without_git_env(bin)
        .args(args)
        .current_dir(repo)
        .env("PANSCHEMA_CACHE_ROOT", cache)
        .output()
        .expect("run the binary");
    assert!(
        out.status.success(),
        "{bin} {args:?} failed; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A schema repository with two editions, three releases, and an edge:
/// `First edition` from `v0.1.0` (holding `v0.1.0` and `v0.1.1`) and
/// `Second edition` from `v0.2.0` (holding `v0.2.0` and `main`), each
/// book saying which it is. Its site, built by `panschema publish` and
/// `mdbook-panschema editions` with the `mdbook` on `PATH`, is under
/// `site/`.
fn built_editions_site() -> tempfile::TempDir {
    let version = Command::new("mdbook")
        .arg("--version")
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned());
    assert!(
        version
            .as_ref()
            .is_ok_and(|v| v.starts_with("mdbook v0.5.")),
        "this test builds real books: put mdbook 0.5 on PATH; found {version:?}"
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join("repo");
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&repo).expect("repo dir");
    super::git(&repo, &["init", "--initial-branch=main", "--quiet"]);
    super::git(&repo, &["config", "user.email", "test@example.com"]);
    super::git(&repo, &["config", "user.name", "Test"]);
    super::git(&repo, &["config", "commit.gpgsign", "false"]);
    super::git(&repo, &["config", "tag.gpgSign", "false"]);

    let write = |rel: &str, body: &str| {
        let path = repo.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, body).expect("write");
    };
    write(".gitignore", "site/\n");
    write(
        "panschema-publish.toml",
        r#"[schema]
name = "editions_site"
version = "0.2.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[publishing]
edge = "main"
output_dir = "site/schema"

[[book_link]]
schema_path = "schema/current/"
label = "Schema"

[editions]
output_dir = "site"

[[editions.edition]]
id = "1"
name = "First edition"
book = "books/1"
from = "v0.1.0"

[[editions.edition]]
id = "2"
name = "Second edition"
book = "books/2"
from = "v0.2.0"
"#,
    );
    for (id, word) in [("1", "one"), ("2", "two")] {
        write(
            &format!("books/{id}/book.toml"),
            &format!("[book]\ntitle = \"Edition {word}\"\n"),
        );
        write(
            &format!("books/{id}/src/SUMMARY.md"),
            "# Summary\n\n- [Chapter](chapter.md)\n",
        );
        write(
            &format!("books/{id}/src/chapter.md"),
            &format!("# Chapter\n\nThe book of edition {word}.\n"),
        );
    }
    for tag in ["v0.1.0", "v0.1.1", "v0.2.0", ""] {
        write(
            "schema.yaml",
            &format!(
                "id: https://example.org/editions_site\nname: editions_site\n\
                 version: {tag}\ndefault_range: string\n\
                 classes:\n  Thing:\n    description: A thing as of {tag}.\n"
            ),
        );
        super::git(&repo, &["add", "."]);
        super::git(&repo, &["commit", "--quiet", "-m", "next"]);
        if !tag.is_empty() {
            super::git(&repo, &["tag", tag]);
        }
    }

    run_bin(env!("CARGO_BIN_EXE_panschema"), &repo, &cache, &["publish"]);
    run_bin(
        env!("CARGO_BIN_EXE_mdbook-panschema"),
        &repo,
        &cache,
        &["editions"],
    );
    dir
}

/// A site built from a scratch repository by `panschema publish` and
/// `mdbook-panschema editions` with the real `mdbook`, read in a
/// browser: each edition is its own book, the switcher, the banner and
/// the version list agree about which edition holds which release, and
/// every version the list offers opens schema docs whose brand leads
/// back to the edition holding that version.
#[test]
fn e2e_a_built_editions_site_links_agree_both_ways() {
    let dir = built_editions_site();
    let site = dir.path().join("repo").join("site");
    let url = |rel: &str| format!("file://{}/{rel}", site.display());
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let playwright = Playwright::launch().await.expect("playwright");
        let browser = playwright.chromium().launch().await.expect("chromium");
        let text = |page: playwright_rs::Page| async move {
            page.evaluate_value("document.body.innerText")
                .await
                .expect("text")
        };
        let opens = |page: playwright_rs::Page, words: &'static str| async move {
            let body = text(page).await;
            assert!(
                body.contains(&format!("The book of edition {words}.")),
                "expected edition {words}'s book; got:\n{body:.300}"
            );
        };

        // The first edition, which is not current.
        let page = browser.new_page().await.expect("page");
        page.goto(&url("book/1/chapter.html"), None)
            .await
            .expect("goto");
        opens(page.clone(), "one").await;
        let options = page
            .evaluate_value(
                "Array.from(document.querySelectorAll('select.edition-switcher option'))\
                 .map(o => o.value + '|' + o.textContent + '|' + o.selected).join(',')",
            )
            .await
            .expect("options");
        assert_eq!(options, "2|Second edition|false,1|First edition|true");
        let banner = page
            .locator(".edition-banner")
            .text_content()
            .await
            .expect("banner")
            .unwrap_or_default();
        let (first, newest) = (banner.find("v0.1.0"), banner.find("v0.1.1"));
        assert!(
            banner.contains("First edition")
                && first.is_some()
                && newest.is_some()
                && first < newest,
            "the banner names the edition and the first and newest release it covers; \
             got: {banner}"
        );
        let current = page
            .evaluate_value(
                "(a => a.textContent + '|' + a.href)(document.querySelector('.edition-banner a'))",
            )
            .await
            .expect("banner link");
        assert!(
            current.starts_with("Second edition|") && current.ends_with("/book/2/"),
            "got: {current}"
        );

        page.locator(".schema-docs-menu .icon-button")
            .click(None)
            .await
            .expect("open the version list");
        assert_eq!(
            page.evaluate_value(VERSION_GROUPS).await.expect("groups"),
            "Second edition:main v0.2.0|+First edition:v0.1.1* v0.1.0"
        );
        let targets = page
            .evaluate_value(
                "Array.from(document.querySelectorAll('.schema-docs-edition a'))\
                 .map(a => a.textContent + '|' + a.href).join(',')",
            )
            .await
            .expect("targets");

        // Each version the list offers leads to its own schema docs, whose
        // brand leads back to the edition holding it.
        for target in targets.split(',') {
            let (version, href) = target.split_once('|').expect("version|href");
            assert!(
                href.ends_with(&format!("/repo/site/schema/{version}/")),
                "{version} links its own schema docs; got {href}"
            );
            let edition = if version.starts_with("v0.1.") {
                "1"
            } else {
                "2"
            };
            let docs = browser.new_page().await.expect("page");
            docs.goto(&format!("{href}index.html"), None)
                .await
                .expect("goto");
            let brand = docs
                .evaluate_value("document.querySelector('a.site-title').href")
                .await
                .expect("brand");
            assert!(
                brand.ends_with(&format!("/book/{edition}/")),
                "{version}'s schema docs brand book/{edition}/; got {brand}"
            );
        }

        // The second edition at its own path, and its copy at the site
        // root: the current edition, so neither carries a banner.
        for rel in ["book/2/chapter.html", "chapter.html"] {
            let page = browser.new_page().await.expect("page");
            page.goto(&url(rel), None).await.expect("goto");
            opens(page.clone(), "two").await;
            assert_eq!(
                page.locator(".edition-banner")
                    .count()
                    .await
                    .expect("count"),
                0,
                "{rel}: the current edition carries no banner"
            );
            page.locator(".schema-docs-menu .icon-button")
                .click(None)
                .await
                .expect("open the version list");
            assert_eq!(
                page.evaluate_value(VERSION_GROUPS).await.expect("groups"),
                "+Second edition:main v0.2.0*|First edition:v0.1.1 v0.1.0",
                "{rel}"
            );
        }

        // Choosing the first edition opens its book. Over `file://` the
        // directory has no index page of its own, so its index is read.
        let page = browser.new_page().await.expect("page");
        page.goto(&url("chapter.html"), None).await.expect("goto");
        page.locator("select.edition-switcher")
            .select_option("1", None)
            .await
            .expect("select");
        page.wait_for_url("**/book/1/", None)
            .await
            .expect("choosing the first edition goes to its root");
        let index = format!("{}index.html", page.url());
        page.goto(&index, None).await.expect("goto");
        opens(page.clone(), "one").await;

        let docs = browser.new_page().await.expect("page");
        docs.goto(&url("schema/current/index.html"), None)
            .await
            .expect("goto");
        let brand = docs
            .evaluate_value("document.querySelector('a.site-title').href")
            .await
            .expect("brand");
        assert!(
            brand.ends_with("/book/2/"),
            "the current schema docs brand the current edition; got {brand}"
        );
    });
}

/// The canonical's path is percent-encoded, so an edition id the URL
/// spells differently from the id (`#` is legal in one and starts a URL
/// fragment) still has its switcher.
#[test]
fn e2e_an_edition_id_the_url_spells_differently_still_gets_the_switcher() {
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
