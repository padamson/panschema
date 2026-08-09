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
    Annotations, OntologyClass, OntologyIndividual, OntologyMetadata, OntologyProperty,
    PropertyCharacteristics, PropertyType, PropertyValue,
};

/// OWL namespace
const OWL_NS: &str = "http://www.w3.org/2002/07/owl#";

/// SKOS namespace
const SKOS_NS: &str = "http://www.w3.org/2004/02/skos/core#";

/// Collect every literal object for a (subject, predicate) pair.
fn collect_literal_values<T: Term>(
    graph: &FastGraph,
    subject: &SimpleTerm,
    predicate: T,
) -> Vec<String> {
    graph
        .triples_matching([subject], [predicate], Any)
        .filter_map(Result::ok)
        .filter_map(|t| t.o().lexical_form().map(|l| l.to_string()))
        .collect()
}

/// Collect every IRI object for a (subject, predicate) pair.
fn collect_iri_values<T: Term>(
    graph: &FastGraph,
    subject: &SimpleTerm,
    predicate: T,
) -> Vec<String> {
    graph
        .triples_matching([subject], [predicate], Any)
        .filter_map(Result::ok)
        .filter_map(|t| t.o().iri().map(|i| i.to_string()))
        .collect()
}

/// True when `subject owl:deprecated true` is asserted.
fn read_deprecated(graph: &FastGraph, subject: &SimpleTerm, owl: &Namespace<&str>) -> bool {
    let Ok(owl_deprecated) = owl.get("deprecated") else {
        return false;
    };
    graph
        .triples_matching([subject], [owl_deprecated], Any)
        .filter_map(Result::ok)
        .any(|t| {
            t.o()
                .lexical_form()
                .is_some_and(|l| l == "true" || l == "1")
        })
}

/// Read the SKOS / editorial cross-references the writer emits onto a
/// class or property IRI: `owl:deprecated`, `skos:altLabel`,
/// `rdfs:seeAlso`, and the five SKOS mapping predicates.
fn read_annotations(graph: &FastGraph, subject: &SimpleTerm, owl: &Namespace<&str>) -> Annotations {
    let skos = Namespace::new_unchecked(SKOS_NS);
    let mapping_iris = |name: &str| {
        skos.get(name)
            .map(|p| {
                let p: SimpleTerm = p.into_term();
                collect_iri_values(graph, subject, &p)
            })
            .unwrap_or_default()
    };
    let aliases = skos
        .get("altLabel")
        .map(|p| {
            let p: SimpleTerm = p.into_term();
            collect_literal_values(graph, subject, &p)
        })
        .unwrap_or_default();

    Annotations {
        deprecated: read_deprecated(graph, subject, owl),
        aliases,
        see_also: collect_iri_values(graph, subject, rdfs::seeAlso),
        exact_mappings: mapping_iris("exactMatch"),
        close_mappings: mapping_iris("closeMatch"),
        related_mappings: mapping_iris("relatedMatch"),
        narrow_mappings: mapping_iris("narrowMatch"),
        broad_mappings: mapping_iris("broadMatch"),
    }
}

