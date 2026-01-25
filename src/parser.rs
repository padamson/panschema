use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use sophia::api::graph::Graph;
use sophia::api::ns::{Namespace, rdf, rdfs};
use sophia::api::prelude::*;
use sophia::api::term::SimpleTerm;
use sophia::inmem::graph::FastGraph;
use sophia::turtle::parser::turtle;

use crate::model::{OntologyClass, OntologyMetadata, OntologyProperty, PropertyType};

/// OWL namespace
const OWL_NS: &str = "http://www.w3.org/2002/07/owl#";

/// Extract the local name (fragment or last path segment) from an IRI
pub fn extract_id_from_iri(iri: &str) -> String {
    // Try fragment first (after #)
    if let Some(pos) = iri.rfind('#') {
        return iri[pos + 1..].to_string();
    }
    // Fall back to last path segment (after /)
    if let Some(pos) = iri.rfind('/') {
        return iri[pos + 1..].to_string();
    }
    // Last resort: use the whole IRI
    iri.to_string()
}

/// Parse a Turtle file and extract ontology metadata
pub fn parse_ontology(path: &Path) -> anyhow::Result<OntologyMetadata> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let graph: FastGraph = turtle::parse_bufread(reader).collect_triples()?;

    let owl = Namespace::new_unchecked(OWL_NS);
    let owl_ontology = owl.get("Ontology")?;

    // Find the ontology IRI (subject of rdf:type owl:Ontology)
    let ontology_iri = graph
        .triples_matching(Any, [rdf::type_], [owl_ontology])
        .filter_map(Result::ok)
        .map(|t| t.s().to_owned())
        .next()
        .ok_or_else(|| anyhow::anyhow!("No owl:Ontology found in {}", path.display()))?;

    // Extract the IRI string
    let iri = match &ontology_iri {
        SimpleTerm::Iri(iri) => iri.to_string(),
        _ => anyhow::bail!("Ontology subject is not an IRI"),
    };

    // Helper to get a string literal for a predicate
    fn get_literal_value<T: Term>(
        graph: &FastGraph,
        subject: &SimpleTerm,
        predicate: T,
    ) -> Option<String> {
        graph
            .triples_matching([subject], [predicate], Any)
            .filter_map(Result::ok)
            .filter_map(|t| match t.o() {
                SimpleTerm::LiteralLanguage(lit, _) => Some(lit.to_string()),
                SimpleTerm::LiteralDatatype(lit, _) => Some(lit.to_string()),
                _ => None,
            })
            .next()
    }

    let owl_version_info = owl.get("versionInfo")?;

    let label = get_literal_value(&graph, &ontology_iri, rdfs::label);
    let comment = get_literal_value(&graph, &ontology_iri, rdfs::comment);
    let version = get_literal_value(&graph, &ontology_iri, owl_version_info);

    // Extract all owl:Class entities
    let owl_class = owl.get("Class")?;
    let owl_class_term: SimpleTerm = owl_class.into_term();
    let classes = extract_classes(&graph, &owl_class_term)?;

    // Extract all properties
    let owl_object_property = owl.get("ObjectProperty")?;
    let owl_object_property_term: SimpleTerm = owl_object_property.into_term();
    let owl_datatype_property = owl.get("DatatypeProperty")?;
    let owl_datatype_property_term: SimpleTerm = owl_datatype_property.into_term();
    let owl_inverse_of = owl.get("inverseOf")?;
    let owl_inverse_of_term: SimpleTerm = owl_inverse_of.into_term();
    let properties = extract_properties(
        &graph,
        &owl_object_property_term,
        &owl_datatype_property_term,
        &owl_inverse_of_term,
    )?;

    Ok(OntologyMetadata {
        iri,
        label,
        comment,
        version,
        classes,
        properties,
    })
}

