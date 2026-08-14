//! Install the mdbook→schema toolbar link into an mdbook book.
//!
//! `mdbook-panschema install` drops two assets (`schema-link.js`,
//! `schema-link.css`) into the book directory and wires them into
//! `book.toml`'s `output.html.additional-js` / `additional-css`, the way
//! `mdbook-admonish install` does. The button's target and label are
//! baked in from `[book_link]` in `panschema-publish.toml`.

use std::path::{Path, PathBuf};

use crate::publish::{BookLinkConfig, PUBLISH_FILENAME, PublishConfig};
use anyhow::Context;
use toml_edit::{Array, DocumentMut, Item, Table, value};

/// The button asset. `__PANSCHEMA_SCHEMA_PATH__` / `__PANSCHEMA_LABEL__`
/// are replaced at install time with JSON string literals.
const SCHEMA_LINK_JS: &str = include_str!("mdbook_assets/schema-link.js");
const SCHEMA_LINK_CSS: &str = include_str!("mdbook_assets/schema-link.css");

const JS_FILENAME: &str = "schema-link.js";
const CSS_FILENAME: &str = "schema-link.css";
const JS_ENTRY: &str = "./schema-link.js";
const CSS_ENTRY: &str = "./schema-link.css";

/// What `install` did — so the CLI can report it and callers can assert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    /// Assets written and `book.toml` wired.
    Installed,
    /// `[book_link]` was absent or `enabled = false` — nothing changed.
    Disabled,
}

/// Install (or refresh) the toolbar-link assets in `book_dir` per `cfg`.
///
/// A no-op returning [`InstallOutcome::Disabled`] when `cfg.enabled` is
/// false. Otherwise writes the assets (overwriting any prior copy — this
/// is how a re-run picks up an upgraded button) and idempotently adds
/// them to `book.toml`.
pub fn install(book_dir: &Path, cfg: &BookLinkConfig) -> anyhow::Result<InstallOutcome> {
    if !cfg.enabled() {
        return Ok(InstallOutcome::Disabled);
    }

    // One array drives both renderings: the asset draws a plain link for a
    // single entry and a drop-down for several, so a book that grows a
    // second schema needs no change here.
    let entries: Vec<serde_json::Value> = cfg
        .entries()
        .iter()
        .map(|e| serde_json::json!({ "schemaPath": e.schema_path, "label": e.label }))
        .collect();
    let js = SCHEMA_LINK_JS.replace("__PANSCHEMA_LINKS__", &serde_json::to_string(&entries)?);

    std::fs::write(book_dir.join(JS_FILENAME), js)
        .with_context(|| format!("writing {JS_FILENAME} to {}", book_dir.display()))?;
    std::fs::write(book_dir.join(CSS_FILENAME), SCHEMA_LINK_CSS)
        .with_context(|| format!("writing {CSS_FILENAME} to {}", book_dir.display()))?;

    wire_book_toml(&book_dir.join("book.toml"))?;
    Ok(InstallOutcome::Installed)
}

/// Outcome of [`run`], mapped to a message by the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallReport {
    /// Assets installed and `book.toml` wired.
    Installed,
    /// `[book_link].enabled` was false — nothing changed.
    Disabled,
    /// The publish spec has no `[book_link]` section — nothing changed.
    NoBookLink,
}

/// Discover `panschema-publish.toml` at or above `book_dir`, read its
/// `[book_link]`, and install accordingly. Errors when no publish spec is
/// found or it fails to parse.
pub fn run(book_dir: &Path) -> anyhow::Result<InstallReport> {
    let publish_path = find_publish_toml(book_dir).with_context(|| {
        format!(
            "no {PUBLISH_FILENAME} found at or above {}",
            book_dir.display()
        )
    })?;
    let config = PublishConfig::from_path(&publish_path)?;

    match config.book_link {
        None => Ok(InstallReport::NoBookLink),
        Some(book_link) => Ok(match install(book_dir, &book_link)? {
            InstallOutcome::Installed => InstallReport::Installed,
            InstallOutcome::Disabled => InstallReport::Disabled,
        }),
    }
}

