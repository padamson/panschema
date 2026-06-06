//! HTML Writer
//!
//! Writes LinkML SchemaDefinition to HTML documentation.

use std::fs;
use std::path::Path;

use askama::Template;

use crate::graph_writer::GraphWriter;
use crate::io::{IoError, IoResult, Writer};
use crate::linkml::SchemaDefinition;
use crate::linkml_resolve::resolve_effective_slots as resolve_slots;

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
    /// Layout-algorithm identifier rendered into the
    /// `--graph-layout` CSS custom property. The JS picker reads this
    /// to set its initial selection.
    graph_default_layout: &'a str,
    /// Multi-version cohort context. When `Some`, the header gains a
    /// version dropdown and the body may show a stale/edge banner.
    /// Always `None` for the `panschema generate` path.
    version_context: Option<&'a VersionContext>,
    /// URL the header brand link targets. `"./"` for single-version
    /// output (page sits at the deploy root). `panschema publish`
    /// supplies this explicitly from the manifest's
    /// `[publishing].site_root_url` (default `"../current/"`).
    site_root_href: &'a str,
}

/// Per-page context describing the multi-version cohort this page is
/// part of. Drives the version-dropdown control in the header and the
/// "you're viewing X; current is Y" / "edge build" banners. Absent
/// (`None`) when the schema is rendered as a single-version output by
/// `panschema generate`; present (`Some(_)`) when rendered by
/// `panschema publish` for a versioned site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionContext {
    /// Ordered list of versions to show in the dropdown. Conventionally
    /// edge first (if present), then released versions newest to oldest.
    pub all_versions: Vec<String>,
    /// The version this specific page is rendered for.
    pub viewing: String,
    /// The version `current/` aliases. Used to decide whether to show
    /// the "you're viewing X; current is Y" banner.
    pub current: String,
    /// Edge ref name (e.g. `"main"`), if the cohort includes an edge
    /// build. Pages rendered for this ref get the "edge build from HEAD"
    /// banner.
    pub edge: Option<String>,
    /// URL template with a literal `{version}` placeholder. The dropdown
    /// JS substitutes each option's value to form cross-version links.
    pub url_pattern: String,
}

impl VersionContext {
    /// Substitute `{version}` in `url_pattern` to produce a navigation
    /// URL for the given version.
    pub fn url_for(&self, version: &str) -> String {
        self.url_pattern.replace("{version}", version)
    }

    /// `true` if `version` is the cohort's `edge` ref. Templates use
    /// this to badge the edge entry in the dropdown.
    pub fn is_edge(&self, version: &str) -> bool {
        self.edge.as_deref() == Some(version)
    }

    /// `true` when the page being rendered is the cohort's `current`
    /// version (so the stale-banner can be suppressed).
    pub fn viewing_is_current(&self) -> bool {
        self.viewing == self.current
    }

    /// `true` when the page being rendered is the cohort's edge build
    /// (so the edge-banner can be shown).
    pub fn viewing_is_edge(&self) -> bool {
        self.edge.as_deref() == Some(self.viewing.as_str())
    }
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
    /// Layout-algorithm identifier (e.g. `"sgd"` / `"force-directed"`)
    /// for the initial value of the graph-viz layout picker. Consumers
    /// override per-schema via the manifest's `html_default_layout`
    /// field. Defaults to `"sgd"` — visibly the best quality-per-time
    /// for typical schema graphs (cleaner cluster separation than
    /// force-directed at lower init cost than stress). The JS picker
    /// falls back to force-directed in 3D mode since SGD is 2D-only.
    pub graph_default_layout: String,
    /// Optional multi-version cohort context. Set by `panschema publish`;
    /// `None` for the single-version `panschema generate` path. When
    /// present, the rendered page gains a version dropdown in the header
    /// and a banner when `viewing` differs from `current` or matches `edge`.
    pub version_context: Option<VersionContext>,
    /// Override for the header brand link target. `None` means use the
    /// per-flow default: `"./"` for single-version output (page sits at
    /// the deploy root) — `panschema publish` always sets this explicitly
    /// from the manifest's `site_root_url`.
    pub site_root_href: Option<String>,
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
    /// 16:8 graph aspect ratio, SGD default layout — see
    /// [`Self::graph_default_layout`] for the choice).
    pub fn new() -> Self {
        Self {
            include_graph: true,
            graph_aspect: (16, 8),
            graph_default_layout: "sgd".to_string(),
            version_context: None,
            site_root_href: None,
        }
    }

