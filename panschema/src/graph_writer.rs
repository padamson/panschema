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

    /// Individual (A-box instance) nodes: Teal (#29B8B3) — distinct from
    /// every T-box kind so the instance graph reads apart from the schema.
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
    /// An OWL `NamedIndividual` — an A-box instance, drawn only in the
    /// separate instance graph, never in the schema (T-box) graph.
    Individual,
}

impl NodeType {
    /// Get the default color for this node type
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
    /// An object-property assertion between two individuals in the
    /// instance graph (labelled by the property).
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

    /// Optional URI for linking. A curie with a declared prefix is
    /// expanded to the full IRI here; see [`GraphNode::uri_unresolved`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    /// True when `uri` is a curie whose prefix isn't declared in the
    /// schema, so it couldn't be expanded — the hover card surfaces it
    /// verbatim with a `?` indicator.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub uri_unresolved: bool,

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
    /// `mixins:` list — each with its effective shape — plus the
    /// immediate parents and mixins for the inheritance view.
    Class {
        slots: Vec<SlotSummary>,
        parents: Vec<String>,
        mixins: Vec<String>,
        /// The class's conditional `rules`, each with the rendered
        /// "when … then …" summary. Empty when the class declares none.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        rules: Vec<RuleSummary>,
    },
    /// Resolved view of a LinkML slot. `required` / `multivalued`
    /// are the effective-cardinality reconciliation of the bool
    /// flags with the explicit `min` / `max` bounds. `pattern`,
    /// `identifier`, and `any_of` (the element ranges of a
    /// polymorphic range) surface the constraint fields authors
    /// edit most.
    Slot {
        /// Every class this slot is a domain of (explicit `domain:`, or
        /// the classes that list it in `slots:`). A slot can belong to
        /// several classes, so this is a list, not a single value.
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
    /// each with its optional description and curie-expanded
    /// `meaning` IRI.
    Enum {
        permissible_values: Vec<PermissibleValueSummary>,
    },
    /// An OWL individual in the instance graph: the class ids it is an
    /// instance of (resolvable to class cards) plus its literal-valued
    /// property assertions (object-valued ones become edges instead).
    Individual {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        types: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        literals: Vec<PropertyLiteral>,
    },
}

/// A literal-valued property assertion on an individual, shown on the
/// instance node's hover (object-valued assertions become edges).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PropertyLiteral {
    pub property: String,
    pub value: String,
}

/// One class rule in the hover payload: its title/description plus the
/// rendered "when … then …" summary — the same projection the HTML card
/// uses (see [`crate::rules`]). Carried so the viz popup can surface rules;
/// the viz-side `KindMetadata::Class` mirror gains this field when it
/// consumes it (the popup redesign), and ignores it until then.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuleSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Slots that make the rule fire (precondition side, `any_of` included),
    /// for highlighting the rule's trigger nodes on hover.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trigger_slots: Vec<String>,
    /// Slots the rule then constrains (postcondition side), for placing
    /// governed-slot glyphs and highlighting the governed nodes on hover.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub governed_slots: Vec<String>,
}

/// One permissible value of an enum in the hover card: the value
/// text plus the optional `description` (tooltip) and curie-expanded
/// `meaning` IRI (for future click-to-jump affordances).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PermissibleValueSummary {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meaning: Option<String>,
}

/// One slot in a class's resolved view, carrying the effective
/// shape (post `slot_usage` overlay, bounds reconciled with flags)
/// so the hover card shows what the class actually has — not the
/// slot's global, un-refined definition.
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

/// Sorted list of every slot reachable from `class_name` via its
/// direct `attributes:`, `slots:` references, `is_a` chain,
/// `mixins:`, and `slot_usage` overlay. Delegates to the shared
/// resolver in `linkml_resolve` so the hover card, the HTML class
/// card, and the Rust writer all observe the same slot list for
/// every class.
///
/// Returned in `BTreeMap` (alphabetical) order — the resolver's
/// natural output. The hover card's "+N more" cap means alphabetical
/// order is fine for the 5-slot summary; authors who need the full
/// list and a specific order click-to-pin the persistent panel.
/// Inherited entries carry their origin so the hover card
/// distinguishes a class's own slots from flattened ones; each
/// entry carries the slot's effective shape, not the global
/// un-refined definition.
/// Resolve a node's URI for display via [`expand_curie`], which keeps
/// full IRIs (`http(s)://`, `urn:`) verbatim, expands a known
/// `prefix:local` curie against the schema's prefixes, and expands a
/// bare name against the default prefix. When it can't resolve — an
/// unrecognised prefix, or a bare name with no default prefix — the
/// value is surfaced verbatim and flagged so the hover marks it `?`.
/// Returns `(display_uri, unresolved)`.
///
/// [`expand_curie`]: crate::linkml_resolve::expand_curie
/// Local name of an IRI: the part after the last `#` or `/`, else the
/// whole string. Mirrors how the HTML individual card resolves a type
/// IRI to a class id.
fn local_name(iri: &str) -> &str {
    iri.rsplit(['#', '/']).next().unwrap_or(iri)
}