/// Locate `panschema-publish.toml` by walking up from `start` (the book
/// dir), mirroring cargo-style manifest discovery. The publish spec
/// typically sits at the schema-package root, above the book directory.
fn find_publish_toml(start: &Path) -> Option<PathBuf> {
    crate::manifest::discover_file(start, PUBLISH_FILENAME)
}

/// Add the two assets to `book.toml`'s `output.html.additional-*` arrays,
/// creating the tables if absent and skipping entries already present.
/// Uses `toml_edit` so existing comments, key order, and whitespace are
/// preserved.
fn wire_book_toml(book_toml: &Path) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(book_toml)
        .with_context(|| format!("reading {}", book_toml.display()))?;
    let mut doc: DocumentMut = text
        .parse()
        .with_context(|| format!("parsing {}", book_toml.display()))?;

    ensure_in_array(&mut doc, "additional-js", JS_ENTRY)?;
    ensure_in_array(&mut doc, "additional-css", CSS_ENTRY)?;

    let updated = doc.to_string();
    if updated != text {
        std::fs::write(book_toml, updated)
            .with_context(|| format!("writing {}", book_toml.display()))?;
    }
    Ok(())
}

/// Ensure `output.html.<key>` is an array containing `val`, creating the
/// `output` / `html` tables and the array as needed. Idempotent.
fn ensure_in_array(doc: &mut DocumentMut, key: &str, val: &str) -> anyhow::Result<()> {
    let output = doc
        .as_table_mut()
        .entry("output")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .context("`output` in book.toml is not a table")?;
    let html = output
        .entry("html")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .context("`output.html` in book.toml is not a table")?;
    let array = html
        .entry(key)
        .or_insert(value(Array::new()))
        .as_array_mut()
        .with_context(|| format!("`output.html.{key}` in book.toml is not an array"))?;

    if !array.iter().any(|v| v.as_str() == Some(val)) {
        array.push(val);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::{BookLinkEntry, BookLinkTable};

    fn cfg(enabled: bool) -> BookLinkConfig {
        BookLinkConfig::Single(BookLinkTable {
            enabled,
            schema_path: "schema/current/".to_string(),
            label: "Schema reference".to_string(),
        })
    }

    /// The `[[book_link]]` form, one entry per (path, label) pair.
    fn list_cfg(entries: &[(&str, &str)]) -> BookLinkConfig {
        BookLinkConfig::List(
            entries
                .iter()
                .map(|(p, l)| BookLinkEntry {
                    schema_path: p.to_string(),
                    label: l.to_string(),
                })
                .collect(),
        )
    }

    /// A single entry renders the plain link a one-schema book has always
    /// had — the property that lets a book adopt the list form without any
    /// visible change.
    #[test]
    fn one_entry_installs_a_plain_link_not_a_menu() {
        let dir = book_dir();
        install(dir.path(), &list_cfg(&[("schema/current/", "Wine schema")])).unwrap();
        let js = std::fs::read_to_string(dir.path().join("schema-link.js")).unwrap();

        assert!(
            js.contains(r#""schemaPath":"schema/current/""#)
                && js.contains(r#""label":"Wine schema""#),
            "the entry should be baked in; got:\n{js}"
        );
        assert!(
            !js.contains("__PANSCHEMA_LINKS__"),
            "the placeholder must be substituted; got:\n{js}"
        );
    }

    /// Several entries all reach the asset, in declaration order, so the
    /// menu lists them the way the author wrote them.
    #[test]
    fn several_entries_are_baked_in_in_declaration_order() {
        let dir = book_dir();
        install(
            dir.path(),
            &list_cfg(&[
                ("schema/current/", "Wine schema"),
                ("schema/cqa/current/", "CQ&A contract"),
            ]),
        )
        .unwrap();
        let js = std::fs::read_to_string(dir.path().join("schema-link.js")).unwrap();

        let wine = js.find("Wine schema").expect("first entry present");
        let cqa = js.find("CQ&A contract").expect("second entry present");
        assert!(wine < cqa, "declaration order should survive; got:\n{js}");
        assert!(
            js.contains("schema/cqa/current/"),
            "the dependency schema's path should be baked in; got:\n{js}"
        );
    }

    /// An empty list is the list form's way of saying off, and must leave
    /// the book untouched exactly as `enabled = false` does.
    #[test]
    fn an_empty_list_installs_nothing() {
        let dir = book_dir();
        let before = std::fs::read_to_string(dir.path().join("book.toml")).unwrap();

        let outcome = install(dir.path(), &list_cfg(&[])).unwrap();
        assert_eq!(outcome, InstallOutcome::Disabled);
        assert!(!dir.path().join("schema-link.js").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("book.toml")).unwrap(),
            before,
            "an empty list must not touch book.toml"
        );
    }

    /// Every href is built from `path_to_root`, so the links resolve under
    /// a GitHub Pages project prefix rather than only at a domain root.
    #[test]
    fn menu_links_are_relative_to_path_to_root() {
        let dir = book_dir();
        install(dir.path(), &list_cfg(&[("a/", "A"), ("b/", "B")])).unwrap();
        let js = std::fs::read_to_string(dir.path().join("schema-link.js")).unwrap();

        assert!(
            js.contains("root + entry.schemaPath"),
            "menu entries should be root-relative; got:\n{js}"
        );
        assert!(
            js.contains("root + links[0].schemaPath"),
            "the single-link form should be root-relative too; got:\n{js}"
        );
    }

    /// A book dir with a minimal `book.toml` (no `[output.html]` yet, so
    /// the wiring exercises table creation).
    fn book_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("book.toml"), "[book]\ntitle = \"Test\"\n").unwrap();
        dir
    }

    #[test]
    fn install_writes_assets_and_wires_book_toml() {
        let dir = book_dir();
        let outcome = install(dir.path(), &cfg(true)).unwrap();
        assert_eq!(outcome, InstallOutcome::Installed);

        assert!(dir.path().join("schema-link.js").is_file());
        assert!(dir.path().join("schema-link.css").is_file());

        let book_toml = std::fs::read_to_string(dir.path().join("book.toml")).unwrap();
        assert!(
            book_toml.contains("additional-js") && book_toml.contains("./schema-link.js"),
            "book.toml should list the js asset; got:\n{book_toml}"
        );
        assert!(
            book_toml.contains("additional-css") && book_toml.contains("./schema-link.css"),
            "book.toml should list the css asset; got:\n{book_toml}"
        );
    }

    #[test]
    fn install_is_idempotent() {
        let dir = book_dir();
        install(dir.path(), &cfg(true)).unwrap();
        install(dir.path(), &cfg(true)).unwrap();

        let book_toml = std::fs::read_to_string(dir.path().join("book.toml")).unwrap();
        assert_eq!(
            book_toml.matches("./schema-link.js").count(),
            1,
            "re-running install must not duplicate the js entry; got:\n{book_toml}"
        );
        assert_eq!(book_toml.matches("./schema-link.css").count(), 1);
    }

    #[test]
    fn install_disabled_is_noop() {
        let dir = book_dir();
        let before = std::fs::read_to_string(dir.path().join("book.toml")).unwrap();

        let outcome = install(dir.path(), &cfg(false)).unwrap();

        assert_eq!(outcome, InstallOutcome::Disabled);
        assert!(!dir.path().join("schema-link.js").exists());
        assert!(!dir.path().join("schema-link.css").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("book.toml")).unwrap(),
            before,
            "disabled install must not touch book.toml"
        );
    }

    #[test]
    fn install_bakes_schema_path_and_label_into_js() {
        let dir = book_dir();
        let c = list_cfg(&[("docs/model/", "Data model")]);
        install(dir.path(), &c).unwrap();

        let js = std::fs::read_to_string(dir.path().join("schema-link.js")).unwrap();
        assert!(
            js.contains("\"docs/model/\""),
            "schema_path baked in; got:\n{js}"
        );
        assert!(js.contains("\"Data model\""), "label baked in; got:\n{js}");
        assert!(
            !js.contains("__PANSCHEMA_SCHEMA_PATH__"),
            "placeholder must be fully substituted"
        );
    }

    /// Write a `panschema-publish.toml` with the given `[book_link]` body
    /// into `dir`.
    fn write_publish(dir: &Path, book_link_body: &str) {
        let toml = format!(
            "[schema]\nname = \"x\"\nversion = \"0.1.0\"\nlinkml = \"1.7.0\"\n\
             [files]\nmain = \"schema.yaml\"\n{book_link_body}"
        );
        std::fs::write(dir.join("panschema-publish.toml"), toml).unwrap();
    }

    #[test]
    fn find_publish_toml_walks_up_to_an_ancestor() {
        let root = tempfile::tempdir().unwrap();
        write_publish(root.path(), "");
        let nested = root.path().join("book").join("src");
        std::fs::create_dir_all(&nested).unwrap();

        let found = find_publish_toml(&nested).expect("should walk up and find the publish spec");
        assert_eq!(
            found.canonicalize().unwrap(),
            root.path()
                .join("panschema-publish.toml")
                .canonicalize()
                .unwrap(),
            "must return the ancestor's publish spec, walking up past `book`"
        );
    }

    #[test]
    fn find_publish_toml_returns_none_when_absent() {
        // A tempdir with no publish spec at or above it (within the tree).
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("deep").join("nested");
        std::fs::create_dir_all(&sub).unwrap();
        assert!(find_publish_toml(&sub).is_none());
    }

    #[test]
    fn run_installs_when_book_link_enabled() {
        let root = tempfile::tempdir().unwrap();
        write_publish(
            root.path(),
            "[book_link]\nenabled = true\nschema_path = \"schema/current/\"\nlabel = \"Schema reference\"\n",
        );
        let book = root.path().join("book");
        std::fs::create_dir_all(&book).unwrap();
        std::fs::write(book.join("book.toml"), "[book]\ntitle = \"T\"\n").unwrap();

        assert_eq!(run(&book).unwrap(), InstallReport::Installed);
        assert!(book.join("schema-link.js").is_file());
    }

    #[test]
    fn run_reports_disabled_without_writing_assets() {
        let root = tempfile::tempdir().unwrap();
        write_publish(root.path(), "[book_link]\nenabled = false\n");
        let book = root.path().join("book");
        std::fs::create_dir_all(&book).unwrap();
        std::fs::write(book.join("book.toml"), "[book]\ntitle = \"T\"\n").unwrap();

        assert_eq!(run(&book).unwrap(), InstallReport::Disabled);
        assert!(!book.join("schema-link.js").exists());
    }

    #[test]
    fn run_reports_no_book_link_when_section_absent() {
        let root = tempfile::tempdir().unwrap();
        write_publish(root.path(), "");
        let book = root.path().join("book");
        std::fs::create_dir_all(&book).unwrap();
        std::fs::write(book.join("book.toml"), "[book]\ntitle = \"T\"\n").unwrap();

        assert_eq!(run(&book).unwrap(), InstallReport::NoBookLink);
        assert!(!book.join("schema-link.js").exists());
    }

    #[test]
    fn wire_book_toml_preserves_existing_additional_entries() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("book.toml"),
            "[output.html]\nadditional-js = [\"./existing.js\"]\n",
        )
        .unwrap();
        install(dir.path(), &cfg(true)).unwrap();

        let book_toml = std::fs::read_to_string(dir.path().join("book.toml")).unwrap();
        assert!(
            book_toml.contains("./existing.js"),
            "must keep existing entries"
        );
        assert!(
            book_toml.contains("./schema-link.js"),
            "must add the new entry"
        );
    }
}
