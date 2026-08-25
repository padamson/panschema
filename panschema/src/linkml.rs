//! LinkML Internal Representation (IR)
//!
//! This module defines Rust structs that mirror the LinkML metamodel,
//! serving as the canonical internal representation for panschema.
//!
//! Reference: <https://linkml.io/linkml-model/latest/docs/specification/>

// Allow dead code in this module - the LinkML IR defines many optional fields
// that may not be populated by all readers or consumed by all writers. This is
// by design to support the full LinkML metamodel across different formats.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// LinkML annotations on an element: a tag→value map.
///
/// Values were `string` before LinkML 1.6 and any object from 1.6 on.
/// panschema itself reads only its own `panschema:*` tags, all
/// string-valued (see [`Annotations::get_str`]); a value it does not
/// interpret loads whole and is preserved for whoever does.
///
/// The reading follows the LinkML reference implementation, so a schema
/// means the same thing here and in the Python toolchain. The metamodel
/// spells one annotation three ways, and all three denote the same map:
///
/// ```yaml
/// annotations:
///   note: hello                         # compact: the value directly
///   note: { tag: note, value: hello }   # expanded, keyed by tag
/// annotations:
///   - tag: note                         # expanded, as a list
///     value: hello
/// ```
///
/// A mapping under a tag is always the expanded form's `Annotation`
/// body, never a bare value: a structured value rides under `value:`
/// (`review_status: {value: {stage: draft}}`), a body key outside the
/// metamodel is a parse error pointing at that spelling, and a body
/// `tag` contradicting the key it is filed under is a parse error too —
/// each exactly where the reference implementation also refuses. A
/// body without `value:` reads as null, as it does there.
///
/// One deliberate divergence, for the tool's own string-valued tags: a
/// *scalar* value is read as a string rendering of the parsed scalar
/// (`panschema:label: 2024` means the label "2024"; quote a value to
/// control its exact spelling), where the reference implementation
/// keeps the number. Values inside a structure keep their types.
#[derive(Debug, Clone, Default)]
pub struct Annotations {
    values: BTreeMap<String, serde_norway::Value>,
    /// Tags whose authored body carried nested `annotations` or
    /// `extensions`, which panschema does not model. Recorded so load
    /// diagnostics can say the nesting was dropped instead of the drop
    /// being silent; the annotation's own value is kept regardless.
    unmodeled_nesting: std::collections::BTreeSet<String>,
}

/// Equality is over the tag→value content only. The unmodeled-nesting
/// record is a load-time observation, not model content: including it
/// would make two identically-modeled definitions unequal — failing the
/// imports merge's collision check and serialize-reload equality — just
/// because one spelled its annotation with nesting panschema drops.
impl PartialEq for Annotations {
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values
    }
}

impl Annotations {
    /// The value carried under `tag`, whatever its shape — the public
    /// read path for a preserved structured value.
    pub fn get(&self, tag: &str) -> Option<&serde_norway::Value> {
        self.values.get(tag)
    }

    /// The value under `tag` when it is a string. Every `panschema:*` tag
    /// is string-valued (scalars read lexically, so a bare number or
    /// boolean qualifies); a tag carrying a structured value is not a
    /// string and reads as absent here rather than as some rendering of
    /// the structure.
    pub fn get_str(&self, tag: &str) -> Option<&str> {
        self.values.get(tag).and_then(|v| v.as_str())
    }

    /// Record a string-valued annotation, replacing any value under
    /// `tag` — and any nesting record with it, so the diagnostic can
    /// never name a value that has been written over.
    pub fn insert(&mut self, tag: impl Into<String>, value: impl Into<String>) {
        self.set_parsed(tag.into(), serde_norway::Value::String(value.into()), false);
    }

    /// Take the annotation under `tag` out of the map, whatever its
    /// shape — a caller owning a tag clears it unconditionally and then
    /// judges the value, so no stray shape can keep the tag occupied.
    /// The nesting record goes with it: a diagnostic must not name an
    /// annotation the schema no longer carries.
    pub fn remove(&mut self, tag: &str) -> Option<serde_norway::Value> {
        self.unmodeled_nesting.remove(tag);
        self.values.remove(tag)
    }

    /// The one write path: stores the value and keeps the nesting
    /// record in step — set on a nested body, cleared on any overwrite —
    /// so the record is always about the value actually held.
    fn set_parsed(&mut self, tag: String, value: serde_norway::Value, nested: bool) {
        if nested {
            self.unmodeled_nesting.insert(tag.clone());
        } else {
            self.unmodeled_nesting.remove(&tag);
        }
        self.values.insert(tag, value);
    }

    /// Whether the element carries no annotations, which is how an empty
    /// map stays out of serialized output rather than appearing as `{}`.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The tags whose authored bodies carried nested annotations or
    /// extensions panschema dropped, in tag order — the load
    /// diagnostics' feed.
    pub fn tags_with_unmodeled_nesting(&self) -> impl Iterator<Item = &str> {
        self.unmodeled_nesting.iter().map(String::as_str)
    }

    /// The element's display label: its `panschema:label` annotation or
    /// `fallback` (its name) — the one definition of label resolution,
    /// so HTML, graph, RDF, and instance output cannot drift.
    pub fn label_or(&self, fallback: &str) -> String {
        self.label_or_ref(fallback).to_string()
    }

    /// Borrowed form of [`Annotations::label_or`], for sort keys and
    /// other sites that never need to own the label.
    pub fn label_or_ref<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.get_str("panschema:label").unwrap_or(fallback)
    }
}

