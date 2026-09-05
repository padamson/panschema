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

/// Whether `generate` should fail rather than merely warn: true only
/// when strict mode is on and the schema has at least one blocking
/// finding. The caller sums the counts it already computes for its own
/// message, so adding a finding kind never widens this signature.
pub fn should_fail_strict(strict: bool, blocking_findings: usize) -> bool {
    strict && blocking_findings > 0
}

/// Which side of a rule carries an impossible constant — the sides fail
/// differently, so the message says which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RuleSide {
    Precondition,
    Postcondition,
}

/// A rule's `equals_string` constant that is not a permissible value of
/// its slot's enum: at a precondition the rule can never fire, at a
/// postcondition no record can ever satisfy it — either way a defect
/// that otherwise dies silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpossibleRuleValue {
    pub class: String,
    /// The rule's title, or its 1-based position (`#2`) when untitled.
    pub rule: String,
    pub slot: String,
    pub value: String,
    pub enum_name: String,
    pub side: RuleSide,
    /// The constant sits in an `any_of` alternative: a dead branch, not
    /// a dead rule — a sibling alternative may still satisfy the side.
    pub alternative: bool,
}

impl ImpossibleRuleValue {
    pub fn message(&self) -> String {
        let (where_, consequence) = match (self.side, self.alternative) {
            (RuleSide::Precondition, false) => ("precondition", "the rule can never fire"),
            (RuleSide::Precondition, true) => (
                "precondition alternative",
                "this alternative can never hold, though a sibling may still fire the rule",
            ),
            (RuleSide::Postcondition, false) => ("postcondition", "no record can satisfy the rule"),
            (RuleSide::Postcondition, true) => (
                "postcondition alternative",
                "this alternative can never hold, though a sibling may still satisfy the rule",
            ),
        };
        format!(
            "class `{}` rule `{}`: {where_} tests `{}` for `{}`, which is not a \
             permissible value of enum `{}` — {consequence}",
            self.class, self.rule, self.slot, self.value, self.enum_name
        )
    }
}

/// Rule constants checked against their slots' enum value spaces, on
/// the resolved view (each class's effective slots), so `slot_usage`
/// ranges count. The walk and its conservatism live in
/// [`crate::rules::enum_equals_constants`], and membership is
/// [`crate::rules::permitted_value_key`] — the same matching `validate`
/// enforces — so what this refuses is exactly what conforming data
/// could never hold. One finding per distinct defect, however many
/// `any_of` branches repeat it.
pub fn impossible_rule_values(schema: &SchemaDefinition) -> Vec<ImpossibleRuleValue> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (class_name, class) in &schema.classes {
        if class.rules.is_empty() {
            continue;
        }
        let resolved =
            crate::linkml_resolve::resolve_effective_slots_with_provenance(class, schema);
        for (index, rule) in class.rules.iter().enumerate() {
            let rule_name = crate::rules::rule_label(rule, index);
            for (side, conditions) in [
                (RuleSide::Precondition, rule.preconditions.as_ref()),
                (RuleSide::Postcondition, rule.postconditions.as_ref()),
            ] {
                let Some(conditions) = conditions else {
                    continue;
                };
                for constant in crate::rules::enum_equals_constants(conditions, &resolved, schema) {
                    if schema
                        .enums
                        .get(&constant.enum_name)
                        .and_then(|e| crate::rules::permitted_value_key(e, &constant.value))
                        .is_some()
                    {
                        continue;
                    }
                    // Keyed by rule position, not label: same-titled
                    // rules are distinct defects.
                    if seen.insert((
                        class_name.clone(),
                        index,
                        constant.slot.clone(),
                        constant.value.clone(),
                        side,
                    )) {
                        out.push(ImpossibleRuleValue {
                            class: class_name.clone(),
                            rule: rule_name.clone(),
                            slot: constant.slot,
                            value: constant.value,
                            enum_name: constant.enum_name,
                            side,
                            alternative: constant.alternative,
                        });
                    }
                }
            }
        }
    }
    out
}

/// A slot name defined at more than one site whose definitions would mint
/// one RDF property IRI — distinct slots in LinkML, one property in OWL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollidingSlot {
    pub name: String,
    /// Where each colliding definition lives, sorted: `` class `X` `` or
    /// `` top-level `slots:` ``.
    pub sites: Vec<String>,
}

impl CollidingSlot {
    pub fn message(&self) -> String {
        format!(
            "slot `{}` is defined at {} sites ({}); RDF output mints one property \
             IRI for all of them, so only one definition survives a read-back and \
             the emitted OWL asserts a single property about every declaring class \
             — rename them, share one top-level slot, or give each an explicit \
             `slot_uri`",
            self.name,
            self.sites.len(),
            self.sites.join(", ")
        )
    }
}

/// Slot names whose definitions collide at one RDF property IRI.
///
/// A *definition site* is a top-level `slots:` entry or a class's
/// `attributes:` entry. A class listing a top-level slot via `slots:` is a
/// *use*, not a definition — sharing one slot is the supported way to say
/// two classes carry the same property. Identity follows IRI minting: an
/// explicit `slot_uri` is its own identity, everything else falls back to
/// the name, mirroring how the RDF writers derive property IRIs.
pub fn colliding_slot_definitions(schema: &SchemaDefinition) -> Vec<CollidingSlot> {
    use std::collections::BTreeMap;
    // identity key -> (display name, sorted definition sites)
    let mut sites: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    let mut record = |slot: &crate::linkml::SlotDefinition, name: &str, site: String| {
        let key = match &slot.slot_uri {
            Some(uri) => format!("uri:{uri}"),
            None => format!("name:{name}"),
        };
        let entry = sites
            .entry(key)
            .or_insert_with(|| (name.to_string(), Vec::new()));
        entry.1.push(site);
    };
    for (name, slot) in &schema.slots {
        record(slot, name, "top-level `slots:`".to_string());
    }
    for (class_name, class) in &schema.classes {
        for (name, slot) in &class.attributes {
            record(slot, name, format!("class `{class_name}`"));
        }
    }
    sites
        .into_values()
        .filter(|(_, s)| s.len() > 1)
        .map(|(name, mut sites)| {
            sites.sort();
            CollidingSlot { name, sites }
        })
        .collect()
}

/// An annotation whose authored body carried nested `annotations` or
/// `extensions`. panschema keeps the annotation's value and does not
/// model the nesting, so the drop is reported rather than silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmodeledAnnotationNesting {
    /// Where the annotation sits: `` schema `X` ``, `` class `X` ``,
    /// `` slot `X` ``, `` enum `X` ``, or `` type `X` ``.
    pub site: String,
    pub tag: String,
}

impl UnmodeledAnnotationNesting {
    pub fn message(&self) -> String {
        format!(
            "annotation `{}` ({}) carries nested annotations or extensions, which \
             panschema does not model; the annotation's value is kept, the nesting \
             is dropped",
            self.tag, self.site
        )
    }
}

/// Walk every annotation-bearing site in the schema — the schema
/// itself, classes, class attributes, class `slot_usage` entries,
/// top-level slots, enums, and types — handing each visitor the
/// annotations plus a site formatter it calls only when it has a
/// finding, so the common no-findings walk allocates nothing.
fn each_annotation_site<F>(schema: &SchemaDefinition, mut visit: F)
where
    F: FnMut(&crate::linkml::Annotations, &dyn Fn() -> String),
{
    visit(&schema.annotations, &|| format!("schema `{}`", schema.name));
    for (name, class) in &schema.classes {
        visit(&class.annotations, &|| format!("class `{name}`"));
        for (attr_name, attr) in &class.attributes {
            visit(&attr.annotations, &|| {
                format!("slot `{attr_name}` (class `{name}`)")
            });
        }
        for (slot_name, usage) in &class.slot_usage {
            visit(&usage.annotations, &|| {
                format!("slot `{slot_name}` (slot_usage in class `{name}`)")
            });
        }
    }
    for (name, slot) in &schema.slots {
        visit(&slot.annotations, &|| format!("slot `{name}`"));
    }
    for (name, enum_def) in &schema.enums {
        visit(&enum_def.annotations, &|| format!("enum `{name}`"));
    }
    for (name, type_def) in &schema.types {
        visit(&type_def.annotations, &|| format!("type `{name}`"));
    }
}

/// Every annotation whose body carried nesting panschema dropped at
/// load, across every annotation-bearing site, in site order.
pub fn unmodeled_annotation_nesting(schema: &SchemaDefinition) -> Vec<UnmodeledAnnotationNesting> {
    let mut out = Vec::new();
    each_annotation_site(schema, |annotations, site| {
        let mut tags = annotations.tags_with_unmodeled_nesting().peekable();
        if tags.peek().is_none() {
            return;
        }
        let site = site();
        out.extend(tags.map(|tag| UnmodeledAnnotationNesting {
            site: site.clone(),
            tag: tag.to_string(),
        }));
    });
    out
}

/// A `panschema:`-namespace annotation carrying a non-string value.
/// The tool reads its own tags as strings (bare scalars included, read
/// lexically), so a structured or null value under one is ignored —
/// reported here so the fallback (a raw name for a label, an inferred
/// property type, a missing individual assertion) is never silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpaquePanschemaAnnotation {
    /// Where the annotation sits, phrased as in
    /// [`UnmodeledAnnotationNesting::site`].
    pub site: String,
    pub tag: String,
}

impl OpaquePanschemaAnnotation {
    pub fn message(&self) -> String {
        format!(
            "annotation `{}` ({}) is in the `panschema:` namespace but does not \
             carry a string value, so panschema ignores it and falls back as if \
             it were absent",
            self.tag, self.site
        )
    }
}

/// Every `panschema:*` annotation whose value is not a string, across
/// every annotation-bearing site, in site order.
pub fn opaque_panschema_annotations(schema: &SchemaDefinition) -> Vec<OpaquePanschemaAnnotation> {
    let mut out = Vec::new();
    each_annotation_site(schema, |annotations, site| {
        let mut opaque = annotations
            .into_iter()
            .filter(|(tag, _)| tag.starts_with("panschema:"))
            .filter(|(tag, _)| annotations.get_str(tag).is_none())
            .peekable();
        if opaque.peek().is_none() {
            return;
        }
        let site = site();
        out.extend(opaque.map(|(tag, _)| OpaquePanschemaAnnotation {
            site: site.clone(),
            tag: tag.clone(),
        }));
    });
    out
}

/// A slot left untyped after load: it states no `range:`, carries no
/// `any_of` union, is not voided by `maximum_cardinality: 0`, and no
/// `default_range` applied to it when its schema file loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntypedSlot {
    pub name: String,
    /// Where the slot is defined: `` class `X` `` or `` top-level `slots:` ``.
    pub site: String,
}

impl UntypedSlot {
    pub fn message(&self) -> String {
        format!(
            "slot `{}` ({}) resolves with no `range`, and no `default_range` applied to it \
             at load — the outputs disagree on what that means: JSON Schema types it as \
             `string` while RDF, SHACL, Postgres, HTML, and `verify` leave it \
             unconstrained; declare a range (`range:` or `slot_usage` in YAML, `rdfs:range` \
             in OWL/Turtle)",
            self.name, self.site
        )
    }
}

/// Slots left untyped after load-time `default_range` materialization,
/// read from the **resolved** view — each class's effective slots, with
/// `is_a`/mixin inheritance and `slot_usage` overrides applied — so a
/// top-level slot every consumer ranges via `slot_usage` is not reported,
/// and a slot a class introduces *only* through `slot_usage` (which no
/// default can fill) is. A top-level slot no class uses is checked raw:
/// the RDF, HTML, and graph writers project it anyway.
///
/// "Rangeless" is [`crate::linkml_resolve::default_range_would_fill`] —
/// the same predicate the loader fills by — so a slot is reported
/// precisely when no default could have typed it, including a slot from
/// an imported file that declares no `default_range` of its own, whatever
/// the root schema declares.
pub fn untyped_slots(schema: &SchemaDefinition) -> Vec<UntypedSlot> {
    let untyped = crate::linkml_resolve::default_range_would_fill;
    let mut used: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for (class_name, class) in &schema.classes {
        for (name, slot) in crate::linkml_resolve::resolve_effective_slots(class, schema) {
            if untyped(&slot) {
                out.push(UntypedSlot {
                    name: name.clone(),
                    site: format!("class `{class_name}`"),
                });
            }
            used.insert(name);
        }
    }
    for (name, slot) in &schema.slots {
        if !used.contains(name) && untyped(slot) {
            out.push(UntypedSlot {
                name: name.clone(),
                site: "top-level `slots:`".to_string(),
            });
        }
    }
    out
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
    out.extend(
        unchecked_specializations(schema)
            .iter()
            .map(|u| u.message()),
    );
    out.extend(
        colliding_slot_definitions(schema)
            .iter()
            .map(|c| c.message()),
    );
    out.extend(untyped_slots(schema).iter().map(|u| u.message()));
    out.extend(
        unmodeled_annotation_nesting(schema)
            .iter()
            .map(|u| u.message()),
    );
    out.extend(
        opaque_panschema_annotations(schema)
            .iter()
            .map(|o| o.message()),
    );
    for family in SLOT_SEMANTICS_FAMILIES {
        out.extend((family.issues)(schema));
    }
    out.extend(impossible_rule_values(schema).iter().map(|i| i.message()));
    // The metamodel recommends at most one `tree_root` per schema. Several
    // are supported here — each dataset is read against the root it conforms
    // to — but the deviation from that "should" is stated, because upstream
    // LinkML tooling may warn on the schema or pick one root arbitrarily.
    let roots: Vec<&String> = schema
        .classes
        .iter()
        .filter(|(_, c)| c.tree_root)
        .map(|(name, _)| name)
        .collect();
    if roots.len() > 1 {
        out.push(format!(
            "schema declares {} `tree_root` classes ({}); the LinkML metamodel \
             recommends at most one — panschema reads each dataset against the \
             root it conforms to, but other LinkML tooling may not",
            roots.len(),
            roots
                .iter()
                .map(|r| format!("`{r}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
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
    /// Which reference it is: `default_range`, `range`, `is_a`, `mixin`,
    /// `inverse`, or `specializes` (a slot-level `is_a`).
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
            "default_range" => ("has default_range", "class, enum, type, or built-in type"),
            "is_a" => ("has parent", "class"),
            "mixin" => ("mixes in", "class"),
            "inverse" => ("has inverse", "slot"),
            "specializes" => ("specializes", "slot"),
            _ => ("references", "definition"),
        };
        format!(
            "{} {verb} `{}`, which names no {expected} the schema defines",
            self.referrer, self.name
        )
    }
}

/// Report every reference that doesn't resolve against the loaded schema —
/// the schema's `default_range`, a slot `range` (each must be a class, enum,
/// `types:` entry, or built-in), a class `is_a`/`mixin` (must be a class), or
/// a slot `inverse` (must be a known slot). Deterministic order: the schema
/// reference, then class references by class name, then slot references
/// (top-level slots, then inline attributes).
pub fn dangling_references(schema: &SchemaDefinition) -> Vec<DanglingRef> {
    let mut out = Vec::new();

    let resolves_as_type = |name: &str| {
        schema.classes.contains_key(name)
            || schema.enums.contains_key(name)
            || schema.types.contains_key(name)
            || LINKML_BUILTIN_TYPES.contains(&name)
    };

    // A typo'd `default_range` otherwise fails silently: it types nothing,
    // and a schema whose slots all declare ranges shows no symptom at all.
    if let Some(default) = schema.default_range.as_deref()
        && !resolves_as_type(default)
    {
        out.push(DanglingRef {
            referrer: "schema".to_string(),
            kind: "default_range",
            name: default.to_string(),
        });
    }

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
        if let Some(parent) = &slot.is_a
            && !all_slot_names.contains(parent.as_str())
        {
            out.push(DanglingRef {
                referrer: format!("slot `{slot_name}`"),
                kind: "specializes",
                name: parent.clone(),
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

    // `slot_usage` overrides can introduce references of their own — an
    // `is_a` declared there resolves against the same slot namespace, and a
    // typo'd parent would otherwise evaporate with the whole subset claim.
    for (class_name, class) in &schema.classes {
        for (slot_name, override_def) in &class.slot_usage {
            if let Some(parent) = &override_def.is_a
                && !all_slot_names.contains(parent.as_str())
            {
                out.push(DanglingRef {
                    referrer: format!("slot `{slot_name}` (class `{class_name}`)"),
                    kind: "specializes",
                    name: parent.clone(),
                });
            }
        }
    }

    out
}

/// A typed instance reference (an A-box object assertion) whose target
/// identifier names no instance in the data — the A-box analog of a dangling
/// schema reference. Surfaced so an authoring loop (e.g. an LLM agent building
/// an instance graph) gets a concrete "fix this" signal instead of a silently
/// dropped edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DanglingInstanceRef {
    /// The referring instance's id.
    pub referrer: String,
    /// The property whose value is the missing reference.
    pub property: String,
    /// The unresolved target identifier.
    pub target: String,
}

impl DanglingInstanceRef {
    /// The problem clause naming the property and missing target, without the
    /// referrer — for a caller that labels the record its own way.
    pub fn detail(&self) -> String {
        format!(
            "property `{}` references `{}`, which names no instance in the data",
            self.property, self.target
        )
    }

    /// A standalone warning line naming the referring record, the property,
    /// and the missing target id.
    pub fn message(&self) -> String {
        format!("instance `{}` {}", self.referrer, self.detail())
    }
}

/// Every typed reference in `set` whose target isn't the id of some instance in
/// `set`. Deterministic: sorted by referrer id, then property, then target.
pub fn dangling_instance_references(
    set: &crate::instances::InstanceSet,
) -> Vec<DanglingInstanceRef> {
    use std::collections::HashSet;
    let ids: HashSet<&str> = set.instances.iter().map(|i| i.id.as_str()).collect();
    let mut out = Vec::new();
    for inst in &set.instances {
        for r in &inst.references {
            // A reference that points outside this dataset names no record
            // here by design; reporting it as dangling would make every
            // cross-graph edge an error.
            if r.external {
                continue;
            }
            if !ids.contains(r.target.as_str()) {
                out.push(DanglingInstanceRef {
                    referrer: inst.id.clone(),
                    property: r.property.clone(),
                    target: r.target.clone(),
                });
            }
        }
    }
    out.sort_by(|a, b| {
        (&a.referrer, &a.property, &a.target).cmp(&(&b.referrer, &b.property, &b.target))
    });
    out
}

/// An external reference that resolves to no record of the sibling
/// datasets it is declared to resolve against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedSiblingReference {
    /// The referring record's id.
    pub referrer: String,
    /// The slot carrying the reference.
    pub property: String,
    /// The reference as authored (CURIE or absolute IRI).
    pub target: String,
    /// The IRI the reference denotes after prefix expansion — what a
    /// sibling record would have to mint for the reference to resolve.
    pub expanded: String,
}

impl UnresolvedSiblingReference {
    pub fn message(&self) -> String {
        format!(
            "instance `{}`: property `{}` references `{}`, which no resolve-against \
             dataset mints (expected a record whose IRI is {})",
            self.referrer, self.property, self.target, self.expanded
        )
    }
}

/// The IRIs `sets` mint under `schema`'s rules — the sibling side of a
/// `resolve_against` check. Uses the same minting the RDF emission uses,
/// so a `key`-scoped record's IRI (minted beneath its dataset root) is
/// what a reference must match, never a fabricated `namespace + bare id`.
pub fn minted_instance_iris(
    schema: &crate::linkml::SchemaDefinition,
    sets: &[crate::instances::InstanceSet],
) -> std::collections::BTreeSet<String> {
    sets.iter()
        .flat_map(|set| &set.instances)
        .map(|inst| crate::rdf_serializers::instance_iri_string(schema, inst))
        .collect()
}

/// A reference landing in no namespace any resolve-against sibling owns
/// — outside the check's jurisdiction by design, surfaced for the opt-in
/// coverage gate so a typo'd namespace cannot pass as an outside
/// vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UncoveredSiblingReference {
    /// The referring record's id.
    pub referrer: String,
    /// The slot carrying the reference.
    pub property: String,
    /// The reference as authored (CURIE or absolute IRI).
    pub target: String,
}

impl UncoveredSiblingReference {
    pub fn message(&self) -> String {
        format!(
            "instance `{}`: `{}` references `{}`, which lands in no namespace covered by \
             resolve_against",
            self.referrer, self.property, self.target
        )
    }
}

/// What a `resolve_against` pass found in one dataset: how many external
/// references target a sibling-owned namespace at all, which of those no
/// sibling record mints, and which references fall outside every owned
/// namespace — one classification pass, so the jurisdiction rule cannot
/// drift between the resolution check and the coverage gate.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SiblingResolution {
    /// External references whose expanded IRI falls in a sibling-owned
    /// namespace — the ones the check claims jurisdiction over.
    pub checked: usize,
    /// The checked references no sibling-minted IRI matches.
    pub unresolved: Vec<UnresolvedSiblingReference>,
    /// References landing in no owned namespace. Unchecked by design;
    /// the coverage gate reports them when opted in.
    pub uncovered: Vec<UncoveredSiblingReference>,
}

/// Resolve `set`'s external references against sibling datasets.
///
/// Targets expand against the *referring* schema through the same
/// derivation the RDF emission uses ([`crate::rdf_serializers::resolve_reference_iri`]),
/// so the check and the emitted graphs agree on what each reference
/// denotes. Only references landing in a sibling-owned namespace are
/// required to resolve: a dataset can also reference vocabularies outside
/// the manifest by design, and those stay in the cross-graph summary's
/// "not checked here" rather than becoming false failures. In-dataset
/// references are not touched here; the dangling check owns those.
pub fn resolve_sibling_references(
    schema: &crate::linkml::SchemaDefinition,
    set: &crate::instances::InstanceSet,
    owned_namespaces: &[String],
    sibling_iris: &std::collections::BTreeSet<String>,
) -> SiblingResolution {
    let mut resolution = SiblingResolution::default();
    for r in &set.external_references {
        let expanded = crate::rdf_serializers::resolve_reference_iri(schema, &r.target);
        if !owned_namespaces
            .iter()
            .any(|ns| expanded.starts_with(ns.as_str()))
        {
            resolution.uncovered.push(UncoveredSiblingReference {
                referrer: r.referrer.clone(),
                property: r.property.clone(),
                target: r.target.clone(),
            });
            continue;
        }
        resolution.checked += 1;
        if !sibling_iris.contains(&expanded) {
            resolution.unresolved.push(UnresolvedSiblingReference {
                referrer: r.referrer.clone(),
                property: r.property.clone(),
                target: r.target.clone(),
                expanded,
            });
        }
    }
    resolution
}

/// A stated absence claim a sibling record contradicts: the referring
/// record claims no single record joins its listed anchors, but one does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnverifiedAbsence {
    /// The claiming record's id.
    pub referrer: String,
    /// The anchors as authored on the claiming record.
    pub anchors: Vec<String>,
    /// The authored `via` narrowing, when the claim carried one.
    pub via: Option<String>,
    /// The sibling record that references every listed anchor.
    pub joined_by: String,
}

impl UnverifiedAbsence {
    pub fn message(&self) -> String {
        let joiner = match &self.via {
            Some(via) => format!("`{via}` record"),
            None => "record".to_string(),
        };
        let claim = match self.anchors.as_slice() {
            [one] => format!(
                "no {joiner} references `{one}`, but sibling record `{}` references it",
                self.joined_by
            ),
            _ => format!(
                "no {joiner} joins `{}`, but sibling record `{}` references all of them",
                self.anchors.join("`, `"),
                self.joined_by
            ),
        };
        format!(
            "instance `{}`: claims {claim} — the stated absence does not hold",
            self.referrer
        )
    }
}

/// A stated absence claim the check could not evaluate, and why — an
/// uncheckable claim is never counted as holding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UncheckableAbsence {
    /// The claiming record's id.
    pub referrer: String,
    /// Why the claim could not be evaluated.
    pub reason: String,
}

