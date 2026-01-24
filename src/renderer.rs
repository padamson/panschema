use std::fs;
use std::path::Path;

use askama::Template;

use crate::model::OntologyMetadata;

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    title: &'a str,
    iri: &'a str,
    version: Option<&'a str>,
    comment: Option<&'a str>,
}

/// Render ontology documentation to the output directory
pub fn render(metadata: &OntologyMetadata, output_dir: &Path) -> anyhow::Result<()> {
    // Create output directory if it doesn't exist
    fs::create_dir_all(output_dir)?;

    let template = IndexTemplate {
        title: metadata.title(),
        iri: &metadata.iri,
        version: metadata.version.as_deref(),
        comment: metadata.comment.as_deref(),
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