/// Extract all owl:Class entities from the graph
fn extract_classes(
    graph: &FastGraph,
    owl_class: &SimpleTerm<'_>,
) -> anyhow::Result<Vec<OntologyClass>> {
    // Helper to get a string literal for a predicate
    fn get_literal_value<T: Term>(
        graph: &FastGraph,
        subject: &SimpleTerm,
        predicate: T,
    ) -> Option<String> {
        graph
            .triples_matching([subject], [predicate], Any)
            .filter_map(Result::ok)
            .filter_map(|t| match t.o() {
                SimpleTerm::LiteralLanguage(lit, _) => Some(lit.to_string()),
                SimpleTerm::LiteralDatatype(lit, _) => Some(lit.to_string()),
                _ => None,
            })
            .next()
    }

    // Helper to get an IRI value for a predicate
    fn get_iri_value<T: Term>(
        graph: &FastGraph,
        subject: &SimpleTerm,
        predicate: T,
    ) -> Option<String> {
        graph
            .triples_matching([subject], [predicate], Any)
            .filter_map(Result::ok)
            .filter_map(|t| match t.o() {
                SimpleTerm::Iri(iri) => Some(iri.to_string()),
                _ => None,
            })
            .next()
    }

    // Find all subjects with rdf:type owl:Class
    let class_iris: Vec<SimpleTerm> = graph
        .triples_matching(Any, [rdf::type_], [owl_class])
        .filter_map(Result::ok)
        .map(|t| t.s().to_owned())
        .collect();

    let mut classes = Vec::new();

    for class_iri in class_iris {
        // Skip blank nodes and non-IRI subjects
        let iri = match &class_iri {
            SimpleTerm::Iri(iri) => iri.to_string(),
            _ => continue,
        };

        // Skip built-in OWL classes
        if iri.starts_with(OWL_NS) {
            continue;
        }

        let id = extract_id_from_iri(&iri);
        let label = get_literal_value(graph, &class_iri, rdfs::label);
        let comment = get_literal_value(graph, &class_iri, rdfs::comment);
        let superclass_iri = get_iri_value(graph, &class_iri, rdfs::subClassOf);

        classes.push(OntologyClass {
            iri,
            id,
            label,
            comment,
            superclass_iri,
        });
    }

    // Sort classes by label (or id if no label) for consistent ordering
    classes.sort_by(|a, b| Ord::cmp(a.display_label(), b.display_label()));

    Ok(classes)
}

