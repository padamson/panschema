use std::fs;
use std::path::Path;

use askama::Template;

use crate::model::OntologyMetadata;

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
    namespaces: &'a [Namespace],
    /// Empty slice for class cards that don't have properties yet
    empty_properties: &'a [EntityRef],
}

/// Render ontology documentation to the output directory
pub fn render(metadata: &OntologyMetadata, output_dir: &Path) -> anyhow::Result<()> {
    // Create output directory if it doesn't exist
    fs::create_dir_all(output_dir)?;

    // Default namespaces (will be extracted from ontology in future)
    let namespaces = vec![
        Namespace {
            prefix: "".to_string(),
            iri: metadata.iri.clone(),
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

    // Convert extracted classes to EntityRef for sidebar navigation
    let classes: Vec<EntityRef> = metadata
        .classes
        .iter()
        .map(|c| EntityRef {
            id: c.id.clone(),
            label: c.display_label().to_string(),
        })
        .collect();

    // Build full class data with computed subclasses
    let class_data: Vec<ClassData> = metadata
        .classes
        .iter()
        .map(|c| {
            // Find superclass EntityRef if this class has a superclass_iri
            let superclass = c.superclass_iri.as_ref().and_then(|super_iri| {
                metadata
                    .classes
                    .iter()
                    .find(|sc| &sc.iri == super_iri)
                    .map(|sc| EntityRef {
                        id: sc.id.clone(),
                        label: sc.display_label().to_string(),
                    })
            });

            // Find all classes that have this class as their superclass
            let subclasses: Vec<EntityRef> = metadata
                .classes
                .iter()
                .filter(|sub| sub.superclass_iri.as_ref() == Some(&c.iri))
                .map(|sub| EntityRef {
                    id: sub.id.clone(),
                    label: sub.display_label().to_string(),
                })
                .collect();

            ClassData {
                id: c.id.clone(),
                label: c.display_label().to_string(),
                iri: c.iri.clone(),
                description: c.comment.clone(),
                superclass,
                subclasses,
            }
        })
        .collect();

    // Empty for now - will be populated when parser extracts properties
    let properties: Vec<EntityRef> = vec![];

    let template = IndexTemplate {
        title: metadata.title(),
        iri: &metadata.iri,
        version: metadata.version.as_deref(),
        comment: metadata.comment.as_deref(),
        active_section: "overview",
        classes: &classes,
        class_data: &class_data,
        properties: &properties,
        namespaces: &namespaces,
        empty_properties: &[],
    };

    let html = template.render()?;
    let output_path = output_dir.join("index.html");
    fs::write(&output_path, html)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{OntologyClass, OntologyMetadata};

    #[test]
    fn renders_index_html() {
        let metadata = OntologyMetadata {
            iri: "http://example.org/test".to_string(),
            label: Some("Test Ontology".to_string()),
            comment: Some("A test ontology.".to_string()),
            version: Some("1.0.0".to_string()),
            classes: vec![],
        };

        let temp_dir = std::env::temp_dir().join("rontodoc_test");
        render(&metadata, &temp_dir).expect("Render failed");

        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read output");
        assert!(html.contains("Test Ontology"));
        assert!(html.contains("http://example.org/test"));
        assert!(html.contains("1.0.0"));
        assert!(html.contains("A test ontology."));

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn renders_class_cards_with_labels_and_descriptions() {
        let metadata = OntologyMetadata {
            iri: "http://example.org/test".to_string(),
            label: Some("Test Ontology".to_string()),
            comment: None,
            version: None,
            classes: vec![OntologyClass {
                iri: "http://example.org/test#Animal".to_string(),
                id: "Animal".to_string(),
                label: Some("Animal".to_string()),
                comment: Some("A living creature.".to_string()),
                superclass_iri: None,
            }],
        };

        let temp_dir = std::env::temp_dir().join("rontodoc_class_test");
        render(&metadata, &temp_dir).expect("Render failed");

        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read output");

        // Verify class card is rendered
        assert!(html.contains("class-card"), "Should contain class card");
        assert!(
            html.contains("id=\"class-Animal\""),
            "Should have class anchor"
        );
        assert!(html.contains("Animal"), "Should contain class label");
        assert!(
            html.contains("http://example.org/test#Animal"),
            "Should contain class IRI"
        );
        assert!(
            html.contains("A living creature."),
            "Should contain class description"
        );

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn renders_class_hierarchy_relationships() {
        let metadata = OntologyMetadata {
            iri: "http://example.org/test".to_string(),
            label: Some("Test Ontology".to_string()),
            comment: None,
            version: None,
            classes: vec![
                OntologyClass {
                    iri: "http://example.org/test#Animal".to_string(),
                    id: "Animal".to_string(),
                    label: Some("Animal".to_string()),
                    comment: None,
                    superclass_iri: None,
                },
                OntologyClass {
                    iri: "http://example.org/test#Mammal".to_string(),
                    id: "Mammal".to_string(),
                    label: Some("Mammal".to_string()),
                    comment: None,
                    superclass_iri: Some("http://example.org/test#Animal".to_string()),
                },
                OntologyClass {
                    iri: "http://example.org/test#Dog".to_string(),
                    id: "Dog".to_string(),
                    label: Some("Dog".to_string()),
                    comment: None,
                    superclass_iri: Some("http://example.org/test#Mammal".to_string()),
                },
            ],
        };

        let temp_dir = std::env::temp_dir().join("rontodoc_hierarchy_test");
        render(&metadata, &temp_dir).expect("Render failed");

        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read output");

        // Verify superclass is shown (Dog -> Mammal)
        assert!(
            html.contains("Subclass of"),
            "Should show 'Subclass of' label"
        );
        assert!(
            html.contains("href=\"#class-Mammal\""),
            "Dog should link to Mammal as superclass"
        );

        // Verify subclasses are shown (Animal has Mammal as subclass)
        assert!(
            html.contains("Superclass of"),
            "Should show 'Superclass of' label"
        );

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }
}
