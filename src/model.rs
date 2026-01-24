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
    /// Classes defined in the ontology
    pub classes: Vec<OntologyClass>,
}

/// A class (owl:Class) extracted from an ontology
#[derive(Debug, Clone)]
pub struct OntologyClass {
    /// The class IRI
    pub iri: String,
    /// A short identifier derived from the IRI (e.g., "Animal" from "http://example.org#Animal")
    pub id: String,
    /// The class label (rdfs:label)
    pub label: Option<String>,
    /// The class description (rdfs:comment)
    pub comment: Option<String>,
    /// IRI of the superclass (rdfs:subClassOf)
    pub superclass_iri: Option<String>,
}

impl OntologyClass {
    /// Returns the display label (label if available, otherwise id)
    pub fn display_label(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.id)
    }
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
            classes: vec![],
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
            classes: vec![],
        };
        assert_eq!(meta.title(), "http://example.org/onto");
    }

    #[test]
    fn class_display_label_uses_label_when_present() {
        let class = OntologyClass {
            iri: "http://example.org/onto#Animal".to_string(),
            id: "Animal".to_string(),
            label: Some("Animal".to_string()),
            comment: None,
            superclass_iri: None,
        };
        assert_eq!(class.display_label(), "Animal");
    }

    #[test]
    fn class_display_label_falls_back_to_id() {
        let class = OntologyClass {
            iri: "http://example.org/onto#Animal".to_string(),
            id: "Animal".to_string(),
            label: None,
            comment: None,
            superclass_iri: None,
        };
        assert_eq!(class.display_label(), "Animal");
    }
}
