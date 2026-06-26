//! Component rendering for isolated preview and testing.
//!
//! This module provides the ability to render individual UI components
//! in isolation, enabling:
//! - Snapshot testing of component HTML output
//! - Style guide generation
//! - Component-driven development workflow
//!
//! # Adding New Components
//!
//! 1. Create the template in `templates/components/your_component.html`
//! 2. Add the Askama template struct in this module
//! 3. Add a render method to `ComponentRenderer`
//! 4. Add the component to the style guide template
//! 5. Write snapshot tests for the component

// Allow dead code in this module - these components and renderer methods are
// infrastructure for the styleguide and component testing. They are used via
// tests gated behind #[cfg(feature = "dev")] and through the styleguide command.
#![allow(dead_code)]

use askama::Template;

/// A reference to an entity (class, property, individual) used in cross-references.
#[derive(Debug, Clone)]
pub struct EntityRef {
    pub id: String,
    pub label: String,
}

impl EntityRef {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

/// Namespace prefix/IRI mapping.
#[derive(Debug, Clone)]
pub struct Namespace {
    pub prefix: String,
    pub iri: String,
}

impl Namespace {
    pub fn new(prefix: impl Into<String>, iri: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            iri: iri.into(),
        }
    }
}

/// A property value specification for individual card previews.
#[derive(Debug, Clone)]
pub struct PropertyValueSpec {
    pub property_label: String,
    pub property_ref: Option<EntityRef>,
    pub value: String,
}

impl PropertyValueSpec {
    pub fn new(
        label: impl Into<String>,
        property_ref: Option<EntityRef>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            property_label: label.into(),
            property_ref,
            value: value.into(),
        }
    }
}

/// Range specification for a property (either a class ref or datatype string).
#[derive(Debug, Clone)]
pub struct RangeSpec {
    pub class_ref: Option<EntityRef>,
    pub datatype: String,
}

impl RangeSpec {
    pub fn class(class_ref: EntityRef) -> Self {
        Self {
            class_ref: Some(class_ref),
            datatype: String::new(),
        }
    }

    pub fn datatype(datatype: impl Into<String>) -> Self {
        Self {
            class_ref: None,
            datatype: datatype.into(),
        }
    }
}

/// Sample data for component previews and testing.
#[derive(Debug, Clone)]
pub struct SampleData {
    pub title: String,
    pub iri: String,
    pub version: Option<String>,
    pub comment: Option<String>,
    pub classes: Vec<EntityRef>,
    pub slots: Vec<EntityRef>,
    pub individuals: Vec<EntityRef>,
    pub namespaces: Vec<Namespace>,
}

impl Default for SampleData {
    fn default() -> Self {
        Self {
            title: "Example Ontology".to_string(),
            iri: "https://example.org/ontology/example".to_string(),
            version: Some("1.0.0".to_string()),
            comment: Some(
                "An example ontology for demonstrating panschema components.".to_string(),
            ),
            classes: vec![
                EntityRef::new("person", "Person"),
                EntityRef::new("organization", "Organization"),
                EntityRef::new("event", "Event"),
            ],
            slots: vec![
                EntityRef::new("name", "name"),
                EntityRef::new("member-of", "memberOf"),
                EntityRef::new("participates-in", "participatesIn"),
            ],
            individuals: vec![
                EntityRef::new("john-doe", "John Doe"),
                EntityRef::new("acme-corp", "Acme Corp"),
            ],
            namespaces: vec![
                Namespace::new("ex", "https://example.org/ontology/example#"),
                Namespace::new("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
                Namespace::new("rdfs", "http://www.w3.org/2000/01/rdf-schema#"),
                Namespace::new("owl", "http://www.w3.org/2002/07/owl#"),
                Namespace::new("xsd", "http://www.w3.org/2001/XMLSchema#"),
            ],
        }
    }
}

impl SampleData {
    /// Create sample data with minimal fields (no optional values).
    pub fn minimal() -> Self {
        Self {
            title: "Minimal Ontology".to_string(),
            iri: "https://example.org/minimal".to_string(),
            version: None,
            comment: None,
            classes: vec![],
            slots: vec![],
            individuals: vec![],
            namespaces: vec![Namespace::new("", "https://example.org/minimal#")],
        }
    }
}

/// Header component template.
#[derive(Template)]
#[template(path = "components/header.html")]
pub struct HeaderComponent<'a> {
    pub title: &'a str,
    /// Matches the field on the main `IndexTemplate`. The styleguide
    /// renders without a version cohort, so this is always `None` here.
    pub version_context: Option<&'a panschema::html_writer::VersionContext>,
    /// Matches the field on the main `IndexTemplate`. The styleguide
    /// page sits at the output root, so `"./"` is the right value.
    pub site_root_href: &'a str,
}