impl<'a> IntoIterator for &'a Annotations {
    type Item = (&'a String, &'a serde_norway::Value);
    type IntoIter = std::collections::btree_map::Iter<'a, String, serde_norway::Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}

impl Serialize for Annotations {
    /// Emits the compact spelling, except that a structured value is
    /// wrapped back under `value:` — the one form that re-reads
    /// identically here (a bare mapping would be taken for an
    /// `Annotation` body) and loads in the Python toolchain.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.values.len()))?;
        for (tag, value) in &self.values {
            if value.is_mapping() {
                let mut body = serde_norway::Mapping::new();
                body.insert("value".into(), value.clone());
                map.serialize_entry(tag, &serde_norway::Value::Mapping(body))?;
            } else {
                map.serialize_entry(tag, value)?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Annotations {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct AnnotationsVisitor;

        impl<'de> serde::de::Visitor<'de> for AnnotationsVisitor {
            type Value = Annotations;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("annotations as a tag-keyed map or a list of tag/value objects")
            }

            // `#[mutants::skip]`: the body is exactly `Default::default()`,
            // so the replacement mutant is the same code — unkillable by
            // construction, not untested (the empty-annotations test pins
            // the behavior).
            #[mutants::skip]
            fn visit_unit<E: serde::de::Error>(self) -> Result<Annotations, E> {
                // A bare `annotations:` key — every entry commented out —
                // is no annotations, as the reference implementation also
                // reads it.
                Ok(Annotations::default())
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Annotations, A::Error> {
                use serde::de::Error;
                let mut out = Annotations::default();
                while let Some((tag, raw)) = map.next_entry::<String, serde_norway::Value>()? {
                    let (value, nested) = match raw {
                        serde_norway::Value::Mapping(body) => {
                            annotation_body_value(&tag, body).map_err(A::Error::custom)?
                        }
                        serde_norway::Value::Tagged(_) => {
                            return Err(A::Error::custom(format!(
                                "annotation `{tag}` carries a YAML-tagged value, which the \
                                 LinkML reference implementation refuses"
                            )));
                        }
                        other => (scalar_lexical(other), false),
                    };
                    out.set_parsed(tag, value, nested);
                }
                Ok(out)
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Annotations, A::Error> {
                use serde::de::Error;
                let mut out = Annotations::default();
                while let Some(item) = seq.next_element::<serde_norway::Value>()? {
                    let serde_norway::Value::Mapping(body) = item else {
                        return Err(A::Error::custom(
                            "an annotation in list form is a mapping with `tag` and `value`",
                        ));
                    };
                    let tag = body
                        .get("tag")
                        .and_then(|t| t.as_str())
                        .ok_or_else(|| {
                            A::Error::custom("an annotation in list form needs a `tag`")
                        })?
                        .to_string();
                    let (value, nested) =
                        annotation_body_value(&tag, body).map_err(A::Error::custom)?;
                    out.set_parsed(tag, value, nested);
                }
                Ok(out)
            }
        }

        // `deserialize_any` rather than `deserialize_map`: the shape is not
        // known until it is seen, and the classes carrying annotations are
        // read through serde's `flatten` buffer, which answers only this.
        deserializer.deserialize_any(AnnotationsVisitor)
    }
}

/// Read a mapping under `tag` as the metamodel's `Annotation` body:
/// its keys are the model's own (`tag`, `value`, `annotations`,
/// `extensions`) and nothing else; a `tag` inside must agree with the
/// key the body is filed under; the value is whatever rides under
/// `value:` (null when absent). Returns the value plus whether the body
/// carried nesting panschema does not model. Both refusals mirror the
/// LinkML reference implementation, so a schema that loads here loads
/// there.
fn annotation_body_value(
    tag: &str,
    mut body: serde_norway::Mapping,
) -> Result<(serde_norway::Value, bool), String> {
    for key in body.keys() {
        let known = key
            .as_str()
            .is_some_and(|k| matches!(k, "tag" | "value" | "annotations" | "extensions"));
        if !known {
            return Err(format!(
                "annotation `{tag}` carries `{}`, which is not part of the LinkML \
                 Annotation model; a structured value belongs under `value:` — \
                 `{tag}: {{value: {{...}}}}`",
                key.as_str().unwrap_or("a non-string key")
            ));
        }
    }
    if let Some(inner) = body.get("tag")
        && inner.as_str() != Some(tag)
    {
        return Err(format!(
            "annotation filed under `{tag}` declares a different tag ({})",
            inner.as_str().unwrap_or("not a string")
        ));
    }
    let nested = body.contains_key("annotations") || body.contains_key("extensions");
    let value = body.remove("value").unwrap_or(serde_norway::Value::Null);
    if matches!(value, serde_norway::Value::Tagged(_)) {
        return Err(format!(
            "annotation `{tag}` carries a YAML-tagged value, which the LinkML \
             reference implementation refuses"
        ));
    }
    Ok((scalar_lexical(value), nested))
}

/// A scalar annotation value reads as a string rendering of the parsed
/// scalar — the shape every `panschema:*` consumer expects, and what an
/// unquoted `panschema:label: 2024` plainly means. The rendering is the
/// parsed value's canonical form (`1e3` becomes "1000.0"); quote the
/// value to control its exact spelling. Structures and nulls pass
/// through; values inside a structure are never touched.
fn scalar_lexical(value: serde_norway::Value) -> serde_norway::Value {
    match value {
        serde_norway::Value::Bool(b) => serde_norway::Value::String(b.to_string()),
        serde_norway::Value::Number(n) => serde_norway::Value::String(n.to_string()),
        other => other,
    }
}

/// A prefix mapping for CURIE expansion
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prefix {
    /// The prefix name (e.g., "schema", "rdfs")
    pub prefix_prefix: String,
    /// The IRI expansion (e.g., "http://schema.org/")
    pub prefix_reference: String,
}

/// A contributor to the schema (author, editor, etc.)
///
/// Used to capture Dublin Core-style contributor metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contributor {
    /// The contributor's name
    pub name: String,
    /// ORCID identifier URL (e.g., "https://orcid.org/0000-0002-1825-0097")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orcid: Option<String>,
    /// Role in the project (e.g., "author", "editor", "contributor")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

impl Contributor {
    /// Create a new contributor with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            orcid: None,
            role: None,
        }
    }

    /// Create a contributor with name and role
    pub fn with_role(name: impl Into<String>, role: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            orcid: None,
            role: Some(role.into()),
        }
    }
}

/// A worked example value for an element.
///
/// Corresponds to one entry in LinkML's `examples` metaslot (a list of
/// structured `example` objects). Rendered as an item in the card's
/// "Examples" section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Example {
    /// The example value, shown verbatim.
    pub value: String,
    /// Optional explanation of what the value illustrates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The range LinkML's derivation rules give a schema that omits
/// `default_range`. Applied at read time by the LinkML YAML reader, so an
/// omitted default means the same thing here as through linkml-runtime.
pub const LINKML_DEFAULT_RANGE: &str = "string";

/// Root container for a LinkML schema
///
/// Corresponds to LinkML SchemaDefinition.
/// Reference: <https://linkml.io/linkml-model/latest/docs/SchemaDefinition/>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaDefinition {
    /// A unique, machine-readable identifier for the schema
    pub name: String,
    /// The official URI for this schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Human-readable title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Schema description/documentation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Deprecation note. When set, the element is marked deprecated:
    /// the card shows a "Deprecated" badge with this text, and RDF emits
    /// `owl:deprecated true` on the element IRI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    /// Alternative names for the element. Rendered as a comma-joined
    /// "Aliases" row on the card; RDF emits one `skos:altLabel` per
    /// entry on the element IRI.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Related-resource references (URIorCURIE). Rendered as a "See
    /// also" row of CURIE-expanded links on the card; RDF emits one
    /// `rdfs:seeAlso` per entry on the element IRI.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub see_also: Vec<String>,
    /// Worked examples for the element. Rendered as an "Examples"
    /// section on the card; LinkML `examples` has no standard RDF
    /// predicate, so it is not emitted to RDF.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<Example>,
    /// Schema version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// License for the schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Contributors to the schema (authors, editors, etc.)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contributors: Vec<Contributor>,
    /// Creation date (ISO 8601 format, e.g., "2025-01-15")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    /// Last modification date (ISO 8601 format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    /// Imported schemas/ontologies (URIs)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<String>,
    /// Prefix mappings for CURIE expansion
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub prefixes: BTreeMap<String, String>,
    /// Default prefix for unprefixed names
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_prefix: Option<String>,
    /// Default range for slots without explicit range
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_range: Option<String>,
    /// Class definitions in this schema
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub classes: BTreeMap<String, ClassDefinition>,
    /// Slot definitions in this schema
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub slots: BTreeMap<String, SlotDefinition>,
    /// Enum definitions in this schema
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub enums: BTreeMap<String, EnumDefinition>,
    /// Type definitions in this schema
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub types: BTreeMap<String, TypeDefinition>,
    /// Format-specific annotations (e.g., OWL-specific metadata)
    #[serde(default, skip_serializing_if = "Annotations::is_empty")]
    pub annotations: Annotations,
}

impl SchemaDefinition {
    /// The definition of a slot referenced by *name*: the top-level
    /// `slots:` entry, or the first class attribute (classes in map order)
    /// declaring it. The one by-name lookup for every site that holds a
    /// slot name rather than its definition — IRI derivation and display
    /// labels resolve the same declaration, so they cannot drift.
    pub fn find_slot(&self, name: &str) -> Option<&SlotDefinition> {
        self.slots.get(name).or_else(|| {
            self.classes
                .values()
                .find_map(|class| class.attributes.get(name))
        })
    }

    /// The display label for the slot named `name`: its `panschema:label`
    /// annotation — resolved through [`SchemaDefinition::find_slot`], so
    /// attribute-declared slots are covered — or the name itself. Shared
    /// by the HTML slot cards and the instance pages, so the same slot
    /// cannot show two different names.
    pub fn slot_display_label(&self, name: &str) -> String {
        self.find_slot(name)
            .map(|def| def.annotations.label_or(name))
            .unwrap_or_else(|| name.to_string())
    }

    /// Create a new schema with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            id: None,
            title: None,
            description: None,
            deprecated: None,
            aliases: Vec::new(),
            see_also: Vec::new(),
            examples: Vec::new(),
            version: None,
            license: None,
            contributors: Vec::new(),
            created: None,
            modified: None,
            imports: Vec::new(),
            prefixes: BTreeMap::new(),
            default_prefix: None,
            default_range: None,
            classes: BTreeMap::new(),
            slots: BTreeMap::new(),
            enums: BTreeMap::new(),
            types: BTreeMap::new(),
            annotations: Annotations::default(),
        }
    }

    /// Returns the display title (title if available, otherwise name)
    pub fn display_title(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }
}

/// A conditional constraint on a class: LinkML's `rules` metaslot.
///
/// Corresponds to LinkML ClassRule.
/// Reference: <https://linkml.io/linkml-model/latest/docs/ClassRule/>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassRule {
    /// Short label for the rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Human-readable explanation of the rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Condition that must hold for the rule to apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preconditions: Option<RuleConditions>,
    /// Condition required once the rule applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postconditions: Option<RuleConditions>,
}

/// An anonymous class expression used as a [`ClassRule`]'s pre/postcondition:
/// LinkML's `slot_conditions` map, slot name -> the constraint subset
/// panschema renders on that slot elsewhere.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleConditions {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub slot_conditions: BTreeMap<String, SlotCondition>,
    /// LinkML `any_of`: alternative condition sets. The condition holds when
    /// *any* one of these sub-condition sets holds — e.g. a precondition
    /// that fires when `verdict` is `approved` **or** `rejected`. Empty when
    /// the condition is a plain `slot_conditions` map.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub any_of: Vec<RuleConditions>,
}

/// LinkML `value_presence` check on a slot condition: the slot's value must
/// be present (non-null) or absent (null) for the condition to hold — the
/// checkable content of a postcondition like "once approved, `approved_by`
/// is present".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValuePresence {
    #[serde(rename = "PRESENT")]
    Present,
    #[serde(rename = "ABSENT")]
    Absent,
}

