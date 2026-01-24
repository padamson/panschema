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

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    title: &'a str,
    iri: &'a str,
    version: Option<&'a str>,
    comment: Option<&'a str>,
    active_section: &'a str,
    classes: &'a [EntityRef],
    properties: &'a [EntityRef],
    namespaces: &'a [Namespace],
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

    // Empty for now - will be populated when parser extracts classes/properties
    let classes: Vec<EntityRef> = vec![];
    let properties: Vec<EntityRef> = vec![];

    let template = IndexTemplate {
        title: metadata.title(),
        iri: &metadata.iri,
        version: metadata.version.as_deref(),
        comment: metadata.comment.as_deref(),
        active_section: "overview",
        classes: &classes,
        properties: &properties,
        namespaces: &namespaces,
    };

    let html = template.render()?;
    let output_path = output_dir.join("index.html");
    fs::write(&output_path, html)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::OntologyMetadata;

    #[test]
    fn renders_index_html() {
        let metadata = OntologyMetadata {
            iri: "http://example.org/test".to_string(),
            label: Some("Test Ontology".to_string()),
            comment: Some("A test ontology.".to_string()),
            version: Some("1.0.0".to_string()),
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
}
