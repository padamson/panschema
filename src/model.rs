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
    /// Properties defined in the ontology
    pub properties: Vec<OntologyProperty>,
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

/// The type of an OWL property
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyType {
    /// owl:ObjectProperty - relates individuals to individuals
    ObjectProperty,
    /// owl:DatatypeProperty - relates individuals to data values
    DatatypeProperty,
}

impl PropertyType {
    /// Returns a display string for the property type
    pub fn display(&self) -> &str {
        match self {
            PropertyType::ObjectProperty => "Object Property",
            PropertyType::DatatypeProperty => "Datatype Property",
        }
    }
}

/// A property (owl:ObjectProperty or owl:DatatypeProperty) extracted from an ontology
#[derive(Debug, Clone)]
pub struct OntologyProperty {
    /// The property IRI
    pub iri: String,
    /// A short identifier derived from the IRI
    pub id: String,
    /// The property label (rdfs:label)
    pub label: Option<String>,
    /// The property description (rdfs:comment)
    pub comment: Option<String>,
    /// The property type (object or datatype)
    pub property_type: PropertyType,
    /// IRI of the domain class (rdfs:domain)
    pub domain_iri: Option<String>,
    /// IRI of the range (rdfs:range) - class IRI for object properties, datatype IRI for datatype properties
    pub range_iri: Option<String>,
    /// IRI of the inverse property (owl:inverseOf)
    pub inverse_of_iri: Option<String>,
}

impl OntologyProperty {
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
            properties: vec![],
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
            properties: vec![],
        };
        assert_eq!(meta.title(), "http://example.org/onto");
    }

    #[test]
    fn property_display_label_uses_label_when_present() {
        let prop = OntologyProperty {
            iri: "http://example.org/onto#hasOwner".to_string(),
            id: "hasOwner".to_string(),
            label: Some("has owner".to_string()),
            comment: None,
            property_type: PropertyType::ObjectProperty,
            domain_iri: None,
            range_iri: None,
            inverse_of_iri: None,
        };
        assert_eq!(prop.display_label(), "has owner");
    }

    #[test]
    fn property_display_label_falls_back_to_id() {
        let prop = OntologyProperty {
            iri: "http://example.org/onto#hasOwner".to_string(),
            id: "hasOwner".to_string(),
            label: None,
            comment: None,
            property_type: PropertyType::ObjectProperty,
            domain_iri: None,
            range_iri: None,
            inverse_of_iri: None,
        };
        assert_eq!(prop.display_label(), "hasOwner");
    }

    #[test]
    fn property_type_display() {
        assert_eq!(PropertyType::ObjectProperty.display(), "Object Property");
        assert_eq!(
            PropertyType::DatatypeProperty.display(),
            "Datatype Property"
        );
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