/// One slot's constraint within a [`RuleConditions`]' `slot_conditions` map.
///
/// Mirrors the subset of LinkML's `SlotDefinition`-shaped slot condition
/// panschema already renders on ordinary slots (`range` / `required` /
/// cardinality / value bounds / `pattern`), plus `equals_string` /
/// `equals_number` — the equality checks a precondition like "when
/// `status` has value `actual`" needs, since none of the other fields
/// express equality.
///
/// `equals_string`/`equals_number` are **membership** tests: the condition
/// holds when at least one of the slot's values equals the constant —
/// `sh:hasValue` semantics, enforced identically by `validate`, the SHACL
/// projection, and the Postgres `CHECK` (`= ANY` on an array column). An
/// absent slot never satisfies one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SlotCondition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_cardinality: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum_cardinality: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equals_string: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equals_number: Option<f64>,
    /// LinkML `value_presence`: the slot's value must be present or absent
    /// for the condition to hold — the checkable content of a postcondition
    /// like "once approved, `approved_by` is present".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_presence: Option<ValuePresence>,
    /// LinkML `any_of` on this slot's condition: the slot's value satisfies
    /// *any* of these alternative sub-conditions — e.g. `verdict` is
    /// `approved` **or** `rejected`. Distinct from [`RuleConditions::any_of`]
    /// (which alternates whole condition sets); this alternates a single
    /// slot's value expressions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub any_of: Vec<SlotCondition>,
}

/// A uniqueness constraint on a class: LinkML's `unique_keys` metaslot.
///
/// The tuple of `unique_key_slots` must be unique across instances of the
/// class. Corresponds to LinkML UniqueKey.
/// Reference: <https://linkml.io/linkml-model/latest/docs/UniqueKey/>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UniqueKey {
    /// The slots whose combined values must be unique.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unique_key_slots: Vec<String>,
    /// Human-readable explanation of the constraint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A class definition in a LinkML schema
///
/// Corresponds to LinkML ClassDefinition.
/// Reference: <https://linkml.io/linkml-model/latest/docs/ClassDefinition/>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassDefinition {
    /// The unique name of this class within the schema.
    /// In dict-keyed contexts (e.g. YAML `classes:`) this is inferred
    /// from the dict key by `YamlReader::backfill_names` if absent.
    #[serde(default)]
    pub name: String,
    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Deprecation note; see [`SchemaDefinition::deprecated`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    /// Alternative names; see [`SchemaDefinition::aliases`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Related-resource references; see [`SchemaDefinition::see_also`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub see_also: Vec<String>,
    /// Worked examples; see [`SchemaDefinition::examples`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<Example>,
    /// Primary parent class (single inheritance)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_a: Option<String>,
    /// Secondary parent classes (mixins)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mixins: Vec<String>,
    /// Whether this class is abstract (cannot be instantiated directly)
    #[serde(default, skip_serializing_if = "is_false")]
    pub r#abstract: bool,
    /// Whether this class is the data-tree root — the container that a
    /// conforming instance-data file is a single instance of. The instance
    /// reader uses it as the entry point into an A-box, and the JSON-Schema
    /// writer roots its document at it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub tree_root: bool,
    /// Slots that apply to this class
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<String>,
    /// Inline slot definitions specific to this class
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, SlotDefinition>,
    /// Slot refinements in the context of this class
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub slot_usage: BTreeMap<String, SlotDefinition>,
    /// URI for semantic interpretation (e.g., owl:Class IRI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_uri: Option<String>,
    /// External `rdfs:subClassOf` target — typically an upstream
    /// ontology class the schema author is grounding this class in
    /// (BFO, CCO, IAO, …). Distinct from `is_a`, which models
    /// intra-schema inheritance. Single-valued per the LinkML
    /// metamodel; authors needing multiple groundings use mixins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subclass_of: Option<String>,
    /// Cross-ontology mappings (SKOS-aligned). Each value is a CURIE
    /// or IRI in an upstream vocabulary (BFO, CCO, IAO, …).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exact_mappings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub close_mappings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_mappings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub narrow_mappings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub broad_mappings: Vec<String>,
    /// Format-specific annotations
    #[serde(default, skip_serializing_if = "Annotations::is_empty")]
    pub annotations: Annotations,
    /// Conditional constraints (LinkML `rules`): each fires its
    /// postconditions when its preconditions hold. Rendered as a "Rules"
    /// section on the class card.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<ClassRule>,
    /// Uniqueness constraints (LinkML `unique_keys`): each names a tuple
    /// of slots whose combined values must be unique across instances.
    /// Rendered as a "Unique keys" row on the class card.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unique_keys: BTreeMap<String, UniqueKey>,
    /// LinkML keys present on this class in the source but not modeled
    /// by panschema. Captured (rather than silently dropped by serde)
    /// so [`crate::diagnostics`] can warn when a producer writes a
    /// construct — e.g. `unique_keys` — that won't render or emit.
    /// Populated only by the YAML reader; empty otherwise.
    #[serde(flatten, default)]
    pub unmodeled: BTreeMap<String, serde_norway::Value>,
}

impl ClassDefinition {
    /// Create a new class with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            deprecated: None,
            aliases: Vec::new(),
            see_also: Vec::new(),
            examples: Vec::new(),
            is_a: None,
            mixins: Vec::new(),
            unmodeled: BTreeMap::new(),
            r#abstract: false,
            tree_root: false,
            slots: Vec::new(),
            attributes: BTreeMap::new(),
            slot_usage: BTreeMap::new(),
            class_uri: None,
            subclass_of: None,
            exact_mappings: Vec::new(),
            close_mappings: Vec::new(),
            related_mappings: Vec::new(),
            narrow_mappings: Vec::new(),
            broad_mappings: Vec::new(),
            annotations: Annotations::default(),
            rules: Vec::new(),
            unique_keys: BTreeMap::new(),
        }
    }

    /// The class's display label: its `panschema:label` annotation, or
    /// its name.
    pub fn display_label(&self) -> &str {
        self.annotations.label_or_ref(&self.name)
    }
}

/// A slot (property) definition in a LinkML schema
///
/// Corresponds to LinkML SlotDefinition.
/// Reference: <https://linkml.io/linkml-model/latest/docs/SlotDefinition/>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlotDefinition {
    /// The unique name of this slot within the schema.
    /// Inferred from the dict key by `YamlReader::backfill_names` if absent.
    #[serde(default)]
    pub name: String,
    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Deprecation note; see [`SchemaDefinition::deprecated`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    /// Alternative names; see [`SchemaDefinition::aliases`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Related-resource references; see [`SchemaDefinition::see_also`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub see_also: Vec<String>,
    /// Worked examples; see [`SchemaDefinition::examples`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<Example>,
    /// The type of values this slot holds (class name, type name, or enum name)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    /// The class that owns this slot (domain)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// Default value applied when the slot is absent (LinkML `ifabsent`).
    /// Carries a LinkML `ifabsent` expression verbatim (e.g.
    /// `"ItemStatus(planned)"`); consumers parse the form. Absent when
    /// unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ifabsent: Option<String>,
    /// Whether this slot must be present
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
    /// Whether this slot can hold multiple values
    #[serde(default, skip_serializing_if = "is_false")]
    pub multivalued: bool,
    /// LinkML `designates_type`: this slot's authored value names the
    /// record's class (by name, IRI, or CURIE), taking precedence over
    /// any key-based inference when a union range must choose a member.
    #[serde(default, skip_serializing_if = "is_false")]
    pub designates_type: bool,
    /// Minimum number of values (for multivalued slots)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_cardinality: Option<u32>,
    /// Maximum number of values (for multivalued slots)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_cardinality: Option<u32>,
    /// Regular expression pattern for string values
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// Whether this slot uniquely identifies instances
    #[serde(default, skip_serializing_if = "is_false")]
    pub identifier: bool,
    /// Whether this slot identifies instances uniquely **within their
    /// container** — LinkML's locally-scoped counterpart to `identifier`,
    /// which claims global uniqueness. A class may declare one or the other,
    /// not both.
    #[serde(default, skip_serializing_if = "is_false")]
    pub key: bool,
    /// Whether class-ranged values are written inline (nested objects) or as
    /// references. LinkML's default depends on the range class: always
    /// inlined without an identifier/key, referenced with one. `None` when
    /// the schema doesn't say. Modeled but not yet enforced when reading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inlined: Option<bool>,
    /// When inlined and multivalued: `true` for a list of objects, `false`
    /// for a dict keyed by the identifier/key. `None` when unspecified.
    /// Modeled but not yet enforced when reading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inlined_as_list: Option<bool>,
    /// URI for semantic interpretation (e.g., owl:ObjectProperty IRI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_uri: Option<String>,
    /// Inverse slot (for bidirectional relationships)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inverse: Option<String>,
    /// Parent slot this one specializes (LinkML slot-level `is_a`): every
    /// value of this slot is also a value of the parent. Projects as
    /// `rdfs:subPropertyOf` in RDF output. The parent's unset option- and
    /// list-valued metaslots are inherited at load time
    /// ([`crate::linkml_resolve::resolve_slot_inheritance`]); boolean
    /// metaslots are not, since the IR cannot distinguish a stated
    /// `false` from silence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_a: Option<String>,
    /// OWL object-property characteristics. Each, when set, maps to an
    /// `owl:<Name>Property` `rdf:type` axiom in the RDF output and a
    /// characteristic badge on the slot card. These are LinkML's
    /// `symmetric` / `asymmetric` / `reflexive` / `irreflexive` /
    /// `transitive` relationship metaslots.
    #[serde(default, skip_serializing_if = "is_false")]
    pub symmetric: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub asymmetric: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reflexive: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub irreflexive: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub transitive: bool,
    /// Inclusive lower / upper bounds on a numeric value (LinkML's
    /// `minimum_value` / `maximum_value`). Rendered as a card badge and
    /// emitted in RDF as an `owl:withRestrictions` `xsd:minInclusive` /
    /// `xsd:maxInclusive` datatype restriction on the property's range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_value: Option<f64>,
    /// Polymorphic range alternatives. A value of this slot matches any
    /// one of the branches; each branch is itself a partial slot
    /// definition that can override `range`, `required`, `multivalued`,
    /// etc. Vec already provides heap indirection, so the recursive type
    /// is fine.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub any_of: Vec<SlotDefinition>,
    /// Cross-ontology mappings; see [`ClassDefinition::exact_mappings`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exact_mappings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub close_mappings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_mappings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub narrow_mappings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub broad_mappings: Vec<String>,
    /// Format-specific annotations
    #[serde(default, skip_serializing_if = "Annotations::is_empty")]
    pub annotations: Annotations,
}

