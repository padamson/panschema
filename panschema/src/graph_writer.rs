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

        graph
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

            graph.nodes.push(GraphNode {
                id: format!("class:{}", name),
                label,
                node_type: NodeType::Class,
                color,
                description: class_def.description.clone(),
                uri: class_def.class_uri.clone(),
                is_abstract: class_def.r#abstract,
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

            graph.nodes.push(GraphNode {
                id: format!("slot:{}", name),
                label,
                node_type: NodeType::Slot,
                color: NodeType::Slot.color(),
                description: slot_def.description.clone(),
                uri: slot_def.slot_uri.clone(),
                is_abstract: false,
            });

            // Add domain edge (slot -> class)
            if self.options.include_domain_edges {
                if let Some(domain) = &slot_def.domain {
                    if schema.classes.contains_key(domain) {
                        graph.edges.push(GraphEdge {
                            source: format!("slot:{}", name),
                            target: format!("class:{}", domain),
                            edge_type: EdgeType::Domain,
                            label: Some("domain".to_string()),
                        });
                    }
                }
            }

            // Add range edge (slot -> class/enum/type)
            if self.options.include_range_edges {
                if let Some(range) = &slot_def.range {
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
            }

            // Add inverse edge (slot <-> slot)
            if self.options.include_inverse_edges {
                if let Some(inverse) = &slot_def.inverse {
                    if schema.slots.contains_key(inverse) {
                        graph.edges.push(GraphEdge {
                            source: format!("slot:{}", name),
                            target: format!("slot:{}", inverse),
                            edge_type: EdgeType::Inverse,
                            label: Some("inverseOf".to_string()),
                        });
                    }
                }
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
            graph.nodes.push(GraphNode {
                id: format!("enum:{}", name),
                label: name.clone(),
                node_type: NodeType::Enum,
                color: NodeType::Enum.color(),
                description: enum_def.description.clone(),
                uri: None,
                is_abstract: false,
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
            });

            // Add typeof edge (type -> parent type)
            if let Some(parent) = &type_def.typeof_ {
                if schema.types.contains_key(parent) {
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
    use crate::linkml::{ClassDefinition, EnumDefinition, SlotDefinition, TypeDefinition};

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
}
