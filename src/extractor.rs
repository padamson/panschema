use crate::generator::{Entity, IndexTemplate};
use anyhow::Result;
use oxigraph::model::Term;
use oxigraph::sparql::QueryResults;
use oxigraph::store::Store;

// Vocabulary Constants (Candidates in order of preference)
const OWL_ONTOLOGY: &str = "http://www.w3.org/2002/07/owl#Ontology";
const OWL_CLASS: &str = "http://www.w3.org/2002/07/owl#Class";
const OWL_OBJECT_PROPERTY: &str = "http://www.w3.org/2002/07/owl#ObjectProperty";
const OWL_DATATYPE_PROPERTY: &str = "http://www.w3.org/2002/07/owl#DatatypeProperty";

const OWL_ANNOTATION_PROPERTY: &str = "http://www.w3.org/2002/07/owl#AnnotationProperty";
const OWL_NAMED_INDIVIDUAL: &str = "http://www.w3.org/2002/07/owl#NamedIndividual";
const OWL_DISJOINT_WITH: &str = "http://www.w3.org/2002/07/owl#disjointWith";

const RDFS_SUBCLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
const RDFS_LABEL: &str = "http://www.w3.org/2000/01/rdf-schema#label";

const CANDIDATES_TITLE: &[&str] = &[
    "http://purl.org/dc/elements/1.1/title",
    "http://purl.org/dc/terms/title",
    "http://www.w3.org/2000/01/rdf-schema#label",
    "http://www.w3.org/2004/02/skos/core#prefLabel",
];

const CANDIDATES_DESCRIPTION: &[&str] = &[
    "http://purl.org/dc/elements/1.1/description",
    "http://purl.org/dc/terms/description",
    "http://www.w3.org/2000/01/rdf-schema#comment",
];

const CANDIDATES_VERSION: &[&str] = &[
    "http://www.w3.org/2002/07/owl#versionInfo",
    "http://purl.org/dc/terms/modified",
    "http://purl.org/dc/elements/1.1/date",
];

// Candidates for Entity Label (Classes, Properties)
const CANDIDATES_LABEL: &[&str] = &[
    "http://www.w3.org/2000/01/rdf-schema#label",
    "http://www.w3.org/2004/02/skos/core#prefLabel",
    "http://purl.org/dc/elements/1.1/title",
];

// Candidates for Entity Comment
const CANDIDATES_COMMENT: &[&str] = &[
    "http://www.w3.org/2000/01/rdf-schema#comment",
    "http://www.w3.org/2004/02/skos/core#definition",
    "http://purl.org/dc/elements/1.1/description",
];