impl SlotDefinition {
    /// Create a new slot with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            deprecated: None,
            aliases: Vec::new(),
            see_also: Vec::new(),
            examples: Vec::new(),
            range: None,
            domain: None,
            ifabsent: None,
            required: false,
            multivalued: false,
            designates_type: false,
            minimum_cardinality: None,
            maximum_cardinality: None,
            pattern: None,
            identifier: false,
            key: false,
            inlined: None,
            inlined_as_list: None,
            slot_uri: None,
            inverse: None,
            is_a: None,
            symmetric: false,
            asymmetric: false,
            reflexive: false,
            irreflexive: false,
            transitive: false,
            minimum_value: None,
            maximum_value: None,
            any_of: Vec::new(),
            exact_mappings: Vec::new(),
            close_mappings: Vec::new(),
            related_mappings: Vec::new(),
            narrow_mappings: Vec::new(),
            broad_mappings: Vec::new(),
            annotations: Annotations::default(),
        }
    }

    /// The slot's display label: its `panschema:label` annotation, or
    /// its name.
    pub fn display_label(&self) -> &str {
        self.annotations.label_or_ref(&self.name)
    }
}

/// An enumeration definition in a LinkML schema
///
/// Corresponds to LinkML EnumDefinition.
/// Reference: <https://linkml.io/linkml-model/latest/docs/EnumDefinition/>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumDefinition {
    /// The unique name of this enum within the schema.
    /// Inferred from the dict key by `YamlReader::backfill_names` if absent.
    #[serde(default)]
    pub name: String,
    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Deprecation note; see [`SchemaDefinition::deprecated`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    /// Alternative names; see [`SchemaDefinition::aliases`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Related-resource references; see [`SchemaDefinition::see_also`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub see_also: Vec<String>,
    /// Worked examples; see [`SchemaDefinition::examples`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<Example>,
    /// The allowed values for this enum
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub permissible_values: BTreeMap<String, PermissibleValue>,
    /// Format-specific annotations
    #[serde(default, skip_serializing_if = "Annotations::is_empty")]
    pub annotations: Annotations,
}

impl EnumDefinition {
    /// Create a new enum with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            deprecated: None,
            aliases: Vec::new(),
            see_also: Vec::new(),
            examples: Vec::new(),
            permissible_values: BTreeMap::new(),
            annotations: Annotations::default(),
        }
    }
}

/// A permissible value within an enumeration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissibleValue {
    /// The value text.
    /// Inferred from the dict key by `YamlReader::backfill_names` if absent.
    #[serde(default)]
    pub text: String,
    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// URI for semantic interpretation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meaning: Option<String>,
}

impl PermissibleValue {
    /// Create a new permissible value with the given text
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            description: None,
            meaning: None,
        }
    }
}

/// A type definition in a LinkML schema
///
/// Corresponds to LinkML TypeDefinition.
/// Reference: <https://linkml.io/linkml-model/latest/docs/TypeDefinition/>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeDefinition {
    /// The unique name of this type within the schema.
    /// Inferred from the dict key by `YamlReader::backfill_names` if absent.
    #[serde(default)]
    pub name: String,
    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Deprecation note; see [`SchemaDefinition::deprecated`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    /// Alternative names; see [`SchemaDefinition::aliases`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Related-resource references; see [`SchemaDefinition::see_also`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub see_also: Vec<String>,
    /// Worked examples; see [`SchemaDefinition::examples`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<Example>,
    /// Parent type (for type inheritance). LinkML spells this `typeof`; the
    /// field carries a trailing underscore only to dodge the Rust keyword, so
    /// it must be renamed for (de)serialization — without this, `typeof:` in a
    /// schema is silently ignored and the type's base is lost.
    #[serde(rename = "typeof", skip_serializing_if = "Option::is_none")]
    pub typeof_: Option<String>,
    /// URI for the underlying datatype (e.g., xsd:string)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// Regular expression pattern for validation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// Format-specific annotations
    #[serde(default, skip_serializing_if = "Annotations::is_empty")]
    pub annotations: Annotations,
}

impl TypeDefinition {
    /// Create a new type with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            deprecated: None,
            aliases: Vec::new(),
            see_also: Vec::new(),
            examples: Vec::new(),
            typeof_: None,
            uri: None,
            pattern: None,
            annotations: Annotations::default(),
        }
    }
}

