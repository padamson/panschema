//! HTML Writer
//!
//! Writes LinkML SchemaDefinition to HTML documentation.

use std::fs;
use std::path::Path;

use askama::Template;

use crate::graph_writer::GraphWriter;
use crate::io::{IoError, IoResult, Writer};
use crate::linkml::SchemaDefinition;
use crate::rust_writer::resolve_slots;

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
    pub mixins: Vec<EntityRef>,
    pub slots: Vec<SlotInClass>,
}

/// A slot as it appears on a specific class, with framing resolved for
/// rendering.
#[derive(Debug, Clone)]
pub struct SlotInClass {
    pub name: String,
    pub range: Option<RangeRef>,
    pub required: bool,
    pub multivalued: bool,
    /// Members of an `any_of` union; empty for single-range slots.
    pub any_of: Vec<RangeRef>,
    pub description: Option<String>,
    /// `true` when this class's `slot_usage` overrides an inherited slot.
    pub refined_here: bool,
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
    /// Graph viz aspect ratio components, rendered into the
    /// `.graph-container` CSS rule.
    graph_aspect_w: u32,
    graph_aspect_h: u32,
}

/// Writer for HTML documentation output
pub struct HtmlWriter {
    /// Whether to include graph visualization (default: true)
    pub include_graph: bool,
    /// Schema graph viz aspect ratio as `(width, height)`. Default 16:8
    /// — fits a typical laptop screen alongside browser chrome and an
    /// OS task bar. Consumers can override per-schema via the manifest's
    /// `html_graph_aspect = "W:H"` field.
    pub graph_aspect: (u32, u32),
}

/// Parse a `"W:H"` aspect-ratio string. Both components must be positive
/// integers and at most 9999 (a sanity cap; nothing useful needs more
/// digits and bigger values would suggest a typo such as a wall-clock
/// time slipped into the field).
pub fn parse_graph_aspect(s: &str) -> Result<(u32, u32), String> {
    let (w_str, h_str) = s
        .split_once(':')
        .ok_or_else(|| format!("aspect ratio `{s}` must be `W:H` (e.g. `16:9`)"))?;
    let w: u32 = w_str
        .trim()
        .parse()
        .map_err(|_| format!("aspect ratio width `{w_str}` is not a non-negative integer"))?;
    let h: u32 = h_str
        .trim()
        .parse()
        .map_err(|_| format!("aspect ratio height `{h_str}` is not a non-negative integer"))?;
    if w == 0 || h == 0 {
        return Err(format!("aspect ratio `{s}` must have non-zero components"));
    }
    if w > 9999 || h > 9999 {
        return Err(format!("aspect ratio `{s}` components must be <= 9999"));
    }
    Ok((w, h))
}

/// Embedded WASM visualization files (from panschema-viz build)
mod wasm_files {
    /// JavaScript bindings for WASM visualization
    pub const VIZ_JS: &str = include_str!("../../panschema-viz/pkg/panschema_viz.js");

    /// Compiled WASM binary
    pub const VIZ_WASM: &[u8] = include_bytes!("../../panschema-viz/pkg/panschema_viz_bg.wasm");
}

impl HtmlWriter {
    /// Create a new HTML writer with default options (graph enabled,
    /// 16:8 graph aspect ratio).
    pub fn new() -> Self {
        Self {
            include_graph: true,
            graph_aspect: (16, 8),
        }
    }

    /// Create a new HTML writer with custom options
    pub fn with_options(include_graph: bool) -> Self {
        Self {
            include_graph,
            graph_aspect: (16, 8),
        }
    }

    /// Override the schema graph viz aspect ratio. The writer accepts
    /// any pair of positive `u32`s; pre-validate strings via
    /// [`parse_graph_aspect`].
    #[must_use]
    pub fn with_graph_aspect(mut self, w: u32, h: u32) -> Self {
        self.graph_aspect = (w, h);
        self
    }

