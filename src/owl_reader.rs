//! OWL/Turtle Reader
//!
//! Reads OWL ontologies in Turtle format and converts them to LinkML IR.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use sophia::api::graph::Graph;
use sophia::api::ns::{Namespace, rdf, rdfs};
use sophia::api::prelude::*;
use sophia::api::term::SimpleTerm;
use sophia::inmem::graph::FastGraph;
use sophia::turtle::parser::turtle;

use crate::io::{IoError, IoResult, Reader};
use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};
use crate::owl_model::{
    OntologyClass, OntologyIndividual, OntologyMetadata, OntologyProperty, PropertyType,
    PropertyValue,
};

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

/// Reader for OWL ontologies in Turtle (.ttl) format
pub struct OwlReader;

impl OwlReader {
    /// Create a new OWL reader
    pub fn new() -> Self {
        Self
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
        let classes = Self::extract_classes(&graph, &owl_class_term)?;

        // Extract all properties
        let owl_object_property = owl.get("ObjectProperty")?;
        let owl_object_property_term: SimpleTerm = owl_object_property.into_term();
        let owl_datatype_property = owl.get("DatatypeProperty")?;
        let owl_datatype_property_term: SimpleTerm = owl_datatype_property.into_term();
        let owl_inverse_of = owl.get("inverseOf")?;
        let owl_inverse_of_term: SimpleTerm = owl_inverse_of.into_term();
        let properties = Self::extract_properties(
            &graph,
            &owl_object_property_term,
            &owl_datatype_property_term,
            &owl_inverse_of_term,
        )?;

        // Extract all owl:NamedIndividual entities
        let owl_named_individual = owl.get("NamedIndividual")?;
        let owl_named_individual_term: SimpleTerm = owl_named_individual.into_term();
        let individuals = Self::extract_individuals(&graph, &owl_named_individual_term)?;

        Ok(OntologyMetadata {
            iri,
            label,
            comment,
            version,
            classes,
            properties,
            individuals,
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

    /// Extract all owl:NamedIndividual entities from the graph
    fn extract_individuals(
        graph: &FastGraph,
        owl_named_individual: &SimpleTerm<'_>,
    ) -> anyhow::Result<Vec<OntologyIndividual>> {
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

        // RDF/RDFS/OWL namespace prefixes for filtering metadata predicates
        const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
        const RDFS_NS: &str = "http://www.w3.org/2000/01/rdf-schema#";

        // Find all subjects with rdf:type owl:NamedIndividual
        let ind_iris: Vec<SimpleTerm> = graph
            .triples_matching(Any, [rdf::type_], [owl_named_individual])
            .filter_map(Result::ok)
            .map(|t| t.s().to_owned())
            .collect();

        let mut individuals = Vec::new();

        for ind_iri in ind_iris {
            // Skip blank nodes and non-IRI subjects
            let iri = match &ind_iri {
                SimpleTerm::Iri(iri) => iri.to_string(),
                _ => continue,
            };

            let id = extract_id_from_iri(&iri);
            let label = get_literal_value(graph, &ind_iri, rdfs::label);
            let comment = get_literal_value(graph, &ind_iri, rdfs::comment);

            // Extract type IRIs (excluding owl:NamedIndividual and OWL built-ins)
            let type_iris: Vec<String> = graph
                .triples_matching([&ind_iri], [rdf::type_], Any)
                .filter_map(Result::ok)
                .filter_map(|t| match t.o() {
                    SimpleTerm::Iri(iri) => {
                        let iri_str = iri.to_string();
                        if iri_str.starts_with(OWL_NS) || iri_str.starts_with(RDF_NS) {
                            None
                        } else {
                            Some(iri_str)
                        }
                    }
                    _ => None,
                })
                .collect();

            // Extract property values (all triples except metadata predicates)
            let mut property_values: Vec<PropertyValue> = graph
                .triples_matching([&ind_iri], Any, Any)
                .filter_map(Result::ok)
                .filter_map(|t| {
                    // Get the predicate IRI
                    let pred_iri = match t.p() {
                        SimpleTerm::Iri(iri) => iri.to_string(),
                        _ => return None,
                    };

                    // Skip metadata predicates
                    if pred_iri.starts_with(RDF_NS) || pred_iri.starts_with(RDFS_NS) {
                        return None;
                    }

                    // Get the object value as a string
                    let value = match t.o() {
                        SimpleTerm::LiteralLanguage(lit, _) => lit.to_string(),
                        SimpleTerm::LiteralDatatype(lit, _) => lit.to_string(),
                        SimpleTerm::Iri(iri) => iri.to_string(),
                        _ => return None,
                    };

                    let property_id = extract_id_from_iri(&pred_iri);

                    // Look up the property's rdfs:label in the graph
                    let pred_term: SimpleTerm = SimpleTerm::Iri(
                        sophia::api::term::IriRef::new_unchecked(pred_iri.clone().into()),
                    );
                    let property_label = get_literal_value(graph, &pred_term, rdfs::label);

                    Some(PropertyValue {
                        property_id,
                        property_label,
                        value,
                    })
                })
                .collect();

            // Sort property values by property_id for consistent ordering
            property_values.sort_by(|a, b| a.property_id.cmp(&b.property_id));

            individuals.push(OntologyIndividual {
                iri,
                id,
                label,
                comment,
                type_iris,
                property_values,
            });
        }

        // Sort individuals by display_label for consistent ordering
        individuals.sort_by(|a, b| Ord::cmp(a.display_label(), b.display_label()));

        Ok(individuals)
    }

    /// Map OntologyMetadata to LinkML SchemaDefinition
    fn map_to_linkml(metadata: &OntologyMetadata) -> SchemaDefinition {
        let mut schema = SchemaDefinition::new(extract_id_from_iri(&metadata.iri));

        // Map ontology metadata
        schema.id = Some(metadata.iri.clone());
        schema.title = Some(metadata.title().to_string());
        schema.description = metadata.comment.clone();
        schema.version = metadata.version.clone();

        // Record source format in annotations
        schema
            .annotations
            .insert("panschema:source_format".to_string(), "owl".to_string());

        // Build class lookup for hierarchy resolution
        let class_iris: std::collections::HashSet<_> =
            metadata.classes.iter().map(|c| c.iri.as_str()).collect();

        // Map classes
        for owl_class in &metadata.classes {
            let mut class_def = ClassDefinition::new(&owl_class.id);
            class_def.description = owl_class.comment.clone();
            class_def.class_uri = Some(owl_class.iri.clone());

            // Map superclass to is_a
            if let Some(ref superclass_iri) = owl_class.superclass_iri {
                let superclass_id = extract_id_from_iri(superclass_iri);
                class_def.is_a = Some(superclass_id);
            }

            // Store label in annotations if different from id
            if let Some(ref label) = owl_class.label {
                if label != &owl_class.id {
                    class_def
                        .annotations
                        .insert("panschema:label".to_string(), label.clone());
                }
            }

            schema.classes.insert(owl_class.id.clone(), class_def);
        }

        // Map properties to slots
        for owl_prop in &metadata.properties {
            let mut slot_def = SlotDefinition::new(&owl_prop.id);
            slot_def.description = owl_prop.comment.clone();
            slot_def.slot_uri = Some(owl_prop.iri.clone());

            // Map domain
            if let Some(ref domain_iri) = owl_prop.domain_iri {
                let domain_id = extract_id_from_iri(domain_iri);
                slot_def.domain = Some(domain_id);
            }

            // Map range - check if it's a class or a datatype
            if let Some(ref range_iri) = owl_prop.range_iri {
                let range_id = extract_id_from_iri(range_iri);

                // If range is a known class, use the class name
                // Otherwise, it's probably an XSD datatype
                if class_iris.contains(range_iri.as_str()) {
                    slot_def.range = Some(range_id);
                } else {
                    // Map XSD types to LinkML built-in types
                    slot_def.range = Some(Self::map_xsd_to_linkml(&range_id));
                }
            }

            // Map inverse relationship
            if let Some(ref inverse_iri) = owl_prop.inverse_of_iri {
                let inverse_id = extract_id_from_iri(inverse_iri);
                slot_def.inverse = Some(inverse_id);
            }

            // Store property type in annotations
            let prop_type = match owl_prop.property_type {
                PropertyType::ObjectProperty => "ObjectProperty",
                PropertyType::DatatypeProperty => "DatatypeProperty",
            };
            slot_def.annotations.insert(
                "panschema:owl_property_type".to_string(),
                prop_type.to_string(),
            );

            // Store label in annotations if different from id
            if let Some(ref label) = owl_prop.label {
                if label != &owl_prop.id {
                    slot_def
                        .annotations
                        .insert("panschema:label".to_string(), label.clone());
                }
            }

            schema.slots.insert(owl_prop.id.clone(), slot_def);
        }

        // Map individuals as annotations on the schema
        // (LinkML doesn't have a direct equivalent, so we store them as metadata)
        if !metadata.individuals.is_empty() {
            let individual_names: Vec<_> = metadata
                .individuals
                .iter()
                .map(|ind| ind.id.clone())
                .collect();
            schema.annotations.insert(
                "panschema:individuals".to_string(),
                individual_names.join(","),
            );

            // Store detailed individual data in annotations
            for individual in &metadata.individuals {
                let key = format!("panschema:individual:{}", individual.id);
                let types = individual.type_iris.join(",");
                schema.annotations.insert(key, types);

                // Store IRI
                let iri_key = format!("panschema:individual:{}:_iri", individual.id);
                schema.annotations.insert(iri_key, individual.iri.clone());

                // Store label if present
                if let Some(ref label) = individual.label {
                    let label_key = format!("panschema:individual:{}:_label", individual.id);
                    schema.annotations.insert(label_key, label.clone());
                }

                // Store comment if present
                if let Some(ref comment) = individual.comment {
                    let comment_key = format!("panschema:individual:{}:_comment", individual.id);
                    schema.annotations.insert(comment_key, comment.clone());
                }

                // Store property values
                for pv in &individual.property_values {
                    let pv_key =
                        format!("panschema:individual:{}:{}", individual.id, pv.property_id);
                    schema.annotations.insert(pv_key, pv.value.clone());

                    // Store property label if present
                    if let Some(ref prop_label) = pv.property_label {
                        let prop_label_key = format!(
                            "panschema:individual:{}:{}:_label",
                            individual.id, pv.property_id
                        );
                        schema
                            .annotations
                            .insert(prop_label_key, prop_label.clone());
                    }
                }
            }
        }

        schema
    }

    /// Map XSD datatypes to LinkML built-in types
    fn map_xsd_to_linkml(xsd_type: &str) -> String {
        match xsd_type {
            "string" => "string".to_string(),
            "integer" | "int" | "long" | "short" | "byte" => "integer".to_string(),
            "float" | "double" | "decimal" => "float".to_string(),
            "boolean" => "boolean".to_string(),
            "date" => "date".to_string(),
            "dateTime" => "datetime".to_string(),
            "time" => "time".to_string(),
            "anyURI" => "uri".to_string(),
            _ => xsd_type.to_string(), // Keep unknown types as-is
        }
    }
}

impl Default for OwlReader {
    fn default() -> Self {
        Self::new()
    }
}

impl Reader for OwlReader {
    fn read(&self, input: &Path) -> IoResult<SchemaDefinition> {
        let metadata = Self::parse_ontology(input).map_err(|e| IoError::Parse(e.to_string()))?;

        Ok(Self::map_to_linkml(&metadata))
    }

    fn supported_extensions(&self) -> &[&str] {
        &["ttl", "turtle"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn reference_ontology_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("reference.ttl")
    }

    // Parser tests (moved from parser.rs)
    #[test]
    fn parses_reference_ontology() {
        let path = reference_ontology_path();
        let meta = OwlReader::parse_ontology(&path).expect("Failed to parse reference ontology");

        assert_eq!(meta.iri, "http://example.org/panschema/reference");
        assert_eq!(meta.label, Some("panschema Reference Ontology".to_string()));
        assert!(meta.comment.is_some());
        assert_eq!(meta.version, Some("0.2.0".to_string()));
    }

    #[test]
    fn extracts_classes_from_reference_ontology() {
        let path = reference_ontology_path();
        let meta = OwlReader::parse_ontology(&path).expect("Failed to parse reference ontology");

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
            Some("http://example.org/panschema/reference#Mammal".to_string())
        );

        // Check a root class (no superclass)
        let animal = meta.classes.iter().find(|c| c.id == "Animal").unwrap();
        assert_eq!(animal.superclass_iri, None);
    }

    #[test]
    fn extracts_properties_from_reference_ontology() {
        let path = reference_ontology_path();
        let meta = OwlReader::parse_ontology(&path).expect("Failed to parse reference ontology");

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
        let path = reference_ontology_path();
        let meta = OwlReader::parse_ontology(&path).expect("Failed to parse reference ontology");

        let has_owner = meta.properties.iter().find(|p| p.id == "hasOwner").unwrap();
        assert_eq!(has_owner.label, Some("has owner".to_string()));
        assert_eq!(
            has_owner.comment,
            Some("Relates an animal to its owner.".to_string())
        );
        assert_eq!(has_owner.property_type, PropertyType::ObjectProperty);
        assert_eq!(
            has_owner.domain_iri,
            Some("http://example.org/panschema/reference#Animal".to_string())
        );
        assert_eq!(
            has_owner.range_iri,
            Some("http://example.org/panschema/reference#Person".to_string())
        );
    }

    #[test]
    fn extracts_inverse_of_relationship() {
        let path = reference_ontology_path();
        let meta = OwlReader::parse_ontology(&path).expect("Failed to parse reference ontology");

        let owns = meta.properties.iter().find(|p| p.id == "owns").unwrap();
        assert_eq!(
            owns.inverse_of_iri,
            Some("http://example.org/panschema/reference#hasOwner".to_string())
        );
    }

    #[test]
    fn extracts_datatype_properties() {
        let path = reference_ontology_path();
        let meta = OwlReader::parse_ontology(&path).expect("Failed to parse reference ontology");

        let has_age = meta.properties.iter().find(|p| p.id == "hasAge").unwrap();
        assert_eq!(has_age.property_type, PropertyType::DatatypeProperty);
        assert_eq!(
            has_age.domain_iri,
            Some("http://example.org/panschema/reference#Animal".to_string())
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

    #[test]
    fn extracts_individuals_from_reference_ontology() {
        let path = reference_ontology_path();
        let meta = OwlReader::parse_ontology(&path).expect("Failed to parse reference ontology");

        // Reference ontology has 1 individual: fido
        assert_eq!(meta.individuals.len(), 1);

        let fido = &meta.individuals[0];
        assert_eq!(fido.id, "fido");
        assert_eq!(fido.label, Some("Fido".to_string()));
        assert_eq!(fido.iri, "http://example.org/panschema/reference#fido");
    }

    #[test]
    fn extracts_individual_types() {
        let path = reference_ontology_path();
        let meta = OwlReader::parse_ontology(&path).expect("Failed to parse reference ontology");

        let fido = &meta.individuals[0];
        // fido is a Dog (owl:NamedIndividual should be filtered out)
        assert_eq!(fido.type_iris.len(), 1);
        assert_eq!(
            fido.type_iris[0],
            "http://example.org/panschema/reference#Dog"
        );
    }

    #[test]
    fn extracts_individual_property_values() {
        let path = reference_ontology_path();
        let meta = OwlReader::parse_ontology(&path).expect("Failed to parse reference ontology");

        let fido = &meta.individuals[0];
        // fido has hasAge=5 and hasName="Fido"
        assert_eq!(fido.property_values.len(), 2);

        let has_age = fido
            .property_values
            .iter()
            .find(|pv| pv.property_id == "hasAge")
            .unwrap();
        assert_eq!(has_age.value, "5");
        assert_eq!(has_age.property_label, Some("has age".to_string()));

        let has_name = fido
            .property_values
            .iter()
            .find(|pv| pv.property_id == "hasName")
            .unwrap();
        assert_eq!(has_name.value, "Fido");
        assert_eq!(has_name.property_label, Some("has name".to_string()));
    }

    // Reader tests
    #[test]
    fn owl_reader_supports_ttl_extension() {
        let reader = OwlReader::new();
        assert!(reader.supports_extension("ttl"));
        assert!(reader.supports_extension("TTL"));
        assert!(reader.supports_extension("turtle"));
        assert!(!reader.supports_extension("yaml"));
    }

    #[test]
    fn owl_reader_parses_reference_ontology() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        assert_eq!(schema.name, "reference");
        assert!(schema.id.is_some());
    }

    #[test]
    fn owl_reader_maps_ontology_metadata() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        assert_eq!(
            schema.title,
            Some("panschema Reference Ontology".to_string())
        );
        assert!(schema.description.is_some());
        assert_eq!(schema.version, Some("0.2.0".to_string()));
    }

    #[test]
    fn owl_reader_preserves_source_format_annotation() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        assert_eq!(
            schema.annotations.get("panschema:source_format"),
            Some(&"owl".to_string())
        );
    }

    #[test]
    fn owl_reader_maps_classes() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        // Reference ontology has: Animal, Mammal, Dog, Cat, Person
        assert!(schema.classes.contains_key("Animal"));
        assert!(schema.classes.contains_key("Dog"));
        assert!(schema.classes.contains_key("Person"));

        // Check class hierarchy
        let dog = schema.classes.get("Dog").unwrap();
        assert_eq!(dog.is_a, Some("Mammal".to_string()));

        let mammal = schema.classes.get("Mammal").unwrap();
        assert_eq!(mammal.is_a, Some("Animal".to_string()));
    }

    #[test]
    fn owl_reader_maps_class_metadata() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let animal = schema.classes.get("Animal").unwrap();
        assert!(animal.class_uri.is_some());
        assert!(animal.description.is_some());
    }