/// Extracts metadata from the ontology store.
///
/// # Errors
/// Returns an error if the SPARQL queries fail.
pub fn extract_metadata(store: &Store, prefixes: Vec<(String, String)>) -> Result<IndexTemplate> {
    // 1. Find the Ontology IRI (Subject)
    let ontology_iri_query = format!("SELECT ?o WHERE {{ ?o a <{OWL_ONTOLOGY}> }} LIMIT 1");

    // Default values
    let mut title = "Untitled Ontology".to_string();
    let mut version = "0.0.0".to_string();
    let mut description = String::new();

    if let QueryResults::Solutions(solutions) = store.query(&ontology_iri_query)? {
        if let Some(solution) = solutions.into_iter().next() {
            let solution = solution?;
            if let Some(ontology_node) = solution.get("o") {
                let subject = ontology_node.to_string(); // Uses <IRI> format

                // 2. Build Dynamic Metadata Query
                let build_parts = |base: &str, candidates: &[&str]| -> (String, String) {
                    use std::fmt::Write;
                    let mut parts = String::new();
                    let mut vars = Vec::new();
                    for (i, uri) in candidates.iter().enumerate() {
                        let var = format!("?{base}_{i}");
                        let _ = write!(parts, "OPTIONAL {{ {subject} <{uri}> {var} }} . ");
                        vars.push(var);
                    }
                    // COALESCE(?v0, ?v1 ...)
                    let coalesce = format!("COALESCE({})", vars.join(", "));
                    (parts, coalesce)
                };

                let (title_part, title_expr) = build_parts("title", CANDIDATES_TITLE);
                let (desc_part, desc_expr) = build_parts("desc", CANDIDATES_DESCRIPTION);
                let (ver_part, ver_expr) = build_parts("ver", CANDIDATES_VERSION);

                let metadata_query = format!(
                    "
                    SELECT ({title_expr} AS ?title) ({ver_expr} AS ?version) ({desc_expr} AS ?desc)
                    WHERE {{
                        {title_part}
                        {desc_part}
                        {ver_part}
                    }} LIMIT 1
                "
                );

                if let QueryResults::Solutions(meta_solutions) = store.query(&metadata_query)? {
                    if let Some(meta_sol) = meta_solutions.into_iter().next() {
                        let meta_sol = meta_sol?;

                        if let Some(term) = meta_sol.get("title") {
                            title = extract_string(term);
                        }

                        if let Some(term) = meta_sol.get("version") {
                            version = extract_string(term);
                        }

                        if let Some(term) = meta_sol.get("desc") {
                            description = extract_string(term);
                        }
                    }
                }
            }
        }
    }

    // 3. Extract Entities
    let classes = extract_entities(store, OWL_CLASS)?;
    let object_properties = extract_entities(store, OWL_OBJECT_PROPERTY)?;
    let data_properties = extract_entities(store, OWL_DATATYPE_PROPERTY)?;
    let annotation_properties = extract_entities(store, OWL_ANNOTATION_PROPERTY)?;
    let named_individuals = extract_entities(store, OWL_NAMED_INDIVIDUAL)?;

    // Return template with extracted metadata
    Ok(IndexTemplate {
        title,
        version,
        description: description.clone(),
        abstract_text: description,
        namespaces: prefixes, // Use passed prefixes
        classes,
        object_properties,
        data_properties,
        annotation_properties,
        named_individuals,
    })
}

#[allow(clippy::option_if_let_else)]
fn extract_entities(store: &Store, type_iri: &str) -> Result<Vec<Entity>> {
    let (label_part, label_expr) = build_optional_parts("label", CANDIDATES_LABEL);
    let (comment_part, comment_expr) = build_optional_parts("comment", CANDIDATES_COMMENT);

    // Query: Select all subjects (?s) of the given type, and extract best label/comment
    let query = format!(
        "
        SELECT ?s ({label_expr} AS ?label) ({comment_expr} AS ?comment)
        WHERE {{
            ?s a <{type_iri}> .
            FILTER (isIRI(?s)) .
            {label_part}
            {comment_part}
        }}
        ORDER BY ?label
    "
    );

    let mut entities = Vec::new();
    if let QueryResults::Solutions(solutions) = store.query(&query)? {
        for solution in solutions {
            let solution = solution?;
            if let Some(s) = solution.get("s") {
                // Get IRI string
                let raw_iri = match s {
                    Term::NamedNode(n) => n.as_str().to_string(),
                    _ => continue,
                };

                // Extract Label
                let label = solution.get("label").map_or_else(
                    || {
                        if let Some(pos) = raw_iri.rfind('#') {
                            raw_iri[pos + 1..].to_string()
                        } else if let Some(pos) = raw_iri.rfind('/') {
                            raw_iri[pos + 1..].to_string()
                        } else {
                            raw_iri.clone()
                        }
                    },
                    extract_string,
                );

                // Extract Comment
                let comment = solution
                    .get("comment")
                    .map_or_else(String::new, extract_string);

                // Extract ID
                let mut id = "entity".to_string();
                if let Some(pos) = raw_iri.rfind('#') {
                    id = raw_iri[pos + 1..].to_string();
                } else if let Some(pos) = raw_iri.rfind('/') {
                    id = raw_iri[pos + 1..].to_string();
                }
                if id.is_empty() || id == "entity" {
                    id = raw_iri.replace(|c: char| !c.is_alphanumeric(), "_");
                }

                // --- NEW: Class Axioms (Slice 6) ---
                let (superclasses, disjoints) = if type_iri == OWL_CLASS {
                    extract_class_axioms(store, &raw_iri)?
                } else {
                    (Vec::new(), Vec::new())
                };

                entities.push(Entity {
                    id,
                    label,
                    iri: raw_iri,
                    comment,
                    superclasses,
                    disjoints,
                });
            }
        }
    }

    Ok(entities)
}