/// Capitalize the first character (ASCII), leaving the rest untouched —
/// the instance node's display label when the individual has no
/// `rdfs:label`, matching the HTML individual card's fallback.
fn capitalize_first(id: &str) -> String {
    let mut chars = id.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn resolve_node_uri(schema: &SchemaDefinition, uri: Option<&str>) -> (Option<String>, bool) {
    match uri {
        None => (None, false),
        Some(v) => match crate::linkml_resolve::expand_curie(schema, v) {
            Some(full) => (Some(full), false),
            None => (Some(v.to_string()), true),
        },
    }
}

fn resolve_class_slots(schema: &SchemaDefinition, class_name: &str) -> Vec<SlotSummary> {
    let Some(class_def) = schema.classes.get(class_name) else {
        return Vec::new();
    };
    crate::linkml_resolve::resolve_effective_slots_with_provenance(class_def, schema)
        .into_iter()
        .map(|(name, rs)| {
            let cardinality = crate::linkml_resolve::effective_cardinality(&rs.definition);
            SlotSummary {
                name,
                range: rs.definition.range.clone(),
                required: cardinality.required,
                multivalued: cardinality.multivalued,
                min: cardinality.min,
                max: cardinality.max,
                origin: rs.provenance.origin_label(class_name),
            }
        })
        .collect()
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

        // Per-class induced range edges (slot_usage narrowing). Also needs
        // enum/type nodes present.
        self.add_induced_range_edges(schema, &mut graph);

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

    /// Build the separate instance (A-box) graph from the OWL individuals
    /// panschema ingested into `panschema:individual*` annotations: one
    /// node per individual (kind `Individual`, carrying its type class ids
    /// and literal-valued assertions), and one `Assertion` edge per
    /// object-property assertion that links two individuals (its value is
    /// the target individual's IRI). Distinct from [`schema_to_graph`] —
    /// no individual nodes ever enter the schema (T-box) graph. Returns a
    /// graph with no nodes when the schema declares no individuals, so the
    /// caller can omit the instance-graph section entirely.
    pub fn schema_to_instance_graph(&self, schema: &SchemaDefinition) -> GraphData {
        use std::collections::HashMap;

        let mut graph = GraphData::new(schema.name.clone(), schema.title.clone());

        let Some(ids_csv) = schema.annotations.get("panschema:individuals") else {
            return graph;
        };
        let ids: Vec<&str> = ids_csv
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();

        // Map each individual's IRI to its node id so an object-property
        // assertion (whose value is the target's IRI) resolves to an edge.
        let node_id_of = |id: &str| format!("individual:{id}");
        let mut iri_to_node: HashMap<&str, String> = HashMap::new();
        for id in &ids {
            if let Some(iri) = schema
                .annotations
                .get(&format!("panschema:individual:{id}:_iri"))
            {
                iri_to_node.insert(iri.as_str(), node_id_of(id));
            }
        }

        for id in &ids {
            let node_id = node_id_of(id);

            let label = schema
                .annotations
                .get(&format!("panschema:individual:{id}:_label"))
                .cloned()
                .unwrap_or_else(|| capitalize_first(id));

            // rdf:type IRIs → the class ids the schema actually defines.
            let types: Vec<String> = schema
                .annotations
                .get(&format!("panschema:individual:{id}"))
                .map(|csv| {
                    csv.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(local_name)
                        .filter(|tid| schema.classes.contains_key(*tid))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();

            // Walk this individual's property assertions. An object-valued
            // one (value == a known individual's IRI) becomes an edge; a
            // literal-valued one becomes hover metadata.
            let prefix = format!("panschema:individual:{id}:");
            let mut literals: Vec<PropertyLiteral> = Vec::new();
            for (key, value) in &schema.annotations {
                let Some(prop) = key.strip_prefix(&prefix) else {
                    continue;
                };
                // Skip the reserved sub-keys (`_iri`/`_label`/`_comment`)
                // and the per-property `:_label` companion keys.
                if prop.starts_with('_') || prop.ends_with(":_label") {
                    continue;
                }
                let prop_label = schema
                    .annotations
                    .get(&format!("{key}:_label"))
                    .cloned()
                    .or_else(|| {
                        schema
                            .slots
                            .get(prop)
                            .and_then(|s| s.annotations.get("panschema:label").cloned())
                    })
                    .unwrap_or_else(|| prop.to_string());

                if let Some(target) = iri_to_node.get(value.as_str()) {
                    graph.edges.push(GraphEdge {
                        source: node_id.clone(),
                        target: target.clone(),
                        edge_type: EdgeType::Assertion,
                        label: Some(prop_label),
                    });
                } else {
                    literals.push(PropertyLiteral {
                        property: prop_label,
                        value: value.clone(),
                    });
                }
            }
            literals.sort_by(|a, b| a.property.cmp(&b.property));

            let comment = schema
                .annotations
                .get(&format!("panschema:individual:{id}:_comment"))
                .cloned();
            let (uri, uri_unresolved) = resolve_node_uri(
                schema,
                schema
                    .annotations
                    .get(&format!("panschema:individual:{id}:_iri"))
                    .map(String::as_str),
            );

            graph.nodes.push(GraphNode {
                id: node_id,
                label,
                node_type: NodeType::Individual,
                color: NodeType::Individual.color(),
                description: comment,
                uri,
                uri_unresolved,
                is_abstract: false,
                kind_metadata: Some(KindMetadata::Individual { types, literals }),
            });
        }

        // Stable order: nodes by id, edges by (source, target, label).
        graph.nodes.sort_by(|a, b| a.id.cmp(&b.id));
        graph.edges.sort_by(|a, b| {
            (&a.source, &a.target, &a.label).cmp(&(&b.source, &b.target, &b.label))
        });
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

    /// Emit per-class range edges for slots a class narrows via
    /// `slot_usage`. The shared slot node carries the slot's *global*
    /// range (its union edges, from [`add_slots`]); a class that narrows
    /// the range for itself needs its actual I/O drawn directly, the same
    /// way inline attributes connect a class to its range. So when a class
    /// refines a slot's range — a single `range`, a smaller `any_of`, or
    /// `maximum_cardinality: 0` — draw `class -> induced target` edges for
    /// the induced members (and none at all when the slot is suppressed).
    /// Classes that don't narrow the range rely on the global slot-side
    /// edges, so this pass only fires on a range-affecting `slot_usage`.
    fn add_induced_range_edges(&self, schema: &SchemaDefinition, graph: &mut GraphData) {
        if !self.options.include_range_edges {
            return;
        }
        let mut seen: std::collections::HashSet<(String, String, String)> =
            std::collections::HashSet::new();
        for (class_name, class_def) in &schema.classes {
            if class_def.slot_usage.is_empty() {
                continue;
            }
            let resolved =
                crate::linkml_resolve::resolve_effective_slots_with_provenance(class_def, schema);
            for (slot_name, override_def) in &class_def.slot_usage {
                // Only a refinement that names a range draws per-class
                // edges; a `slot_usage` that just tightens `required`
                // leaves the range inherited and rides the global
                // slot-side edges. (A pure `maximum_cardinality: 0`
                // refinement names no range and is suppressed downstream
                // regardless, so it correctly draws nothing.)
                let narrows_range = override_def.range.is_some() || !override_def.any_of.is_empty();
                if !narrows_range {
                    continue;
                }
                let Some(rs) = resolved.get(slot_name) else {
                    continue;
                };
                // A suppressed slot (`maximum_cardinality: 0`) draws no edge.
                if rs.induced.suppressed {
                    continue;
                }
                for range in &rs.induced.ranges {
                    if let Some(target) = self.resolve_range_target(schema, range) {
                        let key = (
                            format!("class:{}", class_name),
                            target.clone(),
                            slot_name.clone(),
                        );
                        if seen.insert(key.clone()) {
                            graph.edges.push(GraphEdge {
                                source: key.0,
                                target,
                                edge_type: EdgeType::Range,
                                label: Some(slot_name.clone()),
                            });
                        }
                    }
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
                rules: class_def
                    .rules
                    .iter()
                    .map(|r| {
                        let participants = crate::rules::rule_participants(r);
                        RuleSummary {
                            title: r.title.clone(),
                            description: r.description.clone(),
                            summary: crate::rules::rule_summary(r),
                            trigger_slots: participants.trigger,
                            governed_slots: participants.governed,
                        }
                    })
                    .collect(),
            });

            let (uri, uri_unresolved) = resolve_node_uri(schema, class_def.class_uri.as_deref());
            graph.nodes.push(GraphNode {
                id: format!("class:{}", name),
                label,
                node_type: NodeType::Class,
                color,
                description: class_def.description.clone(),
                uri,
                uri_unresolved,
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

            let cardinality = crate::linkml_resolve::effective_cardinality(slot_def);
            let kind_metadata = Some(KindMetadata::Slot {
                // Effective domains: the slot's own `domain:` or, the
                // common case, every class that lists it in `slots:` — so
                // the hover names all the slot's owning classes (its
                // domain connections), not just its range.
                domains: crate::linkml_resolve::resolve_slot_domains(schema, name, slot_def),
                range: slot_def.range.clone(),
                required: cardinality.required,
                multivalued: cardinality.multivalued,
                min: cardinality.min,
                max: cardinality.max,
                pattern: slot_def.pattern.clone(),
                identifier: slot_def.identifier,
                any_of: slot_def
                    .any_of
                    .iter()
                    .filter_map(|s| s.range.clone())
                    .collect(),
            });

            let (uri, uri_unresolved) = resolve_node_uri(schema, slot_def.slot_uri.as_deref());
            graph.nodes.push(GraphNode {
                id: format!("slot:{}", name),
                label,
                node_type: NodeType::Slot,
                color: NodeType::Slot.color(),
                description: slot_def.description.clone(),
                uri,
                uri_unresolved,
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

            // Add range edges (slot -> class/enum/type). A slot's range
            // can be a single `range:` or a polymorphic `any_of` union of
            // member ranges; draw one edge per distinct target so the
            // union members aren't left disconnected (a missing edge here
            // makes panschema's docs misrepresent the schema).
            if self.options.include_range_edges {
                let mut seen = std::collections::HashSet::new();
                let ranges = slot_def
                    .range
                    .iter()
                    .chain(slot_def.any_of.iter().filter_map(|s| s.range.as_ref()));
                for range in ranges {
                    if let Some(target) = self.resolve_range_target(schema, range)
                        && seen.insert(target.clone())
                    {
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
            let permissible_values = enum_def
                .permissible_values
                .iter()
                .map(|(text, pv)| PermissibleValueSummary {
                    text: text.clone(),
                    description: pv.description.clone(),
                    meaning: pv.meaning.as_deref().map(|m| {
                        crate::linkml_resolve::expand_curie(schema, m)
                            .unwrap_or_else(|| m.to_string())
                    }),
                })
                .collect();
            let kind_metadata = Some(KindMetadata::Enum { permissible_values });

            graph.nodes.push(GraphNode {
                id: format!("enum:{}", name),
                label: name.clone(),
                node_type: NodeType::Enum,
                color: NodeType::Enum.color(),
                description: enum_def.description.clone(),
                uri: None,
                uri_unresolved: false,
                is_abstract: false,
                kind_metadata,
            });
        }
    }

    /// Add type nodes and typeof edges
    fn add_types(&self, schema: &SchemaDefinition, graph: &mut GraphData) {
        for (name, type_def) in &schema.types {
            let (uri, uri_unresolved) = resolve_node_uri(schema, type_def.uri.as_deref());
            graph.nodes.push(GraphNode {
                id: format!("type:{}", name),
                label: name.clone(),
                node_type: NodeType::Type,
                color: NodeType::Type.color(),
                description: type_def.description.clone(),
                uri,
                uri_unresolved,
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

        crate::io::ensure_output_parent(output)?;
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

    // ========== Instance (A-box) graph ==========

    #[test]
    fn instance_graph_is_empty_when_the_schema_has_no_individuals() {
        let schema = SchemaDefinition::new("no-individuals");
        let graph = GraphWriter::new().schema_to_instance_graph(&schema);
        assert!(
            graph.nodes.is_empty() && graph.edges.is_empty(),
            "a schema with no individuals yields no instance graph"
        );
    }

    #[test]
    fn instance_graph_emits_typed_nodes_and_assertion_edges() {
        use crate::io::Reader;
        use crate::owl_reader::OwlReader;

        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/instance_graph.ttl");
        let schema = OwlReader::new().read(&fixture).expect("read fixture");
        let graph = GraphWriter::new().schema_to_instance_graph(&schema);

        // One node per individual, all of kind Individual — no T-box nodes.
        assert_eq!(graph.nodes.len(), 2, "two individuals → two nodes");
        assert!(
            graph
                .nodes
                .iter()
                .all(|n| n.node_type == NodeType::Individual),
            "the instance graph holds only Individual nodes"
        );
        let wine = graph
            .nodes
            .iter()
            .find(|n| n.id == "individual:chateauMorgon")
            .expect("wine individual node");

        // It carries its type (a resolvable class id) and its literal
        // assertion as hover metadata — not as an edge.
        match wine.kind_metadata.as_ref().expect("individual metadata") {
            KindMetadata::Individual { types, literals } => {
                assert_eq!(types, &["Wine"], "typed as its rdf:type class");
                assert_eq!(
                    literals,
                    &[PropertyLiteral {
                        property: "color".to_string(),
                        value: "red".to_string(),
                    }],
                    "the datatype assertion is node metadata, not an edge"
                );
            }
            other => panic!("expected Individual metadata, got {other:?}"),
        }

        // The object assertion (fromRegion → beaujolais) is exactly one
        // labelled edge between the two individual nodes.
        assert_eq!(
            graph.edges.len(),
            1,
            "one object-property assertion → one edge"
        );
        let edge = &graph.edges[0];
        assert_eq!(edge.source, "individual:chateauMorgon");
        assert_eq!(edge.target, "individual:beaujolais");
        assert_eq!(edge.edge_type, EdgeType::Assertion);
        assert_eq!(edge.label.as_deref(), Some("from region"));
    }

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
            uri_unresolved: false,
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
    ) -> (&'a [SlotSummary], &'a [String], &'a [String]) {
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
                ..
            } => (slots.as_slice(), parents.as_slice(), mixins.as_slice()),
            other => panic!("expected Class metadata, got {:?}", other),
        }
    }

    /// Pull the `rules` list from a class node's `KindMetadata::Class`.
    fn class_rules<'a>(graph: &'a GraphData, name: &str) -> &'a [RuleSummary] {
        let id = format!("class:{}", name);
        let node = graph
            .nodes
            .iter()
            .find(|n| n.id == id)
            .expect("class node should exist");
        match node.kind_metadata.as_ref().expect("class needs metadata") {
            KindMetadata::Class { rules, .. } => rules.as_slice(),
            other => panic!("expected Class metadata, got {:?}", other),
        }
    }

    #[test]
    fn class_kind_metadata_carries_rules_for_the_hover_payload() {
        use crate::linkml::{ClassRule, RuleConditions, SlotCondition, ValuePresence};
        // The graph export must carry class rules so the viz popup can show
        // them (today it emits nothing). Same full projection as the HTML
        // card: an `any_of` trigger and a `value_presence` consequence.
        let mut schema = SchemaDefinition::new("approvals");
        let mut cls = ClassDefinition::new("ImageApproval");
        let alt = |v: &str| RuleConditions {
            any_of: Vec::new(),
            slot_conditions: std::collections::BTreeMap::from([(
                "verdict".to_string(),
                SlotCondition {
                    equals_string: Some(v.to_string()),
                    ..Default::default()
                },
            )]),
        };
        cls.rules = vec![ClassRule {
            title: Some("attributed once decided".to_string()),
            description: None,
            preconditions: Some(RuleConditions {
                slot_conditions: std::collections::BTreeMap::new(),
                any_of: vec![alt("approved"), alt("rejected")],
            }),
            postconditions: Some(RuleConditions {
                any_of: Vec::new(),
                slot_conditions: std::collections::BTreeMap::from([(
                    "approved_by".to_string(),
                    SlotCondition {
                        value_presence: Some(ValuePresence::Present),
                        ..Default::default()
                    },
                )]),
            }),
        }];
        schema.classes.insert("ImageApproval".to_string(), cls);

        let graph = GraphWriter::new().schema_to_graph(&schema);
        let rules = class_rules(&graph, "ImageApproval");
        assert_eq!(
            rules.len(),
            1,
            "the class rule must appear in graph metadata"
        );
        assert_eq!(rules[0].title.as_deref(), Some("attributed once decided"));
        let summary = rules[0].summary.as_deref().expect("rule summary");
        assert!(
            summary.contains("approved") && summary.contains("rejected"),
            "trigger must render both any_of alternatives; got: {summary}"
        );
        assert!(
            summary.contains("approved_by") && summary.contains("is present"),
            "consequence must render the value_presence postcondition; got: {summary}"
        );
    }

    #[test]
    fn class_rules_carry_trigger_and_governed_participant_slots() {
        use crate::linkml::{ClassRule, RuleConditions, SlotCondition, ValuePresence};
        // For highlight-on-hover the viz needs the slots each rule touches,
        // split by side: trigger (precondition, incl. any_of) vs governed
        // (postcondition).
        let mut schema = SchemaDefinition::new("approvals");
        let mut cls = ClassDefinition::new("ImageApproval");
        let alt = |v: &str| RuleConditions {
            any_of: Vec::new(),
            slot_conditions: std::collections::BTreeMap::from([(
                "verdict".to_string(),
                SlotCondition {
                    equals_string: Some(v.to_string()),
                    ..Default::default()
                },
            )]),
        };
        cls.rules = vec![ClassRule {
            title: None,
            description: None,
            preconditions: Some(RuleConditions {
                slot_conditions: std::collections::BTreeMap::new(),
                any_of: vec![alt("approved"), alt("rejected")],
            }),
            postconditions: Some(RuleConditions {
                any_of: Vec::new(),
                slot_conditions: std::collections::BTreeMap::from([(
                    "approved_by".to_string(),
                    SlotCondition {
                        value_presence: Some(ValuePresence::Present),
                        ..Default::default()
                    },
                )]),
            }),
        }];
        schema.classes.insert("ImageApproval".to_string(), cls);

        let graph = GraphWriter::new().schema_to_graph(&schema);
        let rules = class_rules(&graph, "ImageApproval");
        assert_eq!(
            rules[0].trigger_slots,
            vec!["verdict"],
            "trigger slots come from the any_of precondition branches"
        );
        assert_eq!(
            rules[0].governed_slots,
            vec!["approved_by"],
            "governed slots come from the postconditions"
        );
    }

    #[test]
    fn class_kind_metadata_collects_inherited_slots_from_is_a_chain() {
        // Authors care about "what's the full set of fields on this
        // class?". The hover card surfaces every slot reachable
        // through `is_a` and `mixins`. Returned in alphabetical
        // (BTreeMap) order so the 5-slot summary is deterministic
        // and the persistent details panel takes over for longer
        // lists.
        let mut schema = SchemaDefinition::new("inheritance");
        schema
            .slots
            .insert("name".to_string(), SlotDefinition::new("name"));
        schema
            .slots
            .insert("breed".to_string(), SlotDefinition::new("breed"));

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
        let summary: Vec<(&str, Option<&str>)> = slots
            .iter()
            .map(|s| (s.name.as_str(), s.origin.as_deref()))
            .collect();
        assert_eq!(
            summary,
            vec![("breed", None), ("name", Some("Animal"))],
            "inherited entries carry their origin"
        );
        assert_eq!(parents, &["Animal".to_string()]);
        assert!(mixins.is_empty());
    }

    #[test]
    fn class_kind_metadata_walks_mixins_and_dedupes() {
        // Mixins flatten in: their slots show up in the consuming
        // class's resolved list. When the same slot name reaches a
        // class via two paths (its own slot ref plus a mixin), the
        // list keeps only one entry so the card doesn't show
        // duplicate rows.
        let mut schema = SchemaDefinition::new("mixin_resolve");
        schema
            .slots
            .insert("name".to_string(), SlotDefinition::new("name"));
        schema
            .slots
            .insert("age".to_string(), SlotDefinition::new("age"));

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
        let summary: Vec<(&str, Option<&str>)> = slots
            .iter()
            .map(|s| (s.name.as_str(), s.origin.as_deref()))
            .collect();
        assert_eq!(
            summary,
            vec![("age", None), ("name", Some("mixin Named"))],
            "the mixin path wins the dedup, so its origin is reported"
        );
        assert_eq!(mixins, &["Named".to_string()]);
    }

    #[test]
    fn class_kind_metadata_surfaces_slot_usage_refined_slots() {
        // The previous walker stopped at is_a + mixins + attributes +
        // slots:; slots introduced via `slot_usage` on a subclass were
        // invisible to the hover card. Sharing the resolver with the
        // Rust writer means slot_usage entries now show up in the
        // class's resolved slot list — this is the schema-author-facing
        // payoff for the resolver lift.
        let mut schema = SchemaDefinition::new("slot_usage_refine");
        schema.slots.insert(
            "wasGeneratedBy".to_string(),
            SlotDefinition::new("wasGeneratedBy"),
        );

        let mut activity = ClassDefinition::new("Activity");
        activity.slots = vec!["wasGeneratedBy".into()];
        schema.classes.insert("Activity".to_string(), activity);

        let mut question = ClassDefinition::new("Question");
        question.is_a = Some("Activity".into());
        // Refine wasGeneratedBy via slot_usage — the old walker missed
        // this contribution; the shared resolver catches it.
        let mut refined = SlotDefinition::new("wasGeneratedBy");
        refined.range = Some("QuestionFormation".into());
        question.slot_usage.insert("wasGeneratedBy".into(), refined);
        schema.classes.insert("Question".to_string(), question);

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);

        let (slots, _, _) = class_kind_metadata(&graph, "Question");
        let names: Vec<&str> = slots.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["wasGeneratedBy"],
            "wasGeneratedBy should appear in Question's resolved slots \
             via the slot_usage overlay even though it was inherited \
             from Activity"
        );
        assert_eq!(
            slots[0].range.as_deref(),
            Some("QuestionFormation"),
            "the entry must carry Question's refined range, not the \
             slot's global un-refined definition"
        );
        assert_eq!(
            slots[0].origin.as_deref(),
            Some("Activity"),
            "a refined inherited slot still points at its origin"
        );
    }

    #[test]
    fn slot_summary_carries_effective_cardinality_bounds() {
        // The class hover's per-slot entry reconciles explicit
        // cardinality bounds with the bool flags — a 1..3-bounded
        // slot reads as required and multi-valued with its bounds
        // on the wire.
        let mut schema = SchemaDefinition::new("bounds");
        let mut thing = ClassDefinition::new("Thing");
        let mut tags = SlotDefinition::new("tags");
        tags.minimum_cardinality = Some(1);
        tags.maximum_cardinality = Some(3);
        thing.attributes.insert("tags".into(), tags);
        schema.classes.insert("Thing".into(), thing);

        let graph = GraphWriter::new().schema_to_graph(&schema);
        let (slots, _, _) = class_kind_metadata(&graph, "Thing");
        assert_eq!(slots.len(), 1);
        let summary = &slots[0];
        assert!(summary.required, "min >= 1 reads as required");
        assert!(summary.multivalued, "max > 1 reads as multi-valued");
        assert_eq!(summary.min, Some(1));
        assert_eq!(summary.max, Some(3));
    }

    #[test]
    fn slot_node_metadata_carries_cardinality_bounds() {
        // The slot node's hover renders a Cardinality row; explicit
        // bounds ride the wire and reconcile the flags.
        let mut schema = SchemaDefinition::new("slot_bounds");
        let mut members = SlotDefinition::new("members");
        members.minimum_cardinality = Some(0);
        members.maximum_cardinality = Some(5);
        members.required = true; // bounds win: min 0 → not required
        schema.slots.insert("members".into(), members);

        let graph = GraphWriter::new().schema_to_graph(&schema);
        let node = graph
            .nodes
            .iter()
            .find(|n| n.id == "slot:members")
            .expect("slot node");
        match node.kind_metadata.as_ref().expect("slot metadata") {
            KindMetadata::Slot {
                required,
                multivalued,
                min,
                max,
                ..
            } => {
                assert!(!required, "explicit min 0 overrides the flag");
                assert!(multivalued, "max > 1 reads as multi-valued");
                assert_eq!(*min, Some(0));
                assert_eq!(*max, Some(5));
            }
            other => panic!("expected Slot metadata, got {other:?}"),
        }
    }

    #[test]
    fn slot_kind_metadata_captures_pattern_identifier_and_any_of() {
        // pattern / identifier / any_of are the constraint fields
        // authors edit most; the hover card surfaces them, so the
        // writer must carry them. any_of contributes its element
        // ranges (the polymorphic-range members), not the slots.
        let mut schema = SchemaDefinition::new("slot_constraints");
        let mut id_slot = SlotDefinition::new("identifier_slot");
        id_slot.pattern = Some("^ID:[0-9]+$".to_string());
        id_slot.identifier = true;
        let mut member_a = SlotDefinition::new("a");
        member_a.range = Some("Person".to_string());
        let mut member_b = SlotDefinition::new("b");
        member_b.range = Some("Organization".to_string());
        id_slot.any_of = vec![member_a, member_b];
        schema.slots.insert("identifier_slot".to_string(), id_slot);

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);
        let node = graph
            .nodes
            .iter()
            .find(|n| n.id == "slot:identifier_slot")
            .unwrap();
        match node.kind_metadata.as_ref().unwrap() {
            KindMetadata::Slot {
                pattern,
                identifier,
                any_of,
                ..
            } => {
                assert_eq!(pattern.as_deref(), Some("^ID:[0-9]+$"));
                assert!(*identifier);
                assert_eq!(
                    any_of,
                    &vec!["Person".to_string(), "Organization".to_string()]
                );
            }
            other => panic!("expected Slot metadata, got {:?}", other),
        }
    }

    #[test]
    fn any_of_range_draws_an_edge_per_union_member() {
        // A slot whose range is an `any_of` union must draw a range edge
        // to each member class, so union members aren't left disconnected
        // — the silent-correctness bug where panschema dropped `any_of`
        // ranges and the docs misrepresented the schema.
        let mut schema = SchemaDefinition::new("any_of_edges");
        schema
            .classes
            .insert("Dataset".to_string(), ClassDefinition::new("Dataset"));
        schema
            .classes
            .insert("Question".to_string(), ClassDefinition::new("Question"));
        let mut has_input = SlotDefinition::new("hasInput");
        let mut a = SlotDefinition::new("a");
        a.range = Some("Dataset".to_string());
        let mut b = SlotDefinition::new("b");
        b.range = Some("Question".to_string());
        has_input.any_of = vec![a, b];
        schema.slots.insert("hasInput".to_string(), has_input);

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);
        let mut range_targets: Vec<&str> = graph
            .edges
            .iter()
            .filter(|e| e.source == "slot:hasInput" && e.edge_type == EdgeType::Range)
            .map(|e| e.target.as_str())
            .collect();
        range_targets.sort_unstable();
        assert_eq!(
            range_targets,
            vec!["class:Dataset", "class:Question"],
            "one range edge per any_of member, no dupes"
        );
    }

    #[test]
    fn induced_range_draws_per_class_edges_and_skips_suppressed() {
        // A class that narrows an inherited `any_of` via `slot_usage`
        // gets range edges from itself to the induced members; a class
        // that suppresses the slot (`maximum_cardinality: 0`) gets none.
        // The global slot-side union edges stay (no regression).
        let mut schema = SchemaDefinition::new("induced_edges");
        for c in ["Dataset", "Question", "Result"] {
            schema
                .classes
                .insert(c.to_string(), ClassDefinition::new(c));
        }
        let mut has_input = SlotDefinition::new("hasInput");
        has_input.any_of = ["Dataset", "Question", "Result"]
            .iter()
            .map(|r| {
                let mut b = SlotDefinition::new("b");
                b.range = Some((*r).to_string());
                b
            })
            .collect();
        schema.slots.insert("hasInput".to_string(), has_input);

        let mut act = ClassDefinition::new("Act");
        act.slots = vec!["hasInput".to_string()];
        schema.classes.insert("Act".to_string(), act);

        // Analysis narrows to a single range; the graph draws Analysis -> Dataset.
        let mut analysis = ClassDefinition::new("Analysis");
        analysis.is_a = Some("Act".to_string());
        let mut narrow = SlotDefinition::new("hasInput");
        narrow.range = Some("Dataset".to_string());
        analysis.slot_usage.insert("hasInput".to_string(), narrow);
        schema.classes.insert("Analysis".to_string(), analysis);

        // DesignOfExperiment suppresses the slot; no edge from it.
        let mut design = ClassDefinition::new("DesignOfExperiment");
        design.is_a = Some("Act".to_string());
        let mut suppress = SlotDefinition::new("hasInput");
        suppress.maximum_cardinality = Some(0);
        design.slot_usage.insert("hasInput".to_string(), suppress);
        schema
            .classes
            .insert("DesignOfExperiment".to_string(), design);

        // Extraction narrows to a smaller union; two induced edges.
        let mut extraction = ClassDefinition::new("Extraction");
        extraction.is_a = Some("Act".to_string());
        let mut narrow_union = SlotDefinition::new("hasInput");
        narrow_union.any_of = ["Question", "Result"]
            .iter()
            .map(|r| {
                let mut b = SlotDefinition::new("b");
                b.range = Some((*r).to_string());
                b
            })
            .collect();
        extraction
            .slot_usage
            .insert("hasInput".to_string(), narrow_union);
        schema.classes.insert("Extraction".to_string(), extraction);

        // Tightened refines only `required` — no range change, so it
        // adds no per-class edge and rides the global slot-side edges.
        let mut tightened = ClassDefinition::new("Tightened");
        tightened.is_a = Some("Act".to_string());
        let mut req_only = SlotDefinition::new("hasInput");
        req_only.required = true;
        tightened
            .slot_usage
            .insert("hasInput".to_string(), req_only);
        schema.classes.insert("Tightened".to_string(), tightened);

        let graph = GraphWriter::new().schema_to_graph(&schema);
        let range_edge = |source: &str, target: &str| {
            graph
                .edges
                .iter()
                .any(|e| e.edge_type == EdgeType::Range && e.source == source && e.target == target)
        };

        // Induced edge for the narrowing class, only to its one member.
        assert!(
            range_edge("class:Analysis", "class:Dataset"),
            "narrowing class draws an edge to its induced range"
        );
        assert!(
            !range_edge("class:Analysis", "class:Question")
                && !range_edge("class:Analysis", "class:Result"),
            "narrowing class does NOT draw edges to the dropped union members"
        );

        // Union narrowing: two induced edges, only to the named members.
        let mut extraction_targets: Vec<&str> = graph
            .edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Range && e.source == "class:Extraction")
            .map(|e| e.target.as_str())
            .collect();
        extraction_targets.sort_unstable();
        assert_eq!(
            extraction_targets,
            vec!["class:Question", "class:Result"],
            "a narrowed union draws an edge per induced member"
        );

        // The suppressing class draws no induced range edge at all.
        assert!(
            !graph
                .edges
                .iter()
                .any(|e| e.edge_type == EdgeType::Range && e.source == "class:DesignOfExperiment"),
            "a suppressed slot draws no range edge from the class"
        );

        // A refinement that only tightens `required` names no range, so
        // it adds no per-class edge (it rides the global slot-side edges).
        assert!(
            !graph
                .edges
                .iter()
                .any(|e| e.edge_type == EdgeType::Range && e.source == "class:Tightened"),
            "a non-range refinement draws no per-class range edge"
        );

        // No regression: the global slot-side union edges are still there.
        let mut slot_targets: Vec<&str> = graph
            .edges
            .iter()
            .filter(|e| e.source == "slot:hasInput" && e.edge_type == EdgeType::Range)
            .map(|e| e.target.as_str())
            .collect();
        slot_targets.sort_unstable();
        assert_eq!(
            slot_targets,
            vec!["class:Dataset", "class:Question", "class:Result"],
            "the slot node keeps its global union edges"
        );
    }

    #[test]
    fn node_uri_is_expanded_or_flagged_unresolved() {
        // A curie with a declared prefix expands to the full IRI; a
        // curie with an unknown prefix stays verbatim and is flagged so
        // the hover can mark it `?`; a value already a full IRI is left
        // untouched.
        let mut schema = SchemaDefinition::new("uri_expansion");
        schema
            .prefixes
            .insert("prov".to_string(), "http://www.w3.org/ns/prov#".to_string());
        let mut expanded = ClassDefinition::new("Expanded");
        expanded.class_uri = Some("prov:Entity".to_string());
        schema.classes.insert("Expanded".to_string(), expanded);
        let mut unknown = ClassDefinition::new("Unknown");
        unknown.class_uri = Some("mystery:Thing".to_string());
        schema.classes.insert("Unknown".to_string(), unknown);
        let mut full = ClassDefinition::new("Full");
        full.class_uri = Some("http://example.org/Direct".to_string());
        schema.classes.insert("Full".to_string(), full);

        let writer = GraphWriter::new();
        let graph = writer.schema_to_graph(&schema);
        let node = |id: &str| graph.nodes.iter().find(|n| n.id == id).unwrap();

        let e = node("class:Expanded");
        assert_eq!(e.uri.as_deref(), Some("http://www.w3.org/ns/prov#Entity"));
        assert!(!e.uri_unresolved);

        let u = node("class:Unknown");
        assert_eq!(u.uri.as_deref(), Some("mystery:Thing"));
        assert!(u.uri_unresolved, "unknown prefix should be flagged");

        let f = node("class:Full");
        assert_eq!(f.uri.as_deref(), Some("http://example.org/Direct"));
        assert!(!f.uri_unresolved);
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
                domains,
                range,
                required,
                multivalued,
                min,
                max,
                ..
            } => {
                assert_eq!(domains, &vec!["Animal".to_string()]);
                assert_eq!(range.as_deref(), Some("Person"));
                assert!(*required);
                assert!(*multivalued);
                assert_eq!(*min, None, "no explicit bounds declared");
                assert_eq!(*max, None);
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
        schema
            .prefixes
            .insert("ex".to_string(), "http://example.org/".to_string());
        let mut severity = EnumDefinition::new("Severity");
        let mut low = PermissibleValue::new("low");
        low.description = Some("Low severity".to_string());
        severity.permissible_values.insert("low".to_string(), low);
        let mut high = PermissibleValue::new("high");
        high.meaning = Some("ex:High".to_string());
        severity.permissible_values.insert("high".to_string(), high);
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
                let low = permissible_values.iter().find(|p| p.text == "low").unwrap();
                assert_eq!(low.description.as_deref(), Some("Low severity"));
                let high = permissible_values
                    .iter()
                    .find(|p| p.text == "high")
                    .unwrap();
                // `meaning` is curie-expanded against the schema prefixes.
                assert_eq!(high.meaning.as_deref(), Some("http://example.org/High"));
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
