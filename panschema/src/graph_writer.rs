//! Graph JSON writer for schema visualization
//!
//! Converts `SchemaDefinition` to JSON graph format for force-directed visualization.
//! Outputs graph topology (nodes and edges) without positions - positions are computed
//! at runtime by the force simulation.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::io::{IoError, IoResult, Writer};
use crate::linkml::SchemaDefinition;

/// Color constants for node types (RGBA, normalized 0.0-1.0)
pub mod colors {
    /// Class nodes: Blue (#4A90D9)
    pub const CLASS: [f32; 4] = [0.290, 0.565, 0.851, 1.0];

    /// Slot nodes: Green (#50C878)
    pub const SLOT: [f32; 4] = [0.314, 0.784, 0.471, 1.0];

    /// Enum nodes: Purple (#9B59B6)
    pub const ENUM: [f32; 4] = [0.608, 0.349, 0.714, 1.0];

    /// Type nodes: Orange (#E67E22)
    pub const TYPE: [f32; 4] = [0.902, 0.494, 0.133, 1.0];

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
}

impl NodeType {
    /// Get the default color for this node type
    pub fn color(&self) -> [f32; 4] {
        match self {
            NodeType::Class => colors::CLASS,
            NodeType::Slot => colors::SLOT,
            NodeType::Enum => colors::ENUM,
            NodeType::Type => colors::TYPE,
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

    /// Optional URI for linking
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    /// Whether this is an abstract class (visual indicator)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_abstract: bool,

    /// Resolved per-kind metadata surfaced by the hover card:
    /// slots/parents/mixins for classes; domain/range/required/
    /// multivalued for slots; permissible values for enums.
    /// Populated here so the visualization layer never has to walk
    /// the LinkML IR itself. `None` for kinds whose payload would
    /// be empty (e.g. types).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind_metadata: Option<KindMetadata>,
}

/// Per-kind structured payload carried by [`GraphNode::kind_metadata`].
/// Tagged with `serde(tag = "kind")` so the wire format reads
/// `{"kind": "class", "slots": [...], ...}` — the JS hover card
/// dispatches on the tag to render the right rows. Mirrors the
/// shape in `panschema_viz::graph_types::KindMetadata` so the two
/// crates can serialize/deserialize the same payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum KindMetadata {
    /// Resolved view of a LinkML class: every slot reachable via
    /// direct attributes / `slots:` references / `is_a` chain /
    /// `mixins:` list, plus the immediate parents and mixins for
    /// the inheritance view.
    Class {
        slots: Vec<String>,
        parents: Vec<String>,
        mixins: Vec<String>,
    },
    /// Resolved view of a LinkML slot.
    Slot {
        #[serde(skip_serializing_if = "Option::is_none")]
        domain: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        range: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        required: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        multivalued: bool,
    },
    /// Permissible values for a LinkML enum, in declaration order.
    Enum { permissible_values: Vec<String> },
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

/// Resolve every slot reachable from `class_name` via its direct
/// `attributes:`, `slots:` references, and (transitively) its
/// `is_a` parent and `mixins:` lists. Duplicates are dropped while
/// preserving the first-seen order so the hover card surfaces
/// direct slots before inherited ones. Cycles in the inheritance
/// graph (which a well-formed LinkML schema doesn't have, but a
/// truncated source might) are guarded by a visited-set.
fn resolve_class_slots(schema: &SchemaDefinition, class_name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen_slots: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut visited_classes: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut stack: Vec<String> = vec![class_name.to_string()];

    while let Some(cls_name) = stack.pop() {
        if !visited_classes.insert(cls_name.clone()) {
            continue;
        }
        let Some(cls) = schema.classes.get(&cls_name) else {
            continue;
        };
        for slot_name in cls.attributes.keys().chain(cls.slots.iter()) {
            if seen_slots.insert(slot_name.clone()) {
                out.push(slot_name.clone());
            }
        }
        if let Some(parent) = &cls.is_a {
            stack.push(parent.clone());
        }
        for mixin in &cls.mixins {
            stack.push(mixin.clone());
        }
    }
    out
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

/// Options for controlling graph generation
#[derive(Debug, Clone)]
pub struct GraphOptions {
    /// Include slot nodes (default: true)
    pub include_slots: bool,

    /// Include enum nodes (default: true)
    pub include_enums: bool,

    /// Include type nodes (default: true)
    pub include_types: bool,

    /// Include domain edges from slots to classes (default: true)
    pub include_domain_edges: bool,

    /// Include range edges from slots to targets (default: true)
    pub include_range_edges: bool,

    /// Include inverse edges between slots (default: true)
    pub include_inverse_edges: bool,
}

impl Default for GraphOptions {
    fn default() -> Self {
        Self {
            include_slots: true,
            include_enums: true,
            include_types: true,
            include_domain_edges: true,
            include_range_edges: true,
            include_inverse_edges: true,
        }
    }
}

impl GraphOptions {
    /// Classes only (no slots, enums, or types)
    pub fn classes_only() -> Self {
        Self {
            include_slots: false,
            include_enums: false,
            include_types: false,
            include_domain_edges: false,
            include_range_edges: false,
            include_inverse_edges: false,
        }
    }
}

/// Writer that outputs schema as graph JSON for visualization
pub struct GraphWriter {
    options: GraphOptions,
}

impl GraphWriter {
    /// Create a new GraphWriter with default options
    pub fn new() -> Self {
        Self {
            options: GraphOptions::default(),
        }
    }

    /// Create a GraphWriter with custom options
    pub fn with_options(options: GraphOptions) -> Self {
        Self { options }
    }

    /// Convert SchemaDefinition to GraphData
    pub fn schema_to_graph(&self, schema: &SchemaDefinition) -> GraphData {
        let mut graph = GraphData::new(schema.name.clone(), schema.title.clone());

        // Add class nodes and inheritance edges
        self.add_classes(schema, &mut graph);

        // Add slot nodes and domain/range edges
        if self.options.include_slots {
            self.add_slots(schema, &mut graph);
        }

        // Add enum nodes
        if self.options.include_enums {
            self.add_enums(schema, &mut graph);
        }

        // Add type nodes and typeof edges
        if self.options.include_types {
            self.add_types(schema, &mut graph);
        }

        // Must run after enum/type nodes exist so resolve_range_target can find them.
        self.add_inline_attribute_edges(schema, &mut graph);

        // Must run after `add_slots` so the slot-side `domain` edges are
        // present for dedup. LinkML treats `slot.domain` and `class.slots`
        // as the same relation — `domain_of` is the computed inverse of
        // `domain:` — so a slot referenced from a class's `slots:` list
        // is connected to that class even when the slot itself omits
        // `domain:`.
        if self.options.include_slots {
            self.add_class_side_slot_edges(schema, &mut graph);
        }

        graph
    }

    /// Emit a class↔slot edge for each `slot` referenced from a class's
    /// `slots:` list. The slot-side traversal in [`add_slots`] only emits
    /// when the slot itself declares `domain:`; this pass covers the
    /// LinkML pattern where the class lists the slot but the slot omits
    /// `domain:`. Skipped silently when the named slot isn't declared in
    /// `schema.slots`, matching the existing graceful-skip in `add_slots`'s
    /// inverse-edge path. Idempotent against the slot-side pass — if both
    /// `slot.domain = C` and `C.slots = [s]` are present, a single edge
    /// is emitted.
    fn add_class_side_slot_edges(&self, schema: &SchemaDefinition, graph: &mut GraphData) {
        if !self.options.include_domain_edges {
            return;
        }
        let mut seen: std::collections::HashSet<(String, String)> = graph
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Domain)
            .map(|e| (e.source.clone(), e.target.clone()))
            .collect();
        for (class_name, class_def) in &schema.classes {
            for slot_name in &class_def.slots {
                if !schema.slots.contains_key(slot_name) {
                    continue;
                }
                let key = (
                    format!("slot:{}", slot_name),
                    format!("class:{}", class_name),
                );
                if seen.insert(key.clone()) {
                    graph.edges.push(GraphEdge {
                        source: key.0,
                        target: key.1,
                        edge_type: EdgeType::Domain,
                        label: Some("domain".to_string()),
                    });
                }
            }
        }
    }

    /// Walk inline class attributes and emit range edges from each owning
    /// class to its attribute's range target (when that target is a class,
    /// enum, or type — primitive ranges like `string` produce no edge).
    fn add_inline_attribute_edges(&self, schema: &SchemaDefinition, graph: &mut GraphData) {
        if !self.options.include_range_edges {
            return;
        }
        for (class_name, class_def) in &schema.classes {
            for (attr_name, attr_def) in &class_def.attributes {
                if let Some(range) = &attr_def.range
                    && let Some(target) = self.resolve_range_target(schema, range)
                {
                    graph.edges.push(GraphEdge {
                        source: format!("class:{}", class_name),
                        target,
                        edge_type: EdgeType::Range,
                        label: Some(attr_name.clone()),
                    });
                }
            }
        }
    }

    /// Add class nodes and their inheritance edges
    fn add_classes(&self, schema: &SchemaDefinition, graph: &mut GraphData) {
        for (name, class_def) in &schema.classes {
            // Get label from annotation or use name
            let label = class_def
                .annotations
                .get("panschema:label")
                .cloned()
                .unwrap_or_else(|| name.clone());

            // Get color, adjusting alpha for abstract classes
            let mut color = NodeType::Class.color();
            if class_def.r#abstract {
                color[3] = colors::ABSTRACT_ALPHA;
            }

            let kind_metadata = Some(KindMetadata::Class {
                slots: resolve_class_slots(schema, name),
                parents: class_def.is_a.iter().cloned().collect(),
                mixins: class_def.mixins.clone(),
            });

            graph.nodes.push(GraphNode {
                id: format!("class:{}", name),
                label,
                node_type: NodeType::Class,
                color,
                description: class_def.description.clone(),
                uri: class_def.class_uri.clone(),
                is_abstract: class_def.r#abstract,
                kind_metadata,
            });

            // Add subclass edge (is_a)
            if let Some(parent) = &class_def.is_a {
                graph.edges.push(GraphEdge {
                    source: format!("class:{}", name),
                    target: format!("class:{}", parent),
                    edge_type: EdgeType::SubclassOf,
                    label: None,
                });
            }

            // Add mixin edges
            for mixin in &class_def.mixins {
                graph.edges.push(GraphEdge {
                    source: format!("class:{}", name),
                    target: format!("class:{}", mixin),
                    edge_type: EdgeType::Mixin,
                    label: None,
                });
            }
        }
    }

    /// Add slot nodes and domain/range/inverse edges
    fn add_slots(&self, schema: &SchemaDefinition, graph: &mut GraphData) {
        for (name, slot_def) in &schema.slots {
            let label = slot_def
                .annotations
                .get("panschema:label")
                .cloned()
                .unwrap_or_else(|| name.clone());

            let kind_metadata = Some(KindMetadata::Slot {
                domain: slot_def.domain.clone(),
                range: slot_def.range.clone(),
                required: slot_def.required,
                multivalued: slot_def.multivalued,
            });

            graph.nodes.push(GraphNode {
                id: format!("slot:{}", name),
                label,
                node_type: NodeType::Slot,
                color: NodeType::Slot.color(),
                description: slot_def.description.clone(),
                uri: slot_def.slot_uri.clone(),
                is_abstract: false,
                kind_metadata,
            });

            // Add domain edge (slot -> class)
            if self.options.include_domain_edges
                && let Some(domain) = &slot_def.domain
                && schema.classes.contains_key(domain)
            {
                graph.edges.push(GraphEdge {
                    source: format!("slot:{}", name),
                    target: format!("class:{}", domain),
                    edge_type: EdgeType::Domain,
                    label: Some("domain".to_string()),
                });
            }

            // Add range edge (slot -> class/enum/type)
            if self.options.include_range_edges
                && let Some(range) = &slot_def.range
            {
                let target_id = self.resolve_range_target(schema, range);
                if let Some(target) = target_id {
                    graph.edges.push(GraphEdge {
                        source: format!("slot:{}", name),
                        target,
                        edge_type: EdgeType::Range,
                        label: Some("range".to_string()),
                    });
                }
            }

            // Add inverse edge (slot <-> slot)
            if self.options.include_inverse_edges
                && let Some(inverse) = &slot_def.inverse
                && schema.slots.contains_key(inverse)
            {
                graph.edges.push(GraphEdge {
                    source: format!("slot:{}", name),
                    target: format!("slot:{}", inverse),
                    edge_type: EdgeType::Inverse,
                    label: Some("inverseOf".to_string()),
                });
            }
        }
    }

    /// Resolve range to target node ID
    fn resolve_range_target(&self, schema: &SchemaDefinition, range: &str) -> Option<String> {
        if schema.classes.contains_key(range) {
            Some(format!("class:{}", range))
        } else if self.options.include_enums && schema.enums.contains_key(range) {
            Some(format!("enum:{}", range))
        } else if self.options.include_types && schema.types.contains_key(range) {
            Some(format!("type:{}", range))
        } else {
            // Range is a primitive type (string, integer, etc.) - no node
            None
        }
    }

    /// Add enum nodes
    fn add_enums(&self, schema: &SchemaDefinition, graph: &mut GraphData) {
        for (name, enum_def) in &schema.enums {
            let kind_metadata = Some(KindMetadata::Enum {
                permissible_values: enum_def.permissible_values.keys().cloned().collect(),
            });

            graph.nodes.push(GraphNode {
                id: format!("enum:{}", name),
                label: name.clone(),
                node_type: NodeType::Enum,
                color: NodeType::Enum.color(),
                description: enum_def.description.clone(),
                uri: None,
                is_abstract: false,
                kind_metadata,
            });
        }
    }

    /// Add type nodes and typeof edges
    fn add_types(&self, schema: &SchemaDefinition, graph: &mut GraphData) {
        for (name, type_def) in &schema.types {
            graph.nodes.push(GraphNode {
                id: format!("type:{}", name),
                label: name.clone(),
                node_type: NodeType::Type,
                color: NodeType::Type.color(),
                description: type_def.description.clone(),
                uri: type_def.uri.clone(),
                is_abstract: false,
                kind_metadata: None,
            });

            // Add typeof edge (type -> parent type)
            if let Some(parent) = &type_def.typeof_
                && schema.types.contains_key(parent)
            {
                graph.edges.push(GraphEdge {
                    source: format!("type:{}", name),
                    target: format!("type:{}", parent),
                    edge_type: EdgeType::TypeOf,
                    label: None,
                });
            }
        }
    }
}

impl Default for GraphWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer for GraphWriter {
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()> {
        let graph = self.schema_to_graph(schema);

        let json = serde_json::to_string_pretty(&graph)
            .map_err(|e| IoError::Write(format!("JSON serialization failed: {}", e)))?;

        std::fs::write(output, json).map_err(IoError::Io)?;

        Ok(())
    }