impl UncheckableAbsence {
    pub fn message(&self) -> String {
        format!(
            "instance `{}`: stated absence cannot be verified — {}",
            self.referrer, self.reason
        )
    }
}

/// What an absence-verification pass found over one referring dataset.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AbsenceVerification {
    /// Claims the check could evaluate: distinct anchors all minted by
    /// some sibling dataset, and a `via` (when stated) naming a class
    /// some sibling declares.
    pub claims: usize,
    /// Evaluated claims some sibling record contradicts.
    pub contradicted_claims: usize,
    /// Claims that could not be evaluated, each with its reason.
    pub uncheckable: Vec<UncheckableAbsence>,
    /// Every contradiction found, one per joining record.
    pub unverified: Vec<UnverifiedAbsence>,
}

/// How an `asserts_absence` declaration narrows its claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbsenceVia {
    /// No narrowing: no record of any kind may join the anchors.
    Unnarrowed,
    /// Narrowed to joining records of the class the named slot's value
    /// designates on each claiming record.
    Slot(String),
    /// The declaration's `via_slot` was not a string, so the narrowing
    /// is unreadable: the claims it governs are uncheckable — never
    /// evaluated wide of what the author wrote.
    Malformed,
}

/// One slot's declarations across scopes: the top-level `slots:`
/// declaration binds every class carrying the slot; a class's own
/// declaration (attribute, or `slot_usage` — which overrides the
/// attribute, as `slot_usage` does for every slot property) binds that
/// class's records and wins over the schema-wide one. Class-scoped
/// declarations do not yet propagate to subclasses: the resolved slot
/// view deliberately drops `slot_usage` annotations, so inheritance
/// here would drift from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scoped<T> {
    pub schema_wide: Option<T>,
    pub by_class: std::collections::BTreeMap<String, T>,
}

// Hand-written rather than derived: derive would demand `T: Default`,
// a bound the always-constructible empty scope does not need.
impl<T> Default for Scoped<T> {
    fn default() -> Self {
        Scoped {
            schema_wide: None,
            by_class: std::collections::BTreeMap::new(),
        }
    }
}

/// Schema-declared slot semantics, keyed by the declaring slot — the
/// shape both `asserts_absence` and `expand_against` bindings share.
/// Built by a single parse per family, so what is enforced and what is
/// reported can never drift apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedBindings<T> {
    pub by_slot: std::collections::BTreeMap<String, Scoped<T>>,
}

impl<T> Default for ScopedBindings<T> {
    fn default() -> Self {
        ScopedBindings {
            by_slot: std::collections::BTreeMap::new(),
        }
    }
}

impl<T> ScopedBindings<T> {
    pub fn is_empty(&self) -> bool {
        self.by_slot.is_empty()
    }

    /// How many slots declare — the enablement note's count.
    pub fn len(&self) -> usize {
        self.by_slot.len()
    }

    /// The declaration governing `slot` for a record of `types`: the
    /// record's own class declaration wins over the schema-wide one;
    /// `None` when no declaration applies to this record at all.
    pub fn governing(&self, types: &[String], slot: &str) -> Option<&T> {
        let declarations = self.by_slot.get(slot)?;
        types
            .iter()
            .find_map(|t| declarations.by_class.get(t))
            .or(declarations.schema_wide.as_ref())
    }

    fn insert_schema_wide(&mut self, slot: &str, value: T) {
        self.by_slot
            .entry(slot.to_string())
            .or_default()
            .schema_wide = Some(value);
    }

    fn insert_class(&mut self, class: &str, slot: &str, value: T) {
        self.by_slot
            .entry(slot.to_string())
            .or_default()
            .by_class
            .insert(class.to_string(), value);
    }
}

/// Every `asserts_absence` declaration in the schema.
pub type AbsenceBindings = ScopedBindings<AbsenceVia>;

/// One slot's absence declarations across scopes — the name this type
/// carried before the scoping generalized; kept so a consumer naming
/// it keeps compiling.
pub type SlotAbsenceDeclarations = Scoped<AbsenceVia>;

impl AbsenceBindings {
    /// A single schema-wide binding — the shape a caller that already
    /// knows its one slot (and every direct test) exercises.
    pub fn schema_wide(slot: &str, via: Option<&str>) -> Self {
        let mut bindings = AbsenceBindings::default();
        bindings.insert_schema_wide(
            slot,
            match via {
                Some(v) => AbsenceVia::Slot(v.to_string()),
                None => AbsenceVia::Unnarrowed,
            },
        );
        bindings
    }
}

/// A defect in an `asserts_absence` declaration. The declaration still
/// binds — the defect is reported (and `--strict` refuses it), never
/// silently repaired into a narrower or wider check than the author
/// wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbsenceDeclarationIssue {
    pub slot: String,
    pub detail: String,
}

impl AbsenceDeclarationIssue {
    pub fn message(&self) -> String {
        format!("`asserts_absence` on slot `{}`: {}", self.slot, self.detail)
    }
}

/// Every slot name any class's resolved view carries — the test a
/// declared slot and its `via` target must pass to ever match a record.
fn carried_slot_names(schema: &SchemaDefinition) -> std::collections::BTreeSet<String> {
    let mut carried = std::collections::BTreeSet::new();
    for class in schema.classes.values() {
        carried.extend(
            crate::linkml_resolve::resolve_effective_slots(class, schema)
                .keys()
                .cloned(),
        );
    }
    carried
}

/// The one parse of an `asserts_absence` annotation value: null is a
/// bare assertion, a mapping takes `via_slot` (a string) and nothing
/// else, and anything else — including a non-string `via_slot` — reads
/// as [`AbsenceVia::Malformed`] beside an issue naming the defect.
fn parse_absence_declaration(
    slot: &str,
    value: &serde_norway::Value,
    issues: &mut Vec<AbsenceDeclarationIssue>,
) -> AbsenceVia {
    match value {
        serde_norway::Value::Null => AbsenceVia::Unnarrowed,
        serde_norway::Value::Mapping(m) => {
            let mut via = AbsenceVia::Unnarrowed;
            for (key, field) in m {
                match key.as_str() {
                    Some("via_slot") => match field.as_str() {
                        Some(v) => via = AbsenceVia::Slot(v.to_string()),
                        None => {
                            issues.push(AbsenceDeclarationIssue {
                                slot: slot.to_string(),
                                detail: "`via_slot` must be a string naming a slot; the \
                                         claims are uncheckable until it is"
                                    .to_string(),
                            });
                            via = AbsenceVia::Malformed;
                        }
                    },
                    _ => issues.push(AbsenceDeclarationIssue {
                        slot: slot.to_string(),
                        detail: format!(
                            "unrecognized field `{}` — the declaration takes only `via_slot`",
                            key.as_str().unwrap_or("<non-string key>")
                        ),
                    }),
                }
            }
            via
        }
        _ => {
            issues.push(AbsenceDeclarationIssue {
                slot: slot.to_string(),
                detail: "the value must be a mapping under `value:` (optional `via_slot`), \
                         or null for a bare assertion; the claims are uncheckable until it is"
                    .to_string(),
            });
            AbsenceVia::Malformed
        }
    }
}

/// Every `asserts_absence` declaration and every defect in one, from a
/// single walk: top-level `slots:` bind schema-wide; a class's
/// `attributes:` bind that class; its `slot_usage:` overrides both for
/// that class. Consumed whole by the check and split by the two
/// wrappers below.
pub fn absence_declarations(
    schema: &SchemaDefinition,
) -> (AbsenceBindings, Vec<AbsenceDeclarationIssue>) {
    let mut issues = Vec::new();
    let mut declared_vias: Vec<(String, AbsenceVia)> = Vec::new();
    let bindings = scoped_declarations(schema, "asserts_absence", |name, value| {
        let via = parse_absence_declaration(name, value, &mut issues);
        declared_vias.push((name.to_string(), via.clone()));
        via
    });
    if bindings.is_empty() {
        return (bindings, issues);
    }

    let carried = carried_slot_names(schema);
    for slot in bindings.by_slot.keys() {
        if !carried.contains(slot) {
            issues.push(AbsenceDeclarationIssue {
                slot: slot.clone(),
                detail: "no class carries this slot, so no record can state its claim".to_string(),
            });
        }
    }
    for (slot, via) in &declared_vias {
        if let AbsenceVia::Slot(target) = via
            && !carried.contains(target)
        {
            issues.push(AbsenceDeclarationIssue {
                slot: slot.clone(),
                detail: format!(
                    "`via_slot` names `{target}`, which no class carries; the narrowed \
                     claims will be reported uncheckable"
                ),
            });
        }
    }
    (bindings, issues)
}

/// The declarations alone — the checker's half of
/// [`absence_declarations`].
pub fn absence_bindings(schema: &SchemaDefinition) -> AbsenceBindings {
    absence_declarations(schema).0
}

/// The defects alone — the load diagnostics' half of
/// [`absence_declarations`].
pub fn absence_declaration_issues(schema: &SchemaDefinition) -> Vec<AbsenceDeclarationIssue> {
    absence_declarations(schema).1
}

/// A `records_version_of` declaration: the annotated slot holds the
/// package version a record was written against, and `sibling_slot`
/// names the slot on the same record whose value says which sibling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionPin {
    Of {
        sibling_slot: String,
    },
    /// A defective declaration still binds: its pins are reported
    /// uncheckable, never as agreeing.
    Malformed,
}

pub type VersionBindings = ScopedBindings<VersionPin>;

/// A defect in a `records_version_of` declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionDeclarationIssue {
    pub slot: String,
    pub detail: String,
}

impl VersionDeclarationIssue {
    pub fn message(&self) -> String {
        format!(
            "`records_version_of` on slot `{}`: {}; its pins are uncheckable until it is fixed",
            self.slot, self.detail
        )
    }
}

/// The one parse of a `records_version_of` annotation value: a mapping
/// with a string `sibling_slot` (not the slot itself) and nothing else.
fn parse_version_declaration(
    slot: &str,
    value: &serde_norway::Value,
    issues: &mut Vec<VersionDeclarationIssue>,
) -> VersionPin {
    let mut issue = |detail: String| {
        issues.push(VersionDeclarationIssue {
            slot: slot.to_string(),
            detail,
        })
    };
    let serde_norway::Value::Mapping(m) = value else {
        issue(
            "the value must be a mapping under `value:` with `sibling_slot` naming the slot \
             that says which sibling"
                .to_string(),
        );
        return VersionPin::Malformed;
    };
    for key in m.keys().filter(|k| k.as_str() != Some("sibling_slot")) {
        issue(format!(
            "unrecognized field `{}` — the declaration takes only `sibling_slot`",
            key.as_str().unwrap_or("<non-string key>")
        ));
    }
    match m.get("sibling_slot").map(|v| v.as_str()) {
        None => {
            issue("the declaration needs `sibling_slot`".to_string());
            VersionPin::Malformed
        }
        Some(None) => {
            issue("`sibling_slot` must be a string naming a slot on the same record".to_string());
            VersionPin::Malformed
        }
        Some(Some(named)) if named == slot => {
            issue(
                "`sibling_slot` names the slot itself — a version cannot name its own sibling"
                    .to_string(),
            );
            VersionPin::Malformed
        }
        Some(Some(named)) => VersionPin::Of {
            sibling_slot: named.to_string(),
        },
    }
}

/// The one walk every schema-declared slot semantic shares: top-level
/// `slots:` bind schema-wide; a class's `attributes:` bind that class;
/// its `slot_usage:` overrides both for that class (attributes first,
/// `slot_usage` second, so a plain map insert makes `slot_usage` win —
/// the direction it overrides everywhere else in LinkML). `parse` sees
/// every site carrying `annotation` and decides what it binds.
fn scoped_declarations<T: Clone>(
    schema: &SchemaDefinition,
    annotation: &str,
    mut parse: impl FnMut(&str, &serde_norway::Value) -> T,
) -> ScopedBindings<T> {
    let mut bindings = ScopedBindings::default();
    for (name, slot) in &schema.slots {
        if let Some(value) = slot.annotations.get(annotation) {
            bindings.insert_schema_wide(name, parse(name, value));
        }
    }
    for (class_name, class) in &schema.classes {
        for (name, slot) in class.attributes.iter().chain(class.slot_usage.iter()) {
            if let Some(value) = slot.annotations.get(annotation) {
                bindings.insert_class(class_name, name, parse(name, value));
            }
        }
    }
    bindings
}

/// Every `records_version_of` declaration and every defect in one, from
/// the shared walk. Defects are reported once per slot, however many
/// scopes restate them.
pub fn version_declarations(
    schema: &SchemaDefinition,
) -> (VersionBindings, Vec<VersionDeclarationIssue>) {
    let mut issues = Vec::new();
    let bindings = scoped_declarations(schema, "records_version_of", |name, value| {
        parse_version_declaration(name, value, &mut issues)
    });
    if bindings.is_empty() {
        return (bindings, issues);
    }
    let carried = carried_slot_names(schema);
    let mut post: std::collections::BTreeSet<(String, String)> = std::collections::BTreeSet::new();
    for (slot, scoped) in &bindings.by_slot {
        if !carried.contains(slot) {
            post.insert((
                slot.clone(),
                "no class carries this slot, so no record can state a pin".to_string(),
            ));
        }
        for pin in scoped.schema_wide.iter().chain(scoped.by_class.values()) {
            if let VersionPin::Of { sibling_slot } = pin
                && !carried.contains(sibling_slot)
            {
                post.insert((
                    slot.clone(),
                    format!("`sibling_slot` names `{sibling_slot}`, which no class carries"),
                ));
            }
        }
    }
    issues.extend(
        post.into_iter()
            .map(|(slot, detail)| VersionDeclarationIssue { slot, detail }),
    );
    (bindings, issues)
}

/// One schema-declared slot-semantics family: what it is called, the
/// annotation that declares it, its load-time defects as messages, and
/// how many slots declare it. Load diagnostics, the strict gate, and the
/// check's enablement notes all read this table, so adding a family is
/// one row.
pub struct SlotSemanticsFamily {
    pub noun: &'static str,
    pub annotation: &'static str,
    pub issues: fn(&SchemaDefinition) -> Vec<String>,
    /// Slots declaring the family — `None` for a family the manifest's
    /// `resolve_against` does not gate.
    pub declared: Option<fn(&SchemaDefinition) -> usize>,
}

pub const SLOT_SEMANTICS_FAMILIES: &[SlotSemanticsFamily] = &[
    SlotSemanticsFamily {
        noun: "absence-claim",
        annotation: "asserts_absence",
        issues: |schema| {
            absence_declaration_issues(schema)
                .iter()
                .map(|i| i.message())
                .collect()
        },
        declared: Some(|schema| absence_bindings(schema).len()),
    },
    SlotSemanticsFamily {
        noun: "anchor-expansion",
        annotation: "expand_against",
        issues: |schema| {
            expansion_declaration_issues(schema)
                .iter()
                .map(|i| i.message())
                .collect()
        },
        declared: None,
    },
    SlotSemanticsFamily {
        noun: "version-pin",
        annotation: "records_version_of",
        issues: |schema| {
            version_declaration_issues(schema)
                .iter()
                .map(|i| i.message())
                .collect()
        },
        declared: Some(|schema| version_bindings(schema).len()),
    },
];

/// The declarations alone — the checker's half of [`version_declarations`].
pub fn version_bindings(schema: &SchemaDefinition) -> VersionBindings {
    version_declarations(schema).0
}

/// The defects alone — the load diagnostics' half of [`version_declarations`].
pub fn version_declaration_issues(schema: &SchemaDefinition) -> Vec<VersionDeclarationIssue> {
    version_declarations(schema).1
}

/// What a `resolve_against` sibling resolved to, as a pin can name it:
/// by its `[schemas]` entry key, the name its publish manifest declares,
/// or a dataset name that manifest lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiblingVersion {
    pub entry: String,
    pub published: String,
    /// The sibling schema's `id` IRI, when it declares one.
    pub schema_id: Option<String>,
    pub datasets: Vec<String>,
    pub version: String,
}

impl SiblingVersion {
    /// Whether `value` is one of this sibling's names other than its
    /// entry key: the published name, the schema IRI, or a dataset.
    fn has_alias(&self, value: &str) -> bool {
        value == self.published
            || self.schema_id.as_deref() == Some(value)
            || self.datasets.iter().any(|d| d == value)
    }

    /// The one sibling `value` names: an exact entry key wins outright;
    /// otherwise the published name, the schema IRI, or a listed
    /// dataset, provided exactly one sibling answers to it. None, or several, is an error
    /// naming the reason.
    pub fn resolve<'a>(
        siblings: &'a [SiblingVersion],
        value: &str,
    ) -> Result<&'a SiblingVersion, String> {
        if let Some(exact) = siblings.iter().find(|s| s.entry == value) {
            return Ok(exact);
        }
        let by_alias: Vec<&SiblingVersion> =
            siblings.iter().filter(|s| s.has_alias(value)).collect();
        match by_alias.as_slice() {
            [one] => Ok(one),
            [] => Err(format!(
                "`{value}` names no resolve_against sibling (by entry, published name, schema IRI, or dataset)"
            )),
            many => Err(format!(
                "`{value}` names {} resolve_against siblings ({}); name one by its entry key",
                many.len(),
                many.iter()
                    .map(|s| format!("`{}`", s.entry))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
}

/// Whether two version strings name the same release: both parse as
/// semver (a leading `v` allowed), and compare equal by precedence, so
/// build metadata does not separate them but a pre-release does. An
/// unparsable side is an error naming it.
fn same_release(declared: &str, actual: &str) -> Result<bool, String> {
    let parse = |v: &str| semver::Version::parse(v.trim_start_matches('v'));
    let d = parse(declared).map_err(|_| format!("`{declared}` is not a semver version"))?;
    let a = parse(actual)
        .map_err(|_| format!("the sibling's own version `{actual}` is not a semver version"))?;
    Ok(d.cmp_precedence(&a) == std::cmp::Ordering::Equal)
}

/// A record's declared version disagrees with the sibling it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionMismatch {
    pub record: String,
    pub slot: String,
    pub declared: String,
    pub sibling: String,
    pub actual: String,
}

impl VersionMismatch {
    pub fn message(&self) -> String {
        format!(
            "record `{}`: `{}` records version {} of `{}`, which is at {}",
            self.record, self.slot, self.declared, self.sibling, self.actual
        )
    }
}

/// A pin that could not be evaluated, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UncheckableVersion {
    pub record: String,
    pub slot: String,
    pub detail: String,
}

impl UncheckableVersion {
    pub fn message(&self) -> String {
        format!(
            "record `{}`: the `{}` version pin could not be checked: {}",
            self.record, self.slot, self.detail
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionVerification {
    /// Pins that could be evaluated; `mismatched` counts among them.
    pub pins: usize,
    pub mismatched: Vec<VersionMismatch>,
    pub uncheckable: Vec<UncheckableVersion>,
}

/// Every version pin a dataset's records state, evaluated against the
/// siblings' resolved versions. A record carrying no value at the pinned
/// slot states no pin. A pin that cannot be evaluated — no single string
/// version, no single string at `sibling_slot`, a name matching no
/// sibling or several, an unparsable version, a defective declaration —
/// is reported uncheckable, never as agreeing.
pub fn version_pins(
    set: &crate::instances::InstanceSet,
    declarations: &VersionBindings,
    siblings: &[SiblingVersion],
) -> VersionVerification {
    use crate::instances::{InstanceValue, ScalarValue};

    fn single_string<'a>(
        inst: &'a crate::instances::Instance,
        slot: &str,
    ) -> Result<Option<&'a str>, String> {
        let Some(sv) = inst.slot_values.iter().find(|sv| sv.slot == slot) else {
            return Ok(None);
        };
        match sv.values.as_slice() {
            [] => Ok(None),
            [InstanceValue::Scalar(ScalarValue::String(v))] => Ok(Some(v.as_str())),
            [InstanceValue::Scalar(ScalarValue::Integer(_) | ScalarValue::Float(_))] => Err(
                format!("`{slot}` is a number, not a string — quote it in the data"),
            ),
            [_] => Err(format!("`{slot}` is not a string")),
            _ => Err(format!("`{slot}` has several values where one is expected")),
        }
    }

    fn evaluate(
        inst: &crate::instances::Instance,
        slot: &str,
        declared: &str,
        pin: &VersionPin,
        siblings: &[SiblingVersion],
    ) -> Result<Option<VersionMismatch>, String> {
        let VersionPin::Of { sibling_slot } = pin else {
            return Err(
                "the declaration is defective (see the schema's load warnings)".to_string(),
            );
        };
        let named = single_string(inst, sibling_slot)?.ok_or_else(|| {
            format!("the record has no `{sibling_slot}` value naming the sibling")
        })?;
        let sibling = SiblingVersion::resolve(siblings, named)?;
        Ok(
            (!same_release(declared, &sibling.version)?).then(|| VersionMismatch {
                record: inst.id.clone(),
                slot: slot.to_string(),
                declared: declared.to_string(),
                sibling: sibling.entry.clone(),
                actual: sibling.version.clone(),
            }),
        )
    }

    let mut out = VersionVerification::default();
    for inst in &set.instances {
        for slot_name in declarations.by_slot.keys() {
            let Some(pin) = declarations.governing(&inst.types, slot_name) else {
                continue;
            };
            let outcome = match single_string(inst, slot_name) {
                Ok(None) => continue,
                Ok(Some(declared)) => evaluate(inst, slot_name, declared, pin, siblings),
                Err(detail) => Err(detail),
            };
            match outcome {
                Err(detail) => out.uncheckable.push(UncheckableVersion {
                    record: inst.id.clone(),
                    slot: slot_name.clone(),
                    detail,
                }),
                Ok(mismatch) => {
                    out.pins += 1;
                    out.mismatched.extend(mismatch);
                }
            }
        }
    }
    out
}

/// Every `expand_against` declaration in the schema: the annotated
/// slot's scheme-less values expand against the named base slot's
/// value on the same record.
pub type ExpansionBindings = ScopedBindings<String>;

/// A defect in an `expand_against` declaration. Unlike an absence
/// defect, the safe reading here is *no expansion* — values stay as
/// authored — so a defective declaration does not bind, and the issue
/// says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpansionDeclarationIssue {
    pub slot: String,
    pub detail: String,
}

impl ExpansionDeclarationIssue {
    pub fn message(&self) -> String {
        format!(
            "`expand_against` on slot `{}`: {}; values are read as authored",
            self.slot, self.detail
        )
    }
}

