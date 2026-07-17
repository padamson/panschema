//! Graph data types for visualization
//!
//! These types mirror the ones in panschema::graph_writer but are defined here
//! to avoid WASM compilation issues with panschema's native dependencies.

use serde::{Deserialize, Serialize};

/// Color constants for node types (RGBA, normalized 0.0-1.0)
#[allow(dead_code)]
pub mod colors {
    /// Class nodes: Blue (#4A90D9)
    pub const CLASS: [f32; 4] = [0.290, 0.565, 0.851, 1.0];

    /// Slot nodes: Green (#50C878)
    pub const SLOT: [f32; 4] = [0.314, 0.784, 0.471, 1.0];

    /// Enum nodes: Purple (#9B59B6)
    pub const ENUM: [f32; 4] = [0.608, 0.349, 0.714, 1.0];

    /// Type nodes: Orange (#E67E22)
    pub const TYPE: [f32; 4] = [0.902, 0.494, 0.133, 1.0];

    /// Individual (A-box instance) nodes: Teal (#29B8B3). Mirrors the
    /// writer-side constant so the instance graph reads apart from the
    /// T-box kinds.
    pub const INDIVIDUAL: [f32; 4] = [0.161, 0.722, 0.702, 1.0];

    /// Alpha value for abstract classes
    pub const ABSTRACT_ALPHA: f32 = 0.7;
}

/// Node type enumeration for semantic categorization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Class,
    Slot,
    Enum,
    Type,
    /// An OWL individual (A-box instance) — drawn only in the instance
    /// graph, never the schema graph.
    Individual,
}

impl NodeType {
    /// Get the default color for this node type
    #[allow(dead_code)]
    pub fn color(&self) -> [f32; 4] {
        match self {
            NodeType::Class => colors::CLASS,
            NodeType::Slot => colors::SLOT,
            NodeType::Enum => colors::ENUM,
            NodeType::Type => colors::TYPE,
            NodeType::Individual => colors::INDIVIDUAL,
        }
    }
}

/// Edge type enumeration for semantic categorization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    /// Class inheritance (is_a relationship)
    SubclassOf,
    /// Mixin inheritance
    Mixin,
    /// Property domain (slot -> class)
    Domain,
    /// Property range (slot -> class/type/enum)
    Range,
    /// Inverse property relationship
    Inverse,
    /// Type inheritance (typeof_)
    TypeOf,
    /// Object-property assertion between two individuals (instance graph).
    Assertion,
}

/// A node in the graph representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    /// Unique identifier for the node (e.g., "class:Animal", "slot:hasOwner")
    pub id: String,

    /// Human-readable label for display
    pub label: String,

    /// Node type determines rendering (color, shape, etc.)
    pub node_type: NodeType,

    /// RGBA color as normalized floats (matches NodeInstance.color)
    pub color: [f32; 4],

    /// Optional description/tooltip
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Optional URI for linking (a curie with a known prefix arrives
    /// already expanded; see [`GraphNode::uri_unresolved`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    /// True when `uri` is a curie whose prefix wasn't declared and so
    /// couldn't be expanded — the hover card marks it with a `?`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub uri_unresolved: bool,

    /// Whether this is an abstract class (visual indicator)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_abstract: bool,

    /// Resolved per-kind metadata for the hover-card's structured
    /// view: slots / parents / mixins for classes; domain / range /
    /// required / multivalued for slots; permissible values for
    /// enums. Populated by `GraphWriter` from the LinkML IR — the
    /// visualization layer never walks the IR itself. `None` for
    /// kinds whose extra payload would be empty (e.g. types).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind_metadata: Option<KindMetadata>,
}

/// Per-kind structured payload carried by [`GraphNode::kind_metadata`].
/// Tagged with `serde(tag = "kind")` so the wire format reads
/// `{"kind": "class", "slots": [...], ...}` — the JS hover card
/// dispatches on the tag to render the right rows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum KindMetadata {
    /// Resolved view of a LinkML class: every slot reachable via
    /// direct attributes / `slots:` references / `is_a` chain /
    /// `mixins:` list — each with its effective shape — plus the
    /// immediate parents and mixins for the inheritance view.
    Class {
        slots: Vec<SlotSummary>,
        parents: Vec<String>,
        mixins: Vec<String>,
        /// The class's conditional rules, each carrying the slots it
        /// touches split into trigger (precondition) and governed
        /// (postcondition). Mirrors the writer side; a governed slot's
        /// node draws a marker glyph derived from these lists.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        rules: Vec<RuleSummary>,
    },
    /// Resolved view of a LinkML slot. `required` / `multivalued`
    /// are the effective-cardinality reconciliation of the bool
    /// flags with the explicit `min` / `max` bounds.
    Slot {
        /// Every class this slot is a domain of; a slot can belong to
        /// several classes. Mirrors the writer side.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        domains: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        range: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        required: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        multivalued: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pattern: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        identifier: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        any_of: Vec<String>,
    },
    /// Permissible values for a LinkML enum, in declaration order —
    /// each with its optional description and curie-expanded meaning.
    Enum {
        permissible_values: Vec<PermissibleValueSummary>,
    },
    /// An OWL individual in the instance graph: the class ids it is an
    /// instance of plus its literal-valued property assertions (object
    /// assertions are edges instead). Mirrors the writer side.
    Individual {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        types: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        literals: Vec<PropertyLiteral>,
    },
}

/// A literal-valued property assertion on an individual, shown on the
/// instance node's hover. Mirrors the writer-side struct.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PropertyLiteral {
    pub property: String,
    pub value: String,
}

/// One class rule in the graph metadata — its rendered summary plus
/// the slots it touches, split into trigger (precondition) and governed
/// (postcondition) sides. Mirrors the writer-side struct so the JSON
/// round-trips; the viz uses `governed_slots` to place governed-slot
/// marker glyphs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuleSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trigger_slots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub governed_slots: Vec<String>,
}

/// One permissible value of an enum in the hover card. Mirrors the
/// writer-side struct so the JSON round-trips.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PermissibleValueSummary {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meaning: Option<String>,
}

/// One slot in a class's resolved view — the effective shape after
/// `slot_usage` overlay and cardinality reconciliation, mirrored
/// field-for-field from the writer side so the JSON round-trips.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SlotSummary {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub multivalued: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
    /// Where an inherited slot came from (e.g. `"mixin Named"`);
    /// `None` for the class's own slots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

/// An edge connecting two nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    /// Source node ID
    pub source: String,

    /// Target node ID
    pub target: String,

    /// Edge type determines rendering (color, style, etc.)
    pub edge_type: EdgeType,

    /// Optional label for the edge
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Complete graph data for serialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphData {
    /// Schema name identifier
    pub schema_name: String,

    /// Optional schema title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_title: Option<String>,

    /// All nodes in the graph
    pub nodes: Vec<GraphNode>,

    /// All edges in the graph
    pub edges: Vec<GraphEdge>,

    /// Version of the graph format (for future compatibility)
    pub format_version: String,
}

#[allow(dead_code)]
impl GraphData {
    /// Format version constant
    pub const FORMAT_VERSION: &'static str = "1.0";

    /// Create new GraphData with metadata
    pub fn new(schema_name: String, schema_title: Option<String>) -> Self {
        Self {
            schema_name,
            schema_title,
            nodes: Vec::new(),
            edges: Vec::new(),
            format_version: Self::FORMAT_VERSION.to_string(),
        }
    }
}
