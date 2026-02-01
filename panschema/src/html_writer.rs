//! HTML Writer
//!
//! Writes LinkML SchemaDefinition to HTML documentation.

use std::fs;
use std::path::Path;

use askama::Template;

use crate::graph_writer::GraphWriter;
use crate::io::{IoError, IoResult, Writer};
use crate::linkml::SchemaDefinition;

/// Entity reference for sidebar navigation.
#[derive(Debug, Clone)]
pub struct EntityRef {
    pub id: String,
    pub label: String,
}

/// Namespace prefix/IRI mapping.
#[derive(Debug, Clone)]
pub struct Namespace {
    pub prefix: String,
    pub iri: String,
}

/// Full class data for rendering class cards.
#[derive(Debug, Clone)]
pub struct ClassData {
    pub id: String,
    pub label: String,
    pub iri: String,
    pub description: Option<String>,
    pub superclass: Option<EntityRef>,
    pub subclasses: Vec<EntityRef>,
}

/// Range reference for property cards - either a class link or a datatype name.
#[derive(Debug, Clone)]
pub struct RangeRef {
    pub class_ref: Option<EntityRef>,
    pub datatype: String,
}

/// Full property data for rendering property cards.
#[derive(Debug, Clone)]
pub struct PropertyData {
    pub id: String,
    pub label: String,
    pub iri: String,
    pub property_type: String,
    pub description: Option<String>,
    pub domain: Option<EntityRef>,
    pub range: Option<RangeRef>,
    pub characteristics: Vec<String>,
}

/// A resolved property value for rendering individual cards.
#[derive(Debug, Clone)]
pub struct PropertyValueData {
    pub property_label: String,
    pub property_ref: Option<EntityRef>,
    pub value: String,
}

/// Full individual data for rendering individual cards.
#[derive(Debug, Clone)]
pub struct IndividualData {
    pub id: String,
    pub label: String,
    pub iri: String,
    pub description: Option<String>,
    pub types: Vec<EntityRef>,
    pub property_values: Vec<PropertyValueData>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    title: &'a str,
    iri: &'a str,
    version: Option<&'a str>,
    comment: Option<&'a str>,
    active_section: &'a str,
    classes: &'a [EntityRef],
    class_data: &'a [ClassData],
    properties: &'a [EntityRef],
    property_data: &'a [PropertyData],
    individuals: &'a [EntityRef],
    individual_data: &'a [IndividualData],
    namespaces: &'a [Namespace],
    /// Empty slice for class cards that don't have properties yet
    empty_properties: &'a [EntityRef],
    /// Graph data JSON for visualization (None = no graph)
    graph_json: Option<&'a str>,
    /// Number of nodes in the graph (for sidebar badge)
    graph_node_count: usize,
    /// Number of edges in the graph (for sidebar badge)
    graph_edge_count: usize,
}

/// Writer for HTML documentation output
pub struct HtmlWriter {
    /// Whether to include graph visualization (default: true)
    pub include_graph: bool,
}

/// Embedded WASM visualization files (from panschema-viz build)
mod wasm_files {
    /// JavaScript bindings for WASM visualization
    pub const VIZ_JS: &str = include_str!("../../panschema-viz/pkg/panschema_viz.js");

    /// Compiled WASM binary
    pub const VIZ_WASM: &[u8] = include_bytes!("../../panschema-viz/pkg/panschema_viz_bg.wasm");
}

impl HtmlWriter {
    /// Create a new HTML writer with default options (graph enabled)
    pub fn new() -> Self {
        Self {
            include_graph: true,
        }
    }

    /// Create a new HTML writer with custom options
    pub fn with_options(include_graph: bool) -> Self {
        Self { include_graph }
    }