    /// Build template data from SchemaDefinition
    fn build_template_data(schema: &SchemaDefinition) -> TemplateData {
        let iri = schema.id.clone().unwrap_or_else(|| schema.name.clone());
        let title = schema.title.clone().unwrap_or_else(|| schema.name.clone());

        let namespaces = build_namespaces(schema, &iri);

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

            // Unresolved mixins (from un-loaded imports or typos) are
            // skipped: a broken `#class-X` anchor is worse than omission.
            let mixins: Vec<EntityRef> = class_def
                .mixins
                .iter()
                .filter_map(|mixin_id| {
                    schema.classes.get(mixin_id).map(|mixin_def| {
                        let mixin_label = mixin_def
                            .annotations
                            .get("panschema:label")
                            .cloned()
                            .unwrap_or_else(|| mixin_id.clone());
                        EntityRef {
                            id: mixin_id.clone(),
                            label: mixin_label,
                        }
                    })
                })
                .collect();

            let resolved = resolve_slots(class_def, schema);
            let slots: Vec<SlotInClass> = resolved
                .iter()
                .map(|(slot_name, slot_def)| SlotInClass {
                    name: slot_name.clone(),
                    range: slot_def.range.as_deref().map(|r| range_ref_for(r, schema)),
                    required: slot_def.required,
                    multivalued: slot_def.multivalued,
                    any_of: slot_def
                        .any_of
                        .iter()
                        .filter_map(|branch| {
                            branch
                                .range
                                .as_deref()
                                .or(slot_def.range.as_deref())
                                .map(|r| range_ref_for(r, schema))
                        })
                        .collect(),
                    description: slot_def
                        .description
                        .as_deref()
                        .map(|d| resolve_xrefs(d, schema)),
                    refined_here: class_def.slot_usage.contains_key(slot_name),
                })
                .collect();

            class_data_list.push(ClassData {
                id: (*class_id).clone(),
                label,
                iri: class_def
                    .class_uri
                    .clone()
                    .unwrap_or_else(|| (*class_id).clone()),
                description: class_def
                    .description
                    .as_deref()
                    .map(|d| resolve_xrefs(d, schema)),
                superclass,
                subclasses,
                mixins,
                slots,
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
            graph_aspect_w: self.graph_aspect.0,
            graph_aspect_h: self.graph_aspect.1,
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

/// Resolve `[[Name]]` markers in `text` against `schema` into anchor
/// links, returning HTML. Plain text is HTML-escaped; injected anchors
/// are not. Unresolved names pass through literally with a `<!-- WARNING -->`
/// comment so the gap is visible in the generated HTML source rather
/// than silently dropped.
fn resolve_xrefs(text: &str, schema: &SchemaDefinition) -> String {
    let mut out = String::with_capacity(text.len());
    let mut remainder = text;
    // `split_once` guarantees `after_open` strictly shrinks `remainder`
    // — important for the literal-`[[` fallthrough path below, which
    // would otherwise need an explicit forward-progress assertion.
    while let Some((before, after_open)) = remainder.split_once("[[") {
        push_escaped(&mut out, before);
        if let Some((name, after_close)) = after_open.split_once("]]")
            && is_xref_ident(name)
        {
            out.push_str(&render_xref(name, schema));
            remainder = after_close;
            continue;
        }
        // Not a valid xref — emit a literal `[[` and resume scanning
        // after it, so a later `[[Name]]` in the same string still
        // gets resolved.
        out.push_str("[[");
        remainder = after_open;
    }
    push_escaped(&mut out, remainder);
    out
}

fn is_xref_ident(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn push_escaped(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
}

/// Defaults are appended only for prefix names the schema didn't
/// declare, so generated docs that reference `xsd:string` etc. always
/// have a namespace entry even when the source schema is sparse.
fn build_namespaces(schema: &SchemaDefinition, schema_iri: &str) -> Vec<Namespace> {
    let mut out = Vec::with_capacity(schema.prefixes.len() + 5);
    out.push(Namespace {
        prefix: String::new(),
        iri: schema_iri.to_string(),
    });
    for (prefix, base) in &schema.prefixes {
        out.push(Namespace {
            prefix: prefix.clone(),
            iri: base.clone(),
        });
    }
    let defaults: &[(&str, &str)] = &[
        ("owl", "http://www.w3.org/2002/07/owl#"),
        ("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
        ("rdfs", "http://www.w3.org/2000/01/rdf-schema#"),
        ("xsd", "http://www.w3.org/2001/XMLSchema#"),
    ];
    for (prefix, iri) in defaults {
        if !schema.prefixes.contains_key(*prefix) {
            out.push(Namespace {
                prefix: (*prefix).to_string(),
                iri: (*iri).to_string(),
            });
        }
    }
    out
}

/// Map a LinkML range name onto a `RangeRef` — a class anchor link when
/// the range names a class declared in this schema, otherwise the bare
/// name as a datatype (covers LinkML primitives like `string`, `integer`,
/// `datetime`, plus enum / type names, plus unresolved CURIE-style refs
/// from imported schemas).
fn range_ref_for(range: &str, schema: &SchemaDefinition) -> RangeRef {
    if let Some(class_def) = schema.classes.get(range) {
        let label = class_def
            .annotations
            .get("panschema:label")
            .cloned()
            .unwrap_or_else(|| range.to_string());
        RangeRef {
            class_ref: Some(EntityRef {
                id: range.to_string(),
                label,
            }),
            datatype: String::new(),
        }
    } else {
        RangeRef {
            class_ref: None,
            datatype: range.to_string(),
        }
    }
}

fn render_xref(name: &str, schema: &SchemaDefinition) -> String {
    if schema.classes.contains_key(name) {
        format!(r##"<a href="#class-{name}" class="entity-ref class-ref">{name}</a>"##)
    } else if schema.enums.contains_key(name) {
        format!(r##"<a href="#enum-{name}" class="entity-ref enum-ref">{name}</a>"##)
    } else if schema.slots.contains_key(name) {
        format!(r##"<a href="#prop-{name}" class="entity-ref prop-ref">{name}</a>"##)
    } else {
        format!(
            "[[{name}]]<!-- WARNING: [[{name}]] does not resolve to a class, \
             enum, or slot in this schema -->"
        )
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
    fn html_writer_class_data_resolves_mixin_entity_refs() {
        use crate::linkml::{ClassDefinition, SchemaDefinition};

        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Auditable".to_string(), ClassDefinition::new("Auditable"));
        schema.classes.insert(
            "Publishable".to_string(),
            ClassDefinition::new("Publishable"),
        );
        let mut doc = ClassDefinition::new("Document");
        doc.mixins = vec!["Auditable".to_string(), "Publishable".to_string()];
        schema.classes.insert("Document".to_string(), doc);

        let data = HtmlWriter::build_template_data(&schema);
        let document = data.class_data.iter().find(|c| c.id == "Document").unwrap();
        let mixin_ids: Vec<&str> = document.mixins.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(mixin_ids, vec!["Auditable", "Publishable"]);
    }

    #[test]
    fn html_writer_class_data_skips_unresolved_mixin_refs() {
        // Anchor links to a missing class card would be broken; skip
        // is the conservative choice over emitting a dead link.
        use crate::linkml::{ClassDefinition, SchemaDefinition};

        let mut schema = SchemaDefinition::new("s");
        let mut doc = ClassDefinition::new("Document");
        doc.mixins = vec!["Phantom".to_string()];
        schema.classes.insert("Document".to_string(), doc);

        let data = HtmlWriter::build_template_data(&schema);
        let document = data.class_data.iter().find(|c| c.id == "Document").unwrap();
        assert!(
            document.mixins.is_empty(),
            "expected unresolved mixin to be skipped; got: {:?}",
            document.mixins
        );
    }

    #[test]
    fn resolve_xrefs_links_known_class_reference() {
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Question".to_string(), ClassDefinition::new("Question"));
        let html = resolve_xrefs("see [[Question]] for context", &schema);
        assert_eq!(
            html,
            r##"see <a href="#class-Question" class="entity-ref class-ref">Question</a> for context"##
        );
    }

    #[test]
    fn resolve_xrefs_links_known_enum_reference() {
        use crate::linkml::{EnumDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .enums
            .insert("ActStatus".to_string(), EnumDefinition::new("ActStatus"));
        let html = resolve_xrefs("captured by the [[ActStatus]] enum", &schema);
        assert!(
            html.contains(
                r##"<a href="#enum-ActStatus" class="entity-ref enum-ref">ActStatus</a>"##
            ),
            "expected enum anchor; got: {html}"
        );
    }

    #[test]
    fn resolve_xrefs_links_known_slot_reference() {
        use crate::linkml::{SchemaDefinition, SlotDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .slots
            .insert("status".to_string(), SlotDefinition::new("status"));
        let html = resolve_xrefs("the [[status]] slot", &schema);
        assert!(
            html.contains(r##"<a href="#prop-status" class="entity-ref prop-ref">status</a>"##),
            "expected slot anchor; got: {html}"
        );
    }

    #[test]
    fn resolve_xrefs_emits_warning_comment_for_unresolved_reference() {
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let html = resolve_xrefs("nothing here: [[Phantom]]", &schema);
        assert!(
            html.contains("[[Phantom]]"),
            "expected literal text; got: {html}"
        );
        assert!(
            html.contains("<!-- WARNING:"),
            "expected warning comment; got: {html}"
        );
    }

    #[test]
    fn resolve_xrefs_html_escapes_surrounding_plain_text() {
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let html = resolve_xrefs("if a < b & c > d", &schema);
        // < > & in plain text must be escaped so the output is safe to
        // mark `|safe` in the template.
        assert_eq!(html, "if a &lt; b &amp; c &gt; d");
    }

    #[test]
    fn resolve_xrefs_escapes_quotes_in_plain_text() {
        // Descriptions can contain " and ' (e.g., scimantic's PlannedAct
        // description includes `"Planned" denotes intentionality`).
        // Both must escape so the result is attribute-safe when marked
        // `|safe` in any template context.
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        assert_eq!(
            resolve_xrefs(r#"says "hi" and 'bye'"#, &schema),
            "says &quot;hi&quot; and &#39;bye&#39;"
        );
    }

    #[test]
    fn resolve_xrefs_rejects_invalid_idents() {
        // `[[Name]]` requires a LinkML-style ident: alphabetic or `_`
        // first char, alphanumeric or `_` continuation. Anything else
        // is treated as literal `[[...]]` text, not a cross-reference.
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        // Empty interior.
        assert_eq!(resolve_xrefs("[[]]", &schema), "[[]]");
        // Digit-leading.
        assert_eq!(resolve_xrefs("[[123abc]]", &schema), "[[123abc]]");
        // Contains a space (LinkML idents are alphanumeric + underscore).
        assert_eq!(resolve_xrefs("[[has space]]", &schema), "[[has space]]");
        // Contains a hyphen.
        assert_eq!(resolve_xrefs("[[a-b]]", &schema), "[[a-b]]");
    }

    #[test]
    fn resolve_xrefs_accepts_underscore_leading_ident() {
        // Underscore-leading idents are valid LinkML identifiers and
        // must resolve to a class card link when the class exists.
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("_Internal".to_string(), ClassDefinition::new("_Internal"));
        let html = resolve_xrefs("[[_Internal]]", &schema);
        assert!(
            html.contains(r##"<a href="#class-_Internal""##),
            "expected underscore-leading ident to resolve; got: {html}"
        );
    }

    #[test]
    fn resolve_xrefs_passes_lone_brackets_through() {
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        // A single `[` or `[[` without a matching `]]` isn't an xref.
        let html = resolve_xrefs("[note] and [[unclosed", &schema);
        assert_eq!(html, "[note] and [[unclosed");
    }

    #[test]
    fn build_template_data_resolves_xrefs_in_class_description() {
        use crate::linkml::{ClassDefinition, EnumDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .enums
            .insert("ActStatus".to_string(), EnumDefinition::new("ActStatus"));
        let mut planned = ClassDefinition::new("PlannedAct");
        planned.description = Some("lifecycle captured by the [[ActStatus]] enum".to_string());
        schema.classes.insert("PlannedAct".to_string(), planned);

        let data = HtmlWriter::build_template_data(&schema);
        let card = data
            .class_data
            .iter()
            .find(|c| c.id == "PlannedAct")
            .unwrap();
        let desc = card.description.as_deref().unwrap();
        assert!(
            desc.contains(r##"<a href="#enum-ActStatus""##),
            "expected resolved xref in class description; got: {desc}"
        );
    }

    #[test]
    fn build_namespaces_includes_schema_declared_prefixes() {
        use crate::linkml::SchemaDefinition;
        let mut schema = SchemaDefinition::new("s");
        schema.id = Some("http://example.org/s".to_string());
        schema.prefixes.insert(
            "cco".to_string(),
            "https://www.commoncoreontologies.org/".to_string(),
        );
        schema.prefixes.insert(
            "obo".to_string(),
            "http://purl.obolibrary.org/obo/".to_string(),
        );

        let ns = build_namespaces(&schema, "http://example.org/s");
        let by_prefix: std::collections::BTreeMap<&str, &str> = ns
            .iter()
            .map(|n| (n.prefix.as_str(), n.iri.as_str()))
            .collect();
        assert_eq!(
            by_prefix.get("cco"),
            Some(&"https://www.commoncoreontologies.org/")
        );
        assert_eq!(
            by_prefix.get("obo"),
            Some(&"http://purl.obolibrary.org/obo/")
        );
    }

    #[test]
    fn build_namespaces_appends_default_prefixes_when_schema_lacks_them() {
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let ns = build_namespaces(&schema, "http://example.org/s");
        let prefixes: Vec<&str> = ns.iter().map(|n| n.prefix.as_str()).collect();
        for default in ["owl", "rdf", "rdfs", "xsd"] {
            assert!(
                prefixes.contains(&default),
                "missing default prefix `{default}`; got: {prefixes:?}"
            );
        }
    }

    #[test]
    fn build_namespaces_lets_schema_prefix_override_default() {
        use crate::linkml::SchemaDefinition;
        let mut schema = SchemaDefinition::new("s");
        schema.prefixes.insert(
            "xsd".to_string(),
            "https://example.org/custom-xsd#".to_string(),
        );
        let ns = build_namespaces(&schema, "http://example.org/s");
        let xsd_entries: Vec<&Namespace> = ns.iter().filter(|n| n.prefix == "xsd").collect();
        assert_eq!(xsd_entries.len(), 1, "xsd must appear exactly once");
        assert_eq!(xsd_entries[0].iri, "https://example.org/custom-xsd#");
    }

    #[test]
    fn build_namespaces_keeps_schema_local_empty_prefix() {
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let ns = build_namespaces(&schema, "http://example.org/local");
        assert_eq!(
            ns.iter()
                .find(|n| n.prefix.is_empty())
                .map(|n| n.iri.as_str()),
            Some("http://example.org/local")
        );
    }

    #[test]
    fn class_data_lists_resolved_slots_with_framing() {
        use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};
        let mut schema = SchemaDefinition::new("s");
        let mut def = ClassDefinition::new("Question");
        let mut label = SlotDefinition::new("label");
        label.range = Some("string".to_string());
        label.required = true;
        def.attributes.insert("label".to_string(), label);
        let mut tags = SlotDefinition::new("tags");
        tags.range = Some("string".to_string());
        tags.multivalued = true;
        def.attributes.insert("tags".to_string(), tags);
        schema.classes.insert("Question".to_string(), def);

        let data = HtmlWriter::build_template_data(&schema);
        let card = data.class_data.iter().find(|c| c.id == "Question").unwrap();
        let by_name: std::collections::BTreeMap<&str, &SlotInClass> =
            card.slots.iter().map(|s| (s.name.as_str(), s)).collect();

        let label_slot = by_name["label"];
        assert!(label_slot.required);
        assert!(!label_slot.multivalued);
        assert!(!label_slot.refined_here);
        assert_eq!(label_slot.range.as_ref().unwrap().datatype, "string");

        let tags_slot = by_name["tags"];
        assert!(!tags_slot.required);
        assert!(tags_slot.multivalued);
    }

    #[test]
    fn class_data_flags_slot_usage_refinements_with_refined_here() {
        use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};
        let mut schema = SchemaDefinition::new("s");

        // Global slot defined as optional.
        let mut global = SlotDefinition::new("status");
        global.range = Some("string".to_string());
        schema.slots.insert("status".to_string(), global);

        // Parent declares the slot reference.
        let mut parent = ClassDefinition::new("Parent");
        parent.slots.push("status".to_string());
        schema.classes.insert("Parent".to_string(), parent);

        // Child inherits from Parent AND narrows `status` to required.
        let mut child = ClassDefinition::new("Child");
        child.is_a = Some("Parent".to_string());
        let mut override_def = SlotDefinition::new("status");
        override_def.required = true;
        child.slot_usage.insert("status".to_string(), override_def);
        schema.classes.insert("Child".to_string(), child);

        let data = HtmlWriter::build_template_data(&schema);
        let parent_card = data.class_data.iter().find(|c| c.id == "Parent").unwrap();
        let child_card = data.class_data.iter().find(|c| c.id == "Child").unwrap();

        // Parent has the slot but doesn't refine it.
        let parent_status = parent_card
            .slots
            .iter()
            .find(|s| s.name == "status")
            .unwrap();
        assert!(!parent_status.refined_here);
        assert!(!parent_status.required);

        // Child refines it: required = true AND refined_here = true.
        let child_status = child_card
            .slots
            .iter()
            .find(|s| s.name == "status")
            .unwrap();
        assert!(child_status.refined_here);
        assert!(child_status.required);
    }

    #[test]
    fn class_data_resolves_any_of_branches_into_range_refs() {
        use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Hypothesis".to_string(), ClassDefinition::new("Hypothesis"));
        schema
            .classes
            .insert("Evidence".to_string(), ClassDefinition::new("Evidence"));

        let mut def = ClassDefinition::new("DesignOfExperiment");
        let mut slot = SlotDefinition::new("hasInput");
        let mut hypothesis_branch = SlotDefinition::new("hasInput");
        hypothesis_branch.range = Some("Hypothesis".to_string());
        let mut evidence_branch = SlotDefinition::new("hasInput");
        evidence_branch.range = Some("Evidence".to_string());
        slot.any_of = vec![hypothesis_branch, evidence_branch];
        def.attributes.insert("hasInput".to_string(), slot);
        schema.classes.insert("DesignOfExperiment".to_string(), def);

        let data = HtmlWriter::build_template_data(&schema);
        let card = data
            .class_data
            .iter()
            .find(|c| c.id == "DesignOfExperiment")
            .unwrap();
        let slot = card.slots.iter().find(|s| s.name == "hasInput").unwrap();
        let any_of_ids: Vec<&str> = slot
            .any_of
            .iter()
            .filter_map(|r| r.class_ref.as_ref().map(|c| c.id.as_str()))
            .collect();
        assert_eq!(any_of_ids, vec!["Hypothesis", "Evidence"]);
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
    fn html_writer_emits_responsive_card_grid_and_aspect_ratio_graph() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();
        let writer = HtmlWriter::new();
        let temp_dir = std::env::temp_dir().join("panschema_responsive_layout_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("Write failed");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read");

        // Card grid uses `auto-fill` so it tiles at wide viewports and
        // collapses to one column when the minimum can't fit twice.
        assert!(
            html.contains("repeat(auto-fill, minmax(380px, 1fr))"),
            "responsive card grid template missing from rendered HTML"
        );
        // Graph container uses aspect-ratio instead of a fixed height
        // so it scales with the available content area. Default is 16:8
        // — fits a laptop screen plus browser chrome + OS task bar.
        // The ratio is set via a `--graph-aspect` custom property on the
        // container (inline) so the stylesheet stays valid CSS for IDE
        // linters, and the stylesheet reads from `var(...)` with the same
        // 16/8 fallback.
        assert!(
            html.contains("--graph-aspect: 16 / 8"),
            "graph container --graph-aspect inline custom property missing"
        );
        assert!(
            html.contains("aspect-ratio: var(--graph-aspect, 16 / 8)"),
            "graph container aspect-ratio CSS rule missing from rendered HTML"
        );
        // Old fixed-height rule must be gone.
        assert!(
            !html.contains("height: 500px"),
            "stale `height: 500px` rule still present in graph container CSS"
        );
        // `.content-area`'s hard max-width cap must be gone so the page
        // can expand fluidly with the viewport.
        assert!(
            !html.contains("max-width: var(--content-max-width)"),
            "content-area max-width cap still constrains the layout"
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn html_writer_with_graph_aspect_overrides_the_default() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();
        let writer = HtmlWriter::new().with_graph_aspect(4, 3);
        let temp_dir = std::env::temp_dir().join("panschema_aspect_override_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("Write failed");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read");
        assert!(
            html.contains("--graph-aspect: 4 / 3"),
            "expected overridden 4:3 aspect ratio in inline custom property"
        );
        // The stylesheet keeps `var(--graph-aspect, 16 / 8)` as a fallback
        // regardless of override (so the default applies if the inline
        // attribute is somehow stripped). The override is on the
        // container's inline style, asserted above.
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn parse_graph_aspect_accepts_valid_ratios() {
        assert_eq!(parse_graph_aspect("16:9").unwrap(), (16, 9));
        assert_eq!(parse_graph_aspect("16:8").unwrap(), (16, 8));
        assert_eq!(parse_graph_aspect("4:3").unwrap(), (4, 3));
        // Whitespace tolerance.
        assert_eq!(parse_graph_aspect(" 21 : 9 ").unwrap(), (21, 9));
        // Upper-bound boundary: the sanity cap is `<= 9999`, so 9999
        // itself must round-trip on both sides.
        assert_eq!(parse_graph_aspect("9999:1").unwrap(), (9999, 1));
        assert_eq!(parse_graph_aspect("1:9999").unwrap(), (1, 9999));
        assert_eq!(parse_graph_aspect("9999:9999").unwrap(), (9999, 9999));
    }

    #[test]
    fn parse_graph_aspect_rejects_malformed_input() {
        assert!(parse_graph_aspect("16").is_err(), "missing colon");
        assert!(parse_graph_aspect("16x9").is_err(), "wrong separator");
        assert!(parse_graph_aspect("16:0").is_err(), "zero height");
        assert!(parse_graph_aspect("0:9").is_err(), "zero width");
        assert!(parse_graph_aspect("a:b").is_err(), "non-numeric");
        assert!(parse_graph_aspect("10000:1").is_err(), "exceeds sanity cap");
        assert!(
            parse_graph_aspect("1:10000").is_err(),
            "exceeds sanity cap on height side"
        );
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
