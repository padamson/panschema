//! One element, one IRI. The model crate mints it, the RDF output names the
//! individual or class by it, the graph node carries it, and a scoped
//! record's IRI sits under its scope.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use panschema::graph_writer::{GraphWriter, NodeType};
use panschema::io::{Reader, Writer};
use panschema::linkml_resolve::class_iri_by_name;
use panschema::rdf_serializers::NTriplesWriter;
use panschema::yaml_reader::YamlReader;
use panschema_model::instances::{InstanceSet, instance_iri_string};

const RDF_TYPE: &str = "<http://www.w3.org/1999/02/22-rdf-syntax-ns#type>";
const OWL_CLASS: &str = "http://www.w3.org/2002/07/owl#Class";

/// The subjects typed, by `rdf:type`, as one of `objects`.
fn subjects_typed_as(ntriples: &str, objects: &BTreeSet<String>) -> BTreeSet<String> {
    ntriples
        .lines()
        .filter_map(|line| {
            let (subject, rest) = line.strip_prefix('<')?.split_once('>')?;
            let rest = rest.trim_start().strip_prefix(RDF_TYPE)?;
            let (object, _) = rest.trim_start().strip_prefix('<')?.split_once('>')?;
            objects.contains(object).then(|| subject.to_string())
        })
        .collect()
}

#[test]
fn every_record_and_class_has_one_iri_across_the_model_the_rdf_and_the_graph() {
    let schema = YamlReader::new()
        .read(Path::new("tests/fixtures/shared_id_scoping.yaml"))
        .expect("the scoped schema parses");
    let data: serde_norway::Value = serde_norway::from_str(
        &fs::read_to_string("tests/fixtures/shared_id_scoping_data.yaml").expect("read data"),
    )
    .expect("the dataset parses");
    let set = InstanceSet::from_linkml_data(&schema, &data);
    assert!(!set.instances.is_empty(), "the dataset has records");

    let classes: BTreeSet<String> = schema
        .classes
        .keys()
        .map(|name| class_iri_by_name(name, &schema))
        .collect();
    assert_eq!(
        classes.len(),
        schema.classes.len(),
        "each class mints a distinct IRI"
    );
    let minted: BTreeSet<String> = set
        .instances
        .iter()
        .map(|inst| instance_iri_string(&schema, inst))
        .collect();
    assert_eq!(
        minted.len(),
        set.instances.len(),
        "each record mints a distinct IRI"
    );

    let scoped: Vec<_> = set.instances.iter().filter(|i| i.scope.is_some()).collect();
    assert!(!scoped.is_empty(), "the fixture scopes at least one record");
    for inst in scoped {
        let scope = inst.scope.as_deref().expect("scoped");
        let iri = instance_iri_string(&schema, inst);
        assert!(
            iri.starts_with(&format!("{scope}/")),
            "a scoped record's IRI sits under its scope: {iri} under {scope}"
        );
    }

    let schema_graph = GraphWriter::new().schema_to_graph(&schema);
    let instance_graph = GraphWriter::new().instance_set_to_graph(&schema, &set);
    let record_nodes: BTreeSet<String> = instance_graph
        .nodes
        .iter()
        .filter(|node| matches!(node.node_type, NodeType::Individual))
        .filter_map(|node| node.uri.clone())
        .collect();
    assert_eq!(
        record_nodes, minted,
        "the instance graph's nodes carry the minted IRI"
    );
    // A class node carries a URI only when the schema declares one; where it
    // does, it is the minted IRI.
    for node in schema_graph
        .nodes
        .iter()
        .filter(|n| matches!(n.node_type, NodeType::Class))
    {
        if let Some(uri) = &node.uri {
            assert!(
                classes.contains(uri),
                "a class node's URI is the minted IRI: {uri}"
            );
        }
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let rdf_path = dir.path().join("graph.nt");
    NTriplesWriter::new()
        .with_instances(set)
        .write(&schema, &rdf_path)
        .expect("write the RDF");
    let rdf = fs::read_to_string(&rdf_path).expect("read the RDF");
    let owl_class = BTreeSet::from([OWL_CLASS.to_string()]);
    assert_eq!(
        subjects_typed_as(&rdf, &owl_class),
        classes,
        "the T-box declares each class by the minted IRI"
    );
    assert_eq!(
        subjects_typed_as(&rdf, &classes),
        minted,
        "the A-box types each record, by the minted IRI, as one of the schema's classes"
    );
}