    /// Build template data from SchemaDefinition
    fn build_template_data(schema: &SchemaDefinition) -> TemplateData {
        let iri = schema.id.clone().unwrap_or_else(|| schema.name.clone());
        let title = schema.title.clone().unwrap_or_else(|| schema.name.clone());

        // Build default namespaces
        let namespaces = vec![
            Namespace {
                prefix: "".to_string(),
                iri: iri.clone(),
            },
            Namespace {
                prefix: "owl".to_string(),
                iri: "http://www.w3.org/2002/07/owl#".to_string(),
            },
            Namespace {
                prefix: "rdf".to_string(),
                iri: "http://www.w3.org/1999/02/22-rdf-syntax-ns#".to_string(),
            },
            Namespace {
                prefix: "rdfs".to_string(),
                iri: "http://www.w3.org/2000/01/rdf-schema#".to_string(),
            },
            Namespace {
                prefix: "xsd".to_string(),
                iri: "http://www.w3.org/2001/XMLSchema#".to_string(),
            },
        ];

        // Build class data
        let mut class_refs = Vec::new();
        let mut class_data_list = Vec::new();

        // Sort classes by name for consistent ordering
        let mut sorted_classes: Vec<_> = schema.classes.iter().collect();
        sorted_classes.sort_by(|a, b| {
            let label_a = a.1.annotations.get("panschema:label").unwrap_or(a.0);
            let label_b = b.1.annotations.get("panschema:label").unwrap_or(b.0);
            label_a.cmp(label_b)
        });

        for (class_id, class_def) in &sorted_classes {
            let label = class_def
                .annotations
                .get("panschema:label")
                .cloned()
                .unwrap_or_else(|| (*class_id).clone());

            class_refs.push(EntityRef {
                id: (*class_id).clone(),
                label: label.clone(),
            });

            // Find superclass
            let superclass = class_def.is_a.as_ref().and_then(|parent_id| {
                schema.classes.get(parent_id).map(|parent| {
                    let parent_label = parent
                        .annotations
                        .get("panschema:label")
                        .cloned()
                        .unwrap_or_else(|| parent_id.clone());
                    EntityRef {
                        id: parent_id.clone(),
                        label: parent_label,
                    }
                })
            });

            // Find subclasses
            let subclasses: Vec<EntityRef> = schema
                .classes
                .iter()
                .filter(|(_, c)| c.is_a.as_ref() == Some(class_id))
                .map(|(sub_id, sub_def)| {
                    let sub_label = sub_def
                        .annotations
                        .get("panschema:label")
                        .cloned()
                        .unwrap_or_else(|| sub_id.clone());
                    EntityRef {
                        id: sub_id.clone(),
                        label: sub_label,
                    }
                })
                .collect();

            class_data_list.push(ClassData {
                id: (*class_id).clone(),
                label,
                iri: class_def
                    .class_uri
                    .clone()
                    .unwrap_or_else(|| (*class_id).clone()),
                description: class_def.description.clone(),
                superclass,
                subclasses,
            });
        }

        // Build property (slot) data
        let mut property_refs = Vec::new();
        let mut property_data_list = Vec::new();

        // Sort slots by label for consistent ordering
        let mut sorted_slots: Vec<_> = schema.slots.iter().collect();
        sorted_slots.sort_by(|a, b| {
            let label_a = a.1.annotations.get("panschema:label").unwrap_or(a.0);
            let label_b = b.1.annotations.get("panschema:label").unwrap_or(b.0);
            label_a.cmp(label_b)
        });

        for (slot_id, slot_def) in &sorted_slots {
            let label = slot_def
                .annotations
                .get("panschema:label")
                .cloned()
                .unwrap_or_else(|| (*slot_id).clone());

            property_refs.push(EntityRef {
                id: (*slot_id).clone(),
                label: label.clone(),
            });

            // Determine property type from annotation
            let property_type = slot_def
                .annotations
                .get("panschema:owl_property_type")
                .map(|t| {
                    if t == "ObjectProperty" {
                        "Object Property".to_string()
                    } else {
                        "Datatype Property".to_string()
                    }
                })
                .unwrap_or_else(|| "Property".to_string());

            // Resolve domain to class EntityRef
            let domain = slot_def.domain.as_ref().and_then(|domain_id| {
                schema.classes.get(domain_id).map(|c| {
                    let domain_label = c
                        .annotations
                        .get("panschema:label")
                        .cloned()
                        .unwrap_or_else(|| domain_id.clone());
                    EntityRef {
                        id: domain_id.clone(),
                        label: domain_label,
                    }
                })
            });

            // Resolve range
            let range = slot_def.range.as_ref().map(|range_id| {
                let class_ref = schema.classes.get(range_id).map(|c| {
                    let range_label = c
                        .annotations
                        .get("panschema:label")
                        .cloned()
                        .unwrap_or_else(|| range_id.clone());
                    EntityRef {
                        id: range_id.clone(),
                        label: range_label,
                    }
                });

                RangeRef {
                    class_ref,
                    datatype: range_id.clone(),
                }
            });

            // Build characteristics
            let mut characteristics = Vec::new();
            if let Some(inverse_id) = &slot_def.inverse {
                let inverse_label = schema
                    .slots
                    .get(inverse_id)
                    .and_then(|inv| inv.annotations.get("panschema:label"))
                    .cloned()
                    .unwrap_or_else(|| inverse_id.clone());
                characteristics.push(format!("Inverse of: {}", inverse_label));
            }

            property_data_list.push(PropertyData {
                id: (*slot_id).clone(),
                label,
                iri: slot_def
                    .slot_uri
                    .clone()
                    .unwrap_or_else(|| (*slot_id).clone()),
                property_type,
                description: slot_def.description.clone(),
                domain,
                range,
                characteristics,
            });
        }

        // Build individual data from annotations
        let mut individual_refs = Vec::new();
        let mut individual_data_list = Vec::new();

        if let Some(individuals_str) = schema.annotations.get("panschema:individuals") {
            for ind_id in individuals_str.split(',') {
                let ind_id = ind_id.trim();
                if ind_id.is_empty() {
                    continue;
                }

                // Get type IRIs from annotation
                let type_key = format!("panschema:individual:{}", ind_id);
                let type_iris: Vec<String> = schema
                    .annotations
                    .get(&type_key)
                    .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
                    .unwrap_or_default();

                // Resolve types to EntityRefs
                let types: Vec<EntityRef> = type_iris
                    .iter()
                    .filter_map(|type_iri| {
                        // Extract class ID from IRI
                        let type_id = type_iri
                            .rfind('#')
                            .map(|pos| &type_iri[pos + 1..])
                            .unwrap_or(type_iri);

                        schema.classes.get(type_id).map(|c| {
                            let type_label = c
                                .annotations
                                .get("panschema:label")
                                .cloned()
                                .unwrap_or_else(|| type_id.to_string());
                            EntityRef {
                                id: type_id.to_string(),
                                label: type_label,
                            }
                        })
                    })
                    .collect();

                // Get property values from annotations
                let mut property_values = Vec::new();
                let prefix = format!("panschema:individual:{}:", ind_id);
                for (key, value) in &schema.annotations {
                    if key.starts_with(&prefix) {
                        let prop_id = &key[prefix.len()..];

                        let prop_ref = schema.slots.get(prop_id).map(|slot| {
                            let prop_label = slot
                                .annotations
                                .get("panschema:label")
                                .cloned()
                                .unwrap_or_else(|| prop_id.to_string());
                            EntityRef {
                                id: prop_id.to_string(),
                                label: prop_label,
                            }
                        });

                        let property_label = prop_ref
                            .as_ref()
                            .map(|r| r.label.clone())
                            .unwrap_or_else(|| prop_id.to_string());

                        property_values.push(PropertyValueData {
                            property_label,
                            property_ref: prop_ref,
                            value: value.clone(),
                        });
                    }
                }

                // Sort property values by property_id
                property_values.sort_by(|a, b| a.property_label.cmp(&b.property_label));

                // For now, use id as label (could be stored in annotation)
                let label = ind_id.chars().next().map_or(ind_id.to_string(), |c| {
                    c.to_uppercase().to_string() + &ind_id[1..]
                });

                individual_refs.push(EntityRef {
                    id: ind_id.to_string(),
                    label: label.clone(),
                });

                individual_data_list.push(IndividualData {
                    id: ind_id.to_string(),
                    label,
                    iri: format!("{}#{}", iri, ind_id),
                    description: None,
                    types,
                    property_values,
                });
            }
        }

        TemplateData {
            title,
            iri,
            version: schema.version.clone(),
            comment: schema.description.clone(),
            namespaces,
            class_refs,
            class_data: class_data_list,
            property_refs,
            property_data: property_data_list,
            individual_refs,
            individual_data: individual_data_list,
        }
    }
}

