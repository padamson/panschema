//! Diagnostics against two classes of silent drop: a construct panschema
//! doesn't model at all, and a construct it models but a specific writer
//! doesn't project.
//!
//! **Parse → IR.** `serde` silently ignores unknown YAML keys, so a
//! producer can write a real constraint (a boolean class expression, a
//! not-yet-modeled metaslot) and ship a schema where it is quietly
//! dropped. [`ClassDefinition`] captures such keys in its `unmodeled`
//! catch-all; [`unmodeled_class_constructs`] warns on them.
//!
//! The guard warns by **default**: the ignore-list starts empty, so every
//! unmodeled key is reported until a specific one is identified as safe to
//! silence. That direction is deliberate — an allowlist could only catch
//! drops we already anticipated, leaving the exact blind spot the guard
//! exists to close.
//!
//! **IR → writer.** A construct can be fully IR-modeled (so the guard
//! above never sees it) while a *specific* writer still doesn't project
//! it — e.g. `rules` and `unique_keys` render in HTML but aren't emitted
//! to RDF or Rust. [`classes_with_unprojected_constructs`] warns on that,
//! parameterized by the target format so the message names what was
//! actually requested.
//!
//! [`ClassDefinition`]: crate::linkml::ClassDefinition

use crate::linkml::SchemaDefinition;

/// Class-level LinkML keys panschema parses but deliberately does NOT
/// warn about — a **denylist that starts empty**.
///
/// Every unmodeled key warns until a specific key is identified as one
/// whose non-rendering is correct-by-definition (LinkML's equivalent of a
/// code comment) and added here *with its reason*. Starting empty is the
/// honest default: panschema surfaces every construct it doesn't handle,
/// and we silence individual keys only on evidence, never speculatively.
/// Never add a semantic/constraint construct here — model it, or let it
/// warn. See `docs/linkml-coverage.md`.
const IGNORED_CLASS_KEYS: &[&str] = &[];

/// One unmodeled key found on a class: the key and the class carrying it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmodeledConstruct {
    /// The class the construct was written on.
    pub class: String,
    /// The LinkML key that is parsed but not modeled (and not ignored).
    pub construct: String,
}

impl UnmodeledConstruct {
    /// A user-facing warning line.
    pub fn message(&self) -> String {
        format!(
            "`{}` on class `{}` is parsed but not modeled; it will not render or emit",
            self.construct, self.class
        )
    }
}

/// Report every class key that panschema parsed but did not model,
/// except the known-harmless ones, in a deterministic order (by class
/// name, then by key).
pub fn unmodeled_class_constructs(schema: &SchemaDefinition) -> Vec<UnmodeledConstruct> {
    scan(schema, IGNORED_CLASS_KEYS)
}

/// Whether `generate` should fail rather than merely warn: true only when
/// strict mode is on and at least one unmodeled construct was found.
pub fn should_fail_strict(findings: &[UnmodeledConstruct], strict: bool) -> bool {
    strict && !findings.is_empty()
}

/// The format-independent schema diagnostics the shared load path
/// ([`crate::import_resolve::load_schema`]) emits for every command —
/// unmodeled class constructs, and `unique_keys` naming a slot the class
/// lacks — as ready-to-print message bodies. Format-specific diagnostics
/// (writer projection gaps, Postgres/SHACL skips) and `--strict` enforcement
/// stay at the `generate` call site.
pub fn schema_load_diagnostics(schema: &SchemaDefinition) -> Vec<String> {
    let mut out = Vec::new();
    out.extend(
        unmodeled_class_constructs(schema)
            .iter()
            .map(|u| u.message()),
    );
    out.extend(
        unresolved_unique_key_slots(schema)
            .iter()
            .map(|u| u.message()),
    );
    out.extend(dangling_references(schema).iter().map(|d| d.message()));
    out
}

