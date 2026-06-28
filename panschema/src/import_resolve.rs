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
//! Imports resolve transitively (imports of imports), and a file
//! reached via two paths — a diamond — is loaded and merged exactly
//! once, deduplicated by canonical path.
//!
//! The merge unions `classes`, `slots`, `enums`, `types`, and
//! `prefixes`. When two files define the same name, structurally equal
//! definitions are silently unified; differing ones are an incompatible
//! collision — the kept definition (root precedence) wins, the other is
//! dropped, and a [`Collision`] records both files. Each merged
//! element's origin file is recorded for provenance.
//!
//! A self-import or an import cycle is a hard error, never an infinite
//! loop: every file is canonicalized and tracked on the resolution
//! path, and re-entering a path on that path is rejected — across the
//! full transitive graph, not just direct self-imports.
//!
//! Built-in and remote imports are recognized and skipped as no-ops:
//! the standard `linkml:*` modules, and any CURIE or URL that expands to
//! a remote `http(s)` URI, name well-known schemas a writer already
//! understands rather than local files to merge. Only bare names and
//! relative paths are resolved on disk; a path-shaped entry that names
//! no file is reported through [`ImportError`] rather than crashing.

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

/// One incompatible name collision: two files defined the same element
/// (same `kind` + `name`) with *different* definitions. The kept
/// definition wins (root precedence); the dropped one is discarded.
/// Byte-identical re-definitions never produce a `Collision` — they are
/// silently unified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collision {
    /// Element kind, e.g. `"class"`, `"slot"`, `"enum"`, `"type"`,
    /// `"prefix"`.
    pub kind: String,
    /// Element name (or prefix string) shared by both definitions.
    pub name: String,
    /// File whose definition was kept. `None` when the kept definition
    /// is a root-schema original with no recorded import origin.
    pub kept_from: Option<PathBuf>,
    /// File whose (differing) definition was dropped.
    pub dropped_from: PathBuf,
}

/// Result of a successful import resolution: which elements collided
/// incompatibly across files, and where every merged element
/// originated.
#[derive(Debug, Default)]
pub struct ImportReport {
    /// Incompatible name collisions — the kept definition won (root
    /// precedence), the differing import was discarded. Identical
    /// re-definitions are unified and produce no entry here.
    pub collisions: Vec<Collision>,
    /// For each merged element, the file it came from. Keyed by
    /// `"<kind> <name>"` so the same name across kinds stays distinct.
    pub origins: BTreeMap<String, PathBuf>,
    /// Canonical path of every import file actually read and merged, in
    /// resolution order. A diamond-deduplicated file appears exactly
    /// once here even though two importers reference it; a skipped
    /// duplicate is never re-added.
    pub loaded_files: Vec<PathBuf>,
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
    // points back at it (directly or transitively) is a cycle. It also
    // counts as already loaded so an import pointing at the root is a
    // dedup skip on any path that isn't itself a cycle.
    let root_canonical = canonicalize_lossy(root_path);
    let mut visiting = vec![root_canonical.clone()];
    let mut loaded = std::collections::BTreeSet::from([root_canonical]);
    resolve_into(
        root,
        &base_dir,
        registry,
        &mut report,
        &mut visiting,
        &mut loaded,
    )?;
    Ok(report)
}