impl Default for HtmlWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Container for all template data
struct TemplateData {
    title: String,
    iri: String,
    version: Option<String>,
    comment: Option<String>,
    namespaces: Vec<Namespace>,
    class_refs: Vec<EntityRef>,
    class_data: Vec<ClassData>,
    property_refs: Vec<EntityRef>,
    property_data: Vec<PropertyData>,
    individual_refs: Vec<EntityRef>,
    individual_data: Vec<IndividualData>,
}

impl Writer for HtmlWriter {
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()> {
        // Create output directory if it doesn't exist
        fs::create_dir_all(output).map_err(IoError::Io)?;

        let data = Self::build_template_data(schema);

        // Generate graph JSON for visualization (only if enabled)
        let (graph_json_string, graph_node_count, graph_edge_count) = if self.include_graph {
            let graph_data = GraphWriter::new().schema_to_graph(schema);
            let node_count = graph_data.nodes.len();
            let edge_count = graph_data.edges.len();
            let json =
                serde_json::to_string(&graph_data).map_err(|e| IoError::Write(e.to_string()))?;
            (Some(json), node_count, edge_count)
        } else {
            (None, 0, 0)
        };

        let template = IndexTemplate {
            title: &data.title,
            iri: &data.iri,
            version: data.version.as_deref(),
            comment: data.comment.as_deref(),
            active_section: "metadata",
            classes: &data.class_refs,
            class_data: &data.class_data,
            properties: &data.property_refs,
            property_data: &data.property_data,
            individuals: &data.individual_refs,
            individual_data: &data.individual_data,
            namespaces: &data.namespaces,
            empty_properties: &[],
            graph_json: graph_json_string.as_deref(),
            graph_node_count,
            graph_edge_count,
        };

        let html = template
            .render()
            .map_err(|e| IoError::Write(e.to_string()))?;

        let output_path = output.join("index.html");
        fs::write(&output_path, html).map_err(IoError::Io)?;

        // Copy WASM visualization files if graph is enabled
        if self.include_graph {
            fs::write(output.join("panschema_viz.js"), wasm_files::VIZ_JS).map_err(IoError::Io)?;
            fs::write(output.join("panschema_viz_bg.wasm"), wasm_files::VIZ_WASM)
                .map_err(IoError::Io)?;
        }

        Ok(())
    }