/// Footer component template.
#[derive(Template)]
#[template(path = "components/footer.html")]
pub struct FooterComponent;

/// Hero component template.
#[derive(Template)]
#[template(path = "components/hero.html")]
pub struct HeroComponent<'a> {
    pub title: &'a str,
    pub comment: Option<&'a str>,
}

/// Metadata card component template.
#[derive(Template)]
#[template(path = "components/metadata_card.html")]
pub struct MetadataCardComponent<'a> {
    pub iri: &'a str,
    pub version: Option<&'a str>,
    pub comment: Option<&'a str>,
}

/// Sidebar navigation component template.
#[derive(Template)]
#[template(path = "components/sidebar.html")]
pub struct SidebarComponent<'a> {
    pub active_section: &'a str,
    pub classes: &'a [EntityRef],
    pub slots: &'a [EntityRef],
    pub enums: &'a [EntityRef],
    pub types: &'a [EntityRef],
    pub individuals: &'a [EntityRef],
    pub namespaces: &'a [Namespace],
    /// Graph data JSON for visualization (None = no graph link in sidebar)
    pub graph_json: Option<&'a str>,
    /// Number of nodes in the graph (for sidebar badge)
    pub graph_node_count: usize,
    /// Number of edges in the graph (for sidebar badge)
    pub graph_edge_count: usize,
}

/// Namespace table component template.
#[derive(Template)]
#[template(path = "components/namespace_table.html")]
pub struct NamespaceTableComponent<'a> {
    pub namespaces: &'a [Namespace],
}

/// Section header component template.
#[derive(Template)]
#[template(path = "components/section_header.html")]
pub struct SectionHeaderComponent<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub count: Option<usize>,
    pub description: Option<&'a str>,
}

/// Class card component template.
#[derive(Template)]
#[template(path = "components/class_card.html")]
pub struct ClassCardComponent<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub iri: &'a str,
    pub iri_href: Option<&'a str>,
    pub description: Option<&'a str>,
    pub superclass: Option<&'a EntityRef>,
    pub subclasses: &'a [EntityRef],
    pub mixins: &'a [EntityRef],
    pub slots: &'a [panschema::html_writer::SlotInClass],
    pub mappings: &'a [panschema::html_writer::Mapping],
    pub external_superclasses: &'a [panschema::html_writer::ExternalLink],
    pub is_abstract: bool,
    pub deprecated: Option<&'a str>,
}

/// Property card component template.
#[derive(Template)]
#[template(path = "components/slot_card.html")]
pub struct SlotCardComponent<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub iri: &'a str,
    pub iri_href: Option<&'a str>,
    pub slot_type: &'a str,
    pub description: Option<&'a str>,
    pub domains: &'a [EntityRef],
    pub range: Option<&'a RangeSpec>,
    /// Members of an `any_of` union range; empty for single-range slots.
    pub any_of: &'a [RangeSpec],
    pub pattern: Option<&'a str>,
    pub characteristics: &'a [String],
    pub mappings: &'a [panschema::html_writer::Mapping],
    pub deprecated: Option<&'a str>,
}

/// Individual card component template.
#[derive(Template)]
#[template(path = "components/individual_card.html")]
pub struct IndividualCardComponent<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub iri: &'a str,
    pub description: Option<&'a str>,
    pub types: &'a [EntityRef],
    pub property_values: &'a [PropertyValueSpec],
}

/// Enum card component template.
#[derive(Template)]
#[template(path = "components/enum_card.html")]
pub struct EnumCardComponent<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub description: Option<&'a str>,
    pub permissible_values: &'a [panschema::html_writer::PermissibleValueData],
    pub deprecated: Option<&'a str>,
}

