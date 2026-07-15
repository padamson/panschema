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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
}

impl SchemaDefinition {
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
            annotations: BTreeMap::new(),
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
/// `equals_number` — the equality checks a precondition like "`status` =
/// `actual`" needs, since none of the other fields express equality.
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
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
    pub unmodeled: BTreeMap<String, serde_yaml::Value>,
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
            annotations: BTreeMap::new(),
            rules: Vec::new(),
            unique_keys: BTreeMap::new(),
        }
    }

    /// Returns the display label (name is always available in LinkML)
    pub fn display_label(&self) -> &str {
        &self.name
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
    /// URI for semantic interpretation (e.g., owl:ObjectProperty IRI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_uri: Option<String>,
    /// Inverse slot (for bidirectional relationships)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inverse: Option<String>,
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
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
            minimum_cardinality: None,
            maximum_cardinality: None,
            pattern: None,
            identifier: false,
            slot_uri: None,
            inverse: None,
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
            annotations: BTreeMap::new(),
        }
    }

    /// Returns the display label (name is always available in LinkML)
    pub fn display_label(&self) -> &str {
        &self.name
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
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
            annotations: BTreeMap::new(),
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
    /// Parent type (for type inheritance)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typeof_: Option<String>,
    /// URI for the underlying datatype (e.g., xsd:string)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// Regular expression pattern for validation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// Format-specific annotations
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
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
            annotations: BTreeMap::new(),
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

        let yaml = serde_yaml::to_string(&schema).unwrap();
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
        let class: ClassDefinition = serde_yaml::from_str(yaml).unwrap();
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
        let yaml = serde_yaml::to_string(&slot).unwrap();
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
        let yaml = serde_yaml::to_string(&slot).unwrap();
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
        let schema: SchemaDefinition = serde_yaml::from_str(yaml).unwrap();
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

        let yaml = serde_yaml::to_string(&contributor).unwrap();
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

        let yaml = serde_yaml::to_string(&schema).unwrap();
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
        let schema: SchemaDefinition = serde_yaml::from_str(yaml).unwrap();
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
        let class: ClassDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(class.deprecated.as_deref(), Some("use Person instead"));

        let bare: ClassDefinition = serde_yaml::from_str("name: Person").unwrap();
        assert!(bare.deprecated.is_none());

        // Set notes serialize; unset ones are skipped.
        let out = serde_yaml::to_string(&class).unwrap();
        assert!(
            out.contains("deprecated: use Person instead"),
            "got:\n{out}"
        );
        let bare_out = serde_yaml::to_string(&bare).unwrap();
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
        let class: ClassDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(class.aliases, vec!["Human", "Individual"]);
        assert_eq!(
            class.see_also,
            vec!["schema:Person", "https://example.org/person"]
        );

        let bare: ClassDefinition = serde_yaml::from_str("name: Person").unwrap();
        assert!(bare.aliases.is_empty());
        assert!(bare.see_also.is_empty());

        // Populated lists serialize; empty ones are skipped.
        let out = serde_yaml::to_string(&class).unwrap();
        assert!(out.contains("aliases:"), "got:\n{out}");
        assert!(out.contains("see_also:"), "got:\n{out}");
        let bare_out = serde_yaml::to_string(&bare).unwrap();
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
        let class: ClassDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(class.examples.len(), 2);
        assert_eq!(class.examples[0].value, "us-east-1");
        assert_eq!(
            class.examples[0].description.as_deref(),
            Some("an AWS region")
        );
        assert_eq!(class.examples[1].value, "eastus");
        assert!(class.examples[1].description.is_none());

        let bare: ClassDefinition = serde_yaml::from_str("name: Region").unwrap();
        assert!(bare.examples.is_empty());

        let out = serde_yaml::to_string(&class).unwrap();
        assert!(out.contains("examples:"), "got:\n{out}");
        let bare_out = serde_yaml::to_string(&bare).unwrap();
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
        let class: ClassDefinition = serde_yaml::from_str(yaml).unwrap();
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

        let bare: ClassDefinition = serde_yaml::from_str("name: Deployment").unwrap();
        assert!(bare.rules.is_empty());

        let out = serde_yaml::to_string(&class).unwrap();
        assert!(out.contains("rules:"), "got:\n{out}");
        let bare_out = serde_yaml::to_string(&bare).unwrap();
        assert!(!bare_out.contains("rules:"), "got:\n{bare_out}");
    }

    #[test]
    fn rule_conditions_deserialize_value_presence_and_any_of() {
        // cuisineiq's ImageApproval shape: an `any_of` precondition (the
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
        let class: ClassDefinition = serde_yaml::from_str(yaml).unwrap();
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
        let out = serde_yaml::to_string(&class).unwrap();
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
        let class: ClassDefinition = serde_yaml::from_str(yaml).unwrap();
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
        let out = serde_yaml::to_string(&class).unwrap();
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
        let class: ClassDefinition = serde_yaml::from_str(yaml).unwrap();
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

        let bare: ClassDefinition = serde_yaml::from_str("name: Offering").unwrap();
        assert!(bare.unique_keys.is_empty());

        let out = serde_yaml::to_string(&class).unwrap();
        assert!(out.contains("unique_keys:"), "got:\n{out}");
        let bare_out = serde_yaml::to_string(&bare).unwrap();
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

        let yaml = serde_yaml::to_string(&class).unwrap();
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
        let slot: SlotDefinition = serde_yaml::from_str(yaml).unwrap();
        assert!(slot.transitive);
        assert!(slot.symmetric);
        assert!(!slot.asymmetric);
        assert!(!slot.reflexive);
        assert!(!slot.irreflexive);

        // default-false characteristics are skipped on serialize; set ones survive.
        let out = serde_yaml::to_string(&slot).unwrap();
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
        let slot: SlotDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(slot.minimum_value, Some(0.0));
        assert_eq!(slot.maximum_value, Some(1.0));

        let bare: SlotDefinition = serde_yaml::from_str("name: x").unwrap();
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
        let slot: SlotDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(slot.ifabsent.as_deref(), Some("ItemStatus(planned)"));

        let bare: SlotDefinition = serde_yaml::from_str("name: x").unwrap();
        assert!(bare.ifabsent.is_none());

        // Set values serialize; unset ones are skipped.
        let out = serde_yaml::to_string(&slot).unwrap();
        assert!(out.contains("ifabsent: ItemStatus(planned)"), "got:\n{out}");
        let bare_out = serde_yaml::to_string(&bare).unwrap();
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
            schema.annotations.get("panschema:source_format"),
            Some(&"owl".to_string())
        );
    }

    #[test]
    fn class_preserves_owl_specific_annotations() {
        let mut class = ClassDefinition::new("Person");
        class.annotations.insert(
            "panschema:owl_class_iri".to_string(),
            "http://example.org/Person".to_string(),
        );

        let yaml = serde_yaml::to_string(&class).unwrap();
        assert!(yaml.contains("panschema:owl_class_iri"));
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
        let yaml = serde_yaml::to_string(&schema).unwrap();

        // Deserialize
        let restored: SchemaDefinition = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(schema, restored);
    }
}
