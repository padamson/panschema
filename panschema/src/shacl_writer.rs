//! SHACL Writer
//!
//! Writes a SHACL shapes graph (Turtle) from the LinkML IR: one
//! `sh:NodeShape` per class with `sh:property` shapes mirroring each slot's
//! value constraints. A separate artifact from the OWL/TTL output — a
//! validation shapes file a SHACL engine consumes — built from the same
//! class/property IRIs (see `rdf_serializers::build_shacl_graph`).

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use sophia::api::prefix::{Prefix, PrefixMapPair};
use sophia::api::serializer::TripleSerializer;
use sophia::iri::Iri;
use sophia::turtle::serializer::turtle::{TurtleConfig, TurtleSerializer};

use crate::io::{IoError, IoResult, Writer};
use crate::linkml::SchemaDefinition;
use crate::rdf_serializers::build_shacl_graph;

/// Writer for a SHACL shapes graph in Turtle (.ttl) format.
pub struct ShaclWriter;

impl ShaclWriter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ShaclWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer for ShaclWriter {
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()> {
        let graph = build_shacl_graph(schema)?;

        let file = File::create(output).map_err(IoError::Io)?;
        let writer = BufWriter::new(file);

        // Same prefix-aware Turtle config the OWL writer uses, plus a `sh:`
        // declaration so the shapes graph reads in compact form.
        let config = TurtleConfig::new()
            .with_pretty(true)
            .with_own_prefix_map(build_prefix_map(schema));
        let mut serializer = TurtleSerializer::new_with_config(writer, config);

        serializer
            .serialize_graph(&graph)
            .map_err(|e| IoError::Write(format!("Turtle serialization failed: {e}")))?;

        Ok(())
    }

    fn format_id(&self) -> &str {
        "shacl"
    }
}

