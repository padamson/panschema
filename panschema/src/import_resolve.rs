//! Load-time resolution of LinkML `imports:` into one merged schema.
//!
//! A LinkML `SchemaDefinition` may list other schemas in `imports:`.
//! Those entries deserialize today but nothing follows them, so a
//! vocabulary split across files renders only the root file's elements.
//! This module closes that gap: before any writer runs, it loads each
//! imported local file via the [`FormatRegistry`], merges its elements
//! into the root, and hands every writer a schema shaped exactly like a
//! single-file one.
//!
//! The merge unions `classes`, `slots`, `enums`, `types`, and
//! `prefixes`. The importing (root) schema wins on a name collision —
//! the imported definition is dropped and the name recorded so a later
//! diagnostic pass can surface it. Each merged element's origin file is
//! recorded for provenance.
//!
//! A self-import or an import cycle is a hard error, never an infinite
//! loop: every file is canonicalized and tracked on the resolution
//! path, and re-entering a path is rejected.
//!
//! Out of scope here (handled elsewhere): transitive imports, CURIE /
//! URL imports, and builtin `linkml:*` imports. An entry that doesn't
//! resolve to a local file is reported through [`ImportError`] rather
//! than crashing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::io::FormatRegistry;
use crate::linkml::SchemaDefinition;

/// Extensions tried, in order, when an `imports:` entry has no
/// extension of its own (`imports: [common]` → `common.yaml`, then
/// `.yml`, then `.ttl`).
const IMPORT_EXTENSIONS: &[&str] = &["yaml", "yml", "ttl"];

/// Errors raised while resolving `imports:`.
#[derive(Error, Debug)]
pub enum ImportError {
    /// An import entry could not be resolved to a readable local file.
    /// Carries the raw entry and the importing file for context.
    #[error("import `{entry}` (from `{importer}`) could not be resolved to a local file")]
    Unresolvable { entry: String, importer: PathBuf },

    /// A self-import or an import cycle was detected. `path` is the
    /// file re-entered while already on the resolution stack.
    #[error("import cycle detected at `{}`", path.display())]
    Cycle { path: PathBuf },

    /// Reading or parsing an imported file failed.
    #[error("failed to load import `{}`: {source}", path.display())]
    Load {
        path: PathBuf,
        source: crate::io::IoError,
    },
}

/// Result of a successful import resolution: which element names were
/// dropped because the root already defined them, and where every
/// merged element originated.
#[derive(Debug, Default)]
pub struct ImportReport {
    /// Names that collided — the root's definition was kept, the
    /// import's was discarded. Each entry is `"<kind> <name>"`, e.g.
    /// `"class Address"`. A later slice turns these into diagnostics.
    pub collisions: Vec<String>,
    /// For each merged element, the file it came from. Keyed by
    /// `"<kind> <name>"` so the same name across kinds stays distinct.
    pub origins: BTreeMap<String, PathBuf>,
}

/// Resolve `root.imports` in place: load each imported local file and
/// merge its elements into `root`. `root_path` is the file the root
/// schema was read from — imports resolve relative to its directory,
/// and the root sits on the cycle-detection stack so an import that
/// points back at it (directly or transitively) is a cycle.
///
/// On return, `root` carries the union of every successfully imported
/// schema's `classes`, `slots`, `enums`, `types`, and `prefixes`, with
/// the root's own definitions taking precedence on any name collision.
/// The returned [`ImportReport`] records collisions and per-element
/// origins.
///
/// Errors on an unresolvable entry or an import cycle.
pub fn resolve_imports(
    root: &mut SchemaDefinition,
    root_path: &Path,
    registry: &FormatRegistry,
) -> Result<ImportReport, ImportError> {
    let mut report = ImportReport::default();
    let base_dir = root_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    // The root file sits on the resolution stack so an import that
    // points back at it (directly or transitively) is a cycle.
    let mut visiting = vec![canonicalize_lossy(root_path)];
    resolve_into(root, &base_dir, registry, &mut report, &mut visiting)?;
    Ok(report)
}

