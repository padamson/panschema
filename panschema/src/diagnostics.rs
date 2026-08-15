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
/// strict mode is on and the schema has at least one blocking problem — an
/// unmodeled construct or a dangling reference. Keeping the decision here (not
/// inline in the CLI) keeps it unit-testable.
pub fn should_fail_strict(
    unmodeled: &[UnmodeledConstruct],
    dangling: &[DanglingRef],
    colliding: &[CollidingSlot],
    untyped: &[UntypedSlot],
    strict: bool,
) -> bool {
    strict
        && (!unmodeled.is_empty()
            || !dangling.is_empty()
            || !colliding.is_empty()
            || !untyped.is_empty())
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
             `string` while RDF, SHACL, Postgres, HTML, and `validate` leave it \
             unconstrained; declare a range (`range:` or `slot_usage` in YAML, `rdfs:range` \
             in OWL/Turtle) or a `default_range` in the slot's schema file",
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
        let unmodeled = vec![UnmodeledConstruct {
            class: "C".to_string(),
            construct: "rules".to_string(),
        }];
        let dangling = vec![DanglingRef {
            referrer: "slot `x`".to_string(),
            kind: "range",
            name: "Missing".to_string(),
        }];
        let no_unmodeled: Vec<UnmodeledConstruct> = Vec::new();
        let no_dangling: Vec<DanglingRef> = Vec::new();

        // Not strict ⇒ never fail, whatever is present.
        assert!(!should_fail_strict(&unmodeled, &dangling, &[], &[], false));
        // Strict + nothing ⇒ ok.
        assert!(!should_fail_strict(
            &no_unmodeled,
            &no_dangling,
            &[],
            &[],
            true
        ));
        // Strict + any kind of finding ⇒ fail.
        assert!(
            should_fail_strict(&unmodeled, &no_dangling, &[], &[], true),
            "strict + unmodeled ⇒ fail"
        );
        assert!(
            should_fail_strict(&no_unmodeled, &dangling, &[], &[], true),
            "strict + dangling ⇒ fail"
        );
        let untyped = vec![UntypedSlot {
            name: "x".to_string(),
            site: "class `C`".to_string(),
        }];
        assert!(
            should_fail_strict(&no_unmodeled, &no_dangling, &[], &untyped, true),
            "strict + untyped slot ⇒ fail"
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