    /// Create a new HTML writer with custom options
    pub fn with_options(include_graph: bool) -> Self {
        Self {
            include_graph,
            graph_aspect: (16, 8),
            graph_default_layout: "sgd".to_string(),
            version_context: None,
            site_root_href: None,
        }
    }

    /// Attach a multi-version cohort context. Used by `panschema publish`
    /// to inject the dropdown + banner UX into each per-version page.
    #[must_use]
    pub fn with_version_context(mut self, ctx: VersionContext) -> Self {
        self.version_context = Some(ctx);
        self
    }

    /// Override the header brand-link target. Consumed by
    /// `panschema publish` to forward the manifest's `site_root_url`
    /// into each per-version page.
    #[must_use]
    pub fn with_site_root_href(mut self, href: impl Into<String>) -> Self {
        self.site_root_href = Some(href.into());
        self
    }

    /// Override the schema graph viz aspect ratio. The writer accepts
    /// any pair of positive `u32`s; pre-validate strings via
    /// [`parse_graph_aspect`].
    #[must_use]
    pub fn with_graph_aspect(mut self, w: u32, h: u32) -> Self {
        self.graph_aspect = (w, h);
        self
    }

    /// Override the default layout algorithm for the graph picker.
    /// Pre-validate via [`crate::manifest::validate_layout_name`] —
    /// this method does not re-check, on the assumption the value
    /// already passed manifest parsing.
    #[must_use]
    pub fn with_default_layout(mut self, name: impl Into<String>) -> Self {
        self.graph_default_layout = name.into();
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
                        .map(|d| render_description(d, schema)),
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
                    .map(|d| render_description(d, schema)),
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
                description: slot_def
                    .description
                    .as_deref()
                    .map(|d| render_description(d, schema)),
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
            comment: schema
                .description
                .as_deref()
                .map(|d| render_description(d, schema)),
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
            graph_default_layout: &self.graph_default_layout,
            version_context: self.version_context.as_ref(),
            // `panschema generate` writes the page at the output root, so
            // `./` always resolves to the deploy root. `panschema publish`
            // sets this explicitly from the manifest's `site_root_url`.
            site_root_href: self.site_root_href.as_deref().unwrap_or("./"),
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

/// Render a LinkML `description:` value to HTML. Runs CommonMark
/// markdown over the input then expands `[[Name]]` cross-reference
/// markers against `schema` into anchor links.
///
/// Markdown handles inline links (`[text](url)`), emphasis
/// (`**bold**`, `*italic*`), code spans, and block constructs
/// (paragraphs, lists, fenced code). Raw HTML embedded in
/// descriptions is escaped — `<a href="…">…</a>` typed by the
/// author renders as literal angle-bracket text, not a real anchor.
/// Authors who need a clickable link use markdown syntax instead.
///
/// `[[Name]]` markers pass through markdown as plain text (no
/// markdown construct starts with `[[`), so post-processing the
/// rendered HTML to substitute them is safe — they only appear in
/// text nodes, never inside tag attributes.
fn render_description(text: &str, schema: &SchemaDefinition) -> String {
    use pulldown_cmark::{Event, Parser, html};

    // Route raw HTML through text escaping so author-embedded
    // `<a href="…">` cannot inject markup into the output. The
    // pulldown-cmark HTML renderer escapes `< > &` in `Event::Text`
    // automatically.
    let events = Parser::new(text).map(|ev| match ev {
        Event::Html(s) | Event::InlineHtml(s) => Event::Text(s),
        other => other,
    });
    let mut rendered = String::with_capacity(text.len());
    html::push_html(&mut rendered, events);
    substitute_xref_markers(&rendered, schema)
}

/// Walk the markdown-rendered HTML, replacing `[[Name]]` markers
/// (which markdown passes through as text — see [`render_description`])
/// with anchor links. Plain text outside markers is left as-is; it has
/// already been HTML-escaped by the markdown renderer.
fn substitute_xref_markers(html: &str, schema: &SchemaDefinition) -> String {
    let mut out = String::with_capacity(html.len());
    let mut remainder = html;
    while let Some((before, after_open)) = remainder.split_once("[[") {
        out.push_str(before);
        if let Some((name, after_close)) = after_open.split_once("]]")
            && is_xref_ident(name)
        {
            out.push_str(&render_xref(name, schema));
            remainder = after_close;
            continue;
        }
        out.push_str("[[");
        remainder = after_open;
    }
    out.push_str(remainder);
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

    fn cohort_context(viewing: &str, current: &str, edge: Option<&str>) -> VersionContext {
        VersionContext {
            all_versions: vec!["main".into(), "v0.2.0".into(), "v0.1.0".into()],
            viewing: viewing.into(),
            current: current.into(),
            edge: edge.map(String::from),
            url_pattern: "/schema/{version}/".into(),
        }
    }

    #[test]
    fn html_writer_default_layout_is_sgd() {
        // SGD is the picker default — visibly better cluster
        // separation than force-directed on schema graphs. The
        // manifest's `html_default_layout` field still overrides at
        // generate time; this test pins the in-tree fallback so a
        // future regression that flips it back to "force-directed"
        // without a deliberate decision will fail loudly.
        assert_eq!(HtmlWriter::new().graph_default_layout, "sgd");
        assert_eq!(HtmlWriter::with_options(true).graph_default_layout, "sgd");
        assert_eq!(HtmlWriter::with_options(false).graph_default_layout, "sgd");
    }

    #[test]
    fn version_context_is_edge_matches_only_edge_ref() {
        let vc = cohort_context("v0.1.0", "v0.2.0", Some("main"));
        assert!(vc.is_edge("main"));
        assert!(!vc.is_edge("v0.1.0"));
        assert!(!vc.is_edge("v0.2.0"));
        assert!(!vc.is_edge("not-a-ref"));

        // When `edge` is None, nothing is the edge — every probe returns false.
        let vc_no_edge = cohort_context("v0.1.0", "v0.2.0", None);
        assert!(!vc_no_edge.is_edge("main"));
        assert!(!vc_no_edge.is_edge("v0.1.0"));
    }

    #[test]
    fn version_context_viewing_predicates_distinguish_current_and_edge() {
        let viewing_current = cohort_context("v0.2.0", "v0.2.0", Some("main"));
        assert!(viewing_current.viewing_is_current());
        assert!(!viewing_current.viewing_is_edge());

        let viewing_edge = cohort_context("main", "v0.2.0", Some("main"));
        assert!(!viewing_edge.viewing_is_current());
        assert!(viewing_edge.viewing_is_edge());

        let viewing_stale = cohort_context("v0.1.0", "v0.2.0", Some("main"));
        assert!(!viewing_stale.viewing_is_current());
        assert!(!viewing_stale.viewing_is_edge());
    }

    #[test]
    fn version_context_url_for_substitutes_version_placeholder() {
        let vc = cohort_context("v0.1.0", "v0.2.0", None);
        assert_eq!(vc.url_for("v0.2.0"), "/schema/v0.2.0/");
        assert_eq!(vc.url_for("main"), "/schema/main/");
        // A pattern without the placeholder is returned unchanged.
        let vc_no_placeholder = VersionContext {
            url_pattern: "/static-url".into(),
            ..vc
        };
        assert_eq!(vc_no_placeholder.url_for("v0.2.0"), "/static-url");
    }

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
    fn html_writer_emits_single_version_brand_link_as_dot_slash() {
        // `panschema generate` writes index.html at the output root,
        // so the brand link must be `./` — equivalent to the deploy
        // root from that path. The versioned `panschema publish`
        // path is exercised separately in publish::tests.
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();
        let out = tempfile::tempdir().unwrap();
        let writer = HtmlWriter::with_options(false);
        crate::io::Writer::write(&writer, &schema, out.path()).unwrap();
        let html = std::fs::read_to_string(out.path().join("index.html")).unwrap();
        assert!(
            html.contains(r#"<a href="./" class="site-title""#),
            "single-version output must use `./` brand link"
        );
        assert!(
            !html.contains(r#"<a href="/" class="site-title""#),
            "absolute brand link must not appear"
        );
    }

    #[test]
    fn html_writer_renders_schema_description_markdown_as_live_html() {
        // The schema-level description is mounted into the metadata
        // card. Like the entity cards, the writer hands the template
        // already-rendered HTML from `render_description`, so the
        // template must mount it via `|safe` — otherwise Askama
        // double-escapes the writer's output and the user sees the
        // literal `<p>…<a href="…">…</a>` markup as visible text
        // instead of a live link.
        use crate::linkml::SchemaDefinition;
        let mut schema = SchemaDefinition::new("s");
        schema.id = Some("http://example.org/s".to_string());
        schema.description = Some(
            "see the [book](https://example.org/book) for context — Noy & McGuinness".to_string(),
        );
        let out = tempfile::tempdir().unwrap();
        let writer = HtmlWriter::with_options(false);
        crate::io::Writer::write(&writer, &schema, out.path()).unwrap();
        let html = std::fs::read_to_string(out.path().join("index.html")).unwrap();

        assert!(
            html.contains(r#"<a href="https://example.org/book">book</a>"#),
            "schema description markdown link must render as a live anchor; got: {html}"
        );
        // Double-escape signature: any of the writer-produced markup
        // appearing as escaped text means Askama escaped it a second
        // time. `&lt;a ` would mean the anchor's own `<a` got escaped;
        // `&amp;amp;` / `&#38;amp;` would mean the writer's `&amp;` got
        // re-escaped.
        assert!(
            !html.contains("&lt;a "),
            "rendered anchor must not be re-escaped; got: {html}"
        );
        assert!(
            !html.contains("&amp;amp;") && !html.contains("&#38;amp;"),
            "ampersand must not be double-escaped; got: {html}"
        );
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
    fn render_description_links_known_class_reference() {
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Question".to_string(), ClassDefinition::new("Question"));
        let html = render_description("see [[Question]] for context", &schema);
        assert!(
            html.contains(
                r##"<a href="#class-Question" class="entity-ref class-ref">Question</a>"##
            ),
            "expected class anchor; got: {html}"
        );
    }

    #[test]
    fn render_description_links_known_enum_reference() {
        use crate::linkml::{EnumDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .enums
            .insert("ActStatus".to_string(), EnumDefinition::new("ActStatus"));
        let html = render_description("captured by the [[ActStatus]] enum", &schema);
        assert!(
            html.contains(
                r##"<a href="#enum-ActStatus" class="entity-ref enum-ref">ActStatus</a>"##
            ),
            "expected enum anchor; got: {html}"
        );
    }

    #[test]
    fn render_description_links_known_slot_reference() {
        use crate::linkml::{SchemaDefinition, SlotDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .slots
            .insert("status".to_string(), SlotDefinition::new("status"));
        let html = render_description("the [[status]] slot", &schema);
        assert!(
            html.contains(r##"<a href="#prop-status" class="entity-ref prop-ref">status</a>"##),
            "expected slot anchor; got: {html}"
        );
    }

    #[test]
    fn render_description_emits_warning_comment_for_unresolved_reference() {
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let html = render_description("nothing here: [[Phantom]]", &schema);
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
    fn render_description_html_escapes_surrounding_plain_text() {
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let html = render_description("if a < b & c > d", &schema);
        // `< > &` in body content must be escaped — the rendered HTML
        // is mounted via `|safe` in entity descriptions, so the writer
        // can't lean on Askama for escaping. `"` and `'` are body-safe
        // in HTML5 and pass through, matching CommonMark output.
        assert!(html.contains("&lt;"), "got: {html}");
        assert!(html.contains("&amp;"), "got: {html}");
        assert!(html.contains("&gt;"), "got: {html}");
    }

    #[test]
    fn render_description_passes_body_safe_quotes_through() {
        // Descriptions land in element body content (mounted into
        // `<div class="entity-description">…</div>`), where `"` and
        // `'` are HTML5-safe and need no escape. CommonMark output
        // matches; we keep authors' quote characters readable in the
        // rendered source instead of `&quot;`/`&#39;`-encoding them.
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let html = render_description(r#"says "hi" and 'bye'"#, &schema);
        assert!(html.contains(r#"says "hi" and 'bye'"#), "got: {html}");
    }

    #[test]
    fn render_description_rejects_invalid_xref_idents() {
        // `[[Name]]` requires a LinkML-style ident: alphabetic or `_`
        // first char, alphanumeric or `_` continuation. Anything else
        // is treated as literal `[[...]]` text, not a cross-reference.
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        assert!(render_description("[[]]", &schema).contains("[[]]"));
        assert!(render_description("[[123abc]]", &schema).contains("[[123abc]]"));
        assert!(render_description("[[has space]]", &schema).contains("[[has space]]"));
        assert!(render_description("[[a-b]]", &schema).contains("[[a-b]]"));
    }

    #[test]
    fn render_description_accepts_underscore_leading_xref_ident() {
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("_Internal".to_string(), ClassDefinition::new("_Internal"));
        let html = render_description("[[_Internal]]", &schema);
        assert!(
            html.contains(r##"<a href="#class-_Internal""##),
            "expected underscore-leading ident to resolve; got: {html}"
        );
    }

    #[test]
    fn render_description_passes_lone_brackets_through() {
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let html = render_description("[note] and [[unclosed", &schema);
        assert!(html.contains("[note] and [[unclosed"), "got: {html}");
    }

    #[test]
    fn render_description_renders_markdown_inline_links() {
        // `[text](url)` is the canonical markdown affordance for
        // embedding a clickable link in a description. Before this
        // slice, descriptions escaped all markup so the only way to
        // reference another schema entity was the in-band `[[Name]]`
        // marker. Markdown links cover external URLs that don't fit
        // the xref mechanism (book chapters, papers, glossaries).
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let html = render_description("see the [book](../../) for context", &schema);
        assert!(
            html.contains(r#"<a href="../../">book</a>"#),
            "expected rendered markdown link; got: {html}"
        );
    }

    #[test]
    fn render_description_renders_markdown_emphasis_and_code() {
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let html = render_description("**bold** and *italic* and `code`", &schema);
        assert!(
            html.contains("<strong>bold</strong>"),
            "expected bold; got: {html}"
        );
        assert!(
            html.contains("<em>italic</em>"),
            "expected italic; got: {html}"
        );
        assert!(
            html.contains("<code>code</code>"),
            "expected code; got: {html}"
        );
    }

    #[test]
    fn render_description_escapes_raw_html_embedded_by_author() {
        // HTML safety policy: markdown only. Raw HTML in descriptions
        // is escaped so an author can't smuggle markup (or worse,
        // scripts) into the rendered page. The schema author who needs
        // a link uses markdown `[text](url)` syntax instead.
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let html = render_description(r#"plain <a href="evil.html">click</a> tail"#, &schema);
        assert!(
            !html.contains(r#"<a href="evil.html">"#),
            "raw HTML must not survive verbatim; got: {html}"
        );
        // The literal `<a` opener must be escaped in the rendered output.
        assert!(
            html.contains("&lt;a "),
            "raw HTML must be escaped; got: {html}"
        );
    }

    #[test]
    fn render_description_preserves_xref_inside_markdown_link_text() {
        // A `[[ClassName]]` marker nested inside a markdown link's
        // text is processed by xref expansion after markdown renders,
        // so the anchor's display text becomes the resolved entity
        // link. Verifies the ordering decision: markdown first, then
        // xref substitution against the rendered HTML.
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Question".to_string(), ClassDefinition::new("Question"));
        let html = render_description("[via [[Question]]](../../)", &schema);
        // Outer markdown link survives.
        assert!(html.contains(r#"<a href="../../">"#), "got: {html}");
        // Inner xref also resolves.
        assert!(
            html.contains(r##"<a href="#class-Question""##),
            "got: {html}"
        );
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