/// Helper function for serde skip_serializing_if
fn is_false(b: &bool) -> bool {
    !(*b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_and_inlining_flags_round_trip_through_yaml() {
        // The declared shape of instance data lives in the schema; losing
        // any of these on read would make a conforming file unreadable.
        let slot: SlotDefinition = serde_norway::from_str(
            "name: parts\nkey: true\ninlined: true\ninlined_as_list: false\n",
        )
        .expect("parse slot");
        assert!(slot.key, "key parses");
        assert_eq!(slot.inlined, Some(true), "inlined parses");
        assert_eq!(slot.inlined_as_list, Some(false), "inlined_as_list parses");

        let unspecified: SlotDefinition =
            serde_norway::from_str("name: parts\n").expect("parse bare slot");
        assert_eq!(
            unspecified.inlined, None,
            "an unspecified flag stays None — LinkML's default depends on the \
             range class, so a false here would assert something the schema \
             never said"
        );
    }

    // ========== SchemaDefinition Tests ==========

    #[test]
    fn schema_definition_new_creates_minimal_schema() {
        let schema = SchemaDefinition::new("my_schema");
        assert_eq!(schema.name, "my_schema");
        assert!(schema.id.is_none());
        assert!(schema.classes.is_empty());
        assert!(schema.slots.is_empty());
    }

    #[test]
    fn schema_definition_display_title_uses_title_when_present() {
        let mut schema = SchemaDefinition::new("test");
        schema.title = Some("My Schema".to_string());
        assert_eq!(schema.display_title(), "My Schema");
    }

    #[test]
    fn schema_definition_display_title_falls_back_to_name() {
        let schema = SchemaDefinition::new("my_schema");
        assert_eq!(schema.display_title(), "my_schema");
    }

    #[test]
    fn schema_definition_serializes_to_yaml() {
        let mut schema = SchemaDefinition::new("example");
        schema.id = Some("https://example.org/schema".to_string());
        schema.description = Some("An example schema".to_string());

        let yaml = serde_norway::to_string(&schema).unwrap();
        assert!(yaml.contains("name: example"));
        assert!(yaml.contains("id: https://example.org/schema"));
        assert!(yaml.contains("description: An example schema"));
    }

    #[test]
    fn subclass_of_deserializes_as_scalar_per_linkml_metamodel() {
        // LinkML's ClassDefinition.subclass_of is single-valued (not
        // multivalued) — authors needing multiple groundings use
        // mixins. The IR mirrors the metamodel exactly.
        let yaml = "
name: Test
class_uri: ex:Test
subclass_of: cco:ont00000958
";
        let class: ClassDefinition = serde_norway::from_str(yaml).unwrap();
        assert_eq!(class.subclass_of.as_deref(), Some("cco:ont00000958"));
    }

    #[test]
    fn is_false_serde_helper_skips_default_bools() {
        // `is_false` powers `#[serde(skip_serializing_if = "is_false")]`
        // on `required`, `multivalued`, `r#abstract`, etc. If it stops
        // returning `true` for `false`, those fields leak into every
        // serialized output as `field: false` — bloating manifests and
        // breaking round-trip equality with hand-written LinkML.
        let mut slot = SlotDefinition::new("name");
        slot.range = Some("string".to_string());
        // `required` defaults to false and stays false.
        let yaml = serde_norway::to_string(&slot).unwrap();
        assert!(
            !yaml.contains("required:"),
            "default-false `required` should be skipped; got:\n{yaml}"
        );
        assert!(
            !yaml.contains("multivalued:"),
            "default-false `multivalued` should be skipped; got:\n{yaml}"
        );

        // Sanity-check the inverse: a true bool DOES serialize.
        slot.required = true;
        let yaml = serde_norway::to_string(&slot).unwrap();
        assert!(
            yaml.contains("required: true"),
            "true bools must serialize; got:\n{yaml}"
        );
    }

    #[test]
    fn schema_definition_deserializes_from_yaml() {
        let yaml = r#"
name: test_schema
id: https://example.org/test
description: A test schema
"#;
        let schema: SchemaDefinition = serde_norway::from_str(yaml).unwrap();
        assert_eq!(schema.name, "test_schema");
        assert_eq!(schema.id, Some("https://example.org/test".to_string()));
        assert_eq!(schema.description, Some("A test schema".to_string()));
    }

    #[test]
    fn schema_definition_with_classes() {
        let mut schema = SchemaDefinition::new("animals");
        schema
            .classes
            .insert("Animal".to_string(), ClassDefinition::new("Animal"));
        schema.classes.insert("Dog".to_string(), {
            let mut dog = ClassDefinition::new("Dog");
            dog.is_a = Some("Animal".to_string());
            dog
        });

        assert_eq!(schema.classes.len(), 2);
        assert!(schema.classes.contains_key("Animal"));
        assert_eq!(
            schema.classes.get("Dog").unwrap().is_a,
            Some("Animal".to_string())
        );
    }

    // ========== Contributor Tests ==========

    #[test]
    fn contributor_new_creates_minimal_contributor() {
        let contributor = Contributor::new("Jane Doe");
        assert_eq!(contributor.name, "Jane Doe");
        assert!(contributor.orcid.is_none());
        assert!(contributor.role.is_none());
    }

    #[test]
    fn contributor_with_role_sets_name_and_role() {
        let contributor = Contributor::with_role("John Smith", "author");
        assert_eq!(contributor.name, "John Smith");
        assert_eq!(contributor.role, Some("author".to_string()));
        assert!(contributor.orcid.is_none());
    }

    #[test]
    fn contributor_with_all_fields() {
        let mut contributor = Contributor::new("Jane Doe");
        contributor.orcid = Some("https://orcid.org/0000-0002-1825-0097".to_string());
        contributor.role = Some("editor".to_string());

        assert_eq!(contributor.name, "Jane Doe");
        assert_eq!(
            contributor.orcid,
            Some("https://orcid.org/0000-0002-1825-0097".to_string())
        );
        assert_eq!(contributor.role, Some("editor".to_string()));
    }

    #[test]
    fn contributor_serializes_to_yaml() {
        let mut contributor = Contributor::new("Jane Doe");
        contributor.role = Some("author".to_string());

        let yaml = serde_norway::to_string(&contributor).unwrap();
        assert!(yaml.contains("name: Jane Doe"));
        assert!(yaml.contains("role: author"));
        // orcid should be omitted when None
        assert!(!yaml.contains("orcid"));
    }

    // ========== SchemaDefinition Metadata Tests ==========

    #[test]
    fn schema_definition_new_initializes_metadata_fields() {
        let schema = SchemaDefinition::new("test");
        assert!(schema.contributors.is_empty());
        assert!(schema.created.is_none());
        assert!(schema.modified.is_none());
        assert!(schema.imports.is_empty());
    }

    #[test]
    fn schema_definition_with_contributors() {
        let mut schema = SchemaDefinition::new("test");
        schema
            .contributors
            .push(Contributor::with_role("Alice", "author"));
        schema
            .contributors
            .push(Contributor::with_role("Bob", "contributor"));

        assert_eq!(schema.contributors.len(), 2);
        assert_eq!(schema.contributors[0].name, "Alice");
        assert_eq!(schema.contributors[1].name, "Bob");
    }

    #[test]
    fn schema_definition_with_dates() {
        let mut schema = SchemaDefinition::new("test");
        schema.created = Some("2025-01-15".to_string());
        schema.modified = Some("2026-01-29".to_string());

        assert_eq!(schema.created, Some("2025-01-15".to_string()));
        assert_eq!(schema.modified, Some("2026-01-29".to_string()));
    }

    #[test]
    fn schema_definition_with_imports() {
        let mut schema = SchemaDefinition::new("test");
        schema
            .imports
            .push("http://purl.obolibrary.org/obo/bfo.owl".to_string());
        schema.imports.push("http://purl.org/dc/terms/".to_string());

        assert_eq!(schema.imports.len(), 2);
        assert!(
            schema
                .imports
                .contains(&"http://purl.obolibrary.org/obo/bfo.owl".to_string())
        );
    }

    #[test]
    fn schema_definition_metadata_serializes_to_yaml() {
        let mut schema = SchemaDefinition::new("example");
        schema.created = Some("2025-01-15".to_string());
        schema
            .contributors
            .push(Contributor::with_role("Jane Doe", "author"));

        let yaml = serde_norway::to_string(&schema).unwrap();
        assert!(yaml.contains("created: '2025-01-15'") || yaml.contains("created: 2025-01-15"));
        assert!(yaml.contains("name: Jane Doe"));
    }

    #[test]
    fn schema_definition_metadata_deserializes_from_yaml() {
        let yaml = r#"
name: test_schema
created: "2025-01-15"
modified: "2026-01-29"
contributors:
  - name: Jane Doe
    role: author
imports:
  - http://purl.obolibrary.org/obo/bfo.owl
"#;
        let schema: SchemaDefinition = serde_norway::from_str(yaml).unwrap();
        assert_eq!(schema.created, Some("2025-01-15".to_string()));
        assert_eq!(schema.modified, Some("2026-01-29".to_string()));
        assert_eq!(schema.contributors.len(), 1);
        assert_eq!(schema.contributors[0].name, "Jane Doe");
        assert_eq!(schema.imports.len(), 1);
    }

    // ========== ClassDefinition Tests ==========

    #[test]
    fn class_definition_new_creates_minimal_class() {
        let class = ClassDefinition::new("Person");
        assert_eq!(class.name, "Person");
        assert!(class.is_a.is_none());
        assert!(class.mixins.is_empty());
        assert!(!class.r#abstract);
    }

    #[test]
    fn class_definition_deserializes_deprecated() {
        // The `deprecated` common-metadata note parses from LinkML YAML
        // into its `Option<String>` field and is absent when unset. The
        // payload is the migration guidance shown on the card and the
        // signal that drives the RDF `owl:deprecated` axiom.
        let yaml = "
name: LegacyPerson
deprecated: use Person instead
";
        let class: ClassDefinition = serde_norway::from_str(yaml).unwrap();
        assert_eq!(class.deprecated.as_deref(), Some("use Person instead"));

        let bare: ClassDefinition = serde_norway::from_str("name: Person").unwrap();
        assert!(bare.deprecated.is_none());

        // Set notes serialize; unset ones are skipped.
        let out = serde_norway::to_string(&class).unwrap();
        assert!(
            out.contains("deprecated: use Person instead"),
            "got:\n{out}"
        );
        let bare_out = serde_norway::to_string(&bare).unwrap();
        assert!(!bare_out.contains("deprecated:"), "got:\n{bare_out}");
    }

    #[test]
    fn class_definition_deserializes_aliases_and_see_also() {
        // The `aliases` and `see_also` common-metadata lists parse from
        // LinkML YAML into their `Vec<String>` fields and are empty when
        // unset. `aliases` carries alternative names shown on the card;
        // `see_also` carries URIorCURIE references rendered as links and
        // emitted to RDF as `rdfs:seeAlso`.
        let yaml = "
name: Person
aliases:
  - Human
  - Individual
see_also:
  - schema:Person
  - https://example.org/person
";
        let class: ClassDefinition = serde_norway::from_str(yaml).unwrap();
        assert_eq!(class.aliases, vec!["Human", "Individual"]);
        assert_eq!(
            class.see_also,
            vec!["schema:Person", "https://example.org/person"]
        );

        let bare: ClassDefinition = serde_norway::from_str("name: Person").unwrap();
        assert!(bare.aliases.is_empty());
        assert!(bare.see_also.is_empty());

        // Populated lists serialize; empty ones are skipped.
        let out = serde_norway::to_string(&class).unwrap();
        assert!(out.contains("aliases:"), "got:\n{out}");
        assert!(out.contains("see_also:"), "got:\n{out}");
        let bare_out = serde_norway::to_string(&bare).unwrap();
        assert!(!bare_out.contains("aliases:"), "got:\n{bare_out}");
        assert!(!bare_out.contains("see_also:"), "got:\n{bare_out}");
    }

    #[test]
    fn class_definition_deserializes_examples() {
        // The `examples` common-metadata list parses from LinkML YAML
        // into `Vec<Example>`. Each entry carries a `value` and an
        // optional `description`; an entry with no `description` parses
        // to `None`. The list is empty when unset, and an empty list is
        // skipped on serialization.
        let yaml = "
name: Region
examples:
  - value: us-east-1
    description: an AWS region
  - value: eastus
";
        let class: ClassDefinition = serde_norway::from_str(yaml).unwrap();
        assert_eq!(class.examples.len(), 2);
        assert_eq!(class.examples[0].value, "us-east-1");
        assert_eq!(
            class.examples[0].description.as_deref(),
            Some("an AWS region")
        );
        assert_eq!(class.examples[1].value, "eastus");
        assert!(class.examples[1].description.is_none());

        let bare: ClassDefinition = serde_norway::from_str("name: Region").unwrap();
        assert!(bare.examples.is_empty());

        let out = serde_norway::to_string(&class).unwrap();
        assert!(out.contains("examples:"), "got:\n{out}");
        let bare_out = serde_norway::to_string(&bare).unwrap();
        assert!(!bare_out.contains("examples:"), "got:\n{bare_out}");
    }

    #[test]
    fn class_definition_deserializes_rules() {
        // A `rules` entry's `preconditions` / `postconditions` each carry a
        // `slot_conditions` map: slot name -> the constraint subset
        // panschema renders elsewhere (`range` / `required` / cardinality /
        // value bounds / `pattern`), plus `equals_string` / `equals_number`
        // — the equality checks a precondition like "`status` = `actual`"
        // needs, since none of the other fields express equality.
        let yaml = "
name: Deployment
rules:
  - title: actual deployments are located
    description: an actual deployment must name its environment and provider
    preconditions:
      slot_conditions:
        status:
          equals_string: actual
    postconditions:
      slot_conditions:
        in_environment:
          required: true
        on_provider:
          required: true
";
        let class: ClassDefinition = serde_norway::from_str(yaml).unwrap();
        assert_eq!(class.rules.len(), 1);
        let rule = &class.rules[0];
        assert_eq!(
            rule.title.as_deref(),
            Some("actual deployments are located")
        );
        assert_eq!(
            rule.description.as_deref(),
            Some("an actual deployment must name its environment and provider")
        );

        let pre = rule.preconditions.as_ref().expect("preconditions");
        let status_cond = pre.slot_conditions.get("status").expect("status cond");
        assert_eq!(status_cond.equals_string.as_deref(), Some("actual"));

        let post = rule.postconditions.as_ref().expect("postconditions");
        assert!(post.slot_conditions.get("in_environment").unwrap().required);
        assert!(post.slot_conditions.get("on_provider").unwrap().required);

        let bare: ClassDefinition = serde_norway::from_str("name: Deployment").unwrap();
        assert!(bare.rules.is_empty());

        let out = serde_norway::to_string(&class).unwrap();
        assert!(out.contains("rules:"), "got:\n{out}");
        let bare_out = serde_norway::to_string(&bare).unwrap();
        assert!(!bare_out.contains("rules:"), "got:\n{bare_out}");
    }

    #[test]
    fn rule_conditions_deserialize_value_presence_and_any_of() {
        // A real-world `ImageApproval` shape: an `any_of` precondition (the
        // rule fires when `verdict` is any of several values) and
        // `value_presence` postconditions (a slot must be present once the
        // rule applies). Both were silently dropped before being modeled.
        let yaml = "
name: ImageApproval
rules:
  - title: approved or rejected images are attributed
    preconditions:
      any_of:
        - slot_conditions:
            verdict:
              equals_string: approved
        - slot_conditions:
            verdict:
              equals_string: rejected
    postconditions:
      slot_conditions:
        approved_by:
          value_presence: PRESENT
        approved_at:
          value_presence: PRESENT
";
        let class: ClassDefinition = serde_norway::from_str(yaml).unwrap();
        let rule = &class.rules[0];

        // Both `any_of` alternatives are captured, each with its own
        // slot condition.
        let pre = rule.preconditions.as_ref().expect("preconditions");
        assert_eq!(pre.any_of.len(), 2, "both any_of alternatives must parse");
        assert_eq!(
            pre.any_of[0]
                .slot_conditions
                .get("verdict")
                .unwrap()
                .equals_string
                .as_deref(),
            Some("approved")
        );
        assert_eq!(
            pre.any_of[1]
                .slot_conditions
                .get("verdict")
                .unwrap()
                .equals_string
                .as_deref(),
            Some("rejected")
        );

        // The `value_presence` postconditions parse to the modeled enum.
        let post = rule.postconditions.as_ref().expect("postconditions");
        assert_eq!(
            post.slot_conditions
                .get("approved_by")
                .unwrap()
                .value_presence,
            Some(ValuePresence::Present)
        );
        assert_eq!(
            post.slot_conditions
                .get("approved_at")
                .unwrap()
                .value_presence,
            Some(ValuePresence::Present)
        );

        // Round-trips without losing either field.
        let out = serde_norway::to_string(&class).unwrap();
        assert!(
            out.contains("any_of:"),
            "any_of must round-trip; got:\n{out}"
        );
        assert!(
            out.contains("PRESENT"),
            "value_presence must round-trip; got:\n{out}"
        );
    }

    #[test]
    fn slot_condition_deserializes_slot_level_any_of() {
        // The real dogfood shape: `any_of` *inside* a slot condition (the
        // slot's value is any of several), distinct from `any_of` on the
        // whole condition set. Verbatim from a consumer schema's rule.
        let yaml = "
name: ImageApproval
rules:
  - preconditions:
      slot_conditions:
        verdict:
          any_of:
            - equals_string: approved
            - equals_string: rejected
    postconditions:
      slot_conditions:
        approved_by:
          value_presence: PRESENT
";
        let class: ClassDefinition = serde_norway::from_str(yaml).unwrap();
        let pre = class.rules[0].preconditions.as_ref().unwrap();
        let verdict = pre.slot_conditions.get("verdict").unwrap();
        assert_eq!(
            verdict.any_of.len(),
            2,
            "both slot-level alternatives parse"
        );
        assert_eq!(verdict.any_of[0].equals_string.as_deref(), Some("approved"));
        assert_eq!(verdict.any_of[1].equals_string.as_deref(), Some("rejected"));

        // Round-trips without dropping the nested any_of.
        let out = serde_norway::to_string(&class).unwrap();
        assert!(
            out.contains("any_of:"),
            "slot-level any_of must round-trip; got:\n{out}"
        );
    }

    #[test]
    fn class_definition_deserializes_unique_keys() {
        // A `unique_keys` map parses into `BTreeMap<String, UniqueKey>`,
        // each key naming its `unique_key_slots` tuple and an optional
        // `description`. The map is empty when unset, and an empty map is
        // skipped on serialization.
        let yaml = "
name: Offering
unique_keys:
  service_provider_key:
    description: an offering is unique per service type and provider
    unique_key_slots:
      - service_type
      - offered_by
  name_key:
    unique_key_slots:
      - name
";
        let class: ClassDefinition = serde_norway::from_str(yaml).unwrap();
        assert_eq!(class.unique_keys.len(), 2);

        let spk = class
            .unique_keys
            .get("service_provider_key")
            .expect("service_provider_key");
        assert_eq!(
            spk.description.as_deref(),
            Some("an offering is unique per service type and provider")
        );
        assert_eq!(spk.unique_key_slots, vec!["service_type", "offered_by"]);

        let nk = class.unique_keys.get("name_key").expect("name_key");
        assert_eq!(nk.unique_key_slots, vec!["name"]);
        assert!(nk.description.is_none());

        let bare: ClassDefinition = serde_norway::from_str("name: Offering").unwrap();
        assert!(bare.unique_keys.is_empty());

        let out = serde_norway::to_string(&class).unwrap();
        assert!(out.contains("unique_keys:"), "got:\n{out}");
        let bare_out = serde_norway::to_string(&bare).unwrap();
        assert!(!bare_out.contains("unique_keys:"), "got:\n{bare_out}");
    }

    #[test]
    fn class_definition_with_inheritance() {
        let mut class = ClassDefinition::new("Dog");
        class.is_a = Some("Animal".to_string());
        class.description = Some("A domesticated canine".to_string());

        assert_eq!(class.is_a, Some("Animal".to_string()));
        assert_eq!(class.display_label(), "Dog");
    }

    #[test]
    fn class_definition_with_mixins() {
        let mut class = ClassDefinition::new("Person");
        class.mixins = vec!["Named".to_string(), "Aged".to_string()];

        assert_eq!(class.mixins.len(), 2);
        assert!(class.mixins.contains(&"Named".to_string()));
    }

    #[test]
    fn class_definition_serializes_correctly() {
        let mut class = ClassDefinition::new("Animal");
        class.description = Some("A living creature".to_string());
        class.r#abstract = true;

        let yaml = serde_norway::to_string(&class).unwrap();
        assert!(yaml.contains("name: Animal"));
        assert!(yaml.contains("abstract: true"));
    }

    // ========== SlotDefinition Tests ==========

    #[test]
    fn slot_definition_new_creates_minimal_slot() {
        let slot = SlotDefinition::new("name");
        assert_eq!(slot.name, "name");
        assert!(slot.range.is_none());
        assert!(!slot.required);
        assert!(!slot.multivalued);
    }

    #[test]
    fn slot_definition_with_range_and_constraints() {
        let mut slot = SlotDefinition::new("age");
        slot.range = Some("integer".to_string());
        slot.required = true;
        slot.description = Some("The age in years".to_string());

        assert_eq!(slot.range, Some("integer".to_string()));
        assert!(slot.required);
        assert_eq!(slot.display_label(), "age");
    }

    #[test]
    fn slot_definition_with_cardinality() {
        let mut slot = SlotDefinition::new("friends");
        slot.multivalued = true;
        slot.minimum_cardinality = Some(0);
        slot.maximum_cardinality = Some(10);

        assert!(slot.multivalued);
        assert_eq!(slot.minimum_cardinality, Some(0));
        assert_eq!(slot.maximum_cardinality, Some(10));
    }

    #[test]
    fn slot_definition_with_inverse() {
        let mut slot = SlotDefinition::new("has_owner");
        slot.range = Some("Person".to_string());
        slot.inverse = Some("owns".to_string());

        assert_eq!(slot.inverse, Some("owns".to_string()));
    }

    #[test]
    fn slot_definition_deserializes_owl_characteristics() {
        // The five OWL relationship metaslots parse from LinkML YAML into
        // their bool fields (serde-derived) and default to false when
        // absent — the IR-level contract the HTML badge and RDF axiom
        // tests build on but don't exercise (they construct in-memory).
        let yaml = "
name: refines
range: Claim
transitive: true
symmetric: true
";
        let slot: SlotDefinition = serde_norway::from_str(yaml).unwrap();
        assert!(slot.transitive);
        assert!(slot.symmetric);
        assert!(!slot.asymmetric);
        assert!(!slot.reflexive);
        assert!(!slot.irreflexive);

        // default-false characteristics are skipped on serialize; set ones survive.
        let out = serde_norway::to_string(&slot).unwrap();
        assert!(
            out.contains("transitive: true"),
            "set flag must serialize:\n{out}"
        );
        assert!(
            !out.contains("reflexive:"),
            "default-false flags must be skipped:\n{out}"
        );
    }

    #[test]
    fn slot_definition_deserializes_value_bounds() {
        // `minimum_value` / `maximum_value` parse into `Option<f64>` and
        // are absent when unset.
        let yaml = "
name: strength
range: float
minimum_value: 0.0
maximum_value: 1.0
";
        let slot: SlotDefinition = serde_norway::from_str(yaml).unwrap();
        assert_eq!(slot.minimum_value, Some(0.0));
        assert_eq!(slot.maximum_value, Some(1.0));

        let bare: SlotDefinition = serde_norway::from_str("name: x").unwrap();
        assert!(bare.minimum_value.is_none() && bare.maximum_value.is_none());
    }

    #[test]
    fn slot_definition_deserializes_ifabsent() {
        // `ifabsent` parses from LinkML YAML into its `Option<String>`
        // field, carrying the expression verbatim, and is absent when
        // unset. The Rust codegen parses the form to emit a default; the
        // IR stores it unchanged.
        let yaml = "
name: status
range: ItemStatus
ifabsent: ItemStatus(planned)
";
        let slot: SlotDefinition = serde_norway::from_str(yaml).unwrap();
        assert_eq!(slot.ifabsent.as_deref(), Some("ItemStatus(planned)"));

        let bare: SlotDefinition = serde_norway::from_str("name: x").unwrap();
        assert!(bare.ifabsent.is_none());

        // Set values serialize; unset ones are skipped.
        let out = serde_norway::to_string(&slot).unwrap();
        assert!(out.contains("ifabsent: ItemStatus(planned)"), "got:\n{out}");
        let bare_out = serde_norway::to_string(&bare).unwrap();
        assert!(!bare_out.contains("ifabsent:"), "got:\n{bare_out}");
    }

    // ========== EnumDefinition Tests ==========

    #[test]
    fn enum_definition_new_creates_minimal_enum() {
        let enum_def = EnumDefinition::new("Color");
        assert_eq!(enum_def.name, "Color");
        assert!(enum_def.permissible_values.is_empty());
    }

    #[test]
    fn enum_definition_with_values() {
        let mut enum_def = EnumDefinition::new("Status");
        enum_def
            .permissible_values
            .insert("active".to_string(), PermissibleValue::new("active"));
        enum_def.permissible_values.insert("inactive".to_string(), {
            let mut pv = PermissibleValue::new("inactive");
            pv.description = Some("No longer active".to_string());
            pv
        });

        assert_eq!(enum_def.permissible_values.len(), 2);
        assert!(enum_def.permissible_values.contains_key("active"));
    }

    // ========== TypeDefinition Tests ==========

    #[test]
    fn type_definition_new_creates_minimal_type() {
        let type_def = TypeDefinition::new("age_type");
        assert_eq!(type_def.name, "age_type");
        assert!(type_def.uri.is_none());
    }

    #[test]
    fn type_definition_with_uri() {
        let mut type_def = TypeDefinition::new("string");
        type_def.uri = Some("xsd:string".to_string());
        type_def.description = Some("A character string".to_string());

        assert_eq!(type_def.uri, Some("xsd:string".to_string()));
    }

    // ========== Annotation Tests ==========

    #[test]
    fn schema_preserves_source_format_annotation() {
        let mut schema = SchemaDefinition::new("test");
        schema
            .annotations
            .insert("panschema:source_format".to_string(), "owl".to_string());

        assert_eq!(
            schema.annotations.get_str("panschema:source_format"),
            Some("owl")
        );
    }

    #[test]
    fn class_preserves_owl_specific_annotations() {
        let mut class = ClassDefinition::new("Person");
        class.annotations.insert(
            "panschema:owl_class_iri".to_string(),
            "http://example.org/Person".to_string(),
        );

        let yaml = serde_norway::to_string(&class).unwrap();
        assert!(yaml.contains("panschema:owl_class_iri"));
    }

    /// Loads a schema whose fixture slot `s1` carries the given
    /// `annotations:` block, returning that slot's annotations — the
    /// shared scaffold for the annotation contract tests.
    fn slot_annotations(body: &str) -> Annotations {
        let schema: SchemaDefinition = serde_norway::from_str(&format!(
            "id: https://example.org/t\nname: t\nslots:\n  s1:\n    annotations:\n{body}"
        ))
        .unwrap_or_else(|e| panic!("annotations must load: {e}\n{body}"));
        schema.slots["s1"].annotations.clone()
    }

    /// The parse error for a schema whose fixture slot's `annotations:`
    /// block is refused — the contract tests' other half.
    fn slot_annotations_err(body: &str) -> String {
        serde_norway::from_str::<SchemaDefinition>(&format!(
            "id: https://example.org/t\nname: t\nslots:\n  s1:\n    annotations:\n{body}"
        ))
        .expect_err("the annotations block is invalid")
        .to_string()
    }

    /// LinkML 1.6 widened an annotation's value from `string` to any
    /// object: a structured value rides under `value:` (the spelling the
    /// reference implementation reads) and is preserved whole, typed
    /// leaves and all, so a schema declaring semantics panschema does not
    /// itself interpret is still readable by whoever does.
    #[test]
    fn a_structured_annotation_value_loads_and_is_preserved() {
        let annotations = slot_annotations(
            "      review_status:\n        value:\n          stage: draft\n          priority: 2\n",
        );
        let value = annotations
            .get("review_status")
            .expect("the annotation is kept under its tag");
        assert_eq!(
            value.get("stage").and_then(|v| v.as_str()),
            Some("draft"),
            "the structured value's fields survive the load; got: {value:?}"
        );
        assert_eq!(
            value.get("priority").and_then(|v| v.as_i64()),
            Some(2),
            "a non-string leaf inside a structure keeps its type; got: {value:?}"
        );
    }

    /// The metamodel's three spellings of the same annotation — the
    /// compact `tag: value` sugar, the expanded map keyed by tag, and the
    /// expanded list of `tag`/`value` objects — all denote one tag→value
    /// map, so all three load to the same annotations.
    #[test]
    fn the_expanded_annotation_forms_load_as_the_compact_form_does() {
        let compact = slot_annotations("      note: hello\n");
        assert_eq!(
            compact.get_str("note"),
            Some("hello"),
            "the compact form reads as a string"
        );
        assert_eq!(
            slot_annotations("      note:\n        tag: note\n        value: hello\n"),
            compact,
            "the expanded map form denotes the same annotation"
        );
        assert_eq!(
            slot_annotations("      - tag: note\n        value: hello\n"),
            compact,
            "the expanded list form denotes the same annotation"
        );
    }

    /// The `panschema:*` tags the tool itself reads are string-valued,
    /// and `get_str` is how every reader asks for one. A bare scalar
    /// reads as its lexical string — an unquoted `panschema:label: 2024`
    /// means the label "2024" — while a structured value is not a string
    /// and must not masquerade as one.
    #[test]
    fn get_str_reads_scalars_lexically_and_structures_as_absent() {
        let annotations = slot_annotations(
            "      panschema:label: Human\n      year: 2024\n      flag: true\n      ratio: 3.5\n      structured:\n        value:\n          a: b\n",
        );
        assert_eq!(annotations.get_str("panschema:label"), Some("Human"));
        assert_eq!(
            annotations.get_str("year"),
            Some("2024"),
            "an unquoted number reads as its lexical string"
        );
        assert_eq!(annotations.get_str("flag"), Some("true"));
        assert_eq!(annotations.get_str("ratio"), Some("3.5"));
        assert_eq!(
            annotations.get_str("structured"),
            None,
            "a structured value is not a string"
        );
    }

    /// A mapping under a tag is always the metamodel's `Annotation`
    /// body, exactly as the reference implementation reads it — so a
    /// schema panschema loads means the same thing to the Python
    /// toolchain. A body key outside the model is refused with the
    /// `value:`-wrapped spelling shown, and a body `tag` disagreeing
    /// with the key it is filed under is refused too.
    #[test]
    fn an_annotation_body_is_validated_as_the_reference_implementation_reads_it() {
        let foreign_key =
            slot_annotations_err("      note:\n        value: something\n        extra: true\n");
        assert!(
            foreign_key.contains("`extra`") && foreign_key.contains("{value: {...}}"),
            "a foreign body key is refused, pointing at the wrapped spelling; got: {foreign_key}"
        );

        let contradicting_tag =
            slot_annotations_err("      note:\n        tag: elsewhere\n        value: hello\n");
        assert!(
            contradicting_tag.contains("`note`") && contradicting_tag.contains("elsewhere"),
            "a contradicting body tag is refused, naming both; got: {contradicting_tag}"
        );
    }

    /// A body without `value:` reads as null — the reference
    /// implementation's reading — in both expanded spellings.
    #[test]
    fn an_annotation_body_without_a_value_reads_as_null() {
        for body in ["      note:\n        tag: note\n", "      - tag: note\n"] {
            let annotations = slot_annotations(body);
            assert_eq!(
                annotations.get("note"),
                Some(&serde_norway::Value::Null),
                "the tag is present with a null value; body:\n{body}"
            );
            assert_eq!(annotations.get_str("note"), None);
        }
    }

    /// Nested `annotations`/`extensions` on an annotation are not
    /// modeled: the annotation's own value is kept, and the dropped
    /// nesting is recorded so load diagnostics can report it instead of
    /// the drop being silent.
    #[test]
    fn nested_annotations_are_dropped_loudly_not_silently() {
        let annotations = slot_annotations(
            "      note:\n        value: hello\n        annotations:\n          provenance: curated\n",
        );
        assert_eq!(
            annotations.get_str("note"),
            Some("hello"),
            "the annotation's own value is kept"
        );
        assert_eq!(
            annotations
                .tags_with_unmodeled_nesting()
                .collect::<Vec<_>>(),
            vec!["note"],
            "the dropped nesting is recorded for diagnostics"
        );
        let plain = slot_annotations("      note: hello\n");
        assert_eq!(
            plain.tags_with_unmodeled_nesting().count(),
            0,
            "an annotation without nesting records nothing"
        );
    }

    /// Annotations survive a serialization round trip — a structured
    /// value re-emits under `value:`, the one spelling that re-reads
    /// identically here and loads in the Python toolchain — and an
    /// element carrying none serializes without the key rather than with
    /// an empty map.
    #[test]
    fn annotations_round_trip_and_an_element_without_them_omits_the_key() {
        let schema: SchemaDefinition = serde_norway::from_str(
            "id: https://example.org/t\nname: t\nclasses:\n  Plain:\n    description: none here\n  \
             Marked:\n    annotations:\n      panschema:label: Human\n      review_status:\n        \
             value:\n          stage: draft\n      threshold:\n        value:\n          value: 5\n      \
             nested:\n        value: hello\n        annotations:\n          p: c\n",
        )
        .expect("parse");
        let yaml = serde_norway::to_string(&schema).expect("serialize");
        assert!(
            !yaml.contains("annotations: {}"),
            "an element with no annotations omits the key; got:\n{yaml}"
        );
        let restored: SchemaDefinition = serde_norway::from_str(&yaml).expect("re-read");
        assert_eq!(
            restored, schema,
            "string and structured annotations survive the round trip, including a \
             value that itself looks like an expanded body"
        );
    }

    /// An `annotations:` key with nothing under it — every entry
    /// commented out, or an explicit null — is no annotations, as the
    /// reference implementation also reads it.
    #[test]
    fn an_empty_annotations_key_loads_as_no_annotations() {
        let schema: SchemaDefinition = serde_norway::from_str(
            "id: https://example.org/t\nname: t\nslots:\n  s1:\n    annotations:\nclasses:\n  \
             C:\n    annotations: null\n",
        )
        .expect("a bare annotations key is valid");
        assert!(schema.slots["s1"].annotations.is_empty());
        assert!(schema.classes["C"].annotations.is_empty());
    }

    /// A YAML-tagged value is refused wherever it stands for an
    /// annotation's value — bare or under `value:` — because the
    /// reference implementation refuses the unknown tag, and a schema
    /// that loads here must load there.
    #[test]
    fn a_yaml_tagged_annotation_value_is_refused() {
        for body in [
            "      note: !custom {stage: draft}\n",
            "      note:\n        value: !custom {stage: draft}\n",
        ] {
            let err = slot_annotations_err(body);
            assert!(
                err.contains("YAML-tagged"),
                "a tagged value is refused, naming why; body:\n{body}got: {err}"
            );
        }
    }

    /// Equality is over tag→value content only: the unmodeled-nesting
    /// record is load bookkeeping, so two definitions panschema models
    /// identically compare equal — the imports merge must not report a
    /// collision because one spelling carried nesting the model drops.
    #[test]
    fn nesting_bookkeeping_does_not_make_equal_annotations_unequal() {
        let with_nesting = slot_annotations(
            "      note:\n        value: hello\n        annotations:\n          p: c\n",
        );
        let without = slot_annotations("      note: hello\n");
        assert!(
            with_nesting.tags_with_unmodeled_nesting().count() > 0,
            "the fixture actually records nesting"
        );
        assert_eq!(
            with_nesting, without,
            "identically-modeled annotations are equal whatever their spelling carried"
        );
        assert_ne!(
            without,
            slot_annotations("      note: goodbye\n"),
            "differing values still compare unequal"
        );
    }

    /// The nesting record always describes the value actually held:
    /// overwriting a tag — a later duplicate in list form, or a plain
    /// insert — and removing a tag both clear it, so a diagnostic can
    /// never name an annotation the map no longer carries in that shape.
    #[test]
    fn the_nesting_record_follows_the_value_it_describes() {
        let overwritten = slot_annotations(
            "      - tag: note\n        value: x\n        annotations:\n          p: c\n      \
             - tag: note\n        value: y\n",
        );
        assert_eq!(overwritten.get_str("note"), Some("y"));
        assert_eq!(
            overwritten.tags_with_unmodeled_nesting().count(),
            0,
            "the surviving duplicate carried no nesting, so none is recorded"
        );

        let mut annotations = slot_annotations(
            "      note:\n        value: hello\n        annotations:\n          p: c\n",
        );
        annotations.remove("note");
        assert_eq!(
            annotations.tags_with_unmodeled_nesting().count(),
            0,
            "removing the annotation removes its nesting record"
        );

        let mut annotations = slot_annotations(
            "      note:\n        value: hello\n        annotations:\n          p: c\n",
        );
        annotations.insert("note", "rewritten");
        assert_eq!(
            annotations.tags_with_unmodeled_nesting().count(),
            0,
            "overwriting the annotation clears its nesting record"
        );
    }

    /// Annotations are a map or a list of tag/value objects; a scalar
    /// where they belong fails the load saying so, rather than being
    /// quietly read as nothing.
    #[test]
    fn a_scalar_where_annotations_belong_fails_the_load_naming_the_shapes() {
        let err = serde_norway::from_str::<SchemaDefinition>(
            "id: https://example.org/t\nname: t\nslots:\n  s1:\n    annotations: 42\n",
        )
        .expect_err("a scalar is not annotations");
        let message = err.to_string();
        assert!(
            message.contains("tag-keyed map") && message.contains("list"),
            "the error names the shapes annotations may take; got: {message}"
        );
    }

    // ========== Round-trip Tests ==========

    #[test]
    fn schema_roundtrip_yaml() {
        let mut schema = SchemaDefinition::new("roundtrip_test");
        schema.id = Some("https://example.org/roundtrip".to_string());
        schema
            .prefixes
            .insert("ex".to_string(), "https://example.org/".to_string());

        let mut animal = ClassDefinition::new("Animal");
        animal.description = Some("A living thing".to_string());
        schema.classes.insert("Animal".to_string(), animal);

        let mut name_slot = SlotDefinition::new("name");
        name_slot.range = Some("string".to_string());
        name_slot.required = true;
        schema.slots.insert("name".to_string(), name_slot);

        // Serialize
        let yaml = serde_norway::to_string(&schema).unwrap();

        // Deserialize
        let restored: SchemaDefinition = serde_norway::from_str(&yaml).unwrap();

        assert_eq!(schema, restored);
    }
}