    #[test]
    fn owl_reader_maps_properties_to_slots() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        // Reference ontology has: hasOwner, owns, hasName, hasAge
        assert!(schema.slots.contains_key("hasOwner"));
        assert!(schema.slots.contains_key("owns"));
        assert!(schema.slots.contains_key("hasName"));
        assert!(schema.slots.contains_key("hasAge"));
    }

    #[test]
    fn owl_reader_maps_object_property_with_domain_range() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let has_owner = schema.slots.get("hasOwner").unwrap();
        assert!(has_owner.domain.is_some());
        assert!(has_owner.range.is_some());
        assert_eq!(
            has_owner.annotations.get("panschema:owl_property_type"),
            Some(&"ObjectProperty".to_string())
        );
    }

    #[test]
    fn owl_reader_maps_datatype_property() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let has_age = schema.slots.get("hasAge").unwrap();
        assert_eq!(has_age.range, Some("integer".to_string()));
        assert_eq!(
            has_age.annotations.get("panschema:owl_property_type"),
            Some(&"DatatypeProperty".to_string())
        );
    }

    #[test]
    fn owl_reader_maps_inverse_relationship() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let owns = schema.slots.get("owns").unwrap();
        assert_eq!(owns.inverse, Some("hasOwner".to_string()));
    }

    #[test]
    fn owl_reader_stores_individuals_in_annotations() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        // Reference ontology has: fido
        let individuals = schema.annotations.get("panschema:individuals");
        assert!(individuals.is_some());
        assert!(individuals.unwrap().contains("fido"));
    }

    #[test]
    fn xsd_type_mapping() {
        assert_eq!(OwlReader::map_xsd_to_linkml("string"), "string");
        assert_eq!(OwlReader::map_xsd_to_linkml("integer"), "integer");
        assert_eq!(OwlReader::map_xsd_to_linkml("int"), "integer");
        assert_eq!(OwlReader::map_xsd_to_linkml("boolean"), "boolean");
        assert_eq!(OwlReader::map_xsd_to_linkml("float"), "float");
        assert_eq!(OwlReader::map_xsd_to_linkml("anyURI"), "uri");
        assert_eq!(OwlReader::map_xsd_to_linkml("customType"), "customType");
    }

    #[test]
    fn owl_reader_roundtrip_class_count() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        // Verify we got the same number of classes as the direct parser
        let original = OwlReader::parse_ontology(&reference_ontology_path()).unwrap();
        assert_eq!(schema.classes.len(), original.classes.len());
    }

    #[test]
    fn owl_reader_roundtrip_property_count() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        // Verify we got the same number of properties as the direct parser
        let original = OwlReader::parse_ontology(&reference_ontology_path()).unwrap();
        assert_eq!(schema.slots.len(), original.properties.len());
    }
}