/// The schema's `prefixes:` block plus the `sh:` (SHACL) and `xsd:`
/// declarations the shapes graph itself uses, as a sophia prefix map.
/// Entries that fail sophia's prefix/IRI validation are dropped with a
/// `tracing::warn!` (they can't appear in the output anyway).
fn build_prefix_map(schema: &SchemaDefinition) -> Vec<PrefixMapPair> {
    let builtins = [
        ("sh", "http://www.w3.org/ns/shacl#"),
        ("xsd", "http://www.w3.org/2001/XMLSchema#"),
    ];
    schema
        .prefixes
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_str()))
        .chain(builtins)
        .filter_map(|(name, base)| {
            let prefix = Prefix::new(name.to_string().into_boxed_str())
                .map_err(|e| tracing::warn!(prefix = name, error = %e, "skipping invalid prefix"))
                .ok()?;
            let iri = Iri::new(base.to_string().into_boxed_str())
                .map_err(
                    |e| tracing::warn!(prefix = name, base, error = %e, "skipping bad base IRI"),
                )
                .ok()?;
            Some((prefix, iri))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::Writer;
    use crate::linkml::{ClassDefinition, SlotDefinition};
    use std::fs;
    use tempfile::TempDir;

    /// Render `schema` through `ShaclWriter`, load the output into an
    /// independent `oxigraph` store, and return it — the same real-triple-
    /// store oracle the RDF writers use, applied to the shapes graph.
    fn render_to_store(schema: &SchemaDefinition) -> oxigraph::store::Store {
        let dir = TempDir::new().expect("temp dir");
        let out = dir.path().join("shapes.ttl");
        ShaclWriter::new().write(schema, &out).expect("write shacl");
        let ttl = fs::read_to_string(&out).expect("read shapes");
        let store = oxigraph::store::Store::new().expect("store");
        store
            .load_from_slice(oxigraph::io::RdfFormat::Turtle, &ttl)
            .unwrap_or_else(|e| panic!("oxigraph rejected generated SHACL: {e}\n\n{ttl}"));
        store
    }

    fn ask(store: &oxigraph::store::Store, query: &str) -> bool {
        use oxigraph::sparql::{QueryResults, SparqlEvaluator};
        match SparqlEvaluator::new()
            .parse_query(query)
            .unwrap_or_else(|e| panic!("bad SPARQL: {e}\n\n{query}"))
            .on_store(store)
            .execute()
            .expect("query")
        {
            QueryResults::Boolean(b) => b,
            _ => panic!("expected ASK result"),
        }
    }

    const SH: &str = "http://www.w3.org/ns/shacl#";
    const EX: &str = "http://example.org/test";

    fn schema_with_constrained_class() -> SchemaDefinition {
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some(EX.to_string());

        let mut provider = ClassDefinition::new("Provider");
        provider.class_uri = Some(format!("{EX}#Provider"));
        schema.classes.insert("Provider".to_string(), provider);

        let mut deployment = ClassDefinition::new("Deployment");
        deployment.class_uri = Some(format!("{EX}#Deployment"));
        // A required, patterned string.
        let mut code = SlotDefinition::new("code");
        code.range = Some("string".to_string());
        code.required = true;
        code.pattern = Some("^[A-Z]+$".to_string());
        deployment.attributes.insert("code".to_string(), code);
        // A value-bounded float.
        let mut ratio = SlotDefinition::new("ratio");
        ratio.range = Some("float".to_string());
        ratio.minimum_value = Some(0.0);
        ratio.maximum_value = Some(1.0);
        deployment.attributes.insert("ratio".to_string(), ratio);
        // A single-valued class reference.
        let mut on_provider = SlotDefinition::new("on_provider");
        on_provider.range = Some("Provider".to_string());
        deployment
            .attributes
            .insert("on_provider".to_string(), on_provider);
        schema.classes.insert("Deployment".to_string(), deployment);
        schema
    }

    #[test]
    fn shacl_writer_format_id_is_shacl() {
        assert_eq!(ShaclWriter::new().format_id(), "shacl");
    }

    #[test]
    fn output_uses_the_sh_prefix_in_compact_form() {
        // The prefix map must actually reach the serializer — without it the
        // shapes graph emits verbose full `<...shacl#NodeShape>` IRIs instead
        // of compact `sh:NodeShape`. Proves `build_prefix_map`'s output is
        // wired in and non-empty.
        let dir = TempDir::new().unwrap();
        let out = dir.path().join("shapes.ttl");
        ShaclWriter::new()
            .write(&schema_with_constrained_class(), &out)
            .unwrap();
        let ttl = fs::read_to_string(&out).unwrap();
        assert!(
            ttl.contains("sh:NodeShape"),
            "expected compact `sh:` form (so the sh: prefix is declared); got:\n{ttl}"
        );
    }

    #[test]
    fn every_class_gets_a_node_shape_targeting_its_iri() {
        let store = render_to_store(&schema_with_constrained_class());
        for name in ["Provider", "Deployment"] {
            assert!(
                ask(
                    &store,
                    &format!(
                        "ASK {{ <{EX}#{name}Shape> a <{SH}NodeShape> ; <{SH}targetClass> <{EX}#{name}> }}"
                    )
                ),
                "expected a NodeShape targeting {name}"
            );
        }
    }

    #[test]
    fn base_slot_constraints_project_to_property_shapes() {
        let store = render_to_store(&schema_with_constrained_class());

        // required → sh:minCount 1, plus sh:pattern, on the `code` property shape.
        assert!(
            ask(
                &store,
                &format!(
                    "ASK {{ <{EX}#DeploymentShape> <{SH}property> ?p . \
                     ?p <{SH}path> <{EX}#code> ; <{SH}minCount> 1 ; <{SH}pattern> \"^[A-Z]+$\" }}"
                )
            ),
            "required+pattern must project to sh:minCount + sh:pattern"
        );
        // value bounds → sh:minInclusive / sh:maxInclusive on `ratio`.
        assert!(
            ask(
                &store,
                &format!(
                    "ASK {{ ?p <{SH}path> <{EX}#ratio> ; <{SH}minInclusive> ?lo ; <{SH}maxInclusive> ?hi }}"
                )
            ),
            "value bounds must project to sh:minInclusive/maxInclusive"
        );
        // class-valued range → sh:class the target class IRI.
        assert!(
            ask(
                &store,
                &format!(
                    "ASK {{ ?p <{SH}path> <{EX}#on_provider> ; <{SH}class> <{EX}#Provider> }}"
                )
            ),
            "a class-range slot must project to sh:class"
        );
    }

    #[test]
    fn a_scalar_slot_projects_to_sh_datatype() {
        let store = render_to_store(&schema_with_constrained_class());
        assert!(
            ask(
                &store,
                &format!(
                    "ASK {{ ?p <{SH}path> <{EX}#code> ; <{SH}datatype> <http://www.w3.org/2001/XMLSchema#string> }}"
                )
            ),
            "a plain scalar slot must carry sh:datatype"
        );
    }
}
