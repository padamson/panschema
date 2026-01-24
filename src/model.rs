/// Metadata extracted from an ontology
#[derive(Debug, Clone)]
pub struct OntologyMetadata {
    /// The ontology IRI (e.g., "http://example.org/ontology")
    pub iri: String,
    /// The ontology label (rdfs:label)
    pub label: Option<String>,
    /// The ontology description (rdfs:comment)
    pub comment: Option<String>,
    /// The ontology version (owl:versionInfo)
    pub version: Option<String>,
}

impl OntologyMetadata {
    /// Returns the display title (label if available, otherwise IRI)
    pub fn title(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.iri)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_uses_label_when_present() {
        let meta = OntologyMetadata {
            iri: "http://example.org/onto".to_string(),
            label: Some("My Ontology".to_string()),
            comment: None,
            version: None,
        };
        assert_eq!(meta.title(), "My Ontology");
    }

    #[test]
    fn title_falls_back_to_iri() {
        let meta = OntologyMetadata {
            iri: "http://example.org/onto".to_string(),
            label: None,
            comment: None,
            version: None,
        };
        assert_eq!(meta.title(), "http://example.org/onto");
    }
}