/// Every `expand_against` declaration and every defect in one, from a
/// single walk with the same scoping as [`absence_declarations`]. A
/// defective scope never binds — no expansion is the safe reading — and
/// only the defective scope is dropped, so one class's bad declaration
/// cannot disable another's.
///
/// A class-ranged slot may declare expansion exactly when no local
/// record of its range class can exist for a bare value to collide
/// with: every site ranging the class is itself declared external.
/// While any site is not — a `tree_root` collection, or another slot
/// without the annotation — the declaration is refused naming that
/// site. Ranges are judged from the resolved per-class view (a
/// `slot_usage` restates only what it overrides, and `any_of` replaces
/// a scalar `range:` under LinkML's induced semantics, so a read of the
/// raw definition would get both wrong).
pub fn expansion_declarations(
    schema: &SchemaDefinition,
) -> (ExpansionBindings, Vec<ExpansionDeclarationIssue>) {
    let mut bindings = ExpansionBindings::default();
    let mut issues = Vec::new();

    let mut read = |site_class: Option<&str>, name: &str, slot: &crate::linkml::SlotDefinition| {
        let value = slot.annotations.get("expand_against")?;
        let Some(base) = value.as_str() else {
            issues.push(ExpansionDeclarationIssue {
                slot: name.to_string(),
                detail: "the value must be a string naming a slot on the same record".to_string(),
            });
            return None;
        };
        if base == name {
            issues.push(ExpansionDeclarationIssue {
                slot: name.to_string(),
                detail: "the declaration names the slot itself — a value cannot be its own base"
                    .to_string(),
            });
            return None;
        }
        Some((
            site_class.map(String::from),
            name.to_string(),
            base.to_string(),
        ))
    };

    let mut declared: Vec<(Option<String>, String, String)> = Vec::new();
    for (name, slot) in &schema.slots {
        declared.extend(read(None, name, slot));
    }
    for (class_name, class) in &schema.classes {
        for (name, slot) in class.attributes.iter().chain(class.slot_usage.iter()) {
            declared.extend(read(Some(class_name), name, slot));
        }
    }
    if declared.is_empty() {
        // The common schema declares nothing; skip the whole-schema
        // resolution pass entirely.
        return (bindings, issues);
    }

    // One resolution pass: which slots any class carries, and which
    // classes each (class, slot) site ranges — read from the same
    // effective view the loader applies to values.
    let mut carried_pairs: std::collections::BTreeSet<(String, String)> =
        std::collections::BTreeSet::new();
    let mut site_class_ranges: std::collections::BTreeMap<(String, String), Vec<String>> =
        std::collections::BTreeMap::new();
    let mut carried_names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (class_name, class) in &schema.classes {
        for (slot_name, rs) in
            crate::linkml_resolve::resolve_effective_slots_with_provenance(class, schema)
        {
            carried_names.insert(slot_name.clone());
            let class_ranges: Vec<String> = rs
                .effective_ranges()
                .into_iter()
                .filter(|r| schema.classes.contains_key(*r))
                .cloned()
                .collect();
            if !class_ranges.is_empty() {
                site_class_ranges.insert((class_name.clone(), slot_name.clone()), class_ranges);
            }
            carried_pairs.insert((class_name.clone(), slot_name));
        }
    }
    let carried: std::collections::BTreeSet<&str> =
        carried_names.iter().map(String::as_str).collect();

    // The `is_a`/mixin family of a class: a record of a descendant is an
    // instance of the class, and a site ranging an ancestor can
    // materialize one through a type designator — either way a local
    // record of the class can exist at such a site.
    let mut ancestors: std::collections::BTreeMap<&String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    for name in schema.classes.keys() {
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut stack: Vec<&String> = vec![name];
        while let Some(current) = stack.pop() {
            if let Some(class) = schema.classes.get(current) {
                for parent in class.is_a.iter().chain(class.mixins.iter()) {
                    if seen.insert(parent.clone()) {
                        stack.push(parent);
                    }
                }
            }
        }
        ancestors.insert(name, seen);
    }
    let family_of = |r: &str| -> std::collections::BTreeSet<String> {
        let mut family: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        family.insert(r.to_string());
        if let Some(up) = ancestors.get(&r.to_string()) {
            family.extend(up.iter().cloned());
        }
        for (name, up) in &ancestors {
            if up.contains(r) {
                family.insert((*name).clone());
            }
        }
        family
    };

    // Which classes' records can contain a record of each class: every
    // class-ranged site makes the ranged family's members containable
    // by the site's class. The loader walks the authored containment
    // chain, so the static question for a base is whether some class on
    // a containment path to the governed class carries it as a scalar.
    let mut containers_of: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    for ((site_class, _), ranges) in &site_class_ranges {
        for r in ranges {
            for member in family_of(r) {
                containers_of
                    .entry(member)
                    .or_default()
                    .insert(site_class.clone());
            }
        }
    }
    let reach_of = |class: &str| -> std::collections::BTreeSet<String> {
        let mut reach: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut stack = vec![class.to_string()];
        while let Some(current) = stack.pop() {
            if reach.insert(current.clone())
                && let Some(ups) = containers_of.get(&current)
            {
                stack.extend(ups.iter().cloned());
            }
        }
        reach
    };

    // Stage one: defects a declaration owns outright — an uncarried slot
    // or base (reported in that order), or a range class whose family
    // holds a `tree_root` (its records are document roots, no ranging
    // site needed) — refuse before any site vouches for anything.
    struct Candidate {
        scope: Option<String>,
        slot: String,
        base: String,
        target_family: std::collections::BTreeSet<String>,
    }
    let mut candidates: Vec<Candidate> = Vec::new();
    for (scope, slot, base) in &declared {
        let slot_ok = match scope {
            Some(class) => carried_pairs.contains(&(class.clone(), slot.clone())),
            None => carried.contains(slot.as_str()),
        };
        if !slot_ok {
            issues.push(ExpansionDeclarationIssue {
                slot: slot.clone(),
                detail: match scope {
                    Some(class) => format!(
                        "class `{class}` does not carry this slot, so the declaration can \
                         never govern a record"
                    ),
                    None => "no class carries this slot, so the declaration can never govern \
                             a record"
                        .to_string(),
                },
            });
            continue;
        }
        if declared.iter().any(|(_, s, _)| s == base) {
            issues.push(ExpansionDeclarationIssue {
                slot: slot.clone(),
                detail: format!(
                    "`{base}` itself declares `expand_against` — a base is read as \
                     authored, so expansions cannot chain through it"
                ),
            });
            continue;
        }
        // The base must be resolvable at load: carried as a scalar by
        // the governed class or by some class whose records can contain
        // one, since those are exactly the mappings the containment walk
        // consults. A class-ranged carrier holds references, not a
        // namespace string, so it cannot satisfy this.
        let governed_classes: Vec<&String> = match scope {
            Some(class) => vec![class],
            None => carried_pairs
                .iter()
                .filter(|(_, s)| s == slot)
                .map(|(c, _)| c)
                .collect(),
        };
        let mut reachable_carrier = false;
        let mut scalar_reachable = false;
        for class in &governed_classes {
            for within in reach_of(class) {
                if carried_pairs.contains(&(within.clone(), base.clone())) {
                    reachable_carrier = true;
                    if !site_class_ranges.contains_key(&(within.clone(), base.clone())) {
                        scalar_reachable = true;
                    }
                }
            }
        }
        if !scalar_reachable {
            issues.push(ExpansionDeclarationIssue {
                slot: slot.clone(),
                detail: if reachable_carrier {
                    format!(
                        "`{base}` resolves class-ranged everywhere a record could supply \
                         it — its values are references, not a namespace string"
                    )
                } else {
                    format!(
                        "`{base}` is not a slot of the governed class nor of any class \
                         whose records can contain one, so no containment walk can \
                         supply it"
                    )
                },
            });
            continue;
        }
        let range_classes: Vec<&String> = match scope {
            Some(class) => site_class_ranges
                .get(&(class.clone(), slot.clone()))
                .into_iter()
                .flatten()
                .collect(),
            None => site_class_ranges
                .iter()
                .filter(|((_, s), _)| s == slot)
                .flat_map(|(_, ranges)| ranges)
                .collect(),
        };
        let mut target_family: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        for r in &range_classes {
            target_family.extend(family_of(r));
        }
        if let Some(root) = target_family
            .iter()
            .find(|c| schema.classes.get(*c).is_some_and(|class| class.tree_root))
        {
            issues.push(ExpansionDeclarationIssue {
                slot: slot.clone(),
                detail: format!(
                    "the slot's range class family includes `{root}`, a `tree_root` — its \
                     records are document roots, so a bare value stays ambiguous with a \
                     local reference"
                ),
            });
            continue;
        }
        candidates.push(Candidate {
            scope: scope.clone(),
            slot: slot.clone(),
            base: base.clone(),
            target_family,
        });
    }

    // Stage two: only a declaration that itself binds vouches for its
    // site, so refusals iterate to a fixpoint — a candidate blocked
    // through one range class stops vouching for the rest, and the
    // candidates that leaned on it are refused in turn.
    loop {
        let refused = candidates
            .iter()
            .enumerate()
            .find_map(|(index, candidate)| {
                let blocker = site_class_ranges.iter().find(|((c2, s2), ranges)| {
                    ranges.iter().any(|r| candidate.target_family.contains(r))
                        && !candidates.iter().any(|other| {
                            other.slot == *s2
                                && match &other.scope {
                                    None => true,
                                    Some(scope_class) => scope_class == c2,
                                }
                        })
                });
                blocker.map(|((c2, s2), _)| (index, c2.clone(), s2.clone()))
            });
        let Some((index, block_class, block_slot)) = refused else {
            break;
        };
        let candidate = candidates.remove(index);
        issues.push(ExpansionDeclarationIssue {
            slot: candidate.slot,
            detail: format!(
                "the slot's range class can still be locally declared through \
                 `{block_slot}` (class `{block_class}`), whose values are not declared \
                 external — annotate every slot ranging the class (and its `is_a` family) \
                 with `expand_against`, or bare values stay ambiguous with in-dataset \
                 references"
            ),
        });
    }

    for candidate in candidates {
        match &candidate.scope {
            Some(class) => bindings.insert_class(class, &candidate.slot, candidate.base),
            None => bindings.insert_schema_wide(&candidate.slot, candidate.base),
        }
    }
    (bindings, issues)
}

/// Every schema finding `--strict` refuses, counted in one place — the
/// list both the generate gate and the validate gate consume, so the
/// two verbs can never drift on what blocks a strict run.
pub struct StrictBlocking {
    pub unmodeled: usize,
    pub dangling: usize,
    pub colliding: usize,
    pub untyped: usize,
    pub impossible: usize,
    /// Defective schema-declared slot-semantics declarations, every family.
    pub slot_semantics: usize,
}

impl StrictBlocking {
    pub fn total(&self) -> usize {
        self.unmodeled
            + self.dangling
            + self.colliding
            + self.untyped
            + self.impossible
            + self.slot_semantics
    }
}

/// Count every strict-blocking schema finding.
pub fn strict_blocking(schema: &SchemaDefinition) -> StrictBlocking {
    StrictBlocking {
        unmodeled: unmodeled_class_constructs(schema).len(),
        dangling: dangling_references(schema).len(),
        colliding: colliding_slot_definitions(schema).len(),
        untyped: untyped_slots(schema).len(),
        impossible: impossible_rule_values(schema).len(),
        slot_semantics: SLOT_SEMANTICS_FAMILIES
            .iter()
            .map(|family| (family.issues)(schema).len())
            .sum(),
    }
}

/// The declarations alone — the loader's half of
/// [`expansion_declarations`].
pub fn expansion_bindings(schema: &SchemaDefinition) -> ExpansionBindings {
    expansion_declarations(schema).0
}

/// The defects alone — the load diagnostics' half of
/// [`expansion_declarations`].
pub fn expansion_declaration_issues(schema: &SchemaDefinition) -> Vec<ExpansionDeclarationIssue> {
    expansion_declarations(schema).1
}

/// Verify each record's stated absence claim against sibling datasets.
///
/// A record listing anchors under a declared slot (reference targets or
/// IRI-valued scalars alike) claims no single sibling record references
/// them all — for one anchor, that no record references it at all. A
/// "reference" here is an authored citation edge; a sibling citing an
/// anchor's IRI in a plain scalar is not counted, and neither is
/// containment — the container's collection slots and any edge that
/// materialized its target (an inlined child) hold records rather than
/// cite them, while restating an already-declared record inline is a
/// citation. The `via` slot's value — expanded against the referring
/// schema exactly like the anchors — narrows the claim to joining
/// records of that class. Anchors and joins resolve through the
/// sibling's own minting (its whole dataset list at once, so a join in
/// one file reaches records declared in another). A claim that cannot
/// be evaluated — a null or malformed anchor, anchors collapsing to
/// fewer distinct IRIs than authored, an anchor no sibling mints,
/// several `via` values, a malformed `via`, a `via` naming no sibling
/// class — is reported as uncheckable, never as holding: evaluating
/// what remains would silently strengthen the claim.
pub fn unverified_absences(
    schema: &crate::linkml::SchemaDefinition,
    set: &crate::instances::InstanceSet,
    declarations: &AbsenceBindings,
    siblings: &[(
        &crate::linkml::SchemaDefinition,
        &[crate::instances::InstanceSet],
    )],
) -> AbsenceVerification {
    use crate::instances::InstanceValue;
    use std::collections::BTreeSet;

    struct SiblingRecord<'a> {
        id: &'a str,
        types: &'a [String],
        referenced: BTreeSet<String>,
    }
    struct SiblingIndex<'a> {
        records: Vec<SiblingRecord<'a>>,
        schema: &'a SchemaDefinition,
        class_names: Vec<&'a String>,
    }

    let mut minted_union: BTreeSet<String> = BTreeSet::new();
    let indexes: Vec<SiblingIndex> = siblings
        .iter()
        .map(|(sibling_schema, sibling_sets)| {
            let by_id = crate::rdf_serializers::instance_iris_by_id(sibling_schema, sibling_sets);
            minted_union.extend(by_id.values().cloned());
            let mut records = Vec::new();
            for sibling_set in *sibling_sets {
                for record in &sibling_set.instances {
                    let referenced = record
                        .slot_values
                        .iter()
                        .flat_map(|sv| &sv.values)
                        .filter_map(|v| match v {
                            InstanceValue::Reference {
                                target,
                                held: false,
                            } => Some(target),
                            _ => None,
                        })
                        .map(|t| {
                            by_id.get(t.as_str()).cloned().unwrap_or_else(|| {
                                crate::rdf_serializers::resolve_reference_iri(sibling_schema, t)
                            })
                        })
                        .collect();
                    records.push(SiblingRecord {
                        id: &record.id,
                        types: &record.types,
                        referenced,
                    });
                }
            }
            SiblingIndex {
                records,
                schema: sibling_schema,
                class_names: sibling_schema.classes.keys().collect(),
            }
        })
        .collect();

    let mut out = AbsenceVerification::default();
    // The record's governing declaration decides the narrowing; a
    // record whose classes no declaration touches states no claim. The
    // carried set is schema-wide and loop-invariant.
    let carried = carried_slot_names(schema);
    for inst in &set.instances {
        for slot_name in declarations.by_slot.keys() {
            let Some(governing) = declarations.governing(&inst.types, slot_name) else {
                continue;
            };
            let anchors: Result<Vec<String>, &str> = inst
                .slot_values
                .iter()
                .filter(|sv| sv.slot == *slot_name)
                .flat_map(|sv| &sv.values)
                .map(|v| match v {
                    InstanceValue::Reference { target, .. } => Ok(target.clone()),
                    InstanceValue::Scalar(s) => Ok(crate::instances::scalar_to_display(s)),
                    InstanceValue::Unexpected(kind) => Err(*kind),
                })
                .collect();
            let anchors = match anchors {
                Ok(anchors) => anchors,
                Err(kind) => {
                    out.uncheckable.push(UncheckableAbsence {
                        referrer: inst.id.clone(),
                        reason: format!("an anchor value is {kind}, not a reference or IRI"),
                    });
                    continue;
                }
            };
            if anchors.is_empty() {
                continue;
            }
            // A narrowing that cannot be read (malformed) or can never hold
            // a record's value (uncarried slot) would silently evaluate the
            // claim wide — a strictly stronger check than declared.
            // Uncheckable instead.
            let via_slot: Option<&str> = match governing {
                AbsenceVia::Unnarrowed => None,
                AbsenceVia::Malformed => {
                    out.uncheckable.push(UncheckableAbsence {
                        referrer: inst.id.clone(),
                        reason: "the declaration's `via_slot` is malformed, so the narrowing \
                             cannot be evaluated"
                            .to_string(),
                    });
                    continue;
                }
                AbsenceVia::Slot(via) if !carried.contains(via) => {
                    out.uncheckable.push(UncheckableAbsence {
                        referrer: inst.id.clone(),
                        reason: format!(
                            "the declared `via_slot` `{via}` is not a slot of any class, so \
                         the narrowing cannot be evaluated"
                        ),
                    });
                    continue;
                }
                AbsenceVia::Slot(via) => Some(via.as_str()),
            };
            let anchor_iris: BTreeSet<String> = anchors
                .iter()
                .map(|t| crate::rdf_serializers::resolve_reference_iri(schema, t))
                .collect();
            if anchor_iris.len() < anchors.len() {
                out.uncheckable.push(UncheckableAbsence {
                    referrer: inst.id.clone(),
                    reason: "its anchors collapse to fewer distinct IRIs than authored — list \
                         each anchor once"
                        .to_string(),
                });
                continue;
            }
            if let Some(missing) = anchor_iris.iter().find(|iri| !minted_union.contains(*iri)) {
                out.uncheckable.push(UncheckableAbsence {
                    referrer: inst.id.clone(),
                    reason: format!("anchor {missing} resolves to no sibling record"),
                });
                continue;
            }
            let via_values: Vec<&InstanceValue> = via_slot
                .map(|slot| {
                    inst.slot_values
                        .iter()
                        .filter(|sv| sv.slot == slot)
                        .flat_map(|sv| &sv.values)
                        .collect()
                })
                .unwrap_or_default();
            if via_values.len() > 1 {
                out.uncheckable.push(UncheckableAbsence {
                    referrer: inst.id.clone(),
                    reason: "a claim narrows by exactly one `via` value, and this record \
                         carries several"
                        .to_string(),
                });
                continue;
            }
            let via_authored: Option<String> = match via_values.first() {
                Some(InstanceValue::Unexpected(kind)) => {
                    out.uncheckable.push(UncheckableAbsence {
                        referrer: inst.id.clone(),
                        reason: format!("its `via` value is {kind}, not a class reference"),
                    });
                    continue;
                }
                Some(InstanceValue::Scalar(s)) => Some(crate::instances::scalar_to_display(s)),
                Some(InstanceValue::Reference { target, .. }) => Some(target.clone()),
                None => None,
            };
            // The `via` value narrows by the same matching a type designator
            // uses — a sibling's class name first, its IRI or CURIE second,
            // the spelling expanded against the claiming schema exactly like
            // the anchors beside it — and like a designation it must name
            // one thing: an IRI shared by several sibling classes narrows to
            // nothing checkable. Resolved once per sibling and reused below.
            let via_matches: Vec<crate::rdf_serializers::ClassMatch> = match &via_authored {
                Some(via) => indexes
                    .iter()
                    .map(|index| {
                        crate::rdf_serializers::class_named_by_expanded(
                            schema,
                            index.schema,
                            &index.class_names,
                            via,
                        )
                    })
                    .collect(),
                None => Vec::new(),
            };
            if let Some(via) = &via_authored {
                let any_named = via_matches
                    .iter()
                    .any(|m| matches!(m, crate::rdf_serializers::ClassMatch::One(_)));
                let ambiguous = via_matches
                    .iter()
                    .any(|m| matches!(m, crate::rdf_serializers::ClassMatch::Several));
                if ambiguous {
                    out.uncheckable.push(UncheckableAbsence {
                        referrer: inst.id.clone(),
                        reason: format!(
                            "`{via}` names more than one class of a sibling — a narrowing must \
                         name one"
                        ),
                    });
                    continue;
                }
                if !any_named {
                    out.uncheckable.push(UncheckableAbsence {
                        referrer: inst.id.clone(),
                        reason: format!("`{via}` names no class any sibling declares"),
                    });
                    continue;
                }
            }

            out.claims += 1;
            let mut contradicted = false;
            for (position, index) in indexes.iter().enumerate() {
                let allowed: Option<&str> = match via_matches.get(position) {
                    Some(crate::rdf_serializers::ClassMatch::One(name)) => Some(name),
                    _ => None,
                };
                if via_authored.is_some() && allowed.is_none() {
                    continue;
                }
                for record in &index.records {
                    if let Some(allowed) = allowed
                        && !record.types.iter().any(|t| t == allowed)
                    {
                        continue;
                    }
                    if anchor_iris.is_subset(&record.referenced) {
                        out.unverified.push(UnverifiedAbsence {
                            referrer: inst.id.clone(),
                            anchors: anchors.clone(),
                            via: via_authored.clone(),
                            joined_by: record.id.to_string(),
                        });
                        contradicted = true;
                    }
                }
            }
            if contradicted {
                out.contradicted_claims += 1;
            }
        }
    }
    out
}

/// One IRI that more than one dataset mints, and who minted it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IriCollision {
    /// The IRI two or more datasets both produce.
    pub iri: String,
    /// Each dataset that mints it, paired with the id it used. Sorted by
    /// dataset, then id; one entry per (dataset, id) pair.
    pub occurrences: Vec<(String, String)>,
}

impl IriCollision {
    /// A warning line naming the IRI and every dataset that mints it.
    pub fn message(&self) -> String {
        let where_ = self
            .occurrences
            .iter()
            .map(|(dataset, id)| format!("`{id}` in {dataset}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{} is minted by more than one dataset: {where_}", self.iri)
    }
}

/// Every IRI that two or more of `datasets` both mint — the records that
/// silently become one individual when the datasets are loaded together.
///
/// Keyed on the minted IRI rather than the id, so a bare id and its CURIE
/// form are recognised as the same individual. Repetition *within* one
/// dataset is not a collision here; identifier uniqueness owns that, and
/// reporting it twice would make one problem look like two.
///
/// Overlap is not automatically wrong — a teaching preview that is a strict
/// subset of a worked example shares records on purpose — so this states what
/// overlaps and leaves the policy to the author.
pub fn cross_dataset_iri_collisions(
    schema: &crate::linkml::SchemaDefinition,
    datasets: &[(&str, &crate::instances::InstanceSet)],
) -> Vec<IriCollision> {
    use std::collections::BTreeMap;
    // IRI -> (dataset, id) pairs, deduplicated and ordered by the map.
    let mut minted: BTreeMap<String, std::collections::BTreeSet<(String, String)>> =
        BTreeMap::new();
    for (label, set) in datasets {
        for inst in &set.instances {
            let iri = crate::rdf_serializers::instance_iri_string(schema, inst);
            minted
                .entry(iri)
                .or_default()
                .insert(((*label).to_string(), inst.id.clone()));
        }
    }
    minted
        .into_iter()
        .filter_map(|(iri, occurrences)| {
            let datasets_involved: std::collections::BTreeSet<&String> =
                occurrences.iter().map(|(dataset, _)| dataset).collect();
            (datasets_involved.len() > 1).then(|| IriCollision {
                iri,
                occurrences: occurrences.into_iter().collect(),
            })
        })
        .collect()
}

/// One entity that two or more datasets each define locally, so scoping has
/// split it into distinct individuals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnintendedSplit {
    /// The id each dataset used.
    pub id: String,
    /// The class they all instantiate.
    pub class: String,
    /// The datasets defining it, sorted.
    pub datasets: Vec<String>,
}