/// Recursive worker. Loads and merges each entry of `schema.imports`
/// into `root`, following imports of imports while tracking the
/// canonical path of every file on the current resolution stack so a
/// cycle errors instead of looping.
fn resolve_into(
    root: &mut SchemaDefinition,
    base_dir: &Path,
    registry: &FormatRegistry,
    report: &mut ImportReport,
    visiting: &mut Vec<PathBuf>,
) -> Result<(), ImportError> {
    // Take the import list so we can mutate `root` while iterating; the
    // merged schema's `imports` is cleared (every entry is now folded
    // in, so writers see a self-contained schema).
    let imports = std::mem::take(&mut root.imports);

    for entry in imports {
        // CURIE / URL / builtin imports are deferred: an entry with a
        // scheme or namespace separator that isn't a local path is not
        // resolved here. Recognized by the absence of a matching local
        // file below, which surfaces as `Unresolvable`.
        let resolved =
            resolve_entry_path(&entry, base_dir).ok_or_else(|| ImportError::Unresolvable {
                entry: entry.clone(),
                importer: base_dir.to_path_buf(),
            })?;

        let canonical = canonicalize_lossy(&resolved);
        if visiting.contains(&canonical) {
            return Err(ImportError::Cycle { path: canonical });
        }

        let reader = registry
            .reader_for_path(&resolved)
            .map_err(|source| ImportError::Load {
                path: resolved.clone(),
                source,
            })?;
        let mut imported = reader.read(&resolved).map_err(|source| ImportError::Load {
            path: resolved.clone(),
            source,
        })?;

        // Recurse into the imported file's own imports first, resolving
        // them relative to *its* directory, before folding the imported
        // schema into the root.
        let imported_dir = resolved
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| base_dir.to_path_buf());
        visiting.push(canonical);
        resolve_into(&mut imported, &imported_dir, registry, report, visiting)?;
        visiting.pop();

        merge_schema(root, &imported, &resolved, report);
    }

    Ok(())
}

/// Resolve one `imports:` entry to a readable local file path.
///
/// Accepts a bare name (`common`) or a relative path (`shared/common`).
/// When the entry already carries an extension that resolves to a file,
/// it is used as-is; otherwise each candidate in [`IMPORT_EXTENSIONS`]
/// is tried in turn. Returns `None` when nothing on disk matches —
/// which the caller reports rather than crashing.
fn resolve_entry_path(entry: &str, base_dir: &Path) -> Option<PathBuf> {
    let candidate = base_dir.join(entry);
    if candidate.is_file() {
        return Some(candidate);
    }
    for ext in IMPORT_EXTENSIONS {
        let with_ext = base_dir.join(format!("{entry}.{ext}"));
        if with_ext.is_file() {
            return Some(with_ext);
        }
    }
    None
}