/// LinkML's standard built-in scalar types. A slot `range` naming one of
/// these resolves without a class/enum/`types:` definition, so it is not a
/// dangling reference. The full standard set is listed so a valid primitive
/// never trips the [`dangling_references`] warning; a schema's own custom
/// primitives live in `types:` and resolve there.
const LINKML_BUILTIN_TYPES: &[&str] = &[
    "string",
    "integer",
    "boolean",
    "float",
    "double",
    "decimal",
    "time",
    "date",
    "datetime",
    "date_or_datetime",
    "uriorcurie",
    "curie",
    "uri",
    "ncname",
    "objectidentifier",
    "nodeidentifier",
    "jsonpointer",
    "jsonpath",
    "sparqlpath",
];

/// A reference that fails to resolve after loading: a slot `range`, a class
/// `is_a` parent or `mixin`, or a slot `inverse` naming nothing the schema
/// defines. Each writer degrades a dangling reference differently and
/// silently (the graph drops the edge, the RDF/SHACL writers mint an IRI
/// from the bare name, Postgres falls back to `text`); this surfaces it once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DanglingRef {
    /// The slot or class carrying the reference, pre-formatted (e.g.
    /// ``slot `ships_to` ``).
    pub referrer: String,
    /// Which reference it is: `range`, `is_a`, `mixin`, or `inverse`.
    pub kind: &'static str,
    /// The unresolved name.
    pub name: String,
}

impl DanglingRef {
    /// A user-facing warning line naming the referrer, the reference kind, and
    /// the missing name.
    pub fn message(&self) -> String {
        let (verb, expected) = match self.kind {
            "range" => ("has range", "class, enum, type, or built-in type"),
            "is_a" => ("has parent", "class"),
            "mixin" => ("mixes in", "class"),
            "inverse" => ("has inverse", "slot"),
            _ => ("references", "definition"),
        };
        format!(
            "{} {verb} `{}`, which names no {expected} the schema defines",
            self.referrer, self.name
        )
    }
}

/// Report every reference that doesn't resolve against the loaded schema — a
/// slot `range` (must be a class, enum, `types:` entry, or built-in), a class
/// `is_a`/`mixin` (must be a class), or a slot `inverse` (must be a known
/// slot). Deterministic order: class references by class name, then slot
/// references (top-level slots, then inline attributes).
pub fn dangling_references(schema: &SchemaDefinition) -> Vec<DanglingRef> {
    let mut out = Vec::new();

    let resolves_as_type = |name: &str| {
        schema.classes.contains_key(name)
            || schema.enums.contains_key(name)
            || schema.types.contains_key(name)
            || LINKML_BUILTIN_TYPES.contains(&name)
    };

    // Every slot name the schema defines — top-level plus inline attributes —
    // so an `inverse` can resolve against either.
    let mut all_slot_names: std::collections::BTreeSet<&str> =
        schema.slots.keys().map(String::as_str).collect();
    for class in schema.classes.values() {
        all_slot_names.extend(class.attributes.keys().map(String::as_str));
    }

    // Class-level references.
    for (class_name, class) in &schema.classes {
        if let Some(parent) = &class.is_a
            && !schema.classes.contains_key(parent)
        {
            out.push(DanglingRef {
                referrer: format!("class `{class_name}`"),
                kind: "is_a",
                name: parent.clone(),
            });
        }
        for mixin in &class.mixins {
            if !schema.classes.contains_key(mixin) {
                out.push(DanglingRef {
                    referrer: format!("class `{class_name}`"),
                    kind: "mixin",
                    name: mixin.clone(),
                });
            }
        }
    }

    // Slot-level references (top-level slots, then inline attributes).
    let mut slots: Vec<(&str, &_)> = schema.slots.iter().map(|(n, s)| (n.as_str(), s)).collect();
    for class in schema.classes.values() {
        slots.extend(class.attributes.iter().map(|(n, s)| (n.as_str(), s)));
    }
    for (slot_name, slot) in slots {
        if let Some(range) = &slot.range
            && !resolves_as_type(range)
        {
            out.push(DanglingRef {
                referrer: format!("slot `{slot_name}`"),
                kind: "range",
                name: range.clone(),
            });
        }
        if let Some(inverse) = &slot.inverse
            && !all_slot_names.contains(inverse.as_str())
        {
            out.push(DanglingRef {
                referrer: format!("slot `{slot_name}`"),
                kind: "inverse",
                name: inverse.clone(),
            });
        }
    }

    out
}

