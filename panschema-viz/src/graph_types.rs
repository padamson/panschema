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
    #[allow(dead_code)]
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