/// Canonicalize a path for cycle tracking, falling back to the path
/// itself when it can't be canonicalized (e.g. doesn't exist yet) so a
/// missing file surfaces as `Unresolvable`/`Load` rather than a panic.
fn canonicalize_lossy(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Merge `imported` into `root`. For each of `classes`, `slots`,
/// `enums`, `types`, and `prefixes`: an entry whose name is unused in
/// `root` is inserted and its origin recorded; an entry whose name is
/// already present in `root` is dropped (root wins) and the collision
/// recorded.
fn merge_schema(
    root: &mut SchemaDefinition,
    imported: &SchemaDefinition,
    origin: &Path,
    report: &mut ImportReport,
) {
    /// Merge one named map, recording origin on insert and collision on
    /// conflict. `kind` labels the element for the report.
    macro_rules! merge_map {
        ($field:ident, $kind:literal) => {
            for (name, def) in &imported.$field {
                if root.$field.contains_key(name) {
                    report.collisions.push(format!("{} {name}", $kind));
                } else {
                    root.$field.insert(name.clone(), def.clone());
                    report
                        .origins
                        .insert(format!("{} {name}", $kind), origin.to_path_buf());
                }
            }
        };
    }

    merge_map!(classes, "class");
    merge_map!(slots, "slot");
    merge_map!(enums, "enum");
    merge_map!(types, "type");

    // Prefixes union the same way; a clashing prefix keeps the root's
    // mapping. Recorded under a `prefix` label for symmetry.
    for (prefix, base) in &imported.prefixes {
        if root.prefixes.contains_key(prefix) {
            report.collisions.push(format!("prefix {prefix}"));
        } else {
            root.prefixes.insert(prefix.clone(), base.clone());
            report
                .origins
                .insert(format!("prefix {prefix}"), origin.to_path_buf());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/imports")
    }

    /// Load a fixture as the root schema via the registry, the same way
    /// `generate` does before resolving imports.
    fn read_root(name: &str) -> (SchemaDefinition, PathBuf) {
        let registry = FormatRegistry::with_defaults();
        let path = fixtures_dir().join(name);
        let reader = registry.reader_for_path(&path).expect("reader for fixture");
        let schema = reader.read(&path).expect("read fixture");
        (schema, path)
    }

    #[test]
    fn resolve_imports_merges_single_import() {
        // `app.yaml` imports `common.yaml`, which defines `Address`.
        // After resolution the merged root carries `Address` even
        // though the root file never declared it.
        let registry = FormatRegistry::with_defaults();
        let (mut root, path) = read_root("app.yaml");
        assert!(
            !root.classes.contains_key("Address"),
            "Address must originate only in the import"
        );
        let report = resolve_imports(&mut root, &path, &registry).expect("resolve imports");

        assert!(
            root.classes.contains_key("Address"),
            "imported class should be merged into the root"
        );
        // Root's own class survives the merge.
        assert!(root.classes.contains_key("Customer"));
        // Slots, enums, and types from the import are folded in too.
        assert!(root.slots.contains_key("identifier"));
        assert!(root.enums.contains_key("Country"));
        assert!(root.types.contains_key("postal_code"));
        // Provenance points the merged class at the import file.
        assert_eq!(
            report.origins.get("class Address").map(PathBuf::as_path),
            Some(fixtures_dir().join("common.yaml").as_path())
        );
        // No collisions — the two files define disjoint names.
        assert!(report.collisions.is_empty());
        // The merged schema is self-contained: imports were folded in.
        assert!(root.imports.is_empty());
    }

    #[test]
    fn resolve_imports_detects_cycle() {
        // `cycle_a` imports `cycle_b`, which imports `cycle_a` back.
        // Resolution errors with a cycle, and crucially returns rather
        // than looping forever.
        let registry = FormatRegistry::with_defaults();
        let (mut root, path) = read_root("cycle_a.yaml");

        let err = resolve_imports(&mut root, &path, &registry)
            .expect_err("a cyclic import graph must be rejected");
        assert!(
            matches!(err, ImportError::Cycle { .. }),
            "expected a cycle error, got {err:?}"
        );
    }

    #[test]
    fn resolve_imports_root_wins_on_collision() {
        // A root that itself defines a name the import also defines
        // keeps its own definition and records the collision.
        let registry = FormatRegistry::with_defaults();
        let (mut root, path) = read_root("app.yaml");
        // Inject a root-side `Address` so it collides with the import's.
        let mut root_address = crate::linkml::ClassDefinition::new("Address");
        root_address.description = Some("root's own Address".into());
        root.classes.insert("Address".into(), root_address);

        let report = resolve_imports(&mut root, &path, &registry).expect("resolve imports");

        assert_eq!(
            root.classes["Address"].description.as_deref(),
            Some("root's own Address"),
            "the root's definition must win on collision"
        );
        assert!(
            report.collisions.iter().any(|c| c == "class Address"),
            "the collision should be recorded, got {:?}",
            report.collisions
        );
    }

    #[test]
    fn resolve_imports_errors_on_unresolvable_entry() {
        // An import naming no local file is reported, not silently
        // dropped and not a panic.
        let registry = FormatRegistry::with_defaults();
        let mut root = SchemaDefinition::new("orphan_importer");
        root.imports = vec!["does_not_exist".into()];
        let root_path = fixtures_dir().join("orphan_importer.yaml");

        let err = resolve_imports(&mut root, &root_path, &registry)
            .expect_err("an unresolvable import must error");
        assert!(matches!(err, ImportError::Unresolvable { .. }));
    }
}