/// Type card component template.
#[derive(Template)]
#[template(path = "components/type_card.html")]
pub struct TypeCardComponent<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub uri: Option<&'a panschema::html_writer::ExternalLink>,
    pub description: Option<&'a str>,
    pub base_type: Option<&'a EntityRef>,
    pub pattern: Option<&'a str>,
    pub deprecated: Option<&'a str>,
}

/// Sample class data for styleguide previews.
pub struct SampleClass<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub iri: &'a str,
    pub iri_href: Option<&'a str>,
    pub description: Option<&'a str>,
    pub superclass: Option<&'a EntityRef>,
    pub subclasses: &'a [EntityRef],
    pub mixins: &'a [EntityRef],
    pub slots: &'a [panschema::html_writer::SlotInClass],
    pub mappings: &'a [panschema::html_writer::Mapping],
    pub external_superclasses: &'a [panschema::html_writer::ExternalLink],
    pub is_abstract: bool,
    pub deprecated: Option<&'a str>,
}

/// Sample property data for styleguide previews.
pub struct SampleSlot<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub iri: &'a str,
    pub iri_href: Option<&'a str>,
    pub slot_type: &'a str,
    pub description: Option<&'a str>,
    pub domains: &'a [EntityRef],
    pub range: Option<&'a RangeSpec>,
    /// Members of an `any_of` union range; empty for single-range slots.
    pub any_of: &'a [RangeSpec],
    pub pattern: Option<&'a str>,
    pub characteristics: &'a [String],
    pub mappings: &'a [panschema::html_writer::Mapping],
    pub deprecated: Option<&'a str>,
}

/// Sample individual data for styleguide previews.
pub struct SampleIndividual<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub iri: &'a str,
    pub description: Option<&'a str>,
    pub types: &'a [EntityRef],
    pub property_values: &'a [PropertyValueSpec],
}

/// Style guide page template.
#[derive(Template)]
#[template(path = "styleguide.html")]
pub struct StyleGuideTemplate<'a> {
    pub title: &'a str,
    pub iri: &'a str,
    pub version: Option<&'a str>,
    pub comment: Option<&'a str>,
    pub classes: &'a [EntityRef],
    pub slots: &'a [EntityRef],
    pub enums: &'a [EntityRef],
    pub types: &'a [EntityRef],
    pub individuals: &'a [EntityRef],
    pub namespaces: &'a [Namespace],
    /// Graph data JSON (None = no graph link in sidebar)
    pub graph_json: Option<&'a str>,
    /// Number of nodes in the graph (for sidebar badge)
    pub graph_node_count: usize,
    /// Number of edges in the graph (for sidebar badge)
    pub graph_edge_count: usize,
    // Sample data for component previews
    pub sample_class: SampleClass<'a>,
    pub sample_slot: SampleSlot<'a>,
    pub sample_data_slot: SampleSlot<'a>,
    pub sample_individual: SampleIndividual<'a>,
    /// Matches IndexTemplate. Always `None` for the styleguide page.
    pub version_context: Option<&'a panschema::html_writer::VersionContext>,
    /// Matches IndexTemplate. Styleguide page sits at the output root.
    pub site_root_href: &'a str,
}

/// Renders individual components for testing and preview.
pub struct ComponentRenderer;

impl ComponentRenderer {
    /// Render the header component.
    pub fn header(title: &str) -> anyhow::Result<String> {
        let template = HeaderComponent {
            title,
            version_context: None,
            site_root_href: "./",
        };
        Ok(template.render()?)
    }

    /// Render the footer component.
    pub fn footer() -> anyhow::Result<String> {
        let template = FooterComponent;
        Ok(template.render()?)
    }

    /// Render the hero component.
    pub fn hero(title: &str, comment: Option<&str>) -> anyhow::Result<String> {
        let template = HeroComponent { title, comment };
        Ok(template.render()?)
    }

    /// Render the metadata card component.
    pub fn metadata_card(
        iri: &str,
        version: Option<&str>,
        comment: Option<&str>,
    ) -> anyhow::Result<String> {
        let template = MetadataCardComponent {
            iri,
            version,
            comment,
        };
        Ok(template.render()?)
    }

