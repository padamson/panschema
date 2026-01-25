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

// These components are primarily used for testing and the styleguide feature.
// They will be used more extensively as we build out the documentation layout.
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
    pub properties: Vec<EntityRef>,
    pub individuals: Vec<EntityRef>,
    pub namespaces: Vec<Namespace>,
}

impl Default for SampleData {
    fn default() -> Self {
        Self {
            title: "Example Ontology".to_string(),
            iri: "https://example.org/ontology/example".to_string(),
            version: Some("1.0.0".to_string()),
            comment: Some("An example ontology for demonstrating rontodoc components.".to_string()),
            classes: vec![
                EntityRef::new("person", "Person"),
                EntityRef::new("organization", "Organization"),
                EntityRef::new("event", "Event"),
            ],
            properties: vec![
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
            properties: vec![],
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
    pub properties: &'a [EntityRef],
    pub individuals: &'a [EntityRef],
    pub namespaces: &'a [Namespace],
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
    pub description: Option<&'a str>,
    pub superclass: Option<&'a EntityRef>,
    pub subclasses: &'a [EntityRef],
    pub properties: &'a [EntityRef],
}

/// Property card component template.
#[derive(Template)]
#[template(path = "components/property_card.html")]
pub struct PropertyCardComponent<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub iri: &'a str,
    pub property_type: &'a str,
    pub description: Option<&'a str>,
    pub domain: Option<&'a EntityRef>,
    pub range: Option<&'a RangeSpec>,
    pub characteristics: &'a [String],
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

/// Sample class data for styleguide previews.
pub struct SampleClass<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub iri: &'a str,
    pub description: Option<&'a str>,
    pub superclass: Option<&'a EntityRef>,
    pub subclasses: &'a [EntityRef],
    pub properties: &'a [EntityRef],
}

/// Sample property data for styleguide previews.
pub struct SampleProperty<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub iri: &'a str,
    pub property_type: &'a str,
    pub description: Option<&'a str>,
    pub domain: Option<&'a EntityRef>,
    pub range: Option<&'a RangeSpec>,
    pub characteristics: &'a [String],
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
    pub properties: &'a [EntityRef],
    pub individuals: &'a [EntityRef],
    pub namespaces: &'a [Namespace],
    // Sample data for component previews
    pub sample_class: SampleClass<'a>,
    pub sample_property: SampleProperty<'a>,
    pub sample_data_property: SampleProperty<'a>,
    pub sample_individual: SampleIndividual<'a>,
}

/// Renders individual components for testing and preview.
pub struct ComponentRenderer;

impl ComponentRenderer {
    /// Render the header component.
    pub fn header(title: &str) -> anyhow::Result<String> {
        let template = HeaderComponent { title };
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
        properties: &[EntityRef],
        individuals: &[EntityRef],
        namespaces: &[Namespace],
    ) -> anyhow::Result<String> {
        let template = SidebarComponent {
            active_section,
            classes,
            properties,
            individuals,
            namespaces,
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
        properties: &[EntityRef],
    ) -> anyhow::Result<String> {
        let template = ClassCardComponent {
            id,
            label,
            iri,
            description,
            superclass,
            subclasses,
            properties,
        };
        Ok(template.render()?)
    }

    /// Render a property card component.
    #[allow(clippy::too_many_arguments)]
    pub fn property_card(
        id: &str,
        label: &str,
        iri: &str,
        property_type: &str,
        description: Option<&str>,
        domain: Option<&EntityRef>,
        range: Option<&RangeSpec>,
        characteristics: &[String],
    ) -> anyhow::Result<String> {
        let template = PropertyCardComponent {
            id,
            label,
            iri,
            property_type,
            description,
            domain,
            range,
            characteristics,
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

    /// Render the complete style guide page.
    pub fn styleguide(data: &SampleData) -> anyhow::Result<String> {
        // Create sample data for component previews
        let superclass = EntityRef::new("thing", "Thing");
        let subclasses = vec![
            EntityRef::new("employee", "Employee"),
            EntityRef::new("customer", "Customer"),
        ];
        let class_properties = vec![EntityRef::new("name", "name"), EntityRef::new("age", "age")];

        let sample_class = SampleClass {
            id: "person",
            label: "Person",
            iri: "https://example.org/ontology#Person",
            description: Some("Represents a human being."),
            superclass: Some(&superclass),
            subclasses: &subclasses,
            properties: &class_properties,
        };

        let domain = EntityRef::new("person", "Person");
        let range = RangeSpec::class(EntityRef::new("organization", "Organization"));
        let characteristics = vec!["Functional".to_string()];

        let sample_property = SampleProperty {
            id: "member-of",
            label: "memberOf",
            iri: "https://example.org/ontology#memberOf",
            property_type: "Object Property",
            description: Some("Relates a person to their organization."),
            domain: Some(&domain),
            range: Some(&range),
            characteristics: &characteristics,
        };

        let domain2 = EntityRef::new("person", "Person");
        let range2 = RangeSpec::datatype("xsd:string");
        let empty_characteristics: Vec<String> = vec![];

        let sample_data_property = SampleProperty {
            id: "name",
            label: "name",
            iri: "https://example.org/ontology#name",
            property_type: "Data Property",
            description: Some("The name of an entity."),
            domain: Some(&domain2),
            range: Some(&range2),
            characteristics: &empty_characteristics,
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
            properties: &data.properties,
            individuals: &data.individuals,
            namespaces: &data.namespaces,
            sample_class,
            sample_property,
            sample_data_property,
            sample_individual,
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
        assert!(html.contains("rontodoc"));
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
            let properties = vec![
                EntityRef::new("name", "name"),
                EntityRef::new("member-of", "memberOf"),
            ];
            let individuals = vec![EntityRef::new("john-doe", "John Doe")];
            let namespaces = vec![
                Namespace::new("ex", "https://example.org/ontology#"),
                Namespace::new("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
            ];
            let html = ComponentRenderer::sidebar(
                "metadata",
                &classes,
                &properties,
                &individuals,
                &namespaces,
            )
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
            let properties = vec![EntityRef::new("name", "name"), EntityRef::new("age", "age")];
            let html = ComponentRenderer::class_card(
                "person",
                "Person",
                "https://example.org/ontology#Person",
                Some("Represents a human being."),
                Some(&superclass),
                &subclasses,
                &properties,
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
            )
            .unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_property_card_object_property() {
            let domain = EntityRef::new("person", "Person");
            let range = RangeSpec::class(EntityRef::new("organization", "Organization"));
            let html = ComponentRenderer::property_card(
                "member-of",
                "memberOf",
                "https://example.org/ontology#memberOf",
                "Object Property",
                Some("Relates a person to their organization."),
                Some(&domain),
                Some(&range),
                &["Functional".to_string()],
            )
            .unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_property_card_data_property() {
            let domain = EntityRef::new("person", "Person");
            let range = RangeSpec::datatype("xsd:string");
            let html = ComponentRenderer::property_card(
                "name",
                "name",
                "https://example.org/ontology#name",
                "Data Property",
                Some("The name of a person."),
                Some(&domain),
                Some(&range),
                &[],
            )
            .unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_property_card_minimal() {
            let html = ComponentRenderer::property_card(
                "relates-to",
                "relatesTo",
                "https://example.org/ontology#relatesTo",
                "Object Property",
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
    }
}