impl UnintendedSplit {
    /// A warning line naming the record and every dataset defining it.
    pub fn message(&self) -> String {
        format!(
            "`{}` (class `{}`) is defined identically by {}, which scope apart — \
             so they are now distinct individuals. If they denote one entity, \
             move it to a shared dataset and reference it by CURIE.",
            self.id,
            self.class,
            self.datasets.join(", ")
        )
    }
}

/// Records that scoping has split: the same id, of the same class, defined in
/// datasets that mint it to different IRIs.
///
/// This is the inverse of [`cross_dataset_iri_collisions`] and the hazard
/// scoping introduces — after scoping there is no collision left for that
/// check to find, so a shared entity left in two scoped datasets becomes two
/// individuals silently, each dataset internally valid.
///
/// **Records whose content differs are not reported.** Two estates'
/// `api-gateway` are different services that share a generic name, and
/// warning about them would fire on every separation scoping got right.
/// Requiring the records to be indistinguishable is what makes a report worth
/// believing; the cost is silence when one entity is described with differing
/// detail.
pub fn cross_dataset_unintended_splits(
    schema: &crate::linkml::SchemaDefinition,
    datasets: &[(&str, &crate::instances::InstanceSet)],
) -> Vec<UnintendedSplit> {
    use std::collections::BTreeMap;
    // (id, class) -> dataset -> (minted IRI, comparable content)
    type Seen = BTreeMap<String, (String, (String, Vec<crate::instances::SlotValue>))>;
    let mut by_record: BTreeMap<(String, String), Seen> = BTreeMap::new();

    for (label, set) in datasets {
        for inst in &set.instances {
            let Some(class) = inst.types.first() else {
                continue;
            };
            let iri = crate::rdf_serializers::instance_iri_string(schema, inst);
            // The authored assignments, not the display literals: a slot
            // serving as the record's label never reaches `literals`, so
            // comparing those would call every same-named record identical.
            let mut assignments = inst.slot_values.clone();
            assignments.sort_by(|a, b| a.slot.cmp(&b.slot));
            let content = (inst.label.clone(), assignments);
            by_record
                .entry((inst.id.clone(), class.clone()))
                .or_default()
                .insert((*label).to_string(), (iri, content));
        }
    }

    by_record
        .into_iter()
        .filter_map(|((id, class), seen)| {
            if seen.len() < 2 {
                return None;
            }
            let mut values = seen.values();
            let (first_iri, first_content) = values.next()?;
            // Same IRI means they already merged — the collision check owns
            // that, and reporting it here too would double-report one problem.
            if values.clone().all(|(iri, _)| iri == first_iri) {
                return None;
            }
            if !values.all(|(_, content)| content == first_content) {
                return None;
            }
            Some(UnintendedSplit {
                id,
                class,
                datasets: seen.into_keys().collect(),
            })
        })
        .collect()
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
    /// an RDF-specific gap it has nothing to do with. For `rules` the
    /// line also names the projection that does carry the constraint:
    /// OWL has no native construct for conditional rules, so the `shacl`
    /// output is the constraint-bearing RDF projection by design — and an
    /// RDF consumer wanting one graph can union it with the ontology.
    pub fn message(&self, format: &str) -> String {
        let mut message = format!(
            "class `{}` declares `{}`, which panschema does not emit to the `{}` format",
            self.class, self.construct, format
        );
        if self.construct == "rules" {
            message.push_str(" — the `shacl` format carries them as shapes");
        }
        message
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

/// A class using a specializing slot without its parent: the subset the
/// schema states (`child is_a parent`) cannot be checked on that class's
/// records, because there is no parent slot there to contain the values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UncheckedSpecialization {
    /// The class whose records escape the check.
    pub class: String,
    /// The specializing slot the class uses.
    pub child: String,
    /// The parent slot the class does not use.
    pub parent: String,
}

impl UncheckedSpecialization {
    /// A user-facing warning line.
    pub fn message(&self) -> String {
        format!(
            "class `{}` uses slot `{}`, which specializes `{}`, without the parent slot — \
             the subset is not validated for its records",
            self.class, self.child, self.parent
        )
    }
}

/// Report every class whose effective slots include a specializing slot
/// but not its parent — there, `validate` has nothing to check the subset
/// against, while the RDF output still asserts `rdfs:subPropertyOf`, so
/// the skip must be visible rather than silent. A parent that resolves
/// nowhere at all is the dangling-reference diagnostic's report, not a
/// second one here.
pub fn unchecked_specializations(schema: &SchemaDefinition) -> Vec<UncheckedSpecialization> {
    let mut all_slot_names: std::collections::BTreeSet<&str> =
        schema.slots.keys().map(String::as_str).collect();
    for class in schema.classes.values() {
        all_slot_names.extend(class.attributes.keys().map(String::as_str));
    }

    let mut found = Vec::new();
    for (class_name, class) in &schema.classes {
        let effective = crate::linkml_resolve::resolve_effective_slots(class, schema);
        for (child_name, def) in &effective {
            if let Some(parent) = &def.is_a
                && !effective.contains_key(parent)
                && all_slot_names.contains(parent.as_str())
            {
                found.push(UncheckedSpecialization {
                    class: class_name.clone(),
                    child: child_name.clone(),
                    parent: parent.clone(),
                });
            }
        }
    }
    found
}

/// Every specializing slot in the schema — top-level `slots:` and class
/// attributes — as `(child, parent)` pairs, deduplicated and sorted. A
/// writer whose output format has no sub-property form reports the drop
/// through this instead of making it silently; the SHACL writer does so
/// today.
pub fn slot_specializations(schema: &SchemaDefinition) -> Vec<(String, String)> {
    let mut pairs: std::collections::BTreeSet<(String, String)> = std::collections::BTreeSet::new();
    for (name, slot) in &schema.slots {
        if let Some(parent) = &slot.is_a {
            pairs.insert((name.clone(), parent.clone()));
        }
    }
    for class in schema.classes.values() {
        for (name, slot) in &class.attributes {
            if let Some(parent) = &slot.is_a {
                pairs.insert((name.clone(), parent.clone()));
            }
        }
    }
    pairs.into_iter().collect()
}

/// Gap messages for slot specializations a format cannot express — one
/// line per [`slot_specializations`] pair, phrased for the named format.
/// The SHACL writer words its own version (naming the missing SHACL Core
/// constraint); every other writer without a sub-property form uses this.
pub fn slot_specialization_gaps(schema: &SchemaDefinition, format: &str) -> Vec<String> {
    slot_specializations(schema)
        .into_iter()
        .map(|(child, parent)| {
            format!(
                "slot `{child}` specializes `{parent}`, which the {format} output cannot \
                 express — the subset relation is not carried"
            )
        })
        .collect()
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
    #[test]
    fn the_lint_shares_validates_membership_and_reports_each_defect_once() {
        use crate::linkml::{
            ClassDefinition, ClassRule, EnumDefinition, PermissibleValue, RuleConditions,
            SchemaDefinition, SlotCondition, SlotDefinition,
        };
        let mut schema = SchemaDefinition::new("s");
        // An OWL-loaded shape: map key `approved`, display text `Approved`.
        let mut kinds = EnumDefinition::new("Verdict");
        let mut pv = PermissibleValue::new("Approved");
        pv.text = "Approved".to_string();
        kinds.permissible_values.insert("approved".to_string(), pv);
        schema.enums.insert("Verdict".to_string(), kinds);
        // A dynamic enum whose value forms are not modeled: empty space.
        schema
            .enums
            .insert("Country".to_string(), EnumDefinition::new("Country"));
        let mut verdict = SlotDefinition::new("verdict");
        verdict.range = Some("Verdict".to_string());
        schema.slots.insert("verdict".to_string(), verdict);
        let mut country = SlotDefinition::new("country");
        country.range = Some("Country".to_string());
        schema.slots.insert("country".to_string(), country);
        let equals = |slot: &str, v: &str| RuleConditions {
            any_of: Vec::new(),
            slot_conditions: std::collections::BTreeMap::from([(
                slot.to_string(),
                SlotCondition {
                    equals_string: Some(v.to_string()),
                    ..Default::default()
                },
            )]),
        };
        let mut cls = ClassDefinition::new("Answer");
        cls.slots = vec!["verdict".into(), "country".into()];
        cls.rules = vec![
            // Matches by text, as `validate` would accept it: no finding.
            ClassRule {
                title: None,
                description: None,
                preconditions: Some(equals("verdict", "Approved")),
                postconditions: None,
            },
            // An empty value space has nothing to check against: no finding.
            ClassRule {
                title: None,
                description: None,
                preconditions: Some(equals("country", "US")),
                postconditions: None,
            },
            // One typo repeated across alternatives: one finding.
            ClassRule {
                title: None,
                description: None,
                preconditions: Some(RuleConditions {
                    any_of: vec![equals("verdict", "aproved"), equals("verdict", "aproved")],
                    slot_conditions: std::collections::BTreeMap::new(),
                }),
                postconditions: None,
            },
        ];
        schema.classes.insert("Answer".to_string(), cls);

        let findings = impossible_rule_values(&schema);
        assert_eq!(
            findings.len(),
            1,
            "text-matched and unmodeled-space constants pass; the one typo reports \
             once; got: {:?}",
            findings.iter().map(|f| f.message()).collect::<Vec<_>>()
        );
        assert!(findings[0].message().contains("aproved"));
    }

    #[test]
    fn a_dead_alternative_is_not_reported_as_the_whole_rule_dead() {
        use crate::linkml::{
            ClassDefinition, ClassRule, EnumDefinition, PermissibleValue, RuleConditions,
            SchemaDefinition, SlotCondition, SlotDefinition,
        };
        let mut schema = SchemaDefinition::new("s");
        let mut kinds = EnumDefinition::new("Verdict");
        for v in ["approved", "rejected"] {
            kinds
                .permissible_values
                .insert(v.to_string(), PermissibleValue::new(v));
        }
        schema.enums.insert("Verdict".to_string(), kinds);
        let mut verdict = SlotDefinition::new("verdict");
        verdict.range = Some("Verdict".to_string());
        schema.slots.insert("verdict".to_string(), verdict);
        let equals = |v: &str| RuleConditions {
            any_of: Vec::new(),
            slot_conditions: std::collections::BTreeMap::from([(
                "verdict".to_string(),
                SlotCondition {
                    equals_string: Some(v.to_string()),
                    ..Default::default()
                },
            )]),
        };
        let mut cls = ClassDefinition::new("Answer");
        cls.slots = vec!["verdict".into()];
        cls.rules = vec![ClassRule {
            title: Some("escalate".to_string()),
            description: None,
            // One good branch, one typo: the rule still fires.
            preconditions: Some(RuleConditions {
                any_of: vec![equals("approved"), equals("rejcted")],
                slot_conditions: std::collections::BTreeMap::new(),
            }),
            postconditions: None,
        }];
        schema.classes.insert("Answer".to_string(), cls);

        let findings = impossible_rule_values(&schema);
        assert_eq!(findings.len(), 1, "the dead branch is still a defect");
        let msg = findings[0].message();
        assert!(
            msg.contains("alternative")
                && msg.contains("never hold")
                && !msg.contains("never fire"),
            "a dead alternative is named as a dead alternative, not as the whole \
             rule being dead — a sibling branch still fires it; got: {msg}"
        );
    }

    #[test]
    fn same_titled_rules_report_their_defects_separately() {
        use crate::linkml::{
            ClassDefinition, ClassRule, EnumDefinition, PermissibleValue, RuleConditions,
            SchemaDefinition, SlotCondition, SlotDefinition,
        };
        let mut schema = SchemaDefinition::new("s");
        let mut kinds = EnumDefinition::new("Verdict");
        kinds
            .permissible_values
            .insert("approved".to_string(), PermissibleValue::new("approved"));
        schema.enums.insert("Verdict".to_string(), kinds);
        let mut verdict = SlotDefinition::new("verdict");
        verdict.range = Some("Verdict".to_string());
        schema.slots.insert("verdict".to_string(), verdict);
        let bad_rule = ClassRule {
            title: Some("verdict_gate".to_string()),
            description: None,
            preconditions: Some(RuleConditions {
                any_of: Vec::new(),
                slot_conditions: std::collections::BTreeMap::from([(
                    "verdict".to_string(),
                    SlotCondition {
                        equals_string: Some("aproved".to_string()),
                        ..Default::default()
                    },
                )]),
            }),
            postconditions: None,
        };
        let mut cls = ClassDefinition::new("Answer");
        cls.slots = vec!["verdict".into()];
        cls.rules = vec![bad_rule.clone(), bad_rule];
        schema.classes.insert("Answer".to_string(), cls);

        assert_eq!(
            impossible_rule_values(&schema).len(),
            2,
            "two copy-pasted rules sharing a title are two defects, not one"
        );
    }

    #[test]
    fn a_slot_usage_narrowed_union_is_still_linted() {
        use crate::linkml::{
            ClassDefinition, EnumDefinition, PermissibleValue, RuleConditions, SchemaDefinition,
            SlotCondition, SlotDefinition,
        };
        // The top-level slot is a union; the class narrows it to the one
        // enum through `slot_usage`. The induced view knows that, so the
        // class's rule constants are checked.
        let mut schema = SchemaDefinition::new("s");
        let mut kinds = EnumDefinition::new("Verdict");
        kinds
            .permissible_values
            .insert("approved".to_string(), PermissibleValue::new("approved"));
        schema.enums.insert("Verdict".to_string(), kinds);
        let mut verdict = SlotDefinition::new("verdict");
        let mut enum_branch = SlotDefinition::new("verdict");
        enum_branch.range = Some("Verdict".to_string());
        let mut string_branch = SlotDefinition::new("verdict");
        string_branch.range = Some("string".to_string());
        verdict.any_of = vec![enum_branch, string_branch];
        schema.slots.insert("verdict".to_string(), verdict);
        let mut cls = ClassDefinition::new("Answer");
        cls.slots = vec!["verdict".into()];
        let mut narrowed = SlotDefinition::new("verdict");
        narrowed.range = Some("Verdict".to_string());
        cls.slot_usage.insert("verdict".to_string(), narrowed);
        cls.rules = vec![crate::linkml::ClassRule {
            title: None,
            description: None,
            preconditions: Some(RuleConditions {
                any_of: Vec::new(),
                slot_conditions: std::collections::BTreeMap::from([(
                    "verdict".to_string(),
                    SlotCondition {
                        equals_string: Some("aproved".to_string()),
                        ..Default::default()
                    },
                )]),
            }),
            postconditions: None,
        }];
        schema.classes.insert("Answer".to_string(), cls);

        assert_eq!(
            impossible_rule_values(&schema).len(),
            1,
            "the class-narrowed slot is single-enum in the induced view, so its \
             typo'd constant is caught"
        );
    }

    #[test]
    fn a_rule_constant_outside_its_enums_value_space_is_reported() {
        use crate::linkml::{
            ClassDefinition, ClassRule, EnumDefinition, PermissibleValue, RuleConditions,
            SchemaDefinition, SlotCondition, SlotDefinition,
        };
        let mut schema = SchemaDefinition::new("s");
        let mut kinds = EnumDefinition::new("AnswerKind");
        kinds.permissible_values.insert(
            "closed-world-negative".to_string(),
            PermissibleValue::new("closed-world-negative"),
        );
        schema.enums.insert("AnswerKind".to_string(), kinds);
        let mut answer_kind = SlotDefinition::new("answer_kind");
        answer_kind.range = Some("AnswerKind".to_string());
        schema.slots.insert("answer_kind".to_string(), answer_kind);
        let mut status = SlotDefinition::new("status");
        status.range = Some("string".to_string());
        schema.slots.insert("status".to_string(), status);
        let equals = |slot: &str, v: &str| {
            Some(RuleConditions {
                any_of: Vec::new(),
                slot_conditions: std::collections::BTreeMap::from([(
                    slot.to_string(),
                    SlotCondition {
                        equals_string: Some(v.to_string()),
                        ..Default::default()
                    },
                )]),
            })
        };
        let mut cls = ClassDefinition::new("Answer");
        cls.slots = vec!["answer_kind".into(), "status".into()];
        cls.rules = vec![
            // Typo'd precondition constant: the rule can never fire.
            ClassRule {
                title: Some("negatives_state_absence".to_string()),
                description: None,
                preconditions: equals("answer_kind", "closed-word-negative"),
                postconditions: None,
            },
            // Valid constant: no finding.
            ClassRule {
                title: None,
                description: None,
                preconditions: equals("answer_kind", "closed-world-negative"),
                postconditions: None,
            },
            // Impossible postcondition: no record can satisfy the rule.
            ClassRule {
                title: None,
                description: None,
                preconditions: None,
                postconditions: equals("answer_kind", "open-world"),
            },
            // Non-enum slot: constants are unconstrained, no finding.
            ClassRule {
                title: None,
                description: None,
                preconditions: equals("status", "anything"),
                postconditions: None,
            },
        ];
        schema.classes.insert("Answer".to_string(), cls);

        let findings = impossible_rule_values(&schema);
        assert_eq!(
            findings.len(),
            2,
            "exactly the two out-of-space constants are reported; got: {:?}",
            findings.iter().map(|f| f.message()).collect::<Vec<_>>()
        );
        let first = findings[0].message();
        assert!(
            first.contains("negatives_state_absence")
                && first.contains("closed-word-negative")
                && first.contains("AnswerKind")
                && first.contains("never fire"),
            "a dead precondition names the rule, the constant, the enum, and the \
             consequence; got: {first}"
        );
        let second = findings[1].message();
        assert!(
            second.contains("open-world") && second.contains("satisf"),
            "an impossible postcondition states its own consequence; got: {second}"
        );

        assert!(
            should_fail_strict(true, findings.len()),
            "--strict refuses a schema whose rule can never fire"
        );
        assert!(
            !should_fail_strict(true, 0),
            "no findings, no strict failure"
        );
    }

    /// Two classes declaring same-named `attributes:` are distinct slots in
    /// LinkML but mint one property IRI in RDF — only one survives a
    /// read-back, and the emitted OWL asserts one property about both
    /// classes. The diagnostic names every definition site so an author can
    /// pick a rename, a shared top-level slot, or distinct `slot_uri`s.
    #[test]
    fn same_named_attributes_on_two_classes_are_reported() {
        let schema = parse(
            "name: s\nclasses:\n  Recipe:\n    attributes:\n      id:\n        range: string\n  Image:\n    attributes:\n      id:\n        range: string\n",
        );
        let collisions = super::colliding_slot_definitions(&schema);
        assert_eq!(
            collisions.len(),
            1,
            "one colliding name; got {collisions:?}"
        );
        let c = &collisions[0];
        assert_eq!(c.name, "id");
        assert_eq!(c.sites, vec!["class `Image`", "class `Recipe`"]);
        assert!(
            c.message().contains("one property IRI"),
            "the message should state the RDF consequence; got: {}",
            c.message()
        );
    }

    /// A top-level slot listed by several classes via `slots:` is one slot —
    /// the sharing is the point, and it must not be reported.
    #[test]
    fn a_shared_top_level_slot_is_not_a_collision() {
        let schema = parse(
            "name: s\nslots:\n  id:\n    range: string\nclasses:\n  Recipe:\n    slots: [id]\n  Image:\n    slots: [id]\n",
        );
        assert_eq!(super::colliding_slot_definitions(&schema), vec![]);
    }

    /// A class-local attribute shadowing a same-named top-level slot is two
    /// definitions at one IRI, and is reported.
    #[test]
    fn an_attribute_shadowing_a_top_level_slot_is_a_collision() {
        let schema = parse(
            "name: s\nslots:\n  id:\n    range: string\nclasses:\n  Recipe:\n    slots: [id]\n  Image:\n    attributes:\n      id:\n        range: string\n",
        );
        let collisions = super::colliding_slot_definitions(&schema);
        assert_eq!(collisions.len(), 1);
        assert_eq!(
            collisions[0].sites,
            vec!["class `Image`", "top-level `slots:`"]
        );
    }

    /// Distinct explicit `slot_uri`s mint distinct IRIs, so same-named
    /// definitions with their own URIs do not collide.
    #[test]
    fn distinct_slot_uris_do_not_collide() {
        let schema = parse(
            "name: s\nprefixes:\n  ex: https://example.org/\nclasses:\n  Recipe:\n    attributes:\n      id:\n        slot_uri: ex:recipeId\n        range: string\n  Image:\n    attributes:\n      id:\n        range: string\n",
        );
        assert_eq!(super::colliding_slot_definitions(&schema), vec![]);
    }

    use super::*;

    fn parse(yaml: &str) -> SchemaDefinition {
        serde_norway::from_str(yaml).expect("parse schema")
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
    fn a_slot_usage_is_a_naming_no_slot_is_dangling() {
        // The subset claim a `slot_usage` override states dies silently if
        // its parent is a typo — the override merge just carries the bad
        // name, the validator's gate never matches it, and nothing else
        // looks. The dangling pass is where it must surface.
        let schema = parse(
            "name: s\nslots:\n  citations: {}\nclasses:\n  C:\n    slots: [citations]\n    \
             slot_usage:\n      citations:\n        is_a: anchros\n",
        );
        let msgs: Vec<String> = dangling_references(&schema)
            .iter()
            .map(|d| d.message())
            .collect();
        assert!(
            msgs.iter()
                .any(|m| m.contains("citations") && m.contains("anchros") && m.contains("`C`")),
            "the typo'd slot_usage parent must be flagged with its class; got: {msgs:?}"
        );
    }

    #[test]
    fn a_class_using_the_child_without_the_parent_is_flagged() {
        // The subset can only be checked where both slots are usable; a
        // class carrying just the child gets a warning naming all three,
        // and a class carrying both stays silent.
        let schema = parse(
            "name: s\nslots:\n  anchors: {}\n  citations:\n    is_a: anchors\n\
             classes:\n  Partial:\n    slots: [citations]\n  Full:\n    slots: [anchors, citations]\n",
        );
        let found = unchecked_specializations(&schema);
        assert_eq!(
            found.len(),
            1,
            "only the class missing the parent is flagged; got: {found:?}"
        );
        assert_eq!(found[0].class, "Partial");
        assert_eq!(found[0].child, "citations");
        assert_eq!(found[0].parent, "anchors");
        assert_eq!(
            found[0].message(),
            "class `Partial` uses slot `citations`, which specializes `anchors`, without \
             the parent slot — the subset is not validated for its records"
        );
    }

    #[test]
    fn a_dangling_parent_is_not_double_reported_as_unchecked() {
        // A typo'd parent resolves nowhere: that is the dangling-reference
        // report's finding, and the unchecked-specialization pass stays
        // quiet rather than stacking a second warning on the same typo.
        let schema = parse(
            "name: s\nslots:\n  citations:\n    is_a: no_such\nclasses:\n  C:\n    slots: [citations]\n",
        );
        assert!(unchecked_specializations(&schema).is_empty());
    }

    #[test]
    fn a_slot_is_a_naming_no_slot_is_dangling() {
        // A slot-level `is_a` that names no slot states a subset relation
        // against nothing — no subPropertyOf is emitted and no containment
        // is checked, so the typo must be reported, not swallowed.
        let schema = parse("name: s\nslots:\n  child:\n    is_a: no_such_slot\n");
        let msgs: Vec<String> = dangling_references(&schema)
            .iter()
            .map(|d| d.message())
            .collect();
        assert!(
            msgs.iter().any(|m| m
                == "slot `child` specializes `no_such_slot`, which names no slot the schema defines"),
            "the unresolvable slot is_a must be flagged with its own phrasing; got: {msgs:?}"
        );
    }

    #[test]
    fn an_unresolvable_default_range_is_flagged_at_its_source() {
        // A typo'd `default_range` would otherwise fail silently — it types
        // nothing, and a schema with no rangeless slot shows no symptom at
        // all. One warning names the schema-level root cause.
        let schema = parse("name: s\ndefault_range: strng\nclasses:\n  Order: {}\n");
        let msgs: Vec<String> = dangling_references(&schema)
            .iter()
            .map(|d| d.message())
            .collect();
        assert!(
            msgs.iter()
                .any(|m| m.contains("default_range") && m.contains("strng")),
            "the unresolvable default must be flagged; got: {msgs:?}"
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
        // Not strict ⇒ never fail, whatever is present.
        assert!(!should_fail_strict(false, 2));
        // Strict + nothing blocking ⇒ ok.
        assert!(!should_fail_strict(true, 0));
        // Strict + any blocking finding ⇒ fail.
        assert!(should_fail_strict(true, 1), "strict + one finding ⇒ fail");
    }

    /// An annotation whose body carried nested annotations is reported
    /// at load with its site and tag, through the same diagnostics feed
    /// the other silent-drop warnings use; a class attribute's site
    /// names both the slot and its class, and a `slot_usage` entry's
    /// names the slot and the class narrowing it.
    #[test]
    fn nested_annotation_machinery_is_reported_at_load_with_its_site() {
        let schema = parse(
            "id: https://example.org/t\nname: t\nslots:\n  s1:\n    annotations:\n      \
             note:\n        value: hello\n        annotations:\n          provenance: curated\n\
             classes:\n  C:\n    attributes:\n      a1:\n        annotations:\n          \
             marked:\n            value: x\n            extensions:\n              e: 1\n    \
             slot_usage:\n      s1:\n        annotations:\n          narrowed:\n            \
             value: y\n            annotations:\n              p: c\n",
        );
        let findings = unmodeled_annotation_nesting(&schema);
        assert_eq!(
            findings
                .iter()
                .map(|f| (f.site.as_str(), f.tag.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("slot `a1` (class `C`)", "marked"),
                ("slot `s1` (slot_usage in class `C`)", "narrowed"),
                ("slot `s1`", "note"),
            ],
            "each nested-annotation site is reported once, slot_usage included"
        );
        let messages = schema_load_diagnostics(&schema);
        assert!(
            messages
                .iter()
                .any(|m| m.contains("`note`") && m.contains("slot `s1`") && m.contains("dropped")),
            "the load diagnostics carry the warning; got: {messages:?}"
        );
    }

    /// A `panschema:*` tag carrying a non-string value is reported at
    /// load: the tool ignores it and falls back, and the fallback — a
    /// raw name for a label, a vanished individual assertion — must
    /// never be silent.
    #[test]
    fn an_opaque_panschema_tag_is_reported_at_load() {
        let schema = parse(
            "id: https://example.org/t\nname: t\nclasses:\n  C:\n    annotations:\n      \
             panschema:label:\n        value:\n          en: Human\n      \
             other:tool:\n        value:\n          a: b\n",
        );
        let findings = opaque_panschema_annotations(&schema);
        assert_eq!(
            findings
                .iter()
                .map(|f| (f.site.as_str(), f.tag.as_str()))
                .collect::<Vec<_>>(),
            vec![("class `C`", "panschema:label")],
            "only the tool's own namespace is policed; other tools' structured values are theirs"
        );
        let messages = schema_load_diagnostics(&schema);
        assert!(
            messages
                .iter()
                .any(|m| m.contains("`panschema:label`") && m.contains("ignores")),
            "the load diagnostics carry the warning; got: {messages:?}"
        );
    }

    /// A slot with no `range:` that no `default_range` covered means each
    /// output makes its own choice about what untyped means; the diagnostic
    /// names the slot and its site so the author decides instead.
    #[test]
    fn a_rangeless_slot_with_no_default_range_is_reported_with_its_site() {
        let schema = parse(
            "name: s\nslots:\n  note: {}\nclasses:\n  Event:\n    attributes:\n      label: {}\n",
        );
        let found = super::untyped_slots(&schema);
        assert_eq!(
            found,
            vec![
                UntypedSlot {
                    name: "label".to_string(),
                    site: "class `Event`".to_string(),
                },
                UntypedSlot {
                    name: "note".to_string(),
                    site: "top-level `slots:`".to_string(),
                },
            ],
            "both definition sites are named"
        );
        assert!(
            found[0]
                .message()
                .contains("JSON Schema types it as `string`"),
            "the message states how the outputs disagree; got: {}",
            found[0].message()
        );
        assert!(
            schema_load_diagnostics(&schema)
                .iter()
                .any(|m| m.contains("slot `note`")),
            "the shared load path surfaces it for every command"
        );
    }

    /// The reporting condition is the loader's own fill predicate:
    /// a slot the materialization would fill is clean once load runs, and a
    /// slot it deliberately skips is not "untyped".
    #[test]
    fn untyped_slot_reporting_mirrors_default_range_materialization() {
        let mut schema = parse(
            "name: s\ndefault_range: string\nslots:\n  note: {}\nclasses:\n  Event:\n    attributes:\n      label: {}\n",
        );
        crate::linkml_resolve::materialize_default_range(&mut schema);
        assert_eq!(
            super::untyped_slots(&schema),
            vec![],
            "a declared default_range covers every rangeless slot at load"
        );

        let schema = parse(
            "name: s\nslots:\n  typed: {range: string}\n  union:\n    any_of:\n      - range: string\n      - range: integer\n  voided: {maximum_cardinality: 0}\n",
        );
        assert_eq!(
            super::untyped_slots(&schema),
            vec![],
            "an explicit range, a range-carrying any_of, and a voided slot are all typed enough"
        );
    }

    /// A top-level slot every consumer ranges through `slot_usage` is fully
    /// typed in every output, so reporting it (and failing `--strict` on
    /// it) would punish an ordinary LinkML pattern.
    #[test]
    fn a_slot_ranged_by_every_use_via_slot_usage_is_not_untyped() {
        let schema = parse(
            "name: s\nslots:\n  note: {}\nclasses:\n  Event:\n    name: Event\n    slots: [note]\n    slot_usage:\n      note: {range: string}\n",
        );
        assert_eq!(
            super::untyped_slots(&schema),
            vec![],
            "the resolved view carries the slot_usage range"
        );
    }

    /// A slot a class introduces only through `slot_usage` never passes
    /// through `default_range` materialization, so it is exactly the
    /// ambiguity this diagnostic exists to surface.
    #[test]
    fn a_slot_introduced_only_via_slot_usage_is_reported() {
        let schema = parse(
            "name: s\nclasses:\n  Event:\n    name: Event\n    slot_usage:\n      label: {required: true}\n",
        );
        assert_eq!(
            super::untyped_slots(&schema),
            vec![UntypedSlot {
                name: "label".to_string(),
                site: "class `Event`".to_string(),
            }],
        );
    }

    /// An `any_of` whose branches carry only facets constrains values it
    /// never types — as untyped as a bare rangeless slot, and reported the
    /// same. The loader agrees: a declared `default_range` fills it.
    #[test]
    fn a_facet_only_any_of_is_untyped_and_a_default_fills_it() {
        let schema = parse(
            "name: s\nslots:\n  u:\n    any_of:\n      - pattern: '^a'\n      - pattern: '^b'\n",
        );
        assert_eq!(
            super::untyped_slots(&schema),
            vec![UntypedSlot {
                name: "u".to_string(),
                site: "top-level `slots:`".to_string(),
            }],
        );

        let mut schema = parse(
            "name: s\ndefault_range: string\nslots:\n  u:\n    any_of:\n      - pattern: '^a'\n      - pattern: '^b'\n",
        );
        crate::linkml_resolve::materialize_default_range(&mut schema);
        assert_eq!(
            schema.slots.get("u").and_then(|s| s.range.as_deref()),
            Some("string"),
            "the default types the slot; the branches keep constraining values"
        );
        assert_eq!(super::untyped_slots(&schema), vec![]);
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
        assert_eq!(
            found[0].message("ttl"),
            "class `Deployment` declares `rules`, which panschema does not emit to the `ttl` \
             format — the `shacl` format carries them as shapes",
            "the rules warning names the projection that carries the constraint"
        );
        assert_eq!(
            found[1].message("ttl"),
            "class `Offering` declares `unique_keys`, which panschema does not emit to the \
             `ttl` format",
            "the shapes pointer belongs to `rules` alone"
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

    fn instance(id: &str, refs: &[(&str, &str)]) -> crate::instances::Instance {
        crate::instances::Instance {
            id: id.to_string(),
            iri: None,
            uri_unresolved: false,
            label: id.to_string(),
            description: None,
            types: Vec::new(),
            literals: Vec::new(),
            references: refs
                .iter()
                .map(|(property, target)| crate::instances::Reference {
                    external: false,
                    property: property.to_string(),
                    target: target.to_string(),
                })
                .collect(),
            slot_values: Vec::new(),
            scope: None,
        }
    }

    #[test]
    fn dangling_instance_reference_is_reported_with_referrer_property_and_target() {
        // `wineB` listed before `wineA` so the reported order pins the sort.
        let set = crate::instances::InstanceSet {
            instances: vec![
                instance("wineB", &[("produced_by", "ghostTwo")]),
                instance("wineA", &[("produced_by", "ghostOne")]),
                instance("realWinery", &[]),
            ],
            ..Default::default()
        };
        let danglers = dangling_instance_references(&set);
        assert_eq!(danglers.len(), 2, "two references resolve to no instance");
        // Deterministic: sorted by referrer id, then property, then target.
        assert_eq!(danglers[0].referrer, "wineA");
        assert_eq!(danglers[1].referrer, "wineB");

        assert_eq!(danglers[0].property, "produced_by");
        assert_eq!(danglers[0].target, "ghostOne");
        let msg = danglers[0].message();
        assert!(
            msg.contains("wineA") && msg.contains("produced_by") && msg.contains("ghostOne"),
            "message must name referrer, property, and missing target; got: {msg}"
        );
    }

    fn collision_schema() -> SchemaDefinition {
        let mut schema = SchemaDefinition::new("cellar");
        schema.id = Some("https://example.org/cellar".to_string());
        schema.default_prefix = Some("cellar".to_string());
        schema.prefixes.insert(
            "cellar".to_string(),
            "https://example.org/cellar/".to_string(),
        );
        schema
    }

    fn dataset(ids: &[&str]) -> crate::instances::InstanceSet {
        crate::instances::InstanceSet {
            instances: ids.iter().map(|id| instance(id, &[])).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn a_schema_with_several_tree_roots_warns_once_naming_them() {
        // The metamodel: "each schema should have at most one tree root."
        // Two roots are deliberate here (a scoped root plus a reference
        // root), but the deviation from that "should" gets said out loud,
        // since upstream LinkML tooling may not honor per-dataset selection.
        let mut schema = collision_schema();
        for name in ["Enterprise", "ProviderCatalog"] {
            let mut class = crate::linkml::ClassDefinition::new(name);
            class.tree_root = true;
            schema.classes.insert(name.to_string(), class);
        }
        let warnings = schema_load_diagnostics(&schema);
        let root_warnings: Vec<&String> = warnings
            .iter()
            .filter(|w| w.contains("tree_root"))
            .collect();
        assert_eq!(root_warnings.len(), 1, "one warning, not one per root");
        let msg = root_warnings[0];
        assert!(
            msg.contains("Enterprise")
                && msg.contains("ProviderCatalog")
                && msg.contains("at most one"),
            "it names the roots and the metamodel's recommendation; got: {msg}"
        );
    }

    #[test]
    fn a_single_tree_root_draws_no_root_warning() {
        let mut schema = collision_schema();
        let mut class = crate::linkml::ClassDefinition::new("Catalog");
        class.tree_root = true;
        schema.classes.insert("Catalog".to_string(), class);
        assert!(
            !schema_load_diagnostics(&schema)
                .iter()
                .any(|w| w.contains("tree_root")),
            "one root is the recommended shape; nothing to say"
        );
    }

    #[test]
    fn an_id_used_by_two_datasets_is_reported_with_both_dataset_names() {
        // The silent merge this makes visible: loaded together, these two
        // `api-gateway` records are one individual.
        let schema = collision_schema();
        let acme = dataset(&["api-gateway", "billing"]);
        let contoso = dataset(&["api-gateway", "search"]);
        let collisions = cross_dataset_iri_collisions(
            &schema,
            &[("acme.yaml", &acme), ("contoso.yaml", &contoso)],
        );
        assert_eq!(
            collisions.len(),
            1,
            "only the shared id collides; got: {collisions:?}"
        );
        let c = &collisions[0];
        assert_eq!(c.iri, "https://example.org/cellar/api-gateway");
        assert_eq!(
            c.occurrences,
            vec![
                ("acme.yaml".to_string(), "api-gateway".to_string()),
                ("contoso.yaml".to_string(), "api-gateway".to_string()),
            ],
            "each dataset that mints the IRI is named, with the id it used"
        );
        let msg = c.message();
        assert!(
            msg.contains("api-gateway")
                && msg.contains("acme.yaml")
                && msg.contains("contoso.yaml"),
            "the message names the id and every file; got: {msg}"
        );
    }

    #[test]
    fn an_id_repeated_inside_one_dataset_is_not_a_cross_dataset_collision() {
        // That is the identifier-uniqueness check's job; reporting it here too
        // would double-report one problem as two.
        let schema = collision_schema();
        let one = dataset(&["api-gateway", "api-gateway"]);
        assert!(
            cross_dataset_iri_collisions(&schema, &[("one.yaml", &one)]).is_empty(),
            "a single dataset can collide with nothing"
        );
    }

    #[test]
    fn two_spellings_that_mint_one_iri_collide_even_though_the_ids_differ() {
        // Keying on the minted IRI rather than the id is the point: a bare id
        // and its CURIE form are the same individual.
        let schema = collision_schema();
        let bare = dataset(&["gamay"]);
        let curie = dataset(&["cellar:gamay"]);
        let collisions =
            cross_dataset_iri_collisions(&schema, &[("bare.yaml", &bare), ("curie.yaml", &curie)]);
        assert_eq!(
            collisions.len(),
            1,
            "different ids minting one IRI still merge; got: {collisions:?}"
        );
        assert_eq!(
            collisions[0].occurrences,
            vec![
                ("bare.yaml".to_string(), "gamay".to_string()),
                ("curie.yaml".to_string(), "cellar:gamay".to_string()),
            ],
            "and the report shows which spelling each dataset used"
        );
    }

    /// A schema whose root bears an identifier, so its datasets scope apart —
    /// the precondition for an unintended split.
    fn scoped_schema() -> SchemaDefinition {
        let mut schema = collision_schema();
        schema.default_range = Some("string".to_string());
        let mut id = crate::linkml::SlotDefinition::new("id");
        id.identifier = true;

        let mut root = crate::linkml::ClassDefinition::new("Enterprise");
        root.tree_root = true;
        root.attributes.insert("id".to_string(), id);
        let mut providers = crate::linkml::SlotDefinition::new("providers");
        providers.range = Some("Provider".to_string());
        providers.multivalued = true;
        root.attributes.insert("providers".to_string(), providers);
        schema.classes.insert("Enterprise".to_string(), root);

        // Contained records bear a `key` — unique within their container —
        // which is what makes them scope apart per dataset; the root's
        // `identifier` stays global, since the root IS the scope.
        let mut key = crate::linkml::SlotDefinition::new("id");
        key.key = true;
        let mut provider = crate::linkml::ClassDefinition::new("Provider");
        provider.attributes.insert("id".to_string(), key);
        provider.attributes.insert(
            "name".to_string(),
            crate::linkml::SlotDefinition::new("name"),
        );
        schema.classes.insert("Provider".to_string(), provider);
        schema
    }

    fn scoped_set(schema: &SchemaDefinition, yaml: &str) -> crate::instances::InstanceSet {
        let data: serde_norway::Value = serde_norway::from_str(yaml).unwrap();
        crate::instances::InstanceSet::from_linkml_data(schema, &data)
    }

    #[test]
    fn one_entity_defined_in_two_scoped_datasets_is_reported_as_a_possible_split() {
        // The hazard scoping introduces: `aws` is one company, but each estate
        // defined it locally, so the two no longer merge and nothing collides.
        let schema = scoped_schema();
        let acme = scoped_set(
            &schema,
            "id: acme\nproviders:\n  - {id: aws, name: Amazon Web Services}\n",
        );
        let contoso = scoped_set(
            &schema,
            "id: contoso\nproviders:\n  - {id: aws, name: Amazon Web Services}\n",
        );
        let splits = cross_dataset_unintended_splits(
            &schema,
            &[("acme.yaml", &acme), ("contoso.yaml", &contoso)],
        );
        assert_eq!(splits.len(), 1, "one entity, one report; got: {splits:?}");
        assert_eq!(splits[0].id, "aws");
        assert_eq!(splits[0].class, "Provider");
        assert_eq!(
            splits[0].datasets,
            vec!["acme.yaml".to_string(), "contoso.yaml".to_string()]
        );
        let msg = splits[0].message();
        assert!(
            msg.contains("aws") && msg.contains("acme.yaml") && msg.contains("contoso.yaml"),
            "the message names the id and both datasets; got: {msg}"
        );
    }

    /// A benchmark-shaped schema whose `anchors` reference records of
    /// another dataset by IRI — the referring side of `resolve_against`.
    fn benchmark_schema() -> SchemaDefinition {
        let mut schema = SchemaDefinition::new("bench");
        schema.id = Some("https://example.org/bench".to_string());
        schema.default_prefix = Some("bench".to_string());
        schema.prefixes.insert(
            "bench".to_string(),
            "https://example.org/bench/".to_string(),
        );
        schema.prefixes.insert(
            "cellar".to_string(),
            "https://example.org/cellar/".to_string(),
        );
        schema.default_range = Some("string".to_string());
        let mut id = crate::linkml::SlotDefinition::new("id");
        id.identifier = true;
        let mut root = crate::linkml::ClassDefinition::new("Bench");
        root.tree_root = true;
        root.attributes.insert("id".to_string(), id.clone());
        let mut anchors = crate::linkml::SlotDefinition::new("anchors");
        anchors.range = Some("DomainRecord".to_string());
        anchors.multivalued = true;
        root.attributes.insert("anchors".to_string(), anchors);
        schema.classes.insert("Bench".to_string(), root);
        let mut record = crate::linkml::ClassDefinition::new("DomainRecord");
        record.attributes.insert("id".to_string(), id);
        schema.classes.insert("DomainRecord".to_string(), record);
        schema
    }

    /// The sibling namespace every resolution test scopes by — what
    /// `instance_namespace(scoped_schema())` derives.
    const CELLAR_NS: &str = "https://example.org/cellar/";

    #[test]
    fn a_reference_matching_a_sibling_minted_iri_resolves() {
        let sibling_schema = scoped_schema();
        let estate = scoped_set(
            &sibling_schema,
            "id: acme\nproviders:\n  - {id: aws, name: Amazon Web Services}\n",
        );
        let minted = minted_instance_iris(&sibling_schema, std::slice::from_ref(&estate));
        let scoped_aws = minted
            .iter()
            .find(|iri| iri.contains("aws"))
            .expect("the provider mints an IRI")
            .clone();
        assert!(
            scoped_aws.contains("acme"),
            "the key-scoped record mints beneath its root; got: {scoped_aws}"
        );
        let owned = [CELLAR_NS.to_string()];

        let schema = benchmark_schema();
        let resolved = scoped_set(&schema, &format!("id: b1\nanchors:\n  - {scoped_aws}\n"));
        let r = resolve_sibling_references(&schema, &resolved, &owned, &minted);
        assert_eq!(r.checked, 1);
        assert_eq!(r.unresolved, vec![], "the minted IRI resolves");

        let naive = scoped_set(
            &schema,
            "id: b1\nanchors:\n  - https://example.org/cellar/aws\n",
        );
        let missed = resolve_sibling_references(&schema, &naive, &owned, &minted).unresolved;
        assert_eq!(
            missed.len(),
            1,
            "the namespace+bare-id guess does not resolve a scoped record; got: {missed:?}"
        );
        assert_eq!(missed[0].referrer, "b1");
        assert_eq!(missed[0].property, "anchors");
        assert!(
            missed[0].message().contains("anchors")
                && missed[0]
                    .message()
                    .contains("https://example.org/cellar/aws"),
            "the message names the slot and the reference; got: {}",
            missed[0].message()
        );
    }

    #[test]
    fn a_curie_reference_expands_against_the_referring_schema() {
        // The reference is authored as a CURIE against the *referring*
        // schema's declared prefix — the same expansion the RDF emission
        // performs — and resolves when the sibling mints that IRI.
        let sibling_schema = scoped_schema();
        let estate = scoped_set(&sibling_schema, "id: acme\nproviders: []\n");
        let minted = minted_instance_iris(&sibling_schema, std::slice::from_ref(&estate));
        assert!(
            minted.contains("https://example.org/cellar/acme"),
            "the identifier-bearing root mints globally; got: {minted:?}"
        );

        let schema = benchmark_schema();
        let set = scoped_set(&schema, "id: b1\nanchors:\n  - cellar:acme\n");
        let owned = [CELLAR_NS.to_string()];
        let r = resolve_sibling_references(&schema, &set, &owned, &minted);
        assert_eq!(r.checked, 1);
        assert_eq!(
            r.unresolved,
            vec![],
            "the CURIE denotes the sibling's minted IRI"
        );
    }

    #[test]
    fn a_reference_outside_every_owned_namespace_is_not_checked() {
        // A dataset can cite vocabularies outside the manifest by design;
        // only references landing in a sibling-owned namespace are
        // required to resolve, so one schema.org IRI cannot fail the run.
        let sibling_schema = scoped_schema();
        let estate = scoped_set(&sibling_schema, "id: acme\nproviders: []\n");
        let minted = minted_instance_iris(&sibling_schema, std::slice::from_ref(&estate));
        let owned = [CELLAR_NS.to_string()];

        let schema = benchmark_schema();
        let set = scoped_set(
            &schema,
            "id: b1\nanchors:\n  - https://schema.org/Thing\n  - cellar:acme\n",
        );
        let r = resolve_sibling_references(&schema, &set, &owned, &minted);
        assert_eq!(
            r.checked, 1,
            "only the cellar-namespace reference is in jurisdiction"
        );
        assert_eq!(r.unresolved, vec![], "and it resolves");
        assert_eq!(
            r.uncovered.len(),
            1,
            "the out-of-jurisdiction reference is classified in the same pass"
        );
        assert_eq!(r.uncovered[0].target, "https://schema.org/Thing");
        assert!(
            r.uncovered[0].message().contains("no namespace covered"),
            "got: {}",
            r.uncovered[0].message()
        );
    }

    #[test]
    fn a_fully_qualified_self_reference_is_not_the_siblings_problem() {
        // A record referencing its own dataset by full IRI is classified
        // external, but it lands in the referring schema's namespace, not
        // a sibling's — demanding the sibling mint it would fail valid
        // data.
        let sibling_schema = scoped_schema();
        let estate = scoped_set(&sibling_schema, "id: acme\nproviders: []\n");
        let minted = minted_instance_iris(&sibling_schema, std::slice::from_ref(&estate));
        let owned = [CELLAR_NS.to_string()];

        let schema = benchmark_schema();
        let set = scoped_set(
            &schema,
            "id: b1\nanchors:\n  - https://example.org/bench/b1\n",
        );
        let r = resolve_sibling_references(&schema, &set, &owned, &minted);
        assert_eq!(r.checked, 0, "a self-namespace reference is out of scope");
        assert_eq!(r.unresolved, vec![]);
    }

    /// A sibling whose datasets can join records: `Link` records connect
    /// two `Item`s, and the identifier-bearing root holds everything.
    fn linked_sibling_schema() -> SchemaDefinition {
        let mut schema = collision_schema();
        schema.default_range = Some("string".to_string());
        let mut id = crate::linkml::SlotDefinition::new("id");
        id.identifier = true;

        let mut root = crate::linkml::ClassDefinition::new("Root");
        root.tree_root = true;
        root.attributes.insert("id".to_string(), id.clone());
        for (name, range) in [("items", "Item"), ("links", "Link")] {
            let mut slot = crate::linkml::SlotDefinition::new(name);
            slot.range = Some(range.to_string());
            slot.multivalued = true;
            root.attributes.insert(name.to_string(), slot);
        }
        schema.classes.insert("Root".to_string(), root);

        let mut item = crate::linkml::ClassDefinition::new("Item");
        item.attributes.insert("id".to_string(), id.clone());
        schema.classes.insert("Item".to_string(), item);

        let mut link = crate::linkml::ClassDefinition::new("Link");
        link.attributes.insert("id".to_string(), id);
        for end in ["a", "b"] {
            let mut slot = crate::linkml::SlotDefinition::new(end);
            slot.range = Some("Item".to_string());
            link.attributes.insert(end.to_string(), slot);
        }
        schema.classes.insert("Link".to_string(), link);
        schema
    }

    /// A schema states which slots carry absence claims by annotating
    /// them: a top-level declaration binds schema-wide, a class
    /// attribute's binds that class, and both surface in the bindings.
    #[test]
    fn absence_bindings_read_every_annotated_slot() {
        let schema = parse(
            "id: https://example.org/t\nname: t\nslots:\n  unconnected:\n    range: uri\n    \
             annotations:\n      asserts_absence:\n        value:\n          via_slot: \
             connecting_class\n  connecting_class:\n    range: uri\nclasses:\n  Bench:\n    \
             slots: [unconnected, connecting_class]\n    attributes:\n      lone:\n        \
             range: uri\n        multivalued: true\n        annotations:\n          \
             asserts_absence:\n            value: null\n",
        );
        let (bindings, issues) = absence_declarations(&schema);
        assert_eq!(
            bindings.by_slot.keys().collect::<Vec<_>>(),
            vec!["lone", "unconnected"],
            "each annotated slot is one entry, in slot order"
        );
        assert_eq!(
            bindings.governing(&["Bench".to_string()], "unconnected"),
            Some(&AbsenceVia::Slot("connecting_class".to_string())),
            "the top-level declaration governs every carrying class"
        );
        assert_eq!(
            bindings.governing(&["Bench".to_string()], "lone"),
            Some(&AbsenceVia::Unnarrowed),
            "the attribute declaration governs its class"
        );
        assert_eq!(
            bindings.governing(&["Other".to_string()], "lone"),
            None,
            "a class-scoped declaration does not govern other classes"
        );
        assert!(issues.is_empty(), "well-formed declarations raise no issue");
        assert!(
            !bindings.is_empty(),
            "a schema with declarations reports them present"
        );
        assert_eq!(
            bindings.len(),
            2,
            "the enablement note counts declaring slots"
        );
        assert!(
            absence_bindings(&SchemaDefinition::new("bare")).is_empty(),
            "a schema without declarations reports none"
        );
    }

    #[test]
    fn version_declaration_defects_are_load_warnings() {
        let schema = parse(
            "id: https://example.org/t\nname: t\nslots:\n  pinned:\n    range: string\n    \
             annotations:\n      records_version_of:\n        value:\n          sibling_slot: ghost\n          \
             extra: 1\n  selfish:\n    range: string\n    annotations:\n      records_version_of:\n        \
             value:\n          sibling_slot: selfish\n  bare:\n    range: string\n    annotations:\n      \
             records_version_of:\n        value: null\n  orphan:\n    range: string\n    annotations:\n      \
             records_version_of:\n        value:\n          sibling_slot: pinned\nclasses:\n  Bench:\n    \
             slots: [pinned, selfish, bare]\n",
        );
        let issues = version_declaration_issues(&schema);
        let details: Vec<String> = issues.iter().map(|i| i.message()).collect();
        let has = |needle: &str| details.iter().any(|d| d.contains(needle));
        assert!(
            has("unrecognized field `extra`"),
            "unknown field is named: {details:?}"
        );
        assert!(
            has("names `ghost`, which no class carries"),
            "unknown sibling_slot is named: {details:?}"
        );
        assert!(
            has("names the slot itself"),
            "self-naming is refused: {details:?}"
        );
        assert!(
            has("must be a mapping"),
            "a null value is a defect: {details:?}"
        );
        assert!(
            has("`orphan`") && has("no class carries this slot"),
            "an uncarried slot is named: {details:?}"
        );
        let bindings = version_bindings(&schema);
        assert!(
            matches!(
                bindings.governing(&["Bench".to_string()], "bare"),
                Some(VersionPin::Malformed)
            ),
            "a defective declaration still binds, as uncheckable"
        );
        assert!(version_bindings(&SchemaDefinition::new("bare")).is_empty());
    }

    #[test]
    fn version_pins_bind_the_sibling_three_ways_and_report_what_they_cannot_check() {
        let schema = parse(
            "id: https://example.org/t\nname: t\nprefixes:\n  t: https://example.org/t/\ndefault_prefix: t\n\
             slots:\n  id: {identifier: true}\n  target: {range: string}\n  pinned:\n    range: string\n    \
             annotations:\n      records_version_of:\n        value:\n          sibling_slot: target\n\
             classes:\n  Bench:\n    tree_root: true\n    slots: [id, target, pinned]\n",
        );
        // `archive` comes first and calls itself `catalog` in its publish
        // manifest; the entry keyed `catalog` must still win by exact key.
        let siblings = vec![
            SiblingVersion {
                entry: "archive".to_string(),
                published: "catalog".to_string(),
                schema_id: None,
                datasets: vec!["shared".to_string()],
                version: "0.9.0".to_string(),
            },
            SiblingVersion {
                entry: "catalog".to_string(),
                published: "wine".to_string(),
                schema_id: Some("https://example.org/catalog".to_string()),
                datasets: vec!["worked-example".to_string(), "shared".to_string()],
                version: "1.1.0".to_string(),
            },
        ];
        let check = |data: &str| {
            let value: serde_norway::Value = serde_norway::from_str(data).expect("yaml");
            let set = crate::instances::InstanceSet::from_linkml_data(&schema, &value);
            version_pins(&set, &version_bindings(&schema), &siblings)
        };
        for name in [
            "catalog",
            "wine",
            "https://example.org/catalog",
            "worked-example",
        ] {
            let agree = check(&format!("id: b\ntarget: {name}\npinned: 1.1.0\n"));
            assert_eq!(
                (agree.pins, agree.mismatched.len(), agree.uncheckable.len()),
                (1, 0, 0),
                "{name} binds and agrees"
            );
        }
        let stale = check("id: b\ntarget: catalog\npinned: 1.0.0\n");
        assert_eq!(stale.pins, 1);
        assert_eq!(
            stale.mismatched[0].message(),
            "record `b`: `pinned` records version 1.0.0 of `catalog`, which is at 1.1.0"
        );
        let unnamed = check("id: b\npinned: 1.1.0\n");
        assert!(
            unnamed.uncheckable[0]
                .message()
                .contains("no `target` value"),
            "{:?}",
            unnamed.uncheckable
        );
        let stranger = check("id: b\ntarget: elsewhere\npinned: 1.1.0\n");
        assert!(
            stranger.uncheckable[0]
                .message()
                .contains("names no resolve_against sibling"),
            "{:?}",
            stranger.uncheckable
        );
        let unpinned = check("id: b\ntarget: catalog\n");
        assert_eq!(
            unpinned,
            VersionVerification::default(),
            "no value at the pinned slot states no pin"
        );

        let tagged = check("id: b\ntarget: catalog\npinned: v1.1.0\n");
        assert_eq!(
            (tagged.pins, tagged.mismatched.len()),
            (1, 0),
            "a tag spelling names the same release"
        );
        let build = check("id: b\ntarget: catalog\npinned: 1.1.0+sha.abc\n");
        assert_eq!(
            (build.pins, build.mismatched.len()),
            (1, 0),
            "build metadata does not change the release"
        );
        let float = check("id: b\ntarget: catalog\npinned: 1.0\n");
        assert!(
            float.uncheckable[0].message().contains("is a number")
                && float.uncheckable[0].message().contains("quote it"),
            "a bare YAML number gets the quoting hint: {:?}",
            float.uncheckable
        );
        // A slot present with no values states no pin — as absent as a
        // missing slot, not "several values".
        let empty_value: serde_norway::Value =
            serde_norway::from_str("id: b\ntarget: catalog\n").expect("yaml");
        let mut set = crate::instances::InstanceSet::from_linkml_data(&schema, &empty_value);
        set.instances[0]
            .slot_values
            .push(crate::instances::SlotValue {
                slot: "pinned".to_string(),
                values: Vec::new(),
            });
        assert_eq!(
            version_pins(&set, &version_bindings(&schema), &siblings),
            VersionVerification::default(),
            "an empty value list at the pinned slot states no pin"
        );
        let prerelease = check("id: b\ntarget: catalog\npinned: 1.1.0-rc.1\n");
        assert_eq!(
            (prerelease.pins, prerelease.mismatched.len()),
            (1, 1),
            "a pre-release is a different release from the final"
        );
        let garbage = check("id: b\ntarget: catalog\npinned: latest\n");
        assert!(
            garbage.uncheckable[0]
                .message()
                .contains("not a semver version"),
            "{:?}",
            garbage.uncheckable
        );
        let boolean = check("id: b\ntarget: catalog\npinned: true\n");
        assert!(
            boolean.uncheckable[0].message().contains("not a string"),
            "{:?}",
            boolean.uncheckable
        );
        assert!(
            !boolean.uncheckable[0].message().contains("is a number"),
            "a boolean is not called a number"
        );
        let ambiguous = check("id: b\ntarget: shared\npinned: 1.1.0\n");
        assert!(
            ambiguous.uncheckable[0]
                .message()
                .contains("names 2 resolve_against siblings"),
            "a name matching two siblings is uncheckable, not first-match: {:?}",
            ambiguous.uncheckable
        );
    }

    /// A malformed absence declaration warns instead of silently
    /// narrowing or widening the check: an unknown field is named, a
    /// `via_slot` no class carries is named (and the binding stays,
    /// reported uncheckable downstream rather than silently widened),
    /// and an annotated slot no class carries is named.
    #[test]
    fn absence_declaration_defects_are_load_warnings() {
        let schema = parse(
            "id: https://example.org/t\nname: t\nslots:\n  unconnected:\n    range: uri\n    \
             annotations:\n      asserts_absence:\n        value:\n          via_slot: ghost\n          \
             joint_referent: true\n  orphan:\n    range: uri\n    annotations:\n      \
             asserts_absence:\n        value: null\nclasses:\n  Bench:\n    slots: \
             [unconnected]\n",
        );
        let issues = absence_declaration_issues(&schema);
        let details: Vec<&str> = issues.iter().map(|i| i.detail.as_str()).collect();
        assert!(
            details.iter().any(|d| d.contains("joint_referent")),
            "the unknown field is named; got: {details:?}"
        );
        assert!(
            details
                .iter()
                .any(|d| d.contains("ghost") && d.contains("no class")),
            "the missing via target is named; got: {details:?}"
        );
        assert!(
            issues
                .iter()
                .any(|i| i.slot == "orphan" && i.detail.contains("no class")),
            "the annotated-but-uncarried slot is named; got: {issues:?}"
        );
        let messages = schema_load_diagnostics(&schema);
        assert!(
            messages.iter().any(|m| m.contains("asserts_absence")),
            "the load diagnostics carry the warnings; got: {messages:?}"
        );
        assert_eq!(
            absence_bindings(&schema).by_slot.keys().collect::<Vec<_>>(),
            vec!["orphan", "unconnected"],
            "defective declarations still bind; the defect is the warning's job"
        );
    }

    /// A slot annotated `expand_against: <slot>` declares that its
    /// scheme-less values expand against the named slot's value on the
    /// same record. Scope follows the absence declarations: top-level
    /// binds schema-wide, a class's own declaration binds that class,
    /// `slot_usage` overrides.
    #[test]
    fn expansion_declarations_read_every_annotated_slot() {
        let schema = parse(
            "id: https://example.org/t\nname: t\nslots:\n  expected_anchors:\n    range: uri\n    \
             multivalued: true\n    annotations:\n      expand_against: target_schema\n  \
             target_schema:\n    range: uri\n  other_base:\n    range: uri\nclasses:\n  \
             Question:\n    slots: [expected_anchors, target_schema, other_base]\n    \
             slot_usage:\n      expected_anchors:\n        annotations:\n          \
             expand_against: other_base\n  Other:\n    slots: [expected_anchors, \
             target_schema]\n",
        );
        let (bindings, issues) = expansion_declarations(&schema);
        assert_eq!(
            bindings.governing(&["Other".to_string()], "expected_anchors"),
            Some(&"target_schema".to_string()),
            "the top-level declaration governs a class without its own"
        );
        assert_eq!(
            bindings.governing(&["Question".to_string()], "expected_anchors"),
            Some(&"other_base".to_string()),
            "slot_usage overrides the top-level declaration for its class"
        );
        assert!(
            issues.iter().all(|i| !i.detail.contains("expand")),
            "well-formed declarations raise no issue; got: {issues:?}"
        );
    }

    /// The contract-schema shape: a
    /// class-ranged anchor slot whose range class exists only to type
    /// anchors into another graph. Expansion is allowed exactly when no
    /// site could still declare a local record of that class — every
    /// slot ranging it is itself declared external — and refused, naming
    /// the blocking site, while one is not.
    #[test]
    fn a_class_ranged_slot_expands_when_its_range_class_has_no_local_home() {
        let base = "id: https://example.org/cqa\nname: cqa\ndefault_range: string\nclasses:\n  \
                    Benchmark:\n    tree_root: true\n    slots: [id, target_schema, questions]\n  \
                    CompetencyQuestionAnswer:\n    slots: [id, target_schema, expected_anchors, \
                    unconnected_anchors]\n  DomainRecord:\n    slots: [id]\nslots:\n  id: \
                    {identifier: true}\n  target_schema: {range: uri}\n  questions: {range: \
                    CompetencyQuestionAnswer, multivalued: true}\n";

        // Only one of the two DomainRecord-ranged slots is declared
        // external: a local DomainRecord is still conceivable through the
        // other, so the declaration is refused naming it.
        let half = parse(&format!(
            "{base}  expected_anchors:\n    range: DomainRecord\n    multivalued: true\n    \
             annotations:\n      expand_against: target_schema\n  unconnected_anchors:\n    \
             range: DomainRecord\n    multivalued: true\n"
        ));
        let (bindings, issues) = expansion_declarations(&half);
        assert!(
            bindings.is_empty(),
            "one unannotated site keeps the class locally declarable"
        );
        assert!(
            issues
                .iter()
                .any(|i| i.detail.contains("unconnected_anchors")),
            "the refusal names the blocking site; got: {issues:?}"
        );

        // Every DomainRecord-ranged slot declared external: no local
        // record can exist, so bare values are unambiguous and both bind.
        let full = parse(&format!(
            "{base}  expected_anchors:\n    range: DomainRecord\n    multivalued: true\n    \
             annotations:\n      expand_against: target_schema\n  unconnected_anchors:\n    \
             range: DomainRecord\n    multivalued: true\n    annotations:\n      \
             expand_against: target_schema\n"
        ));
        let (bindings, issues) = expansion_declarations(&full);
        assert!(
            issues.is_empty(),
            "no local home, no defect; got: {issues:?}"
        );
        assert_eq!(
            bindings.governing(
                &["CompetencyQuestionAnswer".to_string()],
                "expected_anchors"
            ),
            Some(&"target_schema".to_string()),
            "the class-ranged anchor slot binds"
        );

        // A tree_root collection ranging the class is a local home
        // whatever the anchor slots declare.
        let rooted = parse(
            &format!(
                "{base}  records: {{range: DomainRecord, multivalued: true}}\n  \
             expected_anchors:\n    range: DomainRecord\n    multivalued: true\n    \
             annotations:\n      expand_against: target_schema\n  unconnected_anchors:\n    \
             range: DomainRecord\n    multivalued: true\n    annotations:\n      \
             expand_against: target_schema\n"
            )
            .replace(
                "slots: [id, target_schema, questions]",
                "slots: [id, target_schema, questions, records]",
            ),
        );
        let (bindings, issues) = expansion_declarations(&rooted);
        assert!(
            bindings.is_empty(),
            "a collection slot ranging the class keeps it locally declarable"
        );
        assert!(
            issues.iter().any(|i| i.detail.contains("records")),
            "the refusal names the collection site; got: {issues:?}"
        );
    }

    /// Only declarations that actually bind vouch for their sites: one
    /// refused for its own defect leaves its slot an ordinary reference
    /// slot, so a sibling declaration sharing the range class is refused
    /// too rather than binding on a promise nothing keeps.
    #[test]
    fn a_refused_declaration_does_not_vouch_for_its_site() {
        let schema = parse(
            "id: https://example.org/t\nname: t\nclasses:\n  Q:\n    slots: [expected_anchors, \
             unconnected_anchors, target_schema]\n  DomainRecord:\n    slots: [target_schema]\nslots:\n  \
             target_schema: {range: uri}\n  expected_anchors:\n    range: DomainRecord\n    \
             multivalued: true\n    annotations:\n      expand_against: target_schema\n  \
             unconnected_anchors:\n    range: DomainRecord\n    multivalued: true\n    \
             annotations:\n      expand_against: ghost\n",
        );
        let (bindings, issues) = expansion_declarations(&schema);
        assert!(
            bindings.is_empty(),
            "the sibling of a refused declaration is refused too; got: {bindings:?}"
        );
        assert!(
            issues.iter().any(|i| i.detail.contains("ghost")),
            "the ghost base is reported; got: {issues:?}"
        );
        assert!(
            issues
                .iter()
                .any(|i| i.slot == "expected_anchors" && i.detail.contains("unconnected_anchors")),
            "the sibling refusal names the no-longer-external site; got: {issues:?}"
        );
    }

    /// Refusals cascade to a fixpoint: a declaration blocked through one
    /// of its range classes stops vouching for the others, refusing the
    /// declarations that leaned on it.
    #[test]
    fn a_blocked_declaration_stops_vouching_and_the_refusal_cascades() {
        let schema = parse(
            "id: https://example.org/t\nname: t\nclasses:\n  C:\n    slots: [a, b, u, base]\n  \
             Item: {}\n  Widget: {}\nslots:\n  base: {range: uri}\n  a:\n    range: Item\n    \
             multivalued: true\n    annotations:\n      expand_against: base\n  b:\n    \
             multivalued: true\n    any_of:\n      - range: Item\n      - range: Widget\n    \
             annotations:\n      expand_against: base\n  u:\n    range: Widget\n    \
             multivalued: true\n",
        );
        let (bindings, issues) = expansion_declarations(&schema);
        assert!(
            bindings.is_empty(),
            "b is blocked through Widget, so a is blocked through b; got: {bindings:?}"
        );
        assert!(
            issues
                .iter()
                .any(|i| i.slot == "b" && i.detail.contains("`u`")),
            "b's refusal names u; got: {issues:?}"
        );
        assert!(
            issues
                .iter()
                .any(|i| i.slot == "a" && i.detail.contains("`b`")),
            "a's refusal names b, whose refusal un-vouched the Item site; got: {issues:?}"
        );
    }

    /// A class-scoped declaration vouches only for its own class's
    /// site: the same-named slot on another class, unannotated, is a
    /// blocker, not a beneficiary.
    #[test]
    fn a_class_scoped_declaration_vouches_only_for_its_own_class() {
        let schema = parse(
            "id: https://example.org/t\nname: t\nclasses:\n  Q:\n    slots: [anchors, base]\n    \
             slot_usage:\n      anchors:\n        annotations:\n          expand_against: base\n  \
             R:\n    slots: [anchors, base]\n  Item: {}\nslots:\n  base: {range: uri}\n  \
             anchors:\n    range: Item\n    multivalued: true\n",
        );
        let (bindings, issues) = expansion_declarations(&schema);
        assert!(
            bindings.is_empty(),
            "class R's unannotated site blocks Q's declaration; got: {bindings:?}"
        );
        assert!(
            issues.iter().any(|i| i.detail.contains("`R`")),
            "the refusal names the other class's site; got: {issues:?}"
        );
    }

    /// A range class that is itself a `tree_root` has local records as
    /// document roots, with no ranging slot needed — its declarations
    /// are refused outright.
    #[test]
    fn a_tree_root_range_class_is_a_local_home() {
        let schema = parse(
            "id: https://example.org/t\nname: t\nclasses:\n  Node:\n    tree_root: true\n    \
             slots: [id, base_ns, parent]\nslots:\n  id: {identifier: true}\n  base_ns: {range: \
             uri}\n  parent:\n    range: Node\n    annotations:\n      expand_against: base_ns\n",
        );
        let (bindings, issues) = expansion_declarations(&schema);
        assert!(bindings.is_empty(), "got: {bindings:?}");
        assert!(
            issues
                .iter()
                .any(|i| i.slot == "parent" && i.detail.contains("root")),
            "the refusal says the class's records are document roots; got: {issues:?}"
        );
    }

    /// Local records of a class also exist through its `is_a` family: a
    /// site ranging an ancestor can materialize a record of the class by
    /// type designator, so an unannotated ancestor-ranged site blocks.
    #[test]
    fn an_ancestor_ranged_site_blocks_the_descendants_declaration() {
        let schema = parse(
            "id: https://example.org/t\nname: t\nclasses:\n  Benchmark:\n    tree_root: true\n    \
             slots: [id, records, expected_anchors, target_schema]\n  Entity:\n    slots: [id]\n  \
             DomainRecord:\n    is_a: Entity\nslots:\n  id: {identifier: true}\n  target_schema: \
             {range: uri}\n  records:\n    range: Entity\n    multivalued: true\n  \
             expected_anchors:\n    range: DomainRecord\n    multivalued: true\n    \
             annotations:\n      expand_against: target_schema\n",
        );
        let (bindings, issues) = expansion_declarations(&schema);
        assert!(bindings.is_empty(), "got: {bindings:?}");
        assert!(
            issues
                .iter()
                .any(|i| i.slot == "expected_anchors" && i.detail.contains("records")),
            "the ancestor-ranged site is the blocker; got: {issues:?}"
        );
    }

    /// The strict gate's arithmetic is exact: every family contributes
    /// its own count to the total, and the slot-semantics count is the
    /// sum of every declaration family.
    #[test]
    fn strict_blocking_sums_every_family_exactly() {
        let blocking = StrictBlocking {
            unmodeled: 1,
            dangling: 2,
            colliding: 4,
            untyped: 8,
            impossible: 16,
            slot_semantics: 32,
        };
        assert_eq!(blocking.total(), 63, "each family counts once, added");

        // One absence defect (uncarried annotated slot) and one expansion
        // defect (self-reference): the shared category counts both.
        let schema = parse(
            "id: https://example.org/t\nname: t\nclasses:\n  Bench:\n    slots: [anchor, pinned]\nslots:\n  \
             anchor:\n    range: uri\n    annotations:\n      expand_against: anchor\n  \
             pinned:\n    range: string\n    annotations:\n      records_version_of:\n        value:\n          \
             sibling_slot: pinned\n  \
             orphan:\n    range: uri\n    annotations:\n      asserts_absence:\n        value: \
             null\n",
        );
        assert_eq!(
            strict_blocking(&schema).slot_semantics,
            3,
            "one defect from each declaration family, summed"
        );
        let load = schema_load_diagnostics(&schema);
        for family in ["asserts_absence", "expand_against", "records_version_of"] {
            assert!(
                load.iter().any(|m| m.contains(family)),
                "a defective `{family}` declaration is a load warning; got: {load:?}"
            );
        }
    }

    /// The declarability guard judges the *resolved* slot, so a
    /// `slot_usage` that carries only the annotation — its range
    /// inherited from the top-level declaration, the normal LinkML
    /// spelling — is judged by that inherited class range: with another
    /// site keeping the class locally declarable, the declaration is
    /// refused naming it.
    #[test]
    fn the_class_ranged_guard_reads_the_resolved_slot() {
        let schema = parse(
            "id: https://example.org/t\nname: t\nclasses:\n  Bench:\n    slots: [refs, base]\n    \
             slot_usage:\n      refs:\n        annotations:\n          expand_against: base\n  \
             Audit:\n    slots: [held_items]\n  Item: {}\nslots:\n  refs:\n    range: Item\n    \
             multivalued: true\n  held_items:\n    range: Item\n    multivalued: true\n  base:\n    \
             range: uri\n",
        );
        let (bindings, issues) = expansion_declarations(&schema);
        assert!(
            bindings.governing(&["Bench".to_string()], "refs").is_none(),
            "the inherited class range is seen through the resolved view"
        );
        assert!(
            issues
                .iter()
                .any(|i| i.detail.contains("held_items") && i.detail.contains("locally declared")),
            "the refusal names the site keeping the class declarable; got: {issues:?}"
        );
    }

    /// One defective scope drops only itself: a class's bad declaration
    /// cannot disable the schema-wide one governing every other class.
    #[test]
    fn a_defective_scope_drops_only_itself() {
        let schema = parse(
            "id: https://example.org/t\nname: t\nclasses:\n  Q:\n    slots: [expected_anchors, \
             target_schema]\n    slot_usage:\n      expected_anchors:\n        annotations:\n          \
             expand_against: ghost\n  R:\n    slots: [expected_anchors, target_schema]\nslots:\n  \
             expected_anchors:\n    range: uri\n    multivalued: true\n    annotations:\n      \
             expand_against: target_schema\n  target_schema:\n    range: uri\n",
        );
        let (bindings, issues) = expansion_declarations(&schema);
        assert_eq!(
            bindings.governing(&["R".to_string()], "expected_anchors"),
            Some(&"target_schema".to_string()),
            "the healthy schema-wide declaration still governs other classes"
        );
        assert!(
            issues.iter().any(|i| i.detail.contains("ghost")),
            "the defective scope is reported; got: {issues:?}"
        );
    }

    /// A self-referential declaration and one on a slot no class
    /// carries are both refused: neither can ever expand anything.
    #[test]
    fn self_referential_and_uncarried_declarations_are_refused() {
        let schema = parse(
            "id: https://example.org/t\nname: t\nclasses:\n  Bench:\n    slots: [anchor]\nslots:\n  \
             anchor:\n    range: uri\n    annotations:\n      expand_against: anchor\n  \
             orphan:\n    range: uri\n    annotations:\n      expand_against: anchor\n",
        );
        let (bindings, issues) = expansion_declarations(&schema);
        assert!(bindings.is_empty(), "neither declaration binds");
        assert!(
            issues.iter().any(|i| i.detail.contains("itself")),
            "self-reference is named; got: {issues:?}"
        );
        assert!(
            issues
                .iter()
                .any(|i| i.slot == "orphan" && i.detail.contains("no class carries")),
            "the uncarried slot is named; got: {issues:?}"
        );
    }

    /// The static base check follows containment: a base carried only
    /// by a class no containment path from the governed class reaches
    /// can never be supplied, while the same base on a containing
    /// class binds.
    #[test]
    fn an_unreachable_base_is_refused_at_load() {
        let unreachable = parse(
            "id: https://example.org/t\nname: t\nclasses:\n  Bench:\n    tree_root: true\n    \
             slots: [id, questions]\n  Question:\n    slots: [id, expected_anchors]\n  \
             Elsewhere:\n    slots: [far_base]\n  DomainRecord:\n    slots: [id]\nslots:\n  \
             id: {identifier: true}\n  questions: {range: Question, multivalued: true}\n  \
             far_base: {range: uri}\n  expected_anchors:\n    range: DomainRecord\n    \
             multivalued: true\n    annotations:\n      expand_against: far_base\n",
        );
        let (bindings, issues) = expansion_declarations(&unreachable);
        assert!(bindings.is_empty(), "an unreachable base does not bind");
        assert!(
            issues.iter().any(|i| i.slot == "expected_anchors"
                && i.detail.contains("no containment walk can supply it")),
            "the unreachable base is refused; got: {issues:?}"
        );

        let reachable = parse(
            "id: https://example.org/t\nname: t\nclasses:\n  Bench:\n    tree_root: true\n    \
             slots: [id, far_base, questions]\n  Question:\n    slots: [id, expected_anchors]\n  \
             DomainRecord:\n    slots: [id]\nslots:\n  id: {identifier: true}\n  questions: \
             {range: Question, multivalued: true}\n  far_base: {range: uri}\n  \
             expected_anchors:\n    range: DomainRecord\n    multivalued: true\n    \
             annotations:\n      expand_against: far_base\n",
        );
        let (bindings, issues) = expansion_declarations(&reachable);
        assert!(
            bindings
                .governing(&["Question".to_string()], "expected_anchors")
                .is_some(),
            "a base on the containing class binds; got: {issues:?}"
        );
    }

    /// A base that itself declares `expand_against` is refused: a base
    /// is read as authored, so expansions cannot chain through it.
    #[test]
    fn a_base_that_itself_expands_is_refused() {
        let schema = parse(
            "id: https://example.org/t\nname: t\nclasses:\n  Bench:\n    slots: [id, \
             target_schema, other_base, expected_anchors]\nslots:\n  id: {identifier: true}\n  \
             other_base: {range: uri}\n  target_schema:\n    range: uri\n    annotations:\n      \
             expand_against: other_base\n  expected_anchors:\n    range: uri\n    multivalued: \
             true\n    annotations:\n      expand_against: target_schema\n",
        );
        let (bindings, issues) = expansion_declarations(&schema);
        assert!(
            issues
                .iter()
                .any(|i| i.slot == "expected_anchors" && i.detail.contains("cannot chain")),
            "the chained declaration is refused; got: {issues:?}"
        );
        assert!(
            bindings
                .governing(&["Bench".to_string()], "target_schema")
                .is_some(),
            "the base's own declaration, whose base is plain, still binds"
        );
    }

    /// A base that resolves class-ranged everywhere it could be
    /// supplied is refused: its values are references, not a namespace
    /// string.
    #[test]
    fn a_class_ranged_base_is_refused() {
        let schema = parse(
            "id: https://example.org/t\nname: t\nclasses:\n  Bench:\n    slots: [id, primary, \
             expected_anchors]\n  DomainRecord:\n    slots: [id]\nslots:\n  id: {identifier: \
             true}\n  primary: {range: DomainRecord}\n  expected_anchors:\n    range: uri\n    \
             multivalued: true\n    annotations:\n      expand_against: primary\n",
        );
        let (bindings, issues) = expansion_declarations(&schema);
        assert!(bindings.is_empty(), "a class-ranged base does not bind");
        assert!(
            issues.iter().any(|i| i.slot == "expected_anchors"
                && i.detail.contains("class-ranged")
                && i.detail.contains("references")),
            "the class-ranged base is refused with its reason; got: {issues:?}"
        );
    }

    /// A defective `expand_against` declaration warns and does not
    /// expand: a class-ranged slot's bare values are in-dataset
    /// references, a base slot no class carries can never supply a
    /// base, and a non-string annotation value names no slot.
    #[test]
    fn expansion_declaration_defects_warn_and_do_not_bind() {
        let schema = parse(
            "id: https://example.org/t\nname: t\nclasses:\n  Bench:\n    slots: [refs, \
             held_items, anchors, loose, base]\n  Item: {}\nslots:\n  refs:\n    range: Item\n    \
             multivalued: true\n    annotations:\n      expand_against: base\n  held_items:\n    \
             range: Item\n    multivalued: true\n  anchors:\n    range: uri\n    \
             annotations:\n      expand_against: ghost\n  loose:\n    range: uri\n    \
             annotations:\n      expand_against:\n        value:\n          x: y\n  base:\n    \
             range: uri\n",
        );
        let (bindings, issues) = expansion_declarations(&schema);
        assert!(
            bindings.governing(&["Bench".to_string()], "refs").is_none(),
            "a class-ranged slot does not bind"
        );
        assert!(
            bindings
                .governing(&["Bench".to_string()], "anchors")
                .is_none(),
            "an uncarried base slot does not bind"
        );
        assert!(
            bindings
                .governing(&["Bench".to_string()], "loose")
                .is_none(),
            "a non-string value does not bind"
        );
        let details: Vec<&str> = issues.iter().map(|i| i.detail.as_str()).collect();
        assert!(
            details
                .iter()
                .any(|d| d.contains("locally declared") && d.contains("held_items")),
            "the still-declarable range class is named; got: {details:?}"
        );
        assert!(
            details.iter().any(|d| d.contains("ghost")),
            "the uncarried base slot is named; got: {details:?}"
        );
        assert!(
            details.iter().any(|d| d.contains("string")),
            "the non-string value is named; got: {details:?}"
        );
        let messages = schema_load_diagnostics(&schema);
        assert!(
            messages.iter().any(|m| m.contains("expand_against")),
            "the load diagnostics carry the warnings; got: {messages:?}"
        );
    }

    /// A class-scoped declaration (attribute or `slot_usage`) binds only
    /// records of that class: another class's same-named slot holds
    /// ordinary data, not absence claims.
    #[test]
    fn a_class_scoped_declaration_does_not_claim_other_classes_data() {
        let sibling_schema = linked_sibling_schema();
        let sibling = scoped_set(&sibling_schema, LINKED_DATA);
        // `Bench` (the tree_root) carries plain `unconnected` references;
        // an unrelated class `Audit` declares the same-named attribute
        // with the assertion.
        let mut schema = claiming_schema();
        let mut audit = crate::linkml::ClassDefinition::new("Audit");
        let mut marked = crate::linkml::SlotDefinition::new("unconnected");
        marked.range = Some("uri".to_string());
        marked.multivalued = true;
        marked
            .annotations
            .insert_raw("asserts_absence", serde_norway::Value::Null);
        audit.attributes.insert("unconnected".to_string(), marked);
        schema.classes.insert("Audit".to_string(), audit);

        let set = scoped_set(
            &schema,
            "id: b1\nunconnected:\n  - cellar:w1\n  - cellar:f1\n",
        );
        let declarations = absence_bindings(&schema);
        let found = unverified_absences(
            &schema,
            &set,
            &declarations,
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(
            (
                found.claims,
                found.unverified.len(),
                found.uncheckable.len()
            ),
            (0, 0, 0),
            "a Bench record's values are not Audit's claims; got unverified: {:?}",
            found.unverified
        );
    }

    /// `slot_usage` overrides the top-level declaration for its class,
    /// as it does for every other LinkML slot property: the class's
    /// records use the narrowed `via_slot`, so a joining record of an
    /// excluded class does not contradict the claim.
    #[test]
    fn slot_usage_narrowing_overrides_the_top_level_declaration() {
        let sibling_schema = linked_sibling_schema();
        let sibling = scoped_set(&sibling_schema, LINKED_DATA);
        let mut schema = claiming_schema();
        // Top-level: bare assertion. The claiming class narrows to
        // records of the class named by `via` — LINKED_DATA's joining
        // record `l1` is a `Link`, so narrowing to a class that is not
        // `Link` excludes it.
        let bench = schema.classes.get_mut("Bench").unwrap();
        let unconnected = bench.attributes.get_mut("unconnected").unwrap();
        unconnected
            .annotations
            .insert_raw("asserts_absence", serde_norway::Value::Null);
        let mut narrowed = crate::linkml::SlotDefinition::new("unconnected");
        let mut via_body = serde_norway::Mapping::new();
        via_body.insert("via_slot".into(), "via".into());
        narrowed
            .annotations
            .insert_raw("asserts_absence", serde_norway::Value::Mapping(via_body));
        bench.slot_usage.insert("unconnected".to_string(), narrowed);

        // The record narrows to the sibling's `Item` class: `l1` (a
        // `Link`) joining the anchors is outside the narrowed claim.
        let set = scoped_set(
            &schema,
            "id: b1\nvia: https://example.org/cellar/Item\nunconnected:\n  - cellar:w1\n  - cellar:f1\n",
        );
        let declarations = absence_bindings(&schema);
        let found = unverified_absences(
            &schema,
            &set,
            &declarations,
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(found.claims, 1);
        assert!(
            found.unverified.is_empty(),
            "the slot_usage narrowing governs, so the Link join is outside the claim; got: {:?}",
            found.unverified
        );
    }

    /// A malformed `via_slot` (non-string) poisons the binding: its
    /// claims are uncheckable, never evaluated wide of the author's
    /// declaration — the same rule as a `via_slot` no class carries.
    #[test]
    fn a_malformed_via_slot_makes_claims_uncheckable_not_wider() {
        let sibling_schema = linked_sibling_schema();
        let sibling = scoped_set(&sibling_schema, LINKED_DATA);
        let mut schema = claiming_schema();
        let bench = schema.classes.get_mut("Bench").unwrap();
        let unconnected = bench.attributes.get_mut("unconnected").unwrap();
        let mut body = serde_norway::Mapping::new();
        body.insert("via_slot".into(), serde_norway::Value::Number(42.into()));
        unconnected
            .annotations
            .insert_raw("asserts_absence", serde_norway::Value::Mapping(body));

        let set = scoped_set(
            &schema,
            "id: b1\nunconnected:\n  - cellar:w1\n  - cellar:f1\n",
        );
        let declarations = absence_bindings(&schema);
        let found = unverified_absences(
            &schema,
            &set,
            &declarations,
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(
            found.claims, 0,
            "an uncheckable claim is not counted as evaluated"
        );
        assert_eq!(
            (found.unverified.len(), found.uncheckable.len()),
            (0, 1),
            "the poisoned binding is uncheckable, not widened; got unverified: {:?}",
            found.unverified
        );
    }

    /// A binding whose `via_slot` no class carries narrows to a class no
    /// record can name: the claims it governs are reported uncheckable —
    /// never silently widened to "no record of any kind", which is a
    /// strictly stronger check than the author declared.
    #[test]
    fn an_uncarried_via_slot_makes_claims_uncheckable_not_wider() {
        let sibling_schema = linked_sibling_schema();
        let sibling = scoped_set(&sibling_schema, LINKED_DATA);
        let schema = claiming_schema();
        let set = scoped_set(
            &schema,
            "id: b1\nunconnected:\n  - cellar:w1\n  - cellar:f1\n",
        );
        let found = unverified_absences(
            &schema,
            &set,
            &AbsenceBindings::schema_wide("unconnected", Some("ghost")),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(
            found.claims, 0,
            "an uncheckable claim is not counted as evaluated"
        );
        assert_eq!(
            found.uncheckable.len(),
            1,
            "the narrowed claim is uncheckable; got unverified: {:?}, uncheckable: {:?}",
            found.unverified,
            found.uncheckable
        );
        assert!(
            found.unverified.is_empty(),
            "an uncheckable claim is not evaluated wide; got: {:?}",
            found.unverified
        );
    }

    /// The claiming side: `anchors` carries the record's full anchor set,
    /// `unconnected` the absence claim (a subset of it), `unconnected_iris`
    /// the same claim as IRI-valued scalars, `via` the optional class
    /// narrowing.
    fn claiming_schema() -> SchemaDefinition {
        let mut schema = benchmark_schema();
        let bench = schema.classes.get_mut("Bench").unwrap();
        let mut via = crate::linkml::SlotDefinition::new("via");
        via.range = Some("uri".to_string());
        bench.attributes.insert("via".to_string(), via);
        let mut unconnected = crate::linkml::SlotDefinition::new("unconnected");
        unconnected.range = Some("DomainRecord".to_string());
        unconnected.multivalued = true;
        bench
            .attributes
            .insert("unconnected".to_string(), unconnected);
        let mut unconnected_iris = crate::linkml::SlotDefinition::new("unconnected_iris");
        unconnected_iris.range = Some("uri".to_string());
        unconnected_iris.multivalued = true;
        bench
            .attributes
            .insert("unconnected_iris".to_string(), unconnected_iris);
        schema
    }

    const LINKED_DATA: &str =
        "id: est\nitems:\n  - {id: w1}\n  - {id: f1}\nlinks:\n  - {id: l1, a: w1, b: f1}\n";

    #[test]
    fn a_joined_absence_claim_is_reported() {
        let sibling_schema = linked_sibling_schema();
        let sibling = scoped_set(&sibling_schema, LINKED_DATA);
        let schema = claiming_schema();
        let set = scoped_set(&schema, "id: b1\nanchors:\n  - cellar:w1\n  - cellar:f1\n");
        let found = unverified_absences(
            &schema,
            &set,
            &AbsenceBindings::schema_wide("anchors", None),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(found.claims, 1);
        assert_eq!(
            found.unverified.len(),
            1,
            "l1 joins the anchors; got: {found:?}"
        );
        assert_eq!(found.unverified[0].referrer, "b1");
        assert_eq!(found.unverified[0].joined_by, "l1");
        assert!(
            found.unverified[0].message().contains("does not hold"),
            "got: {}",
            found.unverified[0].message()
        );
    }

    #[test]
    fn a_container_holding_the_anchors_does_not_join_them() {
        // The root references everything it holds — w1 and l1 included —
        // but holding is not joining, per the claim's own semantics.
        let sibling_schema = linked_sibling_schema();
        let sibling = scoped_set(&sibling_schema, LINKED_DATA);
        let schema = claiming_schema();
        let set = scoped_set(&schema, "id: b1\nanchors:\n  - cellar:w1\n  - cellar:l1\n");
        assert_eq!(
            unverified_absences(
                &schema,
                &set,
                &AbsenceBindings::schema_wide("anchors", None),
                &[(&sibling_schema, std::slice::from_ref(&sibling))],
            )
            .unverified,
            vec![],
            "only the exempt container references both"
        );
    }

    #[test]
    fn via_narrows_the_claim_to_one_joining_class() {
        let sibling_schema = linked_sibling_schema();
        let sibling = scoped_set(&sibling_schema, LINKED_DATA);
        let schema = claiming_schema();
        let joined_via_link = scoped_set(
            &schema,
            "id: b1\nanchors:\n  - cellar:w1\n  - cellar:f1\nvia: https://example.org/cellar/Link\n",
        );
        assert_eq!(
            unverified_absences(
                &schema,
                &joined_via_link,
                &AbsenceBindings::schema_wide("anchors", Some("via")),
                &[(&sibling_schema, std::slice::from_ref(&sibling))],
            )
            .unverified
            .len(),
            1,
            "a Link joins them, and Links are the claimed kind"
        );
        let narrowed_elsewhere = scoped_set(
            &schema,
            "id: b1\nanchors:\n  - cellar:w1\n  - cellar:f1\nvia: https://example.org/cellar/Item\n",
        );
        assert_eq!(
            unverified_absences(
                &schema,
                &narrowed_elsewhere,
                &AbsenceBindings::schema_wide("anchors", Some("via")),
                &[(&sibling_schema, std::slice::from_ref(&sibling))],
            )
            .unverified,
            vec![],
            "no Item joins the anchors, so the narrowed claim holds"
        );

        let named_via = scoped_set(
            &schema,
            "id: b1\nanchors:\n  - cellar:w1\n  - cellar:f1\nvia: Link\n",
        );
        assert_eq!(
            unverified_absences(
                &schema,
                &named_via,
                &AbsenceBindings::schema_wide("anchors", Some("via")),
                &[(&sibling_schema, std::slice::from_ref(&sibling))],
            )
            .unverified
            .len(),
            1,
            "a bare class name narrows like the designator's matcher: name first, IRI second"
        );
    }

    /// A subset absence claim: only the bound slot's values form the
    /// claim; the record's wider anchor set never enters it.
    #[test]
    fn a_subset_absence_claim_is_verified_over_only_its_own_anchors() {
        let sibling_schema = linked_sibling_schema();
        let uncovered_pair_joined = scoped_set(
            &sibling_schema,
            "id: est\nitems:\n  - {id: w1}\n  - {id: f1}\n  - {id: g1}\n\
             links:\n  - {id: l1, a: f1, b: g1}\n",
        );
        let schema = claiming_schema();
        let set = scoped_set(
            &schema,
            "id: b1\nanchors:\n  - cellar:w1\n  - cellar:f1\n  - cellar:g1\n\
             unconnected:\n  - cellar:w1\n  - cellar:f1\n",
        );
        let holds = unverified_absences(
            &schema,
            &set,
            &AbsenceBindings::schema_wide("unconnected", None),
            &[(
                &sibling_schema,
                std::slice::from_ref(&uncovered_pair_joined),
            )],
        );
        assert_eq!(holds.claims, 1);
        assert_eq!(
            holds.unverified,
            vec![],
            "l1 joins f1–g1, a pair the claim does not cover"
        );

        let claimed_pair_joined = scoped_set(
            &sibling_schema,
            "id: est\nitems:\n  - {id: w1}\n  - {id: f1}\n  - {id: g1}\n\
             links:\n  - {id: l1, a: f1, b: g1}\n  - {id: l2, a: w1, b: f1}\n",
        );
        let contradicted = unverified_absences(
            &schema,
            &set,
            &AbsenceBindings::schema_wide("unconnected", None),
            &[(&sibling_schema, std::slice::from_ref(&claimed_pair_joined))],
        );
        assert_eq!(
            contradicted.unverified.len(),
            1,
            "l2 joins exactly the claimed pair; got: {contradicted:?}"
        );
        assert_eq!(contradicted.unverified[0].joined_by, "l2");
    }

    /// A CURIE `via` expands against the referring schema exactly like the
    /// anchors beside it, so the natural spelling narrows correctly.
    #[test]
    fn a_curie_via_expands_like_the_anchors() {
        let sibling_schema = linked_sibling_schema();
        let sibling = scoped_set(&sibling_schema, LINKED_DATA);
        let schema = claiming_schema();
        let set = scoped_set(
            &schema,
            "id: b1\nanchors:\n  - cellar:w1\n  - cellar:f1\nvia: cellar:Link\n",
        );
        let found = unverified_absences(
            &schema,
            &set,
            &AbsenceBindings::schema_wide("anchors", Some("via")),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(
            found.unverified.len(),
            1,
            "cellar:Link denotes the Link class; got: {found:?}"
        );

        // The prefix is the claiming schema's vocabulary: a sibling
        // minting the same namespace under its own prefix name still
        // answers to the claimer's spelling.
        let mut renamed_sibling = linked_sibling_schema();
        renamed_sibling.default_prefix = Some("store".to_string());
        renamed_sibling.prefixes.remove("cellar");
        renamed_sibling.prefixes.insert(
            "store".to_string(),
            "https://example.org/cellar/".to_string(),
        );
        let sibling = scoped_set(&renamed_sibling, LINKED_DATA);
        let found = unverified_absences(
            &schema,
            &set,
            &AbsenceBindings::schema_wide("anchors", Some("via")),
            &[(&renamed_sibling, std::slice::from_ref(&sibling))],
        );
        assert_eq!(
            found.unverified.len(),
            1,
            "the CURIE expands against the claiming schema, like the anchors beside it; \
             got: {found:?}"
        );
    }

    /// A multivalued `via`, a `via` naming no sibling class, and a
    /// malformed `via` value are each uncheckable — said so, never holding.
    #[test]
    fn uncheckable_via_claims_are_reported_never_holding() {
        let sibling_schema = linked_sibling_schema();
        let sibling = scoped_set(&sibling_schema, LINKED_DATA);
        let schema = claiming_schema();

        let set = scoped_set(
            &schema,
            "id: b1\nanchors:\n  - cellar:f1\nvia: ['cellar:Link', 'cellar:Item']\n",
        );
        let found = unverified_absences(
            &schema,
            &set,
            &AbsenceBindings::schema_wide("anchors", Some("via")),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(found.claims, 0);
        assert_eq!(
            found.unverified,
            vec![],
            "no first-value narrowing may be evaluated"
        );
        assert_eq!(found.uncheckable.len(), 1, "got: {found:?}");
        assert!(
            found.uncheckable[0].message().contains("one `via` value"),
            "got: {}",
            found.uncheckable[0].message()
        );

        let set = scoped_set(
            &schema,
            "id: b1\nanchors:\n  - cellar:w1\n  - cellar:f1\nvia: cellar:Pairings\n",
        );
        let found = unverified_absences(
            &schema,
            &set,
            &AbsenceBindings::schema_wide("anchors", Some("via")),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(found.claims, 0);
        assert_eq!(found.uncheckable.len(), 1, "got: {found:?}");
        assert!(
            found.uncheckable[0].message().contains("names no class"),
            "got: {}",
            found.uncheckable[0].message()
        );

        let schema = query_claiming_schema();
        let bad_via = scoped_set(
            &schema,
            "id: b1\nqueries:\n  - {id: q1, anchors: ['cellar:f1'], via: {oops: 1}}\n",
        );
        let found = unverified_absences(
            &schema,
            &bad_via,
            &AbsenceBindings::schema_wide("anchors", Some("via")),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(found.claims, 0);
        assert_eq!(
            found.unverified,
            vec![],
            "the unnarrowed claim must not be evaluated in the narrowed one's place"
        );
        assert_eq!(found.uncheckable.len(), 1, "got: {found:?}");
        assert!(
            found.uncheckable[0]
                .message()
                .contains("an object, not a class"),
            "got: {}",
            found.uncheckable[0].message()
        );
    }

    /// Anchors that collapse to fewer distinct IRIs, resolve to no sibling
    /// record, or load as null or malformed are each uncheckable.
    #[test]
    fn uncheckable_anchor_claims_are_reported_never_holding() {
        let sibling_schema = linked_sibling_schema();
        let sibling = scoped_set(&sibling_schema, LINKED_DATA);
        let schema = claiming_schema();

        let set = scoped_set(
            &schema,
            "id: b1\nanchors:\n  - cellar:w1\n  - https://example.org/cellar/w1\n",
        );
        let found = unverified_absences(
            &schema,
            &set,
            &AbsenceBindings::schema_wide("anchors", None),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(found.unverified, vec![], "no false contradiction");
        assert_eq!(found.uncheckable.len(), 1, "got: {found:?}");
        assert!(
            found.uncheckable[0]
                .message()
                .contains("fewer distinct IRIs"),
            "got: {}",
            found.uncheckable[0].message()
        );

        let set = scoped_set(
            &schema,
            "id: b1\nanchors:\n  - cellar:w1\n  - https://example.org/cellar/w1\n  - cellar:f1\n",
        );
        let found = unverified_absences(
            &schema,
            &set,
            &AbsenceBindings::schema_wide("anchors", None),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(found.claims, 0);
        assert_eq!(
            found.unverified,
            vec![],
            "the collapsed pair must not be checked in the triple's place"
        );
        assert_eq!(found.uncheckable.len(), 1, "got: {found:?}");

        let set = scoped_set(
            &schema,
            "id: b1\nanchors:\n  - cellar:w1\n  - cellar:ghost\n",
        );
        let found = unverified_absences(
            &schema,
            &set,
            &AbsenceBindings::schema_wide("anchors", None),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(found.claims, 0);
        assert_eq!(found.uncheckable.len(), 1, "got: {found:?}");
        assert!(
            found.uncheckable[0]
                .message()
                .contains("resolves to no sibling record"),
            "got: {}",
            found.uncheckable[0].message()
        );

        let schema = query_claiming_schema();
        let set = scoped_set(
            &schema,
            "id: b1\nqueries:\n  - {id: q1, anchors: ['cellar:w1', ~]}\n",
        );
        let found = unverified_absences(
            &schema,
            &set,
            &AbsenceBindings::schema_wide("anchors", None),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(found.claims, 0);
        assert_eq!(
            found.unverified,
            vec![],
            "the surviving anchor must not be checked alone"
        );
        assert_eq!(found.uncheckable.len(), 1, "got: {found:?}");
        assert!(
            found.uncheckable[0].message().contains("a null"),
            "got: {}",
            found.uncheckable[0].message()
        );

        let bad_anchor = scoped_set(
            &schema,
            "id: b1\nqueries:\n  - {id: q1, anchors: ['cellar:w1', 42]}\n",
        );
        let found = unverified_absences(
            &schema,
            &bad_anchor,
            &AbsenceBindings::schema_wide("anchors", None),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(found.claims, 0);
        assert_eq!(found.unverified, vec![], "no claim to contradict");
        assert_eq!(found.uncheckable.len(), 1, "got: {found:?}");
        assert!(
            found.uncheckable[0]
                .message()
                .contains("a number, not a reference or IRI"),
            "got: {}",
            found.uncheckable[0].message()
        );
    }

    /// Anchors authored as IRI-valued scalars (a `uri`-ranged slot) form
    /// the same claim reference-valued anchors do.
    #[test]
    fn scalar_iri_anchors_form_a_checkable_claim() {
        let sibling_schema = linked_sibling_schema();
        let sibling = scoped_set(&sibling_schema, LINKED_DATA);
        let schema = claiming_schema();
        let set = scoped_set(
            &schema,
            "id: b1\nunconnected_iris:\n  - https://example.org/cellar/w1\n  - https://example.org/cellar/f1\n",
        );
        let found = unverified_absences(
            &schema,
            &set,
            &AbsenceBindings::schema_wide("unconnected_iris", None),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(found.claims, 1);
        assert_eq!(found.unverified.len(), 1, "l1 joins them; got: {found:?}");
    }

    /// The container exemption is by identity: a record of the root class
    /// that is not the container still counts as a joining record.
    #[test]
    fn a_nested_root_class_record_still_joins() {
        let mut sibling_schema = linked_sibling_schema();
        let root = sibling_schema.classes.get_mut("Root").unwrap();
        let mut subroots = crate::linkml::SlotDefinition::new("subroots");
        subroots.range = Some("Root".to_string());
        subroots.multivalued = true;
        root.attributes.insert("subroots".to_string(), subroots);
        for end in ["partner_a", "partner_b"] {
            let mut slot = crate::linkml::SlotDefinition::new(end);
            slot.range = Some("Item".to_string());
            root.attributes.insert(end.to_string(), slot.clone());
        }
        let sibling = scoped_set(
            &sibling_schema,
            "id: est\nitems:\n  - {id: w1}\n  - {id: f1}\n\
             subroots:\n  - {id: sub1, partner_a: w1, partner_b: f1}\n",
        );
        let schema = claiming_schema();
        let set = scoped_set(&schema, "id: b1\nanchors:\n  - cellar:w1\n  - cellar:f1\n");
        let found = unverified_absences(
            &schema,
            &set,
            &AbsenceBindings::schema_wide("anchors", None),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(
            found.unverified.len(),
            1,
            "sub1 is root-classed but not the container; got: {found:?}"
        );
        assert_eq!(found.unverified[0].joined_by, "sub1");
    }

    /// One claim contradicted in two sibling graphs is one contradicted
    /// claim with two contradictions — the counts stay distinct.
    #[test]
    fn a_claim_contradicted_in_two_graphs_counts_once() {
        let sibling_schema = linked_sibling_schema();
        let first = scoped_set(&sibling_schema, LINKED_DATA);
        let second = scoped_set(&sibling_schema, LINKED_DATA);
        let schema = claiming_schema();
        let set = scoped_set(&schema, "id: b1\nanchors:\n  - cellar:w1\n  - cellar:f1\n");
        let found = unverified_absences(
            &schema,
            &set,
            &AbsenceBindings::schema_wide("anchors", None),
            &[
                (&sibling_schema, std::slice::from_ref(&first)),
                (&sibling_schema, std::slice::from_ref(&second)),
            ],
        );
        assert_eq!(found.claims, 1);
        assert_eq!(found.contradicted_claims, 1);
        assert_eq!(found.unverified.len(), 2, "one per joining record");
    }

    #[test]
    fn single_anchor_claims_are_checked_plain_and_via_narrowed() {
        let sibling_schema = linked_sibling_schema();
        let sibling = scoped_set(&sibling_schema, LINKED_DATA);
        let schema = claiming_schema();

        let contradicted = scoped_set(&schema, "id: b1\nanchors:\n  - cellar:w1\n");
        let found = unverified_absences(
            &schema,
            &contradicted,
            &AbsenceBindings::schema_wide("anchors", None),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(found.claims, 1, "one anchor is a checkable claim");
        assert_eq!(
            found.unverified.len(),
            1,
            "l1 references w1, so the claim is false; got: {found:?}"
        );
        assert!(
            found.unverified[0].message().contains("references it"),
            "the single-anchor message reads as a reference claim; got: {}",
            found.unverified[0].message()
        );

        let holds = scoped_set(&schema, "id: b1\nanchors:\n  - cellar:l1\n");
        let found = unverified_absences(
            &schema,
            &holds,
            &AbsenceBindings::schema_wide("anchors", None),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(found.claims, 1);
        assert_eq!(
            found.unverified,
            vec![],
            "only the exempt container references l1, so the claim holds"
        );

        let set = scoped_set(
            &schema,
            "id: b1\nanchors:\n  - cellar:f1\nvia: cellar:Link\n",
        );
        let found = unverified_absences(
            &schema,
            &set,
            &AbsenceBindings::schema_wide("anchors", Some("via")),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(found.claims, 1);
        assert_eq!(
            found.unverified.len(),
            1,
            "l1 is a Link referencing f1; got: {found:?}"
        );
        assert_eq!(found.unverified[0].joined_by, "l1");
        assert!(
            found.unverified[0]
                .message()
                .contains("`cellar:Link` record"),
            "a narrowed claim is reported as narrowed, not as the broader one; got: {}",
            found.unverified[0].message()
        );
    }

    /// `linked_sibling_schema` plus a `Shelf` class whose `held` slot
    /// contains `Item`s, so containment can occur below the root.
    fn shelved_sibling_schema() -> SchemaDefinition {
        let mut schema = linked_sibling_schema();
        let mut id = crate::linkml::SlotDefinition::new("id");
        id.identifier = true;
        let mut shelf = crate::linkml::ClassDefinition::new("Shelf");
        shelf.attributes.insert("id".to_string(), id);
        let mut held = crate::linkml::SlotDefinition::new("held");
        held.range = Some("Item".to_string());
        held.multivalued = true;
        shelf.attributes.insert("held".to_string(), held);
        schema.classes.insert("Shelf".to_string(), shelf);
        let root = schema.classes.get_mut("Root").unwrap();
        let mut shelves = crate::linkml::SlotDefinition::new("shelves");
        shelves.range = Some("Shelf".to_string());
        shelves.multivalued = true;
        root.attributes.insert("shelves".to_string(), shelves);
        schema
    }

    /// Holding a record — nested or inline — is not joining it; citing it
    /// by id or restating a declared record inline is.
    #[test]
    fn containment_never_joins_and_citations_always_do() {
        let sibling_schema = shelved_sibling_schema();

        // s1 inlines w1 and references f1 by id, under the same slot.
        let sibling = scoped_set(
            &sibling_schema,
            "id: est\nitems:\n  - {id: f1}\nshelves:\n  - {id: s1, held: [{id: w1}, f1]}\n",
        );
        let schema = claiming_schema();

        let held_only = scoped_set(&schema, "id: b1\nanchors:\n  - cellar:w1\n");
        let found = unverified_absences(
            &schema,
            &held_only,
            &AbsenceBindings::schema_wide("anchors", None),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(found.claims, 1);
        assert_eq!(
            found.unverified,
            vec![],
            "s1 holds w1 inline, and holding is not joining; got: {found:?}"
        );

        let referenced = scoped_set(&schema, "id: b1\nanchors:\n  - cellar:f1\n");
        let found = unverified_absences(
            &schema,
            &referenced,
            &AbsenceBindings::schema_wide("anchors", None),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(
            found.unverified.len(),
            1,
            "s1 references f1 by id, which does join; got: {found:?}"
        );
        assert_eq!(found.unverified[0].joined_by, "s1");

        let sibling = scoped_set(
            &sibling_schema,
            "id: est\nshelves:\n  - {id: s1, held: [{id: w1}, w1]}\n",
        );
        let set = scoped_set(&schema, "id: b1\nanchors:\n  - cellar:w1\n");
        let found = unverified_absences(
            &schema,
            &set,
            &AbsenceBindings::schema_wide("anchors", None),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(
            found.unverified.len(),
            1,
            "s1 also cites w1 by id; got: {found:?}"
        );
        assert_eq!(found.unverified[0].joined_by, "s1");

        let mut sibling_schema = linked_sibling_schema();
        let mut cites = crate::linkml::SlotDefinition::new("cites");
        cites.range = Some("Item".to_string());
        cites.multivalued = true;
        sibling_schema
            .classes
            .get_mut("Link")
            .unwrap()
            .attributes
            .insert("cites".to_string(), cites);
        let sibling = scoped_set(
            &sibling_schema,
            "id: est\nitems:\n  - {id: w1}\n  - {id: f1}\n\
             links:\n  - {id: l2, cites: [{id: w1}, {id: f1}]}\n",
        );
        let schema = claiming_schema();
        let set = scoped_set(&schema, "id: b1\nanchors:\n  - cellar:w1\n  - cellar:f1\n");
        let found = unverified_absences(
            &schema,
            &set,
            &AbsenceBindings::schema_wide("anchors", None),
            &[(&sibling_schema, std::slice::from_ref(&sibling))],
        );
        assert_eq!(
            found.unverified.len(),
            1,
            "l2 joins the records it restates inline; got: {found:?}"
        );
        assert_eq!(found.unverified[0].joined_by, "l2");
    }

    /// `claiming_schema` plus a nested `Query` class carrying its own
    /// `anchors`/`via`, so claims can sit below the dataset root — where
    /// a value that fits no slot kind loads as unusable rather than as a
    /// container-slot reference.
    fn query_claiming_schema() -> SchemaDefinition {
        let mut schema = claiming_schema();
        let mut id = crate::linkml::SlotDefinition::new("id");
        id.identifier = true;
        let mut query = crate::linkml::ClassDefinition::new("Query");
        query.attributes.insert("id".to_string(), id);
        let mut anchors = crate::linkml::SlotDefinition::new("anchors");
        anchors.range = Some("DomainRecord".to_string());
        anchors.multivalued = true;
        query.attributes.insert("anchors".to_string(), anchors);
        let mut via = crate::linkml::SlotDefinition::new("via");
        via.range = Some("uri".to_string());
        query.attributes.insert("via".to_string(), via);
        schema.classes.insert("Query".to_string(), query);
        let bench = schema.classes.get_mut("Bench").unwrap();
        let mut queries = crate::linkml::SlotDefinition::new("queries");
        queries.range = Some("Query".to_string());
        queries.multivalued = true;
        bench.attributes.insert("queries".to_string(), queries);
        schema
    }

    #[test]
    fn same_named_records_that_differ_in_content_are_not_reported() {
        // Two estates' `api-gateway` are different services sharing a generic
        // name — the case scoping exists to separate. Warning here would fire
        // on every correct separation and drown the real signal.
        let schema = scoped_schema();
        let acme = scoped_set(
            &schema,
            "id: acme\nproviders:\n  - {id: gw, name: Acme gateway}\n",
        );
        let contoso = scoped_set(
            &schema,
            "id: contoso\nproviders:\n  - {id: gw, name: Contoso gateway}\n",
        );
        assert!(
            cross_dataset_unintended_splits(
                &schema,
                &[("acme.yaml", &acme), ("contoso.yaml", &contoso)]
            )
            .is_empty(),
            "records that differ in content are two things, not one split"
        );
    }

    #[test]
    fn a_split_across_three_datasets_names_all_three() {
        // Three estates each defining `aws` locally is the same mistake, more
        // so — and the report must name every dataset holding a copy, since
        // the fix is to move it out of all of them.
        let schema = scoped_schema();
        let sets: Vec<_> = ["acme", "contoso", "initech"]
            .iter()
            .map(|est| {
                scoped_set(
                    &schema,
                    &format!("id: {est}\nproviders:\n  - {{id: aws, name: Amazon Web Services}}\n"),
                )
            })
            .collect();
        let labelled: Vec<(&str, &crate::instances::InstanceSet)> = vec![
            ("acme.yaml", &sets[0]),
            ("contoso.yaml", &sets[1]),
            ("initech.yaml", &sets[2]),
        ];
        let splits = cross_dataset_unintended_splits(&schema, &labelled);
        assert_eq!(splits.len(), 1, "one entity, one report; got: {splits:?}");
        assert_eq!(
            splits[0].datasets,
            vec![
                "acme.yaml".to_string(),
                "contoso.yaml".to_string(),
                "initech.yaml".to_string()
            ],
            "every dataset holding a copy is named"
        );
    }

    #[test]
    fn records_with_nothing_to_tell_them_apart_are_reported() {
        // Honest about the heuristic's edge: two thin records carrying only an
        // id really do have nothing distinguishing them, so this fires. The
        // report says "defined identically", which is exactly what is true —
        // the author adds detail or shares the record.
        let schema = scoped_schema();
        let acme = scoped_set(&schema, "id: acme\nproviders:\n  - {id: gw}\n");
        let contoso = scoped_set(&schema, "id: contoso\nproviders:\n  - {id: gw}\n");
        let splits = cross_dataset_unintended_splits(
            &schema,
            &[("acme.yaml", &acme), ("contoso.yaml", &contoso)],
        );
        assert_eq!(
            splits.len(),
            1,
            "indistinguishable records are reported, thin or not"
        );
        assert!(
            splits[0].message().contains("defined identically"),
            "and the message states the basis it fired on; got: {}",
            splits[0].message()
        );
    }

    #[test]
    fn records_that_did_not_scope_apart_are_left_to_the_collision_check() {
        // Same scope means they already denote one individual — that is the
        // collision case, and reporting it here too would double-report.
        let schema = scoped_schema();
        let preview = scoped_set(
            &schema,
            "id: acme\nproviders:\n  - {id: aws, name: Amazon Web Services}\n",
        );
        let full = scoped_set(
            &schema,
            "id: acme\nproviders:\n  - {id: aws, name: Amazon Web Services}\n",
        );
        assert!(
            cross_dataset_unintended_splits(
                &schema,
                &[("preview.yaml", &preview), ("full.yaml", &full)]
            )
            .is_empty(),
            "records sharing a scope have not split"
        );
    }

    #[test]
    fn scoping_retires_the_collision_two_estates_used_to_report() {
        // The regression slice 3 exists to be: once each dataset scopes under
        // its own root, the same id in two estates is two individuals, so
        // there is no longer a collision to report.
        let mut schema = collision_schema();
        schema.default_range = Some("string".to_string());
        let mut root = crate::linkml::ClassDefinition::new("Enterprise");
        root.tree_root = true;
        let mut id = crate::linkml::SlotDefinition::new("id");
        id.identifier = true;
        root.attributes.insert("id".to_string(), id);
        let mut deployments = crate::linkml::SlotDefinition::new("deployments");
        deployments.range = Some("Deployment".to_string());
        deployments.multivalued = true;
        root.attributes
            .insert("deployments".to_string(), deployments);
        schema.classes.insert("Enterprise".to_string(), root);
        let mut dep = crate::linkml::ClassDefinition::new("Deployment");
        let mut key = crate::linkml::SlotDefinition::new("id");
        key.key = true;
        dep.attributes.insert("id".to_string(), key);
        schema.classes.insert("Deployment".to_string(), dep);

        let read = |yaml: &str| {
            let data: serde_norway::Value = serde_norway::from_str(yaml).unwrap();
            crate::instances::InstanceSet::from_linkml_data(&schema, &data)
        };
        let acme = read("id: acme\ndeployments:\n  - id: api-gateway\n");
        let contoso = read("id: contoso\ndeployments:\n  - id: api-gateway\n");
        assert!(
            cross_dataset_iri_collisions(
                &schema,
                &[("acme.yaml", &acme), ("contoso.yaml", &contoso)]
            )
            .is_empty(),
            "two scoped estates sharing a service name no longer merge"
        );
    }

    #[test]
    fn datasets_that_share_nothing_are_reported_as_nothing() {
        let schema = collision_schema();
        let a = dataset(&["merlot"]);
        let b = dataset(&["gamay"]);
        assert!(
            cross_dataset_iri_collisions(&schema, &[("a.yaml", &a), ("b.yaml", &b)]).is_empty(),
            "distinct ids across datasets are not a collision"
        );
    }

    #[test]
    fn resolved_instance_references_do_not_warn() {
        let set = crate::instances::InstanceSet {
            instances: vec![
                instance("wineA", &[("produced_by", "realWinery")]),
                instance("realWinery", &[]),
            ],
            ..Default::default()
        };
        assert!(
            dangling_instance_references(&set).is_empty(),
            "a reference to a defined instance is not dangling"
        );
    }
}