    /// Render the sidebar navigation component.
    pub fn sidebar(
        active_section: &str,
        classes: &[EntityRef],
        slots: &[EntityRef],
        individuals: &[EntityRef],
        namespaces: &[Namespace],
    ) -> anyhow::Result<String> {
        let template = SidebarComponent {
            active_section,
            classes,
            slots,
            // Styleguide preview doesn't exercise enum/type nav entries.
            enums: &[],
            types: &[],
            individuals,
            namespaces,
            graph_json: None, // No graph in component preview
            graph_node_count: 0,
            graph_edge_count: 0,
        };
        Ok(template.render()?)
    }

    /// Render the namespace table component.
    pub fn namespace_table(namespaces: &[Namespace]) -> anyhow::Result<String> {
        let template = NamespaceTableComponent { namespaces };
        Ok(template.render()?)
    }

    /// Render a section header component.
    pub fn section_header(
        id: &str,
        title: &str,
        count: Option<usize>,
        description: Option<&str>,
    ) -> anyhow::Result<String> {
        let template = SectionHeaderComponent {
            id,
            title,
            count,
            description,
        };
        Ok(template.render()?)
    }

    /// Render a class card component.
    #[allow(clippy::too_many_arguments)]
    pub fn class_card(
        id: &str,
        label: &str,
        iri: &str,
        description: Option<&str>,
        superclass: Option<&EntityRef>,
        subclasses: &[EntityRef],
        mixins: &[EntityRef],
        slots: &[panschema::html_writer::SlotInClass],
    ) -> anyhow::Result<String> {
        let template = ClassCardComponent {
            id,
            label,
            iri,
            iri_href: None,
            description,
            superclass,
            subclasses,
            mixins,
            slots,
            mappings: &[],
            external_superclasses: &[],
            is_abstract: false,
            deprecated: None,
        };
        Ok(template.render()?)
    }

    /// Render a property card component.
    #[allow(clippy::too_many_arguments)]
    pub fn slot_card(
        id: &str,
        label: &str,
        iri: &str,
        slot_type: &str,
        description: Option<&str>,
        domain: Option<&EntityRef>,
        range: Option<&RangeSpec>,
        characteristics: &[String],
    ) -> anyhow::Result<String> {
        let template = SlotCardComponent {
            id,
            label,
            iri,
            iri_href: None,
            slot_type,
            description,
            domains: domain.map(std::slice::from_ref).unwrap_or(&[]),
            range,
            any_of: &[],
            pattern: None,
            characteristics,
            mappings: &[],
            deprecated: None,
        };
        Ok(template.render()?)
    }

    /// Render an individual card component.
    #[allow(clippy::too_many_arguments)]
    pub fn individual_card(
        id: &str,
        label: &str,
        iri: &str,
        description: Option<&str>,
        types: &[EntityRef],
        property_values: &[PropertyValueSpec],
    ) -> anyhow::Result<String> {
        let template = IndividualCardComponent {
            id,
            label,
            iri,
            description,
            types,
            property_values,
        };
        Ok(template.render()?)
    }

    /// Render an enum card component.
    pub fn enum_card(
        id: &str,
        label: &str,
        description: Option<&str>,
        permissible_values: &[panschema::html_writer::PermissibleValueData],
    ) -> anyhow::Result<String> {
        let template = EnumCardComponent {
            id,
            label,
            description,
            permissible_values,
            deprecated: None,
        };
        Ok(template.render()?)
    }

    /// Render a type card component.
    pub fn type_card(
        id: &str,
        label: &str,
        uri: Option<&panschema::html_writer::ExternalLink>,
        description: Option<&str>,
        base_type: Option<&EntityRef>,
        pattern: Option<&str>,
    ) -> anyhow::Result<String> {
        let template = TypeCardComponent {
            id,
            label,
            uri,
            description,
            base_type,
            pattern,
            deprecated: None,
        };
        Ok(template.render()?)
    }