/// The detection mechanism, parameterized by the ignore-list so tests can
/// exercise it with fabricated keys decoupled from the real list. Warns
/// by default: an unmodeled key is reported unless it is in `ignored`.
fn scan(schema: &SchemaDefinition, ignored: &[&str]) -> Vec<UnmodeledConstruct> {
    let mut found = Vec::new();
    // `classes` and each `unmodeled` map are BTreeMaps, so iteration is
    // name-sorted → a stable report.
    for (class_name, class) in &schema.classes {
        for key in class.unmodeled.keys() {
            if ignored.contains(&key.as_str()) {
                continue;
            }
            found.push(UnmodeledConstruct {
                class: class_name.clone(),
                construct: key.clone(),
            });
        }
    }
    found
}

/// One class-level construct that's IR-modeled — so
/// [`unmodeled_class_constructs`] never sees it — but that the target
/// format's writer doesn't project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnprojectedConstruct {
    /// The class carrying the construct.
    pub class: String,
    /// The construct name (`"rules"` or `"unique_keys"` today).
    pub construct: &'static str,
}

impl UnprojectedConstruct {
    /// A user-facing warning line naming the format that was actually
    /// requested — not a hardcoded one, so `--format rust` doesn't claim
    /// an RDF-specific gap it has nothing to do with.
    pub fn message(&self, format: &str) -> String {
        format!(
            "class `{}` declares `{}`, which panschema does not emit to the `{}` format",
            self.class, self.construct, format
        )
    }
}

/// Report every class-level construct that's IR-modeled but that `format`
/// doesn't project — a second, narrower class of silent drop than
/// [`unmodeled_class_constructs`]: `rules` and `unique_keys` are IR-modeled,
/// so they never reach the `unmodeled` catch-all, but not every writer
/// projects them (HTML and Postgres project both; SHACL projects `rules`
/// only; the rest project neither). Empty for the formats that project the
/// construct; call for every target format.
pub fn classes_with_unprojected_constructs(
    schema: &SchemaDefinition,
    format: &str,
) -> Vec<UnprojectedConstruct> {
    // HTML renders both constructs; Postgres projects both (`unique_keys`
    // as UNIQUE, `rules` as conditional CHECK) — so neither format has an
    // unprojected-construct gap here. Partial cases (an unresolvable
    // unique-key slot, a rule that can't become a CHECK) are surfaced by
    // their own per-construct diagnostics, not this blanket one.
    if format.eq_ignore_ascii_case("html") || format.eq_ignore_ascii_case("postgres") {
        return Vec::new();
    }
    // SHACL projects `rules` (as conditional shapes) but not `unique_keys`
    // yet (SHACL Core has no cross-instance uniqueness) — so for shacl only
    // `unique_keys` is still an unprojected gap.
    let rules_projected = format.eq_ignore_ascii_case("shacl");
    let mut found = Vec::new();
    for (class_name, class) in &schema.classes {
        if !class.rules.is_empty() && !rules_projected {
            found.push(UnprojectedConstruct {
                class: class_name.clone(),
                construct: "rules",
            });
        }
        if !class.unique_keys.is_empty() {
            found.push(UnprojectedConstruct {
                class: class_name.clone(),
                construct: "unique_keys",
            });
        }
    }
    found
}