/// Read the OWL relationship characteristics asserted on a property via
/// `rdf:type owl:<Name>Property`.
fn read_characteristics(
    graph: &FastGraph,
    subject: &SimpleTerm,
    owl: &Namespace<&str>,
) -> PropertyCharacteristics {
    let has_type = |name: &str| {
        owl.get(name)
            .map(|ty| {
                let ty: SimpleTerm = ty.into_term();
                graph
                    .triples_matching([subject], [rdf::type_], [&ty])
                    .filter_map(Result::ok)
                    .next()
                    .is_some()
            })
            .unwrap_or(false)
    };
    PropertyCharacteristics {
        symmetric: has_type("SymmetricProperty"),
        asymmetric: has_type("AsymmetricProperty"),
        reflexive: has_type("ReflexiveProperty"),
        irreflexive: has_type("IrreflexiveProperty"),
        transitive: has_type("TransitiveProperty"),
    }
}

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

        // The dependency parser can panic on malformed input (fuzzing found
        // a debug assertion in sophia_turtle 0.10 doing exactly that), and
        // this reader's contract is that a malformed file is a returned
        // error whoever's code chokes on it. The catch is confined to the
        // parse of one just-opened file, so no shared state can be observed
        // mid-panic.
        let graph: FastGraph = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            turtle::parse_bufread(reader).collect_triples()
        }))
        .map_err(|_| anyhow::anyhow!("Turtle parser crashed on malformed input"))??;

        let owl = Namespace::new_unchecked(OWL_NS);
        let owl_ontology = owl.get("Ontology")?;

        // Find the ontology IRI (subject of rdf:type owl:Ontology)
        let ontology_iri: SimpleTerm = graph
            .triples_matching(Any, [rdf::type_], [owl_ontology])
            .filter_map(Result::ok)
            .map(|t| t.s().into_term::<SimpleTerm>())
            .next()
            .ok_or_else(|| anyhow::anyhow!("No owl:Ontology found in {}", path.display()))?;

        // Extract the IRI string
        let iri = ontology_iri
            .iri()
            .ok_or_else(|| anyhow::anyhow!("Ontology subject is not an IRI"))?
            .to_string();

        // Helper to get a string literal for a predicate
        fn get_literal_value<T: Term>(
            graph: &FastGraph,
            subject: &SimpleTerm,
            predicate: T,
        ) -> Option<String> {
            graph
                .triples_matching([subject], [predicate], Any)
                .filter_map(Result::ok)
                .filter_map(|t| t.o().lexical_form().map(|l| l.to_string()))
                .next()
        }

        let owl_version_info = owl.get("versionInfo")?;

        let label = get_literal_value(&graph, &ontology_iri, rdfs::label);
        let comment = get_literal_value(&graph, &ontology_iri, rdfs::comment);
        let version = get_literal_value(&graph, &ontology_iri, owl_version_info);

        // Extract all owl:Class entities
        let owl_class = owl.get("Class")?;
        let owl_class_term: SimpleTerm = owl_class.into_term();
        let classes = Self::extract_classes(&graph, &owl_class_term, &owl)?;

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
            &owl,
        )?;

        // Extract all owl:NamedIndividual entities
        let owl_named_individual = owl.get("NamedIndividual")?;
        let owl_named_individual_term: SimpleTerm = owl_named_individual.into_term();
        let individuals = Self::extract_individuals(&graph, &owl_named_individual_term)?;

        // A class closed by `owl:oneOf` is an enum, not a class: pull it
        // out of the class list, claim its value individuals, and carry it
        // separately. Done after both extractions so the enum can take
        // labels/comments straight off what was already read.
        let (classes, individuals, enums) = Self::extract_enums(&graph, classes, individuals, &owl);

        Ok(OntologyMetadata {
            iri,
            label,
            comment,
            version,
            classes,
            properties,
            individuals,
            enums,
        })
    }

    /// Split `owl:oneOf`-closed classes out of `classes` as enums, claiming
    /// their value individuals out of `individuals`. Classes without a
    /// `oneOf` pass through untouched, as do individuals of real classes.
    fn extract_enums(
        graph: &FastGraph,
        classes: Vec<OntologyClass>,
        individuals: Vec<OntologyIndividual>,
        owl: &Namespace<&str>,
    ) -> (
        Vec<OntologyClass>,
        Vec<OntologyIndividual>,
        Vec<crate::owl_model::OntologyEnum>,
    ) {
        use sophia::api::ns::rdf;
        let one_of = owl.get("oneOf").expect("owl:oneOf is a valid IRI");

        // Walk an RDF collection from its head cell to rdf:nil, returning
        // member IRIs in list order. A well-formed list ends at nil; a
        // malformed graph can also form a `rdf:rest` cycle, so every cell
        // is remembered and a revisit ends the walk — a hostile input must
        // hang the reader no more than it extends an enum.
        let list_members = |head: SimpleTerm<'_>| -> Vec<String> {
            let mut members = Vec::new();
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            let mut cell = head;
            loop {
                if !seen.insert(format!("{cell:?}")) {
                    break;
                }
                let Some(first) = graph
                    .triples_matching([&cell], [rdf::first], Any)
                    .filter_map(Result::ok)
                    .filter_map(|t| t.o().iri().map(|i| i.to_string()))
                    .next()
                else {
                    break;
                };
                members.push(first);
                let next = graph
                    .triples_matching([&cell], [rdf::rest], Any)
                    .filter_map(Result::ok)
                    .map(|t| t.o().into_term::<SimpleTerm>())
                    .next();
                match next {
                    Some(n) if !Term::eq(&rdf::nil, n.borrow_term()) => cell = n,
                    _ => break,
                }
            }
            members
        };

        let mut enums = Vec::new();
        let mut enum_value_iris: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        let mut kept_classes = Vec::new();

        for class in classes {
            let class_term: SimpleTerm = sophia::api::term::IriRef::new_unchecked(
                sophia::api::MownStr::from(class.iri.as_str()),
            )
            .into_term();
            let head = graph
                .triples_matching([&class_term], [&one_of], Any)
                .filter_map(Result::ok)
                .map(|t| t.o().into_term::<SimpleTerm>())
                .next();
            let Some(head) = head else {
                kept_classes.push(class);
                continue;
            };
            let value_iris = list_members(head);
            let values = value_iris
                .iter()
                .map(|iri| {
                    // Labels/comments come from the already-extracted
                    // individuals when the value is one; an external
                    // `meaning:` IRI may not be, and stays bare.
                    let ind = individuals.iter().find(|i| &i.iri == iri);
                    crate::owl_model::OntologyEnumValue {
                        iri: iri.clone(),
                        label: ind.and_then(|i| i.label.clone()),
                        comment: ind.and_then(|i| i.comment.clone()),
                    }
                })
                .collect();
            enum_value_iris.extend(value_iris);
            enums.push(crate::owl_model::OntologyEnum {
                iri: class.iri,
                id: class.id,
                label: class.label,
                comment: class.comment,
                values,
            });
        }

        let kept_individuals = individuals
            .into_iter()
            .filter(|i| !enum_value_iris.contains(&i.iri))
            .collect();
        (kept_classes, kept_individuals, enums)
    }

    /// Extract all owl:Class entities from the graph
    fn extract_classes(
        graph: &FastGraph,
        owl_class: &SimpleTerm<'_>,
        owl: &Namespace<&str>,
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
                .filter_map(|t| t.o().lexical_form().map(|l| l.to_string()))
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
                .filter_map(|t| t.o().iri().map(|i| i.to_string()))
                .next()
        }

        // Find all subjects with rdf:type owl:Class
        let class_iris: Vec<SimpleTerm> = graph
            .triples_matching(Any, [rdf::type_], [owl_class])
            .filter_map(Result::ok)
            .map(|t| t.s().into_term::<SimpleTerm>())
            .collect();

        let mut classes = Vec::new();

        for class_iri in class_iris {
            // Skip blank nodes and non-IRI subjects
            let Some(iri) = class_iri.iri().map(|i| i.to_string()) else {
                continue;
            };

            // Skip built-in OWL classes
            if iri.starts_with(OWL_NS) {
                continue;
            }

            let id = extract_id_from_iri(&iri);
            let label = get_literal_value(graph, &class_iri, rdfs::label);
            let comment = get_literal_value(graph, &class_iri, rdfs::comment);
            let superclass_iri = get_iri_value(graph, &class_iri, rdfs::subClassOf);
            let annotations = read_annotations(graph, &class_iri, owl);

            classes.push(OntologyClass {
                iri,
                id,
                label,
                comment,
                superclass_iri,
                annotations,
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
        owl: &Namespace<&str>,
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
                .filter_map(|t| t.o().lexical_form().map(|l| l.to_string()))
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
                .filter_map(|t| t.o().iri().map(|i| i.to_string()))
                .next()
        }

        let mut properties = Vec::new();

        // Extract object properties
        let object_prop_iris: Vec<SimpleTerm> = graph
            .triples_matching(Any, [rdf::type_], [owl_object_property])
            .filter_map(Result::ok)
            .map(|t| t.s().into_term::<SimpleTerm>())
            .collect();

        for prop_iri in object_prop_iris {
            let Some(iri) = prop_iri.iri().map(|i| i.to_string()) else {
                continue;
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
            let characteristics = read_characteristics(graph, &prop_iri, owl);
            let annotations = read_annotations(graph, &prop_iri, owl);

            properties.push(OntologyProperty {
                iri,
                id,
                label,
                comment,
                property_type: PropertyType::ObjectProperty,
                domain_iri,
                range_iri,
                inverse_of_iri,
                characteristics,
                annotations,
            });
        }

        // Extract datatype properties
        let datatype_prop_iris: Vec<SimpleTerm> = graph
            .triples_matching(Any, [rdf::type_], [owl_datatype_property])
            .filter_map(Result::ok)
            .map(|t| t.s().into_term::<SimpleTerm>())
            .collect();

        for prop_iri in datatype_prop_iris {
            let Some(iri) = prop_iri.iri().map(|i| i.to_string()) else {
                continue;
            };
            if iri.starts_with(OWL_NS) {
                continue;
            }

            let id = extract_id_from_iri(&iri);
            let label = get_literal_value(graph, &prop_iri, rdfs::label);
            let comment = get_literal_value(graph, &prop_iri, rdfs::comment);
            let domain_iri = get_iri_value(graph, &prop_iri, rdfs::domain);
            let range_iri = get_iri_value(graph, &prop_iri, rdfs::range);
            let characteristics = read_characteristics(graph, &prop_iri, owl);
            let annotations = read_annotations(graph, &prop_iri, owl);

            properties.push(OntologyProperty {
                iri,
                id,
                label,
                comment,
                property_type: PropertyType::DatatypeProperty,
                domain_iri,
                range_iri,
                inverse_of_iri: None,
                characteristics,
                annotations,
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
                .filter_map(|t| t.o().lexical_form().map(|l| l.to_string()))
                .next()
        }

        // RDF/RDFS/OWL namespace prefixes for filtering metadata predicates
        const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
        const RDFS_NS: &str = "http://www.w3.org/2000/01/rdf-schema#";

        // Find all subjects with rdf:type owl:NamedIndividual
        let ind_iris: Vec<SimpleTerm> = graph
            .triples_matching(Any, [rdf::type_], [owl_named_individual])
            .filter_map(Result::ok)
            .map(|t| t.s().into_term::<SimpleTerm>())
            .collect();

        let mut individuals = Vec::new();

        for ind_iri in ind_iris {
            // Skip blank nodes and non-IRI subjects
            let Some(iri) = ind_iri.iri().map(|i| i.to_string()) else {
                continue;
            };

            let id = extract_id_from_iri(&iri);
            let label = get_literal_value(graph, &ind_iri, rdfs::label);
            let comment = get_literal_value(graph, &ind_iri, rdfs::comment);

            // Extract type IRIs (excluding owl:NamedIndividual and OWL built-ins)
            let type_iris: Vec<String> = graph
                .triples_matching([&ind_iri], [rdf::type_], Any)
                .filter_map(Result::ok)
                .filter_map(|t| {
                    t.o().iri().map(|i| i.to_string()).filter(|iri_str| {
                        !iri_str.starts_with(OWL_NS) && !iri_str.starts_with(RDF_NS)
                    })
                })
                .collect();

            // Extract property values (all triples except metadata predicates)
            let mut property_values: Vec<PropertyValue> = graph
                .triples_matching([&ind_iri], Any, Any)
                .filter_map(Result::ok)
                .filter_map(|t| {
                    // Get the predicate IRI
                    let pred_iri = t.p().iri()?.to_string();

                    // Skip metadata predicates
                    if pred_iri.starts_with(RDF_NS) || pred_iri.starts_with(RDFS_NS) {
                        return None;
                    }

                    // Get the object value as a string: literal lexical form
                    // takes precedence; bare IRIs fall back to their string
                    // form. Anything else (blank nodes, triple terms, …) is
                    // skipped because there's no useful string projection.
                    let value = t
                        .o()
                        .lexical_form()
                        .map(|l| l.to_string())
                        .or_else(|| t.o().iri().map(|i| i.to_string()))?;

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
        // Enum IRIs resolve as slot ranges by name, exactly like classes.
        let enum_iris: std::collections::HashSet<_> =
            metadata.enums.iter().map(|e| e.iri.as_str()).collect();

        // Map enums: each `owl:oneOf` member becomes a permissible value.
        // The key is the tail of the value IRI under the enum's own
        // namespace (`{enum}/{key}`, the derived form the writer mints); a
        // member outside that namespace is a `meaning:` IRI, keyed by its
        // label, with the IRI preserved as the value's meaning.
        for owl_enum in &metadata.enums {
            let mut enum_def = crate::linkml::EnumDefinition::new(&owl_enum.id);
            enum_def.description = owl_enum.comment.clone();
            let derived_prefix = format!("{}/", owl_enum.iri);
            for value in &owl_enum.values {
                let (key, meaning) = match value.iri.strip_prefix(&derived_prefix) {
                    Some(tail) => (tail.to_string(), None),
                    None => (
                        value
                            .label
                            .clone()
                            .unwrap_or_else(|| extract_id_from_iri(&value.iri)),
                        Some(value.iri.clone()),
                    ),
                };
                enum_def.permissible_values.insert(
                    key.clone(),
                    crate::linkml::PermissibleValue {
                        text: value.label.clone().unwrap_or(key),
                        description: value.comment.clone(),
                        meaning,
                    },
                );
            }
            schema.enums.insert(owl_enum.id.clone(), enum_def);
        }

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

            // SKOS / editorial cross-references.
            let ann = &owl_class.annotations;
            if ann.deprecated {
                class_def.deprecated = Some(String::new());
            }
            class_def.aliases = ann.aliases.clone();
            class_def.see_also = ann.see_also.clone();
            class_def.exact_mappings = ann.exact_mappings.clone();
            class_def.close_mappings = ann.close_mappings.clone();
            class_def.related_mappings = ann.related_mappings.clone();
            class_def.narrow_mappings = ann.narrow_mappings.clone();
            class_def.broad_mappings = ann.broad_mappings.clone();

            // Store label in annotations if different from id
            if let Some(ref label) = owl_class.label
                && label != &owl_class.id
            {
                class_def
                    .annotations
                    .insert("panschema:label".to_string(), label.clone());
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

                // If range is a known class or enum, use its name
                // Otherwise, it's probably an XSD datatype
                if class_iris.contains(range_iri.as_str()) || enum_iris.contains(range_iri.as_str())
                {
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

            // OWL relationship characteristics.
            let chars = &owl_prop.characteristics;
            slot_def.symmetric = chars.symmetric;
            slot_def.asymmetric = chars.asymmetric;
            slot_def.reflexive = chars.reflexive;
            slot_def.irreflexive = chars.irreflexive;
            slot_def.transitive = chars.transitive;

            // SKOS / editorial cross-references.
            let ann = &owl_prop.annotations;
            if ann.deprecated {
                slot_def.deprecated = Some(String::new());
            }
            slot_def.aliases = ann.aliases.clone();
            slot_def.see_also = ann.see_also.clone();
            slot_def.exact_mappings = ann.exact_mappings.clone();
            slot_def.close_mappings = ann.close_mappings.clone();
            slot_def.related_mappings = ann.related_mappings.clone();
            slot_def.narrow_mappings = ann.narrow_mappings.clone();
            slot_def.broad_mappings = ann.broad_mappings.clone();

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
            if let Some(ref label) = owl_prop.label
                && label != &owl_prop.id
            {
                slot_def
                    .annotations
                    .insert("panschema:label".to_string(), label.clone());
            }

            // `rdfs:domain` states which class carries the property, so the
            // class must list it. Without this the slot exists only at
            // schema level and the shared resolver — which every writer
            // asks for a class's effective slots — reports the class as
            // having none, so an ontology that plainly declares fields
            // projects to empty structs, empty tables, and no shapes.
            //
            // A property with no domain belongs to no class in particular
            // and stays schema-level, exactly as before.
            if let Some(domain) = &slot_def.domain
                && let Some(class) = schema.classes.get_mut(domain)
                && !class.slots.contains(&owl_prop.id)
            {
                class.slots.push(owl_prop.id.clone());
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

    /// A malformed Turtle file the dependency parser panics on (a debug
    /// assertion in sophia_turtle 0.10, found by fuzzing) must come back
    /// as an error like any other malformed file — the reader's contract
    /// does not care whose code the panic is in.
    #[test]
    fn a_parser_panicking_input_returns_an_error_not_a_crash() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("turtle_parser_panic.ttl");
        let result = OwlReader::new().read(&fixture);
        assert!(
            result.is_err(),
            "a malformed file is a returned error, never a panic"
        );
    }

    /// Write a TTL string to a temp file and read it back through the
    /// full reader path.
    fn read_ttl(ttl: &str) -> SchemaDefinition {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("schema.ttl");
        std::fs::write(&path, ttl).expect("write ttl");
        OwlReader::new().read(&path).expect("read ttl")
    }

    /// An `owl:Class` closed by `owl:oneOf` over named individuals is
    /// LinkML's enum, and reads back as one: an `EnumDefinition` carrying
    /// each value's key, label, and description — not a class. This is the
    /// shape panschema's own Turtle writer emits, so it is also what makes
    /// the enum half of a round-trip hold.
    #[test]
    fn a_one_of_closed_class_reads_back_as_an_enum() {
        let schema = read_ttl(
            r#"@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix ex: <https://example.org/s#> .

<https://example.org/s> a owl:Ontology .

ex:Status a owl:Class ;
  rdfs:label "Status" ;
  rdfs:comment "The lifecycle status." ;
  owl:oneOf ( <https://example.org/s#Status/open> <https://example.org/s#Status/closed> ) .

<https://example.org/s#Status/open> a owl:NamedIndividual, ex:Status ;
  rdfs:label "open" ;
  rdfs:comment "Still going." .

<https://example.org/s#Status/closed> a owl:NamedIndividual, ex:Status ;
  rdfs:label "closed" .

ex:Ticket a owl:Class ;
  rdfs:label "Ticket" .

ex:status a owl:ObjectProperty ;
  rdfs:domain ex:Ticket ;
  rdfs:range ex:Status .
"#,
        );

        let status = schema
            .enums
            .get("Status")
            .expect("Status should read back as an enum");
        assert_eq!(status.description.as_deref(), Some("The lifecycle status."));
        let open = status
            .permissible_values
            .get("open")
            .expect("value `open` present");
        assert_eq!(open.text, "open");
        assert_eq!(open.description.as_deref(), Some("Still going."));
        assert!(status.permissible_values.contains_key("closed"));

        assert!(
            !schema.classes.contains_key("Status"),
            "a oneOf-closed class is an enum, not a class; classes: {:?}",
            schema.classes.keys().collect::<Vec<_>>()
        );
        let ticket = schema.classes.get("Ticket").expect("Ticket stays a class");
        let effective = crate::linkml_resolve::resolve_effective_slots(ticket, &schema);
        assert_eq!(
            effective.get("status").and_then(|s| s.range.clone()),
            Some("Status".to_string()),
            "the enum-ranged slot's range should resolve to the enum by name"
        );
    }

    /// The `owl:oneOf` walk stops at `rdf:nil` because nil *is* the list
    /// terminator — not merely because nil happens to carry no
    /// `rdf:first`. A malformed graph that asserts triples about `rdf:nil`
    /// must not smuggle an extra value into every enum in the file.
    #[test]
    fn a_graph_asserting_triples_about_nil_cannot_extend_an_enum() {
        let schema = read_ttl(
            r#"@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix ex: <https://example.org/s#> .

<https://example.org/s> a owl:Ontology .

ex:Color a owl:Class ;
  rdfs:label "Color" ;
  owl:oneOf ( <https://example.org/s#Color/red> ) .

<https://example.org/s#Color/red> a owl:NamedIndividual, ex:Color ;
  rdfs:label "red" .

# Hostile / malformed: rdf:nil is not a list cell, whatever this claims.
rdf:nil rdf:first <https://example.org/s#Color/smuggled> ;
  rdf:rest rdf:nil .
"#,
        );
        let color = schema.enums.get("Color").expect("Color enum");
        assert_eq!(
            color.permissible_values.keys().collect::<Vec<_>>(),
            vec!["red"],
            "the walk must stop at rdf:nil, not read values off it"
        );
    }

    /// An enum whose local name collides with an XSD alias is still an
    /// enum reference, not a datatype. `int` maps to `integer` under the
    /// XSD fallback, so if the known-enum check is skipped the range
    /// dangles.
    #[test]
    fn an_enum_named_like_an_xsd_alias_still_resolves_as_the_enum() {
        let schema = read_ttl(
            r#"@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix ex: <https://example.org/s#> .

<https://example.org/s> a owl:Ontology .

ex:int a owl:Class ;
  rdfs:label "int" ;
  owl:oneOf ( <https://example.org/s#int/one> ) .

<https://example.org/s#int/one> a owl:NamedIndividual, ex:int ;
  rdfs:label "one" .

ex:Ticket a owl:Class ; rdfs:label "Ticket" .

ex:kind a owl:ObjectProperty ;
  rdfs:domain ex:Ticket ;
  rdfs:range ex:int .
"#,
        );
        assert!(schema.enums.contains_key("int"), "the enum reads back");
        let ticket = schema.classes.get("Ticket").expect("Ticket class");
        let effective = crate::linkml_resolve::resolve_effective_slots(ticket, &schema);
        assert_eq!(
            effective.get("kind").and_then(|s| s.range.clone()),
            Some("int".to_string()),
            "the range must name the enum, not be rewritten to `integer` by \
             the XSD fallback"
        );
    }

    /// A malformed graph can close an `rdf:rest` cycle that never reaches
    /// nil. The walk must terminate anyway — a hostile input gets to be
    /// wrong, not to hang the reader.
    #[test]
    fn a_cyclic_one_of_list_terminates_instead_of_hanging() {
        let schema = read_ttl(
            r#"@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix ex: <https://example.org/s#> .

<https://example.org/s> a owl:Ontology .

ex:Color a owl:Class ;
  rdfs:label "Color" ;
  owl:oneOf ex:cell1 .

ex:cell1 rdf:first <https://example.org/s#Color/red> ; rdf:rest ex:cell2 .
ex:cell2 rdf:first <https://example.org/s#Color/blue> ; rdf:rest ex:cell1 .

<https://example.org/s#Color/red> a owl:NamedIndividual, ex:Color ;
  rdfs:label "red" .
<https://example.org/s#Color/blue> a owl:NamedIndividual, ex:Color ;
  rdfs:label "blue" .
"#,
        );
        let color = schema.enums.get("Color").expect("Color enum");
        assert_eq!(
            color.permissible_values.keys().collect::<Vec<_>>(),
            vec!["blue", "red"],
            "each member is read once and the cycle ends the walk"
        );
    }

    /// The enum's value individuals belong to the enum; they must not also
    /// surface as instance individuals, or every enum ships ghost records.
    #[test]
    fn enum_value_individuals_are_not_instance_individuals() {
        let schema = read_ttl(
            r#"@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix ex: <https://example.org/s#> .

<https://example.org/s> a owl:Ontology .

ex:Color a owl:Class ;
  rdfs:label "Color" ;
  owl:oneOf ( <https://example.org/s#Color/red> ) .

<https://example.org/s#Color/red> a owl:NamedIndividual, ex:Color ;
  rdfs:label "red" .

ex:Wine a owl:Class ; rdfs:label "Wine" .
ex:morgon a owl:NamedIndividual, ex:Wine ; rdfs:label "Morgon" .
"#,
        );

        assert!(schema.enums.contains_key("Color"));
        let individuals = schema
            .annotations
            .get("panschema:individuals")
            .cloned()
            .unwrap_or_default();
        assert!(
            individuals.contains("morgon"),
            "a real instance individual survives; got: {individuals}"
        );
        assert!(
            !individuals.contains("red"),
            "an enum value must not double as an instance individual; got: {individuals}"
        );
    }

    /// A property's `rdfs:domain` states which class carries it, so the
    /// class must come back owning that slot. Every writer that asks the
    /// shared resolver "what slots does this class have?" — Rust, Postgres,
    /// SHACL, JSON Schema — sees nothing otherwise, and emits a class with
    /// no fields from an ontology that plainly declares them.
    #[test]
    fn a_property_domain_makes_the_class_own_the_slot() {
        let schema = OwlReader::new()
            .read(&reference_ontology_path())
            .expect("read reference ontology");

        // `Animal` is the declared domain of hasOwner, hasAge, and relatedTo.
        let animal = schema.classes.get("Animal").expect("Animal class");
        let effective = crate::linkml_resolve::resolve_effective_slots(animal, &schema);
        for slot in ["hasOwner", "hasAge", "relatedTo"] {
            assert!(
                effective.contains_key(slot),
                "`{slot}` names Animal as its rdfs:domain, so the resolver must \
                 see it on Animal; got: {:?}",
                effective.keys().collect::<Vec<_>>()
            );
        }

        // Subclasses inherit it through `is_a`, so the whole hierarchy gains
        // the slots rather than only the declared domain.
        let dog = schema.classes.get("Dog").expect("Dog class");
        let inherited = crate::linkml_resolve::resolve_effective_slots(dog, &schema);
        assert!(
            inherited.contains_key("hasAge"),
            "Dog is a subclass of Animal, so it inherits Animal's slots; got: {:?}",
            inherited.keys().collect::<Vec<_>>()
        );
    }

    /// A property with no `rdfs:domain` belongs to no class in particular.
    /// It stays a schema-level slot rather than being attached to an
    /// arbitrary one — the fix for the domain case must not over-reach.
    #[test]
    fn a_property_without_a_domain_is_attached_to_no_class() {
        let schema = OwlReader::new()
            .read(&reference_ontology_path())
            .expect("read reference ontology");

        let undomained: Vec<&String> = schema
            .slots
            .iter()
            .filter(|(_, s)| s.domain.is_none())
            .map(|(name, _)| name)
            .collect();
        assert!(
            !undomained.is_empty(),
            "the fixture should carry at least one domain-less property"
        );
        for (class_name, class) in &schema.classes {
            for slot in &undomained {
                assert!(
                    !class.slots.contains(slot),
                    "`{slot}` has no rdfs:domain, so class `{class_name}` must \
                     not claim it"
                );
            }
        }
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

        // Reference ontology has 6 classes: Animal, Cat, Dog, Mammal, Person, Pet
        assert_eq!(meta.classes.len(), 6);

        // Classes should be sorted alphabetically by display label
        let class_labels: Vec<&str> = meta.classes.iter().map(|c| c.display_label()).collect();
        assert_eq!(
            class_labels,
            vec!["Animal", "Cat", "Dog", "Mammal", "Person", "Pet"]
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

        // Reference ontology has 5 properties: hasAge, hasName, hasOwner, owns, relatedTo
        assert_eq!(meta.properties.len(), 5);

        // Properties should be sorted alphabetically by display label
        let prop_labels: Vec<&str> = meta.properties.iter().map(|p| p.display_label()).collect();
        assert_eq!(
            prop_labels,
            vec!["has age", "has name", "has owner", "owns", "related to"]
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
    fn extracts_owl_deprecated_on_class() {
        let path = reference_ontology_path();
        let meta = OwlReader::parse_ontology(&path).expect("Failed to parse reference ontology");

        // `:Pet owl:deprecated true` is read into the class annotations.
        let pet = meta.classes.iter().find(|c| c.id == "Pet").unwrap();
        assert!(pet.annotations.deprecated);

        // An undeprecated class carries no deprecated flag.
        let dog = meta.classes.iter().find(|c| c.id == "Dog").unwrap();
        assert!(!dog.annotations.deprecated);
    }

    #[test]
    fn reads_deprecated_from_xsd_boolean_one_lexical() {
        // `owl:deprecated` accepts `"1"^^xsd:boolean` as a valid lexical
        // for `true`, not just the canonical `true`. The reader treats
        // both as deprecated; a `"0"` value is not deprecated.
        let ttl = concat!(
            "@prefix owl: <http://www.w3.org/2002/07/owl#> .\n",
            "@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n",
            "@prefix ex: <http://example.org/> .\n",
            "ex:Ont a owl:Ontology .\n",
            "ex:Legacy a owl:Class ; owl:deprecated \"1\"^^xsd:boolean .\n",
            "ex:Live a owl:Class ; owl:deprecated \"0\"^^xsd:boolean .\n",
        );
        let path =
            std::env::temp_dir().join(format!("owl_reader_dep_one_{}.ttl", std::process::id()));
        std::fs::write(&path, ttl).unwrap();
        let meta = OwlReader::parse_ontology(&path).expect("parse inline ontology");
        let _ = std::fs::remove_file(&path);

        let legacy = meta.classes.iter().find(|c| c.id == "Legacy").unwrap();
        assert!(
            legacy.annotations.deprecated,
            "`\"1\"^^xsd:boolean` must read as deprecated"
        );
        let live = meta.classes.iter().find(|c| c.id == "Live").unwrap();
        assert!(
            !live.annotations.deprecated,
            "`\"0\"^^xsd:boolean` must not read as deprecated"
        );
    }

    #[test]
    fn extracts_alt_label_and_see_also_on_class() {
        let path = reference_ontology_path();
        let meta = OwlReader::parse_ontology(&path).expect("Failed to parse reference ontology");

        let person = meta.classes.iter().find(|c| c.id == "Person").unwrap();

        // skos:altLabel literals become aliases (order-independent).
        let mut aliases = person.annotations.aliases.clone();
        aliases.sort();
        assert_eq!(aliases, vec!["Human", "Individual"]);

        // rdfs:seeAlso IRIs become see_also references.
        assert_eq!(
            person.annotations.see_also,
            vec!["http://xmlns.com/foaf/0.1/Person"]
        );
    }

    #[test]
    fn extracts_skos_mappings_on_class() {
        let path = reference_ontology_path();
        let meta = OwlReader::parse_ontology(&path).expect("Failed to parse reference ontology");

        let person = meta.classes.iter().find(|c| c.id == "Person").unwrap();
        assert_eq!(
            person.annotations.exact_mappings,
            vec!["http://schema.org/Person"]
        );

        let cat = meta.classes.iter().find(|c| c.id == "Cat").unwrap();
        assert_eq!(
            cat.annotations.close_mappings,
            vec!["http://dbpedia.org/resource/Cat"]
        );
    }

    #[test]
    fn extracts_skos_mappings_on_property() {
        let path = reference_ontology_path();
        let meta = OwlReader::parse_ontology(&path).expect("Failed to parse reference ontology");

        let owns = meta.properties.iter().find(|p| p.id == "owns").unwrap();
        assert_eq!(
            owns.annotations.exact_mappings,
            vec!["http://purl.org/dc/terms/relation"]
        );
    }

    #[test]
    fn extracts_owl_characteristics_on_property() {
        let path = reference_ontology_path();
        let meta = OwlReader::parse_ontology(&path).expect("Failed to parse reference ontology");

        // `:relatedTo` is declared owl:SymmetricProperty and
        // owl:TransitiveProperty; the other characteristics stay unset.
        let related = meta
            .properties
            .iter()
            .find(|p| p.id == "relatedTo")
            .unwrap();
        assert!(related.characteristics.symmetric);
        assert!(related.characteristics.transitive);
        assert!(!related.characteristics.asymmetric);
        assert!(!related.characteristics.reflexive);
        assert!(!related.characteristics.irreflexive);

        // A plain object property carries no characteristics.
        let has_owner = meta.properties.iter().find(|p| p.id == "hasOwner").unwrap();
        assert!(!has_owner.characteristics.symmetric);
        assert!(!has_owner.characteristics.transitive);
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
    fn owl_reader_maps_deprecated_to_ir() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        // `owl:deprecated true` on a class sets the IR `deprecated` flag.
        // RDF carries only the boolean, so the note text is empty but
        // present (Some, not None).
        let pet = schema.classes.get("Pet").unwrap();
        assert!(pet.deprecated.is_some());

        let dog = schema.classes.get("Dog").unwrap();
        assert!(dog.deprecated.is_none());
    }

    #[test]
    fn owl_reader_maps_aliases_and_see_also_to_ir() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let person = schema.classes.get("Person").unwrap();
        let mut aliases = person.aliases.clone();
        aliases.sort();
        assert_eq!(aliases, vec!["Human", "Individual"]);
        assert_eq!(person.see_also, vec!["http://xmlns.com/foaf/0.1/Person"]);
    }

    #[test]
    fn owl_reader_maps_mappings_to_ir() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let person = schema.classes.get("Person").unwrap();
        assert_eq!(person.exact_mappings, vec!["http://schema.org/Person"]);

        let owns = schema.slots.get("owns").unwrap();
        assert_eq!(
            owns.exact_mappings,
            vec!["http://purl.org/dc/terms/relation"]
        );
    }

    #[test]
    fn owl_reader_maps_characteristics_to_ir() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let related = schema.slots.get("relatedTo").unwrap();
        assert!(related.symmetric);
        assert!(related.transitive);
        assert!(!related.reflexive);

        let owns = schema.slots.get("owns").unwrap();
        assert!(!owns.symmetric);
        assert!(!owns.transitive);
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

        // Pin down every match arm so deleting any of them is caught.
        // The fallback `_` arm would silently substitute the original
        // XSD name (e.g. "dateTime" instead of "datetime"), which is
        // observably wrong downstream.
        assert_eq!(OwlReader::map_xsd_to_linkml("string"), "string");
        assert_eq!(OwlReader::map_xsd_to_linkml("integer"), "integer");
        assert_eq!(OwlReader::map_xsd_to_linkml("long"), "integer");
        assert_eq!(OwlReader::map_xsd_to_linkml("short"), "integer");
        assert_eq!(OwlReader::map_xsd_to_linkml("byte"), "integer");
        assert_eq!(OwlReader::map_xsd_to_linkml("double"), "float");
        assert_eq!(OwlReader::map_xsd_to_linkml("decimal"), "float");
        assert_eq!(OwlReader::map_xsd_to_linkml("date"), "date");
        // dateTime — capitalisation differs from LinkML's `datetime`.
        assert_eq!(OwlReader::map_xsd_to_linkml("dateTime"), "datetime");
        assert_eq!(OwlReader::map_xsd_to_linkml("time"), "time");
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