    /// Render the complete style guide page.
    pub fn styleguide(data: &SampleData) -> anyhow::Result<String> {
        // Create sample data for component previews
        let superclass = EntityRef::new("thing", "Thing");
        let subclasses = vec![
            EntityRef::new("employee", "Employee"),
            EntityRef::new("customer", "Customer"),
        ];

        let class_mixins = vec![
            EntityRef::new("auditable", "Auditable"),
            EntityRef::new("publishable", "Publishable"),
        ];
        let class_slots: Vec<panschema::html_writer::SlotInClass> = vec![
            panschema::html_writer::SlotInClass {
                name: "name".to_string(),
                range: Some(panschema::html_writer::RangeRef {
                    class_ref: None,
                    datatype: "string".to_string(),
                }),
                required: true,
                multivalued: false,
                any_of: vec![],
                suppressed: false,
                description: None,
                refined_here: false,
                origin: Some("mixin Named".to_string()),
                description_tooltip: Some("Full legal name.".to_string()),
            },
            panschema::html_writer::SlotInClass {
                name: "age".to_string(),
                range: Some(panschema::html_writer::RangeRef {
                    class_ref: None,
                    datatype: "integer".to_string(),
                }),
                required: false,
                multivalued: false,
                any_of: vec![],
                suppressed: false,
                description: None,
                refined_here: true,
                origin: None,
                description_tooltip: None,
            },
        ];
        let class_mappings = vec![
            panschema::html_writer::Mapping {
                kind: "exact",
                display: "foaf:Person".to_string(),
                href: Some("http://xmlns.com/foaf/0.1/Person".to_string()),
                label: Some("Person".to_string()),
                definitions: vec!["A person.".to_string()],
            },
            panschema::html_writer::Mapping {
                kind: "related",
                display: "schema:Person".to_string(),
                href: Some("https://schema.org/Person".to_string()),
                label: None,
                definitions: Vec::new(),
            },
        ];
        let sample_class = SampleClass {
            id: "person",
            label: "Person",
            iri: "https://example.org/ontology#Person",
            iri_href: Some("https://example.org/ontology#Person"),
            description: Some("Represents a human being."),
            superclass: Some(&superclass),
            subclasses: &subclasses,
            mixins: &class_mixins,
            slots: &class_slots,
            mappings: &class_mappings,
            external_superclasses: &[],
            is_abstract: false,
            deprecated: None,
        };

        let domain = EntityRef::new("person", "Person");
        let range = RangeSpec::class(EntityRef::new("organization", "Organization"));
        let characteristics = vec!["Functional".to_string()];
        let slot_mappings: Vec<panschema::html_writer::Mapping> = vec![];

        let sample_slot = SampleSlot {
            id: "member-of",
            label: "memberOf",
            iri: "https://example.org/ontology#memberOf",
            iri_href: Some("https://example.org/ontology#memberOf"),
            slot_type: "Slot",
            description: Some("Relates a person to their organization."),
            domains: std::slice::from_ref(&domain),
            range: Some(&range),
            any_of: &[],
            pattern: None,
            characteristics: &characteristics,
            mappings: &slot_mappings,
            deprecated: None,
        };

        let domain2 = EntityRef::new("person", "Person");
        let range2 = RangeSpec::datatype("xsd:string");
        let empty_characteristics: Vec<String> = vec![];

        let sample_data_slot = SampleSlot {
            id: "name",
            label: "name",
            iri: "https://example.org/ontology#name",
            iri_href: Some("https://example.org/ontology#name"),
            slot_type: "Slot",
            description: Some("The name of an entity."),
            domains: std::slice::from_ref(&domain2),
            range: Some(&range2),
            any_of: &[],
            pattern: Some("^[A-Z][a-z]+$"),
            characteristics: &empty_characteristics,
            mappings: &slot_mappings,
            deprecated: None,
        };

        let ind_types = vec![EntityRef::new("person", "Person")];
        let ind_property_values = vec![
            PropertyValueSpec::new("name", Some(EntityRef::new("name", "name")), "John Doe"),
            PropertyValueSpec::new(
                "memberOf",
                Some(EntityRef::new("member-of", "memberOf")),
                "Acme Corp",
            ),
        ];

        let sample_individual = SampleIndividual {
            id: "john-doe",
            label: "John Doe",
            iri: "https://example.org/ontology#JohnDoe",
            description: Some("A sample individual representing a person."),
            types: &ind_types,
            property_values: &ind_property_values,
        };

        let template = StyleGuideTemplate {
            title: &data.title,
            iri: &data.iri,
            version: data.version.as_deref(),
            comment: data.comment.as_deref(),
            classes: &data.classes,
            slots: &data.slots,
            enums: &[],
            types: &[],
            individuals: &data.individuals,
            namespaces: &data.namespaces,
            graph_json: None, // No graph in styleguide
            graph_node_count: 0,
            graph_edge_count: 0,
            sample_class,
            sample_slot,
            sample_data_slot,
            sample_individual,
            version_context: None,
            site_root_href: "./",
        };
        Ok(template.render()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_renders() {
        let html = ComponentRenderer::header("Test Ontology").unwrap();
        assert!(html.contains("Test Ontology"));
        assert!(html.contains("site-header"));
    }

    #[test]
    fn footer_renders() {
        let html = ComponentRenderer::footer().unwrap();
        assert!(html.contains("panschema"));
        assert!(html.contains("site-footer"));
    }

    #[test]
    fn hero_renders_with_comment() {
        let html = ComponentRenderer::hero("My Ontology", Some("A description")).unwrap();
        assert!(html.contains("My Ontology"));
        assert!(html.contains("A description"));
    }

    #[test]
    fn hero_renders_without_comment() {
        let html = ComponentRenderer::hero("My Ontology", None).unwrap();
        assert!(html.contains("My Ontology"));
        // The paragraph element with hero-description class should not be rendered
        assert!(!html.contains("<p class=\"hero-description\">"));
    }

    #[test]
    fn metadata_card_renders_full() {
        let html = ComponentRenderer::metadata_card(
            "https://example.org/onto",
            Some("2.0.0"),
            Some("A test ontology"),
        )
        .unwrap();
        assert!(html.contains("https://example.org/onto"));
        assert!(html.contains("2.0.0"));
        assert!(html.contains("A test ontology"));
    }

    #[test]
    fn metadata_card_renders_minimal() {
        let html =
            ComponentRenderer::metadata_card("https://example.org/onto", None, None).unwrap();
        assert!(html.contains("https://example.org/onto"));
        // Version and comment sections should not appear
        assert!(!html.contains("Version"));
    }

    #[test]
    fn styleguide_renders() {
        let data = SampleData::default();
        let html = ComponentRenderer::styleguide(&data).unwrap();
        assert!(html.contains("Style Guide"));
        assert!(html.contains("Header"));
        assert!(html.contains("Footer"));
        assert!(html.contains("Hero"));
        assert!(html.contains("Metadata Card"));
    }

    // Snapshot tests using insta
    mod snapshots {
        use super::*;

        #[test]
        fn snapshot_header() {
            let html = ComponentRenderer::header("Test Ontology").unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_footer() {
            let html = ComponentRenderer::footer().unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_hero_with_comment() {
            let html =
                ComponentRenderer::hero("Test Ontology", Some("A test ontology description."))
                    .unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_hero_minimal() {
            let html = ComponentRenderer::hero("Test Ontology", None).unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_metadata_card_full() {
            let html = ComponentRenderer::metadata_card(
                "https://example.org/ontology/test",
                Some("1.0.0"),
                Some("A comprehensive test ontology."),
            )
            .unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_metadata_card_minimal() {
            let html =
                ComponentRenderer::metadata_card("https://example.org/ontology/test", None, None)
                    .unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_sidebar_with_items() {
            let classes = vec![
                EntityRef::new("person", "Person"),
                EntityRef::new("organization", "Organization"),
            ];
            let slots = vec![
                EntityRef::new("name", "name"),
                EntityRef::new("member-of", "memberOf"),
            ];
            let individuals = vec![EntityRef::new("john-doe", "John Doe")];
            let namespaces = vec![
                Namespace::new("ex", "https://example.org/ontology#"),
                Namespace::new("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
            ];
            let html =
                ComponentRenderer::sidebar("metadata", &classes, &slots, &individuals, &namespaces)
                    .unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_sidebar_empty() {
            let namespaces = vec![Namespace::new("ex", "https://example.org/ontology#")];
            let html = ComponentRenderer::sidebar("metadata", &[], &[], &[], &namespaces).unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_namespace_table() {
            let namespaces = vec![
                Namespace::new("ex", "https://example.org/ontology#"),
                Namespace::new("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
                Namespace::new("rdfs", "http://www.w3.org/2000/01/rdf-schema#"),
            ];
            let html = ComponentRenderer::namespace_table(&namespaces).unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_section_header_with_count() {
            let html = ComponentRenderer::section_header(
                "classes",
                "Classes",
                Some(5),
                Some("All classes defined in this ontology."),
            )
            .unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_section_header_minimal() {
            let html =
                ComponentRenderer::section_header("overview", "Overview", None, None).unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_class_card_full() {
            let superclass = EntityRef::new("thing", "Thing");
            let subclasses = vec![
                EntityRef::new("employee", "Employee"),
                EntityRef::new("customer", "Customer"),
            ];
            let html = ComponentRenderer::class_card(
                "person",
                "Person",
                "https://example.org/ontology#Person",
                Some("Represents a human being."),
                Some(&superclass),
                &subclasses,
                &[],
                &[],
            )
            .unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_class_card_minimal() {
            let html = ComponentRenderer::class_card(
                "thing",
                "Thing",
                "https://example.org/ontology#Thing",
                None,
                None,
                &[],
                &[],
                &[],
            )
            .unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_class_card_abstract_variant() {
            // Bypass ComponentRenderer::class_card so the test can flip
            // `is_abstract` without expanding the helper signature for
            // a single styleguide variant.
            let template = ClassCardComponent {
                id: "thing",
                label: "Thing",
                iri: "https://example.org/ontology#Thing",
                iri_href: None,
                description: Some("Foundation class — not meant to be instantiated."),
                superclass: None,
                subclasses: &[],
                mixins: &[],
                slots: &[],
                mappings: &[],
                external_superclasses: &[],
                is_abstract: true,
                deprecated: None,
            };
            let html = template.render().unwrap();
            assert!(
                html.contains(r#"<span class="abstract-badge""#),
                "abstract badge should be present when is_abstract = true"
            );
            insta::assert_snapshot!(html);
        }

        #[test]
        fn class_card_renders_deprecated_badge_and_note() {
            // A class marked `deprecated:` renders a "Deprecated" badge in
            // the heading and the deprecation note on the card; an
            // undeprecated class renders neither.
            let deprecated = ClassCardComponent {
                id: "legacy",
                label: "LegacyPerson",
                iri: "https://example.org/ontology#LegacyPerson",
                iri_href: None,
                description: None,
                superclass: None,
                subclasses: &[],
                mixins: &[],
                slots: &[],
                mappings: &[],
                external_superclasses: &[],
                is_abstract: false,
                deprecated: Some("use Person instead"),
            };
            let html = deprecated.render().unwrap();
            assert!(
                html.contains(r#"<span class="deprecated-badge""#),
                "deprecated badge should be present; got:\n{html}"
            );
            assert!(
                html.contains("use Person instead"),
                "deprecation note text should render; got:\n{html}"
            );

            let current = ClassCardComponent {
                deprecated: None,
                ..deprecated
            };
            let html = current.render().unwrap();
            // The `.deprecated-badge` CSS rule is always in the card's
            // <style> block; the badge itself is the `<span>` markup.
            assert!(
                !html.contains(r#"<span class="deprecated-badge""#),
                "undeprecated class must render no badge; got:\n{html}"
            );
            assert!(
                !html.contains(r#"<div class="deprecated-note""#),
                "undeprecated class must render no note; got:\n{html}"
            );
        }

        #[test]
        fn class_card_renders_mixes_in_section_with_anchor_links() {
            let mixins = vec![
                EntityRef::new("auditable", "Auditable"),
                EntityRef::new("publishable", "Publishable"),
            ];
            let html = ComponentRenderer::class_card(
                "document",
                "Document",
                "https://example.org/ontology#Document",
                None,
                None,
                &[],
                &mixins,
                &[],
            )
            .unwrap();
            assert!(
                html.contains("Mixes in"),
                "missing 'Mixes in' label; got:\n{html}"
            );
            assert!(
                html.contains(r##"href="#class-auditable""##),
                "missing anchor link to first mixin; got:\n{html}"
            );
            assert!(
                html.contains(r##"href="#class-publishable""##),
                "missing anchor link to second mixin; got:\n{html}"
            );
        }

        #[test]
        fn class_card_omits_mixes_in_section_when_no_mixins() {
            let html = ComponentRenderer::class_card(
                "leaf",
                "Leaf",
                "https://example.org/ontology#Leaf",
                None,
                None,
                &[],
                &[],
                &[],
            )
            .unwrap();
            assert!(
                !html.contains("Mixes in"),
                "unexpected 'Mixes in' label on class without mixins; got:\n{html}"
            );
        }

        #[test]
        fn snapshot_slot_card_object_property() {
            let domain = EntityRef::new("person", "Person");
            let range = RangeSpec::class(EntityRef::new("organization", "Organization"));
            let html = ComponentRenderer::slot_card(
                "member-of",
                "memberOf",
                "https://example.org/ontology#memberOf",
                "Slot",
                Some("Relates a person to their organization."),
                Some(&domain),
                Some(&range),
                &["Functional".to_string()],
            )
            .unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_slot_card_data_property() {
            let domain = EntityRef::new("person", "Person");
            let range = RangeSpec::datatype("xsd:string");
            let html = ComponentRenderer::slot_card(
                "name",
                "name",
                "https://example.org/ontology#name",
                "Slot",
                Some("The name of a person."),
                Some(&domain),
                Some(&range),
                &[],
            )
            .unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_slot_card_minimal() {
            let html = ComponentRenderer::slot_card(
                "relates-to",
                "relatesTo",
                "https://example.org/ontology#relatesTo",
                "Slot",
                None,
                None,
                None,
                &[],
            )
            .unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_individual_card_full() {
            let types = vec![EntityRef::new("person", "Person")];
            let property_values = vec![
                PropertyValueSpec::new("name", Some(EntityRef::new("name", "name")), "John Doe"),
                PropertyValueSpec::new(
                    "memberOf",
                    Some(EntityRef::new("member-of", "memberOf")),
                    "Acme Corp",
                ),
            ];
            let html = ComponentRenderer::individual_card(
                "john-doe",
                "John Doe",
                "https://example.org/ontology#JohnDoe",
                Some("A sample individual representing a person."),
                &types,
                &property_values,
            )
            .unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_individual_card_minimal() {
            let html = ComponentRenderer::individual_card(
                "thing-1",
                "Thing 1",
                "https://example.org/ontology#Thing1",
                None,
                &[],
                &[],
            )
            .unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_enum_card_full() {
            use panschema::html_writer::{ExternalLink, PermissibleValueData};
            let permissible_values = vec![
                PermissibleValueData {
                    text: "open".to_string(),
                    description: Some("The item is open for changes.".to_string()),
                    meaning: Some(ExternalLink {
                        display: "ex:OpenStatus".to_string(),
                        href: Some("https://example.org/ontology#OpenStatus".to_string()),
                        label: Some("Open Status".to_string()),
                        definitions: vec![],
                    }),
                },
                PermissibleValueData {
                    text: "closed".to_string(),
                    description: None,
                    meaning: None,
                },
            ];
            let html = ComponentRenderer::enum_card(
                "status",
                "Status",
                Some("The lifecycle status of an item."),
                &permissible_values,
            )
            .unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_type_card_full() {
            use panschema::html_writer::ExternalLink;
            let uri = ExternalLink {
                display: "xsd:string".to_string(),
                href: Some("http://www.w3.org/2001/XMLSchema#string".to_string()),
                label: None,
                definitions: vec![],
            };
            let base = EntityRef::new("string", "string");
            let html = ComponentRenderer::type_card(
                "phone-number",
                "PhoneNumber",
                Some(&uri),
                Some("A phone number in E.164 form."),
                Some(&base),
                Some(r"^\+[1-9]\d{1,14}$"),
            )
            .unwrap();
            insta::assert_snapshot!(html);
        }
    }
}