/// A `unique_keys` slot that doesn't resolve to any slot on its class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedKeySlot {
    /// The class carrying the `unique_keys` entry.
    pub class: String,
    /// The `unique_keys` entry (map key) naming the constraint.
    pub key: String,
    /// The referenced slot name that isn't in the class's effective set.
    pub slot: String,
}

impl UnresolvedKeySlot {
    /// A user-facing warning line.
    pub fn message(&self) -> String {
        format!(
            "unique key `{}` on class `{}` references slot `{}`, which the class does not have",
            self.key, self.class, self.slot
        )
    }
}

/// Report every `unique_keys` slot that names a slot the class doesn't
/// actually have, checked against its *effective* slot set (inherited +
/// mixin + inline + `slot_usage`), in deterministic order.
///
/// A structural check with no home yet: a dedicated `validate` surface
/// isn't built, so this routes through the same `generate`-time
/// `eprintln!` warning path as the other diagnostics until it lands.
pub fn unresolved_unique_key_slots(schema: &SchemaDefinition) -> Vec<UnresolvedKeySlot> {
    let mut found = Vec::new();
    for (class_name, class) in &schema.classes {
        if class.unique_keys.is_empty() {
            continue;
        }
        let effective = crate::linkml_resolve::resolve_effective_slots(class, schema);
        for (key_name, key) in &class.unique_keys {
            for slot in &key.unique_key_slots {
                if !effective.contains_key(slot) {
                    found.push(UnresolvedKeySlot {
                        class: class_name.clone(),
                        key: key_name.clone(),
                        slot: slot.clone(),
                    });
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> SchemaDefinition {
        serde_yaml::from_str(yaml).expect("parse schema")
    }

    // Fabricated key — never a real LinkML key — so these mechanism
    // tests stay valid regardless of which real keys are modeled or added
    // to the ignore-list over time.
    const UNKNOWN_KEY: &str = "panschema_test_unmodeled_key";

    #[test]
    fn warns_on_any_unmodeled_key_by_default() {
        // The guard's whole point: an unmodeled key we never enumerated is
        // reported anyway (empty ignore-list ⇒ warn).
        let schema = parse(&format!("name: s\nclasses:\n  C:\n    {UNKNOWN_KEY}: []\n"));
        assert_eq!(
            scan(&schema, &[]),
            vec![UnmodeledConstruct {
                class: "C".to_string(),
                construct: UNKNOWN_KEY.to_string(),
            }]
        );
    }

    #[test]
    fn silences_a_key_on_the_ignore_list() {
        let schema = parse(&format!("name: s\nclasses:\n  C:\n    {UNKNOWN_KEY}: []\n"));
        assert!(scan(&schema, &[UNKNOWN_KEY]).is_empty());
    }

    #[test]
    fn public_fn_reports_unmodeled_keys_through_the_real_ignore_list() {
        // Pins the public entry point (real, empty ignore-list) to
        // actually scan and report — not return nothing.
        let schema = parse(&format!("name: s\nclasses:\n  C:\n    {UNKNOWN_KEY}: []\n"));
        let found = unmodeled_class_constructs(&schema);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].construct, UNKNOWN_KEY);
        assert_eq!(found[0].class, "C");
    }

    #[test]
    fn schema_load_diagnostics_reports_unmodeled_and_unresolved_unique_keys() {
        // The shared load path collects the format-independent schema
        // diagnostics — an unmodeled construct and a `unique_key` naming a slot
        // the class lacks — so `serve` and `publish` surface them just like
        // `generate`, instead of only `generate` warning.
        let schema = parse(&format!(
            "name: s\nclasses:\n  C:\n    {UNKNOWN_KEY}: []\n  Keyed:\n    unique_keys:\n      k:\n        unique_key_slots: [missing]\n"
        ));
        let msgs = schema_load_diagnostics(&schema);
        assert!(
            msgs.iter().any(|m| m.contains(UNKNOWN_KEY)),
            "expected an unmodeled-construct message; got: {msgs:?}"
        );
        assert!(
            msgs.iter().any(|m| m.contains("missing")),
            "expected an unresolved unique-key-slot message; got: {msgs:?}"
        );
    }

    #[test]
    fn dangling_references_flags_a_range_naming_a_missing_class() {
        // A slot range that names no class, enum, type, or built-in primitive
        // is a dangling reference — one clear warning, instead of each writer
        // silently degrading (graph drops the edge, RDF/SHACL fabricate an IRI).
        let schema = parse(
            "name: s\nclasses:\n  Order:\n    slots: [ships_to]\nslots:\n  ships_to:\n    range: Warehouse\n",
        );
        let msgs: Vec<String> = dangling_references(&schema)
            .iter()
            .map(|d| d.message())
            .collect();
        assert!(
            msgs.iter()
                .any(|m| m.contains("ships_to") && m.contains("Warehouse")),
            "expected a dangling-range warning naming `ships_to` -> `Warehouse`; got: {msgs:?}"
        );
    }

    #[test]
    fn dangling_references_accepts_builtin_primitive_ranges() {
        // A valid LinkML primitive range must NOT be flagged — the whole point
        // is to catch typo'd class names, not every non-class range.
        let schema = parse(
            "name: s\nclasses:\n  Order:\n    slots: [code]\nslots:\n  code:\n    range: string\n",
        );
        assert!(
            dangling_references(&schema).is_empty(),
            "a built-in primitive range must not be reported as dangling"
        );
    }

    #[test]
    fn dangling_references_flags_every_reference_kind_with_its_own_message() {
        // Each of the four reference kinds is reported, and each message names
        // its kind — a range, an is_a parent, a mixin, and an inverse that all
        // resolve to nothing.
        let schema = parse(
            "name: s\nclasses:\n  Bad:\n    is_a: MissingParent\n    mixins: [MissingMixin]\nslots:\n  r:\n    range: NoSuchClass\n  inv:\n    inverse: no_such_slot\n",
        );
        let msgs: Vec<String> = dangling_references(&schema)
            .iter()
            .map(|d| d.message())
            .collect();
        assert!(
            msgs.iter()
                .any(|m| m.contains("has range") && m.contains("NoSuchClass")),
            "range message missing or unlabeled; got: {msgs:?}"
        );
        assert!(
            msgs.iter()
                .any(|m| m.contains("has parent") && m.contains("MissingParent")),
            "is_a message missing or unlabeled; got: {msgs:?}"
        );
        assert!(
            msgs.iter()
                .any(|m| m.contains("mixes in") && m.contains("MissingMixin")),
            "mixin message missing or unlabeled; got: {msgs:?}"
        );
        assert!(
            msgs.iter()
                .any(|m| m.contains("has inverse") && m.contains("no_such_slot")),
            "inverse message missing or unlabeled; got: {msgs:?}"
        );
    }

    #[test]
    fn dangling_references_accepts_all_resolving_reference_kinds() {
        // Every reference resolves — is_a/mixin to a class, a range to a class,
        // an enum, a `types:` entry, and a built-in, and an inverse to a known
        // slot — so nothing is flagged. Pins each resolution branch.
        let schema = parse(
            "name: s\nenums:\n  Color: {}\ntypes:\n  MyStr: {}\nclasses:\n  Base: {}\n  Sub:\n    is_a: Base\n    mixins: [Base]\nslots:\n  to_class:\n    range: Base\n  to_enum:\n    range: Color\n  to_type:\n    range: MyStr\n  to_builtin:\n    range: string\n  fwd:\n    inverse: bwd\n  bwd: {}\n",
        );
        assert!(
            dangling_references(&schema).is_empty(),
            "all references resolve, so none should be flagged; got: {:?}",
            dangling_references(&schema)
        );
    }

    #[test]
    fn message_names_the_construct_and_class() {
        let msg = UnmodeledConstruct {
            class: "Deployment".to_string(),
            construct: "rules".to_string(),
        }
        .message();
        assert!(
            msg.contains("rules") && msg.contains("Deployment"),
            "message must name the construct and class; got: {msg}"
        );
    }

    #[test]
    fn strict_fails_only_when_strict_and_findings_present() {
        let some = vec![UnmodeledConstruct {
            class: "C".to_string(),
            construct: "rules".to_string(),
        }];
        let none: Vec<UnmodeledConstruct> = Vec::new();
        assert!(should_fail_strict(&some, true), "strict + findings ⇒ fail");
        assert!(!should_fail_strict(&some, false), "not strict ⇒ never fail");
        assert!(
            !should_fail_strict(&none, true),
            "strict + no findings ⇒ ok"
        );
        assert!(!should_fail_strict(&none, false));
    }

    #[test]
    fn classes_with_unprojected_constructs_covers_rules_and_unique_keys() {
        let schema = parse(
            "name: s\nclasses:\n  Deployment:\n    rules:\n      - description: d\n  Offering:\n    unique_keys:\n      k:\n        unique_key_slots: [x]\n  Bare:\n    description: neither\n",
        );
        let mut found = classes_with_unprojected_constructs(&schema, "ttl");
        found.sort_by(|a, b| (a.class.as_str(), a.construct).cmp(&(b.class.as_str(), b.construct)));
        assert_eq!(
            found,
            vec![
                UnprojectedConstruct {
                    class: "Deployment".to_string(),
                    construct: "rules",
                },
                UnprojectedConstruct {
                    class: "Offering".to_string(),
                    construct: "unique_keys",
                },
            ]
        );
    }

    #[test]
    fn postgres_projects_both_rules_and_unique_keys_so_neither_is_flagged() {
        // The Postgres writer emits both `unique_keys` (UNIQUE) and `rules`
        // (conditional CHECK), so it must not warn that either won't appear.
        // The partial cases — an unresolvable unique-key slot, a rule that
        // can't become a CHECK — are surfaced by their own per-construct
        // diagnostics, not this blanket one.
        let schema = parse(
            "name: s\nclasses:\n  Deployment:\n    rules:\n      - description: d\n  Offering:\n    unique_keys:\n      k:\n        unique_key_slots: [x]\n",
        );
        assert!(
            classes_with_unprojected_constructs(&schema, "postgres").is_empty(),
            "postgres projects both constructs; got: {:?}",
            classes_with_unprojected_constructs(&schema, "postgres")
        );
    }

    #[test]
    fn shacl_projects_rules_so_only_unique_keys_is_flagged() {
        // The SHACL writer emits `rules` as conditional shapes, so it must
        // not warn they won't appear — but it has no `unique_keys`
        // projection yet (SHACL Core has no cross-instance uniqueness), so
        // that one still warns.
        let schema = parse(
            "name: s\nclasses:\n  Deployment:\n    rules:\n      - description: d\n  Offering:\n    unique_keys:\n      k:\n        unique_key_slots: [x]\n",
        );
        let found = classes_with_unprojected_constructs(&schema, "shacl");
        assert_eq!(
            found,
            vec![UnprojectedConstruct {
                class: "Offering".to_string(),
                construct: "unique_keys",
            }],
            "shacl must flag unique_keys but not rules; got: {found:?}"
        );
    }

    #[test]
    fn classes_with_unprojected_constructs_empty_for_html() {
        // HTML is the one writer that fully projects both constructs
        // today — case-insensitively, matching the CLI's format matching.
        let schema =
            parse("name: s\nclasses:\n  Deployment:\n    rules:\n      - description: d\n");
        assert!(classes_with_unprojected_constructs(&schema, "html").is_empty());
        assert!(classes_with_unprojected_constructs(&schema, "HTML").is_empty());
    }

    #[test]
    fn classes_with_unprojected_constructs_empty_when_neither_present() {
        let schema = parse("name: s\nclasses:\n  Bare:\n    description: x\n");
        assert!(classes_with_unprojected_constructs(&schema, "rust").is_empty());
    }

    #[test]
    fn unprojected_construct_message_names_the_requested_format() {
        // An earlier version of this message hardcoded "RDF/OWL" even for
        // `--format rust`. The format argument must flow through into the
        // message verbatim.
        let msg = UnprojectedConstruct {
            class: "Deployment".to_string(),
            construct: "rules",
        }
        .message("rust");
        assert!(
            msg.contains("rust") && msg.contains("Deployment") && msg.contains("rules"),
            "message must name the requested format, class, and construct; got: {msg}"
        );
        assert!(
            !msg.contains("RDF/OWL"),
            "message must not hardcode a format the caller didn't request; got: {msg}"
        );
    }

    // The resolver keys cycle-detection on `ClassDefinition.name`, which the
    // YAML reader backfills from the map key before any diagnostic runs;
    // these tests build classes with names already set to match that
    // precondition (the raw `parse` helper skips backfill).
    use crate::linkml::{ClassDefinition, SlotDefinition, UniqueKey};

    fn class_with_attr(name: &str, attr: &str) -> ClassDefinition {
        let mut c = ClassDefinition::new(name);
        c.attributes
            .insert(attr.to_string(), SlotDefinition::new(attr));
        c
    }

    #[test]
    fn unresolved_unique_key_slots_flags_a_slot_the_class_lacks() {
        // `offered_by` is a real attribute; `ghost` is not — only the
        // latter is flagged, and it names the class, key, and slot.
        let mut schema = SchemaDefinition::new("s");
        let mut offering = class_with_attr("Offering", "offered_by");
        offering.unique_keys.insert(
            "k".to_string(),
            UniqueKey {
                unique_key_slots: vec!["offered_by".to_string(), "ghost".to_string()],
                description: None,
            },
        );
        schema.classes.insert("Offering".to_string(), offering);
        assert_eq!(
            unresolved_unique_key_slots(&schema),
            vec![UnresolvedKeySlot {
                class: "Offering".to_string(),
                key: "k".to_string(),
                slot: "ghost".to_string(),
            }]
        );
    }

    #[test]
    fn unresolved_unique_key_slots_resolves_inherited_slots() {
        // A key slot defined on an `is_a` parent is in the effective set,
        // so it does not warn.
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Base".to_string(), class_with_attr("Base", "name"));
        let mut sub = ClassDefinition::new("Sub");
        sub.is_a = Some("Base".to_string());
        sub.unique_keys.insert(
            "k".to_string(),
            UniqueKey {
                unique_key_slots: vec!["name".to_string()],
                description: None,
            },
        );
        schema.classes.insert("Sub".to_string(), sub);
        assert!(
            unresolved_unique_key_slots(&schema).is_empty(),
            "an inherited slot must resolve"
        );
    }

    #[test]
    fn unresolved_unique_key_slots_message_names_class_key_slot() {
        let msg = UnresolvedKeySlot {
            class: "Offering".to_string(),
            key: "k".to_string(),
            slot: "ghost".to_string(),
        }
        .message();
        assert!(
            msg.contains("Offering") && msg.contains("`k`") && msg.contains("ghost"),
            "message must name class, key, and slot; got: {msg}"
        );
    }

    #[test]
    fn silent_on_modeled_keys() {
        // Modeled keys map to named fields and never reach the `unmodeled`
        // catch-all, so they never warn — independent of the (currently
        // empty) ignore-list.
        let schema = parse(
            "name: s\nclasses:\n  C:\n    description: d\n    abstract: true\n    mixins: [M]\n",
        );
        assert!(
            unmodeled_class_constructs(&schema).is_empty(),
            "modeled keys must not warn"
        );
    }
}
