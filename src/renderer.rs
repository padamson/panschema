use std::fs;
use std::path::Path;

use askama::Template;

use crate::model::OntologyMetadata;
use crate::parser::extract_id_from_iri;

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

    // Convert extracted properties to EntityRef for sidebar navigation
    let properties: Vec<EntityRef> = metadata
        .properties
        .iter()
        .map(|p| EntityRef {
            id: p.id.clone(),
            label: p.display_label().to_string(),
        })
        .collect();

    // Build full property data for rendering property cards
    let property_data: Vec<PropertyData> = metadata
        .properties
        .iter()
        .map(|p| {
            // Resolve domain to a class EntityRef
            let domain = p.domain_iri.as_ref().and_then(|domain_iri| {
                metadata
                    .classes
                    .iter()
                    .find(|c| &c.iri == domain_iri)
                    .map(|c| EntityRef {
                        id: c.id.clone(),
                        label: c.display_label().to_string(),
                    })
            });

            // Resolve range - check if it's a class or a datatype
            let range = p.range_iri.as_ref().map(|range_iri| {
                let class_ref = metadata
                    .classes
                    .iter()
                    .find(|c| &c.iri == range_iri)
                    .map(|c| EntityRef {
                        id: c.id.clone(),
                        label: c.display_label().to_string(),
                    });

                let datatype = extract_id_from_iri(range_iri);

                RangeRef {
                    class_ref,
                    datatype,
                }
            });

            // Build characteristics list
            let mut characteristics = Vec::new();
            if let Some(inverse_iri) = &p.inverse_of_iri {
                let inverse_label = metadata
                    .properties
                    .iter()
                    .find(|ip| &ip.iri == inverse_iri)
                    .map(|ip| ip.display_label().to_string())
                    .unwrap_or_else(|| extract_id_from_iri(inverse_iri));
                characteristics.push(format!("Inverse of: {}", inverse_label));
            }

            PropertyData {
                id: p.id.clone(),
                label: p.display_label().to_string(),
                iri: p.iri.clone(),
                property_type: p.property_type.display().to_string(),
                description: p.comment.clone(),
                domain,
                range,
                characteristics,
            }
        })
        .collect();

    // Convert extracted individuals to EntityRef for sidebar navigation
    let individuals: Vec<EntityRef> = metadata
        .individuals
        .iter()
        .map(|i| EntityRef {
            id: i.id.clone(),
            label: i.display_label().to_string(),
        })
        .collect();

    // Build full individual data for rendering individual cards
    let individual_data: Vec<IndividualData> = metadata
        .individuals
        .iter()
        .map(|i| {
            // Resolve type IRIs to EntityRefs (link to class cards)
            let types: Vec<EntityRef> = i
                .type_iris
                .iter()
                .filter_map(|type_iri| {
                    metadata
                        .classes
                        .iter()
                        .find(|c| &c.iri == type_iri)
                        .map(|c| EntityRef {
                            id: c.id.clone(),
                            label: c.display_label().to_string(),
                        })
                })
                .collect();

            // Resolve property values
            let property_values: Vec<PropertyValueData> = i
                .property_values
                .iter()
                .map(|pv| {
                    let property_ref = metadata
                        .properties
                        .iter()
                        .find(|p| p.iri == pv.property_iri)
                        .map(|p| EntityRef {
                            id: p.id.clone(),
                            label: p.display_label().to_string(),
                        });
                    let property_label = pv
                        .property_label
                        .clone()
                        .or_else(|| property_ref.as_ref().map(|r| r.label.clone()))
                        .unwrap_or_else(|| pv.property_id.clone());

                    PropertyValueData {
                        property_label,
                        property_ref,
                        value: pv.value.clone(),
                    }
                })
                .collect();

            IndividualData {
                id: i.id.clone(),
                label: i.display_label().to_string(),
                iri: i.iri.clone(),
                description: i.comment.clone(),
                types,
                property_values,
            }
        })
        .collect();

    let template = IndexTemplate {
        title: metadata.title(),
        iri: &metadata.iri,
        version: metadata.version.as_deref(),
        comment: metadata.comment.as_deref(),
        active_section: "metadata",
        classes: &classes,
        class_data: &class_data,
        properties: &properties,
        property_data: &property_data,
        individuals: &individuals,
        individual_data: &individual_data,
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
            properties: vec![],
            individuals: vec![],
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
            properties: vec![],
            individuals: vec![],
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
            properties: vec![],
            individuals: vec![],
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

    #[test]
    fn renders_property_cards_with_domain_and_range() {
        use crate::model::{OntologyProperty, PropertyType};

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
                    iri: "http://example.org/test#Person".to_string(),
                    id: "Person".to_string(),
                    label: Some("Person".to_string()),
                    comment: None,
                    superclass_iri: None,
                },
            ],
            properties: vec![OntologyProperty {
                iri: "http://example.org/test#hasOwner".to_string(),
                id: "hasOwner".to_string(),
                label: Some("has owner".to_string()),
                comment: Some("Relates an animal to its owner.".to_string()),
                property_type: PropertyType::ObjectProperty,
                domain_iri: Some("http://example.org/test#Animal".to_string()),
                range_iri: Some("http://example.org/test#Person".to_string()),
                inverse_of_iri: None,
            }],
            individuals: vec![],
        };

        let temp_dir = std::env::temp_dir().join("rontodoc_prop_test");
        render(&metadata, &temp_dir).expect("Render failed");

        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read output");

        // Verify property card is rendered
        assert!(
            html.contains("property-card"),
            "Should contain property card"
        );
        assert!(
            html.contains("id=\"prop-hasOwner\""),
            "Should have property anchor"
        );
        assert!(
            html.contains("Object Property"),
            "Should show property type badge"
        );
        assert!(html.contains("has owner"), "Should contain property label");
        assert!(
            html.contains("Relates an animal to its owner."),
            "Should contain property description"
        );

        // Verify domain links to class
        assert!(html.contains("Domain"), "Should show Domain label");
        assert!(
            html.contains("href=\"#class-Animal\""),
            "Domain should link to Animal"
        );

        // Verify range links to class
        assert!(html.contains("Range"), "Should show Range label");
        assert!(
            html.contains("href=\"#class-Person\""),
            "Range should link to Person"
        );

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn renders_datatype_property_with_xsd_range() {
        use crate::model::{OntologyProperty, PropertyType};

        let metadata = OntologyMetadata {
            iri: "http://example.org/test".to_string(),
            label: Some("Test Ontology".to_string()),
            comment: None,
            version: None,
            classes: vec![],
            properties: vec![OntologyProperty {
                iri: "http://example.org/test#hasAge".to_string(),
                id: "hasAge".to_string(),
                label: Some("has age".to_string()),
                comment: None,
                property_type: PropertyType::DatatypeProperty,
                domain_iri: None,
                range_iri: Some("http://www.w3.org/2001/XMLSchema#integer".to_string()),
                inverse_of_iri: None,
            }],
            individuals: vec![],
        };

        let temp_dir = std::env::temp_dir().join("rontodoc_datatype_prop_test");
        render(&metadata, &temp_dir).expect("Render failed");

        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read output");

        // Verify datatype property renders
        assert!(
            html.contains("Datatype Property"),
            "Should show Datatype Property badge"
        );
        assert!(
            html.contains("integer"),
            "Should display xsd:integer as range"
        );

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn renders_inverse_of_as_characteristic() {
        use crate::model::{OntologyProperty, PropertyType};

        let metadata = OntologyMetadata {
            iri: "http://example.org/test".to_string(),
            label: Some("Test Ontology".to_string()),
            comment: None,
            version: None,
            classes: vec![],
            properties: vec![
                OntologyProperty {
                    iri: "http://example.org/test#hasOwner".to_string(),
                    id: "hasOwner".to_string(),
                    label: Some("has owner".to_string()),
                    comment: None,
                    property_type: PropertyType::ObjectProperty,
                    domain_iri: None,
                    range_iri: None,
                    inverse_of_iri: None,
                },
                OntologyProperty {
                    iri: "http://example.org/test#owns".to_string(),
                    id: "owns".to_string(),
                    label: Some("owns".to_string()),
                    comment: None,
                    property_type: PropertyType::ObjectProperty,
                    domain_iri: None,
                    range_iri: None,
                    inverse_of_iri: Some("http://example.org/test#hasOwner".to_string()),
                },
            ],
            individuals: vec![],
        };

        let temp_dir = std::env::temp_dir().join("rontodoc_inverse_test");
        render(&metadata, &temp_dir).expect("Render failed");

        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read output");

        // Verify inverse relationship shown as characteristic
        assert!(
            html.contains("Inverse of: has owner"),
            "Should show inverse of characteristic"
        );

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn renders_individual_cards_with_type_and_properties() {
        use crate::model::{OntologyIndividual, OntologyProperty, PropertyType, PropertyValue};

        let metadata = OntologyMetadata {
            iri: "http://example.org/test".to_string(),
            label: Some("Test Ontology".to_string()),
            comment: None,
            version: None,
            classes: vec![OntologyClass {
                iri: "http://example.org/test#Dog".to_string(),
                id: "Dog".to_string(),
                label: Some("Dog".to_string()),
                comment: None,
                superclass_iri: None,
            }],
            properties: vec![
                OntologyProperty {
                    iri: "http://example.org/test#hasName".to_string(),
                    id: "hasName".to_string(),
                    label: Some("has name".to_string()),
                    comment: None,
                    property_type: PropertyType::DatatypeProperty,
                    domain_iri: None,
                    range_iri: None,
                    inverse_of_iri: None,
                },
                OntologyProperty {
                    iri: "http://example.org/test#hasAge".to_string(),
                    id: "hasAge".to_string(),
                    label: Some("has age".to_string()),
                    comment: None,
                    property_type: PropertyType::DatatypeProperty,
                    domain_iri: None,
                    range_iri: None,
                    inverse_of_iri: None,
                },
            ],
            individuals: vec![OntologyIndividual {
                iri: "http://example.org/test#fido".to_string(),
                id: "fido".to_string(),
                label: Some("Fido".to_string()),
                comment: None,
                type_iris: vec!["http://example.org/test#Dog".to_string()],
                property_values: vec![
                    PropertyValue {
                        property_iri: "http://example.org/test#hasAge".to_string(),
                        property_id: "hasAge".to_string(),
                        property_label: Some("has age".to_string()),
                        value: "5".to_string(),
                    },
                    PropertyValue {
                        property_iri: "http://example.org/test#hasName".to_string(),
                        property_id: "hasName".to_string(),
                        property_label: Some("has name".to_string()),
                        value: "Fido".to_string(),
                    },
                ],
            }],
        };

        let temp_dir = std::env::temp_dir().join("rontodoc_individual_test");
        render(&metadata, &temp_dir).expect("Render failed");

        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read output");

        // Verify individual card is rendered
        assert!(
            html.contains("individual-card"),
            "Should contain individual card"
        );
        assert!(
            html.contains("id=\"ind-fido\""),
            "Should have individual anchor"
        );
        assert!(html.contains("Individual"), "Should show Individual badge");
        assert!(html.contains("Fido"), "Should contain individual label");

        // Verify type links to class
        assert!(
            html.contains("href=\"#class-Dog\""),
            "Type should link to Dog class"
        );

        // Verify property values
        assert!(
            html.contains("has age"),
            "Should show property label 'has age'"
        );
        assert!(
            html.contains("has name"),
            "Should show property label 'has name'"
        );

        // Verify sidebar has individuals link
        assert!(
            html.contains("href=\"#individuals\""),
            "Sidebar should have individuals link"
        );

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }
}