fn build_optional_parts(base: &str, candidates: &[&str]) -> (String, String) {
    use std::fmt::Write;
    let mut parts = String::new();
    let mut vars = Vec::new();
    for (i, uri) in candidates.iter().enumerate() {
        let var = format!("?{base}_{i}");
        // OPTIONAL { ?s <candidate> ?var }
        let _ = write!(parts, "OPTIONAL {{ ?s <{uri}> {var} }} . ");
        vars.push(var);
    }
    let coalesce = format!("COALESCE({})", vars.join(", "));
    (parts, coalesce)
}

fn extract_class_axioms(
    store: &Store,
    raw_iri: &str,
) -> Result<(
    Vec<crate::generator::TermRef>,
    Vec<crate::generator::TermRef>,
)> {
    let mut superclasses = Vec::new();
    // Get Superclasses
    let sc_query = format!(
        "
        SELECT ?parent ?plabel WHERE {{
            <{raw_iri}> <{RDFS_SUBCLASS_OF}> ?parent .
            FILTER(isIRI(?parent)) .
            OPTIONAL {{ ?parent <{RDFS_LABEL}> ?plabel }}
        }}
    "
    );
    if let QueryResults::Solutions(sc_sols) = store.query(&sc_query)? {
        for sc_sol in sc_sols {
            let sc_sol = sc_sol?;
            if let Some(p) = sc_sol.get("parent") {
                superclasses.push(term_to_ref(p, sc_sol.get("plabel")));
            }
        }
    }

    let mut disjoints = Vec::new();
    // Get Disjoints (Symmetric)
    let dj_query = format!(
        "
        SELECT ?dj ?dlabel WHERE {{
            {{ <{raw_iri}> <{OWL_DISJOINT_WITH}> ?dj }}
            UNION
            {{ ?dj <{OWL_DISJOINT_WITH}> <{raw_iri}> }}

            FILTER(isIRI(?dj)) .
            OPTIONAL {{ ?dj <{RDFS_LABEL}> ?dlabel }}
        }}
    "
    );
    if let QueryResults::Solutions(dj_sols) = store.query(&dj_query)? {
        for dj_sol in dj_sols {
            let dj_sol = dj_sol?;
            if let Some(d) = dj_sol.get("dj") {
                disjoints.push(term_to_ref(d, dj_sol.get("dlabel")));
            }
        }
    }

    Ok((superclasses, disjoints))
}

#[allow(clippy::option_if_let_else)]
fn term_to_ref(term: &Term, label_term: Option<&Term>) -> crate::generator::TermRef {
    let iri = match term {
        Term::NamedNode(n) => n.as_str().to_string(),
        _ => term.to_string(),
    };
    let label = label_term.map_or_else(
        || {
            if let Some(pos) = iri.rfind('#') {
                iri[pos + 1..].to_string()
            } else if let Some(pos) = iri.rfind('/') {
                iri[pos + 1..].to_string()
            } else {
                iri.clone()
            }
        },
        extract_string,
    );
    crate::generator::TermRef { iri, label }
}