    fn format_id(&self) -> &str {
        "html"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::Reader;
    use crate::owl_reader::OwlReader;
    use std::path::PathBuf;

    fn reference_ontology_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("reference.ttl")
    }

    #[test]
    fn html_writer_format_id() {
        let writer = HtmlWriter::new();
        assert_eq!(writer.format_id(), "html");
    }

    #[test]
    fn html_writer_builds_template_data_from_schema() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let data = HtmlWriter::build_template_data(&schema);

        assert_eq!(data.title, "panschema Reference Ontology");
        assert!(data.iri.contains("panschema/reference"));
        assert_eq!(data.version, Some("0.2.0".to_string()));
    }

    #[test]
    fn html_writer_builds_class_data() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let data = HtmlWriter::build_template_data(&schema);

        // Should have 5 classes
        assert_eq!(data.class_refs.len(), 5);
        assert_eq!(data.class_data.len(), 5);

        // Find Dog class
        let dog = data.class_data.iter().find(|c| c.id == "Dog").unwrap();
        assert_eq!(dog.label, "Dog");
        assert!(dog.superclass.is_some());
        assert_eq!(dog.superclass.as_ref().unwrap().id, "Mammal");
    }

    #[test]
    fn html_writer_builds_property_data() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let data = HtmlWriter::build_template_data(&schema);

        // Should have 4 properties
        assert_eq!(data.property_refs.len(), 4);
        assert_eq!(data.property_data.len(), 4);

        // Find hasOwner property
        let has_owner = data
            .property_data
            .iter()
            .find(|p| p.id == "hasOwner")
            .unwrap();
        assert_eq!(has_owner.property_type, "Object Property");
        assert!(has_owner.domain.is_some());
        assert!(has_owner.range.is_some());
    }

    #[test]
    fn html_writer_builds_individual_data() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let data = HtmlWriter::build_template_data(&schema);

        // Should have 1 individual
        assert_eq!(data.individual_refs.len(), 1);
        assert_eq!(data.individual_data.len(), 1);

        let fido = &data.individual_data[0];
        assert_eq!(fido.id, "fido");
    }

    #[test]
    fn html_writer_writes_to_output_directory() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let writer = HtmlWriter::new();
        let temp_dir = std::env::temp_dir().join("panschema_html_writer_test");
        let _ = fs::remove_dir_all(&temp_dir);

        let result = writer.write(&schema, &temp_dir);
        assert!(result.is_ok(), "Write should succeed");

        let output_path = temp_dir.join("index.html");
        assert!(output_path.exists(), "index.html should be created");

        let html = fs::read_to_string(&output_path).expect("Failed to read output");
        assert!(
            html.contains("panschema Reference Ontology"),
            "HTML should contain title"
        );

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn html_writer_roundtrip_produces_valid_html() {
        // TTL → OwlReader → IR → HtmlWriter → HTML
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let writer = HtmlWriter::new();
        let temp_dir = std::env::temp_dir().join("panschema_roundtrip_test");
        let _ = fs::remove_dir_all(&temp_dir);

        writer.write(&schema, &temp_dir).expect("Write failed");

        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read");

        // Verify key elements are present
        assert!(html.contains("panschema Reference Ontology"));
        assert!(html.contains("0.2.0"));
        assert!(html.contains("class-Animal"));
        assert!(html.contains("class-Dog"));
        assert!(html.contains("prop-hasOwner"));
        assert!(html.contains("ind-fido"));

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn html_writer_includes_schema_graph_sidebar_with_counts() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let writer = HtmlWriter::new();
        let temp_dir = std::env::temp_dir().join("panschema_sidebar_graph_test");
        let _ = fs::remove_dir_all(&temp_dir);

        writer.write(&schema, &temp_dir).expect("Write failed");

        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read");

        // Verify Schema Graph link is in sidebar
        assert!(
            html.contains("href=\"#graph-visualization\""),
            "Sidebar should contain Schema Graph link"
        );
        assert!(
            html.contains("Schema Graph"),
            "Sidebar should contain 'Schema Graph' text"
        );

        // Verify the badge contains node/edge counts (format: "X / Y")
        // Reference ontology has 5 classes + 4 properties + 1 individual = nodes
        // and corresponding edges for subclass relationships, domain/range, etc.
        assert!(
            html.contains("<span class=\"badge\">"),
            "Sidebar should contain badge with counts"
        );

        // Schema Graph link should appear between Metadata and Namespaces
        let metadata_pos = html
            .find("href=\"#metadata\"")
            .expect("Metadata link not found");
        let graph_pos = html
            .find("href=\"#graph-visualization\"")
            .expect("Graph link not found");
        let namespaces_pos = html
            .find("href=\"#namespaces\"")
            .expect("Namespaces link not found");

        assert!(
            metadata_pos < graph_pos,
            "Schema Graph should appear after Metadata"
        );
        assert!(
            graph_pos < namespaces_pos,
            "Schema Graph should appear before Namespaces"
        );

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn html_writer_without_graph_excludes_sidebar_link() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let writer = HtmlWriter::with_options(false); // No graph
        let temp_dir = std::env::temp_dir().join("panschema_sidebar_no_graph_test");
        let _ = fs::remove_dir_all(&temp_dir);

        writer.write(&schema, &temp_dir).expect("Write failed");

        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read");

        // Schema Graph link should NOT be present when graph is disabled
        assert!(
            !html.contains("href=\"#graph-visualization\""),
            "Sidebar should not contain Schema Graph link when graph is disabled"
        );

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }
}
