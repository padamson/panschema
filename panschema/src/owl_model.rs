//! OWL ontology model types
//!
//! These types represent the OWL ontology structure as parsed from Turtle files.
//! They are internal to the OwlReader implementation.

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
    /// Named individuals defined in the ontology
    pub individuals: Vec<OntologyIndividual>,
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

/// A property value assertion on a named individual
#[derive(Debug, Clone)]
pub struct PropertyValue {
    /// A short identifier for the property
    pub property_id: String,
    /// The property label (rdfs:label), if known
    pub property_label: Option<String>,
    /// The value as a string
    pub value: String,
}

/// A named individual (owl:NamedIndividual) extracted from an ontology
#[derive(Debug, Clone)]
pub struct OntologyIndividual {
    /// The individual IRI
    pub iri: String,
    /// A short identifier derived from the IRI
    pub id: String,
    /// The individual label (rdfs:label)
    pub label: Option<String>,
    /// The individual description (rdfs:comment)
    pub comment: Option<String>,
    /// IRIs of the types (rdf:type pointing to a class, excluding owl:NamedIndividual)
    pub type_iris: Vec<String>,
    /// Property value assertions on this individual
    pub property_values: Vec<PropertyValue>,
}

impl OntologyIndividual {
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
            individuals: vec![],
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
            individuals: vec![],
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
    fn individual_display_label_uses_label_when_present() {
        let ind = OntologyIndividual {
            iri: "http://example.org/onto#fido".to_string(),
            id: "fido".to_string(),
            label: Some("Fido".to_string()),
            comment: None,
            type_iris: vec![],
            property_values: vec![],
        };
        assert_eq!(ind.display_label(), "Fido");
    }

    #[test]
    fn individual_display_label_falls_back_to_id() {
        let ind = OntologyIndividual {
            iri: "http://example.org/onto#fido".to_string(),
            id: "fido".to_string(),
            label: None,
            comment: None,
            type_iris: vec![],
            property_values: vec![],
        };
        assert_eq!(ind.display_label(), "fido");
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