/// Recursive worker. Loads and merges each entry of `schema.imports`
/// into `root`, following imports of imports.
///
/// Two canonical-path sets guard the walk, and they answer different
/// questions:
///
/// - `visiting` is the *current* resolution path (a stack). A canonical
///   path already on it means an import points back at a file we're
///   still resolving — a cycle, which errors.
/// - `loaded` is every file already merged on *any* path. A canonical
///   path in it (but not on `visiting`) is a diamond: the file was
///   reached via another route and already folded in, so it is skipped
///   — not re-read, not re-merged — and its elements appear once.
///
/// Order at each entry: cycle check first (`visiting`), then dedup skip
/// (`loaded`), else read + recurse + merge + record as loaded.
fn resolve_into(
    root: &mut SchemaDefinition,
    base_dir: &Path,
    registry: &FormatRegistry,
    report: &mut ImportReport,
    visiting: &mut Vec<PathBuf>,
    loaded: &mut std::collections::BTreeSet<PathBuf>,
) -> Result<(), ImportError> {
    // Take the import list so we can mutate `root` while iterating; the
    // merged schema's `imports` is cleared (every entry is now folded
    // in, so writers see a self-contained schema). The declaring
    // schema's own prefixes classify each entry, so snapshot them before
    // merging — which unions imported prefixes into `root` — can change
    // the map mid-loop.
    let imports = std::mem::take(&mut root.imports);
    let prefixes = root.prefixes.clone();

    for entry in imports {
        // Built-in / remote imports are not local files: the standard
        // `linkml:` modules and any CURIE or URL that expands to a remote
        // URI name well-known schemas a writer already understands as
        // built-ins. Skip them as no-ops rather than resolving — or
        // failing to resolve — them as local paths.
        if is_builtin_import(&entry, &prefixes) {
            continue;
        }

        let resolved =
            resolve_entry_path(&entry, base_dir).ok_or_else(|| ImportError::Unresolvable {
                entry: entry.clone(),
                importer: base_dir.to_path_buf(),
            })?;

        let canonical = canonicalize_lossy(&resolved);
        if visiting.contains(&canonical) {
            return Err(ImportError::Cycle { path: canonical });
        }
        // Diamond dedup: a file reached via two paths is merged once.
        // Already loaded (and not on the current path) means another
        // route folded it in, so skip it without re-reading or
        // re-merging.
        if loaded.contains(&canonical) {
            continue;
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
        visiting.push(canonical.clone());
        resolve_into(
            &mut imported,
            &imported_dir,
            registry,
            report,
            visiting,
            loaded,
        )?;
        visiting.pop();

        merge_schema(root, &imported, &resolved, report);
        report.loaded_files.push(canonical.clone());
        loaded.insert(canonical);
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

/// The built-in `linkml:` prefix always denotes well-known LinkML
/// modules (`types`, `units`, `mappings`, `extended_*`), whether or not
/// the schema declares the prefix explicitly.
const LINKML_PREFIX: &str = "linkml";

/// True when an `imports:` entry names a built-in / remote schema rather
/// than a local file, so local-file resolution must be skipped.
///
/// An entry qualifies when it is a bare remote URL, or a CURIE
/// `prefix:local` whose prefix is the built-in `linkml` namespace or
/// expands — via the declaring schema's own `prefixes` — to a remote
/// `http(s)` URI. Bare names and relative paths carry no such prefix and
/// fall through to local resolution.
fn is_builtin_import(entry: &str, prefixes: &BTreeMap<String, String>) -> bool {
    // A bare URL imported directly (e.g. `https://w3id.org/linkml/types`).
    if is_remote_uri(entry) {
        return true;
    }
    // CURIE form `prefix:local`. No colon → bare name / relative path,
    // which is a local import.
    let Some((prefix, _local)) = entry.split_once(':') else {
        return false;
    };
    // The standard LinkML modules are built-ins even when the schema
    // omits the prefix declaration.
    if prefix == LINKML_PREFIX {
        return true;
    }
    // Any other CURIE counts as remote only when its prefix is declared
    // and expands to a remote URI; otherwise treat it as a local path so
    // a genuine local import is never silently skipped.
    prefixes.get(prefix).is_some_and(|base| is_remote_uri(base))
}

/// True for an `http`/`https` URI — the shape a remote schema namespace
/// takes. Local paths and bare names are never remote.
fn is_remote_uri(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// Canonicalize a path for cycle tracking, falling back to the path
/// itself when it can't be canonicalized (e.g. doesn't exist yet) so a
/// missing file surfaces as `Unresolvable`/`Load` rather than a panic.
fn canonicalize_lossy(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Merge `imported` into `root`. For each of `classes`, `slots`,
/// `enums`, `types`, and `prefixes`:
///
/// - A name unused in `root` is inserted and its origin recorded.
/// - A name already present whose *existing* and *incoming* definitions
///   are structurally equal is silently unified — root keeps its copy,
///   no collision recorded (the two files simply agree).
/// - A name already present whose definitions *differ* is an
///   incompatible collision: root keeps its copy (precedence) and a
///   [`Collision`] is recorded naming the file kept (looked up from
///   `report.origins`; `None` for a root original) and the file
///   dropped (`origin`).
fn merge_schema(
    root: &mut SchemaDefinition,
    imported: &SchemaDefinition,
    origin: &Path,
    report: &mut ImportReport,
) {
    /// Merge one named map. On a name clash, compare definitions: equal
    /// → unify silently; differ → keep root's and record a collision.
    /// `kind` labels the element for the report and origin keys.
    macro_rules! merge_map {
        ($field:ident, $kind:literal) => {
            for (name, def) in &imported.$field {
                if let Some(existing) = root.$field.get(name) {
                    if existing != def {
                        let key = format!("{} {name}", $kind);
                        report.collisions.push(Collision {
                            kind: $kind.to_string(),
                            name: name.clone(),
                            kept_from: report.origins.get(&key).cloned(),
                            dropped_from: origin.to_path_buf(),
                        });
                    }
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

    // Prefixes union the same way; an identical mapping unifies, a
    // differing one keeps the root's and records a `prefix` collision.
    for (prefix, base) in &imported.prefixes {
        if let Some(existing) = root.prefixes.get(prefix) {
            if existing != base {
                let key = format!("prefix {prefix}");
                report.collisions.push(Collision {
                    kind: "prefix".to_string(),
                    name: prefix.clone(),
                    kept_from: report.origins.get(&key).cloned(),
                    dropped_from: origin.to_path_buf(),
                });
            }
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
        let collision = report
            .collisions
            .iter()
            .find(|c| c.kind == "class" && c.name == "Address")
            .expect("the collision should be recorded");
        // The root's own definition was kept: a root original has no
        // recorded import origin, so `kept_from` is None. The import's
        // file is the one dropped.
        assert_eq!(collision.kept_from, None);
        assert_eq!(
            collision.dropped_from.as_path(),
            fixtures_dir().join("common.yaml").as_path()
        );
    }

    #[test]
    fn resolve_imports_dedupes_diamond() {
        // `diamond_a` imports B and C; B and C both import D. D's
        // elements appear exactly once and D is processed exactly once —
        // a single recorded origin for `DThing`, not a duplicate.
        let registry = FormatRegistry::with_defaults();
        let (mut root, path) = read_root("diamond_a.yaml");

        let report = resolve_imports(&mut root, &path, &registry).expect("resolve imports");

        // Every arm's class merged in.
        assert!(root.classes.contains_key("AThing"));
        assert!(root.classes.contains_key("BThing"));
        assert!(root.classes.contains_key("CThing"));
        assert!(root.classes.contains_key("DThing"));

        // D was loaded and merged exactly once despite being reached
        // through both arms: its canonical path appears a single time in
        // the loaded-files record (the second arm hit the dedup skip).
        let diamond_d = fixtures_dir().join("diamond_d.yaml");
        let diamond_d_canonical = diamond_d.canonicalize().expect("canonicalize diamond_d");
        let d_loads = report
            .loaded_files
            .iter()
            .filter(|p| **p == diamond_d_canonical)
            .count();
        assert_eq!(
            d_loads, 1,
            "the diamond base must be processed once, not per arm: {:?}",
            report.loaded_files
        );
        // No collision: the base is merged once, never against itself.
        assert!(
            report.collisions.is_empty(),
            "the diamond base must merge once, not collide with itself: {:?}",
            report.collisions
        );
    }

    #[test]
    fn resolve_imports_reports_differing_collision_with_both_files() {
        // `conflict_root` imports two files that each define `Widget`
        // incompatibly. The first import's definition is kept; the
        // second's is dropped. The recorded collision names both files.
        let registry = FormatRegistry::with_defaults();
        let (mut root, path) = read_root("conflict_root.yaml");

        let report = resolve_imports(&mut root, &path, &registry).expect("resolve imports");

        let collision = report
            .collisions
            .iter()
            .find(|c| c.kind == "class" && c.name == "Widget")
            .expect("the incompatible Widget redefinition must be reported");
        assert_eq!(
            collision.kept_from.as_deref(),
            Some(fixtures_dir().join("conflict_one.yaml").as_path()),
            "the first import's definition is the one kept"
        );
        assert_eq!(
            collision.dropped_from.as_path(),
            fixtures_dir().join("conflict_two.yaml").as_path(),
            "the second import's differing definition is the one dropped"
        );
        // The kept definition stands in the merged schema.
        assert_eq!(
            root.classes["Widget"].description.as_deref(),
            Some("A widget as defined by conflict_one")
        );
    }

    #[test]
    fn resolve_imports_unifies_identical_redefinition() {
        // `identical_root` imports two files that define `Gadget`
        // byte-identically. The two definitions agree, so they unify
        // silently: one merged element, no collision.
        let registry = FormatRegistry::with_defaults();
        let (mut root, path) = read_root("identical_root.yaml");

        let report = resolve_imports(&mut root, &path, &registry).expect("resolve imports");

        assert!(root.classes.contains_key("Gadget"));
        assert!(
            report.collisions.is_empty(),
            "identical redefinitions must not be reported as collisions: {:?}",
            report.collisions
        );
    }

    #[test]
    fn merge_schema_collides_on_differing_prefix_and_unifies_identical() {
        // The prefix map merges like the other element kinds: a prefix
        // bound to a *different* URI in the import collides (root wins);
        // a prefix bound *identically* unifies with no collision.
        let mut root = SchemaDefinition::new("root");
        root.prefixes
            .insert("ex".to_string(), "https://root.example/".to_string());
        root.prefixes
            .insert("shared".to_string(), "https://shared.example/".to_string());

        let mut imported = SchemaDefinition::new("imported");
        imported
            .prefixes
            .insert("ex".to_string(), "https://other.example/".to_string());
        imported
            .prefixes
            .insert("shared".to_string(), "https://shared.example/".to_string());

        let mut report = ImportReport::default();
        merge_schema(
            &mut root,
            &imported,
            std::path::Path::new("imported.yaml"),
            &mut report,
        );

        // Root keeps its own `ex` mapping.
        assert_eq!(root.prefixes.get("ex").unwrap(), "https://root.example/");
        // Exactly the differing `ex` collides; the identical `shared`
        // unifies silently.
        let prefix_collisions: Vec<_> = report
            .collisions
            .iter()
            .filter(|c| c.kind == "prefix")
            .collect();
        assert_eq!(
            prefix_collisions.len(),
            1,
            "only the differing prefix collides: {:?}",
            report.collisions
        );
        assert_eq!(prefix_collisions[0].name, "ex");
    }

    #[test]
    fn resolve_imports_detects_transitive_cycle() {
        // `tcycle_a` → `tcycle_b` → `tcycle_c` → `tcycle_a`. Cycle
        // detection holds across the full transitive graph, not just
        // direct self-imports: resolution errors rather than looping.
        let registry = FormatRegistry::with_defaults();
        let (mut root, path) = read_root("tcycle_a.yaml");

        let err = resolve_imports(&mut root, &path, &registry)
            .expect_err("a transitive import cycle must be rejected");
        assert!(
            matches!(err, ImportError::Cycle { .. }),
            "expected a cycle error, got {err:?}"
        );
    }

    #[test]
    fn resolve_imports_skips_builtin_linkml_import() {
        // The standard way to pull in LinkML's built-in types is
        // `imports: - linkml:types` under a `linkml:` prefix that expands
        // to a remote URI. That CURIE names a well-known module, not a
        // local file, so resolution skips it as a no-op: no error, and
        // the root's own elements survive with `imports` cleared.
        let registry = FormatRegistry::with_defaults();
        let mut root = SchemaDefinition::new("builtin_importer");
        root.prefixes
            .insert("linkml".into(), "https://w3id.org/linkml/".into());
        root.classes.insert(
            "Customer".into(),
            crate::linkml::ClassDefinition::new("Customer"),
        );
        root.imports = vec!["linkml:types".into()];
        let root_path = fixtures_dir().join("builtin_importer.yaml");

        let report = resolve_imports(&mut root, &root_path, &registry)
            .expect("a standard linkml: import must resolve as a built-in no-op");

        // The built-in import contributed nothing and was not treated as
        // a missing local file.
        assert!(report.loaded_files.is_empty());
        assert!(report.collisions.is_empty());
        // The root's own definition is untouched and imports are cleared.
        assert!(root.classes.contains_key("Customer"));
        assert!(root.imports.is_empty());
    }

    #[test]
    fn resolve_imports_skips_builtin_linkml_without_declared_prefix() {
        // The `linkml:` modules are built-ins recognized by name, not by
        // prefix expansion: a schema that imports `linkml:types` without
        // declaring the `linkml` prefix must still skip it, never fall
        // through to local-file resolution.
        let registry = FormatRegistry::with_defaults();
        let mut root = SchemaDefinition::new("builtin_importer");
        // Deliberately no `linkml` prefix declared.
        root.imports = vec!["linkml:types".into()];
        let root_path = fixtures_dir().join("builtin_importer.yaml");

        let report = resolve_imports(&mut root, &root_path, &registry)
            .expect("a linkml: import is a built-in even with no prefix declared");
        assert!(report.loaded_files.is_empty());
        assert!(root.imports.is_empty());
    }

    #[test]
    fn resolve_imports_skips_remote_url_and_remote_curie() {
        // Two non-`linkml` remote forms must skip local resolution: a
        // bare `http(s)` URL, and a CURIE whose declared prefix expands
        // to a remote URI. A prefix that does *not* expand to a remote
        // URI is left for local resolution (covered elsewhere).
        let registry = FormatRegistry::with_defaults();
        let mut root = SchemaDefinition::new("remote_importer");
        root.prefixes
            .insert("ex".into(), "https://example.org/".into());
        root.imports = vec![
            "https://w3id.org/linkml/types".into(),
            "ex:CoreTypes".into(),
        ];
        let root_path = fixtures_dir().join("remote_importer.yaml");

        let report = resolve_imports(&mut root, &root_path, &registry)
            .expect("a remote URL and a remote-expanding CURIE must both be skipped");
        assert!(report.loaded_files.is_empty());
        assert!(root.imports.is_empty());
    }

    #[test]
    fn resolve_imports_skips_builtin_alongside_local_import() {
        // A built-in CURIE and a local file in the same `imports:` list:
        // the CURIE is skipped, the local file still resolves and merges.
        // The fix must not over-skip genuine local imports.
        let registry = FormatRegistry::with_defaults();
        let (mut root, path) = read_root("app.yaml");
        // Prepend a standard linkml: import to the local `common` import.
        root.prefixes
            .insert("linkml".into(), "https://w3id.org/linkml/".into());
        root.imports.insert(0, "linkml:types".into());

        let report = resolve_imports(&mut root, &path, &registry).expect("resolve imports");

        // The local import still merged its class.
        assert!(root.classes.contains_key("Address"));
        // Only the local file was loaded; the built-in CURIE was skipped.
        assert_eq!(
            report.loaded_files,
            vec![fixtures_dir().join("common.yaml").canonicalize().unwrap()]
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