    fn format_id(&self) -> &str {
        "graph-json"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linkml::{
        ClassDefinition, EnumDefinition, PermissibleValue, SlotDefinition, TypeDefinition,
    };

    // ========== Empty/Minimal Schema Tests ==========

    #[test]
    fn empty_schema_produces_empty_graph() {
        let schema = SchemaDefinition::new("empty");
        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        assert_eq!(graph.schema_name, "empty");
        assert!(graph.nodes.is_empty());
        assert!(graph.edges.is_empty());
        assert_eq!(graph.format_version, GraphData::FORMAT_VERSION);
    }

    #[test]
    fn single_class_produces_single_node() {
        let mut schema = SchemaDefinition::new("single");
        schema
            .classes
            .insert("Animal".to_string(), ClassDefinition::new("Animal"));

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].id, "class:Animal");
        assert_eq!(graph.nodes[0].node_type, NodeType::Class);
        assert!(graph.edges.is_empty());
    }

    // ========== Class Hierarchy Tests ==========

    #[test]
    fn class_hierarchy_produces_subclass_edges() {
        let mut schema = SchemaDefinition::new("hierarchy");

        schema
            .classes
            .insert("Animal".to_string(), ClassDefinition::new("Animal"));

        let mut dog = ClassDefinition::new("Dog");
        dog.is_a = Some("Animal".to_string());
        schema.classes.insert("Dog".to_string(), dog);

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), 1);

        let edge = &graph.edges[0];
        assert_eq!(edge.source, "class:Dog");
        assert_eq!(edge.target, "class:Animal");
        assert_eq!(edge.edge_type, EdgeType::SubclassOf);
    }

    #[test]
    fn mixin_relationship_produces_mixin_edge() {
        let mut schema = SchemaDefinition::new("mixins");

        schema
            .classes
            .insert("Named".to_string(), ClassDefinition::new("Named"));

        let mut person = ClassDefinition::new("Person");
        person.mixins = vec!["Named".to_string()];
        schema.classes.insert("Person".to_string(), person);

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        let mixin_edge = graph
            .edges
            .iter()
            .find(|e| e.edge_type == EdgeType::Mixin)
            .expect("Should have mixin edge");

        assert_eq!(mixin_edge.source, "class:Person");
        assert_eq!(mixin_edge.target, "class:Named");
    }

    // ========== Slot Tests ==========

    #[test]
    fn slot_with_domain_range_produces_edges() {
        let mut schema = SchemaDefinition::new("slots");

        schema
            .classes
            .insert("Animal".to_string(), ClassDefinition::new("Animal"));
        schema
            .classes
            .insert("Person".to_string(), ClassDefinition::new("Person"));

        let mut has_owner = SlotDefinition::new("hasOwner");
        has_owner.domain = Some("Animal".to_string());
        has_owner.range = Some("Person".to_string());
        schema.slots.insert("hasOwner".to_string(), has_owner);

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        // Should have 3 nodes: Animal, Person, hasOwner
        assert_eq!(graph.nodes.len(), 3);

        // Should have domain and range edges
        let domain_edge = graph
            .edges
            .iter()
            .find(|e| e.edge_type == EdgeType::Domain)
            .expect("Should have domain edge");
        assert_eq!(domain_edge.source, "slot:hasOwner");
        assert_eq!(domain_edge.target, "class:Animal");

        let range_edge = graph
            .edges
            .iter()
            .find(|e| e.edge_type == EdgeType::Range)
            .expect("Should have range edge");
        assert_eq!(range_edge.source, "slot:hasOwner");
        assert_eq!(range_edge.target, "class:Person");
    }

    #[test]
    fn inverse_slots_produce_inverse_edge() {
        let mut schema = SchemaDefinition::new("inverse");

        let mut has_owner = SlotDefinition::new("hasOwner");
        has_owner.inverse = Some("owns".to_string());
        schema.slots.insert("hasOwner".to_string(), has_owner);

        let owns = SlotDefinition::new("owns");
        schema.slots.insert("owns".to_string(), owns);

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        let inverse_edge = graph
            .edges
            .iter()
            .find(|e| e.edge_type == EdgeType::Inverse)
            .expect("Should have inverse edge");
        assert_eq!(inverse_edge.source, "slot:hasOwner");
        assert_eq!(inverse_edge.target, "slot:owns");
    }

    // ========== Class-side `slots:` (inverse domain) Tests ==========

    #[test]
    fn class_side_slot_reference_emits_domain_edge_when_slot_lacks_domain() {
        // A class lists a slot in `class.slots:` but the slot itself
        // declares no `domain:`. LinkML treats this as a valid
        // class↔slot relation (the class card already renders it),
        // so the graph must connect them. Without this pass, the slot
        // appears as an orphan node.
        let mut schema = SchemaDefinition::new("s");
        schema
            .slots
            .insert("title".to_string(), SlotDefinition::new("title"));
        let mut book = ClassDefinition::new("Book");
        book.slots.push("title".to_string());
        schema.classes.insert("Book".to_string(), book);

        let graph = GraphWriter::new().schema_to_graph(&schema);
        let domain_edges: Vec<_> = graph
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Domain)
            .collect();
        assert_eq!(
            domain_edges.len(),
            1,
            "expected one domain edge from class-side reference; got: {:?}",
            domain_edges
        );
        assert_eq!(domain_edges[0].source, "slot:title");
        assert_eq!(domain_edges[0].target, "class:Book");
    }

    #[test]
    fn class_side_slot_reference_is_deduped_against_slot_side_domain() {
        // When BOTH `slot.domain = C` and `C.slots: [s]` are set,
        // the result is a single edge — the two write-paths express
        // the same relation and must not produce two graph edges.
        let mut schema = SchemaDefinition::new("s");
        let mut author_slot = SlotDefinition::new("author");
        author_slot.domain = Some("Book".to_string());
        schema.slots.insert("author".to_string(), author_slot);
        let mut book = ClassDefinition::new("Book");
        book.slots.push("author".to_string());
        schema.classes.insert("Book".to_string(), book);

        let graph = GraphWriter::new().schema_to_graph(&schema);
        let domain_edges: Vec<_> = graph
            .edges
            .iter()
            .filter(|e| {
                e.edge_type == EdgeType::Domain
                    && e.source == "slot:author"
                    && e.target == "class:Book"
            })
            .collect();
        assert_eq!(
            domain_edges.len(),
            1,
            "expected exactly one (slot:author, class:Book) edge; got: {:?}",
            domain_edges
        );
    }

    #[test]
    fn class_side_slot_reference_emits_one_edge_per_host_class() {
        // A slot referenced from N classes' `slots:` lists produces
        // N distinct edges — one per host. The scimantic case is
        // `content` used by both `Evidence` and `Conclusion`.
        let mut schema = SchemaDefinition::new("s");
        schema
            .slots
            .insert("content".to_string(), SlotDefinition::new("content"));
        let mut evidence = ClassDefinition::new("Evidence");
        evidence.slots.push("content".to_string());
        schema.classes.insert("Evidence".to_string(), evidence);
        let mut conclusion = ClassDefinition::new("Conclusion");
        conclusion.slots.push("content".to_string());
        schema.classes.insert("Conclusion".to_string(), conclusion);

        let graph = GraphWriter::new().schema_to_graph(&schema);
        let targets: std::collections::BTreeSet<&str> = graph
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Domain && e.source == "slot:content")
            .map(|e| e.target.as_str())
            .collect();
        assert_eq!(
            targets,
            ["class:Conclusion", "class:Evidence"].into_iter().collect(),
            "expected `content` to connect to both host classes; got: {:?}",
            targets
        );
    }

    #[test]
    fn class_side_slot_reference_skips_undeclared_slot_names() {
        // A class can reference a slot name that isn't declared in
        // `schema.slots` (e.g. typo, removed slot). No edge should
        // be emitted, and the pass must not panic — matching the
        // graceful-skip pattern in `add_slots`'s inverse-edge path.
        let mut schema = SchemaDefinition::new("s");
        let mut book = ClassDefinition::new("Book");
        book.slots.push("phantom".to_string());
        schema.classes.insert("Book".to_string(), book);

        let graph = GraphWriter::new().schema_to_graph(&schema);
        assert!(
            graph
                .edges
                .iter()
                .all(|e| e.source != "slot:phantom" && e.target != "slot:phantom"),
            "no edges should reference the undeclared slot; got: {:?}",
            graph.edges
        );
    }

    #[test]
    fn class_side_slot_pass_respects_include_domain_edges_flag() {
        // The class-side pass shares the `include_domain_edges`
        // toggle with the slot-side pass — they emit the same edge
        // type, so disabling one disables the other.
        let mut schema = SchemaDefinition::new("s");
        schema
            .slots
            .insert("title".to_string(), SlotDefinition::new("title"));
        let mut book = ClassDefinition::new("Book");
        book.slots.push("title".to_string());
        schema.classes.insert("Book".to_string(), book);

        let opts = GraphOptions {
            include_domain_edges: false,
            ..GraphOptions::default()
        };
        let graph = GraphWriter::with_options(opts).schema_to_graph(&schema);
        assert!(
            !graph.edges.iter().any(|e| e.edge_type == EdgeType::Domain),
            "expected no domain edges when include_domain_edges = false; got: {:?}",
            graph.edges
        );
    }

    // ========== Inline Attribute Tests ==========

    #[test]
    fn inline_attribute_with_class_range_produces_edge_from_class() {
        let mut schema = SchemaDefinition::new("inline");
        schema
            .classes
            .insert("Department".to_string(), ClassDefinition::new("Department"));

        let mut person = ClassDefinition::new("Person");
        let mut dept_attr = SlotDefinition::new("department");
        dept_attr.range = Some("Department".to_string());
        person
            .attributes
            .insert("department".to_string(), dept_attr);
        schema.classes.insert("Person".to_string(), person);

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        let attr_edge = graph
            .edges
            .iter()
            .find(|e| e.source == "class:Person" && e.target == "class:Department")
            .expect("Should have edge from Person to Department via inline 'department' attribute");
        assert_eq!(attr_edge.edge_type, EdgeType::Range);
        assert_eq!(attr_edge.label.as_deref(), Some("department"));
    }

    #[test]
    fn inline_attribute_with_enum_range_produces_edge_to_enum() {
        let mut schema = SchemaDefinition::new("inline_enum");
        schema
            .enums
            .insert("YearEnum".to_string(), EnumDefinition::new("YearEnum"));

        let mut student = ClassDefinition::new("Student");
        let mut year_attr = SlotDefinition::new("year");
        year_attr.range = Some("YearEnum".to_string());
        student.attributes.insert("year".to_string(), year_attr);
        schema.classes.insert("Student".to_string(), student);

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        let edge = graph
            .edges
            .iter()
            .find(|e| e.source == "class:Student" && e.target == "enum:YearEnum")
            .expect("Inline attribute with enum range should produce class→enum edge");
        assert_eq!(edge.label.as_deref(), Some("year"));
    }

    #[test]
    fn inline_attribute_with_primitive_range_produces_no_edge() {
        let mut schema = SchemaDefinition::new("inline_primitive");
        let mut person = ClassDefinition::new("Person");
        let mut name_attr = SlotDefinition::new("name");
        name_attr.range = Some("string".to_string());
        person.attributes.insert("name".to_string(), name_attr);
        schema.classes.insert("Person".to_string(), person);

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        // Only the Person node, no edges (string isn't a class/enum/type node)
        assert_eq!(graph.nodes.len(), 1);
        assert!(
            graph.edges.is_empty(),
            "Inline attribute with primitive range should produce no edge, got: {:?}",
            graph.edges
        );
    }

    // ========== Enum and Type Tests ==========

    #[test]
    fn enum_produces_enum_node() {
        let mut schema = SchemaDefinition::new("enums");
        schema
            .enums
            .insert("Status".to_string(), EnumDefinition::new("Status"));

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].id, "enum:Status");
        assert_eq!(graph.nodes[0].node_type, NodeType::Enum);
    }

    #[test]
    fn type_produces_type_node() {
        let mut schema = SchemaDefinition::new("types");
        schema
            .types
            .insert("Email".to_string(), TypeDefinition::new("Email"));

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].id, "type:Email");
        assert_eq!(graph.nodes[0].node_type, NodeType::Type);
    }

    #[test]
    fn type_hierarchy_produces_typeof_edge() {
        let mut schema = SchemaDefinition::new("type_hierarchy");

        schema
            .types
            .insert("string".to_string(), TypeDefinition::new("string"));

        let mut email = TypeDefinition::new("Email");
        email.typeof_ = Some("string".to_string());
        schema.types.insert("Email".to_string(), email);

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        let typeof_edge = graph
            .edges
            .iter()
            .find(|e| e.edge_type == EdgeType::TypeOf)
            .expect("Should have typeof edge");
        assert_eq!(typeof_edge.source, "type:Email");
        assert_eq!(typeof_edge.target, "type:string");
    }

    // ========== Options Tests ==========

    #[test]
    fn classes_only_option_excludes_slots() {
        let mut schema = SchemaDefinition::new("test");
        schema
            .classes
            .insert("Animal".to_string(), ClassDefinition::new("Animal"));
        schema
            .slots
            .insert("name".to_string(), SlotDefinition::new("name"));
        schema
            .enums
            .insert("Status".to_string(), EnumDefinition::new("Status"));
        schema
            .types
            .insert("Email".to_string(), TypeDefinition::new("Email"));

        let writer = GraphWriter::with_options(GraphOptions::classes_only());
        let graph = writer.schema_to_graph(&schema);

        assert_eq!(graph.nodes.len(), 1);
        assert!(graph.nodes.iter().all(|n| n.node_type == NodeType::Class));
    }

    // ========== Color Tests ==========

    #[test]
    fn abstract_class_has_reduced_alpha() {
        let mut schema = SchemaDefinition::new("abstract");

        let mut animal = ClassDefinition::new("Animal");
        animal.r#abstract = true;
        schema.classes.insert("Animal".to_string(), animal);

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        let node = &graph.nodes[0];
        assert!(node.is_abstract);
        assert_eq!(node.color[3], colors::ABSTRACT_ALPHA);
    }

    #[test]
    fn node_types_have_distinct_colors() {
        assert_ne!(NodeType::Class.color(), NodeType::Slot.color());
        assert_ne!(NodeType::Class.color(), NodeType::Enum.color());
        assert_ne!(NodeType::Class.color(), NodeType::Type.color());
        assert_ne!(NodeType::Slot.color(), NodeType::Enum.color());
    }

    // ========== Writer Trait Tests ==========

    #[test]
    fn graph_writer_format_id_is_graph_json() {
        let writer = GraphWriter::new();
        assert_eq!(writer.format_id(), "graph-json");
    }

    #[test]
    fn graph_writer_produces_valid_json_file() {
        let mut schema = SchemaDefinition::new("json_test");
        schema.title = Some("JSON Test".to_string());
        schema
            .classes
            .insert("Animal".to_string(), ClassDefinition::new("Animal"));

        let writer = GraphWriter::new();
        let temp_dir = std::env::temp_dir().join("panschema_graph_writer_test");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let output_path = temp_dir.join("graph.json");

        writer
            .write(&schema, &output_path)
            .expect("Write should succeed");

        assert!(output_path.exists());
        let content = std::fs::read_to_string(&output_path).unwrap();

        // Verify it's valid JSON that can be parsed back
        let parsed: GraphData = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.schema_name, "json_test");
        assert_eq!(parsed.schema_title, Some("JSON Test".to_string()));
        assert_eq!(parsed.nodes.len(), 1);

        // Cleanup
        let _ = std::fs::remove_dir_all(temp_dir);
    }

    // ========== Serialization/Deserialization Tests ==========

    #[test]
    fn graph_data_roundtrip_json() {
        let mut graph = GraphData::new("test".to_string(), Some("Test Graph".to_string()));
        graph.nodes.push(GraphNode {
            id: "class:Animal".to_string(),
            label: "Animal".to_string(),
            node_type: NodeType::Class,
            color: NodeType::Class.color(),
            description: Some("A living thing".to_string()),
            uri: Some("http://example.org#Animal".to_string()),
            is_abstract: false,
            kind_metadata: None,
        });
        graph.edges.push(GraphEdge {
            source: "class:Dog".to_string(),
            target: "class:Animal".to_string(),
            edge_type: EdgeType::SubclassOf,
            label: None,
        });

        let json = serde_json::to_string_pretty(&graph).unwrap();
        let restored: GraphData = serde_json::from_str(&json).unwrap();

        assert_eq!(graph.schema_name, restored.schema_name);
        assert_eq!(graph.nodes.len(), restored.nodes.len());
        assert_eq!(graph.edges.len(), restored.edges.len());
    }

    #[test]
    fn slot_range_to_enum_produces_edge() {
        let mut schema = SchemaDefinition::new("slot_enum");

        schema
            .enums
            .insert("Status".to_string(), EnumDefinition::new("Status"));

        let mut status_slot = SlotDefinition::new("status");
        status_slot.range = Some("Status".to_string());
        schema.slots.insert("status".to_string(), status_slot);

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        let range_edge = graph
            .edges
            .iter()
            .find(|e| e.edge_type == EdgeType::Range)
            .expect("Should have range edge to enum");
        assert_eq!(range_edge.target, "enum:Status");
    }

    // ========== kind_metadata Tests ==========

    /// Pull the `KindMetadata::Class` payload from `graph` for the
    /// named class. Test helper — the production code carries the
    /// metadata on `GraphNode` and doesn't need a direct lookup.
    fn class_kind_metadata<'a>(
        graph: &'a GraphData,
        name: &str,
    ) -> (&'a [String], &'a [String], &'a [String]) {
        let id = format!("class:{}", name);
        let node = graph
            .nodes
            .iter()
            .find(|n| n.id == id)
            .expect("class node should exist");
        match node.kind_metadata.as_ref().expect("class needs metadata") {
            KindMetadata::Class {
                slots,
                parents,
                mixins,
            } => (slots.as_slice(), parents.as_slice(), mixins.as_slice()),
            other => panic!("expected Class metadata, got {:?}", other),
        }
    }

    #[test]
    fn class_kind_metadata_collects_inherited_slots_in_direct_first_order() {
        // Authors care about "what's the full set of fields on this
        // class?". The hover card surfaces every slot reachable
        // through `is_a` and `mixins`, but lists each class's own
        // direct slots first so the inherited ones don't bury what
        // the class itself introduces.
        let mut schema = SchemaDefinition::new("inheritance");
        let mut animal = ClassDefinition::new("Animal");
        animal.slots = vec!["name".into()];
        schema.classes.insert("Animal".to_string(), animal);

        let mut dog = ClassDefinition::new("Dog");
        dog.is_a = Some("Animal".into());
        dog.slots = vec!["breed".into()];
        schema.classes.insert("Dog".to_string(), dog);

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        let (slots, parents, mixins) = class_kind_metadata(&graph, "Dog");
        assert_eq!(slots, &["breed".to_string(), "name".to_string()]);
        assert_eq!(parents, &["Animal".to_string()]);
        assert!(mixins.is_empty());
    }

    #[test]
    fn class_kind_metadata_walks_mixins_and_dedupes() {
        // Mixins flatten in: their slots show up in the consuming
        // class's resolved list. When the same slot name reaches a
        // class via two paths (its own attributes plus a mixin),
        // the list keeps only the first occurrence so the card
        // doesn't show duplicate rows.
        let mut schema = SchemaDefinition::new("mixin_resolve");
        let mut named = ClassDefinition::new("Named");
        named.slots = vec!["name".into()];
        schema.classes.insert("Named".to_string(), named);

        let mut person = ClassDefinition::new("Person");
        person.mixins = vec!["Named".into()];
        person.slots = vec!["name".into(), "age".into()];
        schema.classes.insert("Person".to_string(), person);

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        let (slots, _, mixins) = class_kind_metadata(&graph, "Person");
        assert_eq!(slots, &["name".to_string(), "age".to_string()]);
        assert_eq!(mixins, &["Named".to_string()]);
    }

    #[test]
    fn slot_kind_metadata_captures_domain_range_and_flags() {
        // Required + multivalued ride along on every slot so the
        // hover card can render a "required, multi-valued" line
        // without re-deriving from elsewhere. Domain/range come
        // through verbatim and are what the card pivots on when
        // suggesting jump-to-class affordances.
        let mut schema = SchemaDefinition::new("slot_meta");
        let mut owners = SlotDefinition::new("owners");
        owners.domain = Some("Animal".into());
        owners.range = Some("Person".into());
        owners.required = true;
        owners.multivalued = true;
        schema.slots.insert("owners".to_string(), owners);

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        let node = graph.nodes.iter().find(|n| n.id == "slot:owners").unwrap();
        match node.kind_metadata.as_ref().unwrap() {
            KindMetadata::Slot {
                domain,
                range,
                required,
                multivalued,
            } => {
                assert_eq!(domain.as_deref(), Some("Animal"));
                assert_eq!(range.as_deref(), Some("Person"));
                assert!(*required);
                assert!(*multivalued);
            }
            other => panic!("expected Slot metadata, got {:?}", other),
        }
    }

    #[test]
    fn enum_kind_metadata_surfaces_permissible_values() {
        // The enum's permissible values are what the hover card
        // shows when an author lands on an enum node. Order matches
        // the BTreeMap iteration order, which is sorted — fine for
        // hover-card display since the card chunks long lists with
        // "+N more" anyway.
        let mut schema = SchemaDefinition::new("enum_meta");
        let mut severity = EnumDefinition::new("Severity");
        severity
            .permissible_values
            .insert("low".to_string(), PermissibleValue::new("low"));
        severity
            .permissible_values
            .insert("high".to_string(), PermissibleValue::new("high"));
        schema.enums.insert("Severity".to_string(), severity);

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        let node = graph
            .nodes
            .iter()
            .find(|n| n.id == "enum:Severity")
            .unwrap();
        match node.kind_metadata.as_ref().unwrap() {
            KindMetadata::Enum { permissible_values } => {
                assert!(permissible_values.contains(&"low".to_string()));
                assert!(permissible_values.contains(&"high".to_string()));
            }
            other => panic!("expected Enum metadata, got {:?}", other),
        }
    }

    #[test]
    fn type_nodes_have_no_kind_metadata() {
        // Type nodes have no extra payload worth surfacing yet —
        // their `uri` and `description` already cover what the card
        // would show. Pinning `None` here means the JS card can
        // skip the per-kind dispatch entirely for types and just
        // render the common header.
        let mut schema = SchemaDefinition::new("type_meta");
        schema
            .types
            .insert("Distance".to_string(), TypeDefinition::new("Distance"));

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        let node = graph
            .nodes
            .iter()
            .find(|n| n.id == "type:Distance")
            .unwrap();
        assert!(node.kind_metadata.is_none());
    }
}
