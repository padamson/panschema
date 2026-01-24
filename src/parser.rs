use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use sophia::api::graph::Graph;
use sophia::api::ns::{Namespace, rdf, rdfs};
use sophia::api::prelude::*;
use sophia::api::term::SimpleTerm;
use sophia::inmem::graph::FastGraph;
use sophia::turtle::parser::turtle;

use crate::model::OntologyMetadata;

/// OWL namespace
const OWL_NS: &str = "http://www.w3.org/2002/07/owl#";

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

    Ok(OntologyMetadata {
        iri,
        label,
        comment,
        version,
    })
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
}