/// Extract all owl:ObjectProperty and owl:DatatypeProperty entities from the graph
fn extract_properties(
    graph: &FastGraph,
    owl_object_property: &SimpleTerm<'_>,
    owl_datatype_property: &SimpleTerm<'_>,
    owl_inverse_of: &SimpleTerm<'_>,
) -> anyhow::Result<Vec<OntologyProperty>> {
    // Helper to get a string literal for a predicate
    fn get_literal_value<T: Term>(
        graph: &FastGraph,
        subject: &SimpleTerm,
        predicate: T,
    ) -> Option<String> {
        graph
            .triples_matching([subject], [predicate], Any)
            .filter_map(Result::ok)
            .filter_map(|t| match t.o() {
                SimpleTerm::LiteralLanguage(lit, _) => Some(lit.to_string()),
                SimpleTerm::LiteralDatatype(lit, _) => Some(lit.to_string()),
                _ => None,
            })
            .next()
    }

    // Helper to get an IRI value for a predicate
    fn get_iri_value<T: Term>(
        graph: &FastGraph,
        subject: &SimpleTerm,
        predicate: T,
    ) -> Option<String> {
        graph
            .triples_matching([subject], [predicate], Any)
            .filter_map(Result::ok)
            .filter_map(|t| match t.o() {
                SimpleTerm::Iri(iri) => Some(iri.to_string()),
                _ => None,
            })
            .next()
    }

    let mut properties = Vec::new();

    // Extract object properties
    let object_prop_iris: Vec<SimpleTerm> = graph
        .triples_matching(Any, [rdf::type_], [owl_object_property])
        .filter_map(Result::ok)
        .map(|t| t.s().to_owned())
        .collect();

    for prop_iri in object_prop_iris {
        let iri = match &prop_iri {
            SimpleTerm::Iri(iri) => iri.to_string(),
            _ => continue,
        };
        if iri.starts_with(OWL_NS) {
            continue;
        }

        let id = extract_id_from_iri(&iri);
        let label = get_literal_value(graph, &prop_iri, rdfs::label);
        let comment = get_literal_value(graph, &prop_iri, rdfs::comment);
        let domain_iri = get_iri_value(graph, &prop_iri, rdfs::domain);
        let range_iri = get_iri_value(graph, &prop_iri, rdfs::range);
        let inverse_of_iri = get_iri_value(graph, &prop_iri, owl_inverse_of);

        properties.push(OntologyProperty {
            iri,
            id,
            label,
            comment,
            property_type: PropertyType::ObjectProperty,
            domain_iri,
            range_iri,
            inverse_of_iri,
        });
    }

    // Extract datatype properties
    let datatype_prop_iris: Vec<SimpleTerm> = graph
        .triples_matching(Any, [rdf::type_], [owl_datatype_property])
        .filter_map(Result::ok)
        .map(|t| t.s().to_owned())
        .collect();

    for prop_iri in datatype_prop_iris {
        let iri = match &prop_iri {
            SimpleTerm::Iri(iri) => iri.to_string(),
            _ => continue,
        };
        if iri.starts_with(OWL_NS) {
            continue;
        }

        let id = extract_id_from_iri(&iri);
        let label = get_literal_value(graph, &prop_iri, rdfs::label);
        let comment = get_literal_value(graph, &prop_iri, rdfs::comment);
        let domain_iri = get_iri_value(graph, &prop_iri, rdfs::domain);
        let range_iri = get_iri_value(graph, &prop_iri, rdfs::range);

        properties.push(OntologyProperty {
            iri,
            id,
            label,
            comment,
            property_type: PropertyType::DatatypeProperty,
            domain_iri,
            range_iri,
            inverse_of_iri: None,
        });
    }

    // Sort properties by label (or id if no label) for consistent ordering
    properties.sort_by(|a, b| Ord::cmp(a.display_label(), b.display_label()));

    Ok(properties)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_reference_ontology() {
        let path = PathBuf::from("tests/fixtures/reference.ttl");
        let meta = parse_ontology(&path).expect("Failed to parse reference ontology");

        assert_eq!(meta.iri, "http://example.org/rontodoc/reference");
        assert_eq!(meta.label, Some("Rontodoc Reference Ontology".to_string()));
        assert!(meta.comment.is_some());
        assert_eq!(meta.version, Some("0.1.0".to_string()));
    }

    #[test]
    fn extracts_classes_from_reference_ontology() {
        let path = PathBuf::from("tests/fixtures/reference.ttl");
        let meta = parse_ontology(&path).expect("Failed to parse reference ontology");

        // Reference ontology has 5 classes: Animal, Cat, Dog, Mammal, Person
        assert_eq!(meta.classes.len(), 5);

        // Classes should be sorted alphabetically by display label
        let class_labels: Vec<&str> = meta.classes.iter().map(|c| c.display_label()).collect();
        assert_eq!(
            class_labels,
            vec!["Animal", "Cat", "Dog", "Mammal", "Person"]
        );

        // Check a specific class with subclass relationship
        let dog = meta.classes.iter().find(|c| c.id == "Dog").unwrap();
        assert_eq!(dog.label, Some("Dog".to_string()));
        assert_eq!(
            dog.comment,
            Some("A domesticated carnivorous mammal.".to_string())
        );
        assert_eq!(
            dog.superclass_iri,
            Some("http://example.org/rontodoc/reference#Mammal".to_string())
        );

        // Check a root class (no superclass)
        let animal = meta.classes.iter().find(|c| c.id == "Animal").unwrap();
        assert_eq!(animal.superclass_iri, None);
    }

    #[test]
    fn extracts_properties_from_reference_ontology() {
        let path = PathBuf::from("tests/fixtures/reference.ttl");
        let meta = parse_ontology(&path).expect("Failed to parse reference ontology");

        // Reference ontology has 4 properties: hasAge, hasName, hasOwner, owns
        assert_eq!(meta.properties.len(), 4);

        // Properties should be sorted alphabetically by display label
        let prop_labels: Vec<&str> = meta.properties.iter().map(|p| p.display_label()).collect();
        assert_eq!(
            prop_labels,
            vec!["has age", "has name", "has owner", "owns"]
        );
    }

    #[test]
    fn extracts_object_properties_with_domain_range() {
        use crate::model::PropertyType;

        let path = PathBuf::from("tests/fixtures/reference.ttl");
        let meta = parse_ontology(&path).expect("Failed to parse reference ontology");

        let has_owner = meta.properties.iter().find(|p| p.id == "hasOwner").unwrap();
        assert_eq!(has_owner.label, Some("has owner".to_string()));
        assert_eq!(
            has_owner.comment,
            Some("Relates an animal to its owner.".to_string())
        );
        assert_eq!(has_owner.property_type, PropertyType::ObjectProperty);
        assert_eq!(
            has_owner.domain_iri,
            Some("http://example.org/rontodoc/reference#Animal".to_string())
        );
        assert_eq!(
            has_owner.range_iri,
            Some("http://example.org/rontodoc/reference#Person".to_string())
        );
    }

    #[test]
    fn extracts_inverse_of_relationship() {
        let path = PathBuf::from("tests/fixtures/reference.ttl");
        let meta = parse_ontology(&path).expect("Failed to parse reference ontology");

        let owns = meta.properties.iter().find(|p| p.id == "owns").unwrap();
        assert_eq!(
            owns.inverse_of_iri,
            Some("http://example.org/rontodoc/reference#hasOwner".to_string())
        );
    }

    #[test]
    fn extracts_datatype_properties() {
        use crate::model::PropertyType;

        let path = PathBuf::from("tests/fixtures/reference.ttl");
        let meta = parse_ontology(&path).expect("Failed to parse reference ontology");

        let has_age = meta.properties.iter().find(|p| p.id == "hasAge").unwrap();
        assert_eq!(has_age.property_type, PropertyType::DatatypeProperty);
        assert_eq!(
            has_age.domain_iri,
            Some("http://example.org/rontodoc/reference#Animal".to_string())
        );
        assert_eq!(
            has_age.range_iri,
            Some("http://www.w3.org/2001/XMLSchema#integer".to_string())
        );
        assert_eq!(has_age.inverse_of_iri, None);

        let has_name = meta.properties.iter().find(|p| p.id == "hasName").unwrap();
        assert_eq!(has_name.property_type, PropertyType::DatatypeProperty);
        assert_eq!(has_name.domain_iri, None); // hasName has no domain
        assert_eq!(
            has_name.range_iri,
            Some("http://www.w3.org/2001/XMLSchema#string".to_string())
        );
    }
}