fn extract_string(term: &Term) -> String {
    match term {
        Term::Literal(l) => l.value().to_string(),
        _ => term.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigraph::model::*;
    use oxigraph::store::Store;

    fn create_store() -> Store {
        #[allow(clippy::unwrap_used)]
        Store::new().unwrap()
    }

    fn insert_triple(store: &Store, s: &str, p: &str, o_lit: &str) {
        #[allow(clippy::unwrap_used)]
        let quad = Quad::new(
            NamedNode::new(s).unwrap(),
            NamedNode::new(p).unwrap(),
            Literal::new_simple_literal(o_lit),
            GraphName::DefaultGraph,
        );
        #[allow(clippy::unwrap_used)]
        store.insert(&quad).unwrap();
    }

    fn insert_type(store: &Store, s: &str, t: &str) {
        #[allow(clippy::unwrap_used)]
        let quad = Quad::new(
            NamedNode::new(s).unwrap(),
            NamedNode::new("http://www.w3.org/1999/02/22-rdf-syntax-ns#type").unwrap(),
            NamedNode::new(t).unwrap(),
            GraphName::DefaultGraph,
        );
        #[allow(clippy::unwrap_used)]
        store.insert(&quad).unwrap();
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    #[allow(clippy::expect_used)]
    fn test_extract_full_metadata() {
        let store = create_store();
        let ont_iri = "http://example.org/ont";

        insert_type(&store, ont_iri, super::OWL_ONTOLOGY);
        insert_triple(
            &store,
            ont_iri,
            "http://purl.org/dc/elements/1.1/title",
            "My Title",
        );
        insert_triple(
            &store,
            ont_iri,
            "http://www.w3.org/2002/07/owl#versionInfo",
            "1.2.3",
        );
        insert_triple(
            &store,
            ont_iri,
            "http://purl.org/dc/elements/1.1/description",
            "My Description",
        );

        let prefixes = vec![("ex".to_string(), "http://example.org/".to_string())];
        let template = extract_metadata(&store, prefixes).expect("Extraction failed");

        assert_eq!(template.title, "My Title");
        assert_eq!(template.version, "1.2.3");
        assert_eq!(template.description, "My Description");
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn test_extract_alternative_candidates() {
        let store = create_store();
        let ont_iri = "http://example.org/ont";

        insert_type(&store, ont_iri, super::OWL_ONTOLOGY);
        insert_triple(&store, ont_iri, super::RDFS_LABEL, "Label Title");
        insert_triple(
            &store,
            ont_iri,
            "http://purl.org/dc/elements/1.1/date",
            "2023-01-01",
        );

        let template = extract_metadata(&store, vec![]).expect("Extraction failed");

        assert_eq!(template.title, "Label Title");
        assert_eq!(template.version, "2023-01-01");
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn test_extract_minimal_metadata() {
        let store = create_store();
        insert_type(&store, "http://example.org/ont", super::OWL_ONTOLOGY);

        let template = extract_metadata(&store, vec![]).expect("Extraction failed");

        assert_eq!(template.title, "Untitled Ontology");
        assert_eq!(template.version, "0.0.0");
        assert!(template.namespaces.is_empty());
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    #[allow(clippy::expect_used)]
    fn test_extract_entities() {
        let store = create_store();
        let class1 = "http://example.org/Class1";
        let class2 = "http://example.org/Class2";

        insert_type(&store, class1, OWL_CLASS);
        insert_triple(&store, class1, super::RDFS_LABEL, "My Class 1");
        insert_triple(
            &store,
            class1,
            "http://www.w3.org/2000/01/rdf-schema#comment",
            "Description of Class 1",
        );

        insert_type(&store, class2, OWL_CLASS);
        // No label, should assume local name "Class2"

        let entities = extract_entities(&store, OWL_CLASS).expect("Failed to extract classes");

        assert_eq!(entities.len(), 2);

        // Check Class 1
        let c1 = entities.iter().find(|e| e.iri == class1).unwrap();
        assert_eq!(c1.label, "My Class 1");
        assert_eq!(c1.comment, "Description of Class 1");
        assert_eq!(c1.id, "Class1");

        // Check Class 2
        let c2 = entities.iter().find(|e| e.iri == class2).unwrap();
        assert_eq!(c2.label, "Class2");
        assert_eq!(c2.comment, "");
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn test_extract_advanced_entities() {
        let store = create_store();
        let ann_prop = "http://example.org/annProp";
        let individual = "http://example.org/individual1";

        insert_type(&store, ann_prop, OWL_ANNOTATION_PROPERTY);
        insert_triple(&store, ann_prop, super::RDFS_LABEL, "My Annotation Prop");

        insert_type(&store, individual, OWL_NAMED_INDIVIDUAL);
        insert_triple(&store, individual, super::RDFS_LABEL, "My Individual");

        // Extract directly using extract_metadata to test full flow
        let template = extract_metadata(&store, vec![]).expect("Extraction failed");

        // Check Annotation Property
        assert_eq!(template.annotation_properties.len(), 1);
        let ap = &template.annotation_properties[0];
        assert_eq!(ap.label, "My Annotation Prop");
        assert_eq!(ap.iri, ann_prop);

        // Check Named Individual
        assert_eq!(template.named_individuals.len(), 1);
        let ind = &template.named_individuals[0];
        assert_eq!(ind.label, "My Individual");
        assert_eq!(ind.iri, individual);
    }
}
